// KycData - Minimal on-chain KYC data structure (43 bytes)
// This is the core structure stored on-chain for each verified user
//
// Design Philosophy:
// - Minimal footprint: Only 43 bytes per user
// - Privacy-first: No PII or country data on-chain
// - Hash-linked: data_hash connects to full off-chain record
//
// Reference: TOS-KYC-Level-Design.md Section 3.2

use crate::crypto::Hash;
use crate::kyc::{get_validity_period_seconds, is_valid_kyc_level, level_to_tier, KycStatus};
use crate::serializer::{Reader, ReaderError, Serializer, Writer};
use serde::{Deserialize, Serialize};

/// Minimal on-chain KYC data
/// Total size: 43 bytes per user
///
/// Fields:
/// - level: u16 (2 bytes) - Verification level bitmask
/// - status: KycStatus (1 byte) - Current status
/// - verified_at: u64 (8 bytes) - Verification timestamp
/// - data_hash: Hash (32 bytes) - SHA256 of off-chain data
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct KycData {
    /// KYC level bitmask (u16)
    /// Valid values: 0, 7, 31, 63, 255, 2047, 8191, 16383, 32767
    /// Each bit represents a verification item (15 items total)
    pub level: u16,

    /// KYC status
    pub status: KycStatus,

    /// Verification timestamp (Unix timestamp in seconds)
    /// Used to calculate expiration based on tier
    pub verified_at: u64,

    /// SHA256 hash of off-chain KycOffChainData
    /// Links on-chain record to full compliance data stored by committee
    pub data_hash: Hash,
}

impl KycData {
    /// Create new KycData
    pub fn new(level: u16, verified_at: u64, data_hash: Hash) -> Self {
        Self {
            level,
            status: KycStatus::Active,
            verified_at,
            data_hash,
        }
    }

    /// Create anonymous KycData (Tier 0)
    pub fn anonymous() -> Self {
        Self {
            level: 0,
            status: KycStatus::Active,
            verified_at: 0,
            data_hash: Hash::zero(),
        }
    }

    /// Check if KYC is currently valid
    /// Valid means: status is Active AND not expired
    pub fn is_valid(&self, current_time: u64) -> bool {
        self.status == KycStatus::Active && !self.is_expired(current_time)
    }

    /// Check if KYC has expired based on tier
    /// Note: Tier 0 (anonymous) never expires (expires_at = 0 means no expiration)
    pub fn is_expired(&self, current_time: u64) -> bool {
        let expires_at = self.get_expires_at();
        // expires_at == 0 means no expiration (Tier 0)
        expires_at != 0 && current_time >= expires_at
    }

    /// Get expiration timestamp
    /// Returns 0 for Tier 0 (no expiration)
    pub fn get_expires_at(&self) -> u64 {
        let validity = self.get_validity_period();
        if validity == 0 {
            0 // No expiration
        } else {
            self.verified_at.saturating_add(validity)
        }
    }

    /// Get validity period in seconds based on tier
    /// Reference: TOS-KYC-Level-Design.md TIER_VALIDITY_SECONDS
    pub fn get_validity_period(&self) -> u64 {
        get_validity_period_seconds(self.get_tier())
    }

    /// Get tier from level bitmask
    #[inline]
    pub fn get_tier(&self) -> u8 {
        level_to_tier(self.level)
    }

    /// Check if level is valid (cumulative only)
    #[inline]
    pub fn is_level_valid(&self) -> bool {
        is_valid_kyc_level(self.level)
    }

    /// Check if user has specific verification flags
    #[inline]
    pub fn has_flags(&self, required_flags: u16) -> bool {
        (self.level & required_flags) == required_flags
    }

    /// Check if user meets a required level
    #[inline]
    pub fn meets_level(&self, required_level: u16) -> bool {
        (self.level & required_level) == required_level
    }

    /// Get effective level (0 if not valid)
    pub fn effective_level(&self, current_time: u64) -> u16 {
        if self.is_valid(current_time) {
            self.level
        } else {
            0
        }
    }

    /// Get effective tier (0 if not valid)
    pub fn effective_tier(&self, current_time: u64) -> u8 {
        if self.is_valid(current_time) {
            self.get_tier()
        } else {
            0
        }
    }

    /// Count completed verification items
    #[inline]
    pub fn verification_count(&self) -> u32 {
        self.level.count_ones()
    }

    /// Check if user has at least basic KYC (Tier 1+)
    pub fn has_basic_kyc(&self, current_time: u64) -> bool {
        self.is_valid(current_time) && self.level >= 7
    }

    /// Check if user has identity verified (Tier 2+)
    pub fn has_identity_verified(&self, current_time: u64) -> bool {
        self.is_valid(current_time) && self.level >= 31
    }

    /// Days until expiration (0 if expired or no expiration)
    pub fn days_until_expiry(&self, current_time: u64) -> u64 {
        let expires_at = self.get_expires_at();
        if expires_at == 0 {
            return u64::MAX; // No expiration
        }
        if current_time >= expires_at {
            return 0; // Already expired
        }
        (expires_at - current_time) / (24 * 3600)
    }

    /// Check if KYC is expiring soon (within 30 days)
    pub fn is_expiring_soon(&self, current_time: u64) -> bool {
        let days = self.days_until_expiry(current_time);
        days != u64::MAX && days <= 30
    }

    /// Update status
    pub fn set_status(&mut self, status: KycStatus) {
        self.status = status;
    }

    /// Renew KYC with new verification timestamp and data hash
    pub fn renew(&mut self, new_verified_at: u64, new_data_hash: Hash) {
        self.verified_at = new_verified_at;
        self.data_hash = new_data_hash;
        self.status = KycStatus::Active;
    }

    /// Upgrade level (can only increase)
    pub fn upgrade_level(&mut self, new_level: u16, new_data_hash: Hash, verified_at: u64) -> bool {
        if new_level > self.level && is_valid_kyc_level(new_level) {
            self.level = new_level;
            self.data_hash = new_data_hash;
            self.verified_at = verified_at;
            self.status = KycStatus::Active;
            true
        } else {
            false
        }
    }
}

impl Default for KycData {
    fn default() -> Self {
        Self::anonymous()
    }
}

/// KYC Appeal Record
/// Stored on-chain when a user submits an appeal to parent committee
///
/// The actual appeal review happens off-chain. This record tracks:
/// - Which committee rejected/revoked the user's KYC
/// - Which parent committee is reviewing the appeal
/// - Hashes linking to off-chain appeal documents
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct KycAppealRecord {
    /// The committee that rejected/revoked the KYC
    pub original_committee_id: Hash,

    /// The parent committee reviewing the appeal
    pub parent_committee_id: Hash,

    /// Hash of appeal reason (full reason stored off-chain)
    pub reason_hash: Hash,

    /// Hash of supporting documents
    pub documents_hash: Hash,

    /// When the appeal was submitted (Unix timestamp)
    pub submitted_at: u64,

    /// Status of the appeal
    pub status: AppealStatus,
}

/// Status of a KYC appeal
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Copy)]
#[repr(u8)]
pub enum AppealStatus {
    /// Appeal is pending review
    Pending = 0,
    /// Appeal is under review by parent committee
    UnderReview = 1,
    /// Appeal was approved - KYC reinstated
    Approved = 2,
    /// Appeal was rejected
    Rejected = 3,
    /// Appeal was withdrawn by user
    Withdrawn = 4,
}

impl KycAppealRecord {
    /// Create a new appeal record
    pub fn new(
        original_committee_id: Hash,
        parent_committee_id: Hash,
        reason_hash: Hash,
        documents_hash: Hash,
        submitted_at: u64,
    ) -> Self {
        Self {
            original_committee_id,
            parent_committee_id,
            reason_hash,
            documents_hash,
            submitted_at,
            status: AppealStatus::Pending,
        }
    }

    /// Check if appeal is still pending
    pub fn is_pending(&self) -> bool {
        matches!(self.status, AppealStatus::Pending | AppealStatus::UnderReview)
    }

    /// Check if appeal was resolved (approved or rejected)
    pub fn is_resolved(&self) -> bool {
        matches!(
            self.status,
            AppealStatus::Approved | AppealStatus::Rejected | AppealStatus::Withdrawn
        )
    }
}

impl Serializer for AppealStatus {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let byte = u8::read(reader)?;
        match byte {
            0 => Ok(AppealStatus::Pending),
            1 => Ok(AppealStatus::UnderReview),
            2 => Ok(AppealStatus::Approved),
            3 => Ok(AppealStatus::Rejected),
            4 => Ok(AppealStatus::Withdrawn),
            _ => Err(ReaderError::InvalidValue),
        }
    }

    fn write(&self, writer: &mut Writer) {
        (*self as u8).write(writer);
    }

    fn size(&self) -> usize {
        1
    }
}

impl Serializer for KycAppealRecord {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            original_committee_id: Hash::read(reader)?,
            parent_committee_id: Hash::read(reader)?,
            reason_hash: Hash::read(reader)?,
            documents_hash: Hash::read(reader)?,
            submitted_at: u64::read(reader)?,
            status: AppealStatus::read(reader)?,
        })
    }

    fn write(&self, writer: &mut Writer) {
        self.original_committee_id.write(writer);
        self.parent_committee_id.write(writer);
        self.reason_hash.write(writer);
        self.documents_hash.write(writer);
        self.submitted_at.write(writer);
        self.status.write(writer);
    }

    fn size(&self) -> usize {
        // 32 + 32 + 32 + 32 + 8 + 1 = 137 bytes
        self.original_committee_id.size()
            + self.parent_committee_id.size()
            + self.reason_hash.size()
            + self.documents_hash.size()
            + self.submitted_at.size()
            + self.status.size()
    }
}

impl Serializer for KycData {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let level = u16::read(reader)?;
        let status = KycStatus::read(reader)?;
        let verified_at = u64::read(reader)?;
        let data_hash = Hash::read(reader)?;

        Ok(Self {
            level,
            status,
            verified_at,
            data_hash,
        })
    }

    fn write(&self, writer: &mut Writer) {
        self.level.write(writer);
        self.status.write(writer);
        self.verified_at.write(writer);
        self.data_hash.write(writer);
    }

    fn size(&self) -> usize {
        // 2 + 1 + 8 + 32 = 43 bytes
        self.level.size() + self.status.size() + self.verified_at.size() + self.data_hash.size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_hash() -> Hash {
        Hash::new([1u8; 32])
    }

    #[test]
    fn test_kyc_data_new() {
        let kyc = KycData::new(31, 1000, sample_hash());
        assert_eq!(kyc.level, 31);
        assert_eq!(kyc.status, KycStatus::Active);
        assert_eq!(kyc.verified_at, 1000);
        assert_eq!(kyc.get_tier(), 2);
    }

    #[test]
    fn test_anonymous() {
        let kyc = KycData::anonymous();
        assert_eq!(kyc.level, 0);
        assert_eq!(kyc.get_tier(), 0);
        assert_eq!(kyc.get_expires_at(), 0); // No expiration
    }

    #[test]
    fn test_expiration_tier0() {
        let kyc = KycData::anonymous();
        // Tier 0 never expires
        assert!(!kyc.is_expired(0));
        assert!(!kyc.is_expired(u64::MAX));
        assert_eq!(kyc.days_until_expiry(0), u64::MAX);
    }

    #[test]
    fn test_expiration_tier2() {
        let verified_at = 1000;
        let kyc = KycData::new(31, verified_at, sample_hash()); // Tier 2

        // Tier 2 validity: 1 year
        let one_year = 365 * 24 * 3600;

        // Before expiration
        assert!(!kyc.is_expired(verified_at + one_year - 1));
        assert!(kyc.is_valid(verified_at + one_year - 1));

        // After expiration
        assert!(kyc.is_expired(verified_at + one_year));
        assert!(!kyc.is_valid(verified_at + one_year));
    }

    #[test]
    fn test_expiration_tier4() {
        let verified_at = 1000;
        let kyc = KycData::new(255, verified_at, sample_hash()); // Tier 4

        // Tier 4 validity: 2 years
        let two_years = 2 * 365 * 24 * 3600;

        assert!(!kyc.is_expired(verified_at + two_years - 1));
        assert!(kyc.is_expired(verified_at + two_years));
    }

    #[test]
    fn test_expiration_tier5() {
        let verified_at = 1000;
        let kyc = KycData::new(2047, verified_at, sample_hash()); // Tier 5 (EDD)

        // Tier 5+ validity: 1 year (stricter review)
        let one_year = 365 * 24 * 3600;

        assert!(!kyc.is_expired(verified_at + one_year - 1));
        assert!(kyc.is_expired(verified_at + one_year));
    }

    #[test]
    fn test_is_valid() {
        let kyc = KycData::new(31, 1000, sample_hash());

        // Active and not expired
        assert!(kyc.is_valid(1001));

        // Expired
        let one_year = 365 * 24 * 3600;
        assert!(!kyc.is_valid(1000 + one_year + 1));
    }

    #[test]
    fn test_is_valid_status() {
        let mut kyc = KycData::new(31, 1000, sample_hash());

        kyc.status = KycStatus::Revoked;
        assert!(!kyc.is_valid(1001));

        kyc.status = KycStatus::Suspended;
        assert!(!kyc.is_valid(1001));

        kyc.status = KycStatus::Expired;
        assert!(!kyc.is_valid(1001));
    }

    #[test]
    fn test_has_flags() {
        let kyc = KycData::new(63, 1000, sample_hash()); // Tier 3

        // Has Tier 1 flags
        assert!(kyc.has_flags(7));
        // Has Tier 2 flags
        assert!(kyc.has_flags(31));
        // Has Tier 3 flags
        assert!(kyc.has_flags(63));
        // Does not have Tier 4 flags
        assert!(!kyc.has_flags(255));
    }

    #[test]
    fn test_effective_level() {
        let kyc = KycData::new(63, 1000, sample_hash()); // Tier 3: 2 years validity

        // Valid: returns actual level
        assert_eq!(kyc.effective_level(1001), 63);

        // Expired: returns 0 (Tier 3 has 2 year validity)
        let two_years = 2 * 365 * 24 * 3600;
        assert_eq!(kyc.effective_level(1000 + two_years + 1), 0);
    }

    #[test]
    fn test_upgrade_level() {
        let mut kyc = KycData::new(31, 1000, sample_hash()); // Tier 2

        // Upgrade to Tier 3
        let new_hash = Hash::new([2u8; 32]);
        assert!(kyc.upgrade_level(63, new_hash.clone(), 2000));
        assert_eq!(kyc.level, 63);
        assert_eq!(kyc.data_hash, new_hash);
        assert_eq!(kyc.verified_at, 2000);

        // Cannot downgrade
        let another_hash = Hash::new([3u8; 32]);
        assert!(!kyc.upgrade_level(31, another_hash, 3000));
        assert_eq!(kyc.level, 63); // Unchanged

        // Cannot upgrade to invalid level
        assert!(!kyc.upgrade_level(100, Hash::new([4u8; 32]), 4000));
    }

    #[test]
    fn test_renew() {
        let mut kyc = KycData::new(31, 1000, sample_hash());
        kyc.status = KycStatus::Expired;

        let new_hash = Hash::new([2u8; 32]);
        kyc.renew(5000, new_hash.clone());

        assert_eq!(kyc.verified_at, 5000);
        assert_eq!(kyc.data_hash, new_hash);
        assert_eq!(kyc.status, KycStatus::Active);
    }

    #[test]
    fn test_verification_count() {
        assert_eq!(KycData::new(0, 0, sample_hash()).verification_count(), 0);
        assert_eq!(KycData::new(7, 0, sample_hash()).verification_count(), 3);
        assert_eq!(KycData::new(31, 0, sample_hash()).verification_count(), 5);
        assert_eq!(KycData::new(63, 0, sample_hash()).verification_count(), 6);
        assert_eq!(KycData::new(255, 0, sample_hash()).verification_count(), 8);
        assert_eq!(
            KycData::new(32767, 0, sample_hash()).verification_count(),
            15
        );
    }

    #[test]
    fn test_expiring_soon() {
        let verified_at = 1000;
        let kyc = KycData::new(31, verified_at, sample_hash()); // Tier 2: 1 year validity

        let one_year = 365 * 24 * 3600;
        let thirty_days = 30 * 24 * 3600;

        // Not expiring soon (more than 30 days left)
        assert!(!kyc.is_expiring_soon(verified_at));
        // Note: one_year - thirty_days - 1 still leaves more than 30 days
        // 365 - 30 = 335 days elapsed, 30 days remaining (not "expiring soon" means > 30 days)
        // Actually at one_year - thirty_days - 1, we have 30 days + 1 second left, so not expiring soon
        assert!(!kyc.is_expiring_soon(verified_at + one_year - thirty_days - 24 * 3600));

        // Expiring soon (within 30 days)
        assert!(kyc.is_expiring_soon(verified_at + one_year - thirty_days + 1));
        assert!(kyc.is_expiring_soon(verified_at + one_year - 1));

        // Already expired (days_until_expiry returns 0)
        let days = kyc.days_until_expiry(verified_at + one_year + 1);
        assert_eq!(days, 0);
    }
}
