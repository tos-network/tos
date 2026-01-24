//! Transaction processing test module for TOS TCK.
//!
//! This module provides comprehensive tests for all transaction-related
//! functionality including fee calculation, transaction selection, nonce
//! ordering, block assembly, verify/apply phase separation, mempool
//! operations, and chain reorganization handling.

/// Fee model tests - pure function testing for fee calculation
pub mod fee_model;

/// Transaction selector tests - priority ordering and block packing
pub mod tx_selector;

/// Nonce ordering tests - per-account sequencing and gap handling
pub mod nonce_ordering;

/// Block assembly tests - transaction inclusion and block construction
pub mod block_assembly;

/// Verify/apply phase separation tests - stateless verification vs state mutation
pub mod verify_apply;

/// Mempool operation tests - add/remove/eviction/expiry
pub mod mempool_ops;

/// Chain reorganization handling tests - reorg replay and mempool recovery
pub mod reorg_handling;

use std::sync::Arc;
use tos_common::crypto::elgamal::{
    CompressedPublicKey, KeyPair, PedersenOpening, PublicKey, Signature,
};
use tos_common::crypto::proofs::CiphertextValidityProof;
use tos_common::crypto::{Hash, Hashable, PrivateKey};
use tos_common::serializer::Serializer;
use tos_common::transaction::{
    FeeType, Reference, Transaction, TransactionType, TransferPayload, TxVersion,
    UnoTransferPayload,
};
use tos_crypto::curve25519_dalek::Scalar;
use tos_crypto::merlin::Transcript;

/// Represents a mock transaction with associated metadata for testing.
pub struct MockTransaction {
    /// The computed hash of the transaction
    pub hash: Arc<Hash>,
    /// The wrapped transaction object
    pub tx: Arc<Transaction>,
    /// The byte size of the transaction (may be overridden)
    pub size: usize,
}

/// Creates a deterministic CompressedPublicKey from an integer seed.
/// Different seeds produce different keys, enabling distinct sender identities in tests.
pub fn make_source(seed: u64) -> CompressedPublicKey {
    // Use hash-based derivation to create a valid Ristretto point
    let seed_hash = Hash::new({
        let mut bytes = [0u8; 32];
        let seed_bytes = seed.to_le_bytes();
        bytes[..8].copy_from_slice(&seed_bytes);
        bytes
    });
    PublicKey::from_hash(&seed_hash).compress()
}

/// Create a deterministic KeyPair from a seed byte.
/// Uses the seed to derive a non-zero scalar for reproducible testing.
pub fn make_keypair(seed: u8) -> KeyPair {
    let mut bytes = [0u8; 32];
    bytes[0] = seed.wrapping_add(1); // ensure non-zero
    bytes[1] = seed.wrapping_mul(7).wrapping_add(3);
    bytes[2] = seed.wrapping_mul(13).wrapping_add(37);
    bytes[31] = 0; // ensure the scalar is canonical (MSB clear)
    let scalar = Scalar::from_bytes_mod_order(bytes);
    let private_key = PrivateKey::from_scalar(scalar);
    KeyPair::from_private_key(private_key)
}

// Creates a dummy signature for test transactions.
// Not cryptographically valid, but sufficient for selector ordering tests.
fn make_dummy_signature() -> Signature {
    Signature::new(Scalar::ZERO, Scalar::ZERO)
}

// Creates a unique hash from an integer, used to identify each mock transaction
fn make_hash(id: u64) -> Hash {
    let mut bytes = [0u8; 32];
    let id_bytes = id.to_le_bytes();
    bytes[..8].copy_from_slice(&id_bytes);
    Hash::new(bytes)
}

// Creates a dummy UnoTransferPayload with valid crypto fields for testing.
// Uses real keypairs and proofs for structural validity in ordering tests.
fn make_dummy_uno_transfer() -> UnoTransferPayload {
    let sender_keypair = KeyPair::new();
    let receiver_keypair = KeyPair::new();
    let destination = receiver_keypair.get_public_key().compress();

    let opening = PedersenOpening::generate_new();
    let commitment =
        tos_common::crypto::elgamal::PedersenCommitment::new_with_opening(100u64, &opening);
    let sender_handle = sender_keypair.get_public_key().decrypt_handle(&opening);
    let receiver_handle = receiver_keypair.get_public_key().decrypt_handle(&opening);

    let mut transcript = Transcript::new(b"mock_uno_transfer");
    let proof = CiphertextValidityProof::new(
        receiver_keypair.get_public_key(),
        sender_keypair.get_public_key(),
        100u64,
        &opening,
        TxVersion::T1,
        &mut transcript,
    );

    UnoTransferPayload::new(
        Hash::zero(),
        destination,
        None,
        commitment.compress(),
        sender_handle.compress(),
        receiver_handle.compress(),
        proof,
    )
}

/// Builder for constructing MockTransaction instances with a fluent API.
///
/// Defaults to FeeType::TOS, fee=10000, nonce=0, source=make_source(0),
/// transfer_count=1, tx_id=0.
pub struct MockTransactionBuilder {
    source: CompressedPublicKey,
    fee: u64,
    fee_type: FeeType,
    nonce: u64,
    transfer_count: usize,
    size_override: Option<usize>,
    tx_id: u64,
}

impl MockTransactionBuilder {
    /// Create a new builder with default values (TOS fee type, fee=10000).
    pub fn new() -> Self {
        Self {
            source: make_source(0),
            fee: 10_000,
            fee_type: FeeType::TOS,
            nonce: 0,
            transfer_count: 1,
            size_override: None,
            tx_id: 0,
        }
    }

    /// Set the fee amount.
    pub fn with_fee(mut self, fee: u64) -> Self {
        self.fee = fee;
        self
    }

    /// Set the fee type (TOS, Energy, or UNO).
    pub fn with_fee_type(mut self, fee_type: FeeType) -> Self {
        self.fee_type = fee_type;
        self
    }

    /// Set the nonce value.
    pub fn with_nonce(mut self, nonce: u64) -> Self {
        self.nonce = nonce;
        self
    }

    /// Set the source (sender) public key.
    pub fn with_source(mut self, source: CompressedPublicKey) -> Self {
        self.source = source;
        self
    }

    /// Set the number of transfer outputs to create.
    pub fn with_transfers(mut self, count: usize) -> Self {
        self.transfer_count = count;
        self
    }

    /// Override the byte size reported by MockTransaction.
    pub fn with_size(mut self, size: usize) -> Self {
        self.size_override = Some(size);
        self
    }

    /// Set a unique transaction ID for hash generation.
    pub fn with_tx_id(mut self, id: u64) -> Self {
        self.tx_id = id;
        self
    }

    /// Build the MockTransaction from the configured parameters.
    pub fn build(self) -> MockTransaction {
        let data = match self.fee_type {
            FeeType::UNO => {
                // For UNO transactions, create UnoTransferPayloads with valid crypto
                let transfers: Vec<UnoTransferPayload> = (0..self.transfer_count)
                    .map(|_| make_dummy_uno_transfer())
                    .collect();
                TransactionType::UnoTransfers(transfers)
            }
            _ => {
                // For TOS and Energy fee types, use regular transfers
                let transfers: Vec<TransferPayload> = (0..self.transfer_count)
                    .map(|i| {
                        TransferPayload::new(
                            Hash::zero(),
                            make_source(100 + i as u64),
                            1_000_000,
                            None,
                        )
                    })
                    .collect();
                TransactionType::Transfers(transfers)
            }
        };

        let reference = Reference {
            hash: Hash::zero(),
            topoheight: 0,
        };

        let tx = Transaction::new(
            TxVersion::T1,
            0, // chain_id
            self.source,
            data,
            self.fee,
            self.fee_type,
            self.nonce,
            reference,
            None,
            make_dummy_signature(),
        );

        let hash = if self.tx_id > 0 {
            make_hash(self.tx_id)
        } else {
            tx.hash()
        };

        let actual_size = tx.size();
        let size = self.size_override.unwrap_or(actual_size);

        MockTransaction {
            hash: Arc::new(hash),
            tx: Arc::new(tx),
            size,
        }
    }
}

impl Default for MockTransactionBuilder {
    fn default() -> Self {
        Self::new()
    }
}
