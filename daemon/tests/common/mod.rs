// Common test utilities for parallel execution tests
//
// This module provides helper functions that avoid the deadlock issues
// caused by manually writing versioned balances to storage.

use std::sync::Arc;
use tempdir::TempDir;
use tos_common::{
    account::{VersionedBalance, VersionedNonce},
    asset::{AssetData, VersionedAssetData},
    block::{Block, BlockHeader, BlockVersion, EXTRA_NONCE_SIZE},
    config::{COIN_DECIMALS, TOS_ASSET},
    crypto::{elgamal::CompressedPublicKey, Hash, Hashable, PublicKey},
    immutable::Immutable,
    network::Network,
    serializer::{Reader, Serializer, Writer},
    versioned_type::Versioned,
};
use tos_daemon::core::{
    config::RocksDBConfig,
    error::BlockchainError,
    state::parallel_chain_state::ParallelChainState,
    storage::{AssetProvider, BalanceProvider, NonceProvider, RocksStorage},
};
use tos_environment::Environment;

/// Create a test RocksDBConfig with sensible defaults
fn create_test_rocksdb_config() -> RocksDBConfig {
    use tos_common::utils::detect_available_parallelism;
    use tos_daemon::core::storage::rocksdb::{CacheMode, CompressionMode};

    RocksDBConfig {
        parallelism: detect_available_parallelism(),
        max_background_jobs: detect_available_parallelism(),
        max_subcompaction_jobs: detect_available_parallelism(),
        low_priority_background_threads: detect_available_parallelism(),
        max_open_files: 1000,
        keep_max_log_files: 10,
        compression_mode: CompressionMode::default(),
        cache_mode: CacheMode::default(),
        cache_size: 64 * 1024 * 1024,       // 64MB
        write_buffer_size: 64 * 1024 * 1024, // 64MB
        write_buffer_shared: false,
    }
}

/// Create a test storage instance with TOS asset registered
pub async fn create_test_storage() -> Arc<tokio::sync::RwLock<RocksStorage>> {
    let temp_dir = TempDir::new("tos_parallel_test").unwrap();
    let config = create_test_rocksdb_config();
    let storage = RocksStorage::new(
        &temp_dir.path().to_string_lossy().to_string(),
        Network::Devnet,
        &config,
    );

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

/// Create a dummy block for testing
pub fn create_dummy_block() -> (Block, Hash) {
    let mut buffer = Vec::new();
    let mut writer = Writer::new(&mut buffer);
    writer.write_bytes(&[0u8; 32]);
    let data = writer.as_bytes();

    let mut reader = Reader::new(data);
    let miner = CompressedPublicKey::read(&mut reader).expect("Failed to create test pubkey");

    let header = BlockHeader::new_simple(
        BlockVersion::Nobunaga,
        vec![],
        0,
        [0u8; EXTRA_NONCE_SIZE],
        miner,
        Hash::zero(),
    );

    let block = Block::new(Immutable::Owned(header), vec![]);
    let hash = block.hash();
    (block, hash)
}

/// Setup account state - SAFE version for parallel execution tests
///
/// This function performs storage writes in a single-threaded context,
/// then adds a small delay to let RocksDB complete internal operations before parallel
/// execution begins.
///
/// KEY FEATURES:
/// 1. Writes are done BEFORE ParallelChainState creation
/// 2. Adds tokio::time::sleep(5ms) to let RocksDB flush internal state
/// 3. No concurrent storage access during the write phase
pub async fn setup_account_safe(
    storage: &Arc<tokio::sync::RwLock<RocksStorage>>,
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

    // Give RocksDB time to complete internal flush operations
    // RocksDB is generally faster than sled, but we keep a small delay for safety
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    Ok(())
}

/// Force flush RocksDB storage and wait for completion
///
/// Call this AFTER all account setup is complete and BEFORE creating ParallelChainState.
/// This ensures RocksDB's internal state is fully committed before parallel execution begins.
pub async fn flush_storage_and_wait(storage: &Arc<tokio::sync::RwLock<RocksStorage>>) {
    {
        let mut storage_write = storage.write().await;
        // RocksDB flush is synchronous
        let _ = storage_write.flush().await;
    }

    // Additional safety delay to let caches settle
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
}

/// LEGACY: Setup account state by writing to storage (MAY CAUSE ISSUES IN TESTS)
///
/// This function is kept for reference but should NOT be used in parallel execution tests.
/// Use setup_account_safe() instead.
#[allow(dead_code)]
pub async fn setup_account_in_storage_legacy(
    storage: &Arc<tokio::sync::RwLock<RocksStorage>>,
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
