use crate::core::{
    blockchain::Blockchain,
    error::BlockchainError,
    ghostdag::{
        BlueWorkType, CompactGhostdagData, GhostdagStorageProvider, KType, TosGhostdagData,
    },
    hard_fork::{get_pow_algorithm_for_version, get_version_at_height},
    reachability::ReachabilityData,
    storage::{
        BlocksAtHeightProvider, DagOrderProvider, DifficultyProvider, GhostdagDataProvider,
        MerkleHashProvider, PrunedTopoheightProvider, ReachabilityDataProvider, Storage,
    },
};
use async_trait::async_trait;
use indexmap::{IndexMap, IndexSet};
use log::{debug, trace};
use std::{collections::HashMap, sync::Arc};
use tos_common::{
    block::{BlockHeader, BlockVersion, TopoHeight},
    config::TIPS_LIMIT,
    crypto::Hash,
    difficulty::Difficulty,
    immutable::Immutable,
    time::TimestampMillis,
    varuint::VarUint,
};

// This struct is used to store the block data in the chain validator
struct BlockData {
    header: Arc<BlockHeader>,
    topoheight: TopoHeight,
    difficulty: Difficulty,
    blue_work: BlueWorkType, // GHOSTDAG: Used for consensus chain selection
    blue_score: u64,         // GHOSTDAG: blue_score for this block
    ghostdag_data: Arc<TosGhostdagData>, // GHOSTDAG: Full ghostdag data for correct validation
    p: VarUint,
}

// Chain validator is used to validate the blocks received from the network
// We store the blocks in topological order and we verify the proof of work validity
// This is doing only minimal checks and valid chain order based on topoheight and difficulty
pub struct ChainValidator<'a, S: Storage> {
    // store all blocks data in topological order
    blocks: HashMap<Arc<Hash>, BlockData>,
    // store all blocks hashes at a specific height
    blocks_at_height: IndexMap<u64, IndexSet<Arc<Hash>>>,
    // Blockchain reference used to verify current chain state
    blockchain: &'a Blockchain<S>,
    hash_at_topo: IndexMap<TopoHeight, Arc<Hash>>,
}

// This struct is passed as the Provider param.
// It helps us to keep the lock of the storage and prevent any
// deadlock that could happen if a block is propagated at same time
struct ChainValidatorProvider<'a, S: Storage> {
    parent: &'a ChainValidator<'a, S>,
    storage: &'a S,
}

impl<'a, S: Storage> ChainValidatorProvider<'a, S> {
    // Check in chain validator or in storage if block exists
    pub async fn has_block_with_hash(&self, hash: &Hash) -> Result<bool, BlockchainError> {
        Ok(self.parent.blocks.contains_key(hash) || self.storage.has_block_with_hash(hash).await?)
    }
}

impl<'a, S: Storage> ChainValidator<'a, S> {
    // Starting topoheight must be 1 topoheight above the common point
    pub fn new(blockchain: &'a Blockchain<S>) -> Self {
        Self {
            blocks: HashMap::new(),
            blocks_at_height: IndexMap::new(),
            blockchain,
            hash_at_topo: IndexMap::new(),
        }
    }

    // GHOSTDAG: Check if the chain validator has a higher blue_work than our blockchain
    // blue_work is the cumulative work of all blue blocks in GHOSTDAG consensus
    // This is the correct metric for DAG chain selection
    pub async fn has_higher_blue_work(&self) -> Result<bool, BlockchainError> {
        let new_blue_work = self
            .get_expected_chain_blue_work()
            .ok_or(BlockchainError::NotEnoughBlocks)?;

        // Retrieve the current blue_work from GHOSTDAG data
        let current_blue_work = {
            debug!("locking storage for blue work comparison");
            let storage = self.blockchain.get_storage().read().await;
            debug!("storage lock acquired for blue work comparison");
            let top_block_hash = self
                .blockchain
                .get_top_block_hash_for_storage(&storage)
                .await?;
            storage.get_ghostdag_blue_work(&top_block_hash).await?
        };

        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "Chain comparison: peer blue_work = {}, our blue_work = {}",
                new_blue_work, current_blue_work
            );
        }
        Ok(new_blue_work > current_blue_work)
    }

    // GHOSTDAG: Retrieve the blue_work of the chain validator
    // This is the blue_work of the last block added, which represents the total work of the chain
    pub fn get_expected_chain_blue_work(&self) -> Option<BlueWorkType> {
        debug!("retrieving expected chain blue work");
        let (_, hash) = self.hash_at_topo.last()?;

        if log::log_enabled!(log::Level::Debug) {
            debug!("looking for blue work of {}", hash);
        }
        self.blocks.get(hash).map(|data| data.blue_work)
    }

    // validate the basic chain structure
    // We expect that the block added is the next block ordered by topoheight
    pub async fn insert_block(
        &mut self,
        hash: Hash,
        header: BlockHeader,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "Inserting block {} into chain validator with expected topoheight {}",
                hash, topoheight
            );
        }

        if self.blocks.contains_key(&hash) {
            if log::log_enabled!(log::Level::Debug) {
                debug!("Block {} is already in validator chain!", hash);
            }
            return Err(BlockchainError::AlreadyInChain);
        }

        let storage = self.blockchain.get_storage().read().await;
        debug!("storage locked for chain validator insert block");

        if storage.has_block_with_hash(&hash).await? {
            if log::log_enabled!(log::Level::Debug) {
                debug!("Block {} is already in blockchain!", hash);
            }
            return Err(BlockchainError::AlreadyInChain);
        }

        let provider = ChainValidatorProvider {
            parent: &self,
            storage: &storage,
        };

        // Verify the block version
        let version = get_version_at_height(self.blockchain.get_network(), header.get_blue_score());
        if version != header.get_version() {
            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "Block {} has version {} while expected version is {}",
                    hash,
                    header.get_version(),
                    version
                );
            }
            return Err(BlockchainError::InvalidBlockVersion);
        }

        let tips = header.get_parents();
        let tips_count = tips.len();

        // verify tips count
        if tips_count == 0 || tips_count > TIPS_LIMIT {
            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "Block {} contains {} tips while only {} is accepted",
                    hash, tips_count, TIPS_LIMIT
                );
            }
            return Err(BlockchainError::InvalidTipsCount(hash, tips_count));
        }

        // verify that we have already all its tips
        {
            for tip in tips.iter() {
                if log::log_enabled!(log::Level::Trace) {
                    trace!("Checking tip {} for block {}", tip, hash);
                }
                if !self.blocks.contains_key(tip) && !provider.has_block_with_hash(tip).await? {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!(
                            "Block {} contains tip {} which is not present in chain validator",
                            hash, tip
                        );
                    }
                    return Err(BlockchainError::InvalidTipsNotFound(hash, tip.clone()));
                }
            }
        }

        let algorithm = get_pow_algorithm_for_version(version);
        let pow_hash = header.get_pow_hash(algorithm)?;
        if log::log_enabled!(log::Level::Trace) {
            trace!("POW hash: {}", pow_hash);
        }
        let (difficulty, p) = self
            .blockchain
            .verify_proof_of_work(&provider, &pow_hash, tips.iter())
            .await?;

        // Find the common base between the block and the current blockchain
        let tips_vec = header.get_parents();
        let (base, base_height) = self
            .blockchain
            .find_common_base(&provider, tips_vec)
            .await?;

        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "Common base: {} at height {} and hash {}",
                base,
                base_height,
                hash
            );
        }

        // GHOSTDAG: Compute full GHOSTDAG data for correct blue_score and blue_work validation
        //
        // CONSENSUS FIX: Use full GHOSTDAG algorithm instead of simplified formulas
        // The correct formulas from daemon/src/core/ghostdag/mod.rs:301-328:
        //   blue_score = parent.blue_score + mergeset_blues.len()
        //   blue_work = parent.blue_work + Σ(work(mergeset_blues))
        //
        // This ensures chain_validator uses the SAME calculation as consensus layer,
        // preventing incorrect chain selection during sync (could reject valid heavier chains
        // or accept lighter chains as heavier).
        //
        // Performance note: Full GHOSTDAG is more expensive than simplified formula,
        // but necessary for consensus correctness. The overhead is acceptable because:
        //   1. Chain sync validates blocks once, not on every query
        //   2. GHOSTDAG computation is O(k²) where k is typically small (~18)
        //   3. Parent GHOSTDAG data is cached in ChainValidatorProvider
        //
        // CONSENSUS FIX: Use &provider instead of &*storage
        // ChainValidatorProvider now implements GhostdagStorageProvider trait, allowing
        // GHOSTDAG to find parent blocks in the chain validator cache (not just storage).
        // This is critical for validating chains of blocks during sync where parent
        // blocks may not yet be in storage.
        let ghostdag_data = self
            .blockchain
            .get_ghostdag()
            .ghostdag(&provider, header.get_parents())
            .await?;

        // Verify blue_score matches header claim
        let expected_blue_score = ghostdag_data.blue_score;
        if expected_blue_score != header.get_blue_score() {
            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "Block {} has blue_score {} while expected blue_score is {} (GHOSTDAG)",
                    hash,
                    header.get_blue_score(),
                    expected_blue_score
                );
            }
            return Err(BlockchainError::InvalidBlockHeight(
                expected_blue_score,
                header.get_blue_score(),
            ));
        }

        // Use GHOSTDAG-computed blue_work for chain comparison
        let blue_work = ghostdag_data.blue_work;

        if log::log_enabled!(log::Level::Debug) {
            debug!("Block {} - blue_work: {}", hash, blue_work);
        }

        let hash = Arc::new(hash);
        // Store the block in both maps
        // One is for blocks at height and the other is for the block data
        self.blocks_at_height
            .entry(header.get_blue_score())
            .or_insert_with(IndexSet::new)
            .insert(hash.clone());

        self.blocks.insert(
            hash.clone(),
            BlockData {
                header: Arc::new(header),
                topoheight,
                difficulty,
                blue_work, // GHOSTDAG: Used for consensus chain selection
                blue_score: expected_blue_score, // GHOSTDAG: blue_score from GHOSTDAG computation
                ghostdag_data: Arc::new(ghostdag_data), // GHOSTDAG: Full data for correct validation
                p,
            },
        );

        self.hash_at_topo.insert(topoheight, hash);

        Ok(())
    }

    pub fn get_block(&mut self, hash: &Hash) -> Option<Arc<BlockHeader>> {
        if log::log_enabled!(log::Level::Debug) {
            debug!("retrieving block header for {}", hash);
        }
        self.blocks.get(hash).map(|v| v.header.clone())
    }
}

#[async_trait]
impl<S: Storage> DifficultyProvider for ChainValidatorProvider<'_, S> {
    async fn get_blue_score_for_block_hash(&self, hash: &Hash) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get height for block hash {}", hash);
        }
        if let Some(data) = self.parent.blocks.get(hash) {
            return Ok(data.header.get_blue_score());
        }

        trace!("fallback on storage for get_blue_score_for_block_hash");
        self.storage.get_blue_score_for_block_hash(hash).await
    }

    // Get the block version using its hash
    async fn get_version_for_block_hash(
        &self,
        hash: &Hash,
    ) -> Result<BlockVersion, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get version for block hash {}", hash);
        }
        if let Some(data) = self.parent.blocks.get(hash) {
            return Ok(data.header.get_version());
        }

        trace!("fallback on storage for get_version_for_block_hash");
        self.storage.get_version_for_block_hash(hash).await
    }

    async fn get_timestamp_for_block_hash(
        &self,
        hash: &Hash,
    ) -> Result<TimestampMillis, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get timestamp for block hash {}", hash);
        }
        if let Some(data) = self.parent.blocks.get(hash) {
            return Ok(data.header.get_timestamp());
        }

        trace!("fallback on storage for get_timestamp_for_block_hash");
        self.storage.get_timestamp_for_block_hash(hash).await
    }

    async fn get_difficulty_for_block_hash(
        &self,
        hash: &Hash,
    ) -> Result<Difficulty, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get difficulty for block hash {}", hash);
        }
        if let Some(data) = self.parent.blocks.get(hash) {
            return Ok(data.difficulty);
        }

        trace!("fallback on storage for get_difficulty_for_block_hash");
        self.storage.get_difficulty_for_block_hash(hash).await
    }

    async fn get_past_blocks_for_block_hash(
        &self,
        hash: &Hash,
    ) -> Result<Immutable<IndexSet<Hash>>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get past blocks for block hash {}", hash);
        }
        if let Some(data) = self.parent.blocks.get(hash) {
            let tips: IndexSet<Hash> = data.header.get_parents().iter().cloned().collect();
            return Ok(Immutable::Owned(tips));
        }

        trace!("fallback on storage for get_past_blocks_for_block_hash");
        self.storage.get_past_blocks_for_block_hash(hash).await
    }

    async fn get_block_header_by_hash(
        &self,
        hash: &Hash,
    ) -> Result<Immutable<BlockHeader>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get block header by hash {}", hash);
        }
        if let Some(data) = self.parent.blocks.get(hash) {
            return Ok(Immutable::Arc(data.header.clone()));
        }

        trace!("fallback on storage for get_block_header_by_hash");
        self.storage.get_block_header_by_hash(hash).await
    }

    async fn get_estimated_covariance_for_block_hash(
        &self,
        hash: &Hash,
    ) -> Result<VarUint, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get estimated covariance for block hash {}", hash);
        }
        if let Some(data) = self.parent.blocks.get(hash) {
            return Ok(data.p.clone());
        }

        trace!("fallback on storage for get_estimated_covariance_for_block_hash");
        self.storage
            .get_estimated_covariance_for_block_hash(hash)
            .await
    }
}

#[async_trait]
impl<S: Storage> DagOrderProvider for ChainValidatorProvider<'_, S> {
    async fn get_topo_height_for_hash(&self, hash: &Hash) -> Result<TopoHeight, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get topo height for hash {}", hash);
        }
        if let Some(data) = self.parent.blocks.get(hash) {
            return Ok(data.topoheight);
        }

        trace!("fallback on storage for get_topo_height_for_hash");
        self.storage.get_topo_height_for_hash(hash).await
    }

    // This should never happen in our case
    async fn set_topo_height_for_block(
        &mut self,
        _: &Hash,
        _: TopoHeight,
    ) -> Result<(), BlockchainError> {
        Err(BlockchainError::UnsupportedOperation)
    }

    async fn is_block_topological_ordered(&self, hash: &Hash) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("is block topological ordered {}", hash);
        }
        if self.parent.blocks.contains_key(hash) {
            return Ok(true);
        }

        trace!("fallback on storage for is_block_topological_ordered");
        self.storage.is_block_topological_ordered(hash).await
    }

    async fn get_hash_at_topo_height(
        &self,
        topoheight: TopoHeight,
    ) -> Result<Hash, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get hash at topoheight {}", topoheight);
        }
        if let Some(hash) = self.parent.hash_at_topo.get(&topoheight) {
            return Ok(hash.as_ref().clone());
        }

        trace!("fallback on storage for get_hash_at_topo_height");
        self.storage.get_hash_at_topo_height(topoheight).await
    }

    async fn has_hash_at_topoheight(
        &self,
        topoheight: TopoHeight,
    ) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("has hash at topoheight {}", topoheight);
        }
        if self.parent.hash_at_topo.contains_key(&topoheight) {
            return Ok(true);
        }

        trace!("fallback on storage for has_hash_at_topoheight");
        self.storage.has_hash_at_topoheight(topoheight).await
    }

    async fn get_orphaned_blocks<'a>(
        &'a self,
    ) -> Result<impl Iterator<Item = Result<Hash, BlockchainError>> + 'a, BlockchainError> {
        trace!("get orphaned blocks");
        let iter = self.storage.get_orphaned_blocks().await?;
        Ok(iter)
    }
}

#[async_trait]
impl<S: Storage> BlocksAtHeightProvider for ChainValidatorProvider<'_, S> {
    async fn has_blocks_at_blue_score(&self, blue_score: u64) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("has block at blue_score {}", blue_score);
        }
        if self.parent.blocks_at_height.contains_key(&blue_score) {
            return Ok(true);
        }

        trace!("fallback on storage for has_blocks_at_blue_score");
        self.storage.has_blocks_at_blue_score(blue_score).await
    }

    // Retrieve the blocks hashes at a specific blue_score (DAG depth position)
    async fn get_blocks_at_blue_score(
        &self,
        blue_score: u64,
    ) -> Result<IndexSet<Hash>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("get blocks at blue_score {}", blue_score);
        }
        if let Some(tips) = self.parent.blocks_at_height.get(&blue_score) {
            // TODO
            return Ok(tips.iter().map(|v| v.as_ref().clone()).collect());
        }

        trace!("fallback on storage for get_blocks_at_blue_score");
        self.storage.get_blocks_at_blue_score(blue_score).await
    }

    // Store the blocks hashes at a specific blue_score (DAG depth position)
    async fn set_blocks_at_blue_score(
        &mut self,
        _: &IndexSet<Hash>,
        _: u64,
    ) -> Result<(), BlockchainError> {
        Err(BlockchainError::UnsupportedOperation)
    }

    // Append a block hash at a specific blue_score (DAG depth position)
    async fn add_block_hash_at_blue_score(
        &mut self,
        _: &Hash,
        _: u64,
    ) -> Result<(), BlockchainError> {
        Err(BlockchainError::UnsupportedOperation)
    }

    // Remove a block hash at a specific blue_score (DAG depth position)
    async fn remove_block_hash_at_blue_score(
        &mut self,
        _: &Hash,
        _: u64,
    ) -> Result<(), BlockchainError> {
        Err(BlockchainError::UnsupportedOperation)
    }
}

#[async_trait]
impl<S: Storage> PrunedTopoheightProvider for ChainValidatorProvider<'_, S> {
    async fn get_pruned_topoheight(&self) -> Result<Option<TopoHeight>, BlockchainError> {
        trace!("fallback on storage for get_pruned_topoheight");
        self.storage.get_pruned_topoheight().await
    }

    async fn set_pruned_topoheight(
        &mut self,
        _: Option<TopoHeight>,
    ) -> Result<(), BlockchainError> {
        Err(BlockchainError::UnsupportedOperation)
    }
}

#[async_trait]
impl<S: Storage> MerkleHashProvider for ChainValidatorProvider<'_, S> {
    async fn get_balances_merkle_hash_at_topoheight(
        &self,
        topoheight: TopoHeight,
    ) -> Result<Hash, BlockchainError> {
        trace!("fallback on storage for get_balances_merkle_hash_at_topoheight");
        self.storage
            .get_balances_merkle_hash_at_topoheight(topoheight)
            .await
    }

    async fn set_balances_merkle_hash_at_topoheight(
        &mut self,
        _: TopoHeight,
        _: &Hash,
    ) -> Result<(), BlockchainError> {
        Err(BlockchainError::UnsupportedOperation)
    }
}

// GHOSTDAG Data Provider implementation (TIP-2 Phase 1)
// Checks chain validator cache first, then falls back to underlying storage
#[async_trait]
impl<S: Storage> GhostdagDataProvider for ChainValidatorProvider<'_, S> {
    async fn get_ghostdag_blue_score(&self, hash: &Hash) -> Result<u64, BlockchainError> {
        // Check cache first
        if let Some(data) = self.parent.blocks.get(hash) {
            return Ok(data.blue_score);
        }
        trace!("fallback on storage for get_ghostdag_blue_score");
        self.storage.get_ghostdag_blue_score(hash).await
    }

    async fn get_ghostdag_blue_work(&self, hash: &Hash) -> Result<BlueWorkType, BlockchainError> {
        // Check cache first
        if let Some(data) = self.parent.blocks.get(hash) {
            return Ok(data.blue_work);
        }
        trace!("fallback on storage for get_ghostdag_blue_work");
        self.storage.get_ghostdag_blue_work(hash).await
    }

    async fn get_ghostdag_selected_parent(&self, hash: &Hash) -> Result<Hash, BlockchainError> {
        // Check cache first
        if let Some(data) = self.parent.blocks.get(hash) {
            return Ok(data.ghostdag_data.selected_parent.clone());
        }
        trace!("fallback on storage for get_ghostdag_selected_parent");
        self.storage.get_ghostdag_selected_parent(hash).await
    }

    async fn get_ghostdag_mergeset_blues(
        &self,
        hash: &Hash,
    ) -> Result<Arc<Vec<Hash>>, BlockchainError> {
        // Check cache first
        if let Some(data) = self.parent.blocks.get(hash) {
            return Ok(data.ghostdag_data.mergeset_blues.clone());
        }
        trace!("fallback on storage for get_ghostdag_mergeset_blues");
        self.storage.get_ghostdag_mergeset_blues(hash).await
    }

    async fn get_ghostdag_mergeset_reds(
        &self,
        hash: &Hash,
    ) -> Result<Arc<Vec<Hash>>, BlockchainError> {
        // Check cache first
        if let Some(data) = self.parent.blocks.get(hash) {
            return Ok(data.ghostdag_data.mergeset_reds.clone());
        }
        trace!("fallback on storage for get_ghostdag_mergeset_reds");
        self.storage.get_ghostdag_mergeset_reds(hash).await
    }

    async fn get_ghostdag_blues_anticone_sizes(
        &self,
        hash: &Hash,
    ) -> Result<Arc<std::collections::HashMap<Hash, KType>>, BlockchainError> {
        // Check cache first
        if let Some(data) = self.parent.blocks.get(hash) {
            return Ok(data.ghostdag_data.blues_anticone_sizes.clone());
        }
        trace!("fallback on storage for get_ghostdag_blues_anticone_sizes");
        self.storage.get_ghostdag_blues_anticone_sizes(hash).await
    }

    async fn get_ghostdag_data(
        &self,
        hash: &Hash,
    ) -> Result<Arc<TosGhostdagData>, BlockchainError> {
        // Check cache first
        if let Some(data) = self.parent.blocks.get(hash) {
            return Ok(data.ghostdag_data.clone());
        }
        trace!("fallback on storage for get_ghostdag_data");
        self.storage.get_ghostdag_data(hash).await
    }

    async fn get_ghostdag_compact_data(
        &self,
        hash: &Hash,
    ) -> Result<CompactGhostdagData, BlockchainError> {
        // Check cache first
        if let Some(data) = self.parent.blocks.get(hash) {
            return Ok(CompactGhostdagData {
                blue_score: data.blue_score,
                blue_work: data.blue_work,
                selected_parent: data.ghostdag_data.selected_parent.clone(),
            });
        }
        trace!("fallback on storage for get_ghostdag_compact_data");
        self.storage.get_ghostdag_compact_data(hash).await
    }

    async fn has_ghostdag_data(&self, hash: &Hash) -> Result<bool, BlockchainError> {
        // Check cache first
        if self.parent.blocks.contains_key(hash) {
            return Ok(true);
        }
        trace!("fallback on storage for has_ghostdag_data");
        self.storage.has_ghostdag_data(hash).await
    }

    async fn insert_ghostdag_data(
        &mut self,
        _: &Hash,
        _: Arc<TosGhostdagData>,
    ) -> Result<(), BlockchainError> {
        Err(BlockchainError::UnsupportedOperation)
    }

    async fn delete_ghostdag_data(&mut self, _: &Hash) -> Result<(), BlockchainError> {
        Err(BlockchainError::UnsupportedOperation)
    }
}

// Reachability Data Provider implementation for GHOSTDAG
// ChainValidator doesn't store reachability data, so we always fall back to storage
#[async_trait]
impl<S: Storage> ReachabilityDataProvider for ChainValidatorProvider<'_, S> {
    async fn get_reachability_data(
        &self,
        hash: &Hash,
    ) -> Result<ReachabilityData, BlockchainError> {
        trace!("fallback on storage for get_reachability_data");
        self.storage.get_reachability_data(hash).await
    }

    async fn has_reachability_data(&self, hash: &Hash) -> Result<bool, BlockchainError> {
        trace!("fallback on storage for has_reachability_data");
        self.storage.has_reachability_data(hash).await
    }

    async fn set_reachability_data(
        &mut self,
        _: &Hash,
        _: &ReachabilityData,
    ) -> Result<(), BlockchainError> {
        Err(BlockchainError::UnsupportedOperation)
    }

    async fn delete_reachability_data(&mut self, _: &Hash) -> Result<(), BlockchainError> {
        Err(BlockchainError::UnsupportedOperation)
    }

    async fn get_reindex_root(&self) -> Result<Hash, BlockchainError> {
        trace!("fallback on storage for get_reindex_root");
        self.storage.get_reindex_root().await
    }

    async fn set_reindex_root(&mut self, _: Hash) -> Result<(), BlockchainError> {
        Err(BlockchainError::UnsupportedOperation)
    }
}

// GhostdagStorageProvider implementation
// This allows GHOSTDAG algorithm to use ChainValidatorProvider as storage
// Required for correct chain validation during sync
//
// NOTE: get_block_header_by_hash is provided via DifficultyProvider supertrait
// (implemented above for ChainValidatorProvider)
#[async_trait]
impl<S: Storage> GhostdagStorageProvider for ChainValidatorProvider<'_, S> {
    async fn has_block_with_hash(&self, hash: &Hash) -> Result<bool, BlockchainError> {
        // Check cache first, then storage
        Ok(self.parent.blocks.contains_key(hash) || self.storage.has_block_with_hash(hash).await?)
    }
}
