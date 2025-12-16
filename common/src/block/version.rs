use crate::{
    serializer::{Reader, ReaderError, Serializer, Writer},
    transaction::TxVersion,
};
use core::fmt;

/// TOS Block Version - Named after Japanese Sengoku warlords
///
/// TOS adopts an "Append-Only Architecture" where each version is permanently
/// preserved and new versions are added as hard forks.
///
/// See `/version.md` for complete version naming convention (v0-v61).
///
/// # Version History
/// - v0: `Nobunaga` (織田信長) - Genesis version, the innovator who started unification
///
/// # Future Versions (Append-Only, per version.md)
/// - v1: `Nohime` (濃姫) - Nobunaga's wife
/// - v2: `Oichi` (お市の方) - Nobunaga's sister
/// - v3: `Mitsuhide` (明智光秀) - The Rebel
/// - v11: `Hideyoshi` (豊臣秀吉) - The Builder
/// - v20: `Ieyasu` (徳川家康) - The Unifier
/// - ... (see version.md for full list)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum BlockVersion {
    /// v0: Genesis version - "Nobunaga" (織田信長)
    /// The innovator who started the unification
    /// Features: PoW V2, 3s blocks, all base features
    Nobunaga = 0,
    // === Future Hard Forks (Append-Only, see version.md) ===
    // Nohime = 1,     // v1: Nobunaga's wife
    // Oichi = 2,      // v2: Nobunaga's sister
    // Mitsuhide = 3,  // v3: The Rebel
    // ... see version.md for v4-v61
}

impl BlockVersion {
    /// Check if a transaction version is allowed in a block version
    /// Currently all versions support T0 transactions
    pub const fn is_tx_version_allowed(&self, tx_version: TxVersion) -> bool {
        match self {
            BlockVersion::Nobunaga => matches!(tx_version, TxVersion::T0),
            // Future versions will inherit T0 support and may add new tx versions
        }
    }

    /// Get the transaction version for a given block version
    /// Currently all versions use T0
    pub const fn get_tx_version(&self) -> TxVersion {
        match self {
            BlockVersion::Nobunaga => TxVersion::T0,
            // Future versions may return different tx versions
        }
    }
}

impl TryFrom<u8> for BlockVersion {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(BlockVersion::Nobunaga),
            // Future versions will be added here as they are activated (see version.md)
            // 1 => Ok(BlockVersion::Nohime),
            // 2 => Ok(BlockVersion::Oichi),
            // 3 => Ok(BlockVersion::Mitsuhide),
            _ => Err(()),
        }
    }
}

impl Serializer for BlockVersion {
    fn write(&self, writer: &mut Writer) {
        match self {
            BlockVersion::Nobunaga => writer.write_u8(0),
            // Future versions will be added here
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError>
    where
        Self: Sized,
    {
        let id = reader.read_u8()?;
        Self::try_from(id).map_err(|_| ReaderError::InvalidValue)
    }

    fn size(&self) -> usize {
        1
    }
}

impl fmt::Display for BlockVersion {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            BlockVersion::Nobunaga => write!(f, "Nobunaga"),
            // Future versions will be added here
        }
    }
}

impl serde::Serialize for BlockVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u8(*self as u8)
    }
}

impl<'de> serde::Deserialize<'de> for BlockVersion {
    fn deserialize<D>(deserializer: D) -> Result<BlockVersion, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = u8::deserialize(deserializer)?;
        BlockVersion::try_from(value)
            .map_err(|_| serde::de::Error::custom("Invalid value for BlockVersion"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_version_serde() {
        let version = BlockVersion::Nobunaga;
        let serialized = serde_json::to_string(&version).unwrap();
        assert_eq!(serialized, "0");

        let deserialized: BlockVersion = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, version);
    }

    #[test]
    fn test_block_version_display() {
        assert_eq!(format!("{}", BlockVersion::Nobunaga), "Nobunaga");
    }

    #[test]
    fn test_block_version_tx_version() {
        assert!(BlockVersion::Nobunaga.is_tx_version_allowed(TxVersion::T0));
        assert_eq!(BlockVersion::Nobunaga.get_tx_version(), TxVersion::T0);
    }

    #[test]
    fn test_block_version_try_from() {
        assert_eq!(BlockVersion::try_from(0), Ok(BlockVersion::Nobunaga));
        assert_eq!(BlockVersion::try_from(1), Err(()));
        assert_eq!(BlockVersion::try_from(255), Err(()));
    }
}
