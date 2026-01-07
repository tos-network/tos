use tos_crypto::vrf::{VRF_OUTPUT_SIZE, VRF_PROOF_SIZE, VRF_PUBLIC_KEY_SIZE};

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
    let bytes = hex::decode(value).map_err(E::custom)?;
    let array: [u8; N] = bytes
        .try_into()
        .map_err(|_| E::custom("invalid hex length"))?;
    Ok(array)
}
