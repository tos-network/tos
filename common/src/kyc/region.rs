// KYC Region enumeration
// Represents geographic regions for committee jurisdiction
//
// Design: Privacy-preserving - only region stored, not specific country
// Country data is stored off-chain by regional committees

use crate::serializer::{Reader, ReaderError, Serializer, Writer};
use serde::{Deserialize, Serialize};

/// Geographic region enumeration (privacy-preserving)
/// Used for committee jurisdiction assignment
/// Note: Country data is stored off-chain only
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum KycRegion {
    /// Region not specified
    #[default]
    Unspecified = 0,

    /// Asia Pacific
    /// Countries: CN, JP, KR, SG, IN, TH, VN, MY, PH, ID, etc.
    /// Note: AU is in Oceania
    AsiaPacific = 1,

    /// Europe
    /// Countries: EU members, UK, CH, NO, etc.
    Europe = 2,

    /// North America
    /// Countries: US, CA, MX
    NorthAmerica = 3,

    /// Latin America
    /// Countries: BR, AR, CL, CO, PE, VE, etc.
    LatinAmerica = 4,

    /// Middle East
    /// Countries: AE, SA, EG, IL, TR, etc.
    MiddleEast = 5,

    /// Africa
    /// Countries: NG, ZA, KE, GH, etc.
    Africa = 6,

    /// Oceania
    /// Countries: AU, NZ, Pacific Islands
    Oceania = 7,

    /// Global (multi-region entities)
    /// Used for Global Committee jurisdiction
    Global = 255,
}

impl KycRegion {
    /// Get human-readable region name
    pub fn as_str(&self) -> &'static str {
        match self {
            KycRegion::Unspecified => "Unspecified",
            KycRegion::AsiaPacific => "Asia Pacific",
            KycRegion::Europe => "Europe",
            KycRegion::NorthAmerica => "North America",
            KycRegion::LatinAmerica => "Latin America",
            KycRegion::MiddleEast => "Middle East",
            KycRegion::Africa => "Africa",
            KycRegion::Oceania => "Oceania",
            KycRegion::Global => "Global",
        }
    }

    /// Get short region code
    pub fn code(&self) -> &'static str {
        match self {
            KycRegion::Unspecified => "UNS",
            KycRegion::AsiaPacific => "APAC",
            KycRegion::Europe => "EU",
            KycRegion::NorthAmerica => "NA",
            KycRegion::LatinAmerica => "LATAM",
            KycRegion::MiddleEast => "MENA",
            KycRegion::Africa => "AF",
            KycRegion::Oceania => "OC",
            KycRegion::Global => "GLOBAL",
        }
    }

    /// Convert from u8 for deserialization
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(KycRegion::Unspecified),
            1 => Some(KycRegion::AsiaPacific),
            2 => Some(KycRegion::Europe),
            3 => Some(KycRegion::NorthAmerica),
            4 => Some(KycRegion::LatinAmerica),
            5 => Some(KycRegion::MiddleEast),
            6 => Some(KycRegion::Africa),
            7 => Some(KycRegion::Oceania),
            255 => Some(KycRegion::Global),
            _ => None,
        }
    }

    /// Convert to u8 for serialization
    #[inline]
    pub fn to_u8(self) -> u8 {
        self as u8
    }

    /// Check if this is the Global region
    #[inline]
    pub fn is_global(&self) -> bool {
        matches!(self, KycRegion::Global)
    }

    /// Check if this region can be managed by a parent region
    /// Global region can manage all other regions
    pub fn can_be_managed_by(&self, parent: &KycRegion) -> bool {
        if parent.is_global() {
            return true;
        }
        // Non-global regions can only manage themselves
        self == parent
    }

    /// Get all valid regional committee regions (excludes Unspecified and Global)
    pub fn regional_values() -> &'static [KycRegion] {
        &[
            KycRegion::AsiaPacific,
            KycRegion::Europe,
            KycRegion::NorthAmerica,
            KycRegion::LatinAmerica,
            KycRegion::MiddleEast,
            KycRegion::Africa,
            KycRegion::Oceania,
        ]
    }
}

impl std::fmt::Display for KycRegion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl Serializer for KycRegion {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let value = u8::read(reader)?;
        KycRegion::from_u8(value).ok_or(ReaderError::InvalidValue)
    }

    fn write(&self, writer: &mut Writer) {
        self.to_u8().write(writer);
    }

    fn size(&self) -> usize {
        1
    }
}

/// Map ISO 3166-1 alpha-2 country code to KycRegion
/// This is used off-chain for region assignment
pub fn country_to_region(country_code: &str) -> KycRegion {
    match country_code.to_uppercase().as_str() {
        // Asia Pacific
        "CN" | "JP" | "KR" | "SG" | "HK" | "TW" | "IN" | "TH" | "VN" | "MY" | "PH" | "ID"
        | "BD" | "PK" | "LK" | "NP" | "MM" | "KH" | "LA" | "BN" | "MN" | "KP" | "MO" => {
            KycRegion::AsiaPacific
        }

        // Europe
        "DE" | "FR" | "GB" | "IT" | "ES" | "NL" | "BE" | "AT" | "CH" | "SE" | "NO" | "DK"
        | "FI" | "IE" | "PT" | "GR" | "PL" | "CZ" | "RO" | "HU" | "SK" | "BG" | "HR" | "SI"
        | "EE" | "LV" | "LT" | "LU" | "MT" | "CY" | "IS" | "UA" | "BY" | "RS" | "BA" | "MK"
        | "AL" | "ME" | "MD" | "XK" => KycRegion::Europe,

        // North America
        "US" | "CA" | "MX" => KycRegion::NorthAmerica,

        // Latin America
        "BR" | "AR" | "CL" | "CO" | "PE" | "VE" | "EC" | "BO" | "PY" | "UY" | "GY" | "SR"
        | "PA" | "CR" | "GT" | "HN" | "SV" | "NI" | "CU" | "DO" | "HT" | "JM" | "TT" | "BB"
        | "BS" => KycRegion::LatinAmerica,

        // Middle East (Note: EG is in Africa, CY is in Europe)
        "AE" | "SA" | "IL" | "TR" | "IR" | "IQ" | "KW" | "QA" | "BH" | "OM" | "JO" | "LB"
        | "SY" | "YE" | "PS" => KycRegion::MiddleEast,

        // Africa
        "NG" | "ZA" | "KE" | "GH" | "ET" | "TZ" | "UG" | "DZ" | "MA" | "TN" | "EG" | "LY"
        | "SD" | "SN" | "CI" | "CM" | "AO" | "MZ" | "MG" | "ZW" | "ZM" | "RW" | "MW" | "NA"
        | "BW" | "MU" | "SC" => KycRegion::Africa,

        // Oceania
        "AU" | "NZ" | "FJ" | "PG" | "SB" | "VU" | "NC" | "PF" | "GU" | "TO" | "WS" | "FM"
        | "PW" | "MH" | "KI" | "NR" | "TV" => KycRegion::Oceania,

        // Unknown defaults to Unspecified
        _ => KycRegion::Unspecified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_region_values() {
        assert_eq!(KycRegion::Unspecified.to_u8(), 0);
        assert_eq!(KycRegion::AsiaPacific.to_u8(), 1);
        assert_eq!(KycRegion::Europe.to_u8(), 2);
        assert_eq!(KycRegion::NorthAmerica.to_u8(), 3);
        assert_eq!(KycRegion::LatinAmerica.to_u8(), 4);
        assert_eq!(KycRegion::MiddleEast.to_u8(), 5);
        assert_eq!(KycRegion::Africa.to_u8(), 6);
        assert_eq!(KycRegion::Oceania.to_u8(), 7);
        assert_eq!(KycRegion::Global.to_u8(), 255);
    }

    #[test]
    fn test_u8_conversion() {
        for region in KycRegion::regional_values() {
            let value = region.to_u8();
            let restored = KycRegion::from_u8(value);
            assert_eq!(restored, Some(*region));
        }

        assert_eq!(KycRegion::from_u8(255), Some(KycRegion::Global));
        assert_eq!(KycRegion::from_u8(100), None);
    }

    #[test]
    fn test_can_be_managed_by() {
        // Global can manage all
        for region in KycRegion::regional_values() {
            assert!(region.can_be_managed_by(&KycRegion::Global));
        }

        // Regions can only manage themselves
        assert!(KycRegion::AsiaPacific.can_be_managed_by(&KycRegion::AsiaPacific));
        assert!(!KycRegion::AsiaPacific.can_be_managed_by(&KycRegion::Europe));
    }

    #[test]
    fn test_country_to_region() {
        // Asia Pacific
        assert_eq!(country_to_region("JP"), KycRegion::AsiaPacific);
        assert_eq!(country_to_region("CN"), KycRegion::AsiaPacific);
        assert_eq!(country_to_region("SG"), KycRegion::AsiaPacific);

        // Europe
        assert_eq!(country_to_region("DE"), KycRegion::Europe);
        assert_eq!(country_to_region("GB"), KycRegion::Europe);
        assert_eq!(country_to_region("FR"), KycRegion::Europe);

        // North America
        assert_eq!(country_to_region("US"), KycRegion::NorthAmerica);
        assert_eq!(country_to_region("CA"), KycRegion::NorthAmerica);

        // Oceania (AU is NOT in Asia Pacific)
        assert_eq!(country_to_region("AU"), KycRegion::Oceania);
        assert_eq!(country_to_region("NZ"), KycRegion::Oceania);

        // Unknown
        assert_eq!(country_to_region("XX"), KycRegion::Unspecified);
    }

    #[test]
    fn test_display() {
        assert_eq!(KycRegion::AsiaPacific.to_string(), "Asia Pacific");
        assert_eq!(KycRegion::Europe.to_string(), "Europe");
        assert_eq!(KycRegion::Global.to_string(), "Global");
    }

    #[test]
    fn test_codes() {
        assert_eq!(KycRegion::AsiaPacific.code(), "APAC");
        assert_eq!(KycRegion::Europe.code(), "EU");
        assert_eq!(KycRegion::NorthAmerica.code(), "NA");
        assert_eq!(KycRegion::Global.code(), "GLOBAL");
    }
}
