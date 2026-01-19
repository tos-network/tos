//! End-to-end tests for UNO (privacy-preserving) transfer functionality
//!
//! These tests verify the core UNO transfer infrastructure:
//! 1. UnoTransferPayload creation with valid ZK proofs
//! 2. Transaction serialization with UNO transfers
//! 3. Source commitment structure serialization
//!
//! Note: Proof verification is tested in the crypto module unit tests.

#![allow(clippy::disallowed_methods)]

use tos_common::{
    config::{TOS_ASSET, UNO_ASSET},
    context::Context,
    crypto::{
        elgamal::{KeyPair, PedersenCommitment, PedersenOpening},
        proofs::CiphertextValidityProof,
        Hash, Hashable,
    },
    serializer::{Reader, Serializer},
    transaction::{
        FeeType, Reference, Role, SourceCommitment, Transaction, TransactionType, TransferPayload,
        TxVersion, UnoTransferPayload,
    },
};

/// Helper: Create a test UnoTransferPayload with valid proofs
fn create_test_uno_payload(
    sender_keypair: &KeyPair,
    receiver_keypair: &KeyPair,
    asset: Hash,
    amount: u64,
) -> UnoTransferPayload {
    let destination = receiver_keypair.get_public_key().compress();
    let opening = PedersenOpening::generate_new();

    // Create commitment and handles
    let commitment = PedersenCommitment::new_with_opening(amount, &opening);
    let sender_handle = sender_keypair.get_public_key().decrypt_handle(&opening);
    let receiver_handle = receiver_keypair.get_public_key().decrypt_handle(&opening);

    // Create the ciphertext validity proof using tos_common's re-exported Transcript
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
        asset,
        destination,
        None,
        commitment.compress(),
        sender_handle.compress(),
        receiver_handle.compress(),
        proof,
    )
}

/// Test UnoTransferPayload creation and serialization roundtrip
#[test]
fn test_uno_transfer_payload_serialization() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();

    let payload = create_test_uno_payload(&sender, &receiver, UNO_ASSET, 100);

    // Verify basic properties
    assert_eq!(payload.get_asset(), &UNO_ASSET);
    assert_eq!(
        payload.get_destination(),
        &receiver.get_public_key().compress()
    );

    // Test serialization roundtrip
    let bytes = payload.to_bytes();
    let mut context = Context::new();
    context.store(TxVersion::T1);
    let mut reader = Reader::with_context(&bytes, context);
    let restored = UnoTransferPayload::read(&mut reader).unwrap();

    assert_eq!(payload.get_asset(), restored.get_asset());
    assert_eq!(payload.get_destination(), restored.get_destination());
    assert_eq!(payload.get_commitment(), restored.get_commitment());
    assert_eq!(payload.get_sender_handle(), restored.get_sender_handle());
    assert_eq!(
        payload.get_receiver_handle(),
        restored.get_receiver_handle()
    );
}

/// Test TransactionType::UnoTransfers serialization
#[test]
fn test_uno_transfers_transaction_type() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();

    let payload = create_test_uno_payload(&sender, &receiver, UNO_ASSET, 1000);
    let tx_type = TransactionType::UnoTransfers(vec![payload]);

    // Serialize
    let bytes = tx_type.to_bytes();

    // Deserialize with context
    let mut context = Context::new();
    context.store(TxVersion::T1);
    let mut reader = Reader::with_context(&bytes, context);
    let restored = TransactionType::read(&mut reader).unwrap();

    // Verify it's the right variant
    match restored {
        TransactionType::UnoTransfers(transfers) => {
            assert_eq!(transfers.len(), 1);
            assert_eq!(transfers[0].get_asset(), &UNO_ASSET);
        }
        _ => panic!("Expected UnoTransfers variant"),
    }
}

/// Test multiple UNO transfers in a single transaction type
#[test]
fn test_multiple_uno_transfers() {
    let sender = KeyPair::new();
    let receivers: Vec<_> = (0..3).map(|_| KeyPair::new()).collect();

    let mut payloads = Vec::new();
    for (i, receiver) in receivers.iter().enumerate() {
        let amount = (100 * (i + 1)) as u64;
        let payload = create_test_uno_payload(&sender, receiver, UNO_ASSET, amount);
        payloads.push(payload);
    }

    let tx_type = TransactionType::UnoTransfers(payloads);

    // Verify serialization
    let bytes = tx_type.to_bytes();
    let mut context = Context::new();
    context.store(TxVersion::T1);
    let mut reader = Reader::with_context(&bytes, context);
    let restored = TransactionType::read(&mut reader).unwrap();

    match restored {
        TransactionType::UnoTransfers(transfers) => {
            assert_eq!(transfers.len(), 3);
        }
        _ => panic!("Expected UnoTransfers variant"),
    }
}

/// Test UNO transfer ciphertext can be used by both sender and receiver
#[test]
fn test_uno_transfer_dual_ciphertext() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();

    let payload = create_test_uno_payload(&sender, &receiver, UNO_ASSET, 250);

    // Get ciphertext for sender (uses sender_handle)
    let sender_ct = payload.get_ciphertext(Role::Sender);
    assert_eq!(sender_ct.commitment(), payload.get_commitment());
    assert_eq!(sender_ct.handle(), payload.get_sender_handle());

    // Get ciphertext for receiver (uses receiver_handle)
    let receiver_ct = payload.get_ciphertext(Role::Receiver);
    assert_eq!(receiver_ct.commitment(), payload.get_commitment());
    assert_eq!(receiver_ct.handle(), payload.get_receiver_handle());

    // Both share the same commitment but different handles
    assert_eq!(sender_ct.commitment(), receiver_ct.commitment());
    assert_ne!(sender_ct.handle(), receiver_ct.handle());
}

/// Test Transaction::prepare_transcript creates transcripts correctly
#[test]
fn test_transaction_prepare_transcript() {
    let keypair = KeyPair::new();
    let source = keypair.get_public_key().compress();
    let fee = 1000u64;
    let nonce = 5u64;

    // Create transcript using the static method
    let transcript1 =
        Transaction::prepare_transcript(TxVersion::T1, &source, fee, &FeeType::TOS, nonce);

    // Create another transcript with same parameters
    let transcript2 =
        Transaction::prepare_transcript(TxVersion::T1, &source, fee, &FeeType::TOS, nonce);

    // Transcripts should be created without error
    drop(transcript1);
    drop(transcript2);
}

/// Test that Transaction with UnoTransfers can be hashed
#[test]
fn test_transaction_with_uno_transfers_hashable() {
    use tos_common::crypto::Signature;

    let sender = KeyPair::new();
    let receiver = KeyPair::new();

    let payload = create_test_uno_payload(&sender, &receiver, UNO_ASSET, 100);
    let tx_type = TransactionType::UnoTransfers(vec![payload]);

    let reference = Reference {
        topoheight: 0,
        hash: Hash::zero(),
    };

    // Create a dummy signature (64 bytes)
    let sig_bytes = [0u8; 64];
    let signature = Signature::from_bytes(&sig_bytes).unwrap();

    let tx = Transaction::new(
        TxVersion::T1,
        0,
        sender.get_public_key().compress(),
        tx_type,
        1000,
        FeeType::TOS,
        0,
        reference,
        None,
        signature,
    );

    // Verify hash() method works
    let hash = tx.hash();
    assert_ne!(hash, Hash::zero());

    // Hash should be deterministic
    let hash2 = tx.hash();
    assert_eq!(hash, hash2);

    // Verify it's recognized as UNO transaction
    assert!(tx.has_uno_transfers());
}

/// Test SourceCommitment serialization
#[test]
fn test_source_commitment_serialization() {
    use tos_common::crypto::{elgamal::Ciphertext, proofs::CommitmentEqProof};

    let keypair = KeyPair::new();

    // Create a source ciphertext (simulating existing UNO balance)
    let amount = 1000u64;
    let opening = PedersenOpening::generate_new();
    let commitment = PedersenCommitment::new_with_opening(amount, &opening);
    let handle = keypair.get_public_key().decrypt_handle(&opening);
    let ciphertext = Ciphertext::new(commitment.clone(), handle);

    // Create equality proof
    let mut transcript = tos_common::crypto::new_proof_transcript(b"test_eq_proof");
    let eq_proof = CommitmentEqProof::new(&keypair, &ciphertext, &opening, amount, &mut transcript);

    // Create SourceCommitment
    let source_commitment = SourceCommitment::new(commitment.compress(), eq_proof, UNO_ASSET);

    // Verify serialization roundtrip
    let bytes = source_commitment.to_bytes();
    let mut reader = Reader::new(&bytes);
    let restored = SourceCommitment::read(&mut reader).unwrap();

    assert_eq!(source_commitment.get_asset(), restored.get_asset());
    assert_eq!(
        source_commitment.get_commitment(),
        restored.get_commitment()
    );
}

/// Test that plaintext Transaction still works alongside UNO
#[test]
fn test_plaintext_transaction_hashable() {
    use tos_common::crypto::Signature;

    let keypair = KeyPair::new();
    let receiver = KeyPair::new();

    let transfer = TransferPayload::new(TOS_ASSET, receiver.get_public_key().compress(), 100, None);

    let reference = Reference {
        topoheight: 0,
        hash: Hash::zero(),
    };

    let sig_bytes = [0u8; 64];
    let signature = Signature::from_bytes(&sig_bytes).unwrap();

    let tx = Transaction::new(
        TxVersion::T1,
        0,
        keypair.get_public_key().compress(),
        TransactionType::Transfers(vec![transfer]),
        1000,
        FeeType::TOS,
        0,
        reference,
        None,
        signature,
    );

    // Verify hash works
    let hash = tx.hash();
    assert_ne!(hash, Hash::zero());

    // Should NOT be UNO transaction
    assert!(!tx.has_uno_transfers());
}

/// Test UnoTransferPayload size calculation
#[test]
fn test_uno_transfer_payload_size() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();

    let payload = create_test_uno_payload(&sender, &receiver, UNO_ASSET, 100);

    // Verify size() matches actual serialized bytes
    let bytes = payload.to_bytes();
    assert_eq!(payload.size(), bytes.len());
}

/// Test that UNO_ASSET is distinct from TOS_ASSET
#[test]
fn test_uno_asset_is_distinct() {
    // UNO_ASSET must be different from TOS_ASSET (zero hash)
    assert_ne!(UNO_ASSET, TOS_ASSET);
    assert_ne!(UNO_ASSET, Hash::zero());

    // UNO_ASSET should be 0x01 (last byte)
    let bytes = UNO_ASSET.as_bytes();
    assert_eq!(bytes[31], 0x01);
}

/// Test UnoTransferPayload consume method
#[test]
fn test_uno_transfer_payload_consume() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();

    let payload = create_test_uno_payload(&sender, &receiver, UNO_ASSET, 100);

    // Clone before consume
    let original_asset = payload.get_asset().clone();
    let original_dest = payload.get_destination().clone();
    let original_commitment = payload.get_commitment().clone();

    // Consume
    let (asset, dest, _extra, commitment, _sender_h, _receiver_h) = payload.consume();

    assert_eq!(asset, original_asset);
    assert_eq!(dest, original_dest);
    assert_eq!(commitment, original_commitment);
}
