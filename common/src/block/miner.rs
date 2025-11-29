use crate::{
    crypto::{elgamal::RISTRETTO_COMPRESSED_SIZE, hash, BlueWorkType, Hash, Hashable, PublicKey},
    serializer::{Reader, ReaderError, Serializer, Writer},
    time::TimestampMillis,
};
use log::debug;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use thiserror::Error;
use tos_hash::{v2, Error as TosHashError};

use super::{BlockHeader, EXTRA_NONCE_SIZE, MINER_WORK_SIZE};

/// Work variant for PoW calculation.
///
/// VERSION UNIFICATION: Only V2 algorithm is supported.
/// V1 has been removed as MINER_WORK_SIZE=252 bytes is incompatible with V1 (requires 200 bytes).
pub enum WorkVariant {
    /// Worker not initialized yet
    Uninitialized,
    /// V2 PoW algorithm (tos-hash v2) - the only supported algorithm
    V2(v2::ScratchPad),
}

impl WorkVariant {
    /// Check if the worker is initialized
    pub fn is_initialized(&self) -> bool {
        matches!(self, WorkVariant::V2(_))
    }
}

/// Mining work structure for PoW calculation.
///
/// **SECURITY FIX**: Extended to include ALL GHOSTDAG consensus fields.
/// This follows Kaspa's security model where ALL header fields are hash-protected
/// to prevent peer manipulation during block propagation.
///
/// ## Serialization Format (252 bytes = BLOCK_WORK_SIZE)
///
/// The serialization order MUST match `BlockHeader::get_serialized_header()` exactly:
/// 1. work_hash (32 bytes) - immutable: version, blue_score, parents, merkle_root
/// 2. timestamp (8 bytes) - miner can update
/// 3. nonce (8 bytes) - miner iterates
/// 4. extra_nonce (32 bytes) - miner can set
/// 5. miner (32 bytes) - miner public key
/// 6. daa_score (8 bytes) - GHOSTDAG, immutable from template
/// 7. blue_work (32 bytes) - GHOSTDAG, immutable from template
/// 8. bits (4 bytes) - difficulty target, immutable from template
/// 9. pruning_point (32 bytes) - GHOSTDAG, immutable from template
/// 10. accepted_id_merkle_root (32 bytes) - immutable from template
/// 11. utxo_commitment (32 bytes) - immutable from template
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MinerWork<'a> {
    // Original miner-controlled fields (112 bytes)
    header_work_hash: Hash, // 32 bytes: covers version, blue_score, parents, merkle_root
    timestamp: TimestampMillis, // 8 bytes: miners can update to keep it current
    nonce: u64,             // 8 bytes: miners iterate this for PoW
    extra_nonce: [u8; EXTRA_NONCE_SIZE], // 32 bytes: extra entropy for mining pools
    miner: Option<Cow<'a, PublicKey>>, // 32 bytes: miner's public key

    // GHOSTDAG consensus fields (140 bytes) - IMMUTABLE from template
    // These MUST be included in PoW to prevent header manipulation attacks
    daa_score: u64,                // 8 bytes: Difficulty Adjustment Algorithm score
    blue_work: BlueWorkType,       // 32 bytes (U256): cumulative blue work
    bits: u32,                     // 4 bytes: compact difficulty target
    pruning_point: Hash,           // 32 bytes: reference to pruning point
    accepted_id_merkle_root: Hash, // 32 bytes: for future use
    utxo_commitment: Hash,         // 32 bytes: for future use
}

// Worker is used to store the current work and its variant
// Based on the variant, the worker can compute the POW hash
// It is used by the miner to efficiently switch context in case of algorithm change
pub struct Worker<'a> {
    work: Option<(MinerWork<'a>, [u8; MINER_WORK_SIZE])>,
    variant: WorkVariant,
}

#[derive(Debug, Error)]
pub enum WorkerError {
    #[error("worker is not initialized")]
    Uninitialized,
    #[error("missing miner work")]
    MissingWork,
    #[error(transparent)]
    HashError(#[from] TosHashError),
}

impl<'a> Default for Worker<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Worker<'a> {
    // Create a new worker
    pub fn new() -> Self {
        Self {
            work: None,
            variant: WorkVariant::Uninitialized,
        }
    }

    // Take the current work
    pub fn take_work(&mut self) -> Option<MinerWork<'a>> {
        self.work.take().map(|(work, _)| work)
    }

    /// Set the current work for mining.
    ///
    /// VERSION UNIFICATION: Algorithm parameter removed, always uses V2.
    pub fn set_work(&mut self, work: MinerWork<'a>) {
        // Initialize V2 scratchpad if not already initialized
        if !self.variant.is_initialized() {
            self.variant = WorkVariant::V2(v2::ScratchPad::default());
        }

        let mut slice = [0u8; MINER_WORK_SIZE];
        slice.copy_from_slice(&work.to_bytes());

        self.work = Some((work, slice));
    }

    // Increase the nonce of the current work
    pub fn increase_nonce(&mut self) -> Result<(), WorkerError> {
        match self.work.as_mut() {
            Some((work, input)) => {
                work.increase_nonce();
                input[40..48].copy_from_slice(&work.nonce().to_be_bytes());
            }
            None => return Err(WorkerError::MissingWork),
        };

        Ok(())
    }

    // Set the timestamp of the current work
    pub fn set_timestamp(&mut self, timestamp: TimestampMillis) -> Result<(), WorkerError> {
        match self.work.as_mut() {
            Some((work, input)) => {
                work.set_timestamp(timestamp);
                input[32..40].copy_from_slice(&work.timestamp().to_be_bytes());
            }
            None => return Err(WorkerError::MissingWork),
        };

        Ok(())
    }

    /// Compute the POW hash based on the current work.
    ///
    /// VERSION UNIFICATION: Always uses V2 algorithm.
    pub fn get_pow_hash(&mut self) -> Result<Hash, WorkerError> {
        let work = match self.work.as_ref() {
            Some((_, input)) => input,
            None => return Err(WorkerError::MissingWork),
        };

        let hash = match &mut self.variant {
            WorkVariant::Uninitialized => return Err(WorkerError::Uninitialized),
            WorkVariant::V2(scratch_pad) => v2::tos_hash(work, scratch_pad).map(Hash::new)?,
        };

        Ok(hash)
    }

    // Compute the block hash based on the current work
    // This is used to get the expected block hash
    pub fn get_block_hash(&self) -> Result<Hash, WorkerError> {
        match self.work.as_ref() {
            Some((_, cache)) => Ok(hash(cache)),
            None => Err(WorkerError::MissingWork),
        }
    }
}

impl<'a> MinerWork<'a> {
    /// Create a new MinerWork with all GHOSTDAG consensus fields.
    ///
    /// All consensus fields are IMMUTABLE - they are set from the block template
    /// and must not be changed by the miner (except timestamp within limits).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        header_work_hash: Hash,
        timestamp: TimestampMillis,
        daa_score: u64,
        blue_work: BlueWorkType,
        bits: u32,
        pruning_point: Hash,
        accepted_id_merkle_root: Hash,
        utxo_commitment: Hash,
    ) -> Self {
        Self {
            header_work_hash,
            timestamp,
            nonce: 0,
            miner: None,
            extra_nonce: [0u8; EXTRA_NONCE_SIZE],
            daa_score,
            blue_work,
            bits,
            pruning_point,
            accepted_id_merkle_root,
            utxo_commitment,
        }
    }

    pub fn get_timestamp(&self) -> TimestampMillis {
        self.timestamp
    }

    /// Create MinerWork from a BlockHeader.
    ///
    /// Extracts all fields from the header including GHOSTDAG consensus fields.
    pub fn from_block(header: BlockHeader) -> Self {
        Self {
            header_work_hash: header.get_work_hash(),
            timestamp: header.get_timestamp(),
            nonce: 0,
            miner: Some(Cow::Owned(header.miner)),
            extra_nonce: header.extra_nonce,
            // GHOSTDAG consensus fields
            daa_score: header.daa_score,
            blue_work: header.blue_work,
            bits: header.bits,
            pruning_point: header.pruning_point,
            accepted_id_merkle_root: header.accepted_id_merkle_root,
            utxo_commitment: header.utxo_commitment,
        }
    }

    #[inline(always)]
    pub fn nonce(&self) -> u64 {
        self.nonce
    }

    #[inline(always)]
    pub fn timestamp(&self) -> TimestampMillis {
        self.timestamp
    }

    #[inline(always)]
    pub fn get_header_work_hash(&self) -> &Hash {
        &self.header_work_hash
    }

    #[inline(always)]
    pub fn get_miner(&self) -> Option<&PublicKey> {
        self.miner.as_ref().map(|m| m.as_ref())
    }

    pub fn get_extra_nonce(&mut self) -> &mut [u8; EXTRA_NONCE_SIZE] {
        &mut self.extra_nonce
    }

    #[inline(always)]
    pub fn set_timestamp(&mut self, timestamp: TimestampMillis) {
        self.timestamp = timestamp;
    }

    #[inline(always)]
    pub fn increase_nonce(&mut self) {
        self.nonce += 1;
    }

    #[inline(always)]
    pub fn set_miner(&mut self, miner: Cow<'a, PublicKey>) {
        self.miner = Some(miner);
    }

    #[inline(always)]
    pub fn set_thread_id(&mut self, id: u8) {
        self.extra_nonce[EXTRA_NONCE_SIZE - 1] = id;
    }

    #[inline(always)]
    pub fn set_thread_id_u16(&mut self, id: u16) {
        self.extra_nonce[EXTRA_NONCE_SIZE - 2..].copy_from_slice(id.to_be_bytes().as_ref());
    }

    #[inline(always)]
    pub fn take(
        self,
    ) -> (
        Hash,
        TimestampMillis,
        u64,
        Option<Cow<'a, PublicKey>>,
        [u8; EXTRA_NONCE_SIZE],
    ) {
        (
            self.header_work_hash,
            self.timestamp,
            self.nonce,
            self.miner,
            self.extra_nonce,
        )
    }
}

impl<'a> Serializer for MinerWork<'a> {
    /// Serialize MinerWork for PoW calculation.
    ///
    /// **CRITICAL**: The serialization order MUST match `BlockHeader::get_serialized_header()`
    /// exactly, otherwise PoW validation will fail.
    fn write(&self, writer: &mut Writer) {
        // Original miner-controlled fields (112 bytes)
        writer.write_hash(&self.header_work_hash); // 32 bytes
        writer.write_u64(&self.timestamp); // 8 bytes (big-endian)
        writer.write_u64(&self.nonce); // 8 bytes (big-endian)
        writer.write_bytes(&self.extra_nonce); // 32 bytes

        // Miner public key (32 bytes)
        if let Some(miner) = &self.miner {
            miner.write(writer);
        } else {
            writer.write_bytes(&[0u8; RISTRETTO_COMPRESSED_SIZE]);
        }

        // GHOSTDAG consensus fields (140 bytes) - must match BlockHeader::get_serialized_header()
        // These use LITTLE-endian for consistency with BlockHeader
        writer.write_bytes(&self.daa_score.to_le_bytes()); // 8 bytes (little-endian)
                                                           // CRITICAL: Use to_little_endian() to match BlockHeader::get_serialized_header()
                                                           // write_blue_work() uses big-endian which would cause PoW hash mismatch!
        writer.write_bytes(&self.blue_work.to_little_endian()); // 32 bytes (little-endian U256)
        writer.write_bytes(&self.bits.to_le_bytes()); // 4 bytes (little-endian)
        writer.write_hash(&self.pruning_point); // 32 bytes
        writer.write_hash(&self.accepted_id_merkle_root); // 32 bytes
        writer.write_hash(&self.utxo_commitment); // 32 bytes

        debug_assert!(
            writer.total_write() == MINER_WORK_SIZE,
            "invalid miner work size, expected {}, got {}",
            MINER_WORK_SIZE,
            writer.total_write()
        );
    }

    fn read(reader: &mut Reader) -> Result<MinerWork<'a>, ReaderError> {
        if reader.total_size() != MINER_WORK_SIZE {
            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "invalid miner work size, expected {}, got {}",
                    MINER_WORK_SIZE,
                    reader.total_size()
                );
            }
            return Err(ReaderError::InvalidSize);
        }

        // Original miner-controlled fields
        let header_work_hash = reader.read_hash()?;
        let timestamp = reader.read_u64()?;
        let nonce = reader.read_u64()?;
        let extra_nonce = reader.read_bytes_32()?;
        let miner = Some(Cow::Owned(PublicKey::read(reader)?));

        // GHOSTDAG consensus fields (little-endian to match BlockHeader)
        let daa_score = {
            let bytes = reader.read_bytes_ref(8)?;
            u64::from_le_bytes(bytes.try_into().map_err(|_| ReaderError::InvalidSize)?)
        };
        // CRITICAL: Use from_little_endian() to match BlockHeader::get_serialized_header()
        // read_blue_work() uses big-endian which would cause PoW hash mismatch!
        let blue_work = {
            let bytes = reader.read_bytes_ref(32)?;
            BlueWorkType::from_little_endian(bytes)
        };
        let bits = {
            let bytes = reader.read_bytes_ref(4)?;
            u32::from_le_bytes(bytes.try_into().map_err(|_| ReaderError::InvalidSize)?)
        };
        let pruning_point = reader.read_hash()?;
        let accepted_id_merkle_root = reader.read_hash()?;
        let utxo_commitment = reader.read_hash()?;

        Ok(MinerWork {
            header_work_hash,
            timestamp,
            nonce,
            extra_nonce,
            miner,
            daa_score,
            blue_work,
            bits,
            pruning_point,
            accepted_id_merkle_root,
            utxo_commitment,
        })
    }

    fn size(&self) -> usize {
        MINER_WORK_SIZE
    }
}

// no need to override hash() as its already serialized in good format
// This is used to get the expected block hash
impl Hashable for MinerWork<'_> {}

#[cfg(test)]
mod tests {
    use crate::crypto::KeyPair;
    use primitive_types::U256;

    use super::*;

    #[test]
    fn test_worker() {
        let header_work_hash = Hash::new([255u8; 32]);
        let timestamp = 1234567890;
        let nonce = 0;
        let miner = KeyPair::new().get_public_key().compress();
        let extra_nonce = [0u8; EXTRA_NONCE_SIZE];

        // Create MinerWork with all GHOSTDAG consensus fields
        let work = MinerWork {
            header_work_hash,
            timestamp,
            nonce,
            miner: Some(Cow::Owned(miner)),
            extra_nonce,
            // GHOSTDAG consensus fields
            daa_score: 100,
            blue_work: U256::from(1000),
            bits: 0x1d00ffff,
            pruning_point: Hash::zero(),
            accepted_id_merkle_root: Hash::zero(),
            utxo_commitment: Hash::zero(),
        };
        let work_hex = work.to_hex();

        // Use v2 algorithm which supports 252-byte input (v1 is limited to 200 bytes)
        let work_bytes = work.to_bytes();
        let expected_hash = v2::tos_hash(&work_bytes, &mut v2::ScratchPad::default())
            .map(Hash::new)
            .unwrap();
        let block_hash = work.hash();

        let mut worker = Worker::new();
        worker.set_work(work.clone());

        let worker_hash = worker.get_pow_hash().unwrap();
        let next_worker_hash = worker.get_pow_hash().unwrap();
        let worker_block_hash = work.hash();

        assert_eq!(expected_hash, worker_hash);
        assert_eq!(block_hash, worker_block_hash);

        // Lets do another hash
        assert_eq!(expected_hash, next_worker_hash);

        assert_eq!(work_hex, worker.take_work().unwrap().to_hex());
    }

    #[test]
    fn test_miner_work_size() {
        // Verify MINER_WORK_SIZE matches BLOCK_WORK_SIZE (252 bytes)
        assert_eq!(MINER_WORK_SIZE, 252, "MINER_WORK_SIZE must be 252 bytes");

        let miner = KeyPair::new().get_public_key().compress();
        let work = MinerWork::new(
            Hash::zero(),
            0,
            100,              // daa_score
            U256::from(1000), // blue_work
            0x1d00ffff,       // bits
            Hash::zero(),     // pruning_point
            Hash::zero(),     // accepted_id_merkle_root
            Hash::zero(),     // utxo_commitment
        );

        // Verify serialization size
        let mut work_with_miner = work.clone();
        work_with_miner.set_miner(Cow::Owned(miner));
        let serialized = work_with_miner.to_bytes();
        assert_eq!(
            serialized.len(),
            MINER_WORK_SIZE,
            "Serialized MinerWork must be {} bytes",
            MINER_WORK_SIZE
        );
    }

    /// Test that MinerWork serialization matches BlockHeader.get_serialized_header()
    /// This is CRITICAL - if these don't match, PoW validation will fail!
    #[test]
    fn test_miner_work_matches_block_header_serialization() {
        use crate::block::{BlockHeader, BlockVersion};

        let keypair = KeyPair::new();
        let miner_key = keypair.get_public_key().compress();
        let timestamp = 1234567890u64;
        let nonce = 42u64;
        let extra_nonce = [7u8; EXTRA_NONCE_SIZE];
        let daa_score = 100u64;
        let blue_work = U256::from(1000);
        let bits = 0x1d00ffff;
        let pruning_point = Hash::new([1u8; 32]);
        let accepted_id_merkle_root = Hash::new([2u8; 32]);
        let utxo_commitment = Hash::new([3u8; 32]);
        let hash_merkle_root = Hash::new([4u8; 32]);
        let parents = vec![Hash::new([5u8; 32])];
        let blue_score = 50u64;

        // Create a BlockHeader with all fields
        let mut header = BlockHeader::new(
            BlockVersion::Baseline,
            vec![parents.clone()],
            blue_score,
            daa_score,
            blue_work,
            pruning_point.clone(),
            timestamp,
            bits,
            extra_nonce,
            miner_key.clone(),
            hash_merkle_root.clone(),
            accepted_id_merkle_root.clone(),
            utxo_commitment.clone(),
        );
        header.nonce = nonce;

        // Get the work_hash from the header
        let work_hash = header.get_work_hash();

        // Create a MinerWork with the same fields
        let miner_work = MinerWork {
            header_work_hash: work_hash.clone(),
            timestamp,
            nonce,
            extra_nonce,
            miner: Some(Cow::Owned(miner_key.clone())),
            daa_score,
            blue_work,
            bits,
            pruning_point: pruning_point.clone(),
            accepted_id_merkle_root: accepted_id_merkle_root.clone(),
            utxo_commitment: utxo_commitment.clone(),
        };

        // Get serializations
        let header_serialized = header.get_serialized_header();
        let miner_serialized = miner_work.to_bytes();

        // Print for debugging
        println!("Header serialization ({} bytes):", header_serialized.len());
        println!(
            "MinerWork serialization ({} bytes):",
            miner_serialized.len()
        );

        // Compare each field
        let mut offset = 0;

        // work_hash (32 bytes)
        println!("work_hash:");
        println!("  Header:    {:?}", &header_serialized[offset..offset + 32]);
        println!("  MinerWork: {:?}", &miner_serialized[offset..offset + 32]);
        assert_eq!(
            &header_serialized[offset..offset + 32],
            &miner_serialized[offset..offset + 32],
            "work_hash mismatch"
        );
        offset += 32;

        // timestamp (8 bytes)
        println!("timestamp:");
        println!("  Header:    {:?}", &header_serialized[offset..offset + 8]);
        println!("  MinerWork: {:?}", &miner_serialized[offset..offset + 8]);
        assert_eq!(
            &header_serialized[offset..offset + 8],
            &miner_serialized[offset..offset + 8],
            "timestamp mismatch"
        );
        offset += 8;

        // nonce (8 bytes)
        println!("nonce:");
        println!("  Header:    {:?}", &header_serialized[offset..offset + 8]);
        println!("  MinerWork: {:?}", &miner_serialized[offset..offset + 8]);
        assert_eq!(
            &header_serialized[offset..offset + 8],
            &miner_serialized[offset..offset + 8],
            "nonce mismatch"
        );
        offset += 8;

        // extra_nonce (32 bytes)
        println!("extra_nonce:");
        println!("  Header:    {:?}", &header_serialized[offset..offset + 32]);
        println!("  MinerWork: {:?}", &miner_serialized[offset..offset + 32]);
        assert_eq!(
            &header_serialized[offset..offset + 32],
            &miner_serialized[offset..offset + 32],
            "extra_nonce mismatch"
        );
        offset += 32;

        // miner (32 bytes)
        println!("miner:");
        println!("  Header:    {:?}", &header_serialized[offset..offset + 32]);
        println!("  MinerWork: {:?}", &miner_serialized[offset..offset + 32]);
        assert_eq!(
            &header_serialized[offset..offset + 32],
            &miner_serialized[offset..offset + 32],
            "miner mismatch"
        );
        offset += 32;

        // daa_score (8 bytes)
        println!("daa_score:");
        println!("  Header:    {:?}", &header_serialized[offset..offset + 8]);
        println!("  MinerWork: {:?}", &miner_serialized[offset..offset + 8]);
        assert_eq!(
            &header_serialized[offset..offset + 8],
            &miner_serialized[offset..offset + 8],
            "daa_score mismatch"
        );
        offset += 8;

        // blue_work (32 bytes)
        println!("blue_work:");
        println!("  Header:    {:?}", &header_serialized[offset..offset + 32]);
        println!("  MinerWork: {:?}", &miner_serialized[offset..offset + 32]);
        assert_eq!(
            &header_serialized[offset..offset + 32],
            &miner_serialized[offset..offset + 32],
            "blue_work mismatch"
        );
        offset += 32;

        // bits (4 bytes)
        println!("bits:");
        println!("  Header:    {:?}", &header_serialized[offset..offset + 4]);
        println!("  MinerWork: {:?}", &miner_serialized[offset..offset + 4]);
        assert_eq!(
            &header_serialized[offset..offset + 4],
            &miner_serialized[offset..offset + 4],
            "bits mismatch"
        );
        offset += 4;

        // pruning_point (32 bytes)
        println!("pruning_point:");
        println!("  Header:    {:?}", &header_serialized[offset..offset + 32]);
        println!("  MinerWork: {:?}", &miner_serialized[offset..offset + 32]);
        assert_eq!(
            &header_serialized[offset..offset + 32],
            &miner_serialized[offset..offset + 32],
            "pruning_point mismatch"
        );
        offset += 32;

        // accepted_id_merkle_root (32 bytes)
        println!("accepted_id_merkle_root:");
        println!("  Header:    {:?}", &header_serialized[offset..offset + 32]);
        println!("  MinerWork: {:?}", &miner_serialized[offset..offset + 32]);
        assert_eq!(
            &header_serialized[offset..offset + 32],
            &miner_serialized[offset..offset + 32],
            "accepted_id_merkle_root mismatch"
        );
        offset += 32;

        // utxo_commitment (32 bytes)
        println!("utxo_commitment:");
        println!("  Header:    {:?}", &header_serialized[offset..offset + 32]);
        println!("  MinerWork: {:?}", &miner_serialized[offset..offset + 32]);
        assert_eq!(
            &header_serialized[offset..offset + 32],
            &miner_serialized[offset..offset + 32],
            "utxo_commitment mismatch"
        );
        offset += 32;

        assert_eq!(offset, 252, "Total size should be 252");

        // Final check - entire serialization should match
        assert_eq!(
            header_serialized, miner_serialized,
            "Full serializations must match"
        );

        // Also verify the hash matches
        let header_hash = header.hash();
        let miner_hash = miner_work.hash();
        assert_eq!(header_hash, miner_hash, "Block hashes must match");
    }
}
