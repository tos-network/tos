// File: testing-framework/src/utilities/daemon_helpers.rs
//
// Daemon Test Helpers
//
// Migrated from deprecated testing-integration package.
// These helpers provide RocksDB storage utilities specifically for daemon integration tests.

use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;
use tos_common::{
    account::{VersionedBalance, VersionedNonce},
    asset::{AssetData, VersionedAssetData},
    config::{COIN_DECIMALS, TOS_ASSET},
    crypto::{elgamal::CompressedPublicKey, KeyPair},
    network::Network,
    versioned_type::Versioned,
};
use tos_daemon::core::{
    config::RocksDBConfig,
    error::BlockchainError,
    storage::{
        rocksdb::RocksStorage, AccountProvider, AssetProvider, BalanceProvider, NonceProvider,
    },
};

use super::TempRocksDB;

/// Create a test RocksDB storage instance with TOS asset registered
///
/// This creates a temporary RocksDB storage suitable for daemon integration tests.
/// The storage is automatically cleaned up when dropped (RAII pattern).
///
/// # Configuration
///
/// - Network: Devnet
/// - Cache: 1MB
/// - Location: Temporary directory (auto-cleaned)
/// - Compression: Default (Snappy)
///
/// # Example
///
/// ```ignore
/// use tos_testing_framework::utilities::create_test_rocksdb_storage;
///
/// #[tokio::test]
/// async fn test_with_rocksdb() {
///     let storage = create_test_rocksdb_storage().await;
///
///     // Ready for parallel execution (no flush needed!)
///     let parallel_state = ParallelChainState::new(storage.clone(), 0).await.unwrap();
/// }
/// ```
pub async fn create_test_rocksdb_storage() -> Arc<RwLock<RocksStorage>> {
    // Create temporary directory with RAII cleanup
    let temp_db = TempRocksDB::new().expect("Failed to create temporary RocksDB directory");
    let dir_path = temp_db.path().to_string_lossy().to_string();

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

    storage
        .add_asset(&TOS_ASSET, 0, versioned)
        .await
        .expect("Failed to register TOS asset");

    // Leak the temp_dir to prevent cleanup (storage manages lifecycle now)
    // NOTE: This means the directory will persist until the test process exits.
    // For proper cleanup, tests should drop the storage when done.
    std::mem::forget(temp_db);

    Arc::new(RwLock::new(storage))
}

/// Setup account in RocksDB storage with balance and nonce
///
/// Unlike sled, RocksDB doesn't require delays or flush operations.
/// This function writes directly without waiting.
///
/// # Arguments
///
/// * `storage` - The RocksDB storage instance
/// * `account` - The account public key (compressed)
/// * `balance` - Initial balance in nanoTOS
/// * `nonce` - Initial nonce
///
/// # Example
///
/// ```ignore
/// use tos_testing_framework::utilities::{
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
    storage: &Arc<RwLock<RocksStorage>>,
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

/// Create RocksDB test storage with funded accounts
///
/// This helper avoids mining 300+ blocks in tests by creating storage
/// with pre-funded accounts at genesis (topoheight 0), similar to how
/// Kaspa initializes genesis UTXO balances.
///
/// # Arguments
///
/// * `count` - Number of accounts to create with random keypairs
/// * `balance_per_account` - Initial balance for each account (in nanoTOS)
///
/// # Returns
///
/// * `(storage, keypairs)` - The RocksDB storage instance and the generated keypairs
///
/// # Example
///
/// ```ignore
/// use tos_testing_framework::utilities::create_test_storage_with_funded_accounts;
/// use tos_common::config::COIN_VALUE;
///
/// #[tokio::test]
/// async fn test_transfers() {
///     // Create storage with 10 accounts, each with 1000 TOS
///     let (storage, keypairs) = create_test_storage_with_funded_accounts(10, 1000 * COIN_VALUE).await.unwrap();
///
///     let alice = &keypairs[0];
///     let bob = &keypairs[1];
///
///     // Immediately ready for transactions!
///     // No need to mine 300+ blocks
/// }
/// ```
pub async fn create_test_storage_with_funded_accounts(
    count: usize,
    balance_per_account: u64,
) -> Result<(Arc<RwLock<RocksStorage>>, Vec<KeyPair>), BlockchainError> {
    let storage = create_test_rocksdb_storage().await;

    let mut keypairs = Vec::with_capacity(count);

    for _ in 0..count {
        let keypair = KeyPair::new();
        setup_account_rocksdb(
            &storage,
            &keypair.get_public_key().compress(),
            balance_per_account,
            0,
        )
        .await?;
        keypairs.push(keypair);
    }

    Ok((storage, keypairs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tos_common::{config::COIN_VALUE, serializer::Reader};

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
        let (_topoheight, asset) = storage_read.get_asset(&TOS_ASSET).await.unwrap();
        assert_eq!(asset.get().get_decimals(), COIN_DECIMALS);
    }

    #[tokio::test]
    async fn test_setup_account_rocksdb() {
        let storage = create_test_rocksdb_storage().await;
        let account = create_test_pubkey(1);

        setup_account_rocksdb(&storage, &account, 1000, 5)
            .await
            .unwrap();

        let storage_read = storage.read().await;
        let (_, balance) = storage_read
            .get_last_balance(&account, &TOS_ASSET)
            .await
            .unwrap();
        let (_, nonce) = storage_read.get_last_nonce(&account).await.unwrap();

        assert_eq!(balance.get_balance(), 1000);
        assert_eq!(nonce.get_nonce(), 5);
    }

    #[tokio::test]
    async fn test_create_test_storage_with_funded_accounts() {
        // Create storage with 5 funded accounts
        let (storage, keypairs) = create_test_storage_with_funded_accounts(5, 1000 * COIN_VALUE)
            .await
            .unwrap();

        assert_eq!(keypairs.len(), 5);

        // Verify all accounts have correct balance
        let storage_read = storage.read().await;
        for keypair in keypairs.iter() {
            let pubkey = keypair.get_public_key().compress();
            let (_, balance) = storage_read
                .get_last_balance(&pubkey, &TOS_ASSET)
                .await
                .unwrap();
            assert_eq!(balance.get_balance(), 1000 * COIN_VALUE);

            let (_, nonce) = storage_read.get_last_nonce(&pubkey).await.unwrap();
            assert_eq!(nonce.get_nonce(), 0);

            // Verify account is registered
            let reg_topoheight = storage_read
                .get_account_registration_topoheight(&pubkey)
                .await
                .unwrap();
            assert_eq!(reg_topoheight, 0);
        }
    }

    #[tokio::test]
    async fn test_rocksdb_no_deadlock_immediate_use() {
        // This test verifies RocksDB can be used immediately without delays
        let storage = create_test_rocksdb_storage().await;
        let account = create_test_pubkey(99);

        // Setup account
        setup_account_rocksdb(&storage, &account, 1000, 0)
            .await
            .unwrap();

        // Immediately read from parallel context (would deadlock with sled!)
        let storage_read = storage.read().await;
        let (_, balance) = storage_read
            .get_last_balance(&account, &TOS_ASSET)
            .await
            .unwrap();
        assert_eq!(balance.get_balance(), 1000);

        // No delays, no flush - RocksDB just works!
    }
}
