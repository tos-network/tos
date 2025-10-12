// TOS Mining Module
// Optimized mining support with caching, statistics, and Stratum compatibility

pub mod cache;
pub mod stats;
pub mod stratum;
pub mod template;

pub use cache::{GhostdagCache, BlockTemplateCache, TipSelectionCache};
pub use stats::{MiningStats, MiningStatsSnapshot, BlockStatus};
pub use stratum::{
    StratumJob, StratumNotification, StratumShare, StratumShareResponse, StratumError,
    block_header_to_stratum_job, create_stratum_notification, validate_stratum_share,
    create_share_success, create_share_error,
};
pub use template::{BlockTemplateGenerator, OptimizedTxSelector, CacheStats};
