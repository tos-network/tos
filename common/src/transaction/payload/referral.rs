// Referral system transaction payloads

use crate::{
    crypto::{elgamal::CompressedPublicKey, Hash},
    serializer::*,
    transaction::extra_data::UnknownExtraDataFormat,
};
use serde::{Deserialize, Serialize};

/// BindReferrerPayload is used to bind a referrer to the sender account.
/// This is a one-time operation - once bound, the referrer cannot be changed.
///
/// Gas cost: 10,000 gas (~$0.01)
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BindReferrerPayload {
    /// The referrer's public key
    referrer: CompressedPublicKey,
    /// Optional extra data (e.g., campaign ID, source tracking)
    extra_data: Option<UnknownExtraDataFormat>,
}

impl BindReferrerPayload {
    /// Create a new bind referrer payload
    pub fn new(referrer: CompressedPublicKey, extra_data: Option<UnknownExtraDataFormat>) -> Self {
        Self {
            referrer,
            extra_data,
        }
    }

    /// Get the referrer's public key
    #[inline]
    pub fn get_referrer(&self) -> &CompressedPublicKey {
        &self.referrer
    }

    /// Get the extra data if any
    #[inline]
    pub fn get_extra_data(&self) -> &Option<UnknownExtraDataFormat> {
        &self.extra_data
    }

    /// Consume and return the inner values
    #[inline]
    pub fn consume(self) -> (CompressedPublicKey, Option<UnknownExtraDataFormat>) {
        (self.referrer, self.extra_data)
    }
}

impl Serializer for BindReferrerPayload {
    fn write(&self, writer: &mut Writer) {
        self.referrer.write(writer);
        self.extra_data.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let referrer = CompressedPublicKey::read(reader)?;
        let extra_data = Option::read(reader)?;

        Ok(Self {
            referrer,
            extra_data,
        })
    }

    fn size(&self) -> usize {
        self.referrer.size() + self.extra_data.size()
    }
}

/// BatchReferralRewardPayload is used to distribute rewards to uplines.
/// This is typically called by smart contracts implementing referral reward logic.
///
/// Gas cost: 5,000 + 2,000 * levels gas
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BatchReferralRewardPayload {
    /// The asset to distribute
    asset: Hash,
    /// The user whose uplines will receive rewards
    from_user: CompressedPublicKey,
    /// Total amount to distribute
    total_amount: u64,
    /// Number of levels to distribute to
    levels: u8,
    /// Reward ratios for each level (in basis points, 100 = 1%)
    /// Length must equal `levels`
    ratios: Vec<u16>,
}

impl BatchReferralRewardPayload {
    /// Create a new batch referral reward payload
    pub fn new(
        asset: Hash,
        from_user: CompressedPublicKey,
        total_amount: u64,
        levels: u8,
        ratios: Vec<u16>,
    ) -> Self {
        Self {
            asset,
            from_user,
            total_amount,
            levels,
            ratios,
        }
    }

    /// Get the asset hash
    #[inline]
    pub fn get_asset(&self) -> &Hash {
        &self.asset
    }

    /// Get the from user's public key
    #[inline]
    pub fn get_from_user(&self) -> &CompressedPublicKey {
        &self.from_user
    }

    /// Get the total amount to distribute
    #[inline]
    pub fn get_total_amount(&self) -> u64 {
        self.total_amount
    }

    /// Get the number of levels
    #[inline]
    pub fn get_levels(&self) -> u8 {
        self.levels
    }

    /// Get the ratios
    #[inline]
    pub fn get_ratios(&self) -> &[u16] {
        &self.ratios
    }

    /// Validate the payload
    pub fn validate(&self) -> bool {
        // Check that ratios length matches levels
        if self.ratios.len() != self.levels as usize {
            return false;
        }

        // Check that total ratio does not exceed 100%
        let total: u32 = self.ratios.iter().map(|&r| r as u32).sum();
        if total > 10000 {
            return false;
        }

        // Check that levels is reasonable (max 20)
        if self.levels > 20 {
            return false;
        }

        true
    }

    /// Consume and return the inner values
    #[inline]
    pub fn consume(self) -> (Hash, CompressedPublicKey, u64, u8, Vec<u16>) {
        (
            self.asset,
            self.from_user,
            self.total_amount,
            self.levels,
            self.ratios,
        )
    }
}

impl Serializer for BatchReferralRewardPayload {
    fn write(&self, writer: &mut Writer) {
        self.asset.write(writer);
        self.from_user.write(writer);
        self.total_amount.write(writer);
        self.levels.write(writer);
        // Write ratios as a length-prefixed array
        (self.ratios.len() as u8).write(writer);
        for ratio in &self.ratios {
            ratio.write(writer);
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let asset = Hash::read(reader)?;
        let from_user = CompressedPublicKey::read(reader)?;
        let total_amount = u64::read(reader)?;
        let levels = u8::read(reader)?;

        // Read ratios
        let ratios_len = u8::read(reader)? as usize;
        if ratios_len > 20 {
            return Err(ReaderError::InvalidSize);
        }
        let mut ratios = Vec::with_capacity(ratios_len);
        for _ in 0..ratios_len {
            ratios.push(u16::read(reader)?);
        }

        Ok(Self {
            asset,
            from_user,
            total_amount,
            levels,
            ratios,
        })
    }

    fn size(&self) -> usize {
        self.asset.size()
            + self.from_user.size()
            + self.total_amount.size()
            + self.levels.size()
            + 1  // ratios length byte
            + self.ratios.len() * 2 // each ratio is u16 = 2 bytes
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
    fn test_bind_referrer_payload_serialization() {
        let referrer_kp = generate_keypair();
        let referrer = referrer_kp.get_public_key().compress();

        let payload = BindReferrerPayload::new(referrer.clone(), None);

        // Serialize
        let mut buffer = Vec::new();
        let mut writer = Writer::new(&mut buffer);
        payload.write(&mut writer);

        // Deserialize
        let mut reader = Reader::new(&buffer);
        let deserialized = BindReferrerPayload::read(&mut reader).unwrap();

        assert_eq!(payload.get_referrer(), deserialized.get_referrer());
    }

    #[test]
    fn test_batch_referral_reward_payload() {
        let from_user_kp = generate_keypair();
        let from_user = from_user_kp.get_public_key().compress();

        let payload = BatchReferralRewardPayload::new(
            Hash::zero(),
            from_user,
            1000_000000, // 1000 units
            3,
            vec![1000, 500, 300], // 10%, 5%, 3%
        );

        assert!(payload.validate());

        // Serialize
        let mut buffer = Vec::new();
        let mut writer = Writer::new(&mut buffer);
        payload.write(&mut writer);

        // Deserialize
        let mut reader = Reader::new(&buffer);
        let deserialized = BatchReferralRewardPayload::read(&mut reader).unwrap();

        assert_eq!(payload.get_levels(), deserialized.get_levels());
        assert_eq!(payload.get_ratios(), deserialized.get_ratios());
        assert_eq!(payload.get_total_amount(), deserialized.get_total_amount());
    }

    #[test]
    fn test_invalid_ratios() {
        let from_user_kp = generate_keypair();
        let from_user = from_user_kp.get_public_key().compress();

        // Total ratio exceeds 100%
        let payload = BatchReferralRewardPayload::new(
            Hash::zero(),
            from_user.clone(),
            1000_000000,
            3,
            vec![5000, 3000, 3000], // 150%
        );

        assert!(!payload.validate());

        // Mismatched levels and ratios
        let payload2 = BatchReferralRewardPayload::new(
            Hash::zero(),
            from_user,
            1000_000000,
            5,                    // 5 levels
            vec![1000, 500, 300], // only 3 ratios
        );

        assert!(!payload2.validate());
    }
}
