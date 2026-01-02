use crate::serializer::{Reader, ReaderError, Serializer, Writer};
use core::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
#[derive(Default)]
pub enum TxVersion {
    // Legacy: no chain_id in signing bytes
    T0 = 0,
    // Current: chain_id + fee_limit (Stake 2.0)
    #[default]
    T1 = 1,
}

impl TryFrom<u8> for TxVersion {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(TxVersion::T0),
            1 => Ok(TxVersion::T1),
            _ => Err(()),
        }
    }
}

impl From<TxVersion> for u8 {
    fn from(val: TxVersion) -> Self {
        match val {
            TxVersion::T0 => 0,
            TxVersion::T1 => 1,
        }
    }
}

impl From<TxVersion> for u64 {
    fn from(val: TxVersion) -> Self {
        let byte: u8 = val.into();
        byte as u64
    }
}

impl Serializer for TxVersion {
    fn write(&self, writer: &mut Writer) {
        match self {
            TxVersion::T0 => writer.write_u8(0),
            TxVersion::T1 => writer.write_u8(1),
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

impl fmt::Display for TxVersion {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            TxVersion::T0 => write!(f, "T0"),
            TxVersion::T1 => write!(f, "T1"),
        }
    }
}

impl serde::Serialize for TxVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u8(*self as u8)
    }
}

impl<'de> serde::Deserialize<'de> for TxVersion {
    fn deserialize<D>(deserializer: D) -> Result<TxVersion, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = u8::deserialize(deserializer)?;
        TxVersion::try_from(value)
            .map_err(|_| serde::de::Error::custom("Invalid value for TxVersion"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tx_version_t0() {
        let version = TxVersion::T0;
        let read_version = TxVersion::from_bytes(&version.to_bytes()).expect("test");
        assert_eq!(version, read_version);
    }

    #[test]
    fn test_tx_version_t1() {
        let version = TxVersion::T1;
        let read_version = TxVersion::from_bytes(&version.to_bytes()).expect("test");
        assert_eq!(version, read_version);
    }

    #[test]
    fn test_tx_version_serde_t0() {
        let version = TxVersion::T0;
        let serialized = serde_json::to_string(&version).expect("test");
        assert!(serialized == "0");
        let deserialized: TxVersion =
            serde_json::from_str(&serialized).expect("JSON parsing should succeed");
        assert_eq!(version, deserialized);
    }

    #[test]
    fn test_tx_version_serde_t1() {
        let version = TxVersion::T1;
        let serialized = serde_json::to_string(&version).expect("test");
        assert!(serialized == "1");
        let deserialized: TxVersion =
            serde_json::from_str(&serialized).expect("JSON parsing should succeed");
        assert_eq!(version, deserialized);
    }

    #[test]
    fn test_tx_version_ord() {
        let version0 = TxVersion::T0;
        let version1 = TxVersion::T1;
        assert!(version0 < version1);
    }

    #[test]
    fn test_tx_version_default() {
        let version = TxVersion::default();
        assert_eq!(version, TxVersion::T1);
    }
}
