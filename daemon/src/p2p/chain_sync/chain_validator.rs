use std::{collections::HashMap, sync::Arc};
use async_trait::async_trait;
use indexmap::{IndexMap, IndexSet};
use tos_common::{
    block::{BlockHeader, BlockVersion, TopoHeight},
    config::TIPS_LIMIT,
    crypto::Hash,
    difficulty::{
        CumulativeDifficulty,
        Difficulty
    },
    immutable::Immutable,
    time::TimestampMillis,
    varuint::VarUint
};
use crate::core::{
    blockchain::Blockchain,
    blockdag,
    error::BlockchainError,
    ghostdag::{self, BlueWorkType, CompactGhostdagData, KType, TosGhostdagData},
    hard_fork::{get_pow_algorithm_for_version, get_version_at_height},
    storage::{
        BlocksAtHeightProvider,
        DagOrderProvider,
        DifficultyProvider,
        GhostdagDataProvider,
        MerkleHashProvider,
        PrunedTopoheightProvider,
        Storage
    }
};
use log::{debug, trace};

// This struct is used to store the block data in the chain validator
struct BlockData {
    header: Arc<BlockHeader>,
    topoheight: TopoHeight,
    difficulty: Difficulty,
    cumulative_difficulty: CumulativeDifficulty, // Legacy: kept for P2P compatibility only
    blue_work: BlueWorkType, // GHOSTDAG: Used for consensus chain selection
    p: VarUint
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
    hash_at_topo: IndexMap<TopoHeight, Arc<Hash>>
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
            hash_at_topo: IndexMap::new()
        }
    }

    // GHOSTDAG: Check if the chain validator has a higher blue_work than our blockchain
    // blue_work is the cumulative work of all blue blocks in GHOSTDAG consensus
    // This is the correct metric for DAG chain selection (NOT cumulative_difficulty)
    pub async fn has_higher_cumulative_difficulty(&self) -> Result<bool, BlockchainError> {
        let new_blue_work = self.get_expected_chain_blue_work()
            .ok_or(BlockchainError::NotEnoughBlocks)?;

        // Retrieve the current blue_work from GHOSTDAG data
        let current_blue_work = {
            debug!("locking storage for blue work comparison");
            let storage = self.blockchain.get_storage().read().await;
            debug!("storage lock acquired for blue work comparison");
            let top_block_hash = self.blockchain.get_top_block_hash_for_storage(&storage).await?;
            storage.get_ghostdag_blue_work(&top_block_hash).await?
        };

        debug!("Chain comparison: peer blue_work = {}, our blue_work = {}", new_blue_work, current_blue_work);
        Ok(new_blue_work > current_blue_work)
    }

    // GHOSTDAG: Retrieve the blue_work of the chain validator
    // This is the blue_work of the last block added, which represents the total work of the chain
    pub fn get_expected_chain_blue_work(&self) -> Option<BlueWorkType> {
        debug!("retrieving expected chain blue work");
        let (_, hash) = self.hash_at_topo.last()?;

        debug!("looking for blue work of {}", hash);
        self.blocks.get(hash)
            .map(|data| data.blue_work)
    }

    // validate the basic chain structure
    // We expect that the block added is the next block ordered by topoheight
    pub async fn insert_block(&mut self, hash: Hash, header: BlockHeader, topoheight: TopoHeight) -> Result<(), BlockchainError> {
        debug!("Inserting block {} into chain validator with expected topoheight {}", hash, topoheight);

        if self.blocks.contains_key(&hash) {
            debug!("Block {} is already in validator chain!", hash);
            return Err(BlockchainError::AlreadyInChain)
        }

        let storage = self.blockchain.get_storage().read().await;
        debug!("storage locked for chain validator insert block");

        if storage.has_block_with_hash(&hash).await? {
            debug!("Block {} is already in blockchain!", hash);
            return Err(BlockchainError::AlreadyInChain)
        }

        let provider = ChainValidatorProvider {
            parent: &self,
            storage: &storage,
        };

        // Verify the block version
        let version = get_version_at_height(self.blockchain.get_network(), header.get_blue_score());
        if version != header.get_version() {
            debug!("Block {} has version {} while expected version is {}", hash, header.get_version(), version);
            return Err(BlockchainError::InvalidBlockVersion)
        }

        // GHOSTDAG: Verify the block blue_score by tips
        let blue_score_at_tips = blockdag::calculate_blue_score_at_tips(&provider, header.get_parents().iter()).await?;
        if blue_score_at_tips != header.get_blue_score() {
            debug!("Block {} has blue_score {} while expected blue_score is {}", hash, header.get_blue_score(), blue_score_at_tips);
            return Err(BlockchainError::InvalidBlockHeight(blue_score_at_tips, header.get_blue_score()))
        }

        let tips = header.get_parents();
        let tips_count = tips.len();

        // verify tips count
        if tips_count == 0 || tips_count > TIPS_LIMIT {
            debug!("Block {} contains {} tips while only {} is accepted", hash, tips_count, TIPS_LIMIT);
            return Err(BlockchainError::InvalidTipsCount(hash, tips_count))
        }

        // verify that we have already all its tips
        {
            for tip in tips.iter() {
                trace!("Checking tip {} for block {}", tip, hash);
                if !self.blocks.contains_key(tip) && !provider.has_block_with_hash(tip).await? {
                    debug!("Block {} contains tip {} which is not present in chain validator", hash, tip);
                    return Err(BlockchainError::InvalidTipsNotFound(hash, tip.clone()))
                }
            }
        }

        // GHOSTDAG: Verify the block blue_score by tips
        {
            let blue_score_by_tips = blockdag::calculate_blue_score_at_tips(&provider, header.get_parents().iter()).await?;
            if blue_score_by_tips != header.get_blue_score() {
                debug!("Block {} has blue_score {} while expected blue_score is {}", hash, header.get_blue_score(), blue_score_by_tips);
                return Err(BlockchainError::InvalidBlockHeight(blue_score_by_tips, header.get_blue_score()))
            }
        }

        let algorithm = get_pow_algorithm_for_version(version);
        let pow_hash = header.get_pow_hash(algorithm)?;
        trace!("POW hash: {}", pow_hash);
        let (difficulty, p) = self.blockchain.verify_proof_of_work(&provider, &pow_hash, tips.iter()).await?;

        // Find the common base between the block and the current blockchain
        let tips_vec = header.get_parents();
        let (base, base_height) = self.blockchain.find_common_base(&provider, tips_vec).await?;

        trace!("Common base: {} at height {} and hash {}", base, base_height, hash);

        // Find the cumulative difficulty for this block (legacy - kept for P2P compatibility)
        let (_, cumulative_difficulty) = self.blockchain.find_tip_work_score(
            &provider,
            &hash,
            header.get_parents().iter(),
            Some(difficulty.clone()),
            &base,
            base_height
        ).await?;

        // GHOSTDAG: Calculate blue_work for consensus chain selection
        // blue_work = max(parent.blue_work) + difficulty of this block
        // This is the correct metric for DAG chain selection
        let blue_work = {
            let mut max_parent_blue_work = BlueWorkType::zero();
            for parent_hash in header.get_parents().iter() {
                let parent_blue_work = provider.get_ghostdag_blue_work(parent_hash).await?;
                if parent_blue_work > max_parent_blue_work {
                    max_parent_blue_work = parent_blue_work;
                }
            }
            // Add this block's difficulty (converted to work) to the max parent blue_work
            // Use the GHOSTDAG calc_work_from_difficulty function to properly convert
            let block_work = ghostdag::calc_work_from_difficulty(&difficulty);
            max_parent_blue_work + block_work
        };

        debug!("Block {} - blue_work: {}, cumulative_difficulty (legacy): {}", hash, blue_work, cumulative_difficulty);

        let hash = Arc::new(hash);
        // Store the block in both maps
        // One is for blocks at height and the other is for the block data
        self.blocks_at_height.entry(header.get_blue_score())
            .or_insert_with(IndexSet::new)
            .insert(hash.clone());

        self.blocks.insert(hash.clone(), BlockData {
            header: Arc::new(header),
            topoheight,
            difficulty,
            cumulative_difficulty, // Legacy: kept for P2P compatibility only
            blue_work, // GHOSTDAG: Used for consensus chain selection
            p
        });

        self.hash_at_topo.insert(topoheight, hash);

        Ok(())
    }

    pub fn get_block(&mut self, hash: &Hash) -> Option<Arc<BlockHeader>> {
        debug!("retrieving block header for {}", hash);
        self.blocks.get(hash).map(|v| v.header.clone())
    }
}

#[async_trait]
impl<S: Storage> DifficultyProvider for ChainValidatorProvider<'_, S> {
    async fn get_height_for_block_hash(&self, hash: &Hash) -> Result<u64, BlockchainError> {
        trace!("get height for block hash {}", hash);
        if let Some(data) = self.parent.blocks.get(hash) {
            return Ok(data.header.get_blue_score())
        }

        trace!("fallback on storage for get_height_for_block_hash");
        self.storage.get_height_for_block_hash(hash).await
    }

    // Get the block version using its hash
    async fn get_version_for_block_hash(&self, hash: &Hash) -> Result<BlockVersion, BlockchainError> {
        trace!("get version for block hash {}", hash);
        if let Some(data) = self.parent.blocks.get(hash) {
            return Ok(data.header.get_version())
        }

        trace!("fallback on storage for get_version_for_block_hash");
        self.storage.get_version_for_block_hash(hash).await
    }

    async fn get_timestamp_for_block_hash(&self, hash: &Hash) -> Result<TimestampMillis, BlockchainError> {
        trace!("get timestamp for block hash {}", hash);
        if let Some(data) = self.parent.blocks.get(hash) {
            return Ok(data.header.get_timestamp())
        }

        trace!("fallback on storage for get_timestamp_for_block_hash");
        self.storage.get_timestamp_for_block_hash(hash).await
    }

    async fn get_difficulty_for_block_hash(&self, hash: &Hash) -> Result<Difficulty, BlockchainError> {
        trace!("get difficulty for block hash {}", hash);
        if let Some(data) = self.parent.blocks.get(hash) {
            return Ok(data.difficulty)
        }

        trace!("fallback on storage for get_difficulty_for_block_hash");
        self.storage.get_difficulty_for_block_hash(hash).await
    }

    async fn get_cumulative_difficulty_for_block_hash(&self, hash: &Hash) -> Result<CumulativeDifficulty, BlockchainError> {
        trace!("get cumulative difficulty for block hash {}", hash);
        if let Some(data) = self.parent.blocks.get(hash) {
            return Ok(data.cumulative_difficulty)
        }

        trace!("fallback on storage for get_cumulative_difficulty_for_block_hash");
        self.storage.get_cumulative_difficulty_for_block_hash(hash).await
    }

    async fn get_past_blocks_for_block_hash(&self, hash: &Hash) -> Result<Immutable<IndexSet<Hash>>, BlockchainError> {
        trace!("get past blocks for block hash {}", hash);
        if let Some(data) = self.parent.blocks.get(hash) {
            let tips: IndexSet<Hash> = data.header.get_parents().iter().cloned().collect();
            return Ok(Immutable::Owned(tips))
        }

        trace!("fallback on storage for get_past_blocks_for_block_hash");
        self.storage.get_past_blocks_for_block_hash(hash).await
    }

    async fn get_block_header_by_hash(&self, hash: &Hash) -> Result<Immutable<BlockHeader>, BlockchainError> {
        trace!("get block header by hash {}", hash);
        if let Some(data) = self.parent.blocks.get(hash) {
            return Ok(Immutable::Arc(data.header.clone()))
        }

        trace!("fallback on storage for get_block_header_by_hash");
        self.storage.get_block_header_by_hash(hash).await
    }

    async fn get_estimated_covariance_for_block_hash(&self, hash: &Hash) -> Result<VarUint, BlockchainError> {
        trace!("get estimated covariance for block hash {}", hash);
        if let Some(data) = self.parent.blocks.get(hash) {
            return Ok(data.p.clone())
        }

        trace!("fallback on storage for get_estimated_covariance_for_block_hash");
        self.storage.get_estimated_covariance_for_block_hash(hash).await
    }
}

#[async_trait]
impl<S: Storage> DagOrderProvider for ChainValidatorProvider<'_, S> {
    async fn get_topo_height_for_hash(&self, hash: &Hash) -> Result<TopoHeight, BlockchainError> {
        trace!("get topo height for hash {}", hash);
        if let Some(data) = self.parent.blocks.get(hash) {
            return Ok(data.topoheight);
        }

        trace!("fallback on storage for get_topo_height_for_hash");
        self.storage.get_topo_height_for_hash(hash).await
    }

    // This should never happen in our case
    async fn set_topo_height_for_block(&mut self, _: &Hash, _: TopoHeight) -> Result<(), BlockchainError> {
        Err(BlockchainError::UnsupportedOperation)
    }

    async fn is_block_topological_ordered(&self, hash: &Hash) -> Result<bool, BlockchainError> {
        trace!("is block topological ordered {}", hash);
        if self.parent.blocks.contains_key(hash) {
            return Ok(true)
        }

        trace!("fallback on storage for is_block_topological_ordered");
        self.storage.is_block_topological_ordered(hash).await
    }

    async fn get_hash_at_topo_height(&self, topoheight: TopoHeight) -> Result<Hash, BlockchainError> {
        trace!("get hash at topoheight {}", topoheight);
        if let Some(hash) = self.parent.hash_at_topo.get(&topoheight) {
            return Ok(hash.as_ref().clone())
        }

        trace!("fallback on storage for get_hash_at_topo_height");
        self.storage.get_hash_at_topo_height(topoheight).await
    }

    async fn has_hash_at_topoheight(&self, topoheight: TopoHeight) -> Result<bool, BlockchainError> {
        trace!("has hash at topoheight {}", topoheight);
        if self.parent.hash_at_topo.contains_key(&topoheight) {
            return Ok(true)
        }

        trace!("fallback on storage for has_hash_at_topoheight");
        self.storage.has_hash_at_topoheight(topoheight).await
    }

    async fn get_orphaned_blocks<'a>(&'a self) -> Result<impl Iterator<Item = Result<Hash, BlockchainError>> + 'a, BlockchainError> {
        trace!("get orphaned blocks");
        let iter = self.storage.get_orphaned_blocks().await?;
        Ok(iter)
    }
}

#[async_trait]
impl<S: Storage> BlocksAtHeightProvider for ChainValidatorProvider<'_, S> {
    async fn has_blocks_at_height(&self, height: u64) -> Result<bool, BlockchainError> {
        trace!("has block at height {}", height);
        if self.parent.blocks_at_height.contains_key(&height) {
            return Ok(true)
        }

        trace!("fallback on storage for has_blocks_at_height");
        self.storage.has_blocks_at_height(height).await
    }

    // Retrieve the blocks hashes at a specific height
    async fn get_blocks_at_height(&self, height: u64) -> Result<IndexSet<Hash>, BlockchainError> {
        trace!("get blocks at height {}", height);
        if let Some(tips) = self.parent.blocks_at_height.get(&height) {
            // TODO
            return Ok(tips.iter().map(|v| v.as_ref().clone()).collect())
        }

        trace!("fallback on storage for get_blocks_at_height");
        self.storage.get_blocks_at_height(height).await
    }

    // This is used to store the blocks hashes at a specific height
    async fn set_blocks_at_height(&mut self, _: &IndexSet<Hash>, _: u64) -> Result<(), BlockchainError> {
        Err(BlockchainError::UnsupportedOperation)
    }

    // Append a block hash at a specific height
    async fn add_block_hash_at_height(&mut self, _: &Hash, _: u64) -> Result<(), BlockchainError> {
        Err(BlockchainError::UnsupportedOperation)
    }

    // Remove a block hash at a specific height
    async fn remove_block_hash_at_height(&mut self, _: &Hash, _: u64) -> Result<(), BlockchainError> {
        Err(BlockchainError::UnsupportedOperation)
    }
}

#[async_trait]
impl<S: Storage> PrunedTopoheightProvider for ChainValidatorProvider<'_, S> {
    async fn get_pruned_topoheight(&self) -> Result<Option<TopoHeight>, BlockchainError> {
        trace!("fallback on storage for get_pruned_topoheight");
        self.storage.get_pruned_topoheight().await
    }

    async fn set_pruned_topoheight(&mut self, _: Option<TopoHeight>) -> Result<(), BlockchainError> {
        Err(BlockchainError::UnsupportedOperation)
    }
}

#[async_trait]
impl<S: Storage> MerkleHashProvider for ChainValidatorProvider<'_, S> {
    async fn get_balances_merkle_hash_at_topoheight(&self, topoheight: TopoHeight) -> Result<Hash, BlockchainError> {
        trace!("fallback on storage for get_balances_merkle_hash_at_topoheight");
        self.storage.get_balances_merkle_hash_at_topoheight(topoheight).await
    }

    async fn set_balances_merkle_hash_at_topoheight(&mut self,  _: TopoHeight, _: &Hash) -> Result<(), BlockchainError> {
        Err(BlockchainError::UnsupportedOperation)
    }
}

// GHOSTDAG Data Provider implementation (TIP-2 Phase 1)
// Delegates all GHOSTDAG operations to underlying storage
#[async_trait]
impl<S: Storage> GhostdagDataProvider for ChainValidatorProvider<'_, S> {
    async fn get_ghostdag_blue_score(&self, hash: &Hash) -> Result<u64, BlockchainError> {
        trace!("fallback on storage for get_ghostdag_blue_score");
        self.storage.get_ghostdag_blue_score(hash).await
    }

    async fn get_ghostdag_blue_work(&self, hash: &Hash) -> Result<BlueWorkType, BlockchainError> {
        trace!("fallback on storage for get_ghostdag_blue_work");
        self.storage.get_ghostdag_blue_work(hash).await
    }

    async fn get_ghostdag_selected_parent(&self, hash: &Hash) -> Result<Hash, BlockchainError> {
        trace!("fallback on storage for get_ghostdag_selected_parent");
        self.storage.get_ghostdag_selected_parent(hash).await
    }

    async fn get_ghostdag_mergeset_blues(&self, hash: &Hash) -> Result<Arc<Vec<Hash>>, BlockchainError> {
        trace!("fallback on storage for get_ghostdag_mergeset_blues");
        self.storage.get_ghostdag_mergeset_blues(hash).await
    }

    async fn get_ghostdag_mergeset_reds(&self, hash: &Hash) -> Result<Arc<Vec<Hash>>, BlockchainError> {
        trace!("fallback on storage for get_ghostdag_mergeset_reds");
        self.storage.get_ghostdag_mergeset_reds(hash).await
    }

    async fn get_ghostdag_blues_anticone_sizes(&self, hash: &Hash) -> Result<Arc<std::collections::HashMap<Hash, KType>>, BlockchainError> {
        trace!("fallback on storage for get_ghostdag_blues_anticone_sizes");
        self.storage.get_ghostdag_blues_anticone_sizes(hash).await
    }

    async fn get_ghostdag_data(&self, hash: &Hash) -> Result<Arc<TosGhostdagData>, BlockchainError> {
        trace!("fallback on storage for get_ghostdag_data");
        self.storage.get_ghostdag_data(hash).await
    }

    async fn get_ghostdag_compact_data(&self, hash: &Hash) -> Result<CompactGhostdagData, BlockchainError> {
        trace!("fallback on storage for get_ghostdag_compact_data");
        self.storage.get_ghostdag_compact_data(hash).await
    }

    async fn has_ghostdag_data(&self, hash: &Hash) -> Result<bool, BlockchainError> {
        trace!("fallback on storage for has_ghostdag_data");
        self.storage.has_ghostdag_data(hash).await
    }

    async fn insert_ghostdag_data(&mut self, _: &Hash, _: Arc<TosGhostdagData>) -> Result<(), BlockchainError> {
        Err(BlockchainError::UnsupportedOperation)
    }

    async fn delete_ghostdag_data(&mut self, _: &Hash) -> Result<(), BlockchainError> {
        Err(BlockchainError::UnsupportedOperation)
    }
}