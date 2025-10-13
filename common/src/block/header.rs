// BlockHeader - GHOSTDAG-native DAG header format
//
// This is a BREAKING CHANGE from the legacy chain-based format.

use std::fmt::{Display, Formatter, Error as FmtError};
use serde::Deserialize;
use log::debug;
use crate::{
    block::{BLOCK_WORK_SIZE, HEADER_WORK_SIZE, BlockVersion},
    config::{TIPS_LIMIT, MAX_PARENT_LEVELS},
    crypto::{
        elgamal::CompressedPublicKey,
        hash,
        pow_hash,
        Hash,
        Hashable,
        BlueWorkType,
        BlueWorkWriter,
        BlueWorkReader,
        HASH_SIZE
    },
    serializer::{Reader, ReaderError, Serializer, Writer},
    time::TimestampMillis,
};
use tos_hash::Error as TosHashError;
use super::{Algorithm, MinerWork, EXTRA_NONCE_SIZE};

// Serialize the extra nonce in a hexadecimal string
pub fn serialize_extra_nonce<S: serde::Serializer>(extra_nonce: &[u8; EXTRA_NONCE_SIZE], s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&hex::encode(extra_nonce))
}

// Deserialize the extra nonce from a hexadecimal string
pub fn deserialize_extra_nonce<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<[u8; EXTRA_NONCE_SIZE], D::Error> {
    let mut extra_nonce = [0u8; EXTRA_NONCE_SIZE];
    let hex = String::deserialize(deserializer)?;
    let decoded = hex::decode(hex).map_err(serde::de::Error::custom)?;

    // SECURITY FIX: Validate length before copy_from_slice to prevent panic
    // An attacker could send malformed extraNonce with wrong length, causing node crash
    if decoded.len() != EXTRA_NONCE_SIZE {
        return Err(serde::de::Error::custom(
            format!("Invalid extraNonce length: expected {} bytes, got {}", EXTRA_NONCE_SIZE, decoded.len())
        ));
    }

    extra_nonce.copy_from_slice(&decoded);
    Ok(extra_nonce)
}

/// GHOSTDAG-native block header
///
/// This header format supports DAG structure with GHOSTDAG consensus.
/// Key differences from legacy format:
/// - `parents_by_level` replaces simple `tips` list
/// - `blue_score` replaces linear `height`
/// - Added `daa_score`, `blue_work`, `pruning_point`
/// - `hash_merkle_root` replaces inline `txs_hashes`
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct BlockHeader {
    // Version and format
    pub version: BlockVersion,

    // DAG structure - parents organized by level
    // parents_by_level[0] = direct parents (equivalent to old "tips")
    // parents_by_level[1] = grandparents not in level 0, etc.
    pub parents_by_level: Vec<Vec<Hash>>,

    // GHOSTDAG scores
    pub blue_score: u64,      // Position in blue (selected) chain
    pub daa_score: u64,       // Difficulty adjustment score
    pub blue_work: BlueWorkType, // Cumulative blue work (U256)

    // Pruning
    pub pruning_point: Hash,  // Reference to pruning point

    // Mining fields
    pub timestamp: TimestampMillis,
    pub nonce: u64,
    #[serde(serialize_with = "serialize_extra_nonce")]
    #[serde(deserialize_with = "deserialize_extra_nonce")]
    pub extra_nonce: [u8; EXTRA_NONCE_SIZE],
    pub bits: u32,            // Compact difficulty target
    pub miner: CompressedPublicKey,

    // Merkle roots
    pub hash_merkle_root: Hash,          // Transactions merkle root
    pub accepted_id_merkle_root: Hash,   // Accepted transactions merkle root
    pub utxo_commitment: Hash,           // UTXO set commitment (for future use)
}

impl BlockHeader {
    /// Create a new block header with all fields
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        version: BlockVersion,
        parents_by_level: Vec<Vec<Hash>>,
        blue_score: u64,
        daa_score: u64,
        blue_work: BlueWorkType,
        pruning_point: Hash,
        timestamp: TimestampMillis,
        bits: u32,
        extra_nonce: [u8; EXTRA_NONCE_SIZE],
        miner: CompressedPublicKey,
        hash_merkle_root: Hash,
        accepted_id_merkle_root: Hash,
        utxo_commitment: Hash,
    ) -> Self {
        Self {
            version,
            parents_by_level,
            blue_score,
            daa_score,
            blue_work,
            pruning_point,
            timestamp,
            nonce: 0,
            extra_nonce,
            bits,
            miner,
            hash_merkle_root,
            accepted_id_merkle_root,
            utxo_commitment,
        }
    }

    /// Create a simple block header for testing/genesis
    /// Uses default values for GHOSTDAG fields
    pub fn new_simple(
        version: BlockVersion,
        parents: Vec<Hash>,
        timestamp: TimestampMillis,
        extra_nonce: [u8; EXTRA_NONCE_SIZE],
        miner: CompressedPublicKey,
        hash_merkle_root: Hash,
    ) -> Self {
        Self {
            version,
            parents_by_level: if parents.is_empty() { vec![] } else { vec![parents] },
            blue_score: 0,
            daa_score: 0,
            blue_work: BlueWorkType::zero(),
            pruning_point: Hash::zero(),
            timestamp,
            nonce: 0,
            extra_nonce,
            bits: 0,
            miner,
            hash_merkle_root,
            accepted_id_merkle_root: Hash::zero(),
            utxo_commitment: Hash::zero(),
        }
    }

    /// Apply miner work to this block header
    pub fn apply_miner_work(&mut self, work: MinerWork) {
        let (_, timestamp, nonce, miner, extra_nonce) = work.take();
        self.miner = miner.unwrap().into_owned();
        self.timestamp = timestamp;
        self.nonce = nonce;
        self.extra_nonce = extra_nonce;
    }

    // Getters for common fields
    pub fn get_version(&self) -> BlockVersion {
        self.version
    }

    pub fn set_miner(&mut self, key: CompressedPublicKey) {
        self.miner = key;
    }

    pub fn set_extra_nonce(&mut self, values: [u8; EXTRA_NONCE_SIZE]) {
        self.extra_nonce = values;
    }

    /// Get blue score (replaces get_height for GHOSTDAG)
    pub fn get_blue_score(&self) -> u64 {
        self.blue_score
    }

    /// Get DAA score
    pub fn get_daa_score(&self) -> u64 {
        self.daa_score
    }

    /// Get blue work
    pub fn get_blue_work(&self) -> &BlueWorkType {
        &self.blue_work
    }

    pub fn get_timestamp(&self) -> TimestampMillis {
        self.timestamp
    }

    /// Get direct parents (level 0)
    /// This is equivalent to the old "tips"
    pub fn get_parents(&self) -> &[Hash] {
        if self.parents_by_level.is_empty() {
            &[]
        } else {
            &self.parents_by_level[0]
        }
    }

    /// Get all parents (flattened from all levels)
    pub fn get_all_parents(&self) -> Vec<Hash> {
        self.parents_by_level.iter().flatten().cloned().collect()
    }

    /// Get parents by level structure
    pub fn get_parents_by_level(&self) -> &Vec<Vec<Hash>> {
        &self.parents_by_level
    }

    /// Compute hash of all parents (for POW calculation)
    pub fn get_parents_hash(&self) -> Hash {
        let mut bytes = Vec::new();
        for level in &self.parents_by_level {
            for parent in level {
                bytes.extend(parent.as_bytes());
            }
        }
        hash(&bytes)
    }

    pub fn get_nonce(&self) -> u64 {
        self.nonce
    }

    pub fn get_miner(&self) -> &CompressedPublicKey {
        &self.miner
    }

    pub fn get_extra_nonce(&self) -> &[u8; EXTRA_NONCE_SIZE] {
        &self.extra_nonce
    }

    /// Get transaction merkle root
    pub fn get_hash_merkle_root(&self) -> &Hash {
        &self.hash_merkle_root
    }

    /// Get pruning point
    pub fn get_pruning_point(&self) -> &Hash {
        &self.pruning_point
    }

    /// Build the header work (immutable part in mining process)
    pub fn get_work(&self) -> Vec<u8> {
        let mut bytes: Vec<u8> = Vec::with_capacity(HEADER_WORK_SIZE);

        bytes.extend(self.version.to_bytes()); // 1
        bytes.extend(&self.blue_score.to_be_bytes()); // 1 + 8 = 9
        bytes.extend(self.get_parents_hash().as_bytes()); // 9 + 32 = 41
        bytes.extend(self.hash_merkle_root.as_bytes()); // 41 + 32 = 73

        debug_assert!(bytes.len() == HEADER_WORK_SIZE,
            "Error, invalid header work size, got {} but expected {}", bytes.len(), HEADER_WORK_SIZE);

        bytes
    }

    /// Compute the header work hash (immutable part in mining process)
    pub fn get_work_hash(&self) -> Hash {
        hash(&self.get_work())
    }

    /// Get serialized header for POW calculation
    fn get_serialized_header(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(BLOCK_WORK_SIZE);
        bytes.extend(self.get_work_hash().to_bytes());
        bytes.extend(self.timestamp.to_be_bytes());
        bytes.extend(self.nonce.to_be_bytes());
        bytes.extend(self.extra_nonce);
        bytes.extend(self.miner.as_bytes());

        debug_assert!(bytes.len() == BLOCK_WORK_SIZE,
            "invalid block work size, got {} but expected {}", bytes.len(), BLOCK_WORK_SIZE);

        bytes
    }

    /// Compute the block POW hash
    pub fn get_pow_hash(&self, algorithm: Algorithm) -> Result<Hash, TosHashError> {
        pow_hash(&self.get_serialized_header(), algorithm)
    }

    // REMOVED: All deprecated legacy methods
    // These methods have been removed to avoid confusion between:
    // - blue_score (GHOSTDAG consensus position)
    // - TopoHeight (sequential storage index)
    //
    // Migration guide:
    // - get_height() → Use get_blue_score() for GHOSTDAG, or storage.get_topoheight_for_block() for sequential index
    // - get_tips() → Use get_parents()
    // - get_txs_hashes() → No longer available, use get_hash_merkle_root()
    // - get_transactions() → No longer available, access via Block.get_transactions()
}

impl Serializer for BlockHeader {
    fn write(&self, writer: &mut Writer) {
        // Write version
        self.version.write(writer);

        // Write parents_by_level
        // SECURITY FIX: Validate parent levels count to prevent overflow
        // Without this check, truncation can cause consensus splits across nodes
        assert!(
            self.parents_by_level.len() <= MAX_PARENT_LEVELS,
            "Block header has too many parent levels: {} > {}. This would cause byte overflow in serialization.",
            self.parents_by_level.len(),
            MAX_PARENT_LEVELS
        );
        assert!(
            self.parents_by_level.len() <= 255,
            "Parent levels count {} exceeds u8 maximum (255)",
            self.parents_by_level.len()
        );

        writer.write_u8(self.parents_by_level.len() as u8);
        for level in &self.parents_by_level {
            // Also validate each level size doesn't overflow
            assert!(
                level.len() <= 255,
                "Parent level size {} exceeds u8 maximum (255)",
                level.len()
            );
            writer.write_u8(level.len() as u8);
            for parent in level {
                writer.write_hash(parent);
            }
        }

        // Write GHOSTDAG scores
        writer.write_u64(&self.blue_score);
        writer.write_u64(&self.daa_score);
        writer.write_blue_work(&self.blue_work);

        // Write pruning point
        writer.write_hash(&self.pruning_point);

        // Write mining fields
        writer.write_u64(&self.timestamp);
        writer.write_u64(&self.nonce);
        writer.write_bytes(&self.extra_nonce);
        writer.write_u32(&self.bits);
        self.miner.write(writer);

        // Write merkle roots
        writer.write_hash(&self.hash_merkle_root);
        writer.write_hash(&self.accepted_id_merkle_root);
        writer.write_hash(&self.utxo_commitment);
    }

    fn read(reader: &mut Reader) -> Result<BlockHeader, ReaderError> {
        let version = BlockVersion::read(reader)?;

        // Read parents_by_level
        let levels_count = reader.read_u8()?;

        // SECURITY FIX: Validate levels count to prevent consensus splits
        // Reject headers with too many parent levels
        if levels_count as usize > MAX_PARENT_LEVELS {
            debug!("Error, too many parent levels: {} > {}", levels_count, MAX_PARENT_LEVELS);
            return Err(ReaderError::InvalidValue);
        }

        let mut parents_by_level = Vec::with_capacity(levels_count as usize);
        for _ in 0..levels_count {
            let level_size = reader.read_u8()?;
            if level_size as usize > TIPS_LIMIT {
                debug!("Error, too many parents in level: {}", level_size);
                return Err(ReaderError::InvalidValue);
            }
            let mut level = Vec::with_capacity(level_size as usize);
            for _ in 0..level_size {
                level.push(reader.read_hash()?);
            }
            parents_by_level.push(level);
        }

        // Read GHOSTDAG scores
        let blue_score = reader.read_u64()?;
        let daa_score = reader.read_u64()?;
        let blue_work = reader.read_blue_work()?;

        // Read pruning point
        let pruning_point = reader.read_hash()?;

        // Read mining fields
        let timestamp = reader.read_u64()?;
        let nonce = reader.read_u64()?;
        let extra_nonce: [u8; 32] = reader.read_bytes_32()?;
        let bits = reader.read_u32()?;
        let miner = CompressedPublicKey::read(reader)?;

        // Read merkle roots
        let hash_merkle_root = reader.read_hash()?;
        let accepted_id_merkle_root = reader.read_hash()?;
        let utxo_commitment = reader.read_hash()?;

        Ok(BlockHeader {
            version,
            parents_by_level,
            blue_score,
            daa_score,
            blue_work,
            pruning_point,
            timestamp,
            nonce,
            extra_nonce,
            bits,
            miner,
            hash_merkle_root,
            accepted_id_merkle_root,
            utxo_commitment,
        })
    }

    fn size(&self) -> usize {
        let mut size = 0;

        // Version
        size += 1;

        // parents_by_level
        size += 1; // levels count
        for level in &self.parents_by_level {
            size += 1; // level size
            size += level.len() * HASH_SIZE;
        }

        // GHOSTDAG scores
        size += 8; // blue_score
        size += 8; // daa_score
        size += 32; // blue_work (U256)

        // Pruning point
        size += HASH_SIZE;

        // Mining fields
        size += 8; // timestamp
        size += 8; // nonce
        size += EXTRA_NONCE_SIZE;
        size += 4; // bits
        size += self.miner.size();

        // Merkle roots
        size += HASH_SIZE * 3;

        size
    }
}

impl Hashable for BlockHeader {
    fn hash(&self) -> Hash {
        hash(&self.get_serialized_header())
    }
}

impl Display for BlockHeader {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), FmtError> {
        let parents: Vec<String> = self.get_parents()
            .iter()
            .map(|h| format!("{}", h))
            .collect();

        write!(
            f,
            "BlockHeader[blue_score: {}, parents: [{}], timestamp: {}, nonce: {}, extra_nonce: {}, blue_work: {}]",
            self.blue_score,
            parents.join(", "),
            self.timestamp,
            self.nonce,
            hex::encode(self.extra_nonce),
            self.blue_work
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::KeyPair;

    #[test]
    fn test_block_header_simple() {
        let miner = KeyPair::new().get_public_key().compress();
        let parents = vec![Hash::zero()];

        let header = BlockHeader::new_simple(
            BlockVersion::V0,
            parents,
            0,
            [0u8; 32],
            miner,
            Hash::zero(),
        );

        let serialized = header.to_bytes();
        assert!(serialized.len() == header.size());

        let deserialized = BlockHeader::from_bytes(&serialized).unwrap();
        assert!(header.hash() == deserialized.hash());
    }

    #[test]
    fn test_block_header_serialization() {
        let miner = KeyPair::new().get_public_key().compress();
        let parents_by_level = vec![
            vec![Hash::zero()],
        ];

        let header = BlockHeader::new(
            BlockVersion::V0,
            parents_by_level,
            100,  // blue_score
            100,  // daa_score
            BlueWorkType::from(1000),
            Hash::zero(),
            1234567890,
            0x1d00ffff,
            [0u8; 32],
            miner,
            Hash::zero(),
            Hash::zero(),
            Hash::zero(),
        );

        let serialized = header.to_bytes();
        let deserialized = BlockHeader::from_bytes(&serialized).unwrap();

        assert_eq!(header.blue_score, deserialized.blue_score);
        assert_eq!(header.daa_score, deserialized.daa_score);
        assert_eq!(header.blue_work, deserialized.blue_work);
        assert_eq!(header.hash(), deserialized.hash());
    }

    #[test]
    fn test_parents_by_level() {
        use primitive_types::U256;
        let miner = KeyPair::new().get_public_key().compress();
        let parents_by_level = vec![
            vec![Hash::zero(), Hash::zero()],  // 2 direct parents
            vec![Hash::zero()],                 // 1 grandparent
        ];

        let header = BlockHeader::new(
            BlockVersion::V0,
            parents_by_level.clone(),
            0,
            0,
            U256::zero(),
            Hash::zero(),
            0,
            0,
            [0u8; 32],
            miner,
            Hash::zero(),
            Hash::zero(),
            Hash::zero(),
        );

        // Test get_parents (should return direct parents)
        assert_eq!(header.get_parents().len(), 2);

        // Test get_all_parents (should return all 3 parents)
        assert_eq!(header.get_all_parents().len(), 3);
    }
}
