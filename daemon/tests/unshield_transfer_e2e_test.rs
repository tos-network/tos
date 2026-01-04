//! End-to-end tests for Unshield transfer functionality (UNO -> TOS)
//!
//! These tests verify the Unshield transfer infrastructure:
//! 1. UnshieldTransferPayload creation with valid CiphertextValidityProof
//! 2. Transaction serialization with Unshield transfers
//! 3. TransactionType::UnshieldTransfers (opcode 20) serialization
//!
//! Unshield transfers convert encrypted UNO balance back to plaintext TOS balance.

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
        Reference, Transaction, TransactionType, TransferPayload, TxVersion,
        UnshieldTransferPayload,
    },
};

/// Helper: Create a test UnshieldTransferPayload with valid CiphertextValidityProof
fn create_test_unshield_payload(
    sender_keypair: &KeyPair,
    receiver_keypair: &KeyPair,
    amount: u64,
) -> UnshieldTransferPayload {
    let destination = receiver_keypair.get_public_key().compress();
    let opening = PedersenOpening::generate_new();

    // Create commitment and sender handle
    let commitment = PedersenCommitment::new_with_opening(amount, &opening);
    let sender_handle = sender_keypair.get_public_key().decrypt_handle(&opening);

    // Create CiphertextValidityProof (proves commitment encodes the claimed amount)
    // Note: For serialization compatibility with TxVersion::T0, we provide both pubkeys
    // so that Y_2 is included in the proof.
    let mut transcript = tos_common::crypto::new_proof_transcript(b"unshield_validity_proof");
    let proof = CiphertextValidityProof::new(
        sender_keypair.get_public_key(),   // destination pubkey
        receiver_keypair.get_public_key(), // source pubkey (no Option wrapper)
        amount,
        &opening,
        TxVersion::T1, // current version with Y_2 support
        &mut transcript,
    );

    UnshieldTransferPayload::new(
        TOS_ASSET, // Unshield converts to TOS_ASSET
        destination,
        amount,
        None,
        commitment.compress(),
        sender_handle.compress(),
        proof,
    )
}

/// Test UnshieldTransferPayload creation and serialization roundtrip
#[test]
fn test_unshield_transfer_payload_serialization() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();

    let payload = create_test_unshield_payload(&sender, &receiver, 100);

    // Verify basic properties
    assert_eq!(payload.get_asset(), &TOS_ASSET);
    assert_eq!(payload.get_amount(), 100);
    assert_eq!(
        payload.get_destination(),
        &receiver.get_public_key().compress()
    );

    // Test serialization roundtrip (with TxVersion context)
    let bytes = payload.to_bytes();
    let mut context = Context::new();
    context.store(TxVersion::T0);
    let mut reader = Reader::with_context(&bytes, context);
    let restored = UnshieldTransferPayload::read(&mut reader).unwrap();

    assert_eq!(payload.get_asset(), restored.get_asset());
    assert_eq!(payload.get_destination(), restored.get_destination());
    assert_eq!(payload.get_amount(), restored.get_amount());
    assert_eq!(payload.get_commitment(), restored.get_commitment());
    assert_eq!(payload.get_sender_handle(), restored.get_sender_handle());
}

/// Test TransactionType::UnshieldTransfers serialization (opcode 20)
#[test]
fn test_unshield_transfers_transaction_type() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();

    let payload = create_test_unshield_payload(&sender, &receiver, 1000);
    let tx_type = TransactionType::UnshieldTransfers(vec![payload]);

    // Serialize
    let bytes = tx_type.to_bytes();

    // First byte should be opcode 20 for UnshieldTransfers
    assert_eq!(bytes[0], 20, "UnshieldTransfers should use opcode 20");

    // Deserialize with context
    let mut context = Context::new();
    context.store(TxVersion::T0);
    let mut reader = Reader::with_context(&bytes, context);
    let restored = TransactionType::read(&mut reader).unwrap();

    // Verify it's the right variant
    match restored {
        TransactionType::UnshieldTransfers(transfers) => {
            assert_eq!(transfers.len(), 1);
            assert_eq!(transfers[0].get_asset(), &TOS_ASSET);
            assert_eq!(transfers[0].get_amount(), 1000);
        }
        _ => panic!("Expected UnshieldTransfers variant"),
    }
}

/// Test multiple Unshield transfers in a single transaction
#[test]
fn test_multiple_unshield_transfers() {
    let sender = KeyPair::new();
    let receivers: Vec<_> = (0..3).map(|_| KeyPair::new()).collect();

    let mut payloads = Vec::new();
    for (i, receiver) in receivers.iter().enumerate() {
        let amount = (100 * (i + 1)) as u64;
        let payload = create_test_unshield_payload(&sender, receiver, amount);
        payloads.push(payload);
    }

    let tx_type = TransactionType::UnshieldTransfers(payloads);

    // Verify serialization
    let bytes = tx_type.to_bytes();
    let mut context = Context::new();
    context.store(TxVersion::T0);
    let mut reader = Reader::with_context(&bytes, context);
    let restored = TransactionType::read(&mut reader).unwrap();

    match restored {
        TransactionType::UnshieldTransfers(transfers) => {
            assert_eq!(transfers.len(), 3);
            assert_eq!(transfers[0].get_amount(), 100);
            assert_eq!(transfers[1].get_amount(), 200);
            assert_eq!(transfers[2].get_amount(), 300);
        }
        _ => panic!("Expected UnshieldTransfers variant"),
    }
}

/// Test that Transaction with UnshieldTransfers can be hashed
#[test]
fn test_transaction_with_unshield_transfers_hashable() {
    use tos_common::crypto::Signature;

    let sender = KeyPair::new();
    let receiver = KeyPair::new();

    let payload = create_test_unshield_payload(&sender, &receiver, 100);
    let tx_type = TransactionType::UnshieldTransfers(vec![payload]);

    let reference = Reference {
        topoheight: 0,
        hash: Hash::zero(),
    };

    // Create a dummy signature (64 bytes)
    let sig_bytes = [0u8; 64];
    let signature = Signature::from_bytes(&sig_bytes).unwrap();

    let tx = Transaction::new(
        TxVersion::T0,
        0,
        sender.get_public_key().compress(),
        tx_type,
        1000,
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

    // Verify it's recognized as UNO transaction (Unshield is part of UNO ecosystem)
    assert!(tx.has_uno_transfers());
}

/// Test UnshieldTransferPayload size calculation
#[test]
fn test_unshield_transfer_payload_size() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();

    let payload = create_test_unshield_payload(&sender, &receiver, 100);

    // Verify size() matches actual serialized bytes
    let bytes = payload.to_bytes();
    assert_eq!(payload.size(), bytes.len());
}

/// Test Unshield transfer with extra data (memo)
#[test]
fn test_unshield_transfer_with_extra_data() {
    use tos_common::transaction::extra_data::UnknownExtraDataFormat;

    let sender = KeyPair::new();
    let receiver = KeyPair::new();
    let opening = PedersenOpening::generate_new();
    let amount = 500u64;

    let commitment = PedersenCommitment::new_with_opening(amount, &opening);
    let sender_handle = sender.get_public_key().decrypt_handle(&opening);

    // Note: Include both pubkeys for serialization compatibility
    let mut transcript = tos_common::crypto::new_proof_transcript(b"unshield_validity_proof");
    let proof = CiphertextValidityProof::new(
        sender.get_public_key(),
        receiver.get_public_key(), // source pubkey (no Option wrapper)
        amount,
        &opening,
        TxVersion::T1, // current version with Y_2 support
        &mut transcript,
    );

    let extra_data = Some(UnknownExtraDataFormat(vec![1, 2, 3, 4, 5]));
    let payload = UnshieldTransferPayload::new(
        TOS_ASSET,
        receiver.get_public_key().compress(),
        amount,
        extra_data,
        commitment.compress(),
        sender_handle.compress(),
        proof,
    );

    assert!(payload.get_extra_data().is_some());

    // Verify serialization roundtrip with extra data
    let bytes = payload.to_bytes();
    let mut context = Context::new();
    context.store(TxVersion::T0);
    let mut reader = Reader::with_context(&bytes, context);
    let restored = UnshieldTransferPayload::read(&mut reader).unwrap();
    assert!(restored.get_extra_data().is_some());
}

/// Test UnshieldTransferPayload consume method
#[test]
fn test_unshield_transfer_payload_consume() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();

    let payload = create_test_unshield_payload(&sender, &receiver, 100);

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

/// Test Unshield with various amounts
#[test]
fn test_unshield_transfer_various_amounts() {
    let amounts = [1u64, 100, 1000, 1_000_000, u64::MAX / 2];

    for amount in amounts {
        let sender = KeyPair::new();
        let receiver = KeyPair::new();
        let payload = create_test_unshield_payload(&sender, &receiver, amount);

        assert_eq!(payload.get_amount(), amount);

        // Verify serialization roundtrip
        let bytes = payload.to_bytes();
        let mut context = Context::new();
        context.store(TxVersion::T0);
        let mut reader = Reader::with_context(&bytes, context);
        let restored = UnshieldTransferPayload::read(&mut reader).unwrap();
        assert_eq!(restored.get_amount(), amount);
    }
}

/// Test that Unshield uses TOS_ASSET (converts to plaintext TOS)
#[test]
fn test_unshield_uses_tos_asset() {
    let sender = KeyPair::new();
    let receiver = KeyPair::new();
    let payload = create_test_unshield_payload(&sender, &receiver, 100);

    // Unshield converts to TOS asset (receiver gets plaintext TOS)
    assert_eq!(payload.get_asset(), &TOS_ASSET);
    assert_ne!(payload.get_asset(), &UNO_ASSET);
}

/// Test plaintext Transaction still works alongside Unshield
#[test]
fn test_plaintext_transaction_distinct_from_unshield() {
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
        TxVersion::T0,
        0,
        keypair.get_public_key().compress(),
        TransactionType::Transfers(vec![transfer]),
        1000,
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

/// Test Unshield transaction full serialization roundtrip
#[test]
fn test_unshield_full_transaction_serialization() {
    use tos_common::crypto::Signature;

    let sender = KeyPair::new();
    let receiver = KeyPair::new();

    let payload = create_test_unshield_payload(&sender, &receiver, 5000);
    let tx_type = TransactionType::UnshieldTransfers(vec![payload]);

    let reference = Reference {
        topoheight: 100,
        hash: Hash::new([1u8; 32]),
    };

    let sig_bytes = [0u8; 64];
    let signature = Signature::from_bytes(&sig_bytes).unwrap();

    let tx = Transaction::new(
        TxVersion::T0,
        0,
        sender.get_public_key().compress(),
        tx_type,
        1000,
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
    assert_eq!(tx.get_fee_limit(), restored.get_fee_limit());
    assert_eq!(tx.get_nonce(), restored.get_nonce());
    assert_eq!(tx.hash(), restored.hash());

    // Verify Unshield transfers are preserved
    match restored.get_data() {
        TransactionType::UnshieldTransfers(transfers) => {
            assert_eq!(transfers.len(), 1);
            assert_eq!(transfers[0].get_amount(), 5000);
        }
        _ => panic!("Expected UnshieldTransfers"),
    }
}

/// Test Unshield sender ciphertext structure
#[test]
fn test_unshield_sender_ciphertext() {
    use tos_common::crypto::elgamal::Ciphertext;

    let sender = KeyPair::new();
    let receiver = KeyPair::new();

    let payload = create_test_unshield_payload(&sender, &receiver, 250);

    // Get components for sender ciphertext
    let commitment = payload.get_commitment().decompress().unwrap();
    let sender_handle = payload.get_sender_handle().decompress().unwrap();

    // Create ciphertext from components
    let sender_ct = Ciphertext::new(commitment, sender_handle);

    // Verify ciphertext components match
    assert_eq!(sender_ct.commitment().compress(), *payload.get_commitment());
    assert_eq!(sender_ct.handle().compress(), *payload.get_sender_handle());
}

/// Test opcode distinction between Shield (19) and Unshield (20)
#[test]
fn test_shield_unshield_opcode_distinction() {
    use tos_common::crypto::proofs::ShieldCommitmentProof;
    use tos_common::transaction::ShieldTransferPayload;

    let sender = KeyPair::new();
    let receiver = KeyPair::new();

    // Create Shield payload
    let shield_opening = PedersenOpening::generate_new();
    let shield_commitment = PedersenCommitment::new_with_opening(100u64, &shield_opening);
    let shield_handle = receiver.get_public_key().decrypt_handle(&shield_opening);
    let mut shield_transcript =
        tos_common::crypto::new_proof_transcript(b"shield_commitment_proof");
    let shield_proof = ShieldCommitmentProof::new(
        receiver.get_public_key(),
        100u64,
        &shield_opening,
        &mut shield_transcript,
    );
    let shield_payload = ShieldTransferPayload::new(
        TOS_ASSET,
        receiver.get_public_key().compress(),
        100u64,
        None,
        shield_commitment.compress(),
        shield_handle.compress(),
        shield_proof,
    );

    // Create Unshield payload
    let unshield_payload = create_test_unshield_payload(&sender, &receiver, 100);

    // Serialize both transaction types
    let shield_tx = TransactionType::ShieldTransfers(vec![shield_payload]);
    let unshield_tx = TransactionType::UnshieldTransfers(vec![unshield_payload]);

    let shield_bytes = shield_tx.to_bytes();
    let unshield_bytes = unshield_tx.to_bytes();

    // Verify different opcodes
    assert_eq!(shield_bytes[0], 19, "Shield should use opcode 19");
    assert_eq!(unshield_bytes[0], 20, "Unshield should use opcode 20");
    assert_ne!(shield_bytes[0], unshield_bytes[0]);
}
