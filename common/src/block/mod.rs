mod block;
mod compact;
mod header;
mod merkle;
mod miner;
mod version;

pub use block::Block;
pub use compact::{
    calculate_short_tx_id, CompactBlock, MissingTransactionsRequest, MissingTransactionsResponse,
    ShortTxId,
};
pub use header::BlockHeader;
pub use merkle::calculate_merkle_root;
pub use miner::{MinerWork, Worker};
pub use version::BlockVersion;

use crate::crypto::{Hash, HASH_SIZE};

// Topoheight is the height of the block in the blockdag
// It is a unique identifier for a block and can be changed during the unstable height
// due to a DAG reorganization
pub type TopoHeight = u64;

pub const EXTRA_NONCE_SIZE: usize = 32;
pub const HEADER_WORK_SIZE: usize = 73;

/// Size of the miner work structure for PoW calculation.
///
/// **SECURITY FIX**: Extended from 112 to 252 bytes to match `BLOCK_WORK_SIZE`.
/// This ensures MinerWork and BlockHeader use identical serialization for PoW,
/// following Kaspa's security model where ALL consensus fields are hash-protected.
///
/// This change is REQUIRED because PR #15 added GHOSTDAG fields to the block hash
/// calculation. Without this change, miners compute a different PoW hash than validators.
///
/// Breakdown (total 252 bytes = BLOCK_WORK_SIZE):
/// - work_hash: 32 bytes (covers: version, blue_score, parents, merkle_root)
/// - timestamp: 8 bytes
/// - nonce: 8 bytes
/// - extra_nonce: 32 bytes
/// - miner: 32 bytes
/// - daa_score: 8 bytes (GHOSTDAG)
/// - blue_work: 32 bytes (U256, GHOSTDAG)
/// - bits: 4 bytes (difficulty)
/// - pruning_point: 32 bytes (GHOSTDAG)
/// - accepted_id_merkle_root: 32 bytes
/// - utxo_commitment: 32 bytes
pub const MINER_WORK_SIZE: usize = BLOCK_WORK_SIZE;

/// Size of the full header serialization used for consensus hashing.
///
/// **SECURITY FIX (PR #12)**: Extended from 112 to 252 bytes to include ALL GHOSTDAG
/// consensus fields in the block hash. This follows Kaspa's security model where all
/// header fields are hash-protected to prevent peer manipulation during block propagation.
///
/// # Relationship with MINER_WORK_SIZE
///
/// - `BLOCK_WORK_SIZE` (252 bytes): Full header hash for consensus - used by nodes to
///   verify block identity. Changing any consensus field changes the block hash.
///
/// - `MINER_WORK_SIZE` (112 bytes): Legacy prefix for miner PoW work - used by external
///   miners. Miners only need to vary nonce/timestamp/extra_nonce within this prefix.
///
/// # Breakdown
///
/// Base fields (miner-controlled): 112 bytes (MINER_WORK_SIZE)
/// - work_hash: 32 bytes
/// - timestamp: 8 bytes
/// - nonce: 8 bytes
/// - extra_nonce: 32 bytes
/// - miner: 32 bytes
///
/// Added GHOSTDAG fields: 140 bytes
/// - daa_score: 8 bytes
/// - blue_work: 32 bytes (U256)
/// - bits: 4 bytes
/// - pruning_point: 32 bytes
/// - accepted_id_merkle_root: 32 bytes
/// - utxo_commitment: 32 bytes
///
/// Total: 112 + 140 = 252 bytes
pub const BLOCK_WORK_SIZE: usize = 252;

// Get combined hash for tips
// This is used to get a hash that is unique for a set of tips
pub fn get_combined_hash_for_tips<'a, H: AsRef<Hash>, I: Iterator<Item = H>>(tips: I) -> Hash {
    let mut bytes = [0u8; HASH_SIZE];
    for tip in tips {
        for i in 0..HASH_SIZE {
            bytes[i] ^= tip.as_ref().as_bytes()[i];
        }
    }
    Hash::new(bytes)
}

#[cfg(test)]
mod tests {
    use crate::crypto::Hash;

    #[test]
    fn test_one_hash() {
        let hash = Hash::new([255u8; 32]);
        let combined_hash = super::get_combined_hash_for_tips(std::iter::once(&hash));
        assert_eq!(combined_hash, hash);
    }
}
