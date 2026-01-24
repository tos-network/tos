//! Data Sync & Reorg Testing - Tests FastSync protocol, chain sync,
//! chain validation, DAG reorganization, snapshots, and pruning.

/// Regular chain sync and common point detection tests
pub mod chain_sync;
/// Reorg chain validation tests
pub mod chain_validator;
/// DAG reorganization and block rewind tests
pub mod dag_reorg;
/// FastSync bootstrap protocol tests
pub mod fast_sync;
/// Pruning correctness and safety limit tests
pub mod pruning;
/// Transactional snapshot atomicity tests
pub mod snapshot_atomicity;
/// Pagination and safety limit tests
pub mod sync_limits;

#[cfg(test)]
#[allow(missing_docs)]
pub mod mock {
    use std::collections::BTreeMap;

    // Key constants
    pub const STABLE_LIMIT: u64 = 24;
    pub const PRUNE_SAFETY_LIMIT: u64 = STABLE_LIMIT * 10; // 240
    pub const TIPS_LIMIT: usize = 3;
    pub const MAX_ITEMS_PER_PAGE: usize = 1024;
    pub const MAX_BOOTSTRAP_PAGES: u64 = 100_000;
    pub const MAX_VERSIONS_PER_BALANCE: usize = 1_000_000;
    pub const MAX_ACCUMULATOR_ENTRIES: usize = 10_000_000;
    pub const CHAIN_SYNC_REQUEST_MAX_BLOCKS: usize = 64;
    pub const CHAIN_SYNC_RESPONSE_MIN_BLOCKS: usize = 512;
    pub const CHAIN_SYNC_DEFAULT_RESPONSE_BLOCKS: usize = 4096;
    pub const CHAIN_SYNC_RESPONSE_MAX_BLOCKS: usize = u16::MAX as usize;
    pub const CHAIN_SYNC_TOP_BLOCKS: usize = 10;

    pub type Hash = [u8; 32];
    pub type TopoHeight = u64;
    pub type Difficulty = u64;
    pub type CumulativeDifficulty = u64;

    // Block ID for common point detection
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub struct BlockId {
        pub hash: Hash,
        pub topoheight: TopoHeight,
    }

    // Block metadata
    #[derive(Debug, Clone)]
    pub struct BlockMetadata {
        pub hash: Hash,
        pub topoheight: TopoHeight,
        pub height: u64,
        pub difficulty: Difficulty,
        pub cumulative_difficulty: CumulativeDifficulty,
        pub tips: Vec<Hash>,
        pub txs: Vec<Hash>,
    }

    // Bootstrap step types
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum StepKind {
        ChainInfo,
        Assets,
        Keys,
        Accounts,
        Contracts,
        BlocksMetadata,
    }

    impl StepKind {
        pub fn next(&self) -> Option<StepKind> {
            match self {
                StepKind::ChainInfo => Some(StepKind::Assets),
                StepKind::Assets => Some(StepKind::Keys),
                StepKind::Keys => Some(StepKind::Accounts),
                StepKind::Accounts => Some(StepKind::Contracts),
                StepKind::Contracts => Some(StepKind::BlocksMetadata),
                StepKind::BlocksMetadata => None,
            }
        }
    }

    // Pagination state
    #[derive(Debug, Clone)]
    pub struct PaginationState {
        pub current_page: u64,
        pub items_per_page: usize,
        pub total_items: usize,
    }

    impl PaginationState {
        pub fn new(total_items: usize) -> Self {
            Self {
                current_page: 0,
                items_per_page: MAX_ITEMS_PER_PAGE,
                total_items,
            }
        }

        pub fn next_page(&mut self) -> Option<u64> {
            let offset = self.checked_offset().ok()?;
            if offset >= self.total_items {
                return None;
            }
            let remaining = self.total_items - offset;
            if remaining > self.items_per_page {
                self.current_page += 1;
                Some(self.current_page)
            } else {
                None
            }
        }

        pub fn checked_offset(&self) -> Result<usize, &'static str> {
            (self.current_page as usize)
                .checked_mul(self.items_per_page)
                .ok_or("Page offset overflow")
        }

        pub fn items_this_page(&self) -> usize {
            let offset = match self.checked_offset() {
                Ok(o) => o,
                Err(_) => return 0,
            };
            if offset >= self.total_items {
                return 0;
            }
            std::cmp::min(self.items_per_page, self.total_items - offset)
        }
    }

    // Mock chain state for sync testing
    #[derive(Debug, Clone)]
    pub struct MockChainState {
        pub blocks: BTreeMap<TopoHeight, BlockMetadata>,
        pub tips: Vec<Hash>,
        pub height: u64,
        pub topoheight: TopoHeight,
        pub stable_topoheight: TopoHeight,
        pub pruned_topoheight: Option<TopoHeight>,
        pub cumulative_difficulty: CumulativeDifficulty,
    }

    impl Default for MockChainState {
        fn default() -> Self {
            Self::new()
        }
    }

    impl MockChainState {
        pub fn new() -> Self {
            Self {
                blocks: BTreeMap::new(),
                tips: vec![[0u8; 32]],
                height: 0,
                topoheight: 0,
                stable_topoheight: 0,
                pruned_topoheight: None,
                cumulative_difficulty: 0,
            }
        }

        pub fn add_block(&mut self, meta: BlockMetadata) {
            let topo = meta.topoheight;
            if meta.height > self.height {
                self.height = meta.height;
            }
            if topo > self.topoheight {
                self.topoheight = topo;
            }
            self.cumulative_difficulty += meta.difficulty;
            self.tips = vec![meta.hash];
            self.blocks.insert(topo, meta);
            // Update stable height
            if self.topoheight >= STABLE_LIMIT {
                self.stable_topoheight = self.topoheight - STABLE_LIMIT;
            }
        }

        pub fn get_block_at_topo(&self, topo: TopoHeight) -> Option<&BlockMetadata> {
            self.blocks.get(&topo)
        }

        pub fn has_block(&self, hash: &Hash) -> bool {
            self.blocks.values().any(|b| &b.hash == hash)
        }

        pub fn find_common_point(&self, peer_blocks: &[BlockId]) -> Option<BlockId> {
            // Binary search: find highest block we both have
            let mut best: Option<BlockId> = None;
            for block_id in peer_blocks {
                if self.has_block(&block_id.hash) {
                    match &best {
                        None => best = Some(block_id.clone()),
                        Some(current) => {
                            if block_id.topoheight > current.topoheight {
                                best = Some(block_id.clone());
                            }
                        }
                    }
                }
            }
            best
        }

        // Pop blocks from the top
        pub fn pop_blocks(&mut self, count: u64) -> Vec<BlockMetadata> {
            let mut popped = Vec::new();
            for _ in 0..count {
                if self.topoheight == 0 {
                    break;
                }
                if let Some(pruned) = self.pruned_topoheight {
                    let safety = pruned.saturating_add(PRUNE_SAFETY_LIMIT);
                    if self.topoheight <= safety {
                        break;
                    }
                }
                if let Some(block) = self.blocks.remove(&self.topoheight) {
                    self.cumulative_difficulty =
                        self.cumulative_difficulty.saturating_sub(block.difficulty);
                    popped.push(block);
                }
                self.topoheight = self.topoheight.saturating_sub(1);
            }
            // Update height and tips from remaining blocks
            if let Some((_, last_block)) = self.blocks.iter().next_back() {
                self.height = last_block.height;
                self.tips = vec![last_block.hash];
            } else {
                self.height = 0;
                self.tips = vec![[0u8; 32]];
            }
            // Update stable height
            if self.topoheight >= STABLE_LIMIT {
                self.stable_topoheight = self.topoheight - STABLE_LIMIT;
            } else {
                self.stable_topoheight = 0;
            }
            popped
        }
    }

    // Mock chain validator for reorg testing
    #[derive(Debug)]
    pub struct MockChainValidator {
        pub blocks: Vec<BlockMetadata>,
        pub cumulative_difficulty: CumulativeDifficulty,
    }

    impl Default for MockChainValidator {
        fn default() -> Self {
            Self::new()
        }
    }

    impl MockChainValidator {
        pub fn new() -> Self {
            Self {
                blocks: Vec::new(),
                cumulative_difficulty: 0,
            }
        }

        pub fn insert_block(&mut self, block: BlockMetadata) -> Result<(), &'static str> {
            // Check duplicate
            if self.blocks.iter().any(|b| b.hash == block.hash) {
                return Err("Block already in chain");
            }
            // Check tips count
            if block.tips.is_empty() || block.tips.len() > TIPS_LIMIT {
                return Err("Invalid tips count");
            }
            self.cumulative_difficulty += block.difficulty;
            self.blocks.push(block);
            Ok(())
        }

        pub fn has_higher_cumulative_difficulty(
            &self,
            current: CumulativeDifficulty,
        ) -> Result<bool, &'static str> {
            if self.blocks.is_empty() {
                return Err("No blocks in validator");
            }
            Ok(self.cumulative_difficulty > current)
        }
    }

    // Mock snapshot for atomicity testing
    #[derive(Debug, Clone)]
    pub struct MockSnapshot {
        pub changes: Vec<(String, Option<Vec<u8>>)>, // key -> value (None = delete)
        pub committed: bool,
        pub rolled_back: bool,
    }

    impl Default for MockSnapshot {
        fn default() -> Self {
            Self::new()
        }
    }

    impl MockSnapshot {
        pub fn new() -> Self {
            Self {
                changes: Vec::new(),
                committed: false,
                rolled_back: false,
            }
        }

        pub fn put(&mut self, key: String, value: Vec<u8>) {
            self.changes.push((key, Some(value)));
        }

        pub fn delete(&mut self, key: String) {
            self.changes.push((key, None));
        }

        pub fn commit(&mut self) -> Result<(), &'static str> {
            if self.rolled_back {
                return Err("Cannot commit after rollback");
            }
            self.committed = true;
            Ok(())
        }

        pub fn rollback(&mut self) -> Result<(), &'static str> {
            if self.committed {
                return Err("Cannot rollback after commit");
            }
            self.rolled_back = true;
            self.changes.clear();
            Ok(())
        }
    }

    // Helper to create a linear chain of blocks
    pub fn make_linear_chain(count: u64, base_difficulty: Difficulty) -> MockChainState {
        let mut state = MockChainState::new();
        for i in 1..=count {
            let mut hash = [0u8; 32];
            hash[0..8].copy_from_slice(&i.to_le_bytes());
            let prev_hash = if i == 1 {
                [0u8; 32]
            } else {
                let mut h = [0u8; 32];
                h[0..8].copy_from_slice(&(i - 1).to_le_bytes());
                h
            };
            state.add_block(BlockMetadata {
                hash,
                topoheight: i,
                height: i,
                difficulty: base_difficulty,
                cumulative_difficulty: i * base_difficulty,
                tips: vec![prev_hash],
                txs: Vec::new(),
            });
        }
        state
    }

    // Helper to create block IDs for common point detection
    pub fn make_block_ids(state: &MockChainState) -> Vec<BlockId> {
        state
            .blocks
            .iter()
            .map(|(topo, meta)| BlockId {
                hash: meta.hash,
                topoheight: *topo,
            })
            .collect()
    }
}
