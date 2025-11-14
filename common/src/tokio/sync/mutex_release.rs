//! Production Mutex re-exports (zero-overhead, no debug assertions)
//!
//! This module is used in default production builds and directly re-exports
//! tokio's async Mutex without any wrapper or .expect() calls.
//!
//! For debug builds with deadlock detection, use the `deadlock-detection` feature flag.

pub use tokio::sync::{Mutex, MutexGuard};
