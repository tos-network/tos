// TOS Mining Statistics Module
// Tracks mining performance, block acceptance, and GHOSTDAG metrics

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tos_common::{crypto::Hash, tokio::sync::RwLock};

/// Mining statistics for tracking block production and GHOSTDAG performance
#[derive(Debug)]
pub struct MiningStats {
    /// Total number of blocks found (submitted)
    blocks_found: AtomicU64,

    /// Number of blocks accepted into the chain
    blocks_accepted: AtomicU64,

    /// Number of blocks rejected
    blocks_rejected: AtomicU64,

    /// Number of blue blocks produced
    blue_blocks: AtomicU64,

    /// Number of red blocks produced
    red_blocks: AtomicU64,

    /// Total time spent generating block templates (microseconds)
    template_generation_time_us: AtomicU64,

    /// Number of block templates generated
    templates_generated: AtomicU64,

    /// Total time spent in GHOSTDAG calculations (microseconds)
    ghostdag_calculation_time_us: AtomicU64,

    /// Number of GHOSTDAG calculations performed
    ghostdag_calculations: AtomicU64,

    /// Total time spent in transaction selection (microseconds)
    tx_selection_time_us: AtomicU64,

    /// Number of transaction selection operations
    tx_selections: AtomicU64,

    /// Number of transactions selected for blocks
    txs_selected: AtomicU64,

    /// Cache hits for GHOSTDAG data
    ghostdag_cache_hits: AtomicU64,

    /// Cache misses for GHOSTDAG data
    ghostdag_cache_misses: AtomicU64,

    /// Recent block hashes (for tracking acceptance)
    recent_blocks: RwLock<RecentBlockTracker>,

    /// Statistics start time
    start_time: Instant,
}

/// Tracks recently submitted blocks to determine their acceptance status
#[derive(Debug)]
struct RecentBlockTracker {
    /// Maximum number of blocks to track
    capacity: usize,

    /// Recently submitted blocks (hash -> submission time)
    blocks: Vec<(Hash, Instant, BlockStatus)>,
}

/// Status of a submitted block
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockStatus {
    /// Block was just submitted, status unknown
    Pending,

    /// Block was accepted as a blue block
    AcceptedBlue,

    /// Block was accepted as a red block
    AcceptedRed,

    /// Block was rejected
    Rejected,
}

impl RecentBlockTracker {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            blocks: Vec::with_capacity(capacity),
        }
    }

    fn add_block(&mut self, hash: Hash, status: BlockStatus) {
        // Remove old entries if at capacity
        if self.blocks.len() >= self.capacity {
            self.blocks.remove(0);
        }

        self.blocks.push((hash, Instant::now(), status));
    }

    fn update_status(&mut self, hash: &Hash, status: BlockStatus) -> bool {
        for (block_hash, _, block_status) in self.blocks.iter_mut() {
            if block_hash == hash {
                *block_status = status;
                return true;
            }
        }
        false
    }

    #[allow(dead_code)]
    fn get_status(&self, hash: &Hash) -> Option<BlockStatus> {
        self.blocks
            .iter()
            .find(|(h, _, _)| h == hash)
            .map(|(_, _, status)| *status)
    }

    /// Clean up old entries (older than 5 minutes)
    fn cleanup_old_entries(&mut self) {
        let now = Instant::now();
        self.blocks
            .retain(|(_, time, _)| now.duration_since(*time) < Duration::from_secs(300));
    }
}

impl MiningStats {
    /// Create a new mining statistics tracker
    pub fn new(recent_blocks_capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            blocks_found: AtomicU64::new(0),
            blocks_accepted: AtomicU64::new(0),
            blocks_rejected: AtomicU64::new(0),
            blue_blocks: AtomicU64::new(0),
            red_blocks: AtomicU64::new(0),
            template_generation_time_us: AtomicU64::new(0),
            templates_generated: AtomicU64::new(0),
            ghostdag_calculation_time_us: AtomicU64::new(0),
            ghostdag_calculations: AtomicU64::new(0),
            tx_selection_time_us: AtomicU64::new(0),
            tx_selections: AtomicU64::new(0),
            txs_selected: AtomicU64::new(0),
            ghostdag_cache_hits: AtomicU64::new(0),
            ghostdag_cache_misses: AtomicU64::new(0),
            recent_blocks: RwLock::new(RecentBlockTracker::new(recent_blocks_capacity)),
            start_time: Instant::now(),
        })
    }

    /// Record a block submission
    pub async fn record_block_found(&self, hash: Hash) {
        self.blocks_found.fetch_add(1, Ordering::Relaxed);
        let mut tracker = self.recent_blocks.write().await;
        tracker.add_block(hash, BlockStatus::Pending);
    }

    /// Record a block acceptance
    pub async fn record_block_accepted(&self, hash: &Hash, is_blue: bool) {
        self.blocks_accepted.fetch_add(1, Ordering::Relaxed);

        if is_blue {
            self.blue_blocks.fetch_add(1, Ordering::Relaxed);
        } else {
            self.red_blocks.fetch_add(1, Ordering::Relaxed);
        }

        let mut tracker = self.recent_blocks.write().await;
        let status = if is_blue {
            BlockStatus::AcceptedBlue
        } else {
            BlockStatus::AcceptedRed
        };
        tracker.update_status(hash, status);
    }

    /// Record a block rejection
    pub async fn record_block_rejected(&self, hash: &Hash) {
        self.blocks_rejected.fetch_add(1, Ordering::Relaxed);

        let mut tracker = self.recent_blocks.write().await;
        tracker.update_status(hash, BlockStatus::Rejected);
    }

    /// Record block template generation time
    pub fn record_template_generation(&self, duration: Duration) {
        self.template_generation_time_us
            .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
        self.templates_generated.fetch_add(1, Ordering::Relaxed);
    }

    /// Record GHOSTDAG calculation time
    pub fn record_ghostdag_calculation(&self, duration: Duration) {
        self.ghostdag_calculation_time_us
            .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
        self.ghostdag_calculations.fetch_add(1, Ordering::Relaxed);
    }

    /// Record transaction selection time and count
    pub fn record_tx_selection(&self, duration: Duration, tx_count: u64) {
        self.tx_selection_time_us
            .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
        self.tx_selections.fetch_add(1, Ordering::Relaxed);
        self.txs_selected.fetch_add(tx_count, Ordering::Relaxed);
    }

    /// Record GHOSTDAG cache hit
    pub fn record_cache_hit(&self) {
        self.ghostdag_cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Record GHOSTDAG cache miss
    pub fn record_cache_miss(&self) {
        self.ghostdag_cache_misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Get current statistics snapshot
    pub async fn get_snapshot(&self) -> MiningStatsSnapshot {
        let mut tracker = self.recent_blocks.write().await;
        tracker.cleanup_old_entries();

        let uptime = self.start_time.elapsed();
        let blocks_found = self.blocks_found.load(Ordering::Relaxed);
        let blocks_accepted = self.blocks_accepted.load(Ordering::Relaxed);
        let blocks_rejected = self.blocks_rejected.load(Ordering::Relaxed);
        let blue_blocks = self.blue_blocks.load(Ordering::Relaxed);
        let red_blocks = self.red_blocks.load(Ordering::Relaxed);

        let template_time = self.template_generation_time_us.load(Ordering::Relaxed);
        let templates_generated = self.templates_generated.load(Ordering::Relaxed);

        let ghostdag_time = self.ghostdag_calculation_time_us.load(Ordering::Relaxed);
        let ghostdag_calculations = self.ghostdag_calculations.load(Ordering::Relaxed);

        let tx_selection_time = self.tx_selection_time_us.load(Ordering::Relaxed);
        let tx_selections = self.tx_selections.load(Ordering::Relaxed);
        let txs_selected = self.txs_selected.load(Ordering::Relaxed);

        let cache_hits = self.ghostdag_cache_hits.load(Ordering::Relaxed);
        let cache_misses = self.ghostdag_cache_misses.load(Ordering::Relaxed);

        MiningStatsSnapshot {
            uptime,
            blocks_found,
            blocks_accepted,
            blocks_rejected,
            blue_blocks,
            red_blocks,
            acceptance_rate: if blocks_found > 0 {
                (blocks_accepted as f64 / blocks_found as f64) * 100.0
            } else {
                0.0
            },
            blue_rate: if blocks_accepted > 0 {
                (blue_blocks as f64 / blocks_accepted as f64) * 100.0
            } else {
                0.0
            },
            red_rate: if blocks_accepted > 0 {
                (red_blocks as f64 / blocks_accepted as f64) * 100.0
            } else {
                0.0
            },
            avg_template_generation_ms: if templates_generated > 0 {
                (template_time as f64 / templates_generated as f64) / 1000.0
            } else {
                0.0
            },
            avg_ghostdag_calculation_ms: if ghostdag_calculations > 0 {
                (ghostdag_time as f64 / ghostdag_calculations as f64) / 1000.0
            } else {
                0.0
            },
            avg_tx_selection_ms: if tx_selections > 0 {
                (tx_selection_time as f64 / tx_selections as f64) / 1000.0
            } else {
                0.0
            },
            avg_txs_per_block: if tx_selections > 0 {
                txs_selected as f64 / tx_selections as f64
            } else {
                0.0
            },
            cache_hit_rate: if cache_hits + cache_misses > 0 {
                (cache_hits as f64 / (cache_hits + cache_misses) as f64) * 100.0
            } else {
                0.0
            },
            total_cache_requests: cache_hits + cache_misses,
        }
    }

    /// Reset all statistics
    pub async fn reset(&self) {
        self.blocks_found.store(0, Ordering::Relaxed);
        self.blocks_accepted.store(0, Ordering::Relaxed);
        self.blocks_rejected.store(0, Ordering::Relaxed);
        self.blue_blocks.store(0, Ordering::Relaxed);
        self.red_blocks.store(0, Ordering::Relaxed);
        self.template_generation_time_us.store(0, Ordering::Relaxed);
        self.templates_generated.store(0, Ordering::Relaxed);
        self.ghostdag_calculation_time_us
            .store(0, Ordering::Relaxed);
        self.ghostdag_calculations.store(0, Ordering::Relaxed);
        self.tx_selection_time_us.store(0, Ordering::Relaxed);
        self.tx_selections.store(0, Ordering::Relaxed);
        self.txs_selected.store(0, Ordering::Relaxed);
        self.ghostdag_cache_hits.store(0, Ordering::Relaxed);
        self.ghostdag_cache_misses.store(0, Ordering::Relaxed);

        let mut tracker = self.recent_blocks.write().await;
        tracker.blocks.clear();
    }
}

/// Snapshot of mining statistics at a point in time
#[derive(Debug, Clone)]
pub struct MiningStatsSnapshot {
    /// Uptime since stats tracking started
    pub uptime: Duration,

    /// Total blocks found
    pub blocks_found: u64,

    /// Total blocks accepted
    pub blocks_accepted: u64,

    /// Total blocks rejected
    pub blocks_rejected: u64,

    /// Total blue blocks
    pub blue_blocks: u64,

    /// Total red blocks
    pub red_blocks: u64,

    /// Block acceptance rate (percentage)
    pub acceptance_rate: f64,

    /// Blue block rate (percentage of accepted blocks)
    pub blue_rate: f64,

    /// Red block rate (percentage of accepted blocks)
    pub red_rate: f64,

    /// Average template generation time (milliseconds)
    pub avg_template_generation_ms: f64,

    /// Average GHOSTDAG calculation time (milliseconds)
    pub avg_ghostdag_calculation_ms: f64,

    /// Average transaction selection time (milliseconds)
    pub avg_tx_selection_ms: f64,

    /// Average transactions per block
    pub avg_txs_per_block: f64,

    /// Cache hit rate (percentage)
    pub cache_hit_rate: f64,

    /// Total cache requests (hits + misses)
    pub total_cache_requests: u64,
}

impl std::fmt::Display for MiningStatsSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Mining Statistics:")?;
        writeln!(f, "  Uptime: {:?}", self.uptime)?;
        writeln!(f, "  Blocks Found: {}", self.blocks_found)?;
        writeln!(
            f,
            "  Blocks Accepted: {} ({:.2}%)",
            self.blocks_accepted, self.acceptance_rate
        )?;
        writeln!(f, "  Blocks Rejected: {}", self.blocks_rejected)?;
        writeln!(
            f,
            "  Blue Blocks: {} ({:.2}%)",
            self.blue_blocks, self.blue_rate
        )?;
        writeln!(
            f,
            "  Red Blocks: {} ({:.2}%)",
            self.red_blocks, self.red_rate
        )?;
        writeln!(f, "Performance:")?;
        writeln!(
            f,
            "  Avg Template Generation: {:.2}ms",
            self.avg_template_generation_ms
        )?;
        writeln!(
            f,
            "  Avg GHOSTDAG Calculation: {:.2}ms",
            self.avg_ghostdag_calculation_ms
        )?;
        writeln!(f, "  Avg TX Selection: {:.2}ms", self.avg_tx_selection_ms)?;
        writeln!(f, "  Avg TXs per Block: {:.1}", self.avg_txs_per_block)?;
        writeln!(f, "Cache Performance:")?;
        writeln!(
            f,
            "  Hit Rate: {:.2}% ({} requests)",
            self.cache_hit_rate, self.total_cache_requests
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tos_common::tokio;

    #[tokio::test]
    async fn test_mining_stats_basic() {
        let stats = MiningStats::new(100);

        // Record some block submissions
        let hash1 = Hash::new([1u8; 32]);
        let hash2 = Hash::new([2u8; 32]);

        stats.record_block_found(hash1.clone()).await;
        stats.record_block_found(hash2.clone()).await;

        // Accept one as blue, one as red
        stats.record_block_accepted(&hash1, true).await;
        stats.record_block_accepted(&hash2, false).await;

        let snapshot = stats.get_snapshot().await;

        assert_eq!(snapshot.blocks_found, 2);
        assert_eq!(snapshot.blocks_accepted, 2);
        assert_eq!(snapshot.blocks_rejected, 0);
        assert_eq!(snapshot.blue_blocks, 1);
        assert_eq!(snapshot.red_blocks, 1);
        assert_eq!(snapshot.acceptance_rate, 100.0);
        assert_eq!(snapshot.blue_rate, 50.0);
        assert_eq!(snapshot.red_rate, 50.0);
    }

    #[tokio::test]
    async fn test_mining_stats_rejection() {
        let stats = MiningStats::new(100);

        let hash1 = Hash::new([1u8; 32]);
        let hash2 = Hash::new([2u8; 32]);

        stats.record_block_found(hash1.clone()).await;
        stats.record_block_found(hash2.clone()).await;

        stats.record_block_accepted(&hash1, true).await;
        stats.record_block_rejected(&hash2).await;

        let snapshot = stats.get_snapshot().await;

        assert_eq!(snapshot.blocks_found, 2);
        assert_eq!(snapshot.blocks_accepted, 1);
        assert_eq!(snapshot.blocks_rejected, 1);
        assert_eq!(snapshot.acceptance_rate, 50.0);
    }

    #[tokio::test]
    async fn test_performance_metrics() {
        let stats = MiningStats::new(100);

        // Record some performance metrics
        stats.record_template_generation(Duration::from_millis(10));
        stats.record_template_generation(Duration::from_millis(20));

        stats.record_ghostdag_calculation(Duration::from_micros(500));
        stats.record_ghostdag_calculation(Duration::from_micros(1500));

        stats.record_tx_selection(Duration::from_millis(5), 100);
        stats.record_tx_selection(Duration::from_millis(15), 200);

        let snapshot = stats.get_snapshot().await;

        assert_eq!(snapshot.avg_template_generation_ms, 15.0);
        assert_eq!(snapshot.avg_ghostdag_calculation_ms, 1.0);
        assert_eq!(snapshot.avg_tx_selection_ms, 10.0);
        assert_eq!(snapshot.avg_txs_per_block, 150.0);
    }

    #[tokio::test]
    async fn test_cache_metrics() {
        let stats = MiningStats::new(100);

        // Record cache hits and misses
        stats.record_cache_hit();
        stats.record_cache_hit();
        stats.record_cache_hit();
        stats.record_cache_miss();

        let snapshot = stats.get_snapshot().await;

        assert_eq!(snapshot.cache_hit_rate, 75.0);
        assert_eq!(snapshot.total_cache_requests, 4);
    }

    #[tokio::test]
    async fn test_reset() {
        let stats = MiningStats::new(100);

        let hash = Hash::new([1u8; 32]);
        stats.record_block_found(hash.clone()).await;
        stats.record_block_accepted(&hash, true).await;

        stats.reset().await;

        let snapshot = stats.get_snapshot().await;
        assert_eq!(snapshot.blocks_found, 0);
        assert_eq!(snapshot.blocks_accepted, 0);
    }
}
