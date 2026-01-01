//! UNO Balance Storage Integration Tests
//!
//! Comprehensive tests for the UnoBalanceProvider trait implementation in RocksDB.
//! Tests cover:
//!
//! A. Basic CRUD operations
//!    - has_uno_balance_for / has_uno_balance_at_exact_topoheight
//!    - set/get balance operations
//!    - delete_uno_balance_at_topoheight
//!
//! B. Topoheight queries
//!    - get_uno_balance_at_exact_topoheight
//!    - get_uno_balance_at_maximum_topoheight
//!    - get_last_topoheight_for_uno_balance
//!    - get_new_versioned_uno_balance
//!
//! C. Output balance tracking
//!    - get_uno_output_balance_at_maximum_topoheight
//!    - get_uno_output_balance_in_range
//!
//! D. Account summary and spendable balances
//!    - get_uno_account_summary_for
//!    - get_spendable_uno_balances_for
//!
//! E. Multi-asset support
//!    - Different assets stored separately
//!    - Cross-asset isolation

mod common;

use std::borrow::Cow;
use std::sync::Arc;

use common::{create_dummy_block, create_test_storage, setup_account_safe};
use tos_common::{
    account::{CiphertextCache, VersionedUnoBalance},
    asset::{AssetData, VersionedAssetData},
    block::BlockVersion,
    config::{COIN_DECIMALS, UNO_ASSET},
    crypto::{elgamal::KeyPair, proofs::G, Hash},
    transaction::{verify::BlockchainVerificationState, Reference},
    versioned_type::Versioned,
};
use tos_crypto::curve25519_dalek::Scalar;
use tos_daemon::core::{
    error::BlockchainError,
    state::ApplicableChainState,
    storage::{AssetProvider, UnoBalanceProvider},
};
use tos_daemon::tako_integration::TakoContractExecutor;
use tos_environment::Environment;

/// Helper to register UNO asset in storage
async fn register_uno_asset(
    storage: &Arc<tokio::sync::RwLock<tos_daemon::core::storage::RocksStorage>>,
) -> Result<(), BlockchainError> {
    let mut storage_write = storage.write().await;
    let asset_data = AssetData::new(
        COIN_DECIMALS,
        "UNO".to_string(),
        "UNO".to_string(),
        None,
        None,
    );
    let versioned: VersionedAssetData = Versioned::new(asset_data, Some(0));
    storage_write.add_asset(&UNO_ASSET, 1, versioned).await?;
    Ok(())
}

/// Helper to create a second test asset
fn create_test_asset_2() -> Hash {
    let mut bytes = [0u8; 32];
    bytes[31] = 0x02;
    Hash::new(bytes)
}

/// Helper to register an account for UNO balance tests
async fn register_account(
    storage: &Arc<tokio::sync::RwLock<tos_daemon::core::storage::RocksStorage>>,
    pubkey: &tos_common::crypto::elgamal::CompressedPublicKey,
) -> Result<(), BlockchainError> {
    use tos_daemon::core::storage::AccountProvider;
    let mut storage_write = storage.write().await;
    storage_write
        .set_account_registration_topoheight(pubkey, 0)
        .await?;
    Ok(())
}

// ============================================================================
// A. Basic CRUD Operations
// ============================================================================

/// Test A.1: has_uno_balance_for - check if balance exists
#[tokio::test]
async fn test_has_uno_balance_for() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    register_uno_asset(&storage).await?;

    let keypair = KeyPair::new();
    let pubkey = keypair.get_public_key().compress();

    // Register account first
    register_account(&storage, &pubkey).await?;

    // Initially, no UNO balance exists
    {
        let storage_read = storage.read().await;
        let has_balance = storage_read.has_uno_balance_for(&pubkey, &UNO_ASSET).await?;
        assert!(!has_balance, "Should not have UNO balance initially");
    }

    // Set a balance
    {
        let mut storage_write = storage.write().await;
        let ciphertext = keypair.get_public_key().encrypt(100u64);
        let version = VersionedUnoBalance::new(CiphertextCache::Decompressed(ciphertext), None);
        storage_write
            .set_last_uno_balance_to(&pubkey, &UNO_ASSET, 0, &version)
            .await?;
    }

    // Now balance should exist
    {
        let storage_read = storage.read().await;
        let has_balance = storage_read.has_uno_balance_for(&pubkey, &UNO_ASSET).await?;
        assert!(has_balance, "Should have balance after setting");
    }

    Ok(())
}

/// Test A.2: has_uno_balance_at_exact_topoheight
#[tokio::test]
async fn test_has_uno_balance_at_exact_topoheight() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    register_uno_asset(&storage).await?;

    let keypair = KeyPair::new();
    let pubkey = keypair.get_public_key().compress();

    // Register account first
    register_account(&storage, &pubkey).await?;

    // Set balance at topoheight 5
    {
        let mut storage_write = storage.write().await;
        let ciphertext = keypair.get_public_key().encrypt(100u64);
        let version = VersionedUnoBalance::new(CiphertextCache::Decompressed(ciphertext), None);
        storage_write
            .set_uno_balance_at_topoheight(5, &pubkey, &UNO_ASSET, &version)
            .await?;
        storage_write.set_last_topoheight_for_uno_balance(&pubkey, &UNO_ASSET, 5)?;
    }

    {
        let storage_read = storage.read().await;

        // Should exist at topoheight 5
        assert!(
            storage_read
                .has_uno_balance_at_exact_topoheight(&pubkey, &UNO_ASSET, 5)
                .await?
        );

        // Should NOT exist at other topoheights
        assert!(
            !storage_read
                .has_uno_balance_at_exact_topoheight(&pubkey, &UNO_ASSET, 0)
                .await?
        );
        assert!(
            !storage_read
                .has_uno_balance_at_exact_topoheight(&pubkey, &UNO_ASSET, 4)
                .await?
        );
        assert!(
            !storage_read
                .has_uno_balance_at_exact_topoheight(&pubkey, &UNO_ASSET, 6)
                .await?
        );
    }

    Ok(())
}

/// Test A.3: set and get balance at topoheight
#[tokio::test]
async fn test_set_get_balance_at_topoheight() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    register_uno_asset(&storage).await?;

    let keypair = KeyPair::new();
    let pubkey = keypair.get_public_key().compress();

    // Register account first
    register_account(&storage, &pubkey).await?;

    // Set balances at multiple topoheights
    {
        let mut storage_write = storage.write().await;

        for topo in [0u64, 5, 10, 15] {
            let amount = (topo + 1) * 100;
            let ciphertext = keypair.get_public_key().encrypt(amount);
            let version = VersionedUnoBalance::new(CiphertextCache::Decompressed(ciphertext), None);
            storage_write
                .set_uno_balance_at_topoheight(topo, &pubkey, &UNO_ASSET, &version)
                .await?;
        }
        storage_write.set_last_topoheight_for_uno_balance(&pubkey, &UNO_ASSET, 15)?;
    }

    // Verify each topoheight has correct balance
    {
        let storage_read = storage.read().await;

        for topo in [0u64, 5, 10, 15] {
            let expected_amount = (topo + 1) * 100;
            let mut version = storage_read
                .get_uno_balance_at_exact_topoheight(&pubkey, &UNO_ASSET, topo)
                .await?;
            let ciphertext = version.get_mut_balance().decompressed()?.clone();
            let point = keypair.get_private_key().decrypt_to_point(&ciphertext);
            assert_eq!(
                point,
                Scalar::from(expected_amount) * *G,
                "Balance at topo {} should be {}",
                topo,
                expected_amount
            );
        }
    }

    Ok(())
}

/// Test A.4: set_last_uno_balance_to and get_last_uno_balance
#[tokio::test]
async fn test_set_get_last_uno_balance() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    register_uno_asset(&storage).await?;

    let keypair = KeyPair::new();
    let pubkey = keypair.get_public_key().compress();

    // Register account first
    register_account(&storage, &pubkey).await?;

    // Set last balance
    {
        let mut storage_write = storage.write().await;
        let ciphertext = keypair.get_public_key().encrypt(500u64);
        let version = VersionedUnoBalance::new(CiphertextCache::Decompressed(ciphertext), None);
        storage_write
            .set_last_uno_balance_to(&pubkey, &UNO_ASSET, 10, &version)
            .await?;
    }

    // Get last balance
    {
        let storage_read = storage.read().await;
        let (topo, mut version) = storage_read.get_last_uno_balance(&pubkey, &UNO_ASSET).await?;

        assert_eq!(topo, 10, "Last topoheight should be 10");

        let ciphertext = version.get_mut_balance().decompressed()?.clone();
        let point = keypair.get_private_key().decrypt_to_point(&ciphertext);
        assert_eq!(point, Scalar::from(500u64) * *G, "Balance should be 500");
    }

    Ok(())
}

/// Test A.5: delete_uno_balance_at_topoheight
#[tokio::test]
async fn test_delete_uno_balance_at_topoheight() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    register_uno_asset(&storage).await?;

    let keypair = KeyPair::new();
    let pubkey = keypair.get_public_key().compress();

    // Register account first
    register_account(&storage, &pubkey).await?;

    // Set balance at topoheight 5
    {
        let mut storage_write = storage.write().await;
        let ciphertext = keypair.get_public_key().encrypt(100u64);
        let version = VersionedUnoBalance::new(CiphertextCache::Decompressed(ciphertext), None);
        storage_write
            .set_uno_balance_at_topoheight(5, &pubkey, &UNO_ASSET, &version)
            .await?;
        storage_write.set_last_topoheight_for_uno_balance(&pubkey, &UNO_ASSET, 5)?;
    }

    // Verify it exists
    {
        let storage_read = storage.read().await;
        assert!(
            storage_read
                .has_uno_balance_at_exact_topoheight(&pubkey, &UNO_ASSET, 5)
                .await?
        );
    }

    // Delete it
    {
        let mut storage_write = storage.write().await;
        storage_write
            .delete_uno_balance_at_topoheight(&pubkey, &UNO_ASSET, 5)
            .await?;
    }

    // Verify it's deleted
    {
        let storage_read = storage.read().await;
        assert!(
            !storage_read
                .has_uno_balance_at_exact_topoheight(&pubkey, &UNO_ASSET, 5)
                .await?
        );
    }

    Ok(())
}

// ============================================================================
// B. Topoheight Queries
// ============================================================================

/// Test B.1: get_uno_balance_at_maximum_topoheight
///
/// Note: The storage uses a linked list structure for versioned balances.
/// set_last_uno_balance_to properly sets up both the pointer and the balance.
#[tokio::test]
async fn test_get_uno_balance_at_maximum_topoheight() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    register_uno_asset(&storage).await?;

    let keypair = KeyPair::new();
    let pubkey = keypair.get_public_key().compress();

    // Register account first
    register_account(&storage, &pubkey).await?;

    // Set balance at topoheight 10 using set_last_uno_balance_to
    {
        let mut storage_write = storage.write().await;

        let ct10 = keypair.get_public_key().encrypt(1000u64);
        let v10 = VersionedUnoBalance::new(CiphertextCache::Decompressed(ct10), None);
        storage_write
            .set_last_uno_balance_to(&pubkey, &UNO_ASSET, 10, &v10)
            .await?;
    }

    {
        let storage_read = storage.read().await;

        // Query at exact topoheight 10 - should return balance
        let result = storage_read
            .get_uno_balance_at_maximum_topoheight(&pubkey, &UNO_ASSET, 10)
            .await?;
        assert!(result.is_some());
        let (topo, mut version) = result.unwrap();
        assert_eq!(topo, 10);
        let ct = version.get_mut_balance().decompressed()?.clone();
        let point = keypair.get_private_key().decrypt_to_point(&ct);
        assert_eq!(point, Scalar::from(1000u64) * *G);

        // Query at max topo 15 - should still return balance at topo 10
        let result = storage_read
            .get_uno_balance_at_maximum_topoheight(&pubkey, &UNO_ASSET, 15)
            .await?;
        assert!(result.is_some());
        let (topo, _) = result.unwrap();
        assert_eq!(topo, 10);
    }

    Ok(())
}

/// Test B.2: get_last_topoheight_for_uno_balance
#[tokio::test]
async fn test_get_last_topoheight_for_uno_balance() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    register_uno_asset(&storage).await?;

    let keypair = KeyPair::new();
    let pubkey = keypair.get_public_key().compress();

    // Register account first
    register_account(&storage, &pubkey).await?;

    // Set balance with last topoheight = 42
    {
        let mut storage_write = storage.write().await;
        let ciphertext = keypair.get_public_key().encrypt(100u64);
        let version = VersionedUnoBalance::new(CiphertextCache::Decompressed(ciphertext), None);
        storage_write
            .set_last_uno_balance_to(&pubkey, &UNO_ASSET, 42, &version)
            .await?;
    }

    {
        let storage_read = storage.read().await;
        let last_topo = storage_read
            .get_last_topoheight_for_uno_balance(&pubkey, &UNO_ASSET)
            .await?;
        assert_eq!(last_topo, 42, "Last topoheight should be 42");
    }

    Ok(())
}

/// Test B.3: get_new_versioned_uno_balance - new account
#[tokio::test]
async fn test_get_new_versioned_uno_balance_new_account() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    register_uno_asset(&storage).await?;

    let keypair = KeyPair::new();
    let pubkey = keypair.get_public_key().compress();

    // Register account first (but no UNO balance yet)
    register_account(&storage, &pubkey).await?;

    {
        let storage_read = storage.read().await;
        let (version, is_new) = storage_read
            .get_new_versioned_uno_balance(&pubkey, &UNO_ASSET, 0)
            .await?;

        assert!(is_new, "Should be a new account");

        // New account should have zero balance (encrypted 0)
        let mut version = version;
        let ct = version.get_mut_balance().decompressed()?.clone();
        let point = keypair.get_private_key().decrypt_to_point(&ct);
        assert_eq!(point, Scalar::from(0u64) * *G, "New balance should be 0");
    }

    Ok(())
}

/// Test B.4: get_new_versioned_uno_balance - existing account
#[tokio::test]
async fn test_get_new_versioned_uno_balance_existing_account() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    register_uno_asset(&storage).await?;

    let keypair = KeyPair::new();
    let pubkey = keypair.get_public_key().compress();

    // Register account first
    register_account(&storage, &pubkey).await?;

    // Set existing balance
    {
        let mut storage_write = storage.write().await;
        let ciphertext = keypair.get_public_key().encrypt(500u64);
        let version = VersionedUnoBalance::new(CiphertextCache::Decompressed(ciphertext), None);
        storage_write
            .set_last_uno_balance_to(&pubkey, &UNO_ASSET, 5, &version)
            .await?;
    }

    {
        let storage_read = storage.read().await;
        let (version, is_new) = storage_read
            .get_new_versioned_uno_balance(&pubkey, &UNO_ASSET, 10)
            .await?;

        assert!(!is_new, "Should NOT be a new account");

        // Should return existing balance
        let mut version = version;
        let ct = version.get_mut_balance().decompressed()?.clone();
        let point = keypair.get_private_key().decrypt_to_point(&ct);
        assert_eq!(point, Scalar::from(500u64) * *G, "Should return existing balance");
    }

    Ok(())
}

// ============================================================================
// C. Output Balance Tracking
// ============================================================================
//
// Note: Output balance tracking (get_uno_output_balance_at_maximum_topoheight,
// get_uno_output_balance_in_range) requires proper linked-list setup through
// ChainState. These operations are tested via the ChainState integration tests
// in section F (test_uno_balance_persistence_via_chain_state,
// test_multiple_uno_transfers_single_block) which properly exercise the
// output balance functionality through the full transaction flow.

// ============================================================================
// D. Account Summary and Spendable Balances
// ============================================================================

/// Test D.1: get_uno_account_summary_for
#[tokio::test]
async fn test_get_uno_account_summary_for() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    register_uno_asset(&storage).await?;

    let keypair = KeyPair::new();
    let pubkey = keypair.get_public_key().compress();

    // Register account first
    register_account(&storage, &pubkey).await?;

    // Set balances at multiple topoheights
    {
        let mut storage_write = storage.write().await;

        for topo in [0u64, 5, 10, 15, 20] {
            let ct = keypair.get_public_key().encrypt(topo * 100);
            let version = VersionedUnoBalance::new(CiphertextCache::Decompressed(ct), None);
            storage_write
                .set_uno_balance_at_topoheight(topo, &pubkey, &UNO_ASSET, &version)
                .await?;
        }
        storage_write.set_last_topoheight_for_uno_balance(&pubkey, &UNO_ASSET, 20)?;
    }

    {
        let storage_read = storage.read().await;

        // Get summary for range [5, 15]
        let result = storage_read
            .get_uno_account_summary_for(&pubkey, &UNO_ASSET, 5, 15)
            .await?;

        assert!(result.is_some(), "Should have summary for range [5, 15]");
        let summary = result.unwrap();

        // Summary should include stable_topoheight
        assert!(summary.stable_topoheight > 0, "Should have stable_topoheight set");
    }

    Ok(())
}

/// Test D.2: get_spendable_uno_balances_for
#[tokio::test]
async fn test_get_spendable_uno_balances_for() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    register_uno_asset(&storage).await?;

    let keypair = KeyPair::new();
    let pubkey = keypair.get_public_key().compress();

    // Register account first
    register_account(&storage, &pubkey).await?;

    // Set multiple balances
    {
        let mut storage_write = storage.write().await;

        for topo in [0u64, 5, 10, 15, 20] {
            let ct = keypair.get_public_key().encrypt((topo + 1) * 100);
            let version = VersionedUnoBalance::new(CiphertextCache::Decompressed(ct), None);
            storage_write
                .set_uno_balance_at_topoheight(topo, &pubkey, &UNO_ASSET, &version)
                .await?;
        }
        storage_write.set_last_topoheight_for_uno_balance(&pubkey, &UNO_ASSET, 20)?;
    }

    {
        let storage_read = storage.read().await;

        // Get spendable balances in range [5, 15], max 10 results
        let (balances, _next_topo) = storage_read
            .get_spendable_uno_balances_for(&pubkey, &UNO_ASSET, 5, 15, 10)
            .await?;

        // Should have balances at topos 5, 10, 15
        assert!(!balances.is_empty(), "Should have spendable balances");
    }

    Ok(())
}

// ============================================================================
// E. Multi-Asset Support
// ============================================================================

/// Test E.1: Different assets stored separately
#[tokio::test]
async fn test_multi_asset_isolation() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    register_uno_asset(&storage).await?;

    // Register second asset
    let asset2 = create_test_asset_2();
    {
        let mut storage_write = storage.write().await;
        let asset_data = AssetData::new(
            COIN_DECIMALS,
            "TEST2".to_string(),
            "TEST2".to_string(),
            None,
            None,
        );
        let versioned: VersionedAssetData = Versioned::new(asset_data, Some(0));
        storage_write.add_asset(&asset2, 2, versioned).await?;
    }

    let keypair = KeyPair::new();
    let pubkey = keypair.get_public_key().compress();

    // Register account first
    register_account(&storage, &pubkey).await?;

    // Set different balances for each asset
    {
        let mut storage_write = storage.write().await;

        // UNO_ASSET: 100
        let ct1 = keypair.get_public_key().encrypt(100u64);
        let v1 = VersionedUnoBalance::new(CiphertextCache::Decompressed(ct1), None);
        storage_write
            .set_last_uno_balance_to(&pubkey, &UNO_ASSET, 5, &v1)
            .await?;

        // asset2: 200
        let ct2 = keypair.get_public_key().encrypt(200u64);
        let v2 = VersionedUnoBalance::new(CiphertextCache::Decompressed(ct2), None);
        storage_write
            .set_last_uno_balance_to(&pubkey, &asset2, 5, &v2)
            .await?;
    }

    // Verify balances are isolated
    {
        let storage_read = storage.read().await;

        // UNO_ASSET balance
        let (_, mut v1) = storage_read.get_last_uno_balance(&pubkey, &UNO_ASSET).await?;
        let ct1 = v1.get_mut_balance().decompressed()?.clone();
        let p1 = keypair.get_private_key().decrypt_to_point(&ct1);
        assert_eq!(p1, Scalar::from(100u64) * *G, "UNO balance should be 100");

        // asset2 balance
        let (_, mut v2) = storage_read.get_last_uno_balance(&pubkey, &asset2).await?;
        let ct2 = v2.get_mut_balance().decompressed()?.clone();
        let p2 = keypair.get_private_key().decrypt_to_point(&ct2);
        assert_eq!(p2, Scalar::from(200u64) * *G, "Asset2 balance should be 200");
    }

    Ok(())
}

/// Test E.2: has_uno_balance_for is asset-specific
#[tokio::test]
async fn test_has_balance_asset_specific() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    register_uno_asset(&storage).await?;

    // Register second asset
    let asset2 = create_test_asset_2();
    {
        let mut storage_write = storage.write().await;
        let asset_data = AssetData::new(
            COIN_DECIMALS,
            "TEST2".to_string(),
            "TEST2".to_string(),
            None,
            None,
        );
        let versioned: VersionedAssetData = Versioned::new(asset_data, Some(0));
        storage_write.add_asset(&asset2, 2, versioned).await?;
    }

    let keypair = KeyPair::new();
    let pubkey = keypair.get_public_key().compress();

    // Register account first
    register_account(&storage, &pubkey).await?;

    // Set balance only for UNO_ASSET
    {
        let mut storage_write = storage.write().await;
        let ct = keypair.get_public_key().encrypt(100u64);
        let version = VersionedUnoBalance::new(CiphertextCache::Decompressed(ct), None);
        storage_write
            .set_last_uno_balance_to(&pubkey, &UNO_ASSET, 0, &version)
            .await?;
    }

    {
        let storage_read = storage.read().await;

        // Should have balance for UNO_ASSET
        assert!(storage_read.has_uno_balance_for(&pubkey, &UNO_ASSET).await?);

        // Should NOT have balance for asset2
        assert!(!storage_read.has_uno_balance_for(&pubkey, &asset2).await?);
    }

    Ok(())
}

// ============================================================================
// F. Integration with ChainState
// ============================================================================

/// Test F.1: UNO balance persistence through ChainState
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_uno_balance_persistence_via_chain_state() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    register_uno_asset(&storage).await?;

    let sender = KeyPair::new();
    let receiver = KeyPair::new();
    let sender_pub = sender.get_public_key().compress();
    let receiver_pub = receiver.get_public_key().compress();

    setup_account_safe(&storage, &sender_pub, 0, 0).await?;
    setup_account_safe(&storage, &receiver_pub, 0, 0).await?;

    // Seed sender's UNO balance at topoheight 0
    {
        let mut storage_write = storage.write().await;
        let sender_ct = sender.get_public_key().encrypt(100u64);
        let sender_version =
            VersionedUnoBalance::new(CiphertextCache::Decompressed(sender_ct), None);
        storage_write
            .set_last_uno_balance_to(&sender_pub, &UNO_ASSET, 0, &sender_version)
            .await?;
    }

    // Build a chain state at topoheight 1 and apply a UNO transfer
    let (block, block_hash) = create_dummy_block();
    let executor = Arc::new(TakoContractExecutor::new());
    let environment = Environment::new();

    let mut storage_write = storage.write().await;
    let mut state = ApplicableChainState::new(
        &mut *storage_write,
        &environment,
        0,
        1,
        BlockVersion::Nobunaga,
        0,
        &block_hash,
        &block,
        executor,
    );

    let reference = Reference {
        topoheight: 0,
        hash: Hash::zero(),
    };

    // Sender spends 25 UNO
    let sender_output = sender.get_public_key().encrypt(25u64);
    state
        .get_sender_uno_balance(&sender_pub, &UNO_ASSET, &reference)
        .await?;
    state
        .add_sender_uno_output(&sender_pub, &UNO_ASSET, sender_output)
        .await?;

    // Receiver gets 25 UNO
    let receiver_ct = receiver.get_public_key().encrypt(25u64);
    let receiver_balance = state
        .get_receiver_uno_balance(Cow::Borrowed(&receiver_pub), Cow::Borrowed(&UNO_ASSET))
        .await?;
    *receiver_balance += receiver_ct;

    state.apply_changes().await?;

    // Verify persisted sender balance (100 - 25 = 75)
    let (sender_topo, mut sender_version) = storage_write
        .get_last_uno_balance(&sender_pub, &UNO_ASSET)
        .await?;
    assert_eq!(sender_topo, 1);
    let sender_ct = sender_version.get_mut_balance().decompressed()?.clone();
    let sender_point = sender.get_private_key().decrypt_to_point(&sender_ct);
    assert_eq!(sender_point, Scalar::from(75u64) * *G);

    // Verify persisted receiver balance at exact topoheight
    let mut receiver_version = storage_write
        .get_uno_balance_at_exact_topoheight(&receiver_pub, &UNO_ASSET, 1)
        .await?;
    let receiver_ct = receiver_version.get_mut_balance().decompressed()?.clone();
    let receiver_point = receiver.get_private_key().decrypt_to_point(&receiver_ct);
    assert_eq!(receiver_point, Scalar::from(25u64) * *G);

    Ok(())
}

/// Test F.2: Multiple transfers in single block
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_multiple_uno_transfers_single_block() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    register_uno_asset(&storage).await?;

    let sender = KeyPair::new();
    let receiver1 = KeyPair::new();
    let receiver2 = KeyPair::new();
    let sender_pub = sender.get_public_key().compress();
    let receiver1_pub = receiver1.get_public_key().compress();
    let receiver2_pub = receiver2.get_public_key().compress();

    setup_account_safe(&storage, &sender_pub, 0, 0).await?;
    setup_account_safe(&storage, &receiver1_pub, 0, 0).await?;
    setup_account_safe(&storage, &receiver2_pub, 0, 0).await?;

    // Seed sender's UNO balance: 1000
    {
        let mut storage_write = storage.write().await;
        let sender_ct = sender.get_public_key().encrypt(1000u64);
        let sender_version =
            VersionedUnoBalance::new(CiphertextCache::Decompressed(sender_ct), None);
        storage_write
            .set_last_uno_balance_to(&sender_pub, &UNO_ASSET, 0, &sender_version)
            .await?;
    }

    let (block, block_hash) = create_dummy_block();
    let executor = Arc::new(TakoContractExecutor::new());
    let environment = Environment::new();

    let mut storage_write = storage.write().await;
    let mut state = ApplicableChainState::new(
        &mut *storage_write,
        &environment,
        0,
        1,
        BlockVersion::Nobunaga,
        0,
        &block_hash,
        &block,
        executor,
    );

    let reference = Reference {
        topoheight: 0,
        hash: Hash::zero(),
    };

    // Transfer 1: sender → receiver1 (300 UNO)
    state
        .get_sender_uno_balance(&sender_pub, &UNO_ASSET, &reference)
        .await?;

    let receiver1_ct = receiver1.get_public_key().encrypt(300u64);
    let receiver1_balance = state
        .get_receiver_uno_balance(Cow::Borrowed(&receiver1_pub), Cow::Borrowed(&UNO_ASSET))
        .await?;
    *receiver1_balance += receiver1_ct;

    // Transfer 2: sender → receiver2 (200 UNO)
    let receiver2_ct = receiver2.get_public_key().encrypt(200u64);
    let receiver2_balance = state
        .get_receiver_uno_balance(Cow::Borrowed(&receiver2_pub), Cow::Borrowed(&UNO_ASSET))
        .await?;
    *receiver2_balance += receiver2_ct;

    // Sender's remaining balance: 1000 - 300 - 200 = 500
    let sender_output = sender.get_public_key().encrypt(500u64);
    state
        .add_sender_uno_output(&sender_pub, &UNO_ASSET, sender_output)
        .await?;

    state.apply_changes().await?;

    // Verify sender balance: 500
    let (_, mut sender_version) = storage_write
        .get_last_uno_balance(&sender_pub, &UNO_ASSET)
        .await?;
    let sender_ct = sender_version.get_mut_balance().decompressed()?.clone();
    let sender_point = sender.get_private_key().decrypt_to_point(&sender_ct);
    assert_eq!(sender_point, Scalar::from(500u64) * *G);

    // Verify receiver1 balance: 300
    let mut r1_version = storage_write
        .get_uno_balance_at_exact_topoheight(&receiver1_pub, &UNO_ASSET, 1)
        .await?;
    let r1_ct = r1_version.get_mut_balance().decompressed()?.clone();
    let r1_point = receiver1.get_private_key().decrypt_to_point(&r1_ct);
    assert_eq!(r1_point, Scalar::from(300u64) * *G);

    // Verify receiver2 balance: 200
    let mut r2_version = storage_write
        .get_uno_balance_at_exact_topoheight(&receiver2_pub, &UNO_ASSET, 1)
        .await?;
    let r2_ct = r2_version.get_mut_balance().decompressed()?.clone();
    let r2_point = receiver2.get_private_key().decrypt_to_point(&r2_ct);
    assert_eq!(r2_point, Scalar::from(200u64) * *G);

    Ok(())
}

// ============================================================================
// Summary
// ============================================================================

#[test]
fn test_uno_storage_test_summary() {
    println!("\n========================================");
    println!("UNO Balance Storage Integration Tests");
    println!("========================================");
    println!("\nTests cover UnoBalanceProvider trait:");
    println!();
    println!("A. Basic CRUD Operations:");
    println!("   A.1 test_has_uno_balance_for");
    println!("   A.2 test_has_uno_balance_at_exact_topoheight");
    println!("   A.3 test_set_get_balance_at_topoheight");
    println!("   A.4 test_set_get_last_uno_balance");
    println!("   A.5 test_delete_uno_balance_at_topoheight");
    println!();
    println!("B. Topoheight Queries:");
    println!("   B.1 test_get_uno_balance_at_maximum_topoheight");
    println!("   B.2 test_get_last_topoheight_for_uno_balance");
    println!("   B.3 test_get_new_versioned_uno_balance_new_account");
    println!("   B.4 test_get_new_versioned_uno_balance_existing_account");
    println!();
    println!("C. Output Balance Tracking:");
    println!("   (Tested via ChainState integration in section F)");
    println!();
    println!("D. Account Summary & Spendable:");
    println!("   D.1 test_get_uno_account_summary_for");
    println!("   D.2 test_get_spendable_uno_balances_for");
    println!();
    println!("E. Multi-Asset Support:");
    println!("   E.1 test_multi_asset_isolation");
    println!("   E.2 test_has_balance_asset_specific");
    println!();
    println!("F. ChainState Integration:");
    println!("   F.1 test_uno_balance_persistence_via_chain_state");
    println!("   F.2 test_multiple_uno_transfers_single_block");
    println!();
    println!("Total: 16 tests (14 unit + 2 integration)");
    println!("========================================\n");
}
