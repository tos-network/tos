// KYC Status enumeration
// Represents the current state of a user's KYC verification

use crate::serializer::{Reader, ReaderError, Serializer, Writer};
use serde::{Deserialize, Serialize};

/// KYC status enumeration - distinguishes between different KYC states
/// Stored as u8 (1 byte) on-chain
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum KycStatus {
    /// KYC is active and valid
    /// User can perform transactions up to their tier limit
    #[default]
    Active = 0,

    /// KYC has expired (was valid, now past expiration date)
    /// User must renew to restore full access
    Expired = 1,

    /// KYC has been explicitly revoked by committee
    /// Usually due to fraud, sanctions, or other compliance issues
    /// Requires appeal process to restore
    Revoked = 2,

    /// KYC is temporarily suspended pending review
    /// May be due to suspicious activity or re-verification requirement
    /// Can be lifted by committee without full re-verification
    Suspended = 3,
}

impl KycStatus {
    /// Check if this status allows transactions
    /// Only Active status permits normal transaction activity
    #[inline]
    pub fn allows_transactions(&self) -> bool {
        matches!(self, KycStatus::Active)
    }

    /// Check if this status is considered valid (not revoked or suspended)
    /// Expired status is "valid" in the sense that the user completed KYC,
    /// but transactions are not allowed without renewal
    #[inline]
    pub fn is_valid(&self) -> bool {
        !matches!(self, KycStatus::Revoked | KycStatus::Suspended)
    }

    /// Check if this status can be renewed
    /// Active and Expired statuses can be renewed
    /// Revoked and Suspended require special handling
    #[inline]
    pub fn can_renew(&self) -> bool {
        matches!(self, KycStatus::Active | KycStatus::Expired)
    }

    /// Get human-readable status name
    pub fn as_str(&self) -> &'static str {
        match self {
            KycStatus::Active => "Active",
            KycStatus::Expired => "Expired",
            KycStatus::Revoked => "Revoked",
            KycStatus::Suspended => "Suspended",
        }
    }

    /// Convert from u8 for deserialization
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(KycStatus::Active),
            1 => Some(KycStatus::Expired),
            2 => Some(KycStatus::Revoked),
            3 => Some(KycStatus::Suspended),
            _ => None,
        }
    }

    /// Convert to u8 for serialization
    #[inline]
    pub fn to_u8(self) -> u8 {
        self as u8
    }
}

impl std::fmt::Display for KycStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl Serializer for KycStatus {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let value = u8::read(reader)?;
        KycStatus::from_u8(value).ok_or(ReaderError::InvalidValue)
    }

    fn write(&self, writer: &mut Writer) {
        self.to_u8().write(writer);
    }

    fn size(&self) -> usize {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allows_transactions() {
        assert!(KycStatus::Active.allows_transactions());
        assert!(!KycStatus::Expired.allows_transactions());
        assert!(!KycStatus::Revoked.allows_transactions());
        assert!(!KycStatus::Suspended.allows_transactions());
    }

    #[test]
    fn test_is_valid() {
        assert!(KycStatus::Active.is_valid());
        assert!(KycStatus::Expired.is_valid()); // Valid but expired
        assert!(!KycStatus::Revoked.is_valid());
        assert!(!KycStatus::Suspended.is_valid());
    }

    #[test]
    fn test_can_renew() {
        assert!(KycStatus::Active.can_renew());
        assert!(KycStatus::Expired.can_renew());
        assert!(!KycStatus::Revoked.can_renew());
        assert!(!KycStatus::Suspended.can_renew());
    }

    #[test]
    fn test_u8_conversion() {
        for status in [
            KycStatus::Active,
            KycStatus::Expired,
            KycStatus::Revoked,
            KycStatus::Suspended,
        ] {
            let value = status.to_u8();
            let restored = KycStatus::from_u8(value);
            assert_eq!(restored, Some(status));
        }

        // Invalid values
        assert_eq!(KycStatus::from_u8(4), None);
        assert_eq!(KycStatus::from_u8(255), None);
    }

    #[test]
    fn test_display() {
        assert_eq!(KycStatus::Active.to_string(), "Active");
        assert_eq!(KycStatus::Expired.to_string(), "Expired");
        assert_eq!(KycStatus::Revoked.to_string(), "Revoked");
        assert_eq!(KycStatus::Suspended.to_string(), "Suspended");
    }
}
