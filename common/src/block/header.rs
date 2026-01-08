use super::{Algorithm, MinerWork, EXTRA_NONCE_SIZE};
use crate::block::BlockVrfData;
use crate::{
    block::{BlockVersion, BLOCK_WORK_SIZE, HEADER_WORK_SIZE},
    config::{MAX_TXS_PER_BLOCK, TIPS_LIMIT},
    crypto::{
        elgamal::CompressedPublicKey, hash, pow_hash, Hash, Hashable, HASH_SIZE, SIGNATURE_SIZE,
    },
    immutable::Immutable,
    serializer::{Reader, ReaderError, Serializer, Writer},
    time::TimestampMillis,
};
use indexmap::IndexSet;
use log::debug;
use serde::Deserialize;
use std::{
    fmt::Error,
    fmt::{Display, Formatter},
};
use tos_crypto::vrf::{VRF_OUTPUT_SIZE, VRF_PROOF_SIZE, VRF_PUBLIC_KEY_SIZE};
use tos_hash::Error as TosHashError;

// Serialize the extra nonce in a hexadecimal string
pub fn serialize_extra_nonce<S: serde::Serializer>(
    extra_nonce: &[u8; EXTRA_NONCE_SIZE],
    s: S,
) -> Result<S::Ok, S::Error> {
    s.serialize_str(&hex::encode(extra_nonce))
}

// Deserialize the extra nonce from a hexadecimal string
// Add length validation before copy_from_slice to prevent panic
pub fn deserialize_extra_nonce<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<[u8; EXTRA_NONCE_SIZE], D::Error> {
    let mut extra_nonce = [0u8; EXTRA_NONCE_SIZE];
    let hex = String::deserialize(deserializer)?;
    // SECURITY FIX: Limit input length to prevent memory exhaustion DoS
    const MAX_HEX_LENGTH: usize = EXTRA_NONCE_SIZE * 2;
    if hex.len() > MAX_HEX_LENGTH {
        return Err(serde::de::Error::custom(format!(
            "Invalid extraNonce: hex string length {} exceeds maximum {}",
            hex.len(),
            MAX_HEX_LENGTH
        )));
    }
    let decoded = hex::decode(hex).map_err(serde::de::Error::custom)?;
    // SECURITY FIX: Validate length before copy_from_slice to prevent panic
    if decoded.len() != EXTRA_NONCE_SIZE {
        return Err(serde::de::Error::custom(format!(
            "Invalid extraNonce: expected {} bytes, got {}",
            EXTRA_NONCE_SIZE,
            decoded.len()
        )));
    }
    extra_nonce.copy_from_slice(&decoded);
    Ok(extra_nonce)
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct BlockHeader {
    // Version of the block
    pub version: BlockVersion,
    // All TIPS of the block (previous hashes of the block)
    pub tips: Immutable<IndexSet<Hash>>,
    // Timestamp in milliseconds
    pub timestamp: TimestampMillis,
    // Height of the block
    pub height: u64,
    // Nonce of the block
    // This is the mutable part in mining process
    pub nonce: u64,
    // Extra nonce of the block
    // This is the mutable part in mining process
    // This is to spread even more the work in the network
    #[serde(serialize_with = "serialize_extra_nonce")]
    #[serde(deserialize_with = "deserialize_extra_nonce")]
    pub extra_nonce: [u8; EXTRA_NONCE_SIZE],
    // Miner public key
    pub miner: CompressedPublicKey,
    // All transactions hashes of the block
    pub txs_hashes: IndexSet<Hash>,
    /// Optional VRF data (public key, output, proof) for verifiable randomness
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vrf: Option<BlockVrfData>,
}

impl BlockHeader {
    pub fn new(
        version: BlockVersion,
        height: u64,
        timestamp: TimestampMillis,
        tips: impl Into<Immutable<IndexSet<Hash>>>,
        extra_nonce: [u8; EXTRA_NONCE_SIZE],
        miner: CompressedPublicKey,
        txs_hashes: IndexSet<Hash>,
    ) -> Self {
        BlockHeader {
            version,
            height,
            timestamp,
            tips: tips.into(),
            nonce: 0,
            extra_nonce,
            miner,
            txs_hashes,
            vrf: None,
        }
    }

    // Apply a MinerWork to this block header to match the POW hash
    // Returns error if MinerWork does not contain a miner public key
    pub fn apply_miner_work(&mut self, work: MinerWork) -> Result<(), &'static str> {
        let (_, timestamp, nonce, miner, extra_nonce) = work.take();
        self.miner = miner
            .ok_or("MinerWork missing miner public key")?
            .into_owned();
        self.timestamp = timestamp;
        self.nonce = nonce;
        self.extra_nonce = extra_nonce;
        Ok(())
    }

    pub fn get_version(&self) -> BlockVersion {
        self.version
    }

    pub fn set_miner(&mut self, key: CompressedPublicKey) {
        self.miner = key;
    }

    pub fn set_extra_nonce(&mut self, values: [u8; EXTRA_NONCE_SIZE]) {
        self.extra_nonce = values;
    }

    pub fn get_height(&self) -> u64 {
        self.height
    }

    pub fn get_timestamp(&self) -> TimestampMillis {
        self.timestamp
    }

    pub fn get_tips(&self) -> &IndexSet<Hash> {
        &self.tips
    }

    pub fn get_immutable_tips(&self) -> &Immutable<IndexSet<Hash>> {
        &self.tips
    }

    // Compute a hash covering all tips hashes
    pub fn get_tips_hash(&self) -> Hash {
        let mut bytes = Vec::with_capacity(self.tips.len() * HASH_SIZE);

        for tx in self.tips.iter() {
            bytes.extend(tx.as_bytes())
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

    pub fn get_txs_hashes(&self) -> &IndexSet<Hash> {
        &self.txs_hashes
    }

    pub fn get_vrf_data(&self) -> Option<&BlockVrfData> {
        self.vrf.as_ref()
    }

    pub fn set_vrf_data(&mut self, vrf: Option<BlockVrfData>) {
        self.vrf = vrf;
    }

    pub fn take_txs_hashes(self) -> IndexSet<Hash> {
        self.txs_hashes
    }

    // Compute a hash covering all TXs hashes
    pub fn get_txs_hash(&self) -> Hash {
        let mut bytes = Vec::with_capacity(self.txs_hashes.len() * HASH_SIZE);
        for tx in &self.txs_hashes {
            bytes.extend(tx.as_bytes())
        }

        hash(&bytes)
    }

    pub fn get_txs_count(&self) -> usize {
        self.txs_hashes.len()
    }

    // Build the header work (immutable part in mining process)
    // This is the part that will be used to compute the header work hash
    // See get_work_hash function and get_serialized_header for final hash computation
    pub fn get_work(&self) -> Vec<u8> {
        let mut bytes: Vec<u8> = Vec::with_capacity(HEADER_WORK_SIZE);

        bytes.extend(self.version.to_bytes()); // 1
        bytes.extend(&self.height.to_be_bytes()); // 1 + 8 = 9
        bytes.extend(self.get_tips_hash().as_bytes()); // 9 + 32 = 41
        bytes.extend(self.get_txs_hash().as_bytes()); // 41 + 32 = 73

        debug_assert!(
            bytes.len() == HEADER_WORK_SIZE,
            "Error, invalid header work size, got {} but expected {}",
            bytes.len(),
            HEADER_WORK_SIZE
        );

        bytes
    }

    // compute the header work hash (immutable part in mining process)
    pub fn get_work_hash(&self) -> Hash {
        hash(&self.get_work())
    }

    // This is similar to MinerWork
    fn get_serialized_header(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(BLOCK_WORK_SIZE);
        bytes.extend(self.get_work_hash().to_bytes());
        bytes.extend(self.timestamp.to_be_bytes());
        bytes.extend(self.nonce.to_be_bytes());
        bytes.extend(self.extra_nonce);
        bytes.extend(self.miner.as_bytes());

        debug_assert!(
            bytes.len() == BLOCK_WORK_SIZE,
            "invalid block work size, got {} but expected {}",
            bytes.len(),
            BLOCK_WORK_SIZE
        );

        bytes
    }

    // compute the block POW hash
    pub fn get_pow_hash(&self, algorithm: Algorithm) -> Result<Hash, TosHashError> {
        pow_hash(&self.get_serialized_header(), algorithm)
    }

    pub fn get_transactions(&self) -> &IndexSet<Hash> {
        &self.txs_hashes
    }
}

impl Serializer for BlockHeader {
    fn write(&self, writer: &mut Writer) {
        self.version.write(writer); // 1
        writer.write_u64(&self.height); // 1 + 8 = 9
        writer.write_u64(&self.timestamp); // 9 + 8 = 17
        writer.write_u64(&self.nonce); // 17 + 8 = 25
        writer.write_bytes(&self.extra_nonce); // 25 + 32 = 57
        writer.write_u8(self.tips.len() as u8); // 57 + 1 = 58
        for tip in self.tips.iter() {
            writer.write_hash(tip); // 32 per hash
        }

        writer.write_u16(self.txs_hashes.len() as u16); // 58 + (N*32) + 2 = 60 + (N*32)
        for tx in &self.txs_hashes {
            writer.write_hash(tx); // 32
        }
        self.miner.write(writer); // 60 + (N*32) + (T*32) + 32 = 92 + (N*32) + (T*32)

        // VRF field: flag byte (0 = no VRF, 1 = VRF present) + optional VRF data
        writer.write_u8(if self.vrf.is_some() { 1 } else { 0 });
        if let Some(vrf) = &self.vrf {
            writer.write_bytes(&vrf.public_key);
            writer.write_bytes(&vrf.output);
            writer.write_bytes(&vrf.proof);
            writer.write_bytes(&vrf.binding_signature);
        }
        // Minimum size is 93 bytes (includes VRF flag)
    }

    fn read(reader: &mut Reader) -> Result<BlockHeader, ReaderError> {
        let version = BlockVersion::read(reader)?;
        let height = reader.read_u64()?;
        let timestamp = reader.read_u64()?;
        let nonce = reader.read_u64()?;
        let extra_nonce: [u8; 32] = reader.read_bytes_32()?;

        let tips_count = reader.read_u8()?;
        if tips_count as usize > TIPS_LIMIT {
            debug!("Error, too many tips in block header");
            return Err(ReaderError::InvalidValue);
        }

        let mut tips = IndexSet::with_capacity(tips_count as usize);
        for _ in 0..tips_count {
            if !tips.insert(reader.read_hash()?) {
                debug!("Error, duplicate tip found in block header");
                return Err(ReaderError::InvalidValue);
            }
        }

        let txs_count = reader.read_u16()?;
        // Validate txs_count before allocation to prevent memory exhaustion DoS
        // Uses centralized constant from config.rs (derived from MAX_BLOCK_SIZE)
        if txs_count > MAX_TXS_PER_BLOCK {
            debug!(
                "Error, too many transactions in block header: {} > {}",
                txs_count, MAX_TXS_PER_BLOCK
            );
            return Err(ReaderError::InvalidValue);
        }
        let mut txs_hashes = IndexSet::with_capacity(txs_count as usize);
        for _ in 0..txs_count {
            if !txs_hashes.insert(reader.read_hash()?) {
                debug!("Error, duplicate tx hash found in block header");
                return Err(ReaderError::InvalidValue);
            }
        }

        let miner = CompressedPublicKey::read(reader)?;

        // VRF field: flag byte followed by optional VRF data
        let vrf_flag = reader.read_u8()?;
        let vrf = match vrf_flag {
            0 => None,
            1 => {
                // Static assertions to ensure VRF sizes match reader functions
                // If tos_crypto changes sizes, this will fail at compile time
                const _: () = assert!(VRF_PUBLIC_KEY_SIZE == 32);
                const _: () = assert!(VRF_OUTPUT_SIZE == 32);
                const _: () = assert!(VRF_PROOF_SIZE == 64);

                let public_key = reader.read_bytes_32()?;
                let output = reader.read_bytes_32()?;
                let proof = reader.read_bytes_64()?;
                let binding_signature = reader.read_bytes_64()?;
                Some(BlockVrfData::new(
                    public_key,
                    output,
                    proof,
                    binding_signature,
                ))
            }
            _ => {
                debug!("Error, invalid VRF flag in block header: {}", vrf_flag);
                return Err(ReaderError::InvalidValue);
            }
        };
        Ok(BlockHeader {
            version,
            extra_nonce,
            height,
            timestamp,
            tips: Immutable::Owned(tips),
            miner,
            nonce,
            txs_hashes,
            vrf,
        })
    }

    fn size(&self) -> usize {
        // additional byte for tips count
        let tips_size = 1 + self.tips.len() * HASH_SIZE;
        // 2 bytes for txs count (u16)
        let txs_size = 2 + self.txs_hashes.len() * HASH_SIZE;
        // Version is u8
        let version_size = 1;

        let vrf_size = if self.vrf.is_some() {
            // 1 byte flag + public_key + output + proof + binding_signature
            1 + VRF_PUBLIC_KEY_SIZE + VRF_OUTPUT_SIZE + VRF_PROOF_SIZE + SIGNATURE_SIZE
        } else {
            1 // flag only (0 = no VRF)
        };
        EXTRA_NONCE_SIZE
            + tips_size
            + txs_size
            + version_size
            + self.miner.size()
            + self.timestamp.size()
            + self.height.size()
            + self.nonce.size()
            + vrf_size
    }
}

impl Hashable for BlockHeader {
    // this function has the same behavior as the get_pow_hash function
    // but we use a fast algorithm here
    fn hash(&self) -> Hash {
        hash(&self.get_serialized_header())
    }
}

impl Display for BlockHeader {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        let mut tips = Vec::with_capacity(self.tips.len());
        for hash in self.tips.iter() {
            tips.push(format!("{}", hash));
        }
        write!(f, "BlockHeader[height: {}, tips: [{}], timestamp: {}, nonce: {}, extra_nonce: {}, txs: {}]", self.height, tips.join(", "), self.timestamp, self.nonce, hex::encode(self.extra_nonce), self.txs_hashes.len())
    }
}

#[cfg(test)]
mod tests {
    use super::BlockHeader;
    use crate::{
        block::{BlockVersion, BlockVrfData},
        crypto::{Hash, Hashable, KeyPair},
        serializer::Serializer,
    };
    use indexmap::IndexSet;

    #[test]
    fn test_block_template() {
        let mut tips = IndexSet::new();
        tips.insert(Hash::zero());

        let miner = KeyPair::new().get_public_key().compress();
        let header = BlockHeader::new(
            BlockVersion::Nobunaga,
            0,
            0,
            tips,
            [0u8; 32],
            miner,
            IndexSet::new(),
        );

        let serialized = header.to_bytes();
        assert!(serialized.len() == header.size());

        let deserialized = BlockHeader::from_bytes(&serialized).unwrap();
        assert!(header.hash() == deserialized.hash());
    }

    #[test]
    fn test_block_template_from_hex() {
        let serialized = "00000000000000002d0000018f1cbd697000000000000000000eded85557e887b45989a727b6786e1bd250de65042d9381822fa73d01d2c4ff01d3a0154853dbb01dc28c9102e9d94bea355b8ee0d82c3e078ac80841445e86520000d67ad13934337b85c34985491c437386c95de0d97017131088724cfbedebdc5500";
        let header = BlockHeader::from_hex(serialized).unwrap();
        assert!(header.to_hex() == serialized);
    }

    #[test]
    fn test_block_header_vrf_roundtrip() {
        let mut tips = IndexSet::new();
        tips.insert(Hash::zero());

        let miner = KeyPair::new().get_public_key().compress();
        let mut header = BlockHeader::new(
            BlockVersion::Nobunaga,
            1,
            1,
            tips,
            [0u8; 32],
            miner,
            IndexSet::new(),
        );

        let vrf = BlockVrfData::new([1u8; 32], [2u8; 32], [3u8; 64], [4u8; 64]);
        header.set_vrf_data(Some(vrf.clone()));

        let serialized = header.to_bytes();
        let parsed = BlockHeader::from_bytes(&serialized).unwrap();
        let parsed_vrf = parsed.get_vrf_data().unwrap();

        assert_eq!(parsed_vrf.public_key, vrf.public_key);
        assert_eq!(parsed_vrf.output, vrf.output);
        assert_eq!(parsed_vrf.proof, vrf.proof);
        assert_eq!(parsed_vrf.binding_signature, vrf.binding_signature);
    }

    /// VRF-003: Test that block hash is unchanged when VRF data is set
    ///
    /// This test verifies that the block hash (used for VRF signing) excludes
    /// VRF fields, preventing circular dependency in VRF computation.
    #[test]
    fn test_block_hash_excludes_vrf_fields() {
        let mut tips = IndexSet::new();
        tips.insert(Hash::zero());

        let miner = KeyPair::new().get_public_key().compress();
        let header_without_vrf = BlockHeader::new(
            BlockVersion::Nobunaga,
            1,
            1,
            tips.clone(),
            [0u8; 32],
            miner.clone(),
            IndexSet::new(),
        );

        let mut header_with_vrf = BlockHeader::new(
            BlockVersion::Nobunaga,
            1,
            1,
            tips,
            [0u8; 32],
            miner,
            IndexSet::new(),
        );

        // Set VRF data on one header
        let vrf = BlockVrfData::new([1u8; 32], [2u8; 32], [3u8; 64], [4u8; 64]);
        header_with_vrf.set_vrf_data(Some(vrf));

        // The block hash must be identical regardless of VRF data
        // This is critical for VRF signing: block_hash is input to VRF
        assert_eq!(
            header_without_vrf.hash(),
            header_with_vrf.hash(),
            "Block hash must be unchanged when VRF data is set (VRF-003)"
        );
    }

    /// Test that BlockHeader::size() matches actual serialized size with VRF
    #[test]
    fn test_block_header_size_with_vrf() {
        let mut tips = IndexSet::new();
        tips.insert(Hash::zero());

        let miner = KeyPair::new().get_public_key().compress();
        let mut header = BlockHeader::new(
            BlockVersion::Nobunaga,
            1,
            1,
            tips,
            [0u8; 32],
            miner,
            IndexSet::new(),
        );

        // Test without VRF
        let serialized_no_vrf = header.to_bytes();
        assert_eq!(
            serialized_no_vrf.len(),
            header.size(),
            "size() must match serialized length without VRF"
        );

        // Test with VRF
        let vrf = BlockVrfData::new([1u8; 32], [2u8; 32], [3u8; 64], [4u8; 64]);
        header.set_vrf_data(Some(vrf));

        let serialized_with_vrf = header.to_bytes();
        assert_eq!(
            serialized_with_vrf.len(),
            header.size(),
            "size() must match serialized length with VRF (including binding_signature)"
        );
    }

    // ============================================================================
    // extra_nonce Deserialization Boundary Tests
    // Verifies that deserialize_extra_nonce properly validates input length
    // ============================================================================

    mod extra_nonce_deserialization_tests {
        use crate::block::EXTRA_NONCE_SIZE;
        use serde::de::IntoDeserializer;

        /// Test valid 32-byte hex string (64 hex chars) deserializes correctly
        #[test]
        fn test_valid_extra_nonce_deserializes() {
            // Valid 32-byte hex string (64 hex characters)
            let valid_hex = "00".repeat(32);
            assert_eq!(valid_hex.len(), 64);

            // Create a deserializer from the string
            let deserializer: serde::de::value::StrDeserializer<serde_json::Error> =
                valid_hex.as_str().into_deserializer();

            let result = super::super::deserialize_extra_nonce(deserializer);
            assert!(result.is_ok(), "Valid 32-byte hex should deserialize");
            assert_eq!(result.unwrap(), [0u8; EXTRA_NONCE_SIZE]);
        }

        /// Test that hex string too short fails
        #[test]
        fn test_short_extra_nonce_fails() {
            // 31 bytes = 62 hex chars (too short)
            let short_hex = "00".repeat(31);
            assert_eq!(short_hex.len(), 62);

            let deserializer: serde::de::value::StrDeserializer<serde_json::Error> =
                short_hex.as_str().into_deserializer();

            let result = super::super::deserialize_extra_nonce(deserializer);
            assert!(result.is_err(), "31-byte hex should fail");
            let err_msg = result.unwrap_err().to_string();
            assert!(
                err_msg.contains("expected 32 bytes"),
                "Error should mention expected length: {}",
                err_msg
            );
        }

        /// Test that extremely long hex string is rejected early (DoS prevention)
        #[test]
        fn test_extremely_long_hex_rejected_early() {
            // Create hex string much longer than max allowed
            // MAX_HEX_LENGTH = EXTRA_NONCE_SIZE * 2 = 64
            let extremely_long_hex = "00".repeat(1000);
            assert_eq!(extremely_long_hex.len(), 2000);

            let deserializer: serde::de::value::StrDeserializer<serde_json::Error> =
                extremely_long_hex.as_str().into_deserializer();

            let result = super::super::deserialize_extra_nonce(deserializer);
            assert!(result.is_err(), "Extremely long hex should be rejected");
            let err_msg = result.unwrap_err().to_string();
            assert!(
                err_msg.contains("exceeds maximum"),
                "Error should mention max length exceeded: {}",
                err_msg
            );
        }

        /// Test empty hex string fails
        #[test]
        fn test_empty_extra_nonce_fails() {
            let empty_hex = "";

            let deserializer: serde::de::value::StrDeserializer<serde_json::Error> =
                empty_hex.into_deserializer();

            let result = super::super::deserialize_extra_nonce(deserializer);
            assert!(result.is_err(), "Empty hex should fail");
        }
    }
}
