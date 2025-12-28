// KYC system storage provider trait
//
// This provider manages user KYC data storage and queries.
// Reference: TOS-KYC-Level-Design.md

use crate::core::error::BlockchainError;
use async_trait::async_trait;
use tos_common::{
    block::TopoHeight,
    crypto::{Hash, PublicKey},
    kyc::{KycData, KycStatus},
};

/// Storage provider for user KYC data
#[async_trait]
pub trait KycProvider {
    // ===== Basic KYC Operations =====

    /// Check if a user has any KYC data
    async fn has_kyc(&self, user: &PublicKey) -> Result<bool, BlockchainError>;

    /// Get KYC data for a user
    /// Returns None if user has no KYC record
    async fn get_kyc(&self, user: &PublicKey) -> Result<Option<KycData>, BlockchainError>;

    /// Set KYC data for a user
    ///
    /// # Arguments
    /// * `user` - The user's public key
    /// * `kyc_data` - The KYC data to store (43 bytes)
    /// * `committee_id` - The committee that verified this KYC
    /// * `topoheight` - The block height when KYC was set
    /// * `tx_hash` - The transaction hash
    ///
    /// # Errors
    /// * `KycAlreadySet` - User already has higher or equal KYC level
    async fn set_kyc(
        &mut self,
        user: &PublicKey,
        kyc_data: KycData,
        committee_id: &Hash,
        topoheight: TopoHeight,
        tx_hash: &Hash,
    ) -> Result<(), BlockchainError>;

    /// Update KYC status for a user
    ///
    /// # Arguments
    /// * `user` - The user's public key
    /// * `status` - The new status
    /// * `topoheight` - The block height when status changed
    async fn update_kyc_status(
        &mut self,
        user: &PublicKey,
        status: KycStatus,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    /// Renew KYC for a user (update verified_at timestamp and data_hash)
    ///
    /// # Arguments
    /// * `user` - The user's public key
    /// * `new_verified_at` - The new verification timestamp
    /// * `new_data_hash` - The new off-chain data hash
    /// * `topoheight` - The block height when renewed
    /// * `tx_hash` - The transaction hash
    async fn renew_kyc(
        &mut self,
        user: &PublicKey,
        new_verified_at: u64,
        new_data_hash: Hash,
        topoheight: TopoHeight,
        tx_hash: &Hash,
    ) -> Result<(), BlockchainError>;

    /// Revoke KYC for a user
    ///
    /// # Arguments
    /// * `user` - The user's public key
    /// * `reason_hash` - Hash of revocation reason (stored off-chain)
    /// * `topoheight` - The block height when revoked
    /// * `tx_hash` - The transaction hash
    async fn revoke_kyc(
        &mut self,
        user: &PublicKey,
        reason_hash: &Hash,
        topoheight: TopoHeight,
        tx_hash: &Hash,
    ) -> Result<(), BlockchainError>;

    // ===== KYC Level Queries =====

    /// Get the effective KYC level for a user at current time
    /// Returns 0 if no KYC, KYC expired, or KYC revoked/suspended
    async fn get_effective_level(
        &self,
        user: &PublicKey,
        current_time: u64,
    ) -> Result<u16, BlockchainError>;

    /// Get the effective KYC tier for a user at current time
    /// Returns 0 if no KYC, KYC expired, or KYC revoked/suspended
    async fn get_effective_tier(
        &self,
        user: &PublicKey,
        current_time: u64,
    ) -> Result<u8, BlockchainError>;

    /// Check if user meets a required KYC level
    async fn meets_kyc_level(
        &self,
        user: &PublicKey,
        required_level: u16,
        current_time: u64,
    ) -> Result<bool, BlockchainError>;

    /// Check if user's KYC is valid (Active status and not expired)
    async fn is_kyc_valid(
        &self,
        user: &PublicKey,
        current_time: u64,
    ) -> Result<bool, BlockchainError>;

    // ===== KYC History =====

    /// Get the committee ID that verified a user's KYC
    async fn get_verifying_committee(
        &self,
        user: &PublicKey,
    ) -> Result<Option<Hash>, BlockchainError>;

    /// Get the topoheight when KYC was last updated
    async fn get_kyc_topoheight(
        &self,
        user: &PublicKey,
    ) -> Result<Option<TopoHeight>, BlockchainError>;

    // ===== Batch Operations =====

    /// Get KYC data for multiple users
    async fn get_kyc_batch(
        &self,
        users: &[PublicKey],
    ) -> Result<Vec<(PublicKey, Option<KycData>)>, BlockchainError>;

    /// Check KYC validity for multiple users
    async fn check_kyc_batch(
        &self,
        users: &[PublicKey],
        required_level: u16,
        current_time: u64,
    ) -> Result<Vec<(PublicKey, bool)>, BlockchainError>;

    // ===== Emergency Operations =====

    /// Emergency suspend a user's KYC
    ///
    /// # Arguments
    /// * `user` - The user's public key
    /// * `reason_hash` - Hash of suspension reason
    /// * `expires_at` - When the emergency suspension expires (24h from now)
    /// * `topoheight` - The block height when suspended
    /// * `tx_hash` - The transaction hash
    async fn emergency_suspend(
        &mut self,
        user: &PublicKey,
        reason_hash: &Hash,
        expires_at: u64,
        topoheight: TopoHeight,
        tx_hash: &Hash,
    ) -> Result<(), BlockchainError>;

    /// Get emergency suspension info for a user
    /// Returns (reason_hash, expires_at) if suspended, None otherwise
    async fn get_emergency_suspension(
        &self,
        user: &PublicKey,
    ) -> Result<Option<(Hash, u64)>, BlockchainError>;

    /// Lift emergency suspension (called automatically or by committee)
    async fn lift_emergency_suspension(
        &mut self,
        user: &PublicKey,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    // ===== Administrative Operations =====

    /// Delete KYC record (for rollback scenarios)
    /// Only used internally during chain reorganization
    async fn delete_kyc_record(&mut self, user: &PublicKey) -> Result<(), BlockchainError>;

    /// Get count of users with valid KYC at a specific level or higher
    async fn count_users_at_level(
        &self,
        min_level: u16,
        current_time: u64,
    ) -> Result<u64, BlockchainError>;
}
