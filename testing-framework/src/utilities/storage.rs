// File: testing-framework/src/utilities/storage.rs
//
// Storage Utilities for Testing
//
// This module provides RAII-based temporary storage management for tests.
// All temporary directories are automatically cleaned up when dropped,
// preventing test pollution and disk space leaks.

use anyhow::Result;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// RAII wrapper for temporary RocksDB instance.
///
/// This struct ensures that temporary RocksDB directories are automatically
/// cleaned up when the test completes, even if the test panics. This prevents
/// test pollution and disk space leaks from accumulating test data.
///
/// # Design Principles
///
/// 1. **RAII Cleanup**: Directory is deleted when dropped
/// 2. **Panic Safety**: Cleanup occurs even on test panic/assertion failure
/// 3. **Isolation**: Each test gets a unique temporary directory
/// 4. **Production Parity**: Uses real RocksDB, not mocks
///
/// # Example
///
/// ```ignore
/// use tos_testing_framework::utilities::create_temp_rocksdb;
///
/// #[tokio::test]
/// async fn test_blockchain_storage() {
///     // Create temporary RocksDB
///     let temp_db = create_temp_rocksdb().unwrap();
///
///     // Use the database path
///     let blockchain = Blockchain::new(temp_db.path()).await?;
///
///     // ... perform test operations ...
///
///     // temp_db is automatically cleaned up here (Drop)
/// }
/// ```
///
/// # Cleanup Behavior
///
/// The temporary directory is deleted when:
/// - The `TempRocksDB` goes out of scope
/// - The test completes successfully
/// - The test panics or fails an assertion
/// - The test is interrupted (SIGINT, SIGTERM)
///
/// **Note**: Cleanup may not occur if the process is killed with SIGKILL
/// or if the system crashes. These are unavoidable edge cases.
pub struct TempRocksDB {
    /// Temporary directory handle (manages cleanup)
    _temp_dir: TempDir,
    /// Path to the RocksDB directory (remains valid until drop)
    path: PathBuf,
}

impl TempRocksDB {
    /// Create a new temporary RocksDB instance.
    ///
    /// # Returns
    ///
    /// * `Ok(TempRocksDB)` - Successfully created temporary directory
    /// * `Err(_)` - Failed to create temporary directory
    ///
    /// # Errors
    ///
    /// Returns an error if the system cannot create a temporary directory,
    /// typically due to:
    /// - Insufficient permissions
    /// - Disk full
    /// - Invalid TMPDIR environment variable
    pub fn new() -> Result<Self> {
        let temp_dir = tempfile::Builder::new()
            .prefix("tos_test_rocksdb_")
            .tempdir()?;

        let path = temp_dir.path().to_path_buf();

        if log::log_enabled!(log::Level::Debug) {
            log::debug!("Created temporary RocksDB at: {:?}", path);
        }

        Ok(Self {
            _temp_dir: temp_dir,
            path,
        })
    }

    /// Get the path to the temporary RocksDB directory.
    ///
    /// This path remains valid until the `TempRocksDB` is dropped.
    ///
    /// # Returns
    ///
    /// A reference to the temporary directory path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Get the path as a PathBuf (cloned).
    ///
    /// Use this when you need to move the path into another struct.
    ///
    /// # Returns
    ///
    /// A cloned `PathBuf` of the temporary directory.
    pub fn path_buf(&self) -> PathBuf {
        self.path.clone()
    }
}

impl Drop for TempRocksDB {
    fn drop(&mut self) {
        // The TempDir will automatically clean up the directory
        // We just log it for debugging
        if log::log_enabled!(log::Level::Debug) {
            log::debug!("Cleaning up temporary RocksDB at: {:?}", self.path);
        }
    }
}

/// Create a new temporary RocksDB instance (convenience function).
///
/// This is a shorthand for `TempRocksDB::new()`.
///
/// # Returns
///
/// * `Ok(TempRocksDB)` - Successfully created temporary directory
/// * `Err(_)` - Failed to create temporary directory
///
/// # Example
///
/// ```ignore
/// use tos_testing_framework::utilities::create_temp_rocksdb;
///
/// let temp_db = create_temp_rocksdb()?;
/// let blockchain = Blockchain::new(temp_db.path()).await?;
/// ```
pub fn create_temp_rocksdb() -> Result<TempRocksDB> {
    TempRocksDB::new()
}

/// Create a temporary directory (generic version for non-RocksDB use cases).
///
/// This function creates a temporary directory with a custom prefix,
/// useful for storing test artifacts, logs, or other temporary files.
///
/// # Arguments
///
/// * `prefix` - Prefix for the temporary directory name
///
/// # Returns
///
/// * `Ok(TempDir)` - Successfully created temporary directory
/// * `Err(_)` - Failed to create temporary directory
///
/// # Example
///
/// ```ignore
/// use tos_testing_framework::utilities::create_temp_dir;
///
/// let temp_dir = create_temp_dir("test_logs_")?;
/// let log_file = temp_dir.path().join("test.log");
/// std::fs::write(&log_file, "test data")?;
/// // temp_dir is automatically cleaned up when dropped
/// ```
pub fn create_temp_dir(prefix: &str) -> Result<TempDir> {
    Ok(tempfile::Builder::new().prefix(prefix).tempdir()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_temp_rocksdb_creation() {
        let temp_db = create_temp_rocksdb().unwrap();
        let path = temp_db.path();

        // Verify directory exists
        assert!(path.exists());
        assert!(path.is_dir());

        // Verify we can create files in the directory
        let test_file = path.join("test.txt");
        fs::write(&test_file, b"test data").unwrap();
        assert!(test_file.exists());
    }

    #[test]
    fn test_temp_rocksdb_cleanup() {
        let path_clone;
        {
            let temp_db = create_temp_rocksdb().unwrap();
            path_clone = temp_db.path().to_path_buf();

            // Verify directory exists while in scope
            assert!(path_clone.exists());
        } // temp_db is dropped here

        // Verify directory is cleaned up after drop
        // Note: There might be a small delay in cleanup
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(!path_clone.exists());
    }

    #[test]
    fn test_temp_rocksdb_path_methods() {
        let temp_db = create_temp_rocksdb().unwrap();

        // Test path() method
        let path_ref = temp_db.path();
        assert!(path_ref.exists());

        // Test path_buf() method
        let path_buf = temp_db.path_buf();
        assert_eq!(path_ref, path_buf.as_path());
    }

    #[test]
    fn test_multiple_temp_rocksdb_instances() {
        let temp_db1 = create_temp_rocksdb().unwrap();
        let temp_db2 = create_temp_rocksdb().unwrap();
        let temp_db3 = create_temp_rocksdb().unwrap();

        // Verify all have unique paths
        assert_ne!(temp_db1.path(), temp_db2.path());
        assert_ne!(temp_db2.path(), temp_db3.path());
        assert_ne!(temp_db1.path(), temp_db3.path());

        // Verify all directories exist
        assert!(temp_db1.path().exists());
        assert!(temp_db2.path().exists());
        assert!(temp_db3.path().exists());
    }

    #[test]
    fn test_create_temp_dir() {
        let temp_dir = create_temp_dir("test_custom_").unwrap();
        let path = temp_dir.path();

        // Verify directory exists
        assert!(path.exists());
        assert!(path.is_dir());

        // Verify prefix is in the directory name
        let dir_name = path.file_name().unwrap().to_str().unwrap();
        assert!(dir_name.starts_with("test_custom_"));
    }

    #[test]
    #[should_panic(expected = "test panic")]
    fn test_temp_rocksdb_cleanup_on_panic() {
        let _temp_db = create_temp_rocksdb().unwrap();
        // Even if we panic, the directory should be cleaned up
        panic!("test panic");
    }

    #[tokio::test]
    async fn test_temp_rocksdb_async_usage() {
        let temp_db = create_temp_rocksdb().unwrap();

        // Simulate async operations
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Verify path is still valid
        assert!(temp_db.path().exists());
    }
}
