// Referral Adapter: TOS ReferralProvider → TAKO ReferralProvider
//
// This module bridges TOS's async ReferralProvider with TAKO's synchronous
// ReferralProvider trait, enabling smart contracts to access the native
// referral system via syscalls.
//
// Uses `try_block_on()` pattern for async/sync conversion, following the
// established pattern in scheduled_execution.rs and storage providers.

use tos_common::{
    block::TopoHeight, crypto::Hash, crypto::PublicKey, serializer::Serializer, tokio::try_block_on,
};
// TAKO's ReferralProvider trait (aliased to avoid conflict with TOS's ReferralProvider)
use tos_program_runtime::storage::ReferralProvider as TakoReferralProvider;
use tos_tbpf::error::EbpfError;

use crate::core::storage::ReferralProvider;

/// Adapter that wraps TOS's async ReferralProvider to implement TAKO's ReferralProvider
///
/// # Architecture
///
/// ```text
/// TAKO syscall (e.g., tos_get_uplines)
///     ↓
/// InvokeContext::get_uplines()
///     ↓
/// TosReferralAdapter::get_uplines() [TakoReferralProvider]
///     ↓
/// try_block_on(ReferralProvider::get_uplines()) [async → sync]
///     ↓
/// RocksDB storage query
/// ```
///
/// # Thread Safety
///
/// This adapter uses `try_block_on()` which:
/// - Detects multi-thread runtime and uses `block_in_place`
/// - Falls back to `futures::executor::block_on` in single-thread context
/// - Proven pattern used in contract storage providers
pub struct TosReferralAdapter<'a, P: ReferralProvider + Send + Sync + ?Sized> {
    /// TOS referral storage provider (mutable for team volume write operations)
    provider: &'a mut P,
    /// Current topoheight for volume update timestamps
    topoheight: TopoHeight,
}

impl<'a, P: ReferralProvider + Send + Sync + ?Sized> TosReferralAdapter<'a, P> {
    /// Create a new referral adapter
    ///
    /// # Arguments
    ///
    /// * `provider` - TOS referral storage provider implementing ReferralProvider trait
    /// * `topoheight` - Current block topoheight for volume update timestamps
    pub fn new(provider: &'a mut P, topoheight: TopoHeight) -> Self {
        Self {
            provider,
            topoheight,
        }
    }

    /// Convert [u8; 32] bytes to TOS Hash (for asset)
    fn bytes_to_hash(bytes: &[u8; 32]) -> Hash {
        Hash::new(*bytes)
    }

    /// Convert [u8; 32] bytes to TOS PublicKey
    fn bytes_to_pubkey(bytes: &[u8; 32]) -> Result<PublicKey, EbpfError> {
        PublicKey::from_bytes(bytes).map_err(|_| {
            EbpfError::SyscallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid public key bytes",
            )))
        })
    }

    /// Convert TOS PublicKey to [u8; 32] bytes
    fn pubkey_to_bytes(pubkey: &PublicKey) -> [u8; 32] {
        *pubkey.as_bytes()
    }

    /// Convert blockchain error to EbpfError
    fn convert_error<E: std::fmt::Display>(err: E) -> EbpfError {
        EbpfError::SyscallError(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Referral system error: {}", err),
        )))
    }
}

impl<'a, P: ReferralProvider + Send + Sync + ?Sized> TakoReferralProvider
    for TosReferralAdapter<'a, P>
{
    /// Check if a user has already bound a referrer
    fn has_referrer(&self, user: &[u8; 32]) -> Result<bool, EbpfError> {
        let pubkey = Self::bytes_to_pubkey(user)?;

        try_block_on(self.provider.has_referrer(&pubkey))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    /// Get the referrer for a user
    fn get_referrer(&self, user: &[u8; 32]) -> Result<Option<[u8; 32]>, EbpfError> {
        let pubkey = Self::bytes_to_pubkey(user)?;

        let result = try_block_on(self.provider.get_referrer(&pubkey))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        Ok(result.map(|pk| Self::pubkey_to_bytes(&pk)))
    }

    /// Get N levels of uplines for a user
    ///
    /// # Returns
    /// Tuple of (uplines vector, levels_returned count)
    fn get_uplines(&self, user: &[u8; 32], levels: u8) -> Result<(Vec<[u8; 32]>, u8), EbpfError> {
        let pubkey = Self::bytes_to_pubkey(user)?;

        let result = try_block_on(self.provider.get_uplines(&pubkey, levels))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        let uplines: Vec<[u8; 32]> = result.uplines.iter().map(Self::pubkey_to_bytes).collect();

        Ok((uplines, result.levels_returned))
    }

    /// Get the count of direct referrals for a user
    fn get_direct_referrals_count(&self, user: &[u8; 32]) -> Result<u32, EbpfError> {
        let pubkey = Self::bytes_to_pubkey(user)?;

        try_block_on(self.provider.get_direct_referrals_count(&pubkey))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    /// Get the total team size for a user (all descendants)
    fn get_team_size(&self, user: &[u8; 32]) -> Result<u64, EbpfError> {
        let pubkey = Self::bytes_to_pubkey(user)?;

        // Use cached value for performance (syscalls should be fast)
        try_block_on(self.provider.get_team_size(&pubkey, true))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    /// Get the level (depth) of a user in the referral tree
    fn get_level(&self, user: &[u8; 32]) -> Result<u8, EbpfError> {
        let pubkey = Self::bytes_to_pubkey(user)?;

        try_block_on(self.provider.get_level(&pubkey))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    /// Check if descendant is a descendant of ancestor within max_depth levels
    fn is_downline(
        &self,
        ancestor: &[u8; 32],
        descendant: &[u8; 32],
        max_depth: u8,
    ) -> Result<bool, EbpfError> {
        let ancestor_pk = Self::bytes_to_pubkey(ancestor)?;
        let descendant_pk = Self::bytes_to_pubkey(descendant)?;

        try_block_on(
            self.provider
                .is_downline(&ancestor_pk, &descendant_pk, max_depth),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)
    }

    // ===== Team Volume Operations =====

    /// Add volume to user's upline chain
    fn add_team_volume(
        &mut self,
        user: &[u8; 32],
        asset: &[u8; 32],
        amount: u64,
        levels: u8,
    ) -> Result<(), EbpfError> {
        let user_pk = Self::bytes_to_pubkey(user)?;
        let asset_hash = Self::bytes_to_hash(asset);
        let topoheight = self.topoheight;

        try_block_on(self.provider.add_team_volume(
            &user_pk,
            &asset_hash,
            amount,
            levels,
            topoheight,
        ))
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)
    }

    /// Get team volume for a user-asset pair
    fn get_team_volume(&self, user: &[u8; 32], asset: &[u8; 32]) -> Result<u64, EbpfError> {
        let user_pk = Self::bytes_to_pubkey(user)?;
        let asset_hash = Self::bytes_to_hash(asset);

        try_block_on(self.provider.get_team_volume(&user_pk, &asset_hash))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    /// Get direct volume for a user-asset pair
    fn get_direct_volume(&self, user: &[u8; 32], asset: &[u8; 32]) -> Result<u64, EbpfError> {
        let user_pk = Self::bytes_to_pubkey(user)?;
        let asset_hash = Self::bytes_to_hash(asset);

        try_block_on(self.provider.get_direct_volume(&user_pk, &asset_hash))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    /// Get zone volumes (each direct referral's team volume)
    fn get_zone_volumes(
        &self,
        user: &[u8; 32],
        asset: &[u8; 32],
        limit: u8,
    ) -> Result<(Vec<([u8; 32], u64)>, u32), EbpfError> {
        let user_pk = Self::bytes_to_pubkey(user)?;
        let asset_hash = Self::bytes_to_hash(asset);

        let result = try_block_on(self.provider.get_zone_volumes(
            &user_pk,
            &asset_hash,
            limit as u32,
        ))
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)?;

        // Convert Vec<(PublicKey, u64)> to Vec<([u8; 32], u64)>
        let zones: Vec<([u8; 32], u64)> = result
            .zones
            .iter()
            .map(|(pk, vol)| (Self::pubkey_to_bytes(pk), *vol))
            .collect();

        Ok((zones, result.total_count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::error::BlockchainError;
    use async_trait::async_trait;
    use tos_common::crypto::Hash;
    use tos_common::referral::{
        DirectReferralsResult, DistributionResult, ReferralRecord, ReferralRewardRatios,
        TeamVolumeRecord, UplineResult, ZoneVolumesResult,
    };

    /// Mock referral provider for testing
    struct MockReferralProvider {
        has_referrer_result: bool,
        referrer: Option<PublicKey>,
        uplines: Vec<PublicKey>,
        direct_count: u32,
        team_size: u64,
        level: u8,
        is_downline_result: bool,
    }

    impl Default for MockReferralProvider {
        fn default() -> Self {
            Self {
                has_referrer_result: false,
                referrer: None,
                uplines: vec![],
                direct_count: 0,
                team_size: 0,
                level: 0,
                is_downline_result: false,
            }
        }
    }

    #[async_trait]
    impl ReferralProvider for MockReferralProvider {
        async fn has_referrer(&self, _user: &PublicKey) -> Result<bool, BlockchainError> {
            Ok(self.has_referrer_result)
        }

        async fn get_referrer(
            &self,
            _user: &PublicKey,
        ) -> Result<Option<PublicKey>, BlockchainError> {
            Ok(self.referrer.clone())
        }

        async fn bind_referrer(
            &mut self,
            _user: &PublicKey,
            _referrer: &PublicKey,
            _topoheight: tos_common::block::TopoHeight,
            _tx_hash: Hash,
            _timestamp: u64,
        ) -> Result<(), BlockchainError> {
            Ok(())
        }

        async fn get_referral_record(
            &self,
            _user: &PublicKey,
        ) -> Result<Option<ReferralRecord>, BlockchainError> {
            Ok(None)
        }

        async fn get_uplines(
            &self,
            _user: &PublicKey,
            _levels: u8,
        ) -> Result<UplineResult, BlockchainError> {
            Ok(UplineResult::new(self.uplines.clone()))
        }

        async fn get_level(&self, _user: &PublicKey) -> Result<u8, BlockchainError> {
            Ok(self.level)
        }

        async fn is_downline(
            &self,
            _ancestor: &PublicKey,
            _descendant: &PublicKey,
            _max_depth: u8,
        ) -> Result<bool, BlockchainError> {
            Ok(self.is_downline_result)
        }

        async fn get_direct_referrals(
            &self,
            _user: &PublicKey,
            _offset: u32,
            _limit: u32,
        ) -> Result<DirectReferralsResult, BlockchainError> {
            Ok(DirectReferralsResult::new(vec![], 0, 0))
        }

        async fn get_direct_referrals_count(
            &self,
            _user: &PublicKey,
        ) -> Result<u32, BlockchainError> {
            Ok(self.direct_count)
        }

        async fn get_team_size(
            &self,
            _user: &PublicKey,
            _use_cache: bool,
        ) -> Result<u64, BlockchainError> {
            Ok(self.team_size)
        }

        async fn update_team_size_cache(
            &mut self,
            _user: &PublicKey,
            _size: u64,
        ) -> Result<(), BlockchainError> {
            Ok(())
        }

        async fn distribute_to_uplines(
            &mut self,
            _from_user: &PublicKey,
            _asset: Hash,
            _total_amount: u64,
            _ratios: &ReferralRewardRatios,
        ) -> Result<DistributionResult, BlockchainError> {
            Ok(DistributionResult::new(vec![]))
        }

        async fn delete_referral_record(
            &mut self,
            _user: &PublicKey,
        ) -> Result<(), BlockchainError> {
            Ok(())
        }

        async fn add_to_direct_referrals(
            &mut self,
            _referrer: &PublicKey,
            _user: &PublicKey,
        ) -> Result<(), BlockchainError> {
            Ok(())
        }

        async fn remove_from_direct_referrals(
            &mut self,
            _referrer: &PublicKey,
            _user: &PublicKey,
        ) -> Result<(), BlockchainError> {
            Ok(())
        }

        async fn add_team_volume(
            &mut self,
            _user: &PublicKey,
            _asset: &Hash,
            _amount: u64,
            _propagate_levels: u8,
            _topoheight: TopoHeight,
        ) -> Result<(), BlockchainError> {
            Ok(())
        }

        async fn get_team_volume(
            &self,
            _user: &PublicKey,
            _asset: &Hash,
        ) -> Result<u64, BlockchainError> {
            Ok(0)
        }

        async fn get_direct_volume(
            &self,
            _user: &PublicKey,
            _asset: &Hash,
        ) -> Result<u64, BlockchainError> {
            Ok(0)
        }

        async fn get_zone_volumes(
            &self,
            _user: &PublicKey,
            _asset: &Hash,
            _limit: u32,
        ) -> Result<ZoneVolumesResult, BlockchainError> {
            Ok(ZoneVolumesResult::new(vec![], 0))
        }

        async fn get_team_volume_record(
            &self,
            _user: &PublicKey,
            _asset: &Hash,
        ) -> Result<Option<TeamVolumeRecord>, BlockchainError> {
            Ok(None)
        }
    }

    #[test]
    fn test_has_referrer() {
        let mut provider = MockReferralProvider {
            has_referrer_result: true,
            ..Default::default()
        };
        let adapter = TosReferralAdapter::new(&mut provider, 0);

        let user = [1u8; 32];
        let result = adapter.has_referrer(&user);

        assert!(result.is_ok());
        assert!(result.expect("test"));
    }

    #[test]
    fn test_get_referrer_none() {
        let mut provider = MockReferralProvider::default();
        let adapter = TosReferralAdapter::new(&mut provider, 0);

        let user = [1u8; 32];
        let result = adapter.get_referrer(&user);

        assert!(result.is_ok());
        assert!(result.expect("test").is_none());
    }

    #[test]
    fn test_get_referrer_some() {
        let referrer_pk = PublicKey::from_bytes(&[2u8; 32]).expect("test");
        let mut provider = MockReferralProvider {
            referrer: Some(referrer_pk.clone()),
            ..Default::default()
        };
        let adapter = TosReferralAdapter::new(&mut provider, 0);

        let user = [1u8; 32];
        let result = adapter.get_referrer(&user);

        assert!(result.is_ok());
        let referrer = result.expect("test");
        assert!(referrer.is_some());
        assert_eq!(referrer.expect("test"), *referrer_pk.as_bytes());
    }

    #[test]
    fn test_get_uplines() {
        let upline1 = PublicKey::from_bytes(&[2u8; 32]).expect("test");
        let upline2 = PublicKey::from_bytes(&[3u8; 32]).expect("test");
        let mut provider = MockReferralProvider {
            uplines: vec![upline1.clone(), upline2.clone()],
            ..Default::default()
        };
        let adapter = TosReferralAdapter::new(&mut provider, 0);

        let user = [1u8; 32];
        let result = adapter.get_uplines(&user, 3);

        assert!(result.is_ok());
        let (uplines, levels) = result.expect("test");
        assert_eq!(uplines.len(), 2);
        assert_eq!(levels, 2);
        assert_eq!(uplines[0], *upline1.as_bytes());
        assert_eq!(uplines[1], *upline2.as_bytes());
    }

    #[test]
    fn test_get_direct_referrals_count() {
        let mut provider = MockReferralProvider {
            direct_count: 42,
            ..Default::default()
        };
        let adapter = TosReferralAdapter::new(&mut provider, 0);

        let user = [1u8; 32];
        let result = adapter.get_direct_referrals_count(&user);

        assert!(result.is_ok());
        assert_eq!(result.expect("test"), 42);
    }

    #[test]
    fn test_get_team_size() {
        let mut provider = MockReferralProvider {
            team_size: 1000,
            ..Default::default()
        };
        let adapter = TosReferralAdapter::new(&mut provider, 0);

        let user = [1u8; 32];
        let result = adapter.get_team_size(&user);

        assert!(result.is_ok());
        assert_eq!(result.expect("test"), 1000);
    }

    #[test]
    fn test_get_level() {
        let mut provider = MockReferralProvider {
            level: 5,
            ..Default::default()
        };
        let adapter = TosReferralAdapter::new(&mut provider, 0);

        let user = [1u8; 32];
        let result = adapter.get_level(&user);

        assert!(result.is_ok());
        assert_eq!(result.expect("test"), 5);
    }

    #[test]
    fn test_is_downline() {
        let mut provider = MockReferralProvider {
            is_downline_result: true,
            ..Default::default()
        };
        let adapter = TosReferralAdapter::new(&mut provider, 0);

        let ancestor = [1u8; 32];
        let descendant = [2u8; 32];
        let result = adapter.is_downline(&ancestor, &descendant, 10);

        assert!(result.is_ok());
        assert!(result.expect("test"));
    }
}
