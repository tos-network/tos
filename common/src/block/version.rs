use crate::{
    serializer::{Reader, ReaderError, Serializer, Writer},
    transaction::TxVersion,
};
use core::fmt;

/// Block version enum representing the protocol version.
///
/// # Unified Baseline Version
///
/// All features are enabled from genesis (height 0):
/// - PoW v2 algorithm (1-second blocks)
/// - MultiSig support
/// - P2P enhancements
/// - Smart Contracts
///
/// There is only one version: `Baseline = 0`. No legacy versions exist
/// as this is a fresh start with no chain data to migrate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
#[repr(u8)]
pub enum BlockVersion {
    /// Unified baseline version with all features enabled.
    /// This is the only version - no legacy V0/V1/V2/V3 variants.
    #[default]
    Baseline = 0,
}

impl BlockVersion {
    /// Check if a transaction version is allowed in this block version.
    /// All transaction versions are allowed in Baseline.
    pub const fn is_tx_version_allowed(&self, tx_version: TxVersion) -> bool {
        matches!(tx_version, TxVersion::T0)
    }

    /// Get the transaction version for this block version.
    pub const fn get_tx_version(&self) -> TxVersion {
        TxVersion::T0
    }
}

impl TryFrom<u8> for BlockVersion {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(BlockVersion::Baseline),
            _ => Err(()),
        }
    }
}

impl Serializer for BlockVersion {
    fn write(&self, writer: &mut Writer) {
        writer.write_u8(0) // Always write as Baseline (id=0)
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError>
    where
        Self: Sized,
    {
        let id = reader.read_u8()?;
        match id {
            0 => Ok(BlockVersion::Baseline),
            _ => Err(ReaderError::InvalidValue),
        }
    }

    fn size(&self) -> usize {
        1
    }
}

impl fmt::Display for BlockVersion {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Baseline")
    }
}

impl serde::Serialize for BlockVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u8(0) // Always serialize as 0
    }
}

impl<'de> serde::Deserialize<'de> for BlockVersion {
    fn deserialize<D>(deserializer: D) -> Result<BlockVersion, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = u8::deserialize(deserializer)?;
        match value {
            0 => Ok(BlockVersion::Baseline),
            _ => Err(serde::de::Error::custom(
                "Invalid value for BlockVersion, expected 0",
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_version_serde() {
        let version = BlockVersion::Baseline;
        let serialized = serde_json::to_string(&version).unwrap();
        assert_eq!(serialized, "0");

        let deserialized: BlockVersion = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, version);
    }

    #[test]
    fn test_block_version_baseline_value() {
        assert_eq!(BlockVersion::Baseline as u8, 0);
    }

    #[test]
    fn test_block_version_try_from() {
        assert_eq!(BlockVersion::try_from(0), Ok(BlockVersion::Baseline));
        assert_eq!(BlockVersion::try_from(1), Err(()));
        assert_eq!(BlockVersion::try_from(2), Err(()));
        assert_eq!(BlockVersion::try_from(3), Err(()));
        assert_eq!(BlockVersion::try_from(255), Err(()));
    }

    #[test]
    fn test_block_version_display() {
        assert_eq!(format!("{}", BlockVersion::Baseline), "Baseline");
    }

    #[test]
    fn test_block_version_default() {
        assert_eq!(BlockVersion::default(), BlockVersion::Baseline);
    }

    #[test]
    fn test_tx_version_allowed() {
        assert!(BlockVersion::Baseline.is_tx_version_allowed(TxVersion::T0));
    }

    #[test]
    fn test_get_tx_version() {
        assert_eq!(BlockVersion::Baseline.get_tx_version(), TxVersion::T0);
    }
}
