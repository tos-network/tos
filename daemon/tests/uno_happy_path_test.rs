//! Happy Path Tests for UNO Privacy Transfers
//!
//! These tests verify normal, expected functionality.
//!
//! Test Categories:
//! - HP-U01 ~ HP-U04: UNO Transfer (UNO -> UNO)
//! - HP-S01 ~ HP-S04: Shield Transfer (TOS -> UNO)
//! - HP-US01 ~ HP-US04: Unshield Transfer (UNO -> TOS)

mod common;

use std::borrow::Cow;
use std::sync::Arc;

use common::{create_dummy_block, create_test_storage, setup_account_safe};
use tos_common::{
    account::{CiphertextCache, VersionedUnoBalance},
    asset::{AssetData, VersionedAssetData},
    block::BlockVersion,
    config::{COIN_DECIMALS, COIN_VALUE, TOS_ASSET, UNO_ASSET},
    crypto::{
        elgamal::{KeyPair, PedersenCommitment, PedersenOpening},
        proofs::{CiphertextValidityProof, G},
        Hash,
    },
    transaction::{
        verify::BlockchainVerificationState, Reference, Role, UnoTransferPayload,
        UnshieldTransferPayload,
    },
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
// HP-U01 ~ HP-U04: UNO Transfer (UNO -> UNO)
// ============================================================================

/// HP-U01: Single UNO transfer from Alice to Bob
/// Alice sends 30 UNO to Bob, verify both balances are correct
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_happy_uno_transfer_single() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    setup_uno_asset(&storage).await?;

    let alice = KeyPair::new();
    let bob = KeyPair::new();
    let alice_pub = alice.get_public_key().compress();
    let bob_pub = bob.get_public_key().compress();

    setup_account_safe(&storage, &alice_pub, 100 * COIN_VALUE, 0).await?;
    setup_account_safe(&storage, &bob_pub, 0, 0).await?;
    // Alice has 100 UNO, Bob has 0 UNO
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

    // Transfer 30 UNO from Alice to Bob
    let transfer_amount = 30 * COIN_VALUE;
    {
        let sender_uno = state
            .get_sender_uno_balance(&alice_pub, &UNO_ASSET, &reference)
            .await?;
        let output = alice.get_public_key().encrypt(transfer_amount);
        *sender_uno -= &output;
        state
            .add_sender_uno_output(&alice_pub, &UNO_ASSET, output)
            .await?;

        let receiver_balance = state
            .get_receiver_uno_balance(Cow::Borrowed(&bob_pub), Cow::Borrowed(&UNO_ASSET))
            .await?;
        *receiver_balance += bob.get_public_key().encrypt(transfer_amount);
    }

    state.apply_changes().await?;

    // Verify Alice has 70 UNO
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

    // Verify Bob has 30 UNO
    let (_, mut bv) = storage_write
        .get_last_uno_balance(&bob_pub, &UNO_ASSET)
        .await?;
    let bob_point = bob
        .get_private_key()
        .decrypt_to_point(bv.get_mut_balance().decompressed()?);
    assert_eq!(
        bob_point,
        Scalar::from(30 * COIN_VALUE) * *G,
        "Bob should have 30 UNO"
    );

    Ok(())
}

/// HP-U02: Multiple UNO transfers to different recipients
/// Alice sends 100 to Bob, 200 to Carol, 300 to Dave in same block
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_happy_uno_transfer_multiple_recipients() -> Result<(), BlockchainError> {
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

    let bob_amount = 100 * COIN_VALUE;
    let carol_amount = 200 * COIN_VALUE;
    let dave_amount = 300 * COIN_VALUE;
    let total_sent = bob_amount + carol_amount + dave_amount;

    // Deduct from Alice
    {
        let sender_uno = state
            .get_sender_uno_balance(&alice_pub, &UNO_ASSET, &reference)
            .await?;
        let total_output = alice.get_public_key().encrypt(total_sent);
        *sender_uno -= &total_output;
        state
            .add_sender_uno_output(&alice_pub, &UNO_ASSET, total_output)
            .await?;
    }

    // Credit recipients
    {
        let rb = state
            .get_receiver_uno_balance(Cow::Borrowed(&bob_pub), Cow::Borrowed(&UNO_ASSET))
            .await?;
        *rb += bob.get_public_key().encrypt(bob_amount);
    }
    {
        let rc = state
            .get_receiver_uno_balance(Cow::Borrowed(&carol_pub), Cow::Borrowed(&UNO_ASSET))
            .await?;
        *rc += carol.get_public_key().encrypt(carol_amount);
    }
    {
        let rd = state
            .get_receiver_uno_balance(Cow::Borrowed(&dave_pub), Cow::Borrowed(&UNO_ASSET))
            .await?;
        *rd += dave.get_public_key().encrypt(dave_amount);
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

/// HP-U03: Self UNO transfer (Alice to Alice)
/// Alice sends UNO to herself, balance should remain unchanged
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_happy_uno_transfer_self() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    setup_uno_asset(&storage).await?;

    let alice = KeyPair::new();
    let alice_pub = alice.get_public_key().compress();

    setup_account_safe(&storage, &alice_pub, 100 * COIN_VALUE, 0).await?;
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

    // Transfer 50 UNO from Alice to Alice
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

        // Credit to same account
        let receiver_balance = state
            .get_receiver_uno_balance(Cow::Borrowed(&alice_pub), Cow::Borrowed(&UNO_ASSET))
            .await?;
        *receiver_balance += alice.get_public_key().encrypt(transfer_amount);
    }

    state.apply_changes().await?;

    // Verify Alice still has 100 UNO (net effect is 0)
    let (_, mut av) = storage_write
        .get_last_uno_balance(&alice_pub, &UNO_ASSET)
        .await?;
    let alice_point = alice
        .get_private_key()
        .decrypt_to_point(av.get_mut_balance().decompressed()?);
    assert_eq!(
        alice_point,
        Scalar::from(100 * COIN_VALUE) * *G,
        "Self-transfer: balance unchanged at 100 UNO"
    );

    Ok(())
}

/// HP-U04: UNO transfer with payload verification
/// Verify that UnoTransferPayload structure is correct
#[test]
fn test_happy_uno_transfer_payload() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();
    let amount = 100u64;

    let payload = create_test_uno_payload(&sender, &receiver, amount);

    // Verify payload structure
    assert_eq!(payload.get_asset(), &UNO_ASSET);
    assert_eq!(
        payload.get_destination(),
        &receiver.get_public_key().compress()
    );

    // Verify sender can decrypt sender ciphertext
    let sender_ct = payload.get_ciphertext(Role::Sender);
    let sender_decrypted = sender
        .get_private_key()
        .decrypt_to_point(&sender_ct.decompress().expect("Should decompress"));
    assert_eq!(sender_decrypted, Scalar::from(amount) * *G);

    // Verify receiver can decrypt receiver ciphertext
    let receiver_ct = payload.get_ciphertext(Role::Receiver);
    let receiver_decrypted = receiver
        .get_private_key()
        .decrypt_to_point(&receiver_ct.decompress().expect("Should decompress"));
    assert_eq!(receiver_decrypted, Scalar::from(amount) * *G);
}

// ============================================================================
// HP-S01 ~ HP-S04: Shield Transfer (TOS -> UNO)
// ============================================================================

/// HP-S01: Shield TOS to self's UNO
/// Alice shields 100 TOS to her own UNO account
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_happy_shield_to_self() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    setup_uno_asset(&storage).await?;

    let alice = KeyPair::new();
    let alice_pub = alice.get_public_key().compress();

    // Alice has 1000 TOS, 0 UNO
    setup_account_safe(&storage, &alice_pub, 1000 * COIN_VALUE, 0).await?;

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

    // Shield 100 TOS to UNO
    let shield_amount = 100 * COIN_VALUE;
    {
        let receiver_balance = state
            .get_receiver_uno_balance(Cow::Borrowed(&alice_pub), Cow::Borrowed(&UNO_ASSET))
            .await?;
        *receiver_balance += alice.get_public_key().encrypt(shield_amount);
    }

    state.apply_changes().await?;

    // Verify Alice has 100 UNO
    let (_, mut av) = storage_write
        .get_last_uno_balance(&alice_pub, &UNO_ASSET)
        .await?;
    let alice_point = alice
        .get_private_key()
        .decrypt_to_point(av.get_mut_balance().decompressed()?);
    assert_eq!(
        alice_point,
        Scalar::from(shield_amount) * *G,
        "Alice should have 100 UNO after shielding"
    );

    Ok(())
}

/// HP-S02: Shield TOS to another's UNO
/// Alice shields 100 TOS to Bob's UNO account
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_happy_shield_to_other() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    setup_uno_asset(&storage).await?;

    let alice = KeyPair::new();
    let bob = KeyPair::new();
    let alice_pub = alice.get_public_key().compress();
    let bob_pub = bob.get_public_key().compress();

    setup_account_safe(&storage, &alice_pub, 1000 * COIN_VALUE, 0).await?;
    setup_account_safe(&storage, &bob_pub, 0, 0).await?;

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

    // Alice shields 100 TOS to Bob's UNO
    let shield_amount = 100 * COIN_VALUE;
    {
        let receiver_balance = state
            .get_receiver_uno_balance(Cow::Borrowed(&bob_pub), Cow::Borrowed(&UNO_ASSET))
            .await?;
        // Note: The ciphertext is encrypted with Bob's key
        *receiver_balance += bob.get_public_key().encrypt(shield_amount);
    }

    state.apply_changes().await?;

    // Verify Bob has 100 UNO
    let (_, mut bv) = storage_write
        .get_last_uno_balance(&bob_pub, &UNO_ASSET)
        .await?;
    let bob_point = bob
        .get_private_key()
        .decrypt_to_point(bv.get_mut_balance().decompressed()?);
    assert_eq!(
        bob_point,
        Scalar::from(shield_amount) * *G,
        "Bob should have 100 UNO from Alice's shield"
    );

    // Only Bob can decrypt, not Alice (privacy!)

    Ok(())
}

/// HP-S03: Multiple shield operations in sequence
/// Alice shields 50, then 30, then 20 TOS
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_happy_shield_sequence() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    setup_uno_asset(&storage).await?;

    let alice = KeyPair::new();
    let alice_pub = alice.get_public_key().compress();

    setup_account_safe(&storage, &alice_pub, 1000 * COIN_VALUE, 0).await?;

    let amounts = [50 * COIN_VALUE, 30 * COIN_VALUE, 20 * COIN_VALUE];
    let expected_total: u64 = amounts.iter().sum();

    // Shield in multiple blocks
    for (i, &amount) in amounts.iter().enumerate() {
        let (block, block_hash) = create_dummy_block();
        let executor = Arc::new(TakoContractExecutor::new());
        let environment = Environment::new();

        let mut storage_write = storage.write().await;
        let mut state = ApplicableChainState::new(
            &mut *storage_write,
            &environment,
            i as u64,
            (i + 1) as u64,
            BlockVersion::Nobunaga,
            0,
            &block_hash,
            &block,
            executor,
        );

        let receiver_balance = state
            .get_receiver_uno_balance(Cow::Borrowed(&alice_pub), Cow::Borrowed(&UNO_ASSET))
            .await?;
        *receiver_balance += alice.get_public_key().encrypt(amount);

        state.apply_changes().await?;
    }

    // Verify total UNO balance
    let storage_read = storage.read().await;
    let (_, mut av) = storage_read
        .get_last_uno_balance(&alice_pub, &UNO_ASSET)
        .await?;
    let alice_point = alice
        .get_private_key()
        .decrypt_to_point(av.get_mut_balance().decompressed()?);
    assert_eq!(
        alice_point,
        Scalar::from(expected_total) * *G,
        "Alice should have 100 UNO after 50+30+20 shields"
    );

    Ok(())
}

/// HP-S04: Shield commitment and handle creation
/// Verify shield cryptographic components are valid
#[test]
fn test_happy_shield_commitment() {
    let receiver = KeyPair::new();
    let shield_amount = 100u64;
    let opening = PedersenOpening::generate_new();

    // Create commitment and handle for shield
    let commitment = PedersenCommitment::new_with_opening(shield_amount, &opening);
    let receiver_handle = receiver.get_public_key().decrypt_handle(&opening);

    // Verify commitment can be decompressed
    let compressed = commitment.compress();
    assert!(compressed.decompress().is_ok());

    // Verify handle can be decompressed
    let compressed_handle = receiver_handle.compress();
    assert!(compressed_handle.decompress().is_ok());

    // Create ciphertext and verify receiver can decrypt
    let ct_commitment = compressed.decompress().expect("Should decompress");
    let ct_handle = compressed_handle.decompress().expect("Should decompress");
    let ciphertext = tos_common::crypto::elgamal::Ciphertext::new(ct_commitment, ct_handle);

    let decrypted = receiver.get_private_key().decrypt_to_point(&ciphertext);
    assert_eq!(
        decrypted,
        Scalar::from(shield_amount) * *G,
        "Receiver should decrypt shield amount correctly"
    );
}

// ============================================================================
// HP-US01 ~ HP-US04: Unshield Transfer (UNO -> TOS)
// ============================================================================

/// HP-US01: Unshield UNO to self's TOS
/// Alice unshields 50 UNO to her own TOS
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_happy_unshield_to_self() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    setup_uno_asset(&storage).await?;

    let alice = KeyPair::new();
    let alice_pub = alice.get_public_key().compress();

    // Alice has 0 TOS, 100 UNO
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

    // Unshield 50 UNO
    let unshield_amount = 50 * COIN_VALUE;
    {
        let sender_uno = state
            .get_sender_uno_balance(&alice_pub, &UNO_ASSET, &reference)
            .await?;
        let output = alice.get_public_key().encrypt(unshield_amount);
        *sender_uno -= &output;
        state
            .add_sender_uno_output(&alice_pub, &UNO_ASSET, output)
            .await?;
    }

    state.apply_changes().await?;

    // Verify Alice has 50 UNO remaining
    let (_, mut av) = storage_write
        .get_last_uno_balance(&alice_pub, &UNO_ASSET)
        .await?;
    let alice_point = alice
        .get_private_key()
        .decrypt_to_point(av.get_mut_balance().decompressed()?);
    assert_eq!(
        alice_point,
        Scalar::from(50 * COIN_VALUE) * *G,
        "Alice should have 50 UNO after unshielding 50"
    );

    // TOS credit would be handled by TOS balance system

    Ok(())
}

/// HP-US02: Unshield UNO to another's TOS
/// Alice unshields UNO and credits Bob's TOS
#[test]
fn test_happy_unshield_to_other() {
    // UnshieldTransferPayload allows specifying destination
    let sender = KeyPair::new();
    let receiver = KeyPair::new();
    let amount = 100u64;
    let opening = PedersenOpening::generate_new();

    let commitment = PedersenCommitment::new_with_opening(amount, &opening);
    let sender_handle = sender.get_public_key().decrypt_handle(&opening);

    let mut transcript = tos_common::crypto::new_proof_transcript(b"unshield_test");
    let ct_proof = CiphertextValidityProof::new(
        sender.get_public_key(),
        Some(sender.get_public_key()),
        amount,
        &opening,
        &mut transcript,
    );

    // Create unshield to different receiver
    let payload = UnshieldTransferPayload::new(
        TOS_ASSET,
        receiver.get_public_key().compress(), // Different from sender
        amount,
        None,
        commitment.compress(),
        sender_handle.compress(),
        ct_proof,
    );

    assert_eq!(payload.get_amount(), amount);
    assert_eq!(
        payload.get_destination(),
        &receiver.get_public_key().compress()
    );
    assert_eq!(payload.get_asset(), &TOS_ASSET);
}

/// HP-US03: Unshield partial UNO balance
/// Alice has 100 UNO, unshields 30 UNO, remaining 70 stays encrypted
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_happy_unshield_partial() -> Result<(), BlockchainError> {
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

    // Unshield 30 UNO (partial)
    let unshield_amount = 30 * COIN_VALUE;
    {
        let sender_uno = state
            .get_sender_uno_balance(&alice_pub, &UNO_ASSET, &reference)
            .await?;
        let output = alice.get_public_key().encrypt(unshield_amount);
        *sender_uno -= &output;
        state
            .add_sender_uno_output(&alice_pub, &UNO_ASSET, output)
            .await?;
    }

    state.apply_changes().await?;

    // Verify 70 UNO remaining (still encrypted)
    let (_, mut av) = storage_write
        .get_last_uno_balance(&alice_pub, &UNO_ASSET)
        .await?;
    let alice_point = alice
        .get_private_key()
        .decrypt_to_point(av.get_mut_balance().decompressed()?);
    assert_eq!(
        alice_point,
        Scalar::from(70 * COIN_VALUE) * *G,
        "Alice should have 70 UNO remaining after unshielding 30"
    );

    Ok(())
}

/// HP-US04: Unshield entire UNO balance
/// Alice unshields all 100 UNO, balance becomes 0
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_happy_unshield_entire() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    setup_uno_asset(&storage).await?;

    let alice = KeyPair::new();
    let alice_pub = alice.get_public_key().compress();

    setup_account_safe(&storage, &alice_pub, 0, 0).await?;
    let initial_uno = 100 * COIN_VALUE;
    setup_uno_balance(&storage, &alice, initial_uno, 0).await?;

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

    // Unshield entire balance
    {
        let sender_uno = state
            .get_sender_uno_balance(&alice_pub, &UNO_ASSET, &reference)
            .await?;
        let output = alice.get_public_key().encrypt(initial_uno);
        *sender_uno -= &output;
        state
            .add_sender_uno_output(&alice_pub, &UNO_ASSET, output)
            .await?;
    }

    state.apply_changes().await?;

    // Verify UNO balance is 0
    let (_, mut av) = storage_write
        .get_last_uno_balance(&alice_pub, &UNO_ASSET)
        .await?;
    let alice_point = alice
        .get_private_key()
        .decrypt_to_point(av.get_mut_balance().decompressed()?);
    assert_eq!(
        alice_point,
        Scalar::from(0u64) * *G,
        "Alice should have 0 UNO after unshielding entire balance"
    );

    Ok(())
}
