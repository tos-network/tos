use crate::serializer::{Reader, ReaderError, Serializer, Writer};
use core::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum TxVersion {
    // All operations: Burn, Transfer, Multisig, Deploy Contract, Invoke Contract, Energy
    T0 = 0,
    // Parallel execution support: Pre-declared account dependencies
    V2 = 2
}

impl Default for TxVersion {
    fn default() -> Self {
        TxVersion::T0
    }
}

impl TryFrom<u8> for TxVersion {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(TxVersion::T0),
            2 => Ok(TxVersion::V2),
            _ => Err(()),
        }
    }
}

impl Into<u8> for TxVersion {
    fn into(self) -> u8 {
        match self {
            TxVersion::T0 => 0,
            TxVersion::V2 => 2,
        }
    }
}

impl Into<u64> for TxVersion {
    fn into(self) -> u64 {
        let byte: u8 = self.into();
        byte as u64
    }
}

impl Serializer for TxVersion {
    fn write(&self, writer: &mut Writer) {
        match self {
            TxVersion::T0 => writer.write_u8(0),
            TxVersion::V2 => writer.write_u8(2),
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError>
        where Self: Sized {
        let id = reader.read_u8()?;
        Self::try_from(id).map_err(|_| ReaderError::InvalidValue)
    }

    fn size(&self) -> usize {
        1
    }
}

impl fmt::Display for TxVersion {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            TxVersion::T0 => write!(f, "T0"),
            TxVersion::V2 => write!(f, "V2"),
        }
    }
}

impl serde::Serialize for TxVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where S: serde::Serializer {
        serializer.serialize_u8(*self as u8)
    }
}

impl<'de> serde::Deserialize<'de> for TxVersion {
    fn deserialize<D>(deserializer: D) -> Result<TxVersion, D::Error>
        where D: serde::Deserializer<'de> {
        let value = u8::deserialize(deserializer)?;
        TxVersion::try_from(value).map_err(|_| serde::de::Error::custom("Invalid value for TxVersion"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tx_version() {
        let version = TxVersion::T0;
        let read_version = TxVersion::from_bytes(&version.to_bytes()).unwrap();
        assert_eq!(version, read_version);
    }

    #[test]
    fn test_tx_version_serde() {
        let version = TxVersion::T0;
        let serialized = serde_json::to_string(&version).unwrap();
        assert!(serialized == "0");
        let deserialized: TxVersion = serde_json::from_str(&serialized).unwrap();
        assert_eq!(version, deserialized);
    }

    #[test]
    fn test_tx_version_ord() {
        let version0 = TxVersion::T0;
        let version1 = TxVersion::T0;
        assert!(version0 == version1);
    }
}