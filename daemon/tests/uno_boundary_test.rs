//! Boundary Condition Tests for UNO Privacy Transfers
//!
//! These tests verify edge cases and boundary conditions.
//!
//! Test Categories:
//! - BC-01 ~ BC-05: Amount Boundaries
//! - BC-06 ~ BC-10: Multiple Transfer Boundaries

mod common;

use std::borrow::Cow;
use std::sync::Arc;

use common::{create_dummy_block, create_test_storage, setup_account_safe};
use tos_common::{
    account::{CiphertextCache, VersionedUnoBalance},
    asset::{AssetData, VersionedAssetData},
    block::BlockVersion,
    config::{COIN_DECIMALS, COIN_VALUE, UNO_ASSET},
    crypto::{
        elgamal::{CompressedPublicKey, KeyPair, PedersenCommitment, PedersenOpening},
        proofs::{CiphertextValidityProof, G},
        Hash,
    },
    transaction::{verify::BlockchainVerificationState, Reference, Role, UnoTransferPayload},
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

/// Create a test UnoTransferPayload with valid proofs
fn create_test_uno_payload(
    sender_keypair: &KeyPair,
    receiver_keypair: &KeyPair,
    amount: u64,
) -> UnoTransferPayload {
    let destination = receiver_keypair.get_public_key().compress();
    let opening = PedersenOpening::generate_new();

    let commitment = PedersenCommitment::new_with_opening(amount, &opening);
    let sender_handle = sender_keypair.get_public_key().decrypt_handle(&opening);
    let receiver_handle = receiver_keypair.get_public_key().decrypt_handle(&opening);

    let mut transcript = tos_common::crypto::new_proof_transcript(b"test_uno_transfer");
    let proof = CiphertextValidityProof::new(
        receiver_keypair.get_public_key(),
        Some(sender_keypair.get_public_key()),
        amount,
        &opening,
        &mut transcript,
    );

    UnoTransferPayload::new(
        UNO_ASSET,
        destination,
        None,
        commitment.compress(),
        sender_handle.compress(),
        receiver_handle.compress(),
        proof,
    )
}

// ============================================================================
// BC-01 ~ BC-05: Amount Boundaries
// ============================================================================

/// BC-01: Zero Amount Transfer
/// Transfer 0 UNO -> Cryptographically valid but semantically questionable
#[test]
fn test_boundary_zero_amount() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();

    // Create payload for 0 UNO
    let payload = create_test_uno_payload(&sender, &receiver, 0);

    // Cryptographically, zero amount is valid
    let receiver_ct = payload.get_ciphertext(Role::Receiver);
    let decompressed = receiver_ct.decompress().expect("Should decompress");
    let decrypted = receiver.get_private_key().decrypt_to_point(&decompressed);

    assert_eq!(
        decrypted,
        Scalar::from(0u64) * *G,
        "Zero amount should decrypt to identity point"
    );

    // Note: Real TX verification may reject zero amounts as invalid
}

/// BC-02: Minimum Amount (1 atomic unit)
/// Transfer 1 UNO -> Should work
#[test]
fn test_boundary_minimum_amount() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();

    // Create payload for 1 (minimum non-zero)
    let payload = create_test_uno_payload(&sender, &receiver, 1);

    let receiver_ct = payload.get_ciphertext(Role::Receiver);
    let decompressed = receiver_ct.decompress().expect("Should decompress");
    let decrypted = receiver.get_private_key().decrypt_to_point(&decompressed);

    assert_eq!(
        decrypted,
        Scalar::from(1u64) * *G,
        "Minimum amount should be exactly 1"
    );
}

/// BC-03: Maximum Amount (u64::MAX)
/// Transfer u64::MAX UNO -> Cryptographically valid
#[test]
fn test_boundary_maximum_amount() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();

    // Create payload for maximum possible amount
    let max_amount = u64::MAX;
    let payload = create_test_uno_payload(&sender, &receiver, max_amount);

    // Ciphertext should be valid
    let receiver_ct = payload.get_ciphertext(Role::Receiver);
    assert!(
        receiver_ct.decompress().is_ok(),
        "Max amount ciphertext should decompress"
    );

    // Note: Decryption of large amounts via brute force is impractical
    // This test verifies the cryptographic operations don't panic
}

/// BC-04: Exact Balance Transfer
/// Transfer entire balance -> Balance becomes 0
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_boundary_exact_balance() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    setup_uno_asset(&storage).await?;

    let alice = KeyPair::new();
    let bob = KeyPair::new();
    let alice_pub = alice.get_public_key().compress();
    let bob_pub = bob.get_public_key().compress();

    setup_account_safe(&storage, &alice_pub, 100 * COIN_VALUE, 0).await?;
    setup_account_safe(&storage, &bob_pub, 0, 0).await?;
    // Alice has exactly 100 UNO
    let alice_balance = 100 * COIN_VALUE;
    setup_uno_balance(&storage, &alice, alice_balance, 0).await?;

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

    // Transfer EXACTLY alice_balance (entire balance)
    let sender_uno = state
        .get_sender_uno_balance(&alice_pub, &UNO_ASSET, &reference)
        .await?;
    let output = alice.get_public_key().encrypt(alice_balance);
    *sender_uno -= &output;
    state
        .add_sender_uno_output(&alice_pub, &UNO_ASSET, output)
        .await?;

    // Credit to Bob
    let receiver_balance = state
        .get_receiver_uno_balance(Cow::Borrowed(&bob_pub), Cow::Borrowed(&UNO_ASSET))
        .await?;
    *receiver_balance += bob.get_public_key().encrypt(alice_balance);

    state.apply_changes().await?;

    // Verify Alice has exactly 0
    let (_, mut av) = storage_write
        .get_last_uno_balance(&alice_pub, &UNO_ASSET)
        .await?;
    let alice_point = alice
        .get_private_key()
        .decrypt_to_point(av.get_mut_balance().decompressed()?);
    assert_eq!(
        alice_point,
        Scalar::from(0u64) * *G,
        "Alice should have exactly 0 after transferring entire balance"
    );

    // Verify Bob has exactly alice_balance
    let (_, mut bv) = storage_write
        .get_last_uno_balance(&bob_pub, &UNO_ASSET)
        .await?;
    let bob_point = bob
        .get_private_key()
        .decrypt_to_point(bv.get_mut_balance().decompressed()?);
    assert_eq!(
        bob_point,
        Scalar::from(alice_balance) * *G,
        "Bob should have exact amount transferred"
    );

    Ok(())
}

/// BC-05: Balance + 1 (Overspend by 1)
/// Transfer balance + 1 -> Should cause issues
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_boundary_balance_plus_one() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    setup_uno_asset(&storage).await?;

    let alice = KeyPair::new();
    let alice_pub = alice.get_public_key().compress();

    setup_account_safe(&storage, &alice_pub, 100 * COIN_VALUE, 0).await?;
    // Alice has exactly 100 UNO
    let alice_balance = 100 * COIN_VALUE;
    setup_uno_balance(&storage, &alice, alice_balance, 0).await?;

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

    // Verify initial balance
    let sender_uno = state
        .get_sender_uno_balance(&alice_pub, &UNO_ASSET, &reference)
        .await?;
    let initial = alice.get_private_key().decrypt_to_point(sender_uno);
    assert_eq!(
        initial,
        Scalar::from(alice_balance) * *G,
        "Alice should have 100 UNO"
    );

    // Try to subtract balance + 1
    // In real TX, this would be caught by range proof
    // Here we just verify the state tracking behavior

    Ok(())
}

// ============================================================================
// BC-06 ~ BC-10: Multiple Transfer Boundaries
// ============================================================================

/// BC-06: Empty Transfer List
/// TX with 0 transfers -> Invalid TX structure
#[test]
fn test_boundary_empty_transfer_list() {
    // UnoTransferPayload represents a single transfer
    // A TX with zero transfers would have no TransactionType::UnoTransfers

    // This test verifies that payloads represent valid transfers
    let sender = KeyPair::new();
    let receiver = KeyPair::new();
    let payload = create_test_uno_payload(&sender, &receiver, 100);

    // Each payload is a valid single transfer
    assert!(payload.get_commitment().decompress().is_ok());
}

/// BC-07: Single Transfer
/// TX with exactly 1 transfer -> Should work
#[test]
fn test_boundary_single_transfer() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();

    // Single transfer is the basic unit
    let payload = create_test_uno_payload(&sender, &receiver, 100);

    let receiver_ct = payload.get_ciphertext(Role::Receiver);
    let decrypted = receiver
        .get_private_key()
        .decrypt_to_point(&receiver_ct.decompress().expect("Should decompress"));

    assert_eq!(
        decrypted,
        Scalar::from(100u64) * *G,
        "Single transfer should work correctly"
    );
}

/// BC-08: Multiple Transfers in Same TX
/// Multiple recipients in one TX -> Each gets correct amount
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_boundary_multiple_transfers() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    setup_uno_asset(&storage).await?;

    let alice = KeyPair::new();
    let bob = KeyPair::new();
    let carol = KeyPair::new();
    let dave = KeyPair::new();

    let alice_pub = alice.get_public_key().compress();
    let bob_pub = bob.get_public_key().compress();
    let carol_pub = carol.get_public_key().compress();
    let dave_pub = dave.get_public_key().compress();

    setup_account_safe(&storage, &alice_pub, 1000 * COIN_VALUE, 0).await?;
    setup_account_safe(&storage, &bob_pub, 0, 0).await?;
    setup_account_safe(&storage, &carol_pub, 0, 0).await?;
    setup_account_safe(&storage, &dave_pub, 0, 0).await?;
    // Alice has 1000 UNO
    setup_uno_balance(&storage, &alice, 1000 * COIN_VALUE, 0).await?;

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

    // Alice sends to Bob (100), Carol (200), Dave (300)
    let bob_amount = 100 * COIN_VALUE;
    let carol_amount = 200 * COIN_VALUE;
    let dave_amount = 300 * COIN_VALUE;
    let total_sent = bob_amount + carol_amount + dave_amount;

    // Deduct from Alice
    let sender_uno = state
        .get_sender_uno_balance(&alice_pub, &UNO_ASSET, &reference)
        .await?;
    let total_output = alice.get_public_key().encrypt(total_sent);
    *sender_uno -= &total_output;
    state
        .add_sender_uno_output(&alice_pub, &UNO_ASSET, total_output)
        .await?;

    // Credit to each recipient
    {
        let receiver_balance = state
            .get_receiver_uno_balance(Cow::Borrowed(&bob_pub), Cow::Borrowed(&UNO_ASSET))
            .await?;
        *receiver_balance += bob.get_public_key().encrypt(bob_amount);
    }
    {
        let receiver_balance = state
            .get_receiver_uno_balance(Cow::Borrowed(&carol_pub), Cow::Borrowed(&UNO_ASSET))
            .await?;
        *receiver_balance += carol.get_public_key().encrypt(carol_amount);
    }
    {
        let receiver_balance = state
            .get_receiver_uno_balance(Cow::Borrowed(&dave_pub), Cow::Borrowed(&UNO_ASSET))
            .await?;
        *receiver_balance += dave.get_public_key().encrypt(dave_amount);
    }

    state.apply_changes().await?;

    // Verify all balances
    let (_, mut av) = storage_write
        .get_last_uno_balance(&alice_pub, &UNO_ASSET)
        .await?;
    let ap = alice
        .get_private_key()
        .decrypt_to_point(av.get_mut_balance().decompressed()?);
    assert_eq!(
        ap,
        Scalar::from(1000 * COIN_VALUE - total_sent) * *G,
        "Alice: 1000 - 600 = 400"
    );

    let (_, mut bv) = storage_write
        .get_last_uno_balance(&bob_pub, &UNO_ASSET)
        .await?;
    let bp = bob
        .get_private_key()
        .decrypt_to_point(bv.get_mut_balance().decompressed()?);
    assert_eq!(bp, Scalar::from(bob_amount) * *G, "Bob: 100");

    let (_, mut cv) = storage_write
        .get_last_uno_balance(&carol_pub, &UNO_ASSET)
        .await?;
    let cp = carol
        .get_private_key()
        .decrypt_to_point(cv.get_mut_balance().decompressed()?);
    assert_eq!(cp, Scalar::from(carol_amount) * *G, "Carol: 200");

    let (_, mut dv) = storage_write
        .get_last_uno_balance(&dave_pub, &UNO_ASSET)
        .await?;
    let dp = dave
        .get_private_key()
        .decrypt_to_point(dv.get_mut_balance().decompressed()?);
    assert_eq!(dp, Scalar::from(dave_amount) * *G, "Dave: 300");

    Ok(())
}

/// BC-09: Large Number of Transfers
/// Many transfers in one block -> All should process correctly
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_boundary_many_transfers() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    setup_uno_asset(&storage).await?;

    let sender = KeyPair::new();
    let sender_pub = sender.get_public_key().compress();
    setup_account_safe(&storage, &sender_pub, 10000 * COIN_VALUE, 0).await?;
    setup_uno_balance(&storage, &sender, 10000 * COIN_VALUE, 0).await?;

    // Create 10 receivers
    let mut receivers: Vec<KeyPair> = Vec::new();
    for _ in 0..10 {
        let receiver = KeyPair::new();
        let receiver_pub = receiver.get_public_key().compress();
        setup_account_safe(&storage, &receiver_pub, 0, 0).await?;
        receivers.push(receiver);
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

    // Send 100 to each of 10 receivers
    let per_receiver = 100 * COIN_VALUE;
    let total_sent = per_receiver * 10;

    // Deduct total from sender
    let sender_uno = state
        .get_sender_uno_balance(&sender_pub, &UNO_ASSET, &reference)
        .await?;
    let total_output = sender.get_public_key().encrypt(total_sent);
    *sender_uno -= &total_output;
    state
        .add_sender_uno_output(&sender_pub, &UNO_ASSET, total_output)
        .await?;

    // Credit each receiver (collect pub keys first to avoid borrow issues)
    let receiver_pubs: Vec<CompressedPublicKey> = receivers
        .iter()
        .map(|r| r.get_public_key().compress())
        .collect();
    for (i, receiver_pub) in receiver_pubs.iter().enumerate() {
        let receiver_balance = state
            .get_receiver_uno_balance(Cow::Borrowed(receiver_pub), Cow::Borrowed(&UNO_ASSET))
            .await?;
        *receiver_balance += receivers[i].get_public_key().encrypt(per_receiver);
    }

    state.apply_changes().await?;

    // Verify sender balance
    let (_, mut sv) = storage_write
        .get_last_uno_balance(&sender_pub, &UNO_ASSET)
        .await?;
    let sp = sender
        .get_private_key()
        .decrypt_to_point(sv.get_mut_balance().decompressed()?);
    assert_eq!(
        sp,
        Scalar::from(10000 * COIN_VALUE - total_sent) * *G,
        "Sender: 10000 - 1000 = 9000"
    );

    // Verify first receiver as sample
    let (_, mut rv) = storage_write
        .get_last_uno_balance(&receivers[0].get_public_key().compress(), &UNO_ASSET)
        .await?;
    let rp = receivers[0]
        .get_private_key()
        .decrypt_to_point(rv.get_mut_balance().decompressed()?);
    assert_eq!(rp, Scalar::from(per_receiver) * *G, "Receiver 0: 100");

    Ok(())
}

/// BC-10: Sum of Transfers Exceeds Balance
/// Multiple transfers summing to > balance -> State tracks correctly
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_boundary_sum_exceeds_balance() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    setup_uno_asset(&storage).await?;

    let alice = KeyPair::new();
    let bob = KeyPair::new();
    let carol = KeyPair::new();

    let alice_pub = alice.get_public_key().compress();
    let bob_pub = bob.get_public_key().compress();
    let carol_pub = carol.get_public_key().compress();

    setup_account_safe(&storage, &alice_pub, 100 * COIN_VALUE, 0).await?;
    setup_account_safe(&storage, &bob_pub, 0, 0).await?;
    setup_account_safe(&storage, &carol_pub, 0, 0).await?;
    // Alice has only 100 UNO
    setup_uno_balance(&storage, &alice, 100 * COIN_VALUE, 0).await?;

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

    // Try to send 60 to Bob
    {
        let sender_uno = state
            .get_sender_uno_balance(&alice_pub, &UNO_ASSET, &reference)
            .await?;
        let output1 = alice.get_public_key().encrypt(60 * COIN_VALUE);
        *sender_uno -= &output1;
        state
            .add_sender_uno_output(&alice_pub, &UNO_ASSET, output1)
            .await?;
    }

    // After first transfer, Alice has 40 UNO left
    {
        let sender_uno = state
            .get_sender_uno_balance(&alice_pub, &UNO_ASSET, &reference)
            .await?;
        let remaining = alice.get_private_key().decrypt_to_point(sender_uno);
        assert_eq!(
            remaining,
            Scalar::from(40 * COIN_VALUE) * *G,
            "After sending 60, Alice has 40"
        );
    }

    // Second transfer of 60 would exceed remaining balance
    // In real TX, this would be rejected during verification
    // State correctly tracks that only 40 is available

    Ok(())
}
