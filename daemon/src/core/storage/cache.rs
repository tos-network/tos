// Storage cache module - provides application-level caching for the blockchain
//
// This module implements a layered cache architecture where:
// - ChainCache holds chain state (height, topoheight, difficulty) and DAG operation caches
// - ObjectsCache holds LRU caches for transactions, blocks, and other objects
// - StorageCache combines both and is stored in RocksStorage
// - Snapshot includes a copy of StorageCache for atomic rollback support

use std::{
    collections::HashSet,
    num::NonZeroUsize,
    ops::{Deref, DerefMut},
    sync::Arc,
};

use indexmap::IndexSet;
use lru::LruCache;
use tos_common::{
    block::{BlockHeader, TopoHeight},
    crypto::Hash,
    difficulty::{CumulativeDifficulty, Difficulty},
    tokio::sync::Mutex,
    transaction::Transaction,
};

use crate::config::{DEFAULT_CACHE_SIZE_NONZERO, GENESIS_BLOCK_DIFFICULTY};

use super::Tips;

/// Macro to initialize an LRU cache with a given size
#[macro_export]
macro_rules! init_cache {
    ($cache_size: expr) => {{
        Mutex::new(LruCache::new(
            NonZeroUsize::new($cache_size).expect("Non zero value for cache"),
        ))
    }};
}

/// Counter cache for tracking blockchain statistics
/// These are simple counters that don't require LRU eviction
#[derive(Debug, Default, Clone)]
pub struct CounterCache {
    /// Count of registered assets
    pub assets_count: u64,
    /// Count of registered accounts
    pub accounts_count: u64,
    /// Count of transactions
    pub transactions_count: u64,
    /// Count of blocks
    pub blocks_count: u64,
    /// Count of blocks added to the execution order
    pub blocks_execution_count: u64,
    /// Count of deployed contracts
    pub contracts_count: u64,
    /// Pruned topoheight (if pruning is enabled)
    pub pruned_topoheight: Option<TopoHeight>,
}

/// Chain cache for blockchain state and DAG operation caches
///
/// This struct holds both the chain state (height, topoheight, etc.) and
/// the LRU caches used for DAG computations. Moving this to storage enables
/// atomic rollback of chain state during snapshot operations.
#[derive(Debug)]
pub struct ChainCache {
    /// Cache for tip base computation: (tip_hash, tip_height) -> (base_hash, base_height)
    pub tip_base_cache: Mutex<LruCache<(Hash, u64), (Hash, u64)>>,
    /// Cache for common base computation: combined_tips_hash -> (base_hash, base_height)
    pub common_base_cache: Mutex<LruCache<Hash, (Hash, u64)>>,
    /// Cache for tip work score: (tip_hash, base_hash, base_height) -> (ancestors, cumulative_difficulty)
    pub tip_work_score_cache:
        Mutex<LruCache<(Hash, Hash, u64), (HashSet<Hash>, CumulativeDifficulty)>>,
    /// Cache for full DAG order: (base_hash, tip_hash, base_height) -> ordered_ancestors
    pub full_order_cache: Mutex<LruCache<(Hash, Hash, u64), IndexSet<Hash>>>,
    /// Current network difficulty at tips
    pub difficulty: Difficulty,
    /// Current block height
    pub height: u64,
    /// Current topo height
    pub topoheight: TopoHeight,
    /// Current stable height
    pub stable_height: u64,
    /// Current stable topo height (chain rewind limit)
    pub stable_topoheight: TopoHeight,
    /// Current chain tips
    pub tips: Tips,
}

impl ChainCache {
    /// Clear all LRU caches (does not affect chain state)
    pub fn clear_caches(&mut self) {
        self.tip_base_cache.get_mut().clear();
        self.common_base_cache.get_mut().clear();
        self.tip_work_score_cache.get_mut().clear();
        self.full_order_cache.get_mut().clear();
    }

    /// Create a deep clone with mutable access to LRU caches
    ///
    /// This is used when starting a snapshot to preserve the current cache state.
    /// The returned clone is independent - modifications won't affect the original.
    pub fn clone_mut(&mut self) -> Self {
        Self {
            tip_base_cache: Mutex::new(self.tip_base_cache.get_mut().clone()),
            common_base_cache: Mutex::new(self.common_base_cache.get_mut().clone()),
            tip_work_score_cache: Mutex::new(self.tip_work_score_cache.get_mut().clone()),
            full_order_cache: Mutex::new(self.full_order_cache.get_mut().clone()),
            height: self.height,
            topoheight: self.topoheight,
            stable_height: self.stable_height,
            stable_topoheight: self.stable_topoheight,
            difficulty: self.difficulty,
            tips: self.tips.clone(),
        }
    }
}

impl Default for ChainCache {
    fn default() -> Self {
        Self {
            tip_base_cache: Mutex::new(LruCache::new(DEFAULT_CACHE_SIZE_NONZERO)),
            common_base_cache: Mutex::new(LruCache::new(DEFAULT_CACHE_SIZE_NONZERO)),
            tip_work_score_cache: Mutex::new(LruCache::new(DEFAULT_CACHE_SIZE_NONZERO)),
            full_order_cache: Mutex::new(LruCache::new(DEFAULT_CACHE_SIZE_NONZERO)),
            height: 0,
            topoheight: 0,
            stable_height: 0,
            stable_topoheight: 0,
            difficulty: GENESIS_BLOCK_DIFFICULTY,
            tips: Tips::default(),
        }
    }
}

/// Objects cache for blockchain data objects
///
/// Contains LRU caches for frequently accessed objects like transactions,
/// block headers, and topoheight mappings.
#[derive(Debug)]
pub struct ObjectsCache {
    /// Transaction cache: tx_hash -> Transaction
    pub transactions_cache: Mutex<LruCache<Hash, Arc<Transaction>>>,
    /// Block header cache: block_hash -> BlockHeader
    pub blocks_cache: Mutex<LruCache<Hash, Arc<BlockHeader>>>,
    /// Topoheight by hash cache: block_hash -> topoheight
    pub topo_by_hash_cache: Mutex<LruCache<Hash, TopoHeight>>,
    /// Hash at topoheight cache: topoheight -> block_hash
    pub hash_at_topo_cache: Mutex<LruCache<TopoHeight, Hash>>,
    /// Cumulative difficulty cache: block_hash -> cumulative_difficulty
    pub cumulative_difficulty_cache: Mutex<LruCache<Hash, CumulativeDifficulty>>,
    /// Assets cache: asset_hash -> registration_topoheight
    pub assets_cache: Mutex<LruCache<Hash, TopoHeight>>,
}

impl ObjectsCache {
    /// Create a new ObjectsCache with the specified cache size
    pub fn new(cache_size: usize) -> Self {
        Self {
            transactions_cache: init_cache!(cache_size),
            blocks_cache: init_cache!(cache_size),
            topo_by_hash_cache: init_cache!(cache_size),
            hash_at_topo_cache: init_cache!(cache_size),
            cumulative_difficulty_cache: init_cache!(cache_size),
            assets_cache: init_cache!(cache_size),
        }
    }

    /// Create a deep clone with mutable access to LRU caches
    pub fn clone_mut(&mut self) -> Self {
        Self {
            transactions_cache: Mutex::new(self.transactions_cache.get_mut().clone()),
            blocks_cache: Mutex::new(self.blocks_cache.get_mut().clone()),
            topo_by_hash_cache: Mutex::new(self.topo_by_hash_cache.get_mut().clone()),
            hash_at_topo_cache: Mutex::new(self.hash_at_topo_cache.get_mut().clone()),
            cumulative_difficulty_cache: Mutex::new(
                self.cumulative_difficulty_cache.get_mut().clone(),
            ),
            assets_cache: Mutex::new(self.assets_cache.get_mut().clone()),
        }
    }

    /// Clear all LRU caches
    pub fn clear_caches(&mut self) {
        self.transactions_cache.get_mut().clear();
        self.blocks_cache.get_mut().clear();
        self.topo_by_hash_cache.get_mut().clear();
        self.hash_at_topo_cache.get_mut().clear();
        self.cumulative_difficulty_cache.get_mut().clear();
        self.assets_cache.get_mut().clear();
    }
}

/// Main storage cache container
///
/// Combines CounterCache, ChainCache, and ObjectsCache into a single structure.
/// This is stored in RocksStorage and copied into Snapshot for atomic rollback.
///
/// During a standard clone, only counters are cloned and caches are reset.
/// Use `clone_mut()` for a deep clone that preserves cache contents.
#[derive(Debug, Default)]
pub struct StorageCache {
    /// Counter cache for blockchain statistics
    pub counter: CounterCache,
    /// Chain cache for state and DAG operation caches
    pub chain: ChainCache,
    /// Object caches (optional, depends on cache_size)
    pub objects: Option<ObjectsCache>,
    /// Cache size used for initialization
    pub cache_size: Option<usize>,
}

impl StorageCache {
    /// Create a new StorageCache with optional object caching
    ///
    /// If `cache_size` is Some, ObjectsCache will be initialized.
    /// If None, only ChainCache will be available.
    pub fn new(cache_size: Option<usize>) -> Self {
        Self {
            counter: CounterCache::default(),
            chain: ChainCache::default(),
            objects: cache_size.map(ObjectsCache::new),
            cache_size,
        }
    }

    /// Clear all LRU caches in both ChainCache and ObjectsCache
    ///
    /// This does not affect counter values or chain state.
    pub fn clear_caches(&mut self) {
        self.chain.clear_caches();
        if let Some(objects) = &mut self.objects {
            objects.clear_caches();
        }
    }

    /// Create a deep clone with mutable access to all caches
    ///
    /// This is used when starting a snapshot to preserve the complete cache state.
    /// The returned clone is fully independent from the original.
    pub fn clone_mut(&mut self) -> Self {
        Self {
            counter: self.counter.clone(),
            chain: self.chain.clone_mut(),
            objects: self.objects.as_mut().map(|v| v.clone_mut()),
            cache_size: self.cache_size,
        }
    }
}

/// Deref to CounterCache for convenient access to counter fields
impl Deref for StorageCache {
    type Target = CounterCache;

    fn deref(&self) -> &Self::Target {
        &self.counter
    }
}

/// DerefMut to CounterCache for convenient mutable access to counter fields
impl DerefMut for StorageCache {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.counter
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter_cache_default() {
        let cache = CounterCache::default();
        assert_eq!(cache.assets_count, 0);
        assert_eq!(cache.accounts_count, 0);
        assert_eq!(cache.transactions_count, 0);
        assert_eq!(cache.blocks_count, 0);
        assert_eq!(cache.blocks_execution_count, 0);
        assert_eq!(cache.contracts_count, 0);
        assert!(cache.pruned_topoheight.is_none());
    }

    #[test]
    fn test_chain_cache_default() {
        let cache = ChainCache::default();
        assert_eq!(cache.height, 0);
        assert_eq!(cache.topoheight, 0);
        assert_eq!(cache.stable_height, 0);
        assert_eq!(cache.stable_topoheight, 0);
        assert_eq!(cache.difficulty, GENESIS_BLOCK_DIFFICULTY);
        assert!(cache.tips.is_empty());
    }

    #[test]
    fn test_chain_cache_clone_mut() {
        let mut cache = ChainCache::default();
        cache.height = 100;
        cache.topoheight = 200;

        let cloned = cache.clone_mut();
        assert_eq!(cloned.height, 100);
        assert_eq!(cloned.topoheight, 200);

        // Modify original, cloned should not change
        cache.height = 150;
        assert_eq!(cloned.height, 100);
    }

    #[test]
    fn test_storage_cache_new() {
        let cache = StorageCache::new(Some(1024));
        assert!(cache.objects.is_some());
        assert_eq!(cache.cache_size, Some(1024));

        let cache_no_objects = StorageCache::new(None);
        assert!(cache_no_objects.objects.is_none());
        assert!(cache_no_objects.cache_size.is_none());
    }

    #[test]
    fn test_storage_cache_clone_mut() {
        let mut cache = StorageCache::new(Some(1024));
        cache.chain.height = 100;
        cache.counter.blocks_count = 50;

        let cloned = cache.clone_mut();
        assert_eq!(cloned.chain.height, 100);
        assert_eq!(cloned.counter.blocks_count, 50);

        // Modify original
        cache.chain.height = 200;
        cache.counter.blocks_count = 100;

        // Cloned should retain original values
        assert_eq!(cloned.chain.height, 100);
        assert_eq!(cloned.counter.blocks_count, 50);
    }

    #[test]
    fn test_storage_cache_deref() {
        let mut cache = StorageCache::new(None);
        cache.blocks_count = 42;
        assert_eq!(cache.blocks_count, 42);
    }
}
