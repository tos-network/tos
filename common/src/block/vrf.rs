use crate::crypto::elgamal::CompressedPublicKey;
use crate::crypto::SIGNATURE_SIZE;
use tos_crypto::vrf::{VRF_OUTPUT_SIZE, VRF_PROOF_SIZE, VRF_PUBLIC_KEY_SIZE};

/// Domain separator for VRF input computation.
/// This prevents cross-protocol attacks and ensures inputs are unique to TOS VRF.
const VRF_INPUT_DOMAIN: &[u8] = b"TOS-VRF-INPUT-v1";

/// Domain separator for VRF binding signature.
/// This binds a VRF key to a specific miner for a specific block.
const VRF_BINDING_DOMAIN: &[u8] = b"TOS-VRF-BINDING-v1";

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

/// Compute the message that miner signs to bind VRF key to block.
///
/// This creates a unique binding between:
/// - The chain (via chain_id) - prevents cross-chain replay
/// - The VRF public key being used
/// - The specific block (via block_hash)
///
/// # Security
///
/// The binding message is computed as:
/// ```text
/// message = BLAKE3("TOS-VRF-BINDING-v1" || chain_id || vrf_public_key || block_hash)
/// ```
///
/// This message is then signed by the miner using their keypair.
/// The signature proves the miner authorized this VRF key for this block.
///
/// # Arguments
///
/// * `chain_id` - Network::chain_id() value (0=Mainnet, 1=Testnet, 2=Stagenet, 3=Devnet)
/// * `vrf_public_key` - The VRF public key being bound
/// * `block_hash` - The block hash (excludes VRF fields)
pub fn compute_vrf_binding_message(
    chain_id: u64,
    vrf_public_key: &[u8; VRF_PUBLIC_KEY_SIZE],
    block_hash: &[u8; 32],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(VRF_BINDING_DOMAIN);
    hasher.update(&chain_id.to_le_bytes());
    hasher.update(vrf_public_key);
    hasher.update(block_hash);
    *hasher.finalize().as_bytes()
}

/// VRF data committed in a block.
///
/// This data is produced by the block producer and validated by nodes
/// before contract execution to enable verifiable randomness syscalls.
///
/// # Security
///
/// The `binding_signature` field prevents VRF proof substitution attacks.
/// It is the miner's signature over:
/// ```text
/// BLAKE3("TOS-VRF-BINDING-v1" || chain_id || vrf_public_key || block_hash)
/// ```
/// This proves the miner authorized this specific VRF key for this block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockVrfData {
    /// VRF public key (32 bytes)
    pub public_key: [u8; VRF_PUBLIC_KEY_SIZE],
    /// VRF output - the verifiable random value (32 bytes)
    pub output: [u8; VRF_OUTPUT_SIZE],
    /// VRF proof for verification (64 bytes)
    pub proof: [u8; VRF_PROOF_SIZE],
    /// Miner's signature binding VRF key to this block (64 bytes)
    pub binding_signature: [u8; SIGNATURE_SIZE],
}

impl BlockVrfData {
    pub fn new(
        public_key: [u8; VRF_PUBLIC_KEY_SIZE],
        output: [u8; VRF_OUTPUT_SIZE],
        proof: [u8; VRF_PROOF_SIZE],
        binding_signature: [u8; SIGNATURE_SIZE],
    ) -> Self {
        Self {
            public_key,
            output,
            proof,
            binding_signature,
        }
    }
}

impl serde::Serialize for BlockVrfData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("BlockVrfData", 4)?;
        state.serialize_field("public_key", &hex::encode(self.public_key))?;
        state.serialize_field("output", &hex::encode(self.output))?;
        state.serialize_field("proof", &hex::encode(self.proof))?;
        state.serialize_field("binding_signature", &hex::encode(self.binding_signature))?;
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
            binding_signature: String,
        }

        let helper = BlockVrfDataHex::deserialize(deserializer)?;

        let public_key = decode_fixed::<VRF_PUBLIC_KEY_SIZE, D::Error>(&helper.public_key)?;
        let output = decode_fixed::<VRF_OUTPUT_SIZE, D::Error>(&helper.output)?;
        let proof = decode_fixed::<VRF_PROOF_SIZE, D::Error>(&helper.proof)?;
        let binding_signature =
            decode_fixed::<SIGNATURE_SIZE, D::Error>(&helper.binding_signature)?;

        Ok(BlockVrfData {
            public_key,
            output,
            proof,
            binding_signature,
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
