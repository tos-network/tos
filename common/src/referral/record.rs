// Referral record data structures

use crate::block::TopoHeight;
use crate::crypto::{Hash, PublicKey};
use crate::serializer::{Reader, ReaderError, Serializer, Writer};
use serde::{Deserialize, Serialize};

/// A referral relationship record stored on chain
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReferralRecord {
    /// The user's public key
    pub user: PublicKey,

    /// The referrer's public key (None = no referrer / top-level user)
    pub referrer: Option<PublicKey>,

    /// Block height when the binding occurred
    pub bound_at_topoheight: TopoHeight,

    /// Transaction hash of the binding transaction
    pub bound_tx_hash: Hash,

    /// Timestamp when the binding occurred (Unix timestamp in seconds)
    pub bound_timestamp: u64,

    /// Cached count of direct referrals (users who have this user as referrer)
    pub direct_referrals_count: u32,

    /// Cached total team size (all descendants in the referral tree)
    /// This is updated lazily for performance
    pub team_size: u64,
}

impl ReferralRecord {
    /// Create a new referral record
    pub fn new(
        user: PublicKey,
        referrer: Option<PublicKey>,
        bound_at_topoheight: TopoHeight,
        bound_tx_hash: Hash,
        bound_timestamp: u64,
    ) -> Self {
        Self {
            user,
            referrer,
            bound_at_topoheight,
            bound_tx_hash,
            bound_timestamp,
            direct_referrals_count: 0,
            team_size: 0,
        }
    }

    /// Check if this user has a referrer
    pub fn has_referrer(&self) -> bool {
        self.referrer.is_some()
    }

    /// Increment direct referrals count
    pub fn increment_direct_count(&mut self) {
        self.direct_referrals_count = self.direct_referrals_count.saturating_add(1);
    }

    /// Decrement direct referrals count (for rollback scenarios)
    pub fn decrement_direct_count(&mut self) {
        self.direct_referrals_count = self.direct_referrals_count.saturating_sub(1);
    }

    /// Update team size
    pub fn set_team_size(&mut self, size: u64) {
        self.team_size = size;
    }

    /// Increment team size
    pub fn increment_team_size(&mut self, delta: u64) {
        self.team_size = self.team_size.saturating_add(delta);
    }
}

impl Serializer for ReferralRecord {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let user = PublicKey::read(reader)?;
        let referrer = Option::<PublicKey>::read(reader)?;
        let bound_at_topoheight = TopoHeight::read(reader)?;
        let bound_tx_hash = Hash::read(reader)?;
        let bound_timestamp = u64::read(reader)?;
        let direct_referrals_count = u32::read(reader)?;
        let team_size = u64::read(reader)?;

        Ok(Self {
            user,
            referrer,
            bound_at_topoheight,
            bound_tx_hash,
            bound_timestamp,
            direct_referrals_count,
            team_size,
        })
    }

    fn write(&self, writer: &mut Writer) {
        self.user.write(writer);
        self.referrer.write(writer);
        self.bound_at_topoheight.write(writer);
        self.bound_tx_hash.write(writer);
        self.bound_timestamp.write(writer);
        self.direct_referrals_count.write(writer);
        self.team_size.write(writer);
    }

    fn size(&self) -> usize {
        self.user.size()
            + self.referrer.size()
            + self.bound_at_topoheight.size()
            + self.bound_tx_hash.size()
            + self.bound_timestamp.size()
            + self.direct_referrals_count.size()
            + self.team_size.size()
    }
}

/// Pagination index for large teams
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReferralsIndex {
    /// Total count of direct referrals
    pub total_count: u64,

    /// Page size used for pagination
    pub page_size: u32,

    /// Number of pages
    pub page_count: u32,
}

impl ReferralsIndex {
    /// Create a new index
    pub fn new(total_count: u64, page_size: u32) -> Self {
        let page_count = if total_count == 0 {
            0
        } else {
            ((total_count - 1) / page_size as u64 + 1) as u32
        };

        Self {
            total_count,
            page_size,
            page_count,
        }
    }

    /// Get page number for a given offset
    pub fn get_page_for_offset(&self, offset: u64) -> u32 {
        if self.page_size == 0 {
            return 0;
        }
        (offset / self.page_size as u64) as u32
    }
}

/// Result of upline query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UplineResult {
    /// List of upline public keys, ordered from immediate referrer to higher levels
    pub uplines: Vec<PublicKey>,

    /// Number of levels actually returned (may be less than requested if chain is shorter)
    pub levels_returned: u8,
}

impl UplineResult {
    /// Create a new upline result
    pub fn new(uplines: Vec<PublicKey>) -> Self {
        let levels_returned = uplines.len() as u8;
        Self {
            uplines,
            levels_returned,
        }
    }

    /// Check if any uplines were found
    pub fn is_empty(&self) -> bool {
        self.uplines.is_empty()
    }
}

/// Result of direct referrals query with pagination
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectReferralsResult {
    /// List of direct referral public keys
    pub referrals: Vec<PublicKey>,

    /// Total count of direct referrals
    pub total_count: u32,

    /// Current offset
    pub offset: u32,

    /// Whether there are more results
    pub has_more: bool,
}

impl DirectReferralsResult {
    /// Create a new result
    pub fn new(referrals: Vec<PublicKey>, total_count: u32, offset: u32) -> Self {
        let referrals_len = referrals.len() as u32;
        let has_more = (offset + referrals_len) < total_count;
        Self {
            referrals,
            total_count,
            offset,
            has_more,
        }
    }
}

/// Reward distribution entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardDistribution {
    /// Recipient public key
    pub recipient: PublicKey,

    /// Amount distributed
    pub amount: u64,

    /// Level (1 = immediate referrer, 2 = referrer's referrer, etc.)
    pub level: u8,
}

/// Result of batch reward distribution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributionResult {
    /// List of distributions made
    pub distributions: Vec<RewardDistribution>,

    /// Total amount distributed
    pub total_distributed: u64,

    /// Number of levels that received rewards
    pub levels_rewarded: u8,
}

impl DistributionResult {
    /// Create a new distribution result
    pub fn new(distributions: Vec<RewardDistribution>) -> Self {
        let total_distributed = distributions.iter().map(|d| d.amount).sum();
        let levels_rewarded = distributions.len() as u8;
        Self {
            distributions,
            total_distributed,
            levels_rewarded,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::KeyPair;

    fn generate_keypair() -> KeyPair {
        KeyPair::new()
    }

    #[test]
    fn test_referral_record_creation() {
        let user_kp = generate_keypair();
        let referrer_kp = generate_keypair();

        let record = ReferralRecord::new(
            user_kp.get_public_key().compress(),
            Some(referrer_kp.get_public_key().compress()),
            100,
            Hash::zero(),
            1234567890,
        );

        assert!(record.has_referrer());
        assert_eq!(record.direct_referrals_count, 0);
        assert_eq!(record.team_size, 0);
    }

    #[test]
    fn test_referral_record_no_referrer() {
        let user_kp = generate_keypair();

        let record = ReferralRecord::new(
            user_kp.get_public_key().compress(),
            None,
            100,
            Hash::zero(),
            1234567890,
        );

        assert!(!record.has_referrer());
    }

    #[test]
    fn test_referrals_index() {
        let index = ReferralsIndex::new(1500, 1000);
        assert_eq!(index.page_count, 2);
        assert_eq!(index.get_page_for_offset(0), 0);
        assert_eq!(index.get_page_for_offset(999), 0);
        assert_eq!(index.get_page_for_offset(1000), 1);
    }

    #[test]
    fn test_upline_result() {
        let result = UplineResult::new(vec![]);
        assert!(result.is_empty());
        assert_eq!(result.levels_returned, 0);

        let kp = generate_keypair();
        let result = UplineResult::new(vec![kp.get_public_key().compress()]);
        assert!(!result.is_empty());
        assert_eq!(result.levels_returned, 1);
    }
}
