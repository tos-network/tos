use log::{trace, warn};
/// Storage lifecycle management with reference counting
///
/// This module provides lifetime guards similar to Kaspa's DbLifetime pattern
/// to ensure clean shutdown and prevent resource leaks in TOS storage backends.
use std::sync::Weak;
use tempdir::TempDir;

/// Storage lifetime guard with reference counting
#[derive(Default)]
pub struct StorageLifetime<T> {
    /// Weak reference to the storage instance for reference tracking
    weak_storage_ref: Weak<T>,
    /// Optional temporary directory (deleted on drop if present)
    /// The field is intentionally unused - cleanup happens via Drop trait
    #[allow(dead_code)]
    optional_tempdir: Option<TempDir>,
}

impl<T> StorageLifetime<T> {
    /// Create a new lifetime guard with temporary directory management
    pub fn new(tempdir: TempDir, weak_storage_ref: Weak<T>) -> Self {
        Self {
            optional_tempdir: Some(tempdir),
            weak_storage_ref,
        }
    }

    /// Create a lifetime guard without automatic cleanup
    pub fn without_destroy(weak_storage_ref: Weak<T>) -> Self {
        Self {
            optional_tempdir: None,
            weak_storage_ref,
        }
    }

    /// Get the current count of strong references to the storage
    pub fn strong_count(&self) -> usize {
        self.weak_storage_ref.strong_count()
    }
}

impl<T> Drop for StorageLifetime<T> {
    fn drop(&mut self) {
        if log::log_enabled!(log::Level::Trace) {
            trace!("StorageLifetime dropping, checking for outstanding references");
        }

        // Wait for up to 16 seconds for all strong references to be released
        for i in 0..16 {
            let count = self.weak_storage_ref.strong_count();
            if count > 0 {
                if log::log_enabled!(log::Level::Warn) {
                    warn!(
                        "Storage has {} strong reference(s), waiting 1 second (attempt {}/16)",
                        count,
                        i + 1
                    );
                }
                std::thread::sleep(std::time::Duration::from_millis(1000));
            } else {
                break;
            }
        }

        let final_count = self.weak_storage_ref.strong_count();
        assert_eq!(
            final_count, 0,
            "Storage is expected to have no strong references when lifetime is dropped, but has {}",
            final_count
        );

        if log::log_enabled!(log::Level::Trace) {
            trace!("All storage references released, proceeding with cleanup");
        }
    }
}

/// Get a TOS-specific temporary directory for testing
pub fn get_tos_tempdir() -> TempDir {
    #[allow(clippy::expect_used)]
    {TempDir::new("tos-storage").expect("Failed to create temporary directory")}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, RwLock};

    #[test]
    fn test_lifetime_guard_waits_for_references() {
        let value = Arc::new(RwLock::new(42));
        let weak_ref = Arc::downgrade(&value);
        let lifetime = StorageLifetime::without_destroy(weak_ref);
        assert_eq!(lifetime.strong_count(), 1);
        drop(value);
        assert_eq!(lifetime.strong_count(), 0);
        drop(lifetime);
    }

    #[test]
    fn test_get_tos_tempdir() {
        let tempdir = get_tos_tempdir();
        let path = tempdir.path();
        assert!(path.exists());
        assert!(path.is_dir());
        assert!(path.to_str().unwrap().contains("tos-storage"));
    }
}
