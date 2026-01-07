use crate::crypto::elgamal::CompressedPublicKey;
use tos_crypto::vrf::{VRF_OUTPUT_SIZE, VRF_PROOF_SIZE, VRF_PUBLIC_KEY_SIZE};

/// Domain separator for VRF input computation.
/// This prevents cross-protocol attacks and ensures inputs are unique to TOS VRF.
const VRF_INPUT_DOMAIN: &[u8] = b"TOS-VRF-INPUT-v1";

/// Compute VRF input that binds to block producer identity.
///
/// This prevents VRF proof substitution attacks where an attacker
/// with a valid VRF key replaces another miner's VRF proof.
///
/// # Security
///
/// The VRF input is computed as:
/// ```text
/// vrf_input = BLAKE3("TOS-VRF-INPUT-v1" || block_hash || miner_public_key)
/// ```
///
/// This ensures:
/// 1. Different miners produce different VRF inputs (even for same block hash)
/// 2. An attacker cannot reuse another miner's VRF proof
/// 3. The domain separator prevents cross-protocol attacks
pub fn compute_vrf_input(block_hash: &[u8; 32], miner: &CompressedPublicKey) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(VRF_INPUT_DOMAIN);
    hasher.update(block_hash);
    hasher.update(miner.as_bytes());

    let hash = hasher.finalize();
    *hash.as_bytes()
}

/// VRF data committed in a block.
///
/// This data is produced by the block producer and validated by nodes
/// before contract execution to enable verifiable randomness syscalls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockVrfData {
    pub public_key: [u8; VRF_PUBLIC_KEY_SIZE],
    pub output: [u8; VRF_OUTPUT_SIZE],
    pub proof: [u8; VRF_PROOF_SIZE],
}

impl BlockVrfData {
    pub fn new(
        public_key: [u8; VRF_PUBLIC_KEY_SIZE],
        output: [u8; VRF_OUTPUT_SIZE],
        proof: [u8; VRF_PROOF_SIZE],
    ) -> Self {
        Self {
            public_key,
            output,
            proof,
        }
    }
}

impl serde::Serialize for BlockVrfData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("BlockVrfData", 3)?;
        state.serialize_field("public_key", &hex::encode(self.public_key))?;
        state.serialize_field("output", &hex::encode(self.output))?;
        state.serialize_field("proof", &hex::encode(self.proof))?;
        state.end()
    }
}

impl<'de> serde::Deserialize<'de> for BlockVrfData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct BlockVrfDataHex {
            public_key: String,
            output: String,
            proof: String,
        }

        let helper = BlockVrfDataHex::deserialize(deserializer)?;

        let public_key = decode_fixed::<VRF_PUBLIC_KEY_SIZE, D::Error>(&helper.public_key)?;
        let output = decode_fixed::<VRF_OUTPUT_SIZE, D::Error>(&helper.output)?;
        let proof = decode_fixed::<VRF_PROOF_SIZE, D::Error>(&helper.proof)?;

        Ok(BlockVrfData {
            public_key,
            output,
            proof,
        })
    }
}

fn decode_fixed<const N: usize, E: serde::de::Error>(value: &str) -> Result<[u8; N], E> {
    // SECURITY: Limit hex string length to prevent DoS via unbounded allocation
    // Expected length is N*2 hex chars, allow small margin for whitespace
    const MAX_HEX_MARGIN: usize = 4;
    let max_len = N * 2 + MAX_HEX_MARGIN;
    if value.len() > max_len {
        return Err(E::custom(format!(
            "hex string too long: {} > {}",
            value.len(),
            max_len
        )));
    }

    let bytes = hex::decode(value).map_err(E::custom)?;
    let array: [u8; N] = bytes
        .try_into()
        .map_err(|_| E::custom("invalid hex length"))?;
    Ok(array)
}
