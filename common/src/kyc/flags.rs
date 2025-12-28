// KYC Flags - Bitmask constants for verification items
// Each bit represents an independent verification item (15 items total)
//
// Design: u16 bitmask allows up to 16 verification flags
// Current implementation uses 15 flags (bit 0-14), bit 15 reserved
//
// Reference: TOS-KYC-Level-Design.md Section 2.4

/// KYC verification item flags (u16 bitmask)
/// Each constant represents a specific verification that has been completed
#[allow(non_snake_case)]
pub mod KycFlags {
    // ===== Basic Individual Verification (bit 0-4) =====

    /// Email verification completed
    pub const EMAIL: u16 = 1 << 0; // 1

    /// Phone verification (SMS) completed
    pub const PHONE: u16 = 1 << 1; // 2

    /// Basic info (name, date of birth) provided
    pub const BASIC_INFO: u16 = 1 << 2; // 4

    /// Government-issued ID verified
    pub const GOV_ID: u16 = 1 << 3; // 8

    /// Face/liveness check passed
    pub const LIVENESS: u16 = 1 << 4; // 16

    // ===== Enhanced Verification (bit 5-7) =====

    /// Proof of address (utility bill, bank statement) verified
    pub const ADDRESS: u16 = 1 << 5; // 32

    /// Source of funds documented
    pub const SOF: u16 = 1 << 6; // 64

    /// Source of wealth documented
    pub const SOW: u16 = 1 << 7; // 128

    // ===== Due Diligence (bit 8-10) =====

    /// Background check completed
    pub const BACKGROUND: u16 = 1 << 8; // 256

    /// PEP (Politically Exposed Person) and sanctions screening completed
    pub const SCREENING: u16 = 1 << 9; // 512

    /// Ultimate Beneficial Owner (UBO) identified
    pub const UBO: u16 = 1 << 10; // 1024

    // ===== Institutional Verification (bit 11-14) =====

    /// Company registration verified
    pub const COMPANY: u16 = 1 << 11; // 2048

    /// Directors and shareholders verified
    pub const DIRECTORS: u16 = 1 << 12; // 4096

    /// Compliance audit completed
    pub const AUDIT: u16 = 1 << 13; // 8192

    /// Financial license verified
    pub const LICENSE: u16 = 1 << 14; // 16384

    // ===== Reserved (bit 15) =====

    /// Reserved for future expansion
    pub const RESERVED: u16 = 1 << 15; // 32768

    // ===== Composite Tier Masks (Cumulative) =====
    // Each tier includes all previous tier's flags

    /// Tier 0: Anonymous (no verification)
    pub const TIER_0: u16 = 0;

    /// Tier 1: Basic registration (Email + Phone + Basic Info)
    /// 3 items: 2^3 - 1 = 7
    pub const TIER_1: u16 = EMAIL | PHONE | BASIC_INFO; // 7

    /// Tier 2: Identity verified (Basic + Gov ID + Liveness)
    /// 5 items: 2^5 - 1 = 31
    pub const TIER_2: u16 = TIER_1 | GOV_ID | LIVENESS; // 31

    /// Tier 3: Address verified (Identity + Address)
    /// 6 items: 2^6 - 1 = 63
    pub const TIER_3: u16 = TIER_2 | ADDRESS; // 63

    /// Tier 4: Source of funds verified (Address + SOF + SOW)
    /// 8 items: 2^8 - 1 = 255
    pub const TIER_4: u16 = TIER_3 | SOF | SOW; // 255

    /// Tier 5: Enhanced Due Diligence (SOF + Background + Screening + UBO)
    /// 11 items: 2^11 - 1 = 2047
    pub const TIER_5: u16 = TIER_4 | BACKGROUND | SCREENING | UBO; // 2047

    /// Tier 6: Institutional (EDD + Company + Directors)
    /// 13 items: 2^13 - 1 = 8191
    pub const TIER_6: u16 = TIER_5 | COMPANY | DIRECTORS; // 8191

    /// Tier 7: Audit complete (Institutional + Audit)
    /// 14 items: 2^14 - 1 = 16383
    pub const TIER_7: u16 = TIER_6 | AUDIT; // 16383

    /// Tier 8: Regulated entity (Audit + License)
    /// 15 items: 2^15 - 1 = 32767
    pub const TIER_8: u16 = TIER_7 | LICENSE; // 32767
}

/// Human-readable level names
#[allow(non_snake_case)]
pub mod KycLevelNames {
    pub const TIER_0_NAME: &str = "Anonymous";
    pub const TIER_1_NAME: &str = "Basic";
    pub const TIER_2_NAME: &str = "Identity Verified";
    pub const TIER_3_NAME: &str = "Address Verified";
    pub const TIER_4_NAME: &str = "Source of Funds";
    pub const TIER_5_NAME: &str = "Enhanced Due Diligence";
    pub const TIER_6_NAME: &str = "Institutional";
    pub const TIER_7_NAME: &str = "Audit Complete";
    pub const TIER_8_NAME: &str = "Regulated";
}

/// Get human-readable name for a tier
pub fn get_tier_name(tier: u8) -> &'static str {
    match tier {
        0 => KycLevelNames::TIER_0_NAME,
        1 => KycLevelNames::TIER_1_NAME,
        2 => KycLevelNames::TIER_2_NAME,
        3 => KycLevelNames::TIER_3_NAME,
        4 => KycLevelNames::TIER_4_NAME,
        5 => KycLevelNames::TIER_5_NAME,
        6 => KycLevelNames::TIER_6_NAME,
        7 => KycLevelNames::TIER_7_NAME,
        8 => KycLevelNames::TIER_8_NAME,
        _ => "Unknown",
    }
}

/// Get flag name for a specific bit position
pub fn get_flag_name(bit: u8) -> &'static str {
    match bit {
        0 => "Email",
        1 => "Phone",
        2 => "Basic Info",
        3 => "Government ID",
        4 => "Liveness",
        5 => "Address",
        6 => "Source of Funds",
        7 => "Source of Wealth",
        8 => "Background Check",
        9 => "PEP/Sanctions Screening",
        10 => "Ultimate Beneficial Owner",
        11 => "Company Registration",
        12 => "Directors/Shareholders",
        13 => "Compliance Audit",
        14 => "Financial License",
        15 => "Reserved",
        _ => "Unknown",
    }
}

/// Check if a level has a specific flag
#[inline]
pub fn has_flag(level: u16, flag: u16) -> bool {
    (level & flag) == flag
}

/// Check if user level meets required level
/// Uses bitmask comparison: (user_level & required) == required
#[inline]
pub fn meets_requirement(user_level: u16, required_level: u16) -> bool {
    (user_level & required_level) == required_level
}

/// List all flags that are set in a level
pub fn list_flags(level: u16) -> Vec<&'static str> {
    let mut flags = Vec::new();
    for bit in 0..16 {
        if level & (1 << bit) != 0 {
            flags.push(get_flag_name(bit));
        }
    }
    flags
}

/// List missing flags needed to reach a required level
pub fn list_missing_flags(current_level: u16, required_level: u16) -> Vec<&'static str> {
    let missing = required_level & !current_level;
    list_flags(missing)
}

/// Count the number of verification items completed
#[inline]
pub fn count_flags(level: u16) -> u32 {
    level.count_ones()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_values() {
        // Verify cumulative tier values follow 2^n - 1 pattern
        assert_eq!(KycFlags::TIER_0, 0);
        assert_eq!(KycFlags::TIER_1, 7); // 2^3 - 1
        assert_eq!(KycFlags::TIER_2, 31); // 2^5 - 1
        assert_eq!(KycFlags::TIER_3, 63); // 2^6 - 1
        assert_eq!(KycFlags::TIER_4, 255); // 2^8 - 1
        assert_eq!(KycFlags::TIER_5, 2047); // 2^11 - 1
        assert_eq!(KycFlags::TIER_6, 8191); // 2^13 - 1
        assert_eq!(KycFlags::TIER_7, 16383); // 2^14 - 1
        assert_eq!(KycFlags::TIER_8, 32767); // 2^15 - 1
    }

    #[test]
    fn test_tier_cumulative() {
        // Each tier includes all previous flags
        assert!(meets_requirement(KycFlags::TIER_2, KycFlags::TIER_1));
        assert!(meets_requirement(KycFlags::TIER_3, KycFlags::TIER_2));
        assert!(meets_requirement(KycFlags::TIER_4, KycFlags::TIER_3));
        assert!(meets_requirement(KycFlags::TIER_5, KycFlags::TIER_4));
        assert!(meets_requirement(KycFlags::TIER_6, KycFlags::TIER_5));
        assert!(meets_requirement(KycFlags::TIER_7, KycFlags::TIER_6));
        assert!(meets_requirement(KycFlags::TIER_8, KycFlags::TIER_7));

        // Lower tier does not meet higher tier requirement
        assert!(!meets_requirement(KycFlags::TIER_1, KycFlags::TIER_2));
        assert!(!meets_requirement(KycFlags::TIER_2, KycFlags::TIER_3));
    }

    #[test]
    fn test_has_flag() {
        let level = KycFlags::TIER_2; // 31 = Email + Phone + Basic + ID + Liveness

        assert!(has_flag(level, KycFlags::EMAIL));
        assert!(has_flag(level, KycFlags::PHONE));
        assert!(has_flag(level, KycFlags::BASIC_INFO));
        assert!(has_flag(level, KycFlags::GOV_ID));
        assert!(has_flag(level, KycFlags::LIVENESS));

        assert!(!has_flag(level, KycFlags::ADDRESS));
        assert!(!has_flag(level, KycFlags::SOF));
        assert!(!has_flag(level, KycFlags::COMPANY));
    }

    #[test]
    fn test_count_flags() {
        assert_eq!(count_flags(0), 0);
        assert_eq!(count_flags(KycFlags::TIER_1), 3);
        assert_eq!(count_flags(KycFlags::TIER_2), 5);
        assert_eq!(count_flags(KycFlags::TIER_3), 6);
        assert_eq!(count_flags(KycFlags::TIER_4), 8);
        assert_eq!(count_flags(KycFlags::TIER_5), 11);
        assert_eq!(count_flags(KycFlags::TIER_6), 13);
        assert_eq!(count_flags(KycFlags::TIER_7), 14);
        assert_eq!(count_flags(KycFlags::TIER_8), 15);
    }

    #[test]
    fn test_missing_flags() {
        let current = KycFlags::TIER_2; // 31
        let required = KycFlags::TIER_4; // 255

        let missing = list_missing_flags(current, required);
        assert!(missing.contains(&"Address"));
        assert!(missing.contains(&"Source of Funds"));
        assert!(missing.contains(&"Source of Wealth"));
        assert_eq!(missing.len(), 3);
    }

    #[test]
    fn test_list_flags() {
        let flags = list_flags(KycFlags::TIER_1);
        assert_eq!(flags.len(), 3);
        assert!(flags.contains(&"Email"));
        assert!(flags.contains(&"Phone"));
        assert!(flags.contains(&"Basic Info"));
    }
}
