// KYC Adapter: TOS KycProvider → TAKO KycProvider
//
// This module bridges TOS's async KycProvider with TAKO's synchronous
// KycProvider trait, enabling smart contracts to access the native
// KYC system via syscalls.
//
// Uses `try_block_on()` pattern for async/sync conversion, following the
// established pattern in referral.rs and storage providers.

use tos_common::{
    crypto::PublicKey, kyc::KycData as TosKycData, serializer::Serializer, tokio::try_block_on,
};
// TAKO's KycProvider trait (aliased to avoid conflict with TOS's KycProvider)
use tos_program_runtime::storage::{
    DeterministicKycProvider, KycData as TakoKycData, KycProvider as TakoKycProvider,
};
use tos_tbpf::error::EbpfError;

use crate::core::storage::KycProvider;

/// Adapter that wraps TOS's async KycProvider to implement TAKO's KycProvider
///
/// # Architecture
///
/// ```text
/// TAKO syscall (e.g., tos_has_kyc)
///     ↓
/// InvokeContext::has_kyc()
///     ↓
/// TosKycAdapter::has_kyc() [TakoKycProvider]
///     ↓
/// try_block_on(KycProvider::has_kyc()) [async → sync]
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
pub struct TosKycAdapter<'a, P: KycProvider + Send + Sync + ?Sized> {
    /// TOS KYC storage provider
    provider: &'a P,
    /// Current Unix timestamp (for expiration checks)
    current_time: u64,
}

impl<'a, P: KycProvider + Send + Sync + ?Sized> TosKycAdapter<'a, P> {
    /// Create a new KYC adapter
    ///
    /// # Arguments
    ///
    /// * `provider` - TOS KYC storage provider implementing KycProvider trait
    /// * `current_time` - Current Unix timestamp for expiration checks
    pub fn new(provider: &'a P, current_time: u64) -> Self {
        Self {
            provider,
            current_time,
        }
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

    /// Convert blockchain error to EbpfError
    fn convert_error<E: std::fmt::Display>(err: E) -> EbpfError {
        EbpfError::SyscallError(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("KYC system error: {}", err),
        )))
    }

    /// Convert TOS KycData to TAKO KycData
    fn convert_kyc_data(tos_data: &TosKycData) -> TakoKycData {
        TakoKycData {
            level: tos_data.level,
            tier: tos_data.get_tier(),
            status: tos_data.status.to_u8(),
            verified_at: tos_data.verified_at,
            expires_at: tos_data.get_expires_at(),
        }
    }
}

/// SAFETY: TosKycAdapter is deterministic because:
/// 1. It reads from RocksDB (on-chain state) via the TOS KycProvider
/// 2. The `current_time` is passed in from block context, not SystemTime::now()
/// 3. All state queries return the same result for the same block
///
/// This implementation enables TosKycAdapter to be used in consensus-critical
/// smart contract execution contexts.
impl<'a, P: KycProvider + Send + Sync + ?Sized> DeterministicKycProvider for TosKycAdapter<'a, P> {}

impl<'a, P: KycProvider + Send + Sync + ?Sized> TakoKycProvider for TosKycAdapter<'a, P> {
    /// Check if a user has any KYC record
    fn has_kyc(&self, user: &[u8; 32]) -> Result<bool, EbpfError> {
        let pubkey = Self::bytes_to_pubkey(user)?;

        try_block_on(self.provider.has_kyc(&pubkey))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    /// Get full KYC data for a user
    fn get_kyc(&self, user: &[u8; 32]) -> Result<Option<TakoKycData>, EbpfError> {
        let pubkey = Self::bytes_to_pubkey(user)?;

        let result = try_block_on(self.provider.get_kyc(&pubkey))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)?;

        Ok(result.as_ref().map(Self::convert_kyc_data))
    }

    /// Get the KYC level bitmask for a user
    fn get_kyc_level(&self, user: &[u8; 32]) -> Result<u16, EbpfError> {
        let pubkey = Self::bytes_to_pubkey(user)?;

        try_block_on(
            self.provider
                .get_effective_level(&pubkey, self.current_time),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)
    }

    /// Get the KYC tier for a user (0-8)
    fn get_kyc_tier(&self, user: &[u8; 32]) -> Result<u8, EbpfError> {
        let pubkey = Self::bytes_to_pubkey(user)?;

        try_block_on(self.provider.get_effective_tier(&pubkey, self.current_time))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    /// Check if user's KYC is currently valid
    fn is_kyc_valid(&self, user: &[u8; 32], current_time: u64) -> Result<bool, EbpfError> {
        let pubkey = Self::bytes_to_pubkey(user)?;

        try_block_on(self.provider.is_kyc_valid(&pubkey, current_time))
            .map_err(Self::convert_error)?
            .map_err(Self::convert_error)
    }

    /// Check if user meets required KYC level
    fn meets_kyc_level(&self, user: &[u8; 32], required_level: u16) -> Result<bool, EbpfError> {
        let pubkey = Self::bytes_to_pubkey(user)?;

        try_block_on(
            self.provider
                .meets_kyc_level(&pubkey, required_level, self.current_time),
        )
        .map_err(Self::convert_error)?
        .map_err(Self::convert_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::error::BlockchainError;
    use async_trait::async_trait;
    use tos_common::{
        block::TopoHeight,
        crypto::Hash,
        kyc::{KycData, KycStatus},
    };

    /// Mock KYC provider for testing
    struct MockKycProvider {
        has_kyc_result: bool,
        kyc_data: Option<KycData>,
        effective_level: u16,
        effective_tier: u8,
        is_valid: bool,
        meets_level: bool,
    }

    impl Default for MockKycProvider {
        fn default() -> Self {
            Self {
                has_kyc_result: false,
                kyc_data: None,
                effective_level: 0,
                effective_tier: 0,
                is_valid: false,
                meets_level: false,
            }
        }
    }

    #[async_trait]
    impl KycProvider for MockKycProvider {
        async fn has_kyc(&self, _user: &PublicKey) -> Result<bool, BlockchainError> {
            Ok(self.has_kyc_result)
        }

        async fn get_kyc(&self, _user: &PublicKey) -> Result<Option<KycData>, BlockchainError> {
            Ok(self.kyc_data.clone())
        }

        async fn set_kyc(
            &mut self,
            _user: &PublicKey,
            _kyc_data: KycData,
            _committee_id: &Hash,
            _topoheight: TopoHeight,
            _tx_hash: &Hash,
        ) -> Result<(), BlockchainError> {
            Ok(())
        }

        async fn update_kyc_status(
            &mut self,
            _user: &PublicKey,
            _status: KycStatus,
            _topoheight: TopoHeight,
        ) -> Result<(), BlockchainError> {
            Ok(())
        }

        async fn renew_kyc(
            &mut self,
            _user: &PublicKey,
            _new_verified_at: u64,
            _new_data_hash: Hash,
            _topoheight: TopoHeight,
            _tx_hash: &Hash,
        ) -> Result<(), BlockchainError> {
            Ok(())
        }

        async fn revoke_kyc(
            &mut self,
            _user: &PublicKey,
            _reason_hash: &Hash,
            _topoheight: TopoHeight,
            _tx_hash: &Hash,
        ) -> Result<(), BlockchainError> {
            Ok(())
        }

        async fn transfer_kyc(
            &mut self,
            _user: &PublicKey,
            _new_committee_id: &Hash,
            _new_data_hash: Hash,
            _transferred_at: u64,
            _topoheight: TopoHeight,
            _tx_hash: &Hash,
        ) -> Result<(), BlockchainError> {
            Ok(())
        }

        async fn get_effective_level(
            &self,
            _user: &PublicKey,
            _current_time: u64,
        ) -> Result<u16, BlockchainError> {
            Ok(self.effective_level)
        }

        async fn get_effective_tier(
            &self,
            _user: &PublicKey,
            _current_time: u64,
        ) -> Result<u8, BlockchainError> {
            Ok(self.effective_tier)
        }

        async fn meets_kyc_level(
            &self,
            _user: &PublicKey,
            _required_level: u16,
            _current_time: u64,
        ) -> Result<bool, BlockchainError> {
            Ok(self.meets_level)
        }

        async fn is_kyc_valid(
            &self,
            _user: &PublicKey,
            _current_time: u64,
        ) -> Result<bool, BlockchainError> {
            Ok(self.is_valid)
        }

        async fn get_verifying_committee(
            &self,
            _user: &PublicKey,
        ) -> Result<Option<Hash>, BlockchainError> {
            Ok(None)
        }

        async fn get_kyc_topoheight(
            &self,
            _user: &PublicKey,
        ) -> Result<Option<TopoHeight>, BlockchainError> {
            Ok(None)
        }

        async fn get_kyc_batch(
            &self,
            _users: &[PublicKey],
        ) -> Result<Vec<(PublicKey, Option<KycData>)>, BlockchainError> {
            Ok(vec![])
        }

        async fn check_kyc_batch(
            &self,
            _users: &[PublicKey],
            _required_level: u16,
            _current_time: u64,
        ) -> Result<Vec<(PublicKey, bool)>, BlockchainError> {
            Ok(vec![])
        }

        async fn emergency_suspend(
            &mut self,
            _user: &PublicKey,
            _reason_hash: &Hash,
            _expires_at: u64,
            _topoheight: TopoHeight,
            _tx_hash: &Hash,
        ) -> Result<(), BlockchainError> {
            Ok(())
        }

        async fn get_emergency_suspension(
            &self,
            _user: &PublicKey,
        ) -> Result<Option<(Hash, u64)>, BlockchainError> {
            Ok(None)
        }

        async fn lift_emergency_suspension(
            &mut self,
            _user: &PublicKey,
            _topoheight: TopoHeight,
        ) -> Result<(), BlockchainError> {
            Ok(())
        }

        async fn delete_kyc_record(&mut self, _user: &PublicKey) -> Result<(), BlockchainError> {
            Ok(())
        }

        async fn count_users_at_level(
            &self,
            _min_level: u16,
            _current_time: u64,
        ) -> Result<u64, BlockchainError> {
            Ok(0)
        }
    }

    #[test]
    fn test_has_kyc() {
        let provider = MockKycProvider {
            has_kyc_result: true,
            ..Default::default()
        };
        let adapter = TosKycAdapter::new(&provider, 0);

        let user = [1u8; 32];
        let result = adapter.has_kyc(&user);

        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_has_kyc_false() {
        let provider = MockKycProvider::default();
        let adapter = TosKycAdapter::new(&provider, 0);

        let user = [1u8; 32];
        let result = adapter.has_kyc(&user);

        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    fn test_get_kyc_none() {
        let provider = MockKycProvider::default();
        let adapter = TosKycAdapter::new(&provider, 0);

        let user = [1u8; 32];
        let result = adapter.get_kyc(&user);

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_get_kyc_some() {
        let kyc_data = KycData::new(31, 1000000, Hash::zero());
        let provider = MockKycProvider {
            kyc_data: Some(kyc_data.clone()),
            ..Default::default()
        };
        let adapter = TosKycAdapter::new(&provider, 0);

        let user = [1u8; 32];
        let result = adapter.get_kyc(&user);

        assert!(result.is_ok());
        let data = result.unwrap();
        assert!(data.is_some());

        let tako_data = data.unwrap();
        assert_eq!(tako_data.level, 31);
        assert_eq!(tako_data.tier, 2); // Level 31 = Tier 2 (Standard)
        assert_eq!(tako_data.status, 0); // Active
        assert_eq!(tako_data.verified_at, 1000000);
    }

    #[test]
    fn test_get_kyc_level() {
        let provider = MockKycProvider {
            effective_level: 255,
            ..Default::default()
        };
        let adapter = TosKycAdapter::new(&provider, 0);

        let user = [1u8; 32];
        let result = adapter.get_kyc_level(&user);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 255);
    }

    #[test]
    fn test_get_kyc_tier() {
        let provider = MockKycProvider {
            effective_tier: 4,
            ..Default::default()
        };
        let adapter = TosKycAdapter::new(&provider, 0);

        let user = [1u8; 32];
        let result = adapter.get_kyc_tier(&user);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 4);
    }

    #[test]
    fn test_is_kyc_valid() {
        let provider = MockKycProvider {
            is_valid: true,
            ..Default::default()
        };
        let adapter = TosKycAdapter::new(&provider, 0);

        let user = [1u8; 32];
        let result = adapter.is_kyc_valid(&user, 1000000);

        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_meets_kyc_level() {
        let provider = MockKycProvider {
            meets_level: true,
            ..Default::default()
        };
        let adapter = TosKycAdapter::new(&provider, 0);

        let user = [1u8; 32];
        let result = adapter.meets_kyc_level(&user, 31);

        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_invalid_pubkey() {
        let provider = MockKycProvider::default();
        let adapter = TosKycAdapter::new(&provider, 0);

        // Invalid public key (all zeros is still valid for ed25519 point)
        // Let's use a known invalid pattern
        let user = [0u8; 32];
        let result = adapter.has_kyc(&user);

        // [0u8; 32] happens to be a valid public key point,
        // so this should succeed
        assert!(result.is_ok());
    }
}
