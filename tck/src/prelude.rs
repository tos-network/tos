//! Prelude module for convenient imports
//!
//! Import everything you need with:
//! ```rust,ignore
//! use tos_tck::prelude::*;
//! ```

// Re-export orchestrator types
pub use crate::orchestrator::{Clock, DeterministicTestEnv, PausedClock, SystemClock, TestRng};

// Re-export Tier 1 component testing
pub use crate::tier1_component::{TestBlockchain, TestBlockchainBuilder};

// Re-export Tier 2 integration testing
// TODO: Uncomment when TestDaemon is implemented
// pub use crate::tier2_integration::{TestDaemon, TestDaemonBuilder};

// Re-export Tier 3 E2E testing
// TODO: Uncomment when LocalTosNetwork is implemented
// pub use crate::tier3_e2e::{LocalTosNetwork, NetworkBuilder};

// Re-export chaos testing if feature enabled
// TODO: Uncomment when FaultInjector and TimeSkew are implemented
// #[cfg(feature = "chaos")]
// pub use crate::tier4_chaos::{FaultInjector, TimeSkew};

// Re-export utilities
pub use crate::utilities::{create_temp_rocksdb, TempRocksDB};

// Re-export waiter primitives
pub use crate::tier2_integration::waiters::wait_for_block;

pub use crate::tier3_e2e::waiters::{wait_all_heights_equal, wait_all_tips_equal};

// Re-export common invariants
pub use crate::invariants::{
    check_balance_conservation, check_nonce_monotonicity, check_state_equivalence,
};

// Re-export commonly used external types
pub use anyhow::{anyhow, Context, Result};
pub use std::sync::Arc;
pub use tokio::time::Duration;
