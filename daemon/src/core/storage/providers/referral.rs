// Referral system storage provider trait

use crate::core::error::BlockchainError;
use async_trait::async_trait;
use tos_common::{
    block::TopoHeight,
    crypto::{Hash, PublicKey},
    referral::{
        DirectReferralsResult, DistributionResult, ReferralRecord, ReferralRewardRatios,
        TeamVolumeRecord, UplineResult, ZoneVolumesResult,
    },
};

/// Storage provider for referral relationships
#[async_trait]
pub trait ReferralProvider {
    // ===== Basic Referrer Operations =====

    /// Check if a user has already bound a referrer
    async fn has_referrer(&self, user: &PublicKey) -> Result<bool, BlockchainError>;

    /// Get the referrer for a user
    /// Returns None if user has no referrer (top-level or not registered)
    async fn get_referrer(&self, user: &PublicKey) -> Result<Option<PublicKey>, BlockchainError>;

    /// Bind a referrer to a user
    /// This operation is one-time only - once bound, cannot be changed
    ///
    /// # Arguments
    /// * `user` - The user binding the referrer
    /// * `referrer` - The referrer's public key
    /// * `topoheight` - The block height when binding occurs
    /// * `tx_hash` - The transaction hash
    /// * `timestamp` - Unix timestamp in seconds
    ///
    /// # Errors
    /// * `AlreadyBound` - User already has a referrer
    /// * `SelfReferral` - Cannot set self as referrer
    /// * `CircularReference` - Would create a circular reference chain
    async fn bind_referrer(
        &mut self,
        user: &PublicKey,
        referrer: &PublicKey,
        topoheight: TopoHeight,
        tx_hash: tos_common::crypto::Hash,
        timestamp: u64,
    ) -> Result<(), BlockchainError>;

    // ===== Referral Record Access =====

    /// Get the full referral record for a user
    async fn get_referral_record(
        &self,
        user: &PublicKey,
    ) -> Result<Option<ReferralRecord>, BlockchainError>;

    // ===== Upline Queries =====

    /// Get N levels of uplines for a user
    ///
    /// # Arguments
    /// * `user` - The user to query uplines for
    /// * `levels` - Number of levels to return (max 20)
    ///
    /// # Returns
    /// Vector of upline public keys, ordered from immediate referrer to higher levels.
    /// May return fewer than `levels` if the chain is shorter.
    async fn get_uplines(
        &self,
        user: &PublicKey,
        levels: u8,
    ) -> Result<UplineResult, BlockchainError>;

    /// Get the level (depth) of a user in the referral tree
    /// Returns 0 if user has no referrer (top-level)
    async fn get_level(&self, user: &PublicKey) -> Result<u8, BlockchainError>;

    /// Check if `descendant` is a descendant of `ancestor` within `max_depth` levels
    async fn is_downline(
        &self,
        ancestor: &PublicKey,
        descendant: &PublicKey,
        max_depth: u8,
    ) -> Result<bool, BlockchainError>;

    // ===== Direct Referrals (Downline) Queries =====

    /// Get direct referrals (users who have this user as their referrer)
    ///
    /// # Arguments
    /// * `user` - The referrer to query
    /// * `offset` - Pagination offset
    /// * `limit` - Maximum number of results to return (max 1000)
    async fn get_direct_referrals(
        &self,
        user: &PublicKey,
        offset: u32,
        limit: u32,
    ) -> Result<DirectReferralsResult, BlockchainError>;

    /// Get the count of direct referrals
    async fn get_direct_referrals_count(&self, user: &PublicKey) -> Result<u32, BlockchainError>;

    // ===== Team Statistics =====

    /// Get the total team size (all descendants in the referral tree)
    ///
    /// # Arguments
    /// * `user` - The user to query
    /// * `use_cache` - If true, return cached value; if false, recalculate
    async fn get_team_size(
        &self,
        user: &PublicKey,
        use_cache: bool,
    ) -> Result<u64, BlockchainError>;

    /// Update the cached team size for a user
    async fn update_team_size_cache(
        &mut self,
        user: &PublicKey,
        size: u64,
    ) -> Result<(), BlockchainError>;

    // ===== Team Volume Operations =====

    /// Add volume to user's upline chain
    ///
    /// This propagates the volume up the referral tree:
    /// - Level 1 (immediate referrer): direct_volume += amount, team_volume += amount
    /// - Levels 2+: team_volume += amount only
    ///
    /// # Arguments
    /// * `user` - The user whose action generated the volume (purchaser)
    /// * `asset` - The asset hash for which volume is recorded
    /// * `amount` - Volume amount to add
    /// * `propagate_levels` - Number of upline levels to propagate (max 20)
    /// * `topoheight` - Current block height
    async fn add_team_volume(
        &mut self,
        user: &PublicKey,
        asset: &Hash,
        amount: u64,
        propagate_levels: u8,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    /// Get team volume for a user-asset pair
    async fn get_team_volume(&self, user: &PublicKey, asset: &Hash)
        -> Result<u64, BlockchainError>;

    /// Get direct volume for a user-asset pair (volume from direct referrals only)
    async fn get_direct_volume(
        &self,
        user: &PublicKey,
        asset: &Hash,
    ) -> Result<u64, BlockchainError>;

    /// Get zone volumes (each direct referral's team volume)
    ///
    /// # Arguments
    /// * `user` - The user to query zones for
    /// * `asset` - The asset hash
    /// * `limit` - Maximum number of zones to return
    ///
    /// # Returns
    /// Vector of (direct_referral_address, team_volume) pairs
    async fn get_zone_volumes(
        &self,
        user: &PublicKey,
        asset: &Hash,
        limit: u32,
    ) -> Result<ZoneVolumesResult, BlockchainError>;

    /// Get the full team volume record for a user-asset pair
    async fn get_team_volume_record(
        &self,
        user: &PublicKey,
        asset: &Hash,
    ) -> Result<Option<TeamVolumeRecord>, BlockchainError>;

    // ===== Reward Distribution =====

    /// Distribute rewards to uplines
    ///
    /// # Arguments
    /// * `from_user` - The user whose uplines will receive rewards
    /// * `asset` - The asset hash to distribute
    /// * `total_amount` - Total amount to distribute
    /// * `ratios` - Reward configuration with ratios for each level
    ///
    /// # Returns
    /// Distribution result with details of each transfer made
    ///
    /// This is an atomic operation - either all transfers succeed or none.
    async fn distribute_to_uplines(
        &mut self,
        from_user: &PublicKey,
        asset: tos_common::crypto::Hash,
        total_amount: u64,
        ratios: &ReferralRewardRatios,
    ) -> Result<DistributionResult, BlockchainError>;

    // ===== Administrative Operations =====

    /// Delete referral record (for rollback scenarios)
    /// Only used internally during chain reorganization
    async fn delete_referral_record(&mut self, user: &PublicKey) -> Result<(), BlockchainError>;

    /// Add a user to a referrer's direct referrals list
    async fn add_to_direct_referrals(
        &mut self,
        referrer: &PublicKey,
        user: &PublicKey,
    ) -> Result<(), BlockchainError>;

    /// Remove a user from a referrer's direct referrals list
    async fn remove_from_direct_referrals(
        &mut self,
        referrer: &PublicKey,
        user: &PublicKey,
    ) -> Result<(), BlockchainError>;
}
