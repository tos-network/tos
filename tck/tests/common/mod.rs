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
    crypto::{elgamal::CompressedPublicKey, Hash, Hashable},
    immutable::Immutable,
    network::Network,
    serializer::{Reader, Serializer, Writer},
    versioned_type::Versioned,
};
use tos_daemon::core::{
    config::RocksDBConfig,
    error::BlockchainError,
    storage::{AccountProvider, AssetProvider, BalanceProvider, NonceProvider, RocksStorage},
};

/// Create a test storage instance with TOS asset registered
#[allow(dead_code)]
pub async fn create_test_storage() -> Arc<tokio::sync::RwLock<RocksStorage>> {
    let temp_dir = TempDir::new("tos_parallel_test").unwrap();
    let dir_path = temp_dir.into_path();
    let config = RocksDBConfig::default();
    let storage = RocksStorage::new(&dir_path.to_string_lossy(), Network::Devnet, &config);

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
        storage_write
            .add_asset(&TOS_ASSET, 0, versioned)
            .await
            .unwrap();
    }

    storage_arc
}

/// Create a dummy block for testing
#[allow(dead_code)]
pub fn create_dummy_block() -> (Block, Hash) {
    let mut buffer = Vec::new();
    let mut writer = Writer::new(&mut buffer);
    writer.write_bytes(&[0u8; 32]);
    let data = writer.as_bytes();

    let mut reader = Reader::new(data);
    let miner = CompressedPublicKey::read(&mut reader).expect("Failed to create test pubkey");

    let header = BlockHeader::new(
        BlockVersion::Nobunaga,
        0,                         // height
        0,                         // timestamp
        indexmap::IndexSet::new(), // tips
        [0u8; EXTRA_NONCE_SIZE],
        miner,
        indexmap::IndexSet::new(), // txs_hashes
    );

    let block = Block::new(Immutable::Owned(header), vec![]);
    let hash = block.hash();
    (block, hash)
}

/// Setup account state - SAFE version for parallel execution tests
///
/// RocksDB handles concurrent access better than Sled, so we no longer need
/// the complex workarounds that were required for Sled's internal locking.
#[allow(dead_code)]
pub async fn setup_account_safe(
    storage: &Arc<tokio::sync::RwLock<RocksStorage>>,
    account: &CompressedPublicKey,
    balance: u64,
    nonce: u64,
) -> Result<(), BlockchainError> {
    {
        let mut storage_write = storage.write().await;

        storage_write
            .set_last_nonce_to(account, 0, &VersionedNonce::new(nonce, Some(0)))
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
    }

    Ok(())
}

/// Flush storage and wait for completion
///
/// RocksDB handles flushing more reliably than Sled.
/// This function is kept for API compatibility but the delays are reduced.
#[allow(dead_code)]
pub async fn flush_storage_and_wait(storage: &Arc<tokio::sync::RwLock<RocksStorage>>) {
    {
        let _storage_read = storage.read().await;
        // RocksDB handles flushing internally, minimal delay needed
    }

    // Small delay for safety
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
}

/// LEGACY: Setup account state by writing to storage
///
/// This function is kept for reference and API compatibility.
#[allow(dead_code)]
pub async fn setup_account_in_storage_legacy(
    storage: &Arc<tokio::sync::RwLock<RocksStorage>>,
    account: &CompressedPublicKey,
    balance: u64,
    nonce: u64,
) -> Result<(), BlockchainError> {
    let mut storage_write = storage.write().await;

    storage_write
        .set_last_nonce_to(account, 0, &VersionedNonce::new(nonce, Some(0)))
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
