//! Tokio synchronization primitives with optional debug wrappers
//!
//! ## Feature Flags
//!
//! - **Default** (production): Direct re-exports from tokio::sync (zero overhead, zero .expect())
//! - **`deadlock-detection`**: Debug wrappers with additional assertions and deadlock diagnostics
//!
//! ## Usage
//!
//! ```bash
//! # Production build (default)
//! cargo build --release
//!
//! # Debug build with deadlock detection
//! cargo build --features tos_common/deadlock-detection
//! ```

// Base tokio sync primitives (for non-wrapped types)
#[cfg(all(
    feature = "tokio",
    target_arch = "wasm32",
    target_vendor = "unknown",
    target_os = "unknown"
))]
pub use tokio_with_wasm::sync::*;

#[cfg(all(
    feature = "tokio",
    not(all(
        target_arch = "wasm32",
        target_vendor = "unknown",
        target_os = "unknown"
    ))
))]
pub use tokio::sync::*;

// Debug-mode wrappers (with .expect() calls for diagnostics)
#[cfg(any(test, feature = "deadlock-detection"))]
mod rwlock;
#[cfg(feature = "deadlock-detection")]
pub use rwlock::RwLock;

#[cfg(any(test, feature = "deadlock-detection"))]
mod mutex;
#[cfg(feature = "deadlock-detection")]
pub use mutex::Mutex;

// Production re-exports (direct tokio primitives, zero overhead)
#[cfg(all(feature = "tokio", not(feature = "deadlock-detection")))]
mod rwlock_release;
#[cfg(all(feature = "tokio", not(feature = "deadlock-detection")))]
pub use rwlock_release::{RwLock, RwLockReadGuard, RwLockWriteGuard};

#[cfg(all(feature = "tokio", not(feature = "deadlock-detection")))]
mod mutex_release;
#[cfg(all(feature = "tokio", not(feature = "deadlock-detection")))]
pub use mutex_release::{Mutex, MutexGuard};
