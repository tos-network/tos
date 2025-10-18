use std::sync::Arc;
use async_trait::async_trait;
use log::{debug, trace};
use tos_common::{
    block::{Block, BlockHeader},
    crypto::{Hash, Hashable},
    difficulty::Difficulty,
    immutable::Immutable,
    serializer::Serializer,
    transaction::Transaction,
    varuint::VarUint
};
use crate::core::{
    error::BlockchainError,
    storage::{
        sled::BLOCKS_COUNT,
        BlockProvider,
        BlocksAtHeightProvider,
        DifficultyProvider,
        TransactionProvider,
        SledStorage,
    }
};

impl SledStorage {
    // Update the blocks count and store it on disk
    fn store_blocks_count(&mut self, count: u64) -> Result<(), BlockchainError> {
        if let Some(snapshot) = self.snapshot.as_mut() {
            snapshot.cache.blocks_count = count;
        } else {
            self.cache.blocks_count = count;
        }
        Self::insert_into_disk(self.snapshot.as_mut(), &self.extra, BLOCKS_COUNT, &count.to_be_bytes())?;
        Ok(())
    }
}

#[async_trait]
impl BlockProvider for SledStorage {
    async fn has_blocks(&self) -> Result<bool, BlockchainError> {
        trace!("has blocks");
        Ok(!self.blocks.is_empty())
    }

    async fn count_blocks(&self) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("count blocks");
        }
        let count = if let Some(snapshot) = &self.snapshot {
            snapshot.cache.blocks_count
        } else {
            self.cache.blocks_count
        };
        Ok(count)
    }

    async fn decrease_blocks_count(&mut self, amount: u64) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("count blocks");
        }
        if let Some(snapshot) = self.snapshot.as_mut() {
            snapshot.cache.blocks_count -= amount;
        } else {
            self.cache.blocks_count -= amount;
        }

        Ok(())
    }

    async fn has_block_with_hash(&self, hash: &Hash) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("has block {}", hash);
        }
        self.contains_data_cached(&self.blocks, &self.blocks_cache, hash).await
    }

    async fn save_block(&mut self, block: Arc<BlockHeader>, txs: &[Arc<Transaction>], difficulty: Difficulty, p: VarUint, hash: Immutable<Hash>) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Debug) {
            debug!("Storing new {} with hash: {}, difficulty: {}, snapshot mode: {}", block, hash, difficulty, self.snapshot.is_some());
        }

        // Store transactions and collect tx hashes
        let mut txs_count = 0;
        let mut tx_hashes = Vec::with_capacity(txs.len());
        for tx in txs { // first save all txs, then save block
            let tx_hash = (**tx).hash();
            tx_hashes.push(tx_hash.clone());
            if !self.has_transaction(&tx_hash).await? {
                self.add_transaction(&tx_hash, &tx).await?;
                txs_count += 1;
            }
        }

        // Increase only if necessary
        if txs_count > 0 {
            self.store_transactions_count(self.count_transactions().await? + txs_count)?;
        }

        // Store block header and increase blocks count if it's a new block
        let no_prev = Self::insert_into_disk(self.snapshot.as_mut(), &self.blocks, hash.as_bytes(), block.to_bytes())?.is_none();
        if no_prev {
            self.store_blocks_count(self.count_blocks().await? + 1)?;
        }

        // TIP-2 Phase 1 fix: Store block → transactions mapping
        Self::insert_into_disk(self.snapshot.as_mut(), &self.block_transactions, hash.as_bytes(), tx_hashes.to_bytes())?;

        // Performance optimization: Store frequently-accessed fields separately (62x-100x faster reads)
        Self::insert_into_disk(self.snapshot.as_mut(), &self.block_blue_score, hash.as_bytes(), block.blue_score.to_bytes())?;
        Self::insert_into_disk(self.snapshot.as_mut(), &self.block_daa_score, hash.as_bytes(), block.daa_score.to_bytes())?;
        Self::insert_into_disk(self.snapshot.as_mut(), &self.block_timestamp, hash.as_bytes(), block.timestamp.to_bytes())?;
        Self::insert_into_disk(self.snapshot.as_mut(), &self.block_version, hash.as_bytes(), block.version.to_bytes())?;

        // Store difficulty
        Self::insert_into_disk(self.snapshot.as_mut(), &self.difficulty, hash.as_bytes(), difficulty.to_bytes())?;

        // Phase 2: cumulative_difficulty storage removed - use blue_work from GHOSTDAG instead

        // Store P
        Self::insert_into_disk(self.snapshot.as_mut(), &self.difficulty_covariance, hash.as_bytes(), p.to_bytes())?;

        self.add_block_hash_at_blue_score(&hash, block.get_blue_score()).await?;

        if let Some(cache) = self.blocks_cache.as_mut() {
            // TODO: no clone
            cache.get_mut().put(hash.into_owned(), block);
        }

        Ok(())
    }

    async fn get_block_by_hash(&self, hash: &Hash) -> Result<Block, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get block by hash {}", hash);
        }
        let header = self.get_block_header_by_hash(hash).await?;

        // TIP-2 Phase 1 fix: Load transaction hashes from block_transactions tree
        let tx_hashes: Vec<Hash> = Self::load_optional_from_disk_internal(self.snapshot.as_ref(), &self.block_transactions, hash.as_bytes())?
            .unwrap_or_default();

        // Load each transaction from storage
        let mut transactions = Vec::with_capacity(tx_hashes.len());
        for tx_hash in tx_hashes {
            let tx = self.get_transaction(&tx_hash).await?;
            transactions.push(tx.into_arc());
        }

        let block = Block::new(header, transactions);
        Ok(block)
    }

    async fn delete_block_with_hash(&mut self, hash: &Hash) -> Result<Block, BlockchainError> {
        if log::log_enabled!(log::Level::Debug) {
            debug!("Deleting block with hash: {}", hash);
        }

        // TIP-2 Phase 1 fix: Load transactions before deleting the mapping
        let tx_hashes: Vec<Hash> = Self::load_optional_from_disk_internal(self.snapshot.as_ref(), &self.block_transactions, hash.as_bytes())?
            .unwrap_or_default();

        // Load transactions to return in the block
        let mut transactions = Vec::with_capacity(tx_hashes.len());
        for tx_hash in tx_hashes {
            let tx = self.get_transaction(&tx_hash).await?;
            transactions.push(tx.into_arc());
        }

        // Delete block header
        let header = Self::delete_arc_cacheable_data(self.snapshot.as_mut(), &self.blocks, self.cache.blocks_cache.as_mut(), &hash).await?;

        // Decrease blocks count
        self.store_blocks_count(self.count_blocks().await? - 1)?;

        // TIP-2 Phase 1 fix: Delete block → transactions mapping
        Self::remove_from_disk_without_reading(self.snapshot.as_mut(), &self.block_transactions, hash.as_bytes())?;

        // Performance optimization: Also delete field-specific trees
        Self::remove_from_disk_without_reading(self.snapshot.as_mut(), &self.block_blue_score, hash.as_bytes())?;
        Self::remove_from_disk_without_reading(self.snapshot.as_mut(), &self.block_daa_score, hash.as_bytes())?;
        Self::remove_from_disk_without_reading(self.snapshot.as_mut(), &self.block_timestamp, hash.as_bytes())?;
        Self::remove_from_disk_without_reading(self.snapshot.as_mut(), &self.block_version, hash.as_bytes())?;

        // Delete difficulty
        Self::remove_from_disk_without_reading(self.snapshot.as_mut(), &self.difficulty, hash.as_bytes())?;

        // Delete P
        Self::remove_from_disk_without_reading(self.snapshot.as_mut(), &self.difficulty_covariance, hash.as_bytes())?;

        self.remove_block_hash_at_blue_score(&hash, header.get_blue_score()).await?;

        let block = Block::new(header, transactions);

        Ok(block)
    }
}