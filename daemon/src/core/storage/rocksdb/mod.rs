mod column;
mod providers;
mod snapshot;
mod types;

use std::sync::Arc;

use crate::core::{
    config::RocksDBConfig,
    error::{BlockchainError, DiskContext},
    storage::{cache::StorageCache, BlocksAtHeightProvider, TransactionProvider},
};
use anyhow::Context;
use async_trait::async_trait;
use itertools::Either;
use log::{debug, info, trace};
use rocksdb::{
    BlockBasedOptions, Cache, ColumnFamilyDescriptor, DBCompactionStyle, DBCompressionType,
    DBWithThreadMode, Direction, Env, IteratorMode as InternalIteratorMode, MultiThreaded, Options,
    ReadOptions, SliceTransform, WaitForCompactOptions, WriteOptions,
};
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;
use tos_common::{
    account::EnergyResource,
    ai_mining::AIMiningState,
    block::{BlockHeader, TopoHeight},
    crypto::{Hash, PublicKey},
    immutable::Immutable,
    network::Network,
    serializer::{Count, Serializer},
    tokio,
    transaction::Transaction,
};

pub use column::*;
pub use types::*;

pub use snapshot::Snapshot;

use super::Storage;

macro_rules! cf_handle {
    ($db: expr, $column: expr) => {
        $db.cf_handle($column.as_ref())
            .with_context(|| format!("Column {:?} not found", $column))?
    };
}

type InnerDB = DBWithThreadMode<MultiThreaded>;

#[derive(Debug, Copy, Clone, clap::ValueEnum, Serialize, Deserialize)]
#[clap(rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum CompressionMode {
    None,
    Snappy,
    Zlib,
    Bz2,
    Lz4,
    Lz4hc,
    Zstd,
}

#[derive(Debug, Copy, Clone, clap::ValueEnum, Serialize, Deserialize)]
#[clap(rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum CacheMode {
    None,
    Lru,
    HyperClock,
}

impl Default for CacheMode {
    fn default() -> Self {
        Self::Lru
    }
}

impl Default for CompressionMode {
    fn default() -> Self {
        Self::Snappy
    }
}

impl CompressionMode {
    pub fn convert(self) -> DBCompressionType {
        match self {
            Self::None => DBCompressionType::None,
            Self::Snappy => DBCompressionType::Snappy,
            Self::Zlib => DBCompressionType::Zlib,
            Self::Bz2 => DBCompressionType::Bz2,
            Self::Lz4 => DBCompressionType::Lz4,
            Self::Lz4hc => DBCompressionType::Lz4hc,
            Self::Zstd => DBCompressionType::Zstd,
        }
    }
}

#[derive(Copy, Clone)]
pub enum IteratorMode<'a> {
    Start,
    End,
    // Allow for range start operations
    From(&'a [u8], Direction),
    // Strict prefix to all keys
    WithPrefix(&'a [u8], Direction),
}

impl<'a> IteratorMode<'a> {
    pub fn convert(self) -> (InternalIteratorMode<'a>, ReadOptions) {
        let mut opts = ReadOptions::default();
        let mode = match self {
            Self::Start => InternalIteratorMode::Start,
            Self::End => InternalIteratorMode::End,
            Self::From(prefix, direction) => InternalIteratorMode::From(prefix, direction),
            Self::WithPrefix(prefix, direction) => {
                opts.set_prefix_same_as_start(true);
                InternalIteratorMode::From(prefix, direction)
            }
        };

        (mode, opts)
    }
}

pub struct RocksStorage {
    db: Arc<InnerDB>,
    network: Network,
    cache: StorageCache,
    snapshot: Option<Snapshot>,
}

impl std::ops::Deref for RocksStorage {
    type Target = StorageCache;

    fn deref(&self) -> &Self::Target {
        &self.cache
    }
}

impl std::ops::DerefMut for RocksStorage {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.cache
    }
}

impl RocksStorage {
    pub fn new(
        dir: &str,
        network: Network,
        cache_size: Option<usize>,
        config: &RocksDBConfig,
    ) -> Self {
        let cfs = Column::iter().map(|column| {
            let name = column.to_string();
            let prefix = column.prefix();
            let mut opts = Options::default();
            if let Some(len) = prefix {
                opts.set_prefix_extractor(SliceTransform::create_fixed_prefix(len));
            }

            ColumnFamilyDescriptor::new(name, opts)
        });

        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        opts.set_compaction_style(DBCompactionStyle::Universal);

        opts.increase_parallelism(config.parallelism as _);
        opts.set_max_background_jobs(config.max_background_jobs as _);
        opts.set_max_subcompactions(config.max_subcompaction_jobs as _);

        opts.set_max_open_files(config.max_open_files);
        opts.set_keep_log_file_num(config.keep_max_log_files);

        let mut env = Env::new().expect("Creating new env");
        env.set_low_priority_background_threads(config.low_priority_background_threads as _);
        opts.set_env(&env);
        opts.set_compression_type(config.compression_mode.convert());

        let mut block_opts = BlockBasedOptions::default();
        match config.cache_mode {
            CacheMode::None => {
                block_opts.disable_cache();
            }
            CacheMode::Lru => {
                let cache = Cache::new_lru_cache(config.cache_size as _);
                block_opts.set_block_cache(&cache);
            }
            CacheMode::HyperClock => {
                let cache = Cache::new_hyper_clock_cache(config.cache_size as _, 1024);
                block_opts.set_block_cache(&cache);
            }
        };

        opts.set_block_based_table_factory(&block_opts);
        if config.write_buffer_shared {
            opts.set_db_write_buffer_size(config.write_buffer_size as _);
        } else {
            opts.set_write_buffer_size(config.write_buffer_size as _);
        }

        let db = DBWithThreadMode::<MultiThreaded>::open_cf_descriptors(
            &opts,
            format!("{}{}", dir, network.to_string().to_lowercase()),
            cfs,
        )
        .expect("Failed to open RocksDB");

        Self {
            db: Arc::new(db),
            network,
            cache: StorageCache::new(cache_size),
            snapshot: None,
        }
    }

    pub fn load_cache_from_disk(&mut self) {
        trace!("load cache from disk");
    }

    /// Insert into disk without fsync (backward compatible)
    pub(super) fn insert_into_disk<K: AsRef<[u8]>, V: Serializer>(
        &mut self,
        column: Column,
        key: K,
        value: &V,
    ) -> Result<(), BlockchainError> {
        Self::insert_into_disk_internal(&self.db, self.snapshot.as_mut(), column, key, value, false)
    }

    /// Insert into disk with fsync for critical data (V-22 fix)
    pub(super) fn insert_into_disk_sync<K: AsRef<[u8]>, V: Serializer>(
        &mut self,
        column: Column,
        key: K,
        value: &V,
    ) -> Result<(), BlockchainError> {
        Self::insert_into_disk_internal(&self.db, self.snapshot.as_mut(), column, key, value, true)
    }

    pub(super) fn remove_from_disk<K: AsRef<[u8]>>(
        &mut self,
        column: Column,
        key: K,
    ) -> Result<(), BlockchainError> {
        Self::remove_from_disk_internal(&self.db, self.snapshot.as_mut(), column, key)
    }

    pub fn contains_data<K: AsRef<[u8]>>(
        &self,
        column: Column,
        key: &K,
    ) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("contains data {:?}", column);
        }

        let key_bytes = key.as_ref();
        if let Some(snapshot) = self.snapshot.as_ref() {
            if let Some(v) = snapshot.contains(column, &key_bytes) {
                return Ok(v);
            }
        }

        let cf = cf_handle!(self.db, column);
        let value = self.db.get_pinned_cf(&cf, key_bytes).with_context(|| {
            format!("Error while checking if key exists in column {:?}", column)
        })?;

        Ok(value.is_some())
    }

    // Check if its empty by checking the snapshot cache first, and then the raw DB
    pub fn is_empty(&self, column: Column) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("is empty {:?}", column);
        }

        let cf = cf_handle!(self.db, column);
        let mut iterator = self.db.iterator_cf(&cf, InternalIteratorMode::Start);

        if let Some(snapshot) = self.snapshot.as_ref() {
            return Ok(snapshot.is_empty(column, iterator));
        }

        Ok(iterator.next().is_none())
    }

    // Count how many entries we have stored in a column
    pub fn count_entries(&self, column: Column) -> Result<usize, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("count entries {:?}", column);
        }

        let cf = cf_handle!(self.db, column);
        let iterator = self.db.iterator_cf(&cf, InternalIteratorMode::Start);

        if let Some(snapshot) = self.snapshot.as_ref() {
            return Ok(snapshot.count_entries(column, iterator));
        }

        Ok(iterator.count())
    }

    pub fn load_optional_from_disk<K: AsRef<[u8]> + ?Sized, V: Serializer>(
        &self,
        column: Column,
        key: &K,
    ) -> Result<Option<V>, BlockchainError> {
        Self::load_optional_from_disk_internal(&self.db, self.snapshot.as_ref(), column, key)
    }

    pub fn load_from_disk<K: AsRef<[u8]> + ?Sized, V: Serializer>(
        &self,
        column: Column,
        key: &K,
    ) -> Result<V, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("load from disk internal {:?}", column);
        }

        self.load_optional_from_disk(column, key)?
            .ok_or(BlockchainError::NotFoundOnDisk(DiskContext::LoadData))
    }

    pub fn get_size_from_disk<K: AsRef<[u8]>>(
        &self,
        column: Column,
        key: &K,
    ) -> Result<usize, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("load from disk internal {:?}", column);
        }

        if let Some(v) = self
            .snapshot
            .as_ref()
            .and_then(|s| s.get_size(column, key.as_ref()))
        {
            match v {
                Some(v) => return Ok(v),
                None => return Err(BlockchainError::NotFoundOnDisk(DiskContext::DataLen)),
            }
        }

        let cf = cf_handle!(self.db, column);
        match self
            .db
            .get_pinned_cf(&cf, key.as_ref())
            .with_context(|| format!("Internal error while reading {:?}", column))?
        {
            Some(bytes) => Ok(bytes.len()),
            None => Err(BlockchainError::NotFoundOnDisk(DiskContext::DataLen)),
        }
    }

    // Internal functions for better borrow checking

    pub fn load_optional_from_disk_internal<K: AsRef<[u8]> + ?Sized, V: Serializer>(
        db: &InnerDB,
        snapshot: Option<&Snapshot>,
        column: Column,
        key: &K,
    ) -> Result<Option<V>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("load optional {:?} from disk internal", column);
        }

        if let Some(v) = snapshot.and_then(|s| s.get(column, key.as_ref())) {
            match v {
                Some(v) => return Ok(Some(V::from_bytes(&v)?)),
                None => return Ok(None),
            }
        }

        let cf = cf_handle!(db, column);
        match db
            .get_pinned_cf(&cf, key.as_ref())
            .with_context(|| format!("Internal error while reading column {:?}", column))?
        {
            Some(bytes) => Ok(Some(V::from_bytes(&bytes)?)),
            None => Ok(None),
        }
    }

    /// Insert into disk with optional fsync for durability
    ///
    /// V-22 Fix: Added sync parameter to force fsync for critical writes
    /// When sync=true, forces fsync to guarantee data durability
    pub(super) fn insert_into_disk_internal<K: AsRef<[u8]>, V: Serializer>(
        db: &InnerDB,
        snapshot: Option<&mut Snapshot>,
        column: Column,
        key: K,
        value: &V,
        sync: bool,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("insert into disk {:?} (sync={})", column, sync);
        }

        match snapshot {
            Some(snapshot) => snapshot.put(column, key.as_ref().to_vec(), value.to_bytes()),
            None => {
                let cf = cf_handle!(db, column);
                if sync {
                    // V-22: Critical data - force fsync for durability
                    let mut write_opts = WriteOptions::default();
                    write_opts.set_sync(true);
                    db.put_cf_opt(&cf, key.as_ref(), value.to_bytes(), &write_opts)
                        .with_context(|| {
                            format!(
                                "Error while inserting into disk column {:?} with sync",
                                column
                            )
                        })?
                } else {
                    // Non-critical data - async write
                    db.put_cf(&cf, key.as_ref(), value.to_bytes())
                        .with_context(|| {
                            format!("Error while inserting into disk column {:?}", column)
                        })?
                }
            }
        };

        Ok(())
    }

    pub(super) fn remove_from_disk_internal<K: AsRef<[u8]>>(
        db: &InnerDB,
        snapshot: Option<&mut Snapshot>,
        column: Column,
        key: K,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("remove from disk {:?}", column);
        }

        let bytes = key.as_ref();
        match snapshot {
            Some(snapshot) => snapshot.delete(column, bytes.to_vec()),
            None => {
                let cf = cf_handle!(db, column);
                db.delete_cf(&cf, bytes).with_context(|| {
                    format!("Error while removing from disk column {:?}", column)
                })?;
            }
        };

        Ok(())
    }

    pub fn iter_owned_internal<'a, K, V>(
        db: &'a InnerDB,
        snapshot: Option<&Snapshot>,
        mode: IteratorMode,
        column: Column,
    ) -> Result<impl Iterator<Item = Result<(K, V), BlockchainError>> + 'a, BlockchainError>
    where
        K: Serializer + 'a,
        V: Serializer + 'a,
    {
        if log::log_enabled!(log::Level::Trace) {
            trace!("iter owned {:?}", column);
        }

        let cf = cf_handle!(db, column);
        let (m, opts) = mode.convert();
        let iterator = db.iterator_cf_opt(&cf, opts, m);

        match snapshot {
            Some(snapshot) => Ok(Either::Left(snapshot.iter_owned(column, mode, iterator))),
            None => Ok(Either::Right(iterator.map(|res| {
                let (key, value) = res.context("Internal read error in iter")?;
                let key = K::from_bytes(&key)?;
                let value = V::from_bytes(&value)?;

                Ok((key, value))
            }))),
        }
    }

    pub fn iter_internal<'a, K, V>(
        db: &'a InnerDB,
        snapshot: Option<&'a Snapshot>,
        mode: IteratorMode,
        column: Column,
    ) -> Result<impl Iterator<Item = Result<(K, V), BlockchainError>> + 'a, BlockchainError>
    where
        K: Serializer + 'a,
        V: Serializer + 'a,
    {
        if log::log_enabled!(log::Level::Trace) {
            trace!("iter {:?}", column);
        }

        let cf = cf_handle!(db, column);
        let (m, opts) = mode.convert();
        let iterator = db.iterator_cf_opt(&cf, opts, m);

        match snapshot {
            Some(snapshot) => Ok(Either::Left(snapshot.lazy_iter(column, mode, iterator))),
            None => Ok(Either::Right(iterator.map(|res| {
                let (key, value) = res.context("Internal read error in iter")?;
                let key = K::from_bytes(&key)?;
                let value = V::from_bytes(&value)?;

                Ok((key, value))
            }))),
        }
    }

    pub fn iter_keys_internal<'a, K>(
        db: &'a InnerDB,
        snapshot: Option<&'a Snapshot>,
        mode: IteratorMode,
        column: Column,
    ) -> Result<impl Iterator<Item = Result<K, BlockchainError>> + 'a, BlockchainError>
    where
        K: Serializer + 'a,
    {
        if log::log_enabled!(log::Level::Trace) {
            trace!("iter keys {:?}", column);
        }

        let cf = cf_handle!(db, column);
        let (m, opts) = mode.convert();
        let iterator = db.iterator_cf_opt(&cf, opts, m);

        match snapshot {
            Some(snapshot) => Ok(Either::Left(
                snapshot.lazy_iter_keys(column, mode, iterator),
            )),
            None => Ok(Either::Right(iterator.map(|res| {
                let (key, _) = res.context("Internal read error in iter_keys")?;
                let key = K::from_bytes(&key)?;

                Ok(key)
            }))),
        }
    }

    #[inline(always)]
    pub fn iter<'a, K, V>(
        &'a self,
        column: Column,
        mode: IteratorMode,
    ) -> Result<impl Iterator<Item = Result<(K, V), BlockchainError>> + 'a, BlockchainError>
    where
        K: Serializer + 'a,
        V: Serializer + 'a,
    {
        Self::iter_internal(&self.db, self.snapshot.as_ref(), mode, column)
    }

    #[inline(always)]
    pub fn iter_keys<'a, K>(
        &'a self,
        column: Column,
        mode: IteratorMode,
    ) -> Result<impl Iterator<Item = Result<K, BlockchainError>> + 'a, BlockchainError>
    where
        K: Serializer + 'a,
    {
        Self::iter_keys_internal(&self.db, self.snapshot.as_ref(), mode, column)
    }
}

#[async_trait]
impl Storage for RocksStorage {
    // delete block at topoheight, and all its data related
    async fn delete_block_at_topoheight(
        &mut self,
        topoheight: TopoHeight,
    ) -> Result<
        (
            Hash,
            Immutable<BlockHeader>,
            Vec<(Hash, Immutable<Transaction>)>,
        ),
        BlockchainError,
    > {
        if log::log_enabled!(log::Level::Trace) {
            trace!("Delete block at topoheight {topoheight}");
        }

        // delete topoheight<->hash pointers
        let hash: Hash = self.load_from_disk(Column::HashAtTopo, &topoheight.to_be_bytes())?;
        self.remove_from_disk(Column::HashAtTopo, &topoheight.to_be_bytes())?;

        if log::log_enabled!(log::Level::Trace) {
            trace!("deleting block execution order");
        }
        self.remove_from_disk(Column::BlocksExecutionOrder, hash.as_bytes())?;

        if log::log_enabled!(log::Level::Trace) {
            trace!("hash is {hash} at topo {topoheight}");
        }
        self.remove_from_disk(Column::TopoByHash, &hash)?;

        if log::log_enabled!(log::Level::Trace) {
            trace!("deleting block header {}", hash);
        }
        let block: Immutable<BlockHeader> = self.load_from_disk(Column::Blocks, &hash)?;
        self.remove_from_disk(Column::Blocks, &hash)?;
        if log::log_enabled!(log::Level::Trace) {
            trace!("block header deleted successfully");
        }

        if log::log_enabled!(log::Level::Trace) {
            trace!("deleting topoheight metadata");
        }
        self.remove_from_disk(Column::TopoHeightMetadata, &topoheight.to_be_bytes())?;
        if log::log_enabled!(log::Level::Trace) {
            trace!("topoheight metadata deleted");
        }

        if log::log_enabled!(log::Level::Trace) {
            trace!("deleting block difficulty");
        }
        self.remove_from_disk(Column::BlockDifficulty, &hash)?;
        if log::log_enabled!(log::Level::Trace) {
            trace!("block deleted");
        }

        // TIP-2 Phase 1 fix: Load and delete transactions
        if log::log_enabled!(log::Level::Trace) {
            trace!("loading transactions for block {}", hash);
        }
        let tx_hashes: Vec<Hash> = self
            .load_optional_from_disk(Column::BlockTransactions, hash.as_bytes())?
            .unwrap_or_default();

        // Load each transaction to return
        let mut txs = Vec::with_capacity(tx_hashes.len());
        for tx_hash in tx_hashes {
            let tx = self.get_transaction(&tx_hash).await?;
            txs.push((tx_hash, tx));
        }

        // Delete the block → transactions mapping
        if log::log_enabled!(log::Level::Trace) {
            trace!("deleting BlockTransactions mapping");
        }
        self.remove_from_disk(Column::BlockTransactions, &hash)?;

        // remove the block hash from the set, and delete the set if empty
        if self
            .has_blocks_at_blue_score(block.get_blue_score())
            .await?
        {
            self.remove_block_hash_at_blue_score(&hash, block.get_blue_score())
                .await?;
        }

        Ok((hash, block, txs))
    }

    // Get the size of the chain on disk in bytes
    async fn get_size_on_disk(&self) -> Result<u64, BlockchainError> {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || {
            let mut size = 0;
            for column in Column::iter() {
                let cf = cf_handle!(db, column);
                let metadata = db.get_column_family_metadata_cf(&cf);
                size += metadata.size;
            }

            Ok::<_, BlockchainError>(size)
        })
        .await
        .context("Getting size on disk")?
    }

    // Estimate the size of the DB in bytes
    async fn estimate_size(&self) -> Result<u64, BlockchainError> {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || {
            let mut size = 0;
            for column in Column::iter() {
                for res in
                    Self::iter_internal::<Count, Count>(&db, None, IteratorMode::Start, column)?
                {
                    let (key, value) = res?;
                    size += (key.0 + value.0) as u64;
                }
            }

            Ok::<_, BlockchainError>(size)
        })
        .await
        .context("Estimating size")?
    }

    // Stop the storage and wait for it to finish
    async fn stop(&mut self) -> Result<(), BlockchainError> {
        self.flush().await
    }

    // Flush the inner DB after a block being written
    async fn flush(&mut self) -> Result<(), BlockchainError> {
        trace!("flush DB");

        let db = Arc::clone(&self.db);
        // To prevent starving the current async worker,
        // We execute the following on a blocking thread
        // and simply await its result
        tokio::task::spawn_blocking(move || {
            for column in Column::iter() {
                if log::log_enabled!(log::Level::Info) {
                    info!("compacting {:?}", column);
                }
                let cf = cf_handle!(db, column);
                db.compact_range_cf::<&[u8], &[u8]>(&cf, None, None);
            }

            debug!("wait for compact");
            let options = WaitForCompactOptions::default();
            db.wait_for_compact(&options)
                .context("Error while waiting on compact")?;

            info!("flushing DB");
            db.flush().context("Error while flushing DB")?;

            Ok::<_, BlockchainError>(())
        })
        .await
        .context("Flushing DB")?
    }
}

// EnergyProvider implementation for RocksStorage
#[async_trait]
impl crate::core::storage::EnergyProvider for RocksStorage {
    async fn get_energy_resource(
        &self,
        account: &PublicKey,
    ) -> Result<Option<EnergyResource>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "get energy resource for account {}",
                account.as_address(self.network.is_mainnet())
            );
        }

        // Get the latest topoheight for this account's energy resource
        let topoheight = self.load_optional_from_disk::<Vec<u8>, u64>(
            Column::EnergyResources,
            &account.to_bytes(),
        )?;

        match topoheight {
            Some(topoheight) => {
                // Get the versioned energy resource at that topoheight
                let key = format!(
                    "{}_{}",
                    topoheight,
                    account.as_address(self.network.is_mainnet())
                );
                let energy = self.load_optional_from_disk::<Vec<u8>, EnergyResource>(
                    Column::VersionedEnergyResources,
                    &key.as_bytes().to_vec(),
                )?;
                if log::log_enabled!(log::Level::Trace) {
                    trace!(
                        "Found energy resource at topoheight {}: {:?}",
                        topoheight,
                        energy
                    );
                }
                Ok(energy)
            }
            None => {
                if log::log_enabled!(log::Level::Trace) {
                    trace!(
                        "No energy resource found for account {}",
                        account.as_address(self.network.is_mainnet())
                    );
                }
                Ok(None)
            }
        }
    }

    async fn set_energy_resource(
        &mut self,
        account: &PublicKey,
        topoheight: TopoHeight,
        energy: &EnergyResource,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set energy resource for account {} at topoheight {}: {:?}",
                account.as_address(self.network.is_mainnet()),
                topoheight,
                energy
            );
        }

        // Store the versioned energy resource
        let key = format!(
            "{}_{}",
            topoheight,
            account.as_address(self.network.is_mainnet())
        );
        self.insert_into_disk(Column::VersionedEnergyResources, key.as_bytes(), energy)?;

        // Update the latest topoheight pointer
        self.insert_into_disk(Column::EnergyResources, &account.to_bytes(), &topoheight)?;

        Ok(())
    }
}

#[async_trait]
impl crate::core::storage::AIMiningProvider for RocksStorage {
    async fn get_ai_mining_state(&self) -> Result<Option<AIMiningState>, BlockchainError> {
        trace!("get ai mining state");

        // Get the latest topoheight that has AI mining state
        let key = "AI_MINING_STATE_TOPOHEIGHT".as_bytes().to_vec();
        let topoheight =
            self.load_optional_from_disk::<Vec<u8>, u64>(Column::AIMiningState, &key)?;

        match topoheight {
            Some(topoheight) => {
                // Get the AI mining state at that topoheight
                let state = self.load_optional_from_disk::<Vec<u8>, AIMiningState>(
                    Column::VersionedAIMiningStates,
                    &topoheight.to_be_bytes().to_vec(),
                )?;
                if log::log_enabled!(log::Level::Trace) {
                    trace!(
                        "Found AI mining state at topoheight {}: {:?}",
                        topoheight,
                        state.is_some()
                    );
                }
                Ok(state)
            }
            None => {
                if log::log_enabled!(log::Level::Trace) {
                    trace!("No AI mining state found");
                }
                Ok(None)
            }
        }
    }

    async fn set_ai_mining_state(
        &mut self,
        topoheight: TopoHeight,
        state: &AIMiningState,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set ai mining state at topoheight {}", topoheight);
        }

        // Store the versioned state
        self.insert_into_disk(
            Column::VersionedAIMiningStates,
            &topoheight.to_be_bytes().to_vec(),
            state,
        )?;

        // Update the latest topoheight pointer
        let key = "AI_MINING_STATE_TOPOHEIGHT".as_bytes().to_vec();
        self.insert_into_disk(Column::AIMiningState, &key, &topoheight)?;

        Ok(())
    }

    async fn has_ai_mining_state_at_topoheight(
        &self,
        topoheight: TopoHeight,
    ) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "check if AI mining state exists at topoheight {}",
                topoheight
            );
        }
        let cf = cf_handle!(self.db, Column::VersionedAIMiningStates);
        let exists = self
            .db
            .get_cf(&cf, &topoheight.to_be_bytes())
            .with_context(|| "Failed to check AI mining state existence")?
            .is_some();
        Ok(exists)
    }

    async fn get_ai_mining_state_at_topoheight(
        &self,
        topoheight: TopoHeight,
    ) -> Result<Option<AIMiningState>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get AI mining state at topoheight {}", topoheight);
        }
        let state = self.load_optional_from_disk::<Vec<u8>, AIMiningState>(
            Column::VersionedAIMiningStates,
            &topoheight.to_be_bytes().to_vec(),
        )?;
        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use itertools::Itertools;
    use rocksdb::{Direction, IteratorMode, Options, SliceTransform, DB};
    use tempdir::TempDir;

    #[test]
    fn test_rocks_db_iterator_behavior() {
        // Create a temporary RocksDB instance
        let tmp_dir = TempDir::new("rocksdb-iterator").unwrap();

        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        opts.set_prefix_extractor(SliceTransform::create_fixed_prefix(8));

        let db = DB::open(&opts, tmp_dir.path()).unwrap();

        // Helper to encode a u64 prefix + suffix
        fn make_key(prefix: u64, suffix: &[u8]) -> Vec<u8> {
            let mut key = prefix.to_be_bytes().to_vec();
            key.extend_from_slice(suffix);
            key
        }

        // Insert three test entries
        db.put(make_key(0, b"zero"), b"value0").unwrap();
        db.put(make_key(1, b"aaaa"), b"value1").unwrap();
        db.put(make_key(2, b"bbbb"), b"value2").unwrap();

        // First test: iterator on range
        {
            let prefix = 1u64.to_be_bytes();
            let iter = db.iterator(IteratorMode::From(&prefix, Direction::Forward));

            // Collect matching keys for inspection
            let results: Vec<(Vec<u8>, Vec<u8>)> = iter
                .filter_map_ok(|(k, v)| Some((k.to_vec(), v.to_vec())))
                .collect::<Result<Vec<_>, _>>()
                .unwrap();

            // Extract prefixes for checking
            let prefixes: Vec<u64> = results
                .iter()
                .map(|(k, _)| {
                    let mut buf = [0u8; 8];
                    buf.copy_from_slice(&k[..8]);
                    u64::from_be_bytes(buf)
                })
                .collect();

            // We expect keys with prefix 1 and 2
            assert_eq!(prefixes, vec![1, 2]);
            assert_eq!(results[0].1, b"value1");
            assert_eq!(results[1].1, b"value2");
        }

        // Second test: reverse iterator on range
        {
            let prefix = 2u64.to_be_bytes();
            let iter = db.iterator(IteratorMode::From(&prefix, Direction::Reverse));

            // Collect matching keys for inspection
            let results: Vec<(Vec<u8>, Vec<u8>)> = iter
                .filter_map_ok(|(k, v)| Some((k.to_vec(), v.to_vec())))
                .collect::<Result<Vec<_>, _>>()
                .unwrap();

            // Extract prefixes for checking
            let prefixes: Vec<u64> = results
                .iter()
                .map(|(k, _)| {
                    let mut buf = [0u8; 8];
                    buf.copy_from_slice(&k[..8]);
                    u64::from_be_bytes(buf)
                })
                .collect();

            // We expect keys with prefix 0 only as its below
            assert_eq!(prefixes, vec![1, 0]);
            assert_eq!(results[0].1, b"value1");
            assert_eq!(results[1].1, b"value0");
        }

        // Third test: iterator on prefix only
        // First test: iterator on range
        {
            let prefix = 1u64.to_be_bytes();
            let iter = db.prefix_iterator(prefix);

            // Collect matching keys for inspection
            let results: Vec<(Vec<u8>, Vec<u8>)> = iter
                .filter_map_ok(|(k, v)| Some((k.to_vec(), v.to_vec())))
                .collect::<Result<Vec<_>, _>>()
                .unwrap();

            // Extract prefixes for checking
            let prefixes: Vec<u64> = results
                .iter()
                .map(|(k, _)| {
                    let mut buf = [0u8; 8];
                    buf.copy_from_slice(&k[..8]);
                    u64::from_be_bytes(buf)
                })
                .collect();

            // We expect keys with prefix 1 only
            assert_eq!(prefixes, vec![1]);
            assert_eq!(results[0].1, b"value1");
        }
    }
}
