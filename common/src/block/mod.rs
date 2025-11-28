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
pub use miner::{Algorithm, MinerWork, Worker};
pub use version::BlockVersion;

use crate::crypto::{Hash, HASH_SIZE};

// Topoheight is the height of the block in the blockdag
// It is a unique identifier for a block and can be changed during the unstable height
// due to a DAG reorganization
pub type TopoHeight = u64;

pub const EXTRA_NONCE_SIZE: usize = 32;
pub const HEADER_WORK_SIZE: usize = 73;

// MINER_WORK_SIZE: Size of the miner-controlled fields in the block header
// Used by MinerWork for stratum mining protocol
// 32 (work_hash) + 8 (timestamp) + 8 (nonce) + 32 (extra_nonce) + 32 (miner) = 112 bytes
pub const MINER_WORK_SIZE: usize = 112;

// SECURITY FIX: Updated BLOCK_WORK_SIZE to include ALL GHOSTDAG consensus fields in block hash
// This follows Kaspa's security model where all header fields are hash-protected
// to prevent peer manipulation during block propagation.
//
// Base fields (miner-controlled): 112 bytes (MINER_WORK_SIZE)
// Added GHOSTDAG fields: 8 (daa_score) + 32 (blue_work) + 4 (bits) + 32 (pruning_point) + 32 (accepted_id_merkle_root) + 32 (utxo_commitment) = 140 bytes
// New total: 112 + 140 = 252 bytes
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
