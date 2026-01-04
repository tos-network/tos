// CommitteeProvider implementation for RocksDB storage

use crate::core::{
    error::BlockchainError,
    storage::{
        providers::NetworkProvider,
        rocksdb::{Column, RocksStorage},
        CommitteeProvider,
    },
};
use async_trait::async_trait;
use log::trace;
use tos_common::{
    block::TopoHeight,
    crypto::{Hash, PublicKey},
    kyc::{level_to_tier, CommitteeStatus, KycRegion, MemberRole, MemberStatus, SecurityCommittee},
};

/// Key for the global committee ID
const GLOBAL_COMMITTEE_KEY: &[u8] = b"global_committee_id";

#[async_trait]
impl CommitteeProvider for RocksStorage {
    async fn committee_exists(&self, committee_id: &Hash) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("checking if committee {} exists", committee_id);
        }
        let committee: Option<SecurityCommittee> =
            self.load_optional_from_disk(Column::Committees, committee_id.as_bytes())?;
        Ok(committee.is_some())
    }

    async fn get_committee(
        &self,
        committee_id: &Hash,
    ) -> Result<Option<SecurityCommittee>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("getting committee {}", committee_id);
        }
        self.load_optional_from_disk(Column::Committees, committee_id.as_bytes())
    }

    async fn get_global_committee(&self) -> Result<Option<SecurityCommittee>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("getting global committee");
        }
        if let Some(id) = self.get_global_committee_id().await? {
            self.get_committee(&id).await
        } else {
            Ok(None)
        }
    }

    async fn get_global_committee_id(&self) -> Result<Option<Hash>, BlockchainError> {
        self.load_optional_from_disk(Column::GlobalCommittee, GLOBAL_COMMITTEE_KEY)
    }

    async fn is_global_committee_bootstrapped(&self) -> Result<bool, BlockchainError> {
        Ok(self.get_global_committee_id().await?.is_some())
    }

    async fn bootstrap_global_committee(
        &mut self,
        committee: SecurityCommittee,
        _topoheight: TopoHeight,
        _tx_hash: &Hash,
    ) -> Result<Hash, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("bootstrapping global committee: {}", committee.name);
        }

        // Check if already bootstrapped
        if self.is_global_committee_bootstrapped().await? {
            return Err(BlockchainError::GlobalCommitteeAlreadyExists);
        }

        let committee_id = committee.id.clone();

        // Store the committee
        self.insert_into_disk(Column::Committees, committee_id.as_bytes(), &committee)?;

        // Store the global committee ID reference
        self.insert_into_disk(Column::GlobalCommittee, GLOBAL_COMMITTEE_KEY, &committee_id)?;

        // Index by region
        let region_key = self.make_region_key(committee.region, &committee_id);
        self.insert_into_disk(Column::CommitteesByRegion, &region_key, &())?;

        // Index members
        for member in &committee.members {
            self.add_member_committee_index(&member.public_key, &committee_id)
                .await?;
        }

        Ok(committee_id)
    }

    async fn register_committee(
        &mut self,
        committee: SecurityCommittee,
        parent_id: &Hash,
        _topoheight: TopoHeight,
        _tx_hash: &Hash,
    ) -> Result<Hash, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "registering committee {} under parent {}",
                committee.name,
                parent_id
            );
        }

        // Verify parent exists
        let parent = self
            .get_committee(parent_id)
            .await?
            .ok_or(BlockchainError::ParentCommitteeNotFound)?;

        // Verify max_kyc_level doesn't exceed parent's
        if committee.max_kyc_level > parent.max_kyc_level {
            return Err(BlockchainError::InvalidMaxKycLevel);
        }

        let committee_id = committee.id.clone();

        // Check if committee already exists
        if self.committee_exists(&committee_id).await? {
            return Err(BlockchainError::CommitteeAlreadyExists);
        }

        // Store the committee
        self.insert_into_disk(Column::Committees, committee_id.as_bytes(), &committee)?;

        // Index by region
        let region_key = self.make_region_key(committee.region, &committee_id);
        self.insert_into_disk(Column::CommitteesByRegion, &region_key, &())?;

        // Index members
        for member in &committee.members {
            self.add_member_committee_index(&member.public_key, &committee_id)
                .await?;
        }

        // Add to parent's child list
        self.add_child_committee(parent_id, &committee_id).await?;

        Ok(committee_id)
    }

    async fn update_committee_status(
        &mut self,
        committee_id: &Hash,
        status: CommitteeStatus,
        _topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("updating committee {} status to {:?}", committee_id, status);
        }

        let mut committee: SecurityCommittee = self
            .get_committee(committee_id)
            .await?
            .ok_or(BlockchainError::CommitteeNotFound)?;

        committee.status = status;
        self.insert_into_disk(Column::Committees, committee_id.as_bytes(), &committee)?;

        Ok(())
    }

    async fn update_committee_threshold(
        &mut self,
        committee_id: &Hash,
        threshold: u8,
        _topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "updating committee {} threshold to {}",
                committee_id,
                threshold
            );
        }

        let mut committee: SecurityCommittee = self
            .get_committee(committee_id)
            .await?
            .ok_or(BlockchainError::CommitteeNotFound)?;

        committee.threshold = threshold;
        self.insert_into_disk(Column::Committees, committee_id.as_bytes(), &committee)?;

        Ok(())
    }

    async fn update_committee_kyc_threshold(
        &mut self,
        committee_id: &Hash,
        kyc_threshold: u8,
        _topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "updating committee {} kyc_threshold to {}",
                committee_id,
                kyc_threshold
            );
        }

        let mut committee: SecurityCommittee = self
            .get_committee(committee_id)
            .await?
            .ok_or(BlockchainError::CommitteeNotFound)?;

        committee.kyc_threshold = kyc_threshold;
        self.insert_into_disk(Column::Committees, committee_id.as_bytes(), &committee)?;

        Ok(())
    }

    async fn update_committee_name(
        &mut self,
        committee_id: &Hash,
        name: String,
        _topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("updating committee {} name to {}", committee_id, name);
        }

        let mut committee: SecurityCommittee = self
            .get_committee(committee_id)
            .await?
            .ok_or(BlockchainError::CommitteeNotFound)?;

        committee.name = name;
        self.insert_into_disk(Column::Committees, committee_id.as_bytes(), &committee)?;

        Ok(())
    }

    async fn add_committee_member(
        &mut self,
        committee_id: &Hash,
        member_pubkey: &PublicKey,
        name: Option<String>,
        role: MemberRole,
        _topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "adding member {} to committee {}",
                member_pubkey.as_address(self.is_mainnet()),
                committee_id
            );
        }

        let mut committee: SecurityCommittee = self
            .get_committee(committee_id)
            .await?
            .ok_or(BlockchainError::CommitteeNotFound)?;

        // Check if member already exists
        if committee
            .members
            .iter()
            .any(|m| m.public_key == *member_pubkey)
        {
            return Err(BlockchainError::MemberAlreadyExists);
        }

        // Add member
        committee.add_member(member_pubkey.clone(), name, role);
        self.insert_into_disk(Column::Committees, committee_id.as_bytes(), &committee)?;

        // Index member
        self.add_member_committee_index(member_pubkey, committee_id)
            .await?;

        Ok(())
    }

    async fn remove_committee_member(
        &mut self,
        committee_id: &Hash,
        member_pubkey: &PublicKey,
        _topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "removing member {} from committee {}",
                member_pubkey.as_address(self.is_mainnet()),
                committee_id
            );
        }

        let mut committee: SecurityCommittee = self
            .get_committee(committee_id)
            .await?
            .ok_or(BlockchainError::CommitteeNotFound)?;

        // Cannot remove last member
        if committee.members.len() <= 1 {
            return Err(BlockchainError::CannotRemoveLastMember);
        }

        // Remove member
        if !committee.remove_member(member_pubkey) {
            return Err(BlockchainError::MemberNotFound);
        }

        self.insert_into_disk(Column::Committees, committee_id.as_bytes(), &committee)?;

        // Remove from member index
        self.remove_member_committee_index(member_pubkey, committee_id)
            .await?;

        Ok(())
    }

    async fn update_member_role(
        &mut self,
        committee_id: &Hash,
        member_pubkey: &PublicKey,
        new_role: MemberRole,
        _topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        let mut committee: SecurityCommittee = self
            .get_committee(committee_id)
            .await?
            .ok_or(BlockchainError::CommitteeNotFound)?;

        if let Some(member) = committee
            .members
            .iter_mut()
            .find(|m| m.public_key == *member_pubkey)
        {
            member.role = new_role;
            self.insert_into_disk(Column::Committees, committee_id.as_bytes(), &committee)?;
            Ok(())
        } else {
            Err(BlockchainError::MemberNotFound)
        }
    }

    async fn update_member_status(
        &mut self,
        committee_id: &Hash,
        member_pubkey: &PublicKey,
        new_status: MemberStatus,
        _topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        let mut committee: SecurityCommittee = self
            .get_committee(committee_id)
            .await?
            .ok_or(BlockchainError::CommitteeNotFound)?;

        if let Some(member) = committee
            .members
            .iter_mut()
            .find(|m| m.public_key == *member_pubkey)
        {
            let old_status = member.status.clone();
            member.status = new_status.clone();
            self.insert_into_disk(Column::Committees, committee_id.as_bytes(), &committee)?;

            // When transitioning to MemberStatus::Removed,
            // also remove the member from the MemberCommittees index.
            // This prevents stale indexes where removed members still appear to be
            // associated with committees they're no longer part of.
            if new_status == MemberStatus::Removed {
                self.remove_member_committee_index(member_pubkey, committee_id)
                    .await?;
            }
            if old_status == MemberStatus::Removed && new_status == MemberStatus::Active {
                self.add_member_committee_index(member_pubkey, committee_id)
                    .await?;
            }

            Ok(())
        } else {
            Err(BlockchainError::MemberNotFound)
        }
    }

    async fn is_committee_member(
        &self,
        committee_id: &Hash,
        member_pubkey: &PublicKey,
    ) -> Result<bool, BlockchainError> {
        let committee = self.get_committee(committee_id).await?;
        Ok(committee
            .map(|c| c.members.iter().any(|m| m.public_key == *member_pubkey))
            .unwrap_or(false))
    }

    async fn is_member_active(
        &self,
        committee_id: &Hash,
        member_pubkey: &PublicKey,
    ) -> Result<bool, BlockchainError> {
        let committee = self.get_committee(committee_id).await?;
        Ok(committee
            .map(|c| {
                c.members
                    .iter()
                    .any(|m| m.public_key == *member_pubkey && m.status == MemberStatus::Active)
            })
            .unwrap_or(false))
    }

    async fn get_member_committees(
        &self,
        member_pubkey: &PublicKey,
    ) -> Result<Vec<Hash>, BlockchainError> {
        let committees: Option<Vec<Hash>> =
            self.load_optional_from_disk(Column::MemberCommittees, member_pubkey.as_bytes())?;
        Ok(committees.unwrap_or_default())
    }

    async fn get_committee_member_count(
        &self,
        committee_id: &Hash,
    ) -> Result<usize, BlockchainError> {
        let committee = self.get_committee(committee_id).await?;
        Ok(committee.map(|c| c.members.len()).unwrap_or(0))
    }

    async fn get_active_member_count(&self, committee_id: &Hash) -> Result<usize, BlockchainError> {
        let committee = self.get_committee(committee_id).await?;
        Ok(committee
            .map(|c| {
                c.members
                    .iter()
                    .filter(|m| m.status == MemberStatus::Active)
                    .count()
            })
            .unwrap_or(0))
    }

    async fn get_committees_by_region(
        &self,
        region: KycRegion,
    ) -> Result<Vec<SecurityCommittee>, BlockchainError> {
        // This would require prefix iteration which is expensive
        // For now, iterate all committees and filter
        let all_ids = self.get_all_committee_ids().await?;
        let mut results = Vec::new();
        for id in all_ids {
            if let Some(committee) = self.get_committee(&id).await? {
                if committee.region == region {
                    results.push(committee);
                }
            }
        }
        Ok(results)
    }

    async fn get_active_committees(&self) -> Result<Vec<SecurityCommittee>, BlockchainError> {
        let all_ids = self.get_all_committee_ids().await?;
        let mut results = Vec::new();
        for id in all_ids {
            if let Some(committee) = self.get_committee(&id).await? {
                if committee.status == CommitteeStatus::Active {
                    results.push(committee);
                }
            }
        }
        Ok(results)
    }

    async fn get_child_committees(
        &self,
        parent_id: &Hash,
    ) -> Result<Vec<SecurityCommittee>, BlockchainError> {
        let child_ids: Option<Vec<Hash>> =
            self.load_optional_from_disk(Column::ChildCommittees, parent_id.as_bytes())?;

        let mut results = Vec::new();
        if let Some(ids) = child_ids {
            for id in ids {
                if let Some(committee) = self.get_committee(&id).await? {
                    results.push(committee);
                }
            }
        }
        Ok(results)
    }

    async fn get_parent_committee(
        &self,
        committee_id: &Hash,
    ) -> Result<Option<SecurityCommittee>, BlockchainError> {
        let committee = self.get_committee(committee_id).await?;
        if let Some(c) = committee {
            if let Some(parent_id) = c.parent_id {
                return self.get_committee(&parent_id).await;
            }
        }
        Ok(None)
    }

    async fn get_threshold(&self, committee_id: &Hash) -> Result<u8, BlockchainError> {
        let committee = self
            .get_committee(committee_id)
            .await?
            .ok_or(BlockchainError::CommitteeNotFound)?;
        Ok(committee.threshold)
    }

    async fn get_kyc_threshold(&self, committee_id: &Hash) -> Result<u8, BlockchainError> {
        let committee = self
            .get_committee(committee_id)
            .await?
            .ok_or(BlockchainError::CommitteeNotFound)?;
        Ok(committee.kyc_threshold)
    }

    async fn get_max_kyc_level(&self, committee_id: &Hash) -> Result<u16, BlockchainError> {
        let committee = self
            .get_committee(committee_id)
            .await?
            .ok_or(BlockchainError::CommitteeNotFound)?;
        Ok(committee.max_kyc_level)
    }

    async fn can_approve_level(
        &self,
        committee_id: &Hash,
        level: u16,
    ) -> Result<bool, BlockchainError> {
        let max_level = self.get_max_kyc_level(committee_id).await?;
        Ok(level <= max_level)
    }

    async fn count_valid_approvals(
        &self,
        committee_id: &Hash,
        approver_pubkeys: &[PublicKey],
    ) -> Result<usize, BlockchainError> {
        let mut count = 0;
        for pubkey in approver_pubkeys {
            if self.is_member_active(committee_id, pubkey).await? {
                count += 1;
            }
        }
        Ok(count)
    }

    async fn meets_governance_threshold(
        &self,
        committee_id: &Hash,
        approver_pubkeys: &[PublicKey],
    ) -> Result<bool, BlockchainError> {
        let threshold = self.get_threshold(committee_id).await?;
        let valid_count = self
            .count_valid_approvals(committee_id, approver_pubkeys)
            .await?;
        Ok(valid_count >= threshold as usize)
    }

    async fn meets_kyc_threshold(
        &self,
        committee_id: &Hash,
        approver_pubkeys: &[PublicKey],
        kyc_level: u16,
    ) -> Result<bool, BlockchainError> {
        let base_threshold = self.get_kyc_threshold(committee_id).await?;
        let tier = level_to_tier(kyc_level);

        // Tier 5+ requires kyc_threshold + 1 approvals
        let required_threshold = if tier >= 5 {
            base_threshold + 1
        } else {
            base_threshold
        };

        let valid_count = self
            .count_valid_approvals(committee_id, approver_pubkeys)
            .await?;
        Ok(valid_count >= required_threshold as usize)
    }

    async fn delete_committee(&mut self, committee_id: &Hash) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("deleting committee {}", committee_id);
        }

        // Check if committee has children before deletion
        // Deleting a parent committee would orphan children (their parent_id would point to
        // a non-existent committee). Block deletion if children exist.
        let children: Option<Vec<Hash>> =
            self.load_optional_from_disk(Column::ChildCommittees, committee_id.as_bytes())?;
        if let Some(ref child_list) = children {
            if !child_list.is_empty() {
                if log::log_enabled!(log::Level::Trace) {
                    trace!(
                        "cannot delete committee {}: has {} children",
                        committee_id,
                        child_list.len()
                    );
                }
                return Err(BlockchainError::CommitteeHasChildren);
            }
        }

        // Get committee for cleanup
        if let Some(committee) = self.get_committee(committee_id).await? {
            // Remove from region index
            let region_key = self.make_region_key(committee.region, committee_id);
            self.remove_from_disk(Column::CommitteesByRegion, &region_key)?;

            // Remove member indexes
            for member in &committee.members {
                let _ = self
                    .remove_member_committee_index(&member.public_key, committee_id)
                    .await;
            }

            // Remove from parent's child list if applicable
            if let Some(parent_id) = &committee.parent_id {
                let _ = self.remove_child_committee(parent_id, committee_id).await;
            }
        }

        // Delete the committee itself
        self.remove_from_disk(Column::Committees, committee_id.as_bytes())?;

        // Delete child committees list (should be empty at this point)
        self.remove_from_disk(Column::ChildCommittees, committee_id.as_bytes())?;

        Ok(())
    }

    async fn get_committee_count(&self) -> Result<usize, BlockchainError> {
        let ids = self.get_all_committee_ids().await?;
        Ok(ids.len())
    }

    async fn get_all_committee_ids(&self) -> Result<Vec<Hash>, BlockchainError> {
        // Traverse the full committee hierarchy tree using BFS (iterative approach)
        // This handles arbitrary depth and prevents infinite loops from circular references
        use std::collections::HashSet;
        use std::collections::VecDeque;

        let mut ids = Vec::new();
        let mut visited: HashSet<Hash> = HashSet::new();
        let mut queue: VecDeque<Hash> = VecDeque::new();

        // Start from the global committee if it exists
        if let Some(global_id) = self.get_global_committee_id().await? {
            queue.push_back(global_id);
        }

        // BFS traversal of the committee hierarchy
        while let Some(current_id) = queue.pop_front() {
            // Skip if already visited (prevents infinite loops from circular references)
            if visited.contains(&current_id) {
                continue;
            }

            // Mark as visited and add to results
            visited.insert(current_id.clone());
            ids.push(current_id.clone());

            // Get direct children and add them to the queue for processing
            let children = self.get_child_committees(&current_id).await?;
            for child in children {
                if !visited.contains(&child.id) {
                    queue.push_back(child.id);
                }
            }
        }

        Ok(ids)
    }
}

// Helper methods for RocksStorage
impl RocksStorage {
    fn make_region_key(&self, region: KycRegion, committee_id: &Hash) -> Vec<u8> {
        let mut key = vec![region as u8];
        key.extend_from_slice(committee_id.as_bytes());
        key
    }

    async fn add_member_committee_index(
        &mut self,
        member: &PublicKey,
        committee_id: &Hash,
    ) -> Result<(), BlockchainError> {
        let mut committees: Vec<Hash> = self
            .load_optional_from_disk(Column::MemberCommittees, member.as_bytes())?
            .unwrap_or_default();

        if !committees.contains(committee_id) {
            committees.push(committee_id.clone());
            self.insert_into_disk(Column::MemberCommittees, member.as_bytes(), &committees)?;
        }
        Ok(())
    }

    async fn remove_member_committee_index(
        &mut self,
        member: &PublicKey,
        committee_id: &Hash,
    ) -> Result<(), BlockchainError> {
        let mut committees: Vec<Hash> = self
            .load_optional_from_disk(Column::MemberCommittees, member.as_bytes())?
            .unwrap_or_default();

        committees.retain(|id| id != committee_id);
        self.insert_into_disk(Column::MemberCommittees, member.as_bytes(), &committees)?;
        Ok(())
    }

    async fn add_child_committee(
        &mut self,
        parent_id: &Hash,
        child_id: &Hash,
    ) -> Result<(), BlockchainError> {
        let mut children: Vec<Hash> = self
            .load_optional_from_disk(Column::ChildCommittees, parent_id.as_bytes())?
            .unwrap_or_default();

        if !children.contains(child_id) {
            children.push(child_id.clone());
            self.insert_into_disk(Column::ChildCommittees, parent_id.as_bytes(), &children)?;
        }
        Ok(())
    }

    async fn remove_child_committee(
        &mut self,
        parent_id: &Hash,
        child_id: &Hash,
    ) -> Result<(), BlockchainError> {
        let mut children: Vec<Hash> = self
            .load_optional_from_disk(Column::ChildCommittees, parent_id.as_bytes())?
            .unwrap_or_default();

        children.retain(|id| id != child_id);
        self.insert_into_disk(Column::ChildCommittees, parent_id.as_bytes(), &children)?;
        Ok(())
    }
}
