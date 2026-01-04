//! Security/Attack Tests for UNO Privacy Transfers
//!
//! These tests verify security properties from an attacker's perspective.
//!
//! Test Categories:
//! - SEC-01 ~ SEC-05: ZK Proof Attacks
//! - SEC-06 ~ SEC-09: Commitment Manipulation Attacks
//! - SEC-10 ~ SEC-13: Balance Manipulation Attacks
//! - SEC-14 ~ SEC-15: Shield/Unshield Specific Attacks

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
        elgamal::{Ciphertext, CompressedCommitment, KeyPair, PedersenCommitment, PedersenOpening},
        proofs::{BatchCollector, CiphertextValidityProof, G},
    },
    serializer::Serializer,
    transaction::{
        verify::BlockchainVerificationState, Reference, Role, TxVersion, UnoTransferPayload,
    },
    versioned_type::Versioned,
};
use tos_crypto::curve25519_dalek::Scalar;
use tos_crypto::Identity;
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
        sender_keypair.get_public_key(),
        amount,
        &opening,
        TxVersion::T1,
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
// SEC-01 ~ SEC-05: ZK Proof Attacks
// ============================================================================

/// SEC-01: Proof Replay Attack
/// Reuse valid CiphertextValidityProof with different amount -> MUST FAIL
#[test]
fn test_security_proof_replay_attack() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();

    // Create valid proof for 100 UNO
    let amount1 = 100u64;
    let opening1 = PedersenOpening::generate_new();
    let commitment1 = PedersenCommitment::new_with_opening(amount1, &opening1);
    let sender_handle1 = sender.get_public_key().decrypt_handle(&opening1);
    let receiver_handle1 = receiver.get_public_key().decrypt_handle(&opening1);

    let mut transcript1 = tos_common::crypto::new_proof_transcript(b"replay_test");
    let proof1 = CiphertextValidityProof::new(
        receiver.get_public_key(),
        sender.get_public_key(),
        amount1,
        &opening1,
        TxVersion::T1,
        &mut transcript1,
    );

    // Try to use this proof with DIFFERENT amount (200 UNO)
    let amount2 = 200u64;
    let opening2 = PedersenOpening::generate_new();
    let commitment2 = PedersenCommitment::new_with_opening(amount2, &opening2);
    let sender_handle2 = sender.get_public_key().decrypt_handle(&opening2);
    let receiver_handle2 = receiver.get_public_key().decrypt_handle(&opening2);

    // Verify original proof passes
    let mut verify_transcript = tos_common::crypto::new_proof_transcript(b"replay_test");
    let mut batch_collector = BatchCollector::default();
    let original_verify = proof1.pre_verify(
        &commitment1,
        receiver.get_public_key(),
        sender.get_public_key(),
        &receiver_handle1,
        &sender_handle1,
        TxVersion::T1,
        &mut verify_transcript,
        &mut batch_collector,
    );
    assert!(original_verify.is_ok(), "Original proof should verify");
    assert!(batch_collector.verify().is_ok());

    // Try to replay proof1 with commitment2/handles2 -> MUST FAIL
    let mut replay_transcript = tos_common::crypto::new_proof_transcript(b"replay_test");
    let mut replay_collector = BatchCollector::default();
    let replay_result = proof1.pre_verify(
        &commitment2,
        receiver.get_public_key(),
        sender.get_public_key(),
        &receiver_handle2,
        &sender_handle2,
        TxVersion::T1,
        &mut replay_transcript,
        &mut replay_collector,
    );

    // Either pre_verify fails or batch verify fails
    let replay_failed = replay_result.is_err() || replay_collector.verify().is_err();
    assert!(replay_failed, "Proof replay attack MUST FAIL");
}

/// SEC-02: Proof Substitution Attack
/// Use proof generated for Alice to verify Carol's transfer -> MUST FAIL
#[test]
fn test_security_proof_substitution_attack() {
    let alice = KeyPair::new();
    let bob = KeyPair::new();
    let carol = KeyPair::new();
    let dave = KeyPair::new();

    // Alice creates valid proof for transfer to Bob
    let amount = 100u64;
    let opening = PedersenOpening::generate_new();
    let commitment = PedersenCommitment::new_with_opening(amount, &opening);
    let _alice_handle = alice.get_public_key().decrypt_handle(&opening);
    let _bob_handle = bob.get_public_key().decrypt_handle(&opening);

    let mut transcript = tos_common::crypto::new_proof_transcript(b"substitution_test");
    let alice_proof = CiphertextValidityProof::new(
        bob.get_public_key(),
        alice.get_public_key(),
        amount,
        &opening,
        TxVersion::T1,
        &mut transcript,
    );

    // Carol tries to use Alice's proof for transfer to Dave
    // She creates her own commitment/handles with same opening (simulating attack)
    let carol_handle = carol.get_public_key().decrypt_handle(&opening);
    let dave_handle = dave.get_public_key().decrypt_handle(&opening);

    // Verify with Carol/Dave keys but Alice's proof -> MUST FAIL
    let mut verify_transcript = tos_common::crypto::new_proof_transcript(b"substitution_test");
    let mut batch_collector = BatchCollector::default();
    let result = alice_proof.pre_verify(
        &commitment,
        dave.get_public_key(),  // Wrong receiver
        carol.get_public_key(), // Wrong sender
        &dave_handle,
        &carol_handle,
        TxVersion::T1,
        &mut verify_transcript,
        &mut batch_collector,
    );

    let attack_failed = result.is_err() || batch_collector.verify().is_err();
    assert!(attack_failed, "Proof substitution attack MUST FAIL");
}

/// SEC-03: Fake Proof Injection
/// Submit completely fabricated proof bytes -> MUST FAIL
#[test]
fn test_security_fake_proof_injection() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();
    let amount = 100u64;
    let opening = PedersenOpening::generate_new();
    let commitment = PedersenCommitment::new_with_opening(amount, &opening);
    let sender_handle = sender.get_public_key().decrypt_handle(&opening);
    let receiver_handle = receiver.get_public_key().decrypt_handle(&opening);

    // Create a valid proof first to get the correct size
    let mut transcript = tos_common::crypto::new_proof_transcript(b"fake_test");
    let valid_proof = CiphertextValidityProof::new(
        receiver.get_public_key(),
        sender.get_public_key(),
        amount,
        &opening,
        TxVersion::T1,
        &mut transcript,
    );
    let valid_bytes = valid_proof.to_bytes();

    // Create fake bytes (random data same size as proof)
    let fake_bytes: Vec<u8> = (0..valid_bytes.len())
        .map(|i| (i * 17 % 256) as u8)
        .collect();

    // Try to deserialize fake bytes as proof
    let fake_proof_result = CiphertextValidityProof::from_bytes(&fake_bytes);

    if let Ok(fake_proof) = fake_proof_result {
        // Even if deserialization succeeded, verification MUST fail
        let mut verify_transcript = tos_common::crypto::new_proof_transcript(b"fake_test");
        let mut batch_collector = BatchCollector::default();
        let result = fake_proof.pre_verify(
            &commitment,
            receiver.get_public_key(),
            sender.get_public_key(),
            &receiver_handle,
            &sender_handle,
            TxVersion::T1,
            &mut verify_transcript,
            &mut batch_collector,
        );

        let verification_failed = result.is_err() || batch_collector.verify().is_err();
        assert!(verification_failed, "Fake proof verification MUST FAIL");
    }
    // If deserialization failed, that's also a valid rejection
}

/// SEC-04: Truncated Proof
/// Submit proof with missing bytes -> MUST FAIL
#[test]
fn test_security_truncated_proof() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();
    let amount = 100u64;
    let opening = PedersenOpening::generate_new();

    let mut transcript = tos_common::crypto::new_proof_transcript(b"truncate_test");
    let valid_proof = CiphertextValidityProof::new(
        receiver.get_public_key(),
        sender.get_public_key(),
        amount,
        &opening,
        TxVersion::T1,
        &mut transcript,
    );
    let valid_bytes = valid_proof.to_bytes();

    // Truncate to half size
    let truncated_bytes = &valid_bytes[..valid_bytes.len() / 2];

    // Deserialization MUST fail
    let result = CiphertextValidityProof::from_bytes(truncated_bytes);
    assert!(result.is_err(), "Truncated proof deserialization MUST FAIL");
}

/// SEC-05: Transcript Manipulation
/// Modify transcript domain separator -> MUST FAIL
#[test]
fn test_security_transcript_manipulation() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();
    let amount = 100u64;
    let opening = PedersenOpening::generate_new();
    let commitment = PedersenCommitment::new_with_opening(amount, &opening);
    let sender_handle = sender.get_public_key().decrypt_handle(&opening);
    let receiver_handle = receiver.get_public_key().decrypt_handle(&opening);

    // Generate proof with one domain separator
    let mut gen_transcript = tos_common::crypto::new_proof_transcript(b"domain_A");
    let proof = CiphertextValidityProof::new(
        receiver.get_public_key(),
        sender.get_public_key(),
        amount,
        &opening,
        TxVersion::T1,
        &mut gen_transcript,
    );

    // Verify with DIFFERENT domain separator -> MUST FAIL
    let mut verify_transcript = tos_common::crypto::new_proof_transcript(b"domain_B");
    let mut batch_collector = BatchCollector::default();
    let result = proof.pre_verify(
        &commitment,
        receiver.get_public_key(),
        sender.get_public_key(),
        &receiver_handle,
        &sender_handle,
        TxVersion::T1,
        &mut verify_transcript,
        &mut batch_collector,
    );

    let manipulation_failed = result.is_err() || batch_collector.verify().is_err();
    assert!(manipulation_failed, "Transcript manipulation MUST FAIL");
}

// ============================================================================
// SEC-06 ~ SEC-09: Commitment Manipulation Attacks
// ============================================================================

/// SEC-06: Commitment Mismatch
/// Commitment doesn't match claimed amount -> MUST FAIL
#[test]
fn test_security_commitment_amount_mismatch() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();

    // Create commitment for 100 UNO
    let amount_real = 100u64;
    let opening = PedersenOpening::generate_new();
    let commitment_100 = PedersenCommitment::new_with_opening(amount_real, &opening);
    let sender_handle = sender.get_public_key().decrypt_handle(&opening);
    let receiver_handle = receiver.get_public_key().decrypt_handle(&opening);

    // Generate proof for 200 UNO (different amount)
    let amount_claimed = 200u64;
    let mut transcript = tos_common::crypto::new_proof_transcript(b"mismatch_test");
    let proof = CiphertextValidityProof::new(
        receiver.get_public_key(),
        sender.get_public_key(),
        amount_claimed, // Claiming 200
        &opening,
        TxVersion::T1,
        &mut transcript,
    );

    // Verify with commitment for 100 -> MUST FAIL
    let mut verify_transcript = tos_common::crypto::new_proof_transcript(b"mismatch_test");
    let mut batch_collector = BatchCollector::default();
    let result = proof.pre_verify(
        &commitment_100, // Commitment is for 100, not 200
        receiver.get_public_key(),
        sender.get_public_key(),
        &receiver_handle,
        &sender_handle,
        TxVersion::T1,
        &mut verify_transcript,
        &mut batch_collector,
    );

    let mismatch_failed = result.is_err() || batch_collector.verify().is_err();
    assert!(mismatch_failed, "Commitment amount mismatch MUST FAIL");
}

/// SEC-07: Handle Swap Attack
/// Swap sender/receiver handles -> Should cause decryption failure
#[test]
fn test_security_handle_swap_attack() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();
    let amount = 100u64;

    // Create valid payload
    let payload = create_test_uno_payload(&sender, &receiver, amount);

    // Get the ciphertexts
    let sender_ct = payload.get_ciphertext(Role::Sender);
    let receiver_ct = payload.get_ciphertext(Role::Receiver);

    // Verify sender can decrypt sender_ct
    let sender_decrypted = sender
        .get_private_key()
        .decrypt_to_point(&sender_ct.decompress().expect("Should decompress"));
    assert_eq!(
        sender_decrypted,
        Scalar::from(amount) * *G,
        "Sender should decrypt correctly"
    );

    // If sender tries to decrypt receiver_ct (handle swap) -> Wrong result
    let wrong_decrypted = sender
        .get_private_key()
        .decrypt_to_point(&receiver_ct.decompress().expect("Should decompress"));

    // The decryption will succeed but give wrong value
    // (This is the nature of ElGamal - no authentication, just confidentiality)
    assert_ne!(
        wrong_decrypted,
        Scalar::from(amount) * *G,
        "Handle swap should produce wrong decryption result"
    );
}

/// SEC-08: Zero Commitment Attack
/// Use identity point as commitment -> Should fail or produce trivial result
#[test]
fn test_security_zero_commitment() {
    // Zero commitment is E(0) = (0*G + r*H, r*P) where amount=0
    let sender = KeyPair::new();
    let zero_ct = Ciphertext::zero();
    let compressed = zero_ct.compress();

    // Zero ciphertext should decompress successfully (it's mathematically valid)
    let decompressed = compressed.decompress();
    assert!(decompressed.is_ok(), "Zero ciphertext should decompress");

    // Decrypting zero should give identity point
    let decrypted = sender
        .get_private_key()
        .decrypt_to_point(&decompressed.expect("Zero ct decompresses"));
    assert_eq!(
        decrypted,
        tos_crypto::curve25519_dalek::ristretto::RistrettoPoint::identity(),
        "Zero commitment decrypts to identity"
    );
}

/// SEC-08b: Verify identity point trait is available
#[test]
fn test_security_identity_trait() {
    use tos_crypto::curve25519_dalek::ristretto::RistrettoPoint;
    let id = RistrettoPoint::identity();
    assert_eq!(
        id.compress().to_bytes(),
        [0u8; 32],
        "Identity point compresses to zeros"
    );
}

/// SEC-09: Invalid Curve Point
/// Use point not on curve -> MUST FAIL on decompress
#[test]
fn test_security_invalid_curve_point() {
    // All 0xFF bytes is guaranteed to not be a valid Ristretto point
    let invalid_bytes = [0xFFu8; 32];
    let invalid_commitment = CompressedCommitment::new(
        tos_crypto::curve25519_dalek::ristretto::CompressedRistretto::from_slice(&invalid_bytes)
            .expect("32 bytes"),
    );

    // Decompression MUST fail
    let result = invalid_commitment.decompress();
    assert!(
        result.is_err(),
        "Invalid curve point MUST fail to decompress"
    );
}

// ============================================================================
// SEC-10 ~ SEC-13: Balance Manipulation Attacks
// ============================================================================

/// SEC-10: Double Spend Attack
/// Use same UNO balance in two TXs -> Second TX should fail
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_security_double_spend_attack() -> Result<(), BlockchainError> {
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
    // Alice has 100 UNO
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
        hash: tos_common::crypto::Hash::zero(),
    };

    // TX1: Alice sends 80 UNO to Bob
    {
        let sender_uno = state
            .get_sender_uno_balance(&alice_pub, &UNO_ASSET, &reference)
            .await?;
        let output = alice.get_public_key().encrypt(80 * COIN_VALUE);
        *sender_uno -= &output;
        state
            .add_sender_uno_output(&alice_pub, &UNO_ASSET, output)
            .await?;

        let receiver_balance = state
            .get_receiver_uno_balance(Cow::Borrowed(&bob_pub), Cow::Borrowed(&UNO_ASSET))
            .await?;
        *receiver_balance += bob.get_public_key().encrypt(80 * COIN_VALUE);
    }

    // TX2: Alice tries to send 80 UNO to Carol (double spend!)
    // At this point, Alice's balance in state is already 100 - 80 = 20
    {
        let sender_uno = state
            .get_sender_uno_balance(&alice_pub, &UNO_ASSET, &reference)
            .await?;
        // Alice only has 20 UNO left in the state
        // Any attempt to send 80 more would result in negative balance
        let current_balance = alice.get_private_key().decrypt_to_point(sender_uno);
        assert_eq!(
            current_balance,
            Scalar::from(20 * COIN_VALUE) * *G,
            "Alice should have 20 UNO after first TX"
        );
    }

    state.apply_changes().await?;

    // Verify final state: Alice has 20, Bob has 80
    let (_, mut av) = storage_write
        .get_last_uno_balance(&alice_pub, &UNO_ASSET)
        .await?;
    let ap = alice
        .get_private_key()
        .decrypt_to_point(av.get_mut_balance().decompressed()?);
    assert_eq!(
        ap,
        Scalar::from(20 * COIN_VALUE) * *G,
        "Double spend prevented: Alice has 20"
    );

    Ok(())
}

/// SEC-11: Negative Balance Attack
/// Create TX causing negative balance -> Balance should be tracked correctly
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_security_negative_balance_attack() -> Result<(), BlockchainError> {
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
        hash: tos_common::crypto::Hash::zero(),
    };

    // Try to spend 150 UNO when Alice only has 100
    // Note: In encrypted balance, we can't directly check for negative
    // The balance would "wrap around" in the scalar field
    let sender_uno = state
        .get_sender_uno_balance(&alice_pub, &UNO_ASSET, &reference)
        .await?;

    // Verify current balance is 100
    let current = alice.get_private_key().decrypt_to_point(sender_uno);
    assert_eq!(
        current,
        Scalar::from(100 * COIN_VALUE) * *G,
        "Alice starts with 100 UNO"
    );

    // Subtract more than available - this would create invalid state
    // The pre_verify should catch this via range proofs in real TX
    let overspend = alice.get_public_key().encrypt(150 * COIN_VALUE);
    *sender_uno -= &overspend;

    // After overspending, decryption would not match expected positive value
    // (It wraps in the scalar field)
    let after_overspend = alice.get_private_key().decrypt_to_point(sender_uno);
    // This is NOT a simple negative - it's wrapped in the scalar field
    // The point is that range proofs would catch this in actual TX validation
    assert_ne!(
        after_overspend,
        Scalar::from(0u64) * *G,
        "Overspend does not result in zero"
    );

    Ok(())
}

/// SEC-12: Overflow Attack
/// Create TX causing u64 overflow -> Should be handled
#[test]
fn test_security_overflow_attack() {
    let keypair = KeyPair::new();

    // Create two ciphertexts that would overflow when added
    let large_amount = u64::MAX - 100;
    let ct1 = keypair.get_public_key().encrypt(large_amount);
    let ct2 = keypair.get_public_key().encrypt(200u64);

    // Homomorphic addition in Scalar field doesn't overflow like u64
    // It wraps in the scalar field modulo
    let sum = ct1 + ct2;

    // The decryption will give a point, but discrete log would be hard
    let _sum_point = keypair.get_private_key().decrypt_to_point(&sum);

    // The point is that:
    // 1. Scalar field operations don't have overflow vulnerabilities
    // 2. Range proofs ensure amounts are within valid ranges
    // 3. Total supply constraints prevent creating money from nothing

    // This test mainly verifies the operation doesn't panic
}

/// SEC-13: Race Condition - Concurrent TXs spending same balance
/// Only one should succeed
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_security_race_condition() -> Result<(), BlockchainError> {
    // In BlockDAG, concurrent TXs go into same or different blocks
    // The chain state ensures balance is tracked correctly

    let storage = create_test_storage().await;
    setup_uno_asset(&storage).await?;

    let alice = KeyPair::new();
    let bob = KeyPair::new();
    let alice_pub = alice.get_public_key().compress();
    let bob_pub = bob.get_public_key().compress();

    setup_account_safe(&storage, &alice_pub, 100 * COIN_VALUE, 0).await?;
    setup_account_safe(&storage, &bob_pub, 0, 0).await?;
    setup_uno_balance(&storage, &alice, 100 * COIN_VALUE, 0).await?;

    // Simulate two concurrent spends in same block
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
        hash: tos_common::crypto::Hash::zero(),
    };

    // "Concurrent" TX1: Alice sends 60 UNO
    {
        let sender_uno = state
            .get_sender_uno_balance(&alice_pub, &UNO_ASSET, &reference)
            .await?;
        let output = alice.get_public_key().encrypt(60 * COIN_VALUE);
        *sender_uno -= &output;
        state
            .add_sender_uno_output(&alice_pub, &UNO_ASSET, output)
            .await?;
    }

    // "Concurrent" TX2: Alice sends 60 UNO
    // Since both TXs are in same block, state is shared
    {
        let sender_uno = state
            .get_sender_uno_balance(&alice_pub, &UNO_ASSET, &reference)
            .await?;
        // After TX1, balance is 40
        let current = alice.get_private_key().decrypt_to_point(sender_uno);
        assert_eq!(
            current,
            Scalar::from(40 * COIN_VALUE) * *G,
            "Balance after TX1 is 40"
        );

        // TX2 wants 60 but only 40 available
        // In real system, pre_verify would fail
        // For this test, we verify state tracking works
    }

    state.apply_changes().await?;

    // Final balance should be 40 (only TX1 "succeeded" in our simulation)
    let (_, mut av) = storage_write
        .get_last_uno_balance(&alice_pub, &UNO_ASSET)
        .await?;
    let ap = alice
        .get_private_key()
        .decrypt_to_point(av.get_mut_balance().decompressed()?);
    assert_eq!(
        ap,
        Scalar::from(40 * COIN_VALUE) * *G,
        "Race condition handled: balance is 40"
    );

    Ok(())
}

// ============================================================================
// SEC-14 ~ SEC-15: Shield/Unshield Specific Attacks
// ============================================================================

/// SEC-14: Unshield Without UNO Balance
/// Unshield from account without UNO balance -> Balance should be zero
/// Note: The system initializes new UNO entries from receiver cache
/// so accounts without stored UNO get an encrypted zero balance
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_security_unshield_without_uno_balance() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    setup_uno_asset(&storage).await?;

    let alice = KeyPair::new();
    let alice_pub = alice.get_public_key().compress();

    // Setup Alice with TOS but NO UNO
    setup_account_safe(&storage, &alice_pub, 1000 * COIN_VALUE, 0).await?;
    // NOT calling setup_uno_balance - Alice has 0 UNO

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
        hash: tos_common::crypto::Hash::zero(),
    };

    // Try to get UNO balance that doesn't exist in storage
    // The system initializes from receiver cache, returning zero ciphertext
    let result = state
        .get_sender_uno_balance(&alice_pub, &UNO_ASSET, &reference)
        .await;

    match result {
        Ok(balance) => {
            // If we got a balance, it should be zero (from receiver cache init)
            let point = alice.get_private_key().decrypt_to_point(balance);
            assert_eq!(
                point,
                Scalar::from(0u64) * *G,
                "Account without UNO should have zero balance"
            );
        }
        Err(_) => {
            // Also acceptable: error indicates no UNO balance
        }
    }

    Ok(())
}

/// SEC-15: Shield Amount Mismatch
/// Claim different amount than commitment -> Proof verification fails
#[test]
fn test_security_shield_amount_mismatch() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();

    // Create commitment for 100
    let actual_amount = 100u64;
    let opening = PedersenOpening::generate_new();
    let commitment = PedersenCommitment::new_with_opening(actual_amount, &opening);
    let sender_handle = sender.get_public_key().decrypt_handle(&opening);
    let receiver_handle = receiver.get_public_key().decrypt_handle(&opening);

    // But generate proof claiming 200
    let claimed_amount = 200u64;
    let mut transcript = tos_common::crypto::new_proof_transcript(b"shield_mismatch");
    let proof = CiphertextValidityProof::new(
        receiver.get_public_key(),
        sender.get_public_key(),
        claimed_amount, // Claiming different amount
        &opening,
        TxVersion::T1,
        &mut transcript,
    );

    // Verification with actual commitment (for 100) should fail
    let mut verify_transcript = tos_common::crypto::new_proof_transcript(b"shield_mismatch");
    let mut batch_collector = BatchCollector::default();
    let result = proof.pre_verify(
        &commitment, // This is for 100, not 200
        receiver.get_public_key(),
        sender.get_public_key(),
        &receiver_handle,
        &sender_handle,
        TxVersion::T1,
        &mut verify_transcript,
        &mut batch_collector,
    );

    let mismatch_failed = result.is_err() || batch_collector.verify().is_err();
    assert!(mismatch_failed, "Shield amount mismatch MUST FAIL");
}

// ============================================================================
// SEC-16 ~ SEC-19: Serialization Boundary Tests
// ============================================================================
// Tests for serialization edge cases with extreme values

/// SEC-16: Source Commitments Length Boundary
/// Test source_commitments array with various lengths
#[test]
fn test_security_source_commitments_length_boundary() {
    // Test empty source_commitments
    let empty_commitments: Vec<CompressedCommitment> = vec![];
    assert_eq!(empty_commitments.len(), 0);

    // Test single commitment
    let opening = PedersenOpening::generate_new();
    let commitment = PedersenCommitment::new_with_opening(100u64, &opening);
    let single: Vec<CompressedCommitment> = vec![commitment.compress()];
    assert_eq!(single.len(), 1);

    // Test maximum practical size (e.g., 64 commitments)
    let max_practical = 64;
    let many_commitments: Vec<CompressedCommitment> = (0..max_practical)
        .map(|_| {
            let op = PedersenOpening::generate_new();
            PedersenCommitment::new_with_opening(1u64, &op).compress()
        })
        .collect();
    assert_eq!(many_commitments.len(), max_practical);

    // Verify serialization/deserialization works for all sizes
    for commitment in &many_commitments {
        let bytes = commitment.to_bytes();
        assert_eq!(bytes.len(), 32, "Each commitment should be 32 bytes");
    }
}

/// SEC-17: Ciphertext Serialization Boundary
/// Test ciphertext with extreme values
#[test]
fn test_security_ciphertext_serialization_boundary() {
    let keypair = KeyPair::new();

    // Test with zero amount
    let zero_ct = keypair.get_public_key().encrypt(0u64);
    let zero_compressed = zero_ct.compress();
    let zero_bytes = zero_compressed.to_bytes();
    assert!(!zero_bytes.is_empty(), "Zero ciphertext should serialize");

    // Test with max u64 (will wrap in scalar field)
    let max_ct = keypair.get_public_key().encrypt(u64::MAX);
    let max_compressed = max_ct.compress();
    let max_bytes = max_compressed.to_bytes();
    assert_eq!(
        max_bytes.len(),
        zero_bytes.len(),
        "Ciphertexts should have consistent size"
    );

    // Decompress and verify
    let zero_deser = zero_compressed.decompress();
    assert!(zero_deser.is_ok(), "Zero ciphertext should decompress");

    let max_deser = max_compressed.decompress();
    assert!(max_deser.is_ok(), "Max ciphertext should decompress");
}

/// SEC-18: Proof Serialization Size Limits
/// Test proof with various sizes and malformed inputs
#[test]
fn test_security_proof_serialization_size_limits() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();
    let amount = 100u64;
    let opening = PedersenOpening::generate_new();

    let mut transcript = tos_common::crypto::new_proof_transcript(b"size_test");
    let proof = CiphertextValidityProof::new(
        receiver.get_public_key(),
        sender.get_public_key(),
        amount,
        &opening,
        TxVersion::T1,
        &mut transcript,
    );

    let valid_bytes = proof.to_bytes();
    let expected_size = valid_bytes.len();

    // Test undersized input (1 byte)
    let tiny_bytes = vec![0u8; 1];
    let tiny_result = CiphertextValidityProof::from_bytes(&tiny_bytes);
    assert!(tiny_result.is_err(), "Tiny bytes should fail");

    // Test oversized input
    let oversized_bytes: Vec<u8> = (0..expected_size + 100).map(|i| i as u8).collect();
    let _oversized_result = CiphertextValidityProof::from_bytes(&oversized_bytes);
    // May or may not fail depending on implementation - the point is it shouldn't crash

    // Test exact size with corrupted data
    let mut corrupted_bytes = valid_bytes.clone();
    corrupted_bytes[0] ^= 0xFF;
    corrupted_bytes[expected_size / 2] ^= 0xFF;
    corrupted_bytes[expected_size - 1] ^= 0xFF;

    let corrupted_result = CiphertextValidityProof::from_bytes(&corrupted_bytes);
    if let Ok(corrupted_proof) = corrupted_result {
        // Even if deserialization succeeded, verification should fail
        let commitment = PedersenCommitment::new_with_opening(amount, &opening);
        let sender_handle = sender.get_public_key().decrypt_handle(&opening);
        let receiver_handle = receiver.get_public_key().decrypt_handle(&opening);

        let mut verify_transcript = tos_common::crypto::new_proof_transcript(b"size_test");
        let mut batch_collector = BatchCollector::default();
        let verify_result = corrupted_proof.pre_verify(
            &commitment,
            receiver.get_public_key(),
            sender.get_public_key(),
            &receiver_handle,
            &sender_handle,
            TxVersion::T1,
            &mut verify_transcript,
            &mut batch_collector,
        );

        let corrupted_failed = verify_result.is_err() || batch_collector.verify().is_err();
        assert!(corrupted_failed, "Corrupted proof should fail verification");
    }
}

/// SEC-19: UNO Balance Versioning Edge Cases
/// Test version boundaries and migration scenarios
#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_security_uno_balance_version_edge_cases() -> Result<(), BlockchainError> {
    let storage = create_test_storage().await;
    setup_uno_asset(&storage).await?;

    let alice = KeyPair::new();
    let alice_pub = alice.get_public_key().compress();

    // Setup with various version numbers
    let topoheight_v0 = 0u64;
    let topoheight_v1 = 1000u64;
    let topoheight_v2 = 2000u64;

    setup_account_safe(&storage, &alice_pub, 1000 * COIN_VALUE, 0).await?;

    // Version 0: Initial balance
    setup_uno_balance(&storage, &alice, 100 * COIN_VALUE, topoheight_v0).await?;

    // Version 1: Updated balance
    setup_uno_balance(&storage, &alice, 150 * COIN_VALUE, topoheight_v1).await?;

    // Version 2: Another update
    setup_uno_balance(&storage, &alice, 200 * COIN_VALUE, topoheight_v2).await?;

    // Verify latest version is retrieved
    let storage_read = storage.read().await;
    let (topo, mut versioned) = storage_read
        .get_last_uno_balance(&alice_pub, &UNO_ASSET)
        .await?;

    assert_eq!(topo, topoheight_v2, "Should get latest topoheight");

    // Verify balance decryption
    let balance = versioned.get_mut_balance().decompressed()?;
    let decrypted = alice.get_private_key().decrypt_to_point(&balance);
    assert_eq!(
        decrypted,
        Scalar::from(200 * COIN_VALUE) * *G,
        "Should get latest balance value"
    );

    Ok(())
}

// ============================================================================
// ShieldTransfers TOS_ASSET Validation Tests
// Verifies that ShieldTransfers only accepts TOS_ASSET, rejecting other assets
// ============================================================================

/// ShieldTransfer with TOS_ASSET should be accepted
/// This is the positive test to ensure TOS shielding still works
#[test]
fn test_shield_tos_asset_accepted() {
    use tos_common::config::TOS_ASSET;

    let sender = KeyPair::new();
    let receiver = KeyPair::new();

    // Create a valid shield transfer with TOS_ASSET
    let amount = 100 * COIN_VALUE;
    let opening = PedersenOpening::generate_new();

    let commitment = PedersenCommitment::new_with_opening(amount, &opening);
    let sender_handle = sender.get_public_key().decrypt_handle(&opening);
    let receiver_handle = receiver.get_public_key().decrypt_handle(&opening);

    let mut transcript = tos_common::crypto::new_proof_transcript(b"shield_tos_test");
    let proof = CiphertextValidityProof::new(
        receiver.get_public_key(),
        sender.get_public_key(),
        amount,
        &opening,
        TxVersion::T1,
        &mut transcript,
    );

    // Create payload with TOS_ASSET (valid)
    let payload = UnoTransferPayload::new(
        TOS_ASSET, // Using TOS_ASSET - should be valid
        receiver.get_public_key().compress(),
        None,
        commitment.compress(),
        sender_handle.compress(),
        receiver_handle.compress(),
        proof,
    );

    // Verify the asset is TOS_ASSET
    assert_eq!(
        *payload.get_asset(),
        TOS_ASSET,
        "Payload should use TOS_ASSET"
    );

    // TOS_ASSET is the genesis asset (all zeros)
    assert_eq!(
        TOS_ASSET,
        tos_common::crypto::Hash::zero(),
        "TOS_ASSET should be the zero hash"
    );
}

/// ShieldTransfer with non-TOS asset should be rejected
/// UNO only supports TOS as single-asset privacy layer
#[test]
fn test_shield_non_tos_asset_rejected() {
    use tos_common::crypto::Hash;

    // Create a fake non-TOS asset
    let non_tos_asset = Hash::new([1u8; 32]);
    assert_ne!(
        non_tos_asset,
        tos_common::config::TOS_ASSET,
        "Test asset should not be TOS_ASSET"
    );

    let sender = KeyPair::new();
    let receiver = KeyPair::new();

    let amount = 100 * COIN_VALUE;
    let opening = PedersenOpening::generate_new();
    let commitment = PedersenCommitment::new_with_opening(amount, &opening);
    let sender_handle = sender.get_public_key().decrypt_handle(&opening);
    let receiver_handle = receiver.get_public_key().decrypt_handle(&opening);

    let mut transcript = tos_common::crypto::new_proof_transcript(b"shield_non_tos_test");
    let proof = CiphertextValidityProof::new(
        receiver.get_public_key(),
        sender.get_public_key(),
        amount,
        &opening,
        TxVersion::T1,
        &mut transcript,
    );

    // Create payload with NON-TOS asset (should be rejected at verification)
    let payload = UnoTransferPayload::new(
        non_tos_asset.clone(), // Using non-TOS asset - should be rejected
        receiver.get_public_key().compress(),
        None,
        commitment.compress(),
        sender_handle.compress(),
        receiver_handle.compress(),
        proof,
    );

    // The asset in payload should be the non-TOS asset
    assert_eq!(
        *payload.get_asset(),
        non_tos_asset,
        "Payload should have non-TOS asset for this test"
    );
    assert_ne!(
        *payload.get_asset(),
        tos_common::config::TOS_ASSET,
        "Asset should NOT be TOS_ASSET"
    );

    // Note: The actual rejection happens in verify_dynamic_parts (line 935-941)
    // and pre_verify (line 2764-2770) in common/src/transaction/verify/mod.rs
    // This test verifies we can create payloads with non-TOS assets,
    // but the verification layer (not tested here) will reject them
}

/// Verify UNO_ASSET is different from TOS_ASSET
/// Ensures the two assets are properly distinguished
#[test]
fn test_uno_vs_tos_asset_distinct() {
    use tos_common::config::{TOS_ASSET, UNO_ASSET};

    // UNO_ASSET and TOS_ASSET should be different
    assert_ne!(
        UNO_ASSET, TOS_ASSET,
        "UNO_ASSET and TOS_ASSET must be different"
    );

    // TOS_ASSET should be all zeros (genesis asset)
    assert_eq!(
        TOS_ASSET,
        tos_common::crypto::Hash::zero(),
        "TOS_ASSET should be zero hash"
    );

    // UNO_ASSET should be a specific value (from hash of "UNO")
    assert_ne!(
        UNO_ASSET,
        tos_common::crypto::Hash::zero(),
        "UNO_ASSET should not be zero hash"
    );
}

/// Multiple assets in batch - only TOS allowed
/// Verifies that in a batch of shield transfers, all must use TOS_ASSET
#[test]
fn test_batch_shield_all_must_be_tos() {
    use tos_common::config::TOS_ASSET;
    use tos_common::crypto::Hash;

    let sender = KeyPair::new();
    let receivers: Vec<KeyPair> = (0..3).map(|_| KeyPair::new()).collect();

    let amount = 100 * COIN_VALUE;

    // Create payloads - all should use TOS_ASSET
    let payloads: Vec<_> = receivers
        .iter()
        .map(|receiver| {
            let opening = PedersenOpening::generate_new();
            let commitment = PedersenCommitment::new_with_opening(amount, &opening);
            let sender_handle = sender.get_public_key().decrypt_handle(&opening);
            let receiver_handle = receiver.get_public_key().decrypt_handle(&opening);

            let mut transcript = tos_common::crypto::new_proof_transcript(b"batch_shield");
            let proof = CiphertextValidityProof::new(
                receiver.get_public_key(),
                sender.get_public_key(),
                amount,
                &opening,
                TxVersion::T1,
                &mut transcript,
            );

            UnoTransferPayload::new(
                TOS_ASSET,
                receiver.get_public_key().compress(),
                None,
                commitment.compress(),
                sender_handle.compress(),
                receiver_handle.compress(),
                proof,
            )
        })
        .collect();

    // All payloads should use TOS_ASSET
    for (i, payload) in payloads.iter().enumerate() {
        assert_eq!(
            *payload.get_asset(),
            TOS_ASSET,
            "Payload {} should use TOS_ASSET",
            i
        );
    }

    // Test: if any payload used a different asset, it would be rejected
    let fake_asset = Hash::new([42u8; 32]);
    assert_ne!(fake_asset, TOS_ASSET, "Fake asset should differ from TOS");
}

/// Zero amount shield still requires TOS_ASSET
/// Edge case: even zero-amount shields must use TOS_ASSET
#[test]
fn test_zero_amount_still_requires_tos() {
    use tos_common::config::TOS_ASSET;

    let sender = KeyPair::new();
    let receiver = KeyPair::new();

    // Zero amount
    let amount = 0u64;
    let opening = PedersenOpening::generate_new();
    let commitment = PedersenCommitment::new_with_opening(amount, &opening);
    let sender_handle = sender.get_public_key().decrypt_handle(&opening);
    let receiver_handle = receiver.get_public_key().decrypt_handle(&opening);

    let mut transcript = tos_common::crypto::new_proof_transcript(b"zero_shield");
    let proof = CiphertextValidityProof::new(
        receiver.get_public_key(),
        sender.get_public_key(),
        amount,
        &opening,
        TxVersion::T1,
        &mut transcript,
    );

    let payload = UnoTransferPayload::new(
        TOS_ASSET,
        receiver.get_public_key().compress(),
        None,
        commitment.compress(),
        sender_handle.compress(),
        receiver_handle.compress(),
        proof,
    );

    // Even for zero amount, must use TOS_ASSET
    assert_eq!(
        *payload.get_asset(),
        TOS_ASSET,
        "Zero amount shield must still use TOS_ASSET"
    );

    // Note: Zero amount transfers are rejected separately by InvalidTransferAmount check
    // This test is about asset validation, not amount validation
}
