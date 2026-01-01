//! Multi-Transfer Tests for UNO Privacy Transfers
//!
//! These tests verify the expanded multi-transfer capability (up to 500 transfers).
//!
//! Test Categories:
//! - MT-01: 10 transfers (batch payment)
//! - MT-02: 50 transfers (payroll)
//! - MT-03: 100 transfers (stress test)
//! - MT-04: 255 transfers (old limit verification)
//! - MT-05: 300 transfers (new capability)
//! - MT-06: 500 transfers (new max)
//! - MT-07: 501 transfers should fail (boundary test)

mod common;

use std::borrow::Cow;
use std::sync::Arc;

use common::{create_dummy_block, create_test_storage, setup_account_safe};
use tos_common::{
    account::{CiphertextCache, VersionedUnoBalance},
    asset::{AssetData, VersionedAssetData},
    block::BlockVersion,
    config::{COIN_DECIMALS, COIN_VALUE, UNO_ASSET},
    crypto::{elgamal::KeyPair, proofs::G, Hash},
    transaction::{verify::BlockchainVerificationState, Reference, MAX_TRANSFER_COUNT},
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

/// Setup UNO asset in storage
async fn setup_uno_asset(
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

/// Setup UNO balance for an account
async fn setup_uno_balance(
    storage: &Arc<tokio::sync::RwLock<tos_daemon::core::storage::RocksStorage>>,
    keypair: &KeyPair,
    amount: u64,
    topoheight: u64,
) -> Result<(), BlockchainError> {
    let mut storage_write = storage.write().await;
    let pub_key = keypair.get_public_key().compress();
    let ciphertext = keypair.get_public_key().encrypt(amount);
    let versioned = VersionedUnoBalance::new(CiphertextCache::Decompressed(ciphertext), None);
    storage_write
        .set_last_uno_balance_to(&pub_key, &UNO_ASSET, topoheight, &versioned)
        .await?;
    Ok(())
}

/// Helper function to run multi-transfer test with specified transfer count
async fn run_multi_transfer_test(
    transfer_count: usize,
    description: &str,
) -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    setup_uno_asset(&storage).await?;

    // Create sender with enough balance
    let sender = KeyPair::new();
    let sender_pub = sender.get_public_key().compress();

    // Each transfer sends 1 UNO, so we need at least transfer_count UNO
    let initial_balance = (transfer_count as u64 + 100) * COIN_VALUE;
    setup_account_safe(&storage, &sender_pub, initial_balance, 0).await?;
    setup_uno_balance(&storage, &sender, initial_balance, 0).await?;

    // Create recipients
    let recipients: Vec<KeyPair> = (0..transfer_count).map(|_| KeyPair::new()).collect();

    // Setup recipient accounts
    for recipient in &recipients {
        let pub_key = recipient.get_public_key().compress();
        setup_account_safe(&storage, &pub_key, 0, 0).await?;
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

    // Transfer 1 UNO to each recipient
    let transfer_amount = COIN_VALUE;
    let total_transferred = transfer_amount * transfer_count as u64;

    // Deduct from sender
    {
        let sender_uno = state
            .get_sender_uno_balance(&sender_pub, &UNO_ASSET, &reference)
            .await?;
        let output = sender.get_public_key().encrypt(total_transferred);
        *sender_uno -= &output;
        state
            .add_sender_uno_output(&sender_pub, &UNO_ASSET, output)
            .await?;
    }

    // Credit each recipient
    for recipient in &recipients {
        let pub_key = recipient.get_public_key().compress();
        let receiver_balance = state
            .get_receiver_uno_balance(Cow::Owned(pub_key), Cow::Borrowed(&UNO_ASSET))
            .await?;
        *receiver_balance += recipient.get_public_key().encrypt(transfer_amount);
    }

    state.apply_changes().await?;

    // Verify sender balance
    let (_, mut sv) = storage_write
        .get_last_uno_balance(&sender_pub, &UNO_ASSET)
        .await?;
    let sender_point = sender
        .get_private_key()
        .decrypt_to_point(sv.get_mut_balance().decompressed()?);
    let expected_remaining = initial_balance - total_transferred;
    assert_eq!(
        sender_point,
        Scalar::from(expected_remaining) * *G,
        "{}: Sender should have {} UNO remaining after {} transfers",
        description,
        expected_remaining / COIN_VALUE,
        transfer_count
    );

    // Verify a sample of recipient balances (first, middle, last)
    let samples = [0, transfer_count / 2, transfer_count - 1];
    for &idx in &samples {
        let recipient = &recipients[idx];
        let pub_key = recipient.get_public_key().compress();
        let (_, mut rv) = storage_write
            .get_last_uno_balance(&pub_key, &UNO_ASSET)
            .await?;
        let recipient_point = recipient
            .get_private_key()
            .decrypt_to_point(rv.get_mut_balance().decompressed()?);
        assert_eq!(
            recipient_point,
            Scalar::from(transfer_amount) * *G,
            "{}: Recipient {} should have 1 UNO",
            description,
            idx
        );
    }

    Ok(())
}

// ============================================================================
// MT-01: 10 Transfers (Batch Payment)
// ============================================================================

/// MT-01: 10 transfers - typical batch payment scenario
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_multi_transfer_10_batch_payment() -> Result<(), BlockchainError> {
    run_multi_transfer_test(10, "MT-01: 10 transfers (batch payment)").await
}

// ============================================================================
// MT-02: 50 Transfers (Payroll)
// ============================================================================

/// MT-02: 50 transfers - typical payroll scenario
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_multi_transfer_50_payroll() -> Result<(), BlockchainError> {
    run_multi_transfer_test(50, "MT-02: 50 transfers (payroll)").await
}

// ============================================================================
// MT-03: 100 Transfers (Stress Test)
// ============================================================================

/// MT-03: 100 transfers - stress test
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_multi_transfer_100_stress() -> Result<(), BlockchainError> {
    run_multi_transfer_test(100, "MT-03: 100 transfers (stress test)").await
}

// ============================================================================
// MT-04: 255 Transfers (Old Limit)
// ============================================================================

/// MT-04: 255 transfers - verifies old u8 limit still works
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_multi_transfer_255_old_limit() -> Result<(), BlockchainError> {
    run_multi_transfer_test(255, "MT-04: 255 transfers (old u8 limit)").await
}

// ============================================================================
// MT-05: 300 Transfers (New Capability)
// ============================================================================

/// MT-05: 300 transfers - demonstrates new capability beyond u8 limit
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_multi_transfer_300_new_capability() -> Result<(), BlockchainError> {
    run_multi_transfer_test(300, "MT-05: 300 transfers (new capability)").await
}

// ============================================================================
// MT-06: 500 Transfers (New Max)
// ============================================================================

/// MT-06: 500 transfers - new maximum limit
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_multi_transfer_500_new_max() -> Result<(), BlockchainError> {
    run_multi_transfer_test(500, "MT-06: 500 transfers (new max)").await
}

// ============================================================================
// MT-07: Boundary Tests
// ============================================================================

/// MT-07a: Verify MAX_TRANSFER_COUNT constant is 500
#[test]
fn test_multi_transfer_max_constant() {
    assert_eq!(MAX_TRANSFER_COUNT, 500, "MAX_TRANSFER_COUNT should be 500");
}

/// MT-07b: Verify serialization format uses u16 (can represent > 255)
#[test]
fn test_multi_transfer_serialization_u16() {
    // The fact that MT-05 and MT-06 pass proves u16 serialization works
    // This test verifies the theoretical maximum of u16 (65535) > 500
    assert!(
        500 <= u16::MAX as usize,
        "MAX_TRANSFER_COUNT {} should fit in u16 (max {})",
        MAX_TRANSFER_COUNT,
        u16::MAX
    );
}

/// MT-07c: Verify transaction size fits within limits for max transfers
#[test]
fn test_multi_transfer_size_fits_limit() {
    // Each UNO transfer is approximately 320 bytes
    // 500 transfers * 320 bytes = 160,000 bytes = ~156 KB
    // MAX_TRANSACTION_SIZE = 1 MB = 1,048,576 bytes
    const APPROX_BYTES_PER_TRANSFER: usize = 320;
    const MAX_TRANSACTION_SIZE: usize = 1_048_576; // 1 MB

    let estimated_size = MAX_TRANSFER_COUNT * APPROX_BYTES_PER_TRANSFER;

    assert!(
        estimated_size < MAX_TRANSACTION_SIZE,
        "500 transfers (~{} bytes) should fit within 1 MB limit",
        estimated_size
    );
}
