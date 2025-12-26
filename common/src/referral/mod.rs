// Native Referral System for TOS Blockchain
// This module provides chain-native referral relationship storage and operations.
//
// Key Features:
// - One-time referrer binding (immutable after binding)
// - Efficient upline queries (up to 100 levels)
// - Direct referral list with pagination
// - Team size caching for performance
// - Batch reward distribution to uplines

mod error;
mod record;

pub use error::*;
pub use record::*;

use serde::{Deserialize, Serialize};

/// Maximum number of upline levels that can be queried
pub const MAX_UPLINE_LEVELS: u8 = 100;

/// Maximum number of direct referrals returned per page
pub const MAX_DIRECT_REFERRALS_PER_PAGE: u32 = 1000;

/// Referral reward distribution ratios (in basis points, 100 = 1%)
/// This is the default configuration, projects can customize via smart contracts
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReferralRewardRatios {
    /// Reward ratios for each level (index 0 = level 1, etc.)
    /// Values are in basis points (100 = 1%, 10000 = 100%)
    pub ratios: Vec<u16>,
}

impl Default for ReferralRewardRatios {
    fn default() -> Self {
        // Default: 10%, 5%, 3%, 2%, 1%, 1%, 1%, 1%, 1%, 1% for 10 levels
        Self {
            ratios: vec![1000, 500, 300, 200, 100, 100, 100, 100, 100, 100],
        }
    }
}

impl ReferralRewardRatios {
    /// Create new reward ratios
    pub fn new(ratios: Vec<u16>) -> Self {
        Self { ratios }
    }

    /// Get the number of levels
    pub fn levels(&self) -> u8 {
        self.ratios.len() as u8
    }

    /// Get ratio for a specific level (0-indexed)
    pub fn get_ratio(&self, level: usize) -> Option<u16> {
        self.ratios.get(level).copied()
    }

    /// Calculate total ratio (should not exceed 10000 = 100%)
    pub fn total_ratio(&self) -> u32 {
        self.ratios.iter().map(|&r| r as u32).sum()
    }

    /// Validate that total ratio does not exceed 100%
    pub fn is_valid(&self) -> bool {
        self.total_ratio() <= 10000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_ratios() {
        let ratios = ReferralRewardRatios::default();
        assert_eq!(ratios.levels(), 10);
        assert_eq!(ratios.get_ratio(0), Some(1000)); // 10%
        assert_eq!(ratios.get_ratio(1), Some(500)); // 5%
        assert_eq!(ratios.total_ratio(), 2500); // 25%
        assert!(ratios.is_valid());
    }

    #[test]
    fn test_custom_ratios() {
        let ratios = ReferralRewardRatios::new(vec![2000, 1000, 500]);
        assert_eq!(ratios.levels(), 3);
        assert_eq!(ratios.total_ratio(), 3500); // 35%
        assert!(ratios.is_valid());
    }

    #[test]
    fn test_invalid_ratios() {
        // Total exceeds 100%
        let ratios = ReferralRewardRatios::new(vec![5000, 3000, 3000]);
        assert!(!ratios.is_valid());
    }
}
