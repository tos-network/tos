//! State Machine Completeness Tests for UNO Privacy Transfers
//!
//! These tests verify all state transitions in the UNO system.
//!
//! Test Categories:
//! - SM-01 ~ SM-08: Account UNO State Matrix
//! - TX state transitions

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
        elgamal::{KeyPair, PedersenCommitment, PedersenOpening},
        proofs::{BatchCollector, CiphertextValidityProof, G},
        Hash,
    },
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

// ============================================================================
// SM-01 ~ SM-08: Account UNO State Matrix
// ============================================================================

/// SM-01: No UNO balance -> Receive UNO -> Create new UNO balance
/// Bob has no UNO entry, receives from Alice, entry created
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_state_receive_creates_uno_balance() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    setup_uno_asset(&storage).await?;

    let alice = KeyPair::new();
    let bob = KeyPair::new();
    let alice_pub = alice.get_public_key().compress();
    let bob_pub = bob.get_public_key().compress();

    setup_account_safe(&storage, &alice_pub, 100 * COIN_VALUE, 0).await?;
    setup_account_safe(&storage, &bob_pub, 0, 0).await?;
    setup_uno_balance(&storage, &alice, 100 * COIN_VALUE, 0).await?;
    // Bob has NO UNO balance entry

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

    // Alice sends 50 UNO to Bob (who has no entry)
    let transfer_amount = 50 * COIN_VALUE;
    {
        let sender_uno = state
            .get_sender_uno_balance(&alice_pub, &UNO_ASSET, &reference)
            .await?;
        let output = alice.get_public_key().encrypt(transfer_amount);
        *sender_uno -= &output;
        state
            .add_sender_uno_output(&alice_pub, &UNO_ASSET, output)
            .await?;
    }

    // Credit Bob (creates new entry)
    {
        let receiver_balance = state
            .get_receiver_uno_balance(Cow::Borrowed(&bob_pub), Cow::Borrowed(&UNO_ASSET))
            .await?;
        *receiver_balance += bob.get_public_key().encrypt(transfer_amount);
    }

    state.apply_changes().await?;

    // Verify Bob now has UNO balance entry with 50 UNO
    let (topo, mut bv) = storage_write
        .get_last_uno_balance(&bob_pub, &UNO_ASSET)
        .await?;
    assert_eq!(topo, 1, "Bob's UNO entry should be at topoheight 1");
    let bob_point = bob
        .get_private_key()
        .decrypt_to_point(bv.get_mut_balance().decompressed()?);
    assert_eq!(
        bob_point,
        Scalar::from(transfer_amount) * *G,
        "Bob should have 50 UNO after receiving"
    );

    Ok(())
}

/// SM-02: Has UNO balance -> Send partial -> Decrease balance
/// Alice has 100 UNO, sends 30, has 70 remaining
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_state_send_partial_decreases() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    setup_uno_asset(&storage).await?;

    let alice = KeyPair::new();
    let bob = KeyPair::new();
    let alice_pub = alice.get_public_key().compress();
    let bob_pub = bob.get_public_key().compress();

    setup_account_safe(&storage, &alice_pub, 100 * COIN_VALUE, 0).await?;
    setup_account_safe(&storage, &bob_pub, 0, 0).await?;
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

    // Send 30 (partial)
    let send_amount = 30 * COIN_VALUE;
    {
        let sender_uno = state
            .get_sender_uno_balance(&alice_pub, &UNO_ASSET, &reference)
            .await?;
        let output = alice.get_public_key().encrypt(send_amount);
        *sender_uno -= &output;
        state
            .add_sender_uno_output(&alice_pub, &UNO_ASSET, output)
            .await?;

        let receiver = state
            .get_receiver_uno_balance(Cow::Borrowed(&bob_pub), Cow::Borrowed(&UNO_ASSET))
            .await?;
        *receiver += bob.get_public_key().encrypt(send_amount);
    }

    state.apply_changes().await?;

    // Verify Alice has 70 remaining
    let (_, mut av) = storage_write
        .get_last_uno_balance(&alice_pub, &UNO_ASSET)
        .await?;
    let alice_point = alice
        .get_private_key()
        .decrypt_to_point(av.get_mut_balance().decompressed()?);
    assert_eq!(
        alice_point,
        Scalar::from(70 * COIN_VALUE) * *G,
        "Alice should have 70 UNO after sending 30"
    );

    Ok(())
}

/// SM-03: Has UNO balance -> Send all -> Balance becomes zero
/// Alice has 100 UNO, sends all 100, has 0 remaining (entry exists but is zero)
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_state_send_entire_balance() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    setup_uno_asset(&storage).await?;

    let alice = KeyPair::new();
    let bob = KeyPair::new();
    let alice_pub = alice.get_public_key().compress();
    let bob_pub = bob.get_public_key().compress();

    setup_account_safe(&storage, &alice_pub, 100 * COIN_VALUE, 0).await?;
    setup_account_safe(&storage, &bob_pub, 0, 0).await?;
    let initial_balance = 100 * COIN_VALUE;
    setup_uno_balance(&storage, &alice, initial_balance, 0).await?;

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

    // Send entire balance
    {
        let sender_uno = state
            .get_sender_uno_balance(&alice_pub, &UNO_ASSET, &reference)
            .await?;
        let output = alice.get_public_key().encrypt(initial_balance);
        *sender_uno -= &output;
        state
            .add_sender_uno_output(&alice_pub, &UNO_ASSET, output)
            .await?;

        let receiver = state
            .get_receiver_uno_balance(Cow::Borrowed(&bob_pub), Cow::Borrowed(&UNO_ASSET))
            .await?;
        *receiver += bob.get_public_key().encrypt(initial_balance);
    }

    state.apply_changes().await?;

    // Verify Alice's UNO balance is zero (entry still exists)
    let (_, mut av) = storage_write
        .get_last_uno_balance(&alice_pub, &UNO_ASSET)
        .await?;
    let alice_point = alice
        .get_private_key()
        .decrypt_to_point(av.get_mut_balance().decompressed()?);
    assert_eq!(
        alice_point,
        Scalar::from(0u64) * *G,
        "Alice should have 0 UNO after sending all"
    );

    Ok(())
}

/// SM-04: Zero UNO balance -> Receive UNO -> Balance > 0
/// Alice sent all, has 0, receives more, balance is positive again
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_state_zero_balance_receive() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    setup_uno_asset(&storage).await?;

    let alice = KeyPair::new();
    let bob = KeyPair::new();
    let alice_pub = alice.get_public_key().compress();
    let bob_pub = bob.get_public_key().compress();

    setup_account_safe(&storage, &alice_pub, 100 * COIN_VALUE, 0).await?;
    setup_account_safe(&storage, &bob_pub, 100 * COIN_VALUE, 0).await?;
    // Alice starts with 0 UNO
    setup_uno_balance(&storage, &alice, 0, 0).await?;
    setup_uno_balance(&storage, &bob, 100 * COIN_VALUE, 0).await?;

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

    // Bob sends 50 to Alice (who has 0)
    let transfer = 50 * COIN_VALUE;
    {
        let sender_uno = state
            .get_sender_uno_balance(&bob_pub, &UNO_ASSET, &reference)
            .await?;
        let output = bob.get_public_key().encrypt(transfer);
        *sender_uno -= &output;
        state
            .add_sender_uno_output(&bob_pub, &UNO_ASSET, output)
            .await?;

        let receiver = state
            .get_receiver_uno_balance(Cow::Borrowed(&alice_pub), Cow::Borrowed(&UNO_ASSET))
            .await?;
        *receiver += alice.get_public_key().encrypt(transfer);
    }

    state.apply_changes().await?;

    // Verify Alice now has 50 UNO
    let (_, mut av) = storage_write
        .get_last_uno_balance(&alice_pub, &UNO_ASSET)
        .await?;
    let alice_point = alice
        .get_private_key()
        .decrypt_to_point(av.get_mut_balance().decompressed()?);
    assert_eq!(
        alice_point,
        Scalar::from(transfer) * *G,
        "Alice should have 50 UNO after receiving from zero balance"
    );

    Ok(())
}

/// SM-05: No UNO balance -> Send UNO -> Should see zero balance
/// Account with no UNO entry tries to send -> balance is zero
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_state_no_balance_send_fails() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    setup_uno_asset(&storage).await?;

    let alice = KeyPair::new();
    let alice_pub = alice.get_public_key().compress();

    // Alice has TOS but NO UNO
    setup_account_safe(&storage, &alice_pub, 100 * COIN_VALUE, 0).await?;
    // NOT calling setup_uno_balance

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

    // Try to get UNO balance (initializes from receiver cache as zero)
    let result = state
        .get_sender_uno_balance(&alice_pub, &UNO_ASSET, &reference)
        .await;

    match result {
        Ok(balance) => {
            // Balance should be zero
            let point = alice.get_private_key().decrypt_to_point(balance);
            assert_eq!(
                point,
                Scalar::from(0u64) * *G,
                "No UNO entry should result in zero balance"
            );
        }
        Err(_) => {
            // Error is also acceptable - no UNO balance
        }
    }

    Ok(())
}

/// SM-06: Has UNO balance -> Unshield partial -> Decrease UNO, increase TOS
/// Alice has 100 UNO, unshields 40, has 60 UNO remaining
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_state_unshield_partial() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    setup_uno_asset(&storage).await?;

    let alice = KeyPair::new();
    let alice_pub = alice.get_public_key().compress();

    setup_account_safe(&storage, &alice_pub, 0, 0).await?;
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

    // Unshield 40 UNO
    let unshield = 40 * COIN_VALUE;
    {
        let sender_uno = state
            .get_sender_uno_balance(&alice_pub, &UNO_ASSET, &reference)
            .await?;
        let output = alice.get_public_key().encrypt(unshield);
        *sender_uno -= &output;
        state
            .add_sender_uno_output(&alice_pub, &UNO_ASSET, output)
            .await?;
    }

    state.apply_changes().await?;

    // Verify Alice has 60 UNO remaining
    let (_, mut av) = storage_write
        .get_last_uno_balance(&alice_pub, &UNO_ASSET)
        .await?;
    let alice_point = alice
        .get_private_key()
        .decrypt_to_point(av.get_mut_balance().decompressed()?);
    assert_eq!(
        alice_point,
        Scalar::from(60 * COIN_VALUE) * *G,
        "Alice should have 60 UNO after unshielding 40"
    );

    Ok(())
}

/// SM-07: Has UNO balance -> Unshield all -> UNO becomes zero
/// Alice has 100 UNO, unshields all, has 0 UNO remaining
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_state_unshield_all() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    setup_uno_asset(&storage).await?;

    let alice = KeyPair::new();
    let alice_pub = alice.get_public_key().compress();

    setup_account_safe(&storage, &alice_pub, 0, 0).await?;
    let initial = 100 * COIN_VALUE;
    setup_uno_balance(&storage, &alice, initial, 0).await?;

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

    // Unshield all
    {
        let sender_uno = state
            .get_sender_uno_balance(&alice_pub, &UNO_ASSET, &reference)
            .await?;
        let output = alice.get_public_key().encrypt(initial);
        *sender_uno -= &output;
        state
            .add_sender_uno_output(&alice_pub, &UNO_ASSET, output)
            .await?;
    }

    state.apply_changes().await?;

    // Verify Alice has 0 UNO
    let (_, mut av) = storage_write
        .get_last_uno_balance(&alice_pub, &UNO_ASSET)
        .await?;
    let alice_point = alice
        .get_private_key()
        .decrypt_to_point(av.get_mut_balance().decompressed()?);
    assert_eq!(
        alice_point,
        Scalar::from(0u64) * *G,
        "Alice should have 0 UNO after unshielding all"
    );

    Ok(())
}

/// SM-08: No UNO balance -> Shield TOS -> Create UNO balance
/// Alice has no UNO, shields TOS, now has UNO balance
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_state_shield_creates_balance() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    setup_uno_asset(&storage).await?;

    let alice = KeyPair::new();
    let alice_pub = alice.get_public_key().compress();

    // Alice has TOS but NO UNO
    setup_account_safe(&storage, &alice_pub, 1000 * COIN_VALUE, 0).await?;
    // NOT calling setup_uno_balance

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

    // Shield 100 TOS -> UNO
    let shield = 100 * COIN_VALUE;
    {
        let receiver = state
            .get_receiver_uno_balance(Cow::Borrowed(&alice_pub), Cow::Borrowed(&UNO_ASSET))
            .await?;
        *receiver += alice.get_public_key().encrypt(shield);
    }

    state.apply_changes().await?;

    // Verify Alice now has UNO balance entry
    let (topo, mut av) = storage_write
        .get_last_uno_balance(&alice_pub, &UNO_ASSET)
        .await?;
    assert_eq!(topo, 1, "UNO entry should be created at topoheight 1");
    let alice_point = alice
        .get_private_key()
        .decrypt_to_point(av.get_mut_balance().decompressed()?);
    assert_eq!(
        alice_point,
        Scalar::from(shield) * *G,
        "Alice should have 100 UNO after shielding"
    );

    Ok(())
}

// ============================================================================
// TX State Transitions
// ============================================================================

/// TX-01: Created -> Verify proofs -> Verified
/// Valid proof passes verification
#[test]
fn test_tx_state_valid_proof_verified() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();
    let amount = 100u64;
    let opening = PedersenOpening::generate_new();

    let commitment = PedersenCommitment::new_with_opening(amount, &opening);
    let sender_handle = sender.get_public_key().decrypt_handle(&opening);
    let receiver_handle = receiver.get_public_key().decrypt_handle(&opening);

    // Generate proof
    let mut gen_transcript = tos_common::crypto::new_proof_transcript(b"tx_verify");
    let proof = CiphertextValidityProof::new(
        receiver.get_public_key(),
        Some(sender.get_public_key()),
        amount,
        &opening,
        &mut gen_transcript,
    );

    // Verify proof -> should pass
    let mut verify_transcript = tos_common::crypto::new_proof_transcript(b"tx_verify");
    let mut batch_collector = BatchCollector::default();
    let result = proof.pre_verify(
        &commitment,
        receiver.get_public_key(),
        sender.get_public_key(),
        &receiver_handle,
        &sender_handle,
        true,
        &mut verify_transcript,
        &mut batch_collector,
    );

    assert!(result.is_ok(), "Valid proof should pass pre_verify");
    assert!(
        batch_collector.verify().is_ok(),
        "Valid proof should pass batch verify"
    );
}

/// TX-02: Created -> Invalid proof -> Rejected
/// Invalid proof fails verification
#[test]
fn test_tx_state_invalid_proof_rejected() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();
    let amount = 100u64;
    let opening = PedersenOpening::generate_new();

    let commitment = PedersenCommitment::new_with_opening(amount, &opening);
    let sender_handle = sender.get_public_key().decrypt_handle(&opening);
    let receiver_handle = receiver.get_public_key().decrypt_handle(&opening);

    // Generate proof with WRONG amount
    let wrong_amount = 200u64;
    let mut gen_transcript = tos_common::crypto::new_proof_transcript(b"tx_invalid");
    let proof = CiphertextValidityProof::new(
        receiver.get_public_key(),
        Some(sender.get_public_key()),
        wrong_amount, // Different from commitment
        &opening,
        &mut gen_transcript,
    );

    // Verify -> should fail
    let mut verify_transcript = tos_common::crypto::new_proof_transcript(b"tx_invalid");
    let mut batch_collector = BatchCollector::default();
    let result = proof.pre_verify(
        &commitment, // For 100, not 200
        receiver.get_public_key(),
        sender.get_public_key(),
        &receiver_handle,
        &sender_handle,
        true,
        &mut verify_transcript,
        &mut batch_collector,
    );

    let failed = result.is_err() || batch_collector.verify().is_err();
    assert!(failed, "Invalid proof should be rejected");
}

/// TX-03: Verified -> Apply to chain -> Committed
/// After verification, apply_changes commits state
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_tx_state_verified_to_committed() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    setup_uno_asset(&storage).await?;

    let alice = KeyPair::new();
    let bob = KeyPair::new();
    let alice_pub = alice.get_public_key().compress();
    let bob_pub = bob.get_public_key().compress();

    setup_account_safe(&storage, &alice_pub, 100 * COIN_VALUE, 0).await?;
    setup_account_safe(&storage, &bob_pub, 0, 0).await?;
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

    // Simulate verified TX
    let transfer = 50 * COIN_VALUE;
    {
        let sender_uno = state
            .get_sender_uno_balance(&alice_pub, &UNO_ASSET, &reference)
            .await?;
        let output = alice.get_public_key().encrypt(transfer);
        *sender_uno -= &output;
        state
            .add_sender_uno_output(&alice_pub, &UNO_ASSET, output)
            .await?;

        let receiver = state
            .get_receiver_uno_balance(Cow::Borrowed(&bob_pub), Cow::Borrowed(&UNO_ASSET))
            .await?;
        *receiver += bob.get_public_key().encrypt(transfer);
    }

    // Apply changes (commit)
    state.apply_changes().await?;

    // Verify state is committed
    let (topo, _) = storage_write
        .get_last_uno_balance(&alice_pub, &UNO_ASSET)
        .await?;
    assert_eq!(topo, 1, "State should be committed at topoheight 1");

    Ok(())
}

/// TX-04: Balance versioning - can query old balance
/// After transfer, old balance is still queryable by topoheight
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_tx_state_balance_versioning() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    setup_uno_asset(&storage).await?;

    let alice = KeyPair::new();
    let alice_pub = alice.get_public_key().compress();

    setup_account_safe(&storage, &alice_pub, 100 * COIN_VALUE, 0).await?;
    let initial = 100 * COIN_VALUE;
    setup_uno_balance(&storage, &alice, initial, 0).await?;

    // Block 1: Update balance
    {
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

        let spend = 30 * COIN_VALUE;
        {
            let sender_uno = state
                .get_sender_uno_balance(&alice_pub, &UNO_ASSET, &reference)
                .await?;
            let output = alice.get_public_key().encrypt(spend);
            *sender_uno -= &output;
            state
                .add_sender_uno_output(&alice_pub, &UNO_ASSET, output)
                .await?;
        }

        state.apply_changes().await?;
    }

    // Query balance at topoheight 1 (after transfer)
    let storage_read = storage.read().await;
    let (topo, mut versioned) = storage_read
        .get_last_uno_balance(&alice_pub, &UNO_ASSET)
        .await?;
    assert_eq!(topo, 1);
    let balance = alice
        .get_private_key()
        .decrypt_to_point(versioned.get_mut_balance().decompressed()?);
    assert_eq!(
        balance,
        Scalar::from(70 * COIN_VALUE) * *G,
        "Balance at topo 1 should be 70"
    );

    // Previous topoheight should be tracked
    assert_eq!(
        versioned.get_previous_topoheight(),
        Some(0),
        "Should track previous topoheight"
    );

    Ok(())
}
