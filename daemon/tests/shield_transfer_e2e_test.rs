//! End-to-end tests for Shield transfer functionality (TOS -> UNO)
//!
//! These tests verify the Shield transfer infrastructure:
//! 1. ShieldTransferPayload creation with valid commitment proofs
//! 2. Transaction serialization with Shield transfers
//! 3. TransactionType::ShieldTransfers (opcode 19) serialization
//!
//! Shield transfers convert plaintext TOS balance to encrypted UNO balance.

#![allow(clippy::disallowed_methods)]

use tos_common::{
    config::{TOS_ASSET, UNO_ASSET},
    context::Context,
    crypto::{
        elgamal::{KeyPair, PedersenCommitment, PedersenOpening},
        proofs::ShieldCommitmentProof,
        Hash, Hashable,
    },
    serializer::{Reader, Serializer},
    transaction::{
        FeeType, Reference, ShieldTransferPayload, Transaction, TransactionType, TransferPayload,
        TxVersion,
    },
};

/// Helper: Create a test ShieldTransferPayload with valid commitment proof
fn create_test_shield_payload(receiver_keypair: &KeyPair, amount: u64) -> ShieldTransferPayload {
    let destination = receiver_keypair.get_public_key().compress();
    let opening = PedersenOpening::generate_new();

    // Create commitment and receiver handle
    let commitment = PedersenCommitment::new_with_opening(amount, &opening);
    let receiver_handle = receiver_keypair.get_public_key().decrypt_handle(&opening);

    // Create Shield commitment proof
    let mut transcript = tos_common::crypto::new_proof_transcript(b"shield_commitment_proof");
    let proof = ShieldCommitmentProof::new(
        receiver_keypair.get_public_key(),
        amount,
        &opening,
        &mut transcript,
    );

    ShieldTransferPayload::new(
        TOS_ASSET, // Shield always uses TOS_ASSET as source
        destination,
        amount,
        None,
        commitment.compress(),
        receiver_handle.compress(),
        proof,
    )
}

/// Test ShieldTransferPayload creation and serialization roundtrip
#[test]
fn test_shield_transfer_payload_serialization() {
    let receiver = KeyPair::new();

    let payload = create_test_shield_payload(&receiver, 100);

    // Verify basic properties
    assert_eq!(payload.get_asset(), &TOS_ASSET);
    assert_eq!(payload.get_amount(), 100);
    assert_eq!(
        payload.get_destination(),
        &receiver.get_public_key().compress()
    );

    // Test serialization roundtrip
    let bytes = payload.to_bytes();
    let mut reader = Reader::new(&bytes);
    let restored = ShieldTransferPayload::read(&mut reader).unwrap();

    assert_eq!(payload.get_asset(), restored.get_asset());
    assert_eq!(payload.get_destination(), restored.get_destination());
    assert_eq!(payload.get_amount(), restored.get_amount());
    assert_eq!(payload.get_commitment(), restored.get_commitment());
    assert_eq!(
        payload.get_receiver_handle(),
        restored.get_receiver_handle()
    );
}

/// Test TransactionType::ShieldTransfers serialization (opcode 19)
#[test]
fn test_shield_transfers_transaction_type() {
    let receiver = KeyPair::new();

    let payload = create_test_shield_payload(&receiver, 1000);
    let tx_type = TransactionType::ShieldTransfers(vec![payload]);

    // Serialize
    let bytes = tx_type.to_bytes();

    // First byte should be opcode 19 for ShieldTransfers
    assert_eq!(bytes[0], 19, "ShieldTransfers should use opcode 19");

    // Deserialize with context
    let mut context = Context::new();
    context.store(TxVersion::T1);
    let mut reader = Reader::with_context(&bytes, context);
    let restored = TransactionType::read(&mut reader).unwrap();

    // Verify it's the right variant
    match restored {
        TransactionType::ShieldTransfers(transfers) => {
            assert_eq!(transfers.len(), 1);
            assert_eq!(transfers[0].get_asset(), &TOS_ASSET);
            assert_eq!(transfers[0].get_amount(), 1000);
        }
        _ => unreachable!("Expected ShieldTransfers variant"),
    }
}

/// Test multiple Shield transfers in a single transaction
#[test]
fn test_multiple_shield_transfers() {
    let receivers: Vec<_> = (0..3).map(|_| KeyPair::new()).collect();

    let mut payloads = Vec::new();
    for (i, receiver) in receivers.iter().enumerate() {
        let amount = (100 * (i + 1)) as u64;
        let payload = create_test_shield_payload(receiver, amount);
        payloads.push(payload);
    }

    let tx_type = TransactionType::ShieldTransfers(payloads);

    // Verify serialization
    let bytes = tx_type.to_bytes();
    let mut context = Context::new();
    context.store(TxVersion::T1);
    let mut reader = Reader::with_context(&bytes, context);
    let restored = TransactionType::read(&mut reader).unwrap();

    match restored {
        TransactionType::ShieldTransfers(transfers) => {
            assert_eq!(transfers.len(), 3);
            assert_eq!(transfers[0].get_amount(), 100);
            assert_eq!(transfers[1].get_amount(), 200);
            assert_eq!(transfers[2].get_amount(), 300);
        }
        _ => unreachable!("Expected ShieldTransfers variant"),
    }
}

/// Test that Transaction with ShieldTransfers can be hashed
#[test]
fn test_transaction_with_shield_transfers_hashable() {
    use tos_common::crypto::Signature;

    let sender = KeyPair::new();
    let receiver = KeyPair::new();

    let payload = create_test_shield_payload(&receiver, 100);
    let tx_type = TransactionType::ShieldTransfers(vec![payload]);

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

    // Verify it's recognized as UNO transaction (Shield is part of UNO ecosystem)
    assert!(tx.has_uno_transfers());
}

/// Test ShieldTransferPayload size calculation
#[test]
fn test_shield_transfer_payload_size() {
    let receiver = KeyPair::new();

    let payload = create_test_shield_payload(&receiver, 100);

    // Verify size() matches actual serialized bytes
    let bytes = payload.to_bytes();
    assert_eq!(payload.size(), bytes.len());
}

/// Test Shield transfer with extra data (memo)
#[test]
fn test_shield_transfer_with_extra_data() {
    use tos_common::transaction::extra_data::UnknownExtraDataFormat;

    let receiver = KeyPair::new();
    let opening = PedersenOpening::generate_new();
    let amount = 500u64;

    let commitment = PedersenCommitment::new_with_opening(amount, &opening);
    let receiver_handle = receiver.get_public_key().decrypt_handle(&opening);

    let mut transcript = tos_common::crypto::new_proof_transcript(b"shield_commitment_proof");
    let proof =
        ShieldCommitmentProof::new(receiver.get_public_key(), amount, &opening, &mut transcript);

    let extra_data = Some(UnknownExtraDataFormat(vec![1, 2, 3, 4, 5]));
    let payload = ShieldTransferPayload::new(
        TOS_ASSET,
        receiver.get_public_key().compress(),
        amount,
        extra_data,
        commitment.compress(),
        receiver_handle.compress(),
        proof,
    );

    assert!(payload.get_extra_data().is_some());

    // Verify serialization roundtrip with extra data
    let bytes = payload.to_bytes();
    let mut reader = Reader::new(&bytes);
    let restored = ShieldTransferPayload::read(&mut reader).unwrap();
    assert!(restored.get_extra_data().is_some());
}

/// Test ShieldTransferPayload consume method
#[test]
fn test_shield_transfer_payload_consume() {
    let receiver = KeyPair::new();

    let payload = create_test_shield_payload(&receiver, 100);

    // Clone before consume
    let original_asset = payload.get_asset().clone();
    let original_dest = payload.get_destination().clone();
    let original_amount = payload.get_amount();
    let original_commitment = payload.get_commitment().clone();

    // Consume
    let (asset, dest, amount, _extra, commitment, _handle, _proof) = payload.consume();

    assert_eq!(asset, original_asset);
    assert_eq!(dest, original_dest);
    assert_eq!(amount, original_amount);
    assert_eq!(commitment, original_commitment);
}

/// Test Shield commitment proof verification during serialization roundtrip
#[test]
fn test_shield_commitment_proof_preserved() {
    let receiver = KeyPair::new();
    let amount = 1000u64;

    let payload = create_test_shield_payload(&receiver, amount);

    // Serialize
    let bytes = payload.to_bytes();
    let mut reader = Reader::new(&bytes);
    let restored = ShieldTransferPayload::read(&mut reader).unwrap();

    // Verify the proof can be verified after deserialization
    let commitment = restored.get_commitment().decompress().unwrap();
    let receiver_pubkey = restored.get_destination().decompress().unwrap();
    let receiver_handle = restored.get_receiver_handle().decompress().unwrap();

    let mut transcript = tos_common::crypto::new_proof_transcript(b"shield_commitment_proof");
    let result = restored.get_proof().verify(
        &commitment,
        &receiver_pubkey,
        &receiver_handle,
        restored.get_amount(),
        &mut transcript,
    );

    assert!(result.is_ok(), "Proof verification should succeed");
}

/// Test Shield with various amounts
#[test]
fn test_shield_transfer_various_amounts() {
    let amounts = [1u64, 100, 1000, 1_000_000, u64::MAX / 2];

    for amount in amounts {
        let receiver = KeyPair::new();
        let payload = create_test_shield_payload(&receiver, amount);

        assert_eq!(payload.get_amount(), amount);

        // Verify serialization roundtrip
        let bytes = payload.to_bytes();
        let mut reader = Reader::new(&bytes);
        let restored = ShieldTransferPayload::read(&mut reader).unwrap();
        assert_eq!(restored.get_amount(), amount);
    }
}

/// Test that Shield uses TOS_ASSET (not UNO_ASSET)
#[test]
fn test_shield_uses_tos_asset() {
    let receiver = KeyPair::new();
    let payload = create_test_shield_payload(&receiver, 100);

    // Shield transfers the TOS asset (source is plaintext TOS)
    assert_eq!(payload.get_asset(), &TOS_ASSET);
    assert_ne!(payload.get_asset(), &UNO_ASSET);
}

/// Test plaintext Transaction still works alongside Shield
#[test]
fn test_plaintext_transaction_distinct_from_shield() {
    use tos_common::crypto::Signature;

    let keypair = KeyPair::new();
    let receiver = KeyPair::new();

    // Create plaintext transfer
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

    // Verify it's NOT a UNO transaction
    assert!(!tx.has_uno_transfers());

    // Verify hash works
    let hash = tx.hash();
    assert_ne!(hash, Hash::zero());
}

/// Test Shield transaction full serialization roundtrip
#[test]
fn test_shield_full_transaction_serialization() {
    use tos_common::crypto::Signature;

    let sender = KeyPair::new();
    let receiver = KeyPair::new();

    let payload = create_test_shield_payload(&receiver, 5000);
    let tx_type = TransactionType::ShieldTransfers(vec![payload]);

    let reference = Reference {
        topoheight: 100,
        hash: Hash::new([1u8; 32]),
    };

    let sig_bytes = [0u8; 64];
    let signature = Signature::from_bytes(&sig_bytes).unwrap();

    let tx = Transaction::new(
        TxVersion::T1,
        0,
        sender.get_public_key().compress(),
        tx_type,
        1000,
        FeeType::TOS,
        42,
        reference.clone(),
        None,
        signature,
    );

    // Full serialization roundtrip
    let bytes = tx.to_bytes();
    let mut reader = Reader::new(&bytes);
    let restored = Transaction::read(&mut reader).unwrap();

    assert_eq!(tx.get_version(), restored.get_version());
    assert_eq!(tx.get_source(), restored.get_source());
    assert_eq!(tx.get_fee(), restored.get_fee());
    assert_eq!(tx.get_nonce(), restored.get_nonce());
    assert_eq!(tx.hash(), restored.hash());

    // Verify Shield transfers are preserved
    match restored.get_data() {
        TransactionType::ShieldTransfers(transfers) => {
            assert_eq!(transfers.len(), 1);
            assert_eq!(transfers[0].get_amount(), 5000);
        }
        _ => unreachable!("Expected ShieldTransfers"),
    }
}
