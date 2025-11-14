//! Debug-mode Mutex wrapper with deadlock detection diagnostics
//!
//! **Note**: This module is only compiled when the `deadlock-detection` feature is enabled.
//! In production builds, the parent module re-exports `tokio::sync::Mutex` directly.
//!
//! ## .expect() Usage Documentation
//!
//! The `.expect()` calls in this module are acceptable because:
//!
//! 1. **Debug-only compilation**: This module is NOT compiled in production builds (only with `deadlock-detection` feature)
//! 2. **Unreachable in practice**: Tokio's async Mutex does not poison (unlike `std::sync::Mutex`)
//! 3. **Development diagnostics**: The assertions help catch logic errors and deadlocks during development
//! 4. **Internal invariant tracking**: Only used for tracking lock acquisition state, not user data
//!
//! ## Feature Flag
//!
//! ```bash
//! # Enable this module:
//! cargo build --features tos_common/deadlock-detection
//! ```
//!
//! See TOS_SECURITY_AUDIT_v5.md Section 5.1 for audit approval.

// Debugging tool for deadlock detection - uses .expect() for internal state tracking
// These expects are safe as they only track the deadlock detection state itself
#![allow(clippy::disallowed_methods)]
#![allow(clippy::expect_used)]

use log::{debug, error, log, Level};
use std::{
    future::Future,
    ops::{Deref, DerefMut},
    panic::Location,
    sync::Mutex as StdMutex,
    time::Duration,
};
use tokio::{
    pin,
    sync::{Mutex as InnerMutex, MutexGuard},
    time::interval,
};

pub struct Mutex<T: ?Sized> {
    init_location: &'static Location<'static>,
    last_location: StdMutex<Option<&'static Location<'static>>>,
    inner: InnerMutex<T>,
}

impl<T: ?Sized> Mutex<T> {
    #[track_caller]
    pub fn new(t: T) -> Self
    where
        T: Sized,
    {
        Self {
            init_location: Location::caller(),
            last_location: StdMutex::new(None),
            inner: InnerMutex::new(t),
        }
    }

    #[track_caller]
    pub fn lock(&self) -> impl Future<Output = MutexGuard<'_, T>> {
        let location = Location::caller();
        if log::log_enabled!(log::Level::Debug) {
            debug!("Mutex at {} locking at {}", self.init_location, location);
        }

        async move {
            let mut interval = interval(Duration::from_secs(10));
            // First tick is instant
            interval.tick().await;

            let future = self.inner.lock();
            pin!(future);

            let mut show = true;
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if show {
                            show = false;
                            let last = self.last_location.lock().expect("last lock location");
                            let mut msg = format!("Mutex at {} failed locking at {}.", self.init_location, location);
                            if let Some(last) = *last {
                                msg.push_str(&format!("\n- Last successful lock at: {last}"));
                            };

                            if log::log_enabled!(log::Level::Error) {
                                error!("{msg}");
                            }
                        }
                    }
                    guard = &mut future => {
                        let level = if !show {
                            Level::Warn
                        } else {
                            Level::Debug
                        };
                        log!(level, "Mutex {} write guard acquired at {}", self.init_location, location);
                        *self.last_location.lock().expect("last lock location") = Some(location);
                        return guard;
                    }
                }
            }
        }
    }
}

impl<T: ?Sized> AsRef<InnerMutex<T>> for Mutex<T> {
    fn as_ref(&self) -> &InnerMutex<T> {
        &self.inner
    }
}

impl<T: ?Sized> Deref for Mutex<T> {
    type Target = InnerMutex<T>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T: ?Sized> DerefMut for Mutex<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<T: ?Sized> std::fmt::Debug for Mutex<T>
where
    T: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mutex() {
        let mutex = Mutex::new(42);
        let guard = mutex.lock().await;
        {
            let location = mutex.last_location.lock().unwrap();
            assert!(location.is_some());
        }
        assert_eq!(*guard, 42);
    }
}
