//! Fuzz target for Merkle tree operations
//!
//! Tests that arbitrary inputs do not cause panics
//! when building or verifying Merkle trees.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use tos_common::crypto::Hash;

/// Arbitrary Merkle tree input
#[derive(Debug, Arbitrary)]
struct MerkleInput {
    /// Leaf data (list of hashes)
    leaves: Vec<[u8; 32]>,
    /// Index to verify
    verify_index: usize,
}

fuzz_target!(|input: MerkleInput| {
    // Don't process empty inputs
    if input.leaves.is_empty() || input.leaves.len() > 1000 {
        return;
    }

    // Build Merkle tree from leaves
    let leaves: Vec<Hash> = input
        .leaves
        .iter()
        .map(|bytes| Hash::new(*bytes))
        .collect();

    // Compute Merkle root
    let root = compute_merkle_root(&leaves);

    // Verify a leaf at given index
    if !leaves.is_empty() {
        let index = input.verify_index % leaves.len();
        let proof = generate_merkle_proof(&leaves, index);
        let _ = verify_merkle_proof(&root, &leaves[index], index, &proof);
    }
});

/// Compute Merkle root from leaves
fn compute_merkle_root(leaves: &[Hash]) -> Hash {
    if leaves.is_empty() {
        return Hash::zero();
    }
    if leaves.len() == 1 {
        return leaves[0];
    }

    let mut current_level: Vec<Hash> = leaves.to_vec();

    while current_level.len() > 1 {
        let mut next_level = Vec::new();
        for i in (0..current_level.len()).step_by(2) {
            let left = current_level[i];
            let right = if i + 1 < current_level.len() {
                current_level[i + 1]
            } else {
                current_level[i] // Duplicate last if odd
            };
            next_level.push(hash_pair(&left, &right));
        }
        current_level = next_level;
    }

    current_level[0]
}

/// Generate Merkle proof for leaf at index
fn generate_merkle_proof(leaves: &[Hash], index: usize) -> Vec<Hash> {
    if leaves.len() <= 1 {
        return vec![];
    }

    let mut proof = Vec::new();
    let mut current_level: Vec<Hash> = leaves.to_vec();
    let mut idx = index;

    while current_level.len() > 1 {
        let sibling_idx = if idx % 2 == 0 { idx + 1 } else { idx - 1 };
        if sibling_idx < current_level.len() {
            proof.push(current_level[sibling_idx]);
        } else {
            proof.push(current_level[idx]);
        }

        // Build next level
        let mut next_level = Vec::new();
        for i in (0..current_level.len()).step_by(2) {
            let left = current_level[i];
            let right = if i + 1 < current_level.len() {
                current_level[i + 1]
            } else {
                current_level[i]
            };
            next_level.push(hash_pair(&left, &right));
        }
        current_level = next_level;
        idx /= 2;
    }

    proof
}

/// Verify Merkle proof
fn verify_merkle_proof(root: &Hash, leaf: &Hash, index: usize, proof: &[Hash]) -> bool {
    let mut computed = *leaf;
    let mut idx = index;

    for sibling in proof {
        computed = if idx % 2 == 0 {
            hash_pair(&computed, sibling)
        } else {
            hash_pair(sibling, &computed)
        };
        idx /= 2;
    }

    computed == *root
}

/// Hash two nodes together
fn hash_pair(left: &Hash, right: &Hash) -> Hash {
    let mut combined = [0u8; 64];
    combined[..32].copy_from_slice(left.as_bytes());
    combined[32..].copy_from_slice(right.as_bytes());
    // Simple XOR-based hash for fuzzing (real impl uses SHA3)
    let mut result = [0u8; 32];
    for i in 0..32 {
        result[i] = combined[i] ^ combined[i + 32];
    }
    Hash::new(result)
}
