// TOS Mining Module
// Optimized mining support with caching, statistics, and Stratum compatibility

pub mod cache;
pub mod stats;
pub mod stratum;
pub mod template;

pub use cache::{BlockTemplateCache, GhostdagCache, TipSelectionCache, TransactionCache};
pub use stats::{BlockStatus, MiningStats, MiningStatsSnapshot};
pub use stratum::{
    block_header_to_stratum_job, create_share_error, create_share_success,
    create_stratum_notification, validate_stratum_share, StratumError, StratumJob,
    StratumNotification, StratumShare, StratumShareResponse,
};
pub use template::{BlockTemplateGenerator, CacheStats, OptimizedTxSelector};
