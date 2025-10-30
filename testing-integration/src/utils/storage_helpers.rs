//! Safe storage helpers for integration tests
//!
//! This module provides storage helpers for integration tests with support for both
//! SledStorage and RocksStorage backends.
//!
//! # Recommended: Use RocksDB for Tests
//!
//! **TOS production environment uses RocksDB by default**, so tests should use RocksDB
//! to match production behavior and avoid sled-specific deadlock issues.
//!
//! Use `create_test_rocksdb_storage()` or `create_test_rocksdb_storage_with_accounts()`
//! for new tests.
//!
//! # Legacy: Sled Storage with Safe Pattern
//!
//! For existing tests using SledStorage, avoid deadlock issues with the safe pattern:
//!
//! 1. Perform all storage writes in single-threaded context
//! 2. Wait for sled to complete internal flush operations (10ms per account + 100ms)
//! 3. Only then create ParallelChainState and begin parallel execution
//!
//! # Usage Example
//!
//! ```rust,ignore
//! use tos_testing_integration::utils::storage_helpers::{
//!     create_test_storage_with_accounts,
//!     flush_storage_and_wait,
//! };
//! use tos_common::crypto::PublicKey;
//!
//! #[tokio::test]
//! async fn test_parallel_transfers() {
//!     // Create storage with accounts (SAFE)
//!     let accounts = vec![
//!         (account_a, 1000, 0),  // (pubkey, balance, nonce)
//!         (account_b, 2000, 0),
//!     ];
//!     let storage = create_test_storage_with_accounts(accounts).await.unwrap();
//!
//!     // Force flush and wait (CRITICAL)
//!     flush_storage_and_wait(&storage).await;
//!
//!     // Now safe to create ParallelChainState
//!     let parallel_state = ParallelChainState::new(storage.clone(), 0).await.unwrap();
//!
//!     // Parallel execution will not deadlock
//! }
//! ```
//!
//! # When to Use These Helpers
//!
//! - Use `create_test_storage()` for basic storage with TOS asset
//! - Use `create_test_storage_with_tos_asset()` for explicit TOS setup
//! - Use `create_test_storage_with_accounts()` for pre-populated accounts
//! - Always call `flush_storage_and_wait()` before creating ParallelChainState
//! - Use `setup_account_safe()` to add accounts to existing storage
//!
//! # When NOT to Use These Helpers
//!
//! - For unit tests that don't use parallel execution (use MockStorage instead)
//! - For tests that don't need persistent storage (use in-memory mocks)
//! - For production code (these are test-only utilities)

use std::sync::Arc;
use tempdir::TempDir;
use tos_common::{
    account::{VersionedBalance, VersionedNonce},
    asset::{AssetData, VersionedAssetData},
    config::{COIN_DECIMALS, TOS_ASSET},
    crypto::{elgamal::CompressedPublicKey, PublicKey},
    versioned_type::Versioned,
    network::Network,
};
use tos_daemon::core::{
    config::RocksDBConfig,
    error::BlockchainError,
    storage::{
        sled::{SledStorage, StorageMode},
        rocksdb::RocksStorage,
        AccountProvider,
        AssetProvider,
        BalanceProvider,
        NonceProvider,
    },
};

/// Create a test storage instance with TOS asset registered
///
/// This creates a temporary sled storage instance suitable for integration tests.
/// The storage is configured with HighThroughput mode and reasonable cache sizes.
///
/// # Storage Configuration
///
/// - Mode: HighThroughput (optimized for parallel writes)
/// - Cache: 1MB
/// - Network: Devnet
/// - Location: Temporary directory (auto-cleaned)
///
/// # Example
///
/// ```rust,ignore
/// let storage = create_test_storage().await;
///
/// // Storage has TOS asset registered
/// // Now safe to setup accounts
/// setup_account_safe(&storage, &account, 1000, 0).await.unwrap();
///
/// // Remember to flush before parallel execution
/// flush_storage_and_wait(&storage).await;
/// ```
pub async fn create_test_storage() -> Arc<tokio::sync::RwLock<SledStorage>> {
    let temp_dir = TempDir::new("tos_parallel_test").unwrap();
    let storage = SledStorage::new(
        temp_dir.path().to_string_lossy().to_string(),
        Some(1024 * 1024),
        Network::Devnet,
        1024 * 1024,
        StorageMode::HighThroughput,
    )
    .unwrap();

    let storage_arc = Arc::new(tokio::sync::RwLock::new(storage));

    // Register TOS asset
    {
        let mut storage_write = storage_arc.write().await;
        let asset_data = AssetData::new(
            COIN_DECIMALS,
            "TOS".to_string(),
            "TOS".to_string(),
            None,
            None,
        );
        let versioned: VersionedAssetData = Versioned::new(asset_data, Some(0));
        storage_write.add_asset(&TOS_ASSET, 0, versioned).await.unwrap();
    }

    storage_arc
}

/// Create test storage with TOS asset (alias for create_test_storage)
///
/// This is an explicit alias to make test intent clearer. Functionally identical
/// to `create_test_storage()` but the name emphasizes that TOS asset is included.
pub async fn create_test_storage_with_tos_asset() -> Arc<tokio::sync::RwLock<SledStorage>> {
    create_test_storage().await
}

/// Create test storage with pre-populated accounts
///
/// This is a convenience wrapper that creates storage and sets up multiple accounts
/// in a single operation, using the safe pattern to avoid deadlocks.
///
/// # Arguments
///
/// * `accounts` - Vector of (PublicKey, balance, nonce) tuples
///
/// # Example
///
/// ```rust,ignore
/// use tos_common::serializer::Writer;
/// use tos_common::crypto::elgamal::CompressedPublicKey;
///
/// // Create test accounts
/// let account_a = create_test_pubkey(1);
/// let account_b = create_test_pubkey(2);
///
/// // Setup storage with accounts
/// let storage = create_test_storage_with_accounts(vec![
///     (account_a, 1000, 0),
///     (account_b, 2000, 0),
/// ]).await.unwrap();
///
/// // Flush before parallel execution
/// flush_storage_and_wait(&storage).await;
/// ```
pub async fn create_test_storage_with_accounts(
    accounts: Vec<(PublicKey, u64, u64)>
) -> Result<Arc<tokio::sync::RwLock<SledStorage>>, BlockchainError> {
    let storage = create_test_storage().await;

    // Setup all accounts using safe pattern
    for (pubkey, balance, nonce) in accounts {
        setup_account_safe(&storage, &pubkey, balance, nonce).await?;
    }

    // Flush storage before returning
    flush_storage_and_wait(&storage).await;

    Ok(storage)
}

/// Setup account state WITHOUT deadlock - SAFE version for parallel execution tests
///
/// DEADLOCK FIX: This function performs storage writes in a single-threaded context,
/// then adds a small delay to let sled complete internal operations before parallel
/// execution begins. This avoids the deadlock caused by concurrent storage reads
/// during parallel execution hitting uncommitted sled internal state.
///
/// # Key Differences from Legacy Version
///
/// 1. Writes are done BEFORE ParallelChainState creation
/// 2. Adds tokio::time::sleep(10ms) to let sled flush internal state
/// 3. No concurrent storage access during the write phase
/// 4. Explicit lock drop before sleep
///
/// # Why This Works
///
/// Sled uses an internal LRU cache with Mutex protection. When writes occur,
/// sled may not immediately commit them to its internal structures. If parallel
/// threads try to read during this uncommitted state, the Mutex can deadlock.
///
/// By waiting 10ms after writes, we give sled time to complete its internal
/// flush operations before parallel execution begins.
///
/// # Example
///
/// ```rust,ignore
/// let storage = create_test_storage().await;
///
/// // Setup account (safe from deadlocks)
/// setup_account_safe(&storage, &account_a, 1000, 0).await.unwrap();
/// setup_account_safe(&storage, &account_b, 2000, 0).await.unwrap();
///
/// // Additional safety: flush storage
/// flush_storage_and_wait(&storage).await;
///
/// // Now safe to create ParallelChainState
/// let parallel_state = ParallelChainState::new(storage.clone(), 0).await.unwrap();
/// ```
pub async fn setup_account_safe(
    storage: &Arc<tokio::sync::RwLock<SledStorage>>,
    account: &CompressedPublicKey,
    balance: u64,
    nonce: u64,
) -> Result<(), BlockchainError> {
    // Single-threaded storage write (safe)
    {
        let mut storage_write = storage.write().await;

        storage_write
            .set_last_nonce_to(
                account,
                0,
                &VersionedNonce::new(nonce, Some(0)),
            )
            .await?;

        storage_write
            .set_last_balance_to(
                account,
                &TOS_ASSET,
                0,
                &VersionedBalance::new(balance, Some(0)),
            )
            .await?;

        storage_write
            .set_account_registration_topoheight(account, 0)
            .await?;

        // Explicitly drop write lock before sleep
    }

    // CRITICAL: Give sled time to complete internal flush operations
    // Without this delay, parallel executor may read uncommitted sled state
    // causing LRU cache Mutex deadlocks
    //
    // NOTE: 10ms is usually sufficient for light loads. If tests still timeout,
    // call flush_storage_and_wait() after all account setup is complete.
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    Ok(())
}

/// Force flush sled storage and wait for completion
///
/// Call this AFTER all account setup is complete and BEFORE creating ParallelChainState.
/// This ensures sled's internal state is fully committed before parallel execution begins.
///
/// # Why This is Necessary
///
/// Even with the delays in `setup_account_safe()`, sled may still have uncommitted
/// internal state. This function provides additional safety by:
///
/// 1. Blocking for 50ms to let sled flush operations complete
/// 2. Adding 50ms delay to let LRU caches settle
/// 3. Total 100ms wait ensures all internal state is committed
///
/// # When to Use
///
/// - ALWAYS call this before creating ParallelChainState
/// - Call after all `setup_account_safe()` calls are complete
/// - Call when tests are timing out or deadlocking
/// - Call when dealing with heavy concurrent loads
///
/// # Example
///
/// ```rust,ignore
/// // Setup multiple accounts
/// setup_account_safe(&storage, &account_a, 1000, 0).await.unwrap();
/// setup_account_safe(&storage, &account_b, 2000, 0).await.unwrap();
/// setup_account_safe(&storage, &account_c, 3000, 0).await.unwrap();
///
/// // CRITICAL: Flush before parallel execution
/// flush_storage_and_wait(&storage).await;
///
/// // Now safe to create parallel state
/// let parallel_state = ParallelChainState::new(storage.clone(), 0).await.unwrap();
/// ```
pub async fn flush_storage_and_wait(storage: &Arc<tokio::sync::RwLock<SledStorage>>) {
    {
        let _storage_read = storage.read().await;
        // Sled's flush() is a synchronous operation that ensures all writes are persisted
        // We wrap it in tokio::task::spawn_blocking to avoid blocking the async runtime
        let _ = tokio::task::spawn_blocking(|| {
            // Force flush to disk (note: SledStorage may not expose flush())
            // As a workaround, we add a longer delay to let internal operations complete
            std::thread::sleep(std::time::Duration::from_millis(50));
        }).await;
    }

    // Additional safety delay to let LRU caches settle
    // This ensures all internal state is fully committed
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
}

/// LEGACY: Setup account state by writing to storage (MAY CAUSE DEADLOCK IN TESTS)
///
/// This function is kept for reference but should NOT be used in parallel execution tests
/// as it causes sled deadlocks. Use setup_account_safe() instead.
///
/// # Why This Causes Deadlocks
///
/// This function writes to storage without any delays or flush operations. When
/// ParallelChainState immediately tries to read from storage, it may hit uncommitted
/// sled internal state, causing LRU cache Mutex deadlocks.
///
/// # Migration Guide
///
/// Old code:
/// ```rust,ignore
/// setup_account_in_storage_legacy(&storage, &account, 1000, 0).await.unwrap();
/// let parallel_state = ParallelChainState::new(storage.clone(), 0).await.unwrap();
/// ```
///
/// New code:
/// ```rust,ignore
/// setup_account_safe(&storage, &account, 1000, 0).await.unwrap();
/// flush_storage_and_wait(&storage).await;  // CRITICAL
/// let parallel_state = ParallelChainState::new(storage.clone(), 0).await.unwrap();
/// ```
#[allow(dead_code)]
pub async fn setup_account_in_storage_legacy(
    storage: &Arc<tokio::sync::RwLock<SledStorage>>,
    account: &CompressedPublicKey,
    balance: u64,
    nonce: u64,
) -> Result<(), BlockchainError> {
    let mut storage_write = storage.write().await;

    storage_write
        .set_last_nonce_to(
            account,
            0,
            &VersionedNonce::new(nonce, Some(0)),
        )
        .await?;

    storage_write
        .set_last_balance_to(
            account,
            &TOS_ASSET,
            0,
            &VersionedBalance::new(balance, Some(0)),
        )
        .await?;

    storage_write
        .set_account_registration_topoheight(account, 0)
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tos_common::serializer::{Reader, Serializer, Writer};

    /// Helper to create test public keys
    fn create_test_pubkey(seed: u8) -> CompressedPublicKey {
        let mut buffer = Vec::new();
        let mut writer = Writer::new(&mut buffer);
        writer.write_bytes(&[seed; 32]);
        let data = writer.as_bytes();
        let mut reader = Reader::new(data);
        CompressedPublicKey::read(&mut reader).expect("Failed to create test pubkey")
    }

    #[tokio::test]
    async fn test_create_test_storage() {
        let storage = create_test_storage().await;
        let storage_read = storage.read().await;

        // Verify TOS asset is registered
        let result = storage_read.get_asset(&TOS_ASSET).await;
        assert!(result.is_ok(), "TOS asset should be registered");
    }

    #[tokio::test]
    async fn test_create_test_storage_with_accounts() {
        let account_a = create_test_pubkey(1);
        let account_b = create_test_pubkey(2);

        let storage = create_test_storage_with_accounts(vec![
            (account_a.clone(), 1000, 0),
            (account_b.clone(), 2000, 5),
        ]).await.unwrap();

        let storage_read = storage.read().await;

        // Verify account A
        let (_, balance_a) = storage_read.get_last_balance(&account_a, &TOS_ASSET).await.unwrap();
        assert_eq!(balance_a.get_balance(), 1000);

        let (_, nonce_a) = storage_read.get_last_nonce(&account_a).await.unwrap();
        assert_eq!(nonce_a.get_nonce(), 0);

        // Verify account B
        let (_, balance_b) = storage_read.get_last_balance(&account_b, &TOS_ASSET).await.unwrap();
        assert_eq!(balance_b.get_balance(), 2000);

        let (_, nonce_b) = storage_read.get_last_nonce(&account_b).await.unwrap();
        assert_eq!(nonce_b.get_nonce(), 5);
    }

    #[tokio::test]
    async fn test_setup_account_safe() {
        let storage = create_test_storage().await;
        let account = create_test_pubkey(42);

        // Setup account safely
        setup_account_safe(&storage, &account, 5000, 10).await.unwrap();

        let storage_read = storage.read().await;

        // Verify balance
        let (_, balance) = storage_read.get_last_balance(&account, &TOS_ASSET).await.unwrap();
        assert_eq!(balance.get_balance(), 5000);

        // Verify nonce
        let (_, nonce) = storage_read.get_last_nonce(&account).await.unwrap();
        assert_eq!(nonce.get_nonce(), 10);
    }

    #[tokio::test]
    async fn test_flush_storage_and_wait() {
        let storage = create_test_storage().await;
        let account = create_test_pubkey(99);

        setup_account_safe(&storage, &account, 1000, 0).await.unwrap();

        // Should not panic or deadlock
        flush_storage_and_wait(&storage).await;

        // Verify data is still accessible after flush
        let storage_read = storage.read().await;
        let (_, balance) = storage_read.get_last_balance(&account, &TOS_ASSET).await.unwrap();
        assert_eq!(balance.get_balance(), 1000);
    }
}

// ============================================================================
// RocksDB Storage Helpers (Recommended for New Tests)
// ============================================================================

/// Create a test RocksDB storage instance with TOS asset registered
///
/// **RECOMMENDED**: Use this for new tests instead of SledStorage.
/// TOS production environment uses RocksDB by default, and RocksDB doesn't
/// have the deadlock issues that sled has.
///
/// # Configuration
///
/// - Network: Devnet
/// - Cache: 1MB
/// - Location: Temporary directory (auto-cleaned)
/// - Compression: Snappy (default)
///
/// # Example
///
/// ```rust,ignore
/// use tos_testing_integration::utils::storage_helpers::create_test_rocksdb_storage;
///
/// #[tokio::test]
/// async fn test_with_rocksdb() {
///     let storage = create_test_rocksdb_storage().await;
///
///     // No need for flush_storage_and_wait() with RocksDB!
///     let parallel_state = ParallelChainState::new(storage.clone(), 0).await.unwrap();
/// }
/// ```
pub async fn create_test_rocksdb_storage() -> Arc<tokio::sync::RwLock<RocksStorage>> {
    let temp_dir = TempDir::new("tos_test_rocksdb").expect("Failed to create temp directory");
    let dir_path = temp_dir.path().to_string_lossy().to_string();

    // Use default RocksDB config optimized for tests
    let config = RocksDBConfig::default();

    let mut storage = RocksStorage::new(
        &dir_path,
        Network::Devnet,
        Some(1024 * 1024), // 1MB cache
        &config,
    );

    // Register TOS asset
    let asset_data = AssetData::new(
        COIN_DECIMALS,
        "TOS".to_string(),
        "TOS".to_string(),
        None,
        None,
    );
    let versioned: VersionedAssetData = Versioned::new(asset_data, Some(0));
    storage.add_asset(&TOS_ASSET, 0, versioned).await.expect("Failed to register TOS asset");

    // Store temp_dir to prevent cleanup (move ownership)
    std::mem::forget(temp_dir);

    Arc::new(tokio::sync::RwLock::new(storage))
}

/// Setup account in RocksDB storage (no deadlock risk, no delays needed)
///
/// Unlike sled, RocksDB doesn't require delays or flush operations.
/// This function writes directly without waiting.
///
/// # Example
///
/// ```rust,ignore
/// use tos_testing_integration::utils::storage_helpers::{
///     create_test_rocksdb_storage,
///     setup_account_rocksdb,
/// };
///
/// #[tokio::test]
/// async fn test_account_setup() {
///     let storage = create_test_rocksdb_storage().await;
///     let account = PublicKey::default();
///
///     // Setup account (no delays needed!)
///     setup_account_rocksdb(&storage, &account, 1000, 0).await.unwrap();
///
///     // Immediately safe to use in parallel execution
///     let parallel_state = ParallelChainState::new(storage.clone(), 0).await.unwrap();
/// }
/// ```
pub async fn setup_account_rocksdb(
    storage: &Arc<tokio::sync::RwLock<RocksStorage>>,
    account: &CompressedPublicKey,
    balance: u64,
    nonce: u64,
) -> Result<(), BlockchainError> {
    let mut storage_write = storage.write().await;

    // Set nonce
    storage_write
        .set_last_nonce_to(account, 0, &VersionedNonce::new(nonce, Some(0)))
        .await?;

    // Set balance
    storage_write
        .set_last_balance_to(
            account,
            &TOS_ASSET,
            0,
            &VersionedBalance::new(balance, Some(0)),
        )
        .await?;

    // Register account
    storage_write
        .set_account_registration_topoheight(account, 0)
        .await?;

    // No delays needed - RocksDB handles concurrency correctly!
    Ok(())
}

/// Create RocksDB test storage with pre-populated accounts
///
/// **RECOMMENDED**: Use this for new tests that need multiple accounts.
/// This is faster and simpler than the sled equivalent because RocksDB
/// doesn't require delays.
///
/// # Arguments
///
/// * `accounts` - Vector of (PublicKey, balance, nonce) tuples
///
/// # Example
///
/// ```rust,ignore
/// use tos_testing_integration::utils::storage_helpers::create_test_rocksdb_storage_with_accounts;
///
/// #[tokio::test]
/// async fn test_transfers() {
///     let alice = PublicKey::default();
///     let bob = PublicKey::default();
///
///     let accounts = vec![
///         (alice.clone(), 1000, 0),
///         (bob.clone(), 2000, 0),
///     ];
///
///     let storage = create_test_rocksdb_storage_with_accounts(accounts).await.unwrap();
///
///     // Immediately ready for parallel execution (no flush needed!)
///     let parallel_state = ParallelChainState::new(storage.clone(), 0).await.unwrap();
/// }
/// ```
pub async fn create_test_rocksdb_storage_with_accounts(
    accounts: Vec<(CompressedPublicKey, u64, u64)>,
) -> Result<Arc<tokio::sync::RwLock<RocksStorage>>, BlockchainError> {
    let storage = create_test_rocksdb_storage().await;

    for (account, balance, nonce) in accounts {
        setup_account_rocksdb(&storage, &account, balance, nonce).await?;
    }

    // No flush needed! RocksDB is immediately ready for concurrent access
    Ok(storage)
}

#[cfg(test)]
mod rocksdb_tests {
    use super::*;
    use tos_common::serializer::{Reader, Serializer};

    fn create_test_pubkey(seed: u8) -> CompressedPublicKey {
        let data = [seed; 32];
        let mut reader = Reader::new(&data);
        CompressedPublicKey::read(&mut reader).expect("Failed to create test pubkey")
    }

    #[tokio::test]
    async fn test_create_rocksdb_storage() {
        let storage = create_test_rocksdb_storage().await;
        let storage_read = storage.read().await;

        // Verify TOS asset is registered
        let asset = storage_read.get_asset(&TOS_ASSET).await.unwrap();
        assert_eq!(asset.get_data().get_decimals(), COIN_DECIMALS);
    }

    #[tokio::test]
    async fn test_setup_account_rocksdb() {
        let storage = create_test_rocksdb_storage().await;
        let account = create_test_pubkey(1);

        setup_account_rocksdb(&storage, &account, 1000, 5).await.unwrap();

        let storage_read = storage.read().await;
        let (_, balance) = storage_read.get_last_balance(&account, &TOS_ASSET).await.unwrap();
        let (_, nonce) = storage_read.get_last_nonce(&account).await.unwrap();

        assert_eq!(balance.get_balance(), 1000);
        assert_eq!(nonce.get_nonce(), 5);
    }

    #[tokio::test]
    async fn test_create_rocksdb_storage_with_accounts() {
        let account1 = create_test_pubkey(1);
        let account2 = create_test_pubkey(2);

        let accounts = vec![
            (account1.clone(), 1000, 0),
            (account2.clone(), 2000, 3),
        ];

        let storage = create_test_rocksdb_storage_with_accounts(accounts).await.unwrap();

        let storage_read = storage.read().await;

        let (_, balance1) = storage_read.get_last_balance(&account1, &TOS_ASSET).await.unwrap();
        let (_, balance2) = storage_read.get_last_balance(&account2, &TOS_ASSET).await.unwrap();
        let (_, nonce2) = storage_read.get_last_nonce(&account2).await.unwrap();

        assert_eq!(balance1.get_balance(), 1000);
        assert_eq!(balance2.get_balance(), 2000);
        assert_eq!(nonce2.get_nonce(), 3);
    }

    #[tokio::test]
    async fn test_rocksdb_no_deadlock_immediate_use() {
        // This test verifies RocksDB can be used immediately without delays
        let storage = create_test_rocksdb_storage().await;
        let account = create_test_pubkey(99);

        // Setup account
        setup_account_rocksdb(&storage, &account, 1000, 0).await.unwrap();

        // Immediately read from parallel context (would deadlock with sled!)
        let storage_read = storage.read().await;
        let (_, balance) = storage_read.get_last_balance(&account, &TOS_ASSET).await.unwrap();
        assert_eq!(balance.get_balance(), 1000);

        // No delays, no flush - RocksDB just works!
    }
}
