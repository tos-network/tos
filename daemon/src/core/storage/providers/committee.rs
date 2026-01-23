// Security Committee storage provider trait
//
// This provider manages security committee storage and queries.
// Reference: TOS-KYC-Level-Design.md

use crate::core::error::BlockchainError;
use async_trait::async_trait;
use tos_common::{
    block::TopoHeight,
    crypto::{Hash, PublicKey},
    kyc::{CommitteeStatus, KycRegion, MemberRole, MemberStatus, SecurityCommittee},
};

/// Storage provider for security committees
#[async_trait]
pub trait CommitteeProvider {
    // ===== Bootstrap Sync =====

    /// List all committees with skip/limit pagination
    async fn list_all_committees(
        &self,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<(Hash, SecurityCommittee)>, BlockchainError>;

    /// Import committee directly without validation (bootstrap sync)
    async fn import_committee(
        &mut self,
        id: &Hash,
        committee: &SecurityCommittee,
    ) -> Result<(), BlockchainError>;

    /// Set global committee ID directly (bootstrap sync)
    async fn set_global_committee_id(&mut self, id: &Hash) -> Result<(), BlockchainError>;

    // ===== Committee Existence and Retrieval =====

    /// Check if a committee exists
    async fn committee_exists(&self, committee_id: &Hash) -> Result<bool, BlockchainError>;

    /// Get a committee by ID
    async fn get_committee(
        &self,
        committee_id: &Hash,
    ) -> Result<Option<SecurityCommittee>, BlockchainError>;

    /// Get the Global Committee
    /// Returns None if Global Committee has not been bootstrapped yet
    async fn get_global_committee(&self) -> Result<Option<SecurityCommittee>, BlockchainError>;

    /// Get the Global Committee ID
    /// Returns None if Global Committee has not been bootstrapped yet
    async fn get_global_committee_id(&self) -> Result<Option<Hash>, BlockchainError>;

    /// Check if Global Committee has been bootstrapped
    async fn is_global_committee_bootstrapped(&self) -> Result<bool, BlockchainError>;

    // ===== Committee Creation =====

    /// Bootstrap the Global Committee (one-time operation)
    ///
    /// # Arguments
    /// * `committee` - The Global Committee to create
    /// * `topoheight` - The block height when bootstrapped
    /// * `tx_hash` - The transaction hash
    ///
    /// # Errors
    /// * `GlobalCommitteeAlreadyExists` - Global Committee already bootstrapped
    async fn bootstrap_global_committee(
        &mut self,
        committee: SecurityCommittee,
        topoheight: TopoHeight,
        tx_hash: &Hash,
    ) -> Result<Hash, BlockchainError>;

    /// Register a new regional committee
    ///
    /// # Arguments
    /// * `committee` - The committee to register
    /// * `parent_id` - The parent committee ID (Global Committee or regional)
    /// * `topoheight` - The block height when registered
    /// * `tx_hash` - The transaction hash
    ///
    /// # Errors
    /// * `ParentCommitteeNotFound` - Parent committee doesn't exist
    /// * `CommitteeAlreadyExists` - Committee with same ID already exists
    /// * `InvalidMaxKycLevel` - max_kyc_level exceeds parent's max_kyc_level
    async fn register_committee(
        &mut self,
        committee: SecurityCommittee,
        parent_id: &Hash,
        topoheight: TopoHeight,
        tx_hash: &Hash,
    ) -> Result<Hash, BlockchainError>;

    // ===== Committee Updates =====

    /// Update committee status
    async fn update_committee_status(
        &mut self,
        committee_id: &Hash,
        status: CommitteeStatus,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    /// Update committee governance threshold
    async fn update_committee_threshold(
        &mut self,
        committee_id: &Hash,
        threshold: u8,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    /// Update committee KYC threshold
    async fn update_committee_kyc_threshold(
        &mut self,
        committee_id: &Hash,
        kyc_threshold: u8,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    /// Update committee name
    async fn update_committee_name(
        &mut self,
        committee_id: &Hash,
        name: String,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    // ===== Member Management =====

    /// Add a member to a committee
    ///
    /// # Arguments
    /// * `committee_id` - The committee ID
    /// * `member_pubkey` - The member's public key
    /// * `name` - Optional human-readable name
    /// * `role` - The member's role
    /// * `topoheight` - The block height when added
    ///
    /// # Errors
    /// * `CommitteeNotFound` - Committee doesn't exist
    /// * `MemberAlreadyExists` - Member already in committee
    async fn add_committee_member(
        &mut self,
        committee_id: &Hash,
        member_pubkey: &PublicKey,
        name: Option<String>,
        role: MemberRole,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    /// Remove a member from a committee
    ///
    /// # Errors
    /// * `CommitteeNotFound` - Committee doesn't exist
    /// * `MemberNotFound` - Member not in committee
    /// * `CannotRemoveLastMember` - Would leave committee empty
    async fn remove_committee_member(
        &mut self,
        committee_id: &Hash,
        member_pubkey: &PublicKey,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    /// Update member role
    async fn update_member_role(
        &mut self,
        committee_id: &Hash,
        member_pubkey: &PublicKey,
        new_role: MemberRole,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    /// Update member status
    async fn update_member_status(
        &mut self,
        committee_id: &Hash,
        member_pubkey: &PublicKey,
        new_status: MemberStatus,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    // ===== Member Queries =====

    /// Check if a public key is a member of a committee
    async fn is_committee_member(
        &self,
        committee_id: &Hash,
        member_pubkey: &PublicKey,
    ) -> Result<bool, BlockchainError>;

    /// Check if a member is active in a committee
    async fn is_member_active(
        &self,
        committee_id: &Hash,
        member_pubkey: &PublicKey,
    ) -> Result<bool, BlockchainError>;

    /// Get all committees a member belongs to
    async fn get_member_committees(
        &self,
        member_pubkey: &PublicKey,
    ) -> Result<Vec<Hash>, BlockchainError>;

    /// Get member count for a committee
    async fn get_committee_member_count(
        &self,
        committee_id: &Hash,
    ) -> Result<usize, BlockchainError>;

    /// Get active member count for a committee
    async fn get_active_member_count(&self, committee_id: &Hash) -> Result<usize, BlockchainError>;

    // ===== Committee Queries =====

    /// Get all committees in a region
    async fn get_committees_by_region(
        &self,
        region: KycRegion,
    ) -> Result<Vec<SecurityCommittee>, BlockchainError>;

    /// Get all active committees
    async fn get_active_committees(&self) -> Result<Vec<SecurityCommittee>, BlockchainError>;

    /// Get child committees (committees that have this committee as parent)
    async fn get_child_committees(
        &self,
        parent_id: &Hash,
    ) -> Result<Vec<SecurityCommittee>, BlockchainError>;

    /// Get the parent committee of a committee
    async fn get_parent_committee(
        &self,
        committee_id: &Hash,
    ) -> Result<Option<SecurityCommittee>, BlockchainError>;

    // ===== Threshold Queries =====

    /// Get governance threshold for a committee
    async fn get_threshold(&self, committee_id: &Hash) -> Result<u8, BlockchainError>;

    /// Get KYC approval threshold for a committee
    async fn get_kyc_threshold(&self, committee_id: &Hash) -> Result<u8, BlockchainError>;

    /// Get maximum KYC level a committee can approve
    async fn get_max_kyc_level(&self, committee_id: &Hash) -> Result<u16, BlockchainError>;

    /// Check if committee can approve a specific KYC level
    async fn can_approve_level(
        &self,
        committee_id: &Hash,
        level: u16,
    ) -> Result<bool, BlockchainError>;

    // ===== Verification Helpers =====

    /// Count approvals from active committee members
    /// Used to verify if threshold is met
    async fn count_valid_approvals(
        &self,
        committee_id: &Hash,
        approver_pubkeys: &[PublicKey],
    ) -> Result<usize, BlockchainError>;

    /// Check if a set of approvals meets the governance threshold
    async fn meets_governance_threshold(
        &self,
        committee_id: &Hash,
        approver_pubkeys: &[PublicKey],
    ) -> Result<bool, BlockchainError>;

    /// Check if a set of approvals meets the KYC threshold for a given level
    /// Tier 5+ requires kyc_threshold + 1 approvals
    async fn meets_kyc_threshold(
        &self,
        committee_id: &Hash,
        approver_pubkeys: &[PublicKey],
        kyc_level: u16,
    ) -> Result<bool, BlockchainError>;

    // ===== Administrative Operations =====

    /// Delete committee (for rollback scenarios)
    /// Only used internally during chain reorganization
    async fn delete_committee(&mut self, committee_id: &Hash) -> Result<(), BlockchainError>;

    /// Get total committee count
    async fn get_committee_count(&self) -> Result<usize, BlockchainError>;

    /// Get all committee IDs
    async fn get_all_committee_ids(&self) -> Result<Vec<Hash>, BlockchainError>;
}
