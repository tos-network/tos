// TOS KYC Level System
// This module provides chain-native KYC verification infrastructure.
//
// Design Philosophy:
// - On-chain: Only what's needed for smart contract access control (43 bytes/user)
// - Off-chain: Full compliance data managed by regional committees
// - Privacy: No PII or country data on blockchain
//
// Reference: TOS-KYC-Level-Design.md (v2.3)

mod approval;
mod committee;
mod data;
mod error;
mod flags;
#[cfg(test)]
mod integration_tests;
mod region;
mod status;

pub use approval::*;
pub use committee::*;
pub use data::*;
pub use error::*;
pub use flags::*;
pub use region::*;
pub use status::*;

/// Valid cumulative KYC levels (2^n - 1 pattern)
/// Each level represents completed verification items
pub const VALID_KYC_LEVELS: [u16; 9] = [0, 7, 31, 63, 255, 2047, 8191, 16383, 32767];

/// Check if a level value is valid (cumulative only)
/// Non-cumulative levels (e.g., 5, 15, 100) are not allowed
#[inline]
pub fn is_valid_kyc_level(level: u16) -> bool {
    VALID_KYC_LEVELS.contains(&level)
}

/// Convert level bitmask to tier (0-8)
#[inline]
pub fn level_to_tier(level: u16) -> u8 {
    match level {
        0 => 0,
        7 => 1,
        31 => 2,
        63 => 3,
        255 => 4,
        2047 => 5,
        8191 => 6,
        16383 => 7,
        32767 => 8,
        _ => 0, // Invalid level treated as tier 0
    }
}

/// Convert tier (0-8) to level bitmask
#[inline]
pub fn tier_to_level(tier: u8) -> u16 {
    match tier {
        0 => 0,
        1 => 7,
        2 => 31,
        3 => 63,
        4 => 255,
        5 => 2047,
        6 => 8191,
        7 => 16383,
        8 => 32767,
        _ => 0, // Invalid tier treated as level 0
    }
}

/// Get daily limit (USD) by KYC Level (u16)
///
/// # Returns
/// - Valid cumulative level: Returns the daily limit in USD
/// - Invalid/non-cumulative level: Returns 0 (caller MUST reject the transaction)
///
/// # Error Handling
/// Callers should treat return value of 0 as an error condition for non-zero levels
pub fn get_daily_limit(level: u16) -> u64 {
    match level {
        0 => 100,          // Tier 0: Anonymous - $100
        7 => 1_000,        // Tier 1: Basic - $1K
        31 => 10_000,      // Tier 2: Identity Verified - $10K
        63 => 50_000,      // Tier 3: Address Verified - $50K
        255 => 200_000,    // Tier 4: Source of Funds - $200K
        2047 => 1_000_000, // Tier 5: Enhanced Due Diligence - $1M
        8191 => u64::MAX,  // Tier 6: Institutional - No limit
        16383 => u64::MAX, // Tier 7: Audit Complete - No limit
        32767 => u64::MAX, // Tier 8: Regulated - No limit
        _ => 0,            // Invalid level - caller must reject
    }
}

/// KYC validity period in seconds based on tier
/// Reference: FATF risk-based approach
pub const fn get_validity_period_seconds(tier: u8) -> u64 {
    const SECONDS_PER_YEAR: u64 = 365 * 24 * 3600;
    match tier {
        0 => 0,                        // Tier 0: No expiration (anonymous)
        1..=2 => SECONDS_PER_YEAR,     // Tier 1-2: 1 year
        3..=4 => 2 * SECONDS_PER_YEAR, // Tier 3-4: 2 years
        5..=8 => SECONDS_PER_YEAR,     // Tier 5-8: 1 year (EDD, stricter review)
        _ => SECONDS_PER_YEAR,         // Default: 1 year
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_kyc_levels() {
        assert!(is_valid_kyc_level(0));
        assert!(is_valid_kyc_level(7));
        assert!(is_valid_kyc_level(31));
        assert!(is_valid_kyc_level(63));
        assert!(is_valid_kyc_level(255));
        assert!(is_valid_kyc_level(2047));
        assert!(is_valid_kyc_level(8191));
        assert!(is_valid_kyc_level(16383));
        assert!(is_valid_kyc_level(32767));

        // Invalid levels
        assert!(!is_valid_kyc_level(1));
        assert!(!is_valid_kyc_level(5));
        assert!(!is_valid_kyc_level(15));
        assert!(!is_valid_kyc_level(100));
        assert!(!is_valid_kyc_level(1000));
        assert!(!is_valid_kyc_level(65535));
    }

    #[test]
    fn test_level_tier_conversion() {
        for tier in 0..=8 {
            let level = tier_to_level(tier);
            assert_eq!(level_to_tier(level), tier);
        }

        // Invalid tier returns 0
        assert_eq!(tier_to_level(9), 0);
        assert_eq!(tier_to_level(100), 0);

        // Invalid level returns tier 0
        assert_eq!(level_to_tier(100), 0);
        assert_eq!(level_to_tier(65535), 0);
    }

    #[test]
    fn test_daily_limits() {
        assert_eq!(get_daily_limit(0), 100);
        assert_eq!(get_daily_limit(7), 1_000);
        assert_eq!(get_daily_limit(31), 10_000);
        assert_eq!(get_daily_limit(63), 50_000);
        assert_eq!(get_daily_limit(255), 200_000);
        assert_eq!(get_daily_limit(2047), 1_000_000);
        assert_eq!(get_daily_limit(8191), u64::MAX);
        assert_eq!(get_daily_limit(16383), u64::MAX);
        assert_eq!(get_daily_limit(32767), u64::MAX);

        // Invalid levels return 0
        assert_eq!(get_daily_limit(100), 0);
        assert_eq!(get_daily_limit(1000), 0);
    }

    #[test]
    fn test_validity_periods() {
        const YEAR: u64 = 365 * 24 * 3600;

        assert_eq!(get_validity_period_seconds(0), 0); // No expiration
        assert_eq!(get_validity_period_seconds(1), YEAR);
        assert_eq!(get_validity_period_seconds(2), YEAR);
        assert_eq!(get_validity_period_seconds(3), 2 * YEAR);
        assert_eq!(get_validity_period_seconds(4), 2 * YEAR);
        assert_eq!(get_validity_period_seconds(5), YEAR); // EDD stricter
        assert_eq!(get_validity_period_seconds(6), YEAR);
        assert_eq!(get_validity_period_seconds(7), YEAR);
        assert_eq!(get_validity_period_seconds(8), YEAR);
    }
}
