// TOS Compact Block Implementation
//
// Compact blocks reduce bandwidth by 95-98% by sending:
// - Full block header (~200 bytes)
// - Short transaction IDs (6 bytes each)
// - Prefilled transactions (coinbase + any new txs)
//
// Receivers reconstruct the block from their mempool.

use crate::{
    block::{BlockHeader, Block},
    crypto::{Hash, Hashable},
    serializer::{Serializer, Reader, ReaderError, Writer},
    transaction::Transaction,
};
use siphasher::sip::SipHasher13;
use std::hash::Hasher;

/// Short transaction ID (48-bit)
/// Uses SipHash to compress 256-bit transaction ID to 48 bits
pub type ShortTxId = [u8; 6];

/// Compact block for bandwidth-efficient propagation
#[derive(Clone, Debug)]
pub struct CompactBlock {
    /// Full block header (needed for validation)
    pub header: BlockHeader,

    /// Nonce for short ID calculation (prevents adversarial collisions)
    pub nonce: u64,

    /// Short transaction IDs (6 bytes each)
    /// Generated using SipHash(nonce || full_tx_id)[0..6]
    pub short_tx_ids: Vec<ShortTxId>,

    /// Prefilled transactions with their index
    /// Typically includes coinbase and any new transactions
    /// Format: (index_in_block, transaction)
    pub prefilled_txs: Vec<(u16, Transaction)>,
}

impl CompactBlock {
    /// Create a new compact block from a full block
    pub fn from_block(block: Block, nonce: u64) -> Self {
        let transactions = block.get_transactions();
        let mut short_tx_ids = Vec::with_capacity(transactions.len());
        let mut prefilled_txs = Vec::new();

        // Always prefill the coinbase transaction (first transaction)
        if !transactions.is_empty() {
            // Clone the Arc<Transaction> to get Transaction
            prefilled_txs.push((0, (*transactions[0]).clone()));
        }

        // Generate short IDs for all transactions
        for tx in transactions.iter() {
            let tx_hash = tx.hash();
            let short_id = calculate_short_tx_id(nonce, &tx_hash);
            short_tx_ids.push(short_id);

            // Prefill any transaction that's likely new (e.g., very recent timestamp)
            // For now, we only prefill coinbase
        }

        Self {
            header: block.get_header().clone(),
            nonce,
            short_tx_ids,
            prefilled_txs,
        }
    }

    /// Get the size of this compact block in bytes
    pub fn size(&self) -> usize {
        let mut size = 0;
        size += self.header.size();
        size += 8; // nonce
        size += 2; // short_tx_ids length
        size += self.short_tx_ids.len() * 6;
        size += 2; // prefilled_txs length
        for (_, tx) in &self.prefilled_txs {
            size += 2; // index
            size += tx.size();
        }
        size
    }

    /// Calculate expected bandwidth savings
    pub fn compression_ratio(&self, full_block_size: usize) -> f64 {
        let compact_size = self.size();
        1.0 - (compact_size as f64 / full_block_size as f64)
    }
}

/// Calculate short transaction ID using SipHash
///
/// Algorithm:
/// 1. Use block nonce as SipHash key (prevents adversarial collisions)
/// 2. Hash the full transaction ID
/// 3. Take first 48 bits (6 bytes) of the hash
pub fn calculate_short_tx_id(nonce: u64, tx_id: &Hash) -> ShortTxId {
    let mut hasher = SipHasher13::new_with_keys(nonce, 0);
    hasher.write(tx_id.as_bytes());
    let hash = hasher.finish();

    // Take first 6 bytes (48 bits) of the 64-bit hash
    let bytes = hash.to_le_bytes();
    [bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5]]
}

/// Request for missing transactions during block reconstruction
#[derive(Clone, Debug)]
pub struct MissingTransactionsRequest {
    /// Block hash of the compact block
    pub block_hash: Hash,

    /// Indices of missing transactions
    pub missing_indices: Vec<u16>,
}

/// Response containing missing transactions
#[derive(Clone, Debug)]
pub struct MissingTransactionsResponse {
    /// Block hash of the compact block
    pub block_hash: Hash,

    /// Missing transactions in order of request
    pub transactions: Vec<Transaction>,
}

impl Serializer for CompactBlock {
    fn write(&self, writer: &mut Writer) {
        // Write header
        self.header.write(writer);

        // Write nonce
        writer.write_u64(&self.nonce);

        // Write short transaction IDs
        writer.write_u16(self.short_tx_ids.len() as u16);
        for short_id in &self.short_tx_ids {
            writer.write_bytes(short_id);
        }

        // Write prefilled transactions
        writer.write_u16(self.prefilled_txs.len() as u16);
        for (index, tx) in &self.prefilled_txs {
            writer.write_u16(*index);
            tx.write(writer);
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        // Read header
        let header = BlockHeader::read(reader)?;

        // Read nonce
        let nonce = reader.read_u64()?;

        // Read short transaction IDs
        let short_tx_ids_len = reader.read_u16()? as usize;
        if short_tx_ids_len > 100_000 {
            return Err(ReaderError::InvalidSize);
        }

        let mut short_tx_ids = Vec::with_capacity(short_tx_ids_len);
        for _ in 0..short_tx_ids_len {
            let bytes = reader.read_bytes_ref(6)?;
            let mut short_id = [0u8; 6];
            short_id.copy_from_slice(bytes);
            short_tx_ids.push(short_id);
        }

        // Read prefilled transactions
        let prefilled_len = reader.read_u16()? as usize;
        if prefilled_len > short_tx_ids_len {
            return Err(ReaderError::InvalidSize);
        }

        let mut prefilled_txs = Vec::with_capacity(prefilled_len);
        for _ in 0..prefilled_len {
            let index = reader.read_u16()?;
            let tx = Transaction::read(reader)?;
            prefilled_txs.push((index, tx));
        }

        Ok(Self {
            header,
            nonce,
            short_tx_ids,
            prefilled_txs,
        })
    }

    fn size(&self) -> usize {
        self.size()
    }
}

impl Serializer for MissingTransactionsRequest {
    fn write(&self, writer: &mut Writer) {
        writer.write_hash(&self.block_hash);
        writer.write_u16(self.missing_indices.len() as u16);
        for idx in &self.missing_indices {
            writer.write_u16(*idx);
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let block_hash = reader.read_hash()?;
        let len = reader.read_u16()? as usize;

        if len > 100_000 {
            return Err(ReaderError::InvalidSize);
        }

        let mut missing_indices = Vec::with_capacity(len);
        for _ in 0..len {
            missing_indices.push(reader.read_u16()?);
        }

        Ok(Self {
            block_hash,
            missing_indices,
        })
    }

    fn size(&self) -> usize {
        32 + 2 + self.missing_indices.len() * 2
    }
}

impl Serializer for MissingTransactionsResponse {
    fn write(&self, writer: &mut Writer) {
        writer.write_hash(&self.block_hash);
        writer.write_u16(self.transactions.len() as u16);
        for tx in &self.transactions {
            tx.write(writer);
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let block_hash = reader.read_hash()?;
        let len = reader.read_u16()? as usize;

        if len > 100_000 {
            return Err(ReaderError::InvalidSize);
        }

        let mut transactions = Vec::with_capacity(len);
        for _ in 0..len {
            transactions.push(Transaction::read(reader)?);
        }

        Ok(Self {
            block_hash,
            transactions,
        })
    }

    fn size(&self) -> usize {
        let mut size = 32 + 2; // block_hash + len
        for tx in &self.transactions {
            size += tx.size();
        }
        size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_tx_id_generation() {
        let nonce = 12345u64;
        let tx_id = Hash::new([1u8; 32]);

        let short_id = calculate_short_tx_id(nonce, &tx_id);

        // Short ID should be 6 bytes
        assert_eq!(short_id.len(), 6);

        // Same nonce and tx_id should produce same short ID
        let short_id2 = calculate_short_tx_id(nonce, &tx_id);
        assert_eq!(short_id, short_id2);

        // Different nonce should produce different short ID
        let short_id3 = calculate_short_tx_id(54321u64, &tx_id);
        assert_ne!(short_id, short_id3);
    }

    #[test]
    fn test_compact_block_serialization() {
        use crate::block::BlockVersion;
        use crate::crypto::elgamal::CompressedPublicKey;
        use curve25519_dalek::ristretto::CompressedRistretto;
        use primitive_types::U256;

        // Create a simple block header
        let parents = vec![vec![Hash::new([0u8; 32])]]; // Level 0 parents
        let miner = CompressedPublicKey::new(CompressedRistretto([1u8; 32]));
        let header = BlockHeader::new(
            BlockVersion::V0,
            parents,                      // parents_by_level
            100,                          // blue_score
            100,                          // daa_score
            U256::zero(),                 // blue_work
            Hash::new([0u8; 32]),         // pruning_point
            1234567890,                   // timestamp
            0,                            // bits
            [0u8; 32],                    // extra_nonce
            miner,                        // miner
            Hash::new([0u8; 32]),         // hash_merkle_root
            Hash::new([0u8; 32]),         // accepted_id_merkle_root
            Hash::new([0u8; 32])          // utxo_commitment
        );

        // Create empty compact block
        let compact_block = CompactBlock {
            header: header.clone(),
            nonce: 999,
            short_tx_ids: vec![[1,2,3,4,5,6], [7,8,9,10,11,12]],
            prefilled_txs: vec![],
        };

        // Serialize
        let mut bytes = Vec::new();
        let mut writer = Writer::new(&mut bytes);
        compact_block.write(&mut writer);

        // Deserialize
        let mut reader = Reader::new(&bytes);
        let deserialized = CompactBlock::read(&mut reader).unwrap();

        // Verify
        assert_eq!(deserialized.nonce, 999);
        assert_eq!(deserialized.short_tx_ids.len(), 2);
        assert_eq!(deserialized.short_tx_ids[0], [1,2,3,4,5,6]);
        assert_eq!(deserialized.short_tx_ids[1], [7,8,9,10,11,12]);
    }
}
