// KycProvider implementation for RocksDB storage

use crate::core::{
    error::BlockchainError,
    storage::{
        providers::NetworkProvider,
        rocksdb::{Column, RocksStorage},
        KycProvider,
    },
};
use async_trait::async_trait;
use log::trace;
use serde::{Deserialize, Serialize};
use tos_common::{
    block::TopoHeight,
    crypto::{Hash, PublicKey},
    kyc::{level_to_tier, KycAppealRecord, KycData, KycStatus},
    serializer::{Reader, ReaderError, Serializer, Writer},
};

/// Metadata stored alongside KYC data
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct KycMetadata {
    /// Committee that verified this KYC
    pub committee_id: Hash,
    /// Block height when KYC was set/updated
    pub topoheight: TopoHeight,
    /// Transaction hash that set this KYC
    pub tx_hash: Hash,
}

impl Serializer for KycMetadata {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let committee_id = Hash::read(reader)?;
        let topoheight = TopoHeight::read(reader)?;
        let tx_hash = Hash::read(reader)?;

        Ok(Self {
            committee_id,
            topoheight,
            tx_hash,
        })
    }

    fn write(&self, writer: &mut Writer) {
        self.committee_id.write(writer);
        self.topoheight.write(writer);
        self.tx_hash.write(writer);
    }

    fn size(&self) -> usize {
        self.committee_id.size() + self.topoheight.size() + self.tx_hash.size()
    }
}

/// Emergency suspension data
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EmergencySuspensionData {
    /// Hash of suspension reason
    pub reason_hash: Hash,
    /// When the suspension expires
    pub expires_at: u64,
}

impl Serializer for EmergencySuspensionData {
    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let reason_hash = Hash::read(reader)?;
        let expires_at = u64::read(reader)?;

        Ok(Self {
            reason_hash,
            expires_at,
        })
    }

    fn write(&self, writer: &mut Writer) {
        self.reason_hash.write(writer);
        self.expires_at.write(writer);
    }

    fn size(&self) -> usize {
        self.reason_hash.size() + self.expires_at.size()
    }
}

#[async_trait]
impl KycProvider for RocksStorage {
    async fn has_kyc(&self, user: &PublicKey) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "checking if user {} has KYC",
                user.as_address(self.is_mainnet())
            );
        }
        let data: Option<KycData> =
            self.load_optional_from_disk(Column::KycData, user.as_bytes())?;
        Ok(data.is_some())
    }

    async fn get_kyc(&self, user: &PublicKey) -> Result<Option<KycData>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "getting KYC for user {}",
                user.as_address(self.is_mainnet())
            );
        }
        self.load_optional_from_disk(Column::KycData, user.as_bytes())
    }

    async fn set_kyc(
        &mut self,
        user: &PublicKey,
        kyc_data: KycData,
        committee_id: &Hash,
        topoheight: TopoHeight,
        tx_hash: &Hash,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "setting KYC for user {} at level {} by committee {}",
                user.as_address(self.is_mainnet()),
                kyc_data.level,
                committee_id
            );
        }

        // Check if user already has KYC and prevent level downgrade
        let existing_kyc: Option<KycData> =
            self.load_optional_from_disk(Column::KycData, user.as_bytes())?;
        if let Some(existing) = existing_kyc {
            // Reject if the new level is lower than the existing level
            if kyc_data.level < existing.level {
                return Err(BlockchainError::KycDowngradeNotAllowed(
                    existing.level,
                    kyc_data.level,
                ));
            }
        }

        // Store KYC data
        self.insert_into_disk(Column::KycData, user.as_bytes(), &kyc_data)?;

        // Store metadata
        let metadata = KycMetadata {
            committee_id: committee_id.clone(),
            topoheight,
            tx_hash: tx_hash.clone(),
        };
        self.insert_into_disk(Column::KycMetadata, user.as_bytes(), &metadata)?;

        Ok(())
    }

    async fn update_kyc_status(
        &mut self,
        user: &PublicKey,
        status: KycStatus,
        _topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "updating KYC status for user {} to {:?}",
                user.as_address(self.is_mainnet()),
                status
            );
        }

        let mut kyc_data: KycData = self
            .load_optional_from_disk(Column::KycData, user.as_bytes())?
            .ok_or(BlockchainError::KycNotFound)?;

        kyc_data.status = status;
        self.insert_into_disk(Column::KycData, user.as_bytes(), &kyc_data)?;

        Ok(())
    }

    async fn renew_kyc(
        &mut self,
        user: &PublicKey,
        new_verified_at: u64,
        new_data_hash: Hash,
        topoheight: TopoHeight,
        tx_hash: &Hash,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "renewing KYC for user {}",
                user.as_address(self.is_mainnet())
            );
        }

        let mut kyc_data: KycData = self
            .load_optional_from_disk(Column::KycData, user.as_bytes())?
            .ok_or(BlockchainError::KycNotFound)?;

        // Check current status before renewal - revoked KYC cannot be renewed
        // Revoked users must go through full re-verification process
        match kyc_data.status {
            KycStatus::Revoked => {
                if log::log_enabled!(log::Level::Trace) {
                    trace!(
                        "cannot renew KYC for user {}: KYC is revoked",
                        user.as_address(self.is_mainnet())
                    );
                }
                return Err(BlockchainError::KycRevoked);
            }
            // Suspended, Active, and Expired statuses can be renewed
            KycStatus::Suspended | KycStatus::Active | KycStatus::Expired => {
                // Update verification timestamp and data hash
                kyc_data.verified_at = new_verified_at;
                kyc_data.data_hash = new_data_hash;
                // Only set status to Active for these allowed states
                kyc_data.status = KycStatus::Active;
            }
        }

        self.insert_into_disk(Column::KycData, user.as_bytes(), &kyc_data)?;

        // Update metadata with new topoheight and tx_hash
        if let Some(mut metadata) =
            self.load_optional_from_disk::<_, KycMetadata>(Column::KycMetadata, user.as_bytes())?
        {
            metadata.topoheight = topoheight;
            metadata.tx_hash = tx_hash.clone();
            self.insert_into_disk(Column::KycMetadata, user.as_bytes(), &metadata)?;
        }

        Ok(())
    }

    async fn transfer_kyc(
        &mut self,
        user: &PublicKey,
        new_committee_id: &Hash,
        new_data_hash: Hash,
        transferred_at: u64,
        topoheight: TopoHeight,
        tx_hash: &Hash,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "transferring KYC for user {} to committee {}",
                user.as_address(self.is_mainnet()),
                new_committee_id
            );
        }

        // Get existing KYC data
        let mut kyc_data: KycData = self
            .load_optional_from_disk(Column::KycData, user.as_bytes())?
            .ok_or(BlockchainError::KycNotFound)?;

        // Check KYC status before transfer - only Active or Expired can be transferred
        // Revoked or Suspended KYC cannot be transferred to prevent reactivation bypass
        match kyc_data.status {
            KycStatus::Revoked => {
                if log::log_enabled!(log::Level::Trace) {
                    trace!(
                        "cannot transfer KYC for user {}: KYC is revoked",
                        user.as_address(self.is_mainnet())
                    );
                }
                return Err(BlockchainError::KycRevoked);
            }
            KycStatus::Suspended => {
                if log::log_enabled!(log::Level::Trace) {
                    trace!(
                        "cannot transfer KYC for user {}: KYC is suspended",
                        user.as_address(self.is_mainnet())
                    );
                }
                return Err(BlockchainError::KycSuspended);
            }
            KycStatus::Active | KycStatus::Expired => {
                // Allow transfer for Active and Expired statuses
            }
        }

        // Update verified_at, data_hash, and set status to Active
        kyc_data.verified_at = transferred_at;
        kyc_data.data_hash = new_data_hash;
        kyc_data.status = KycStatus::Active;
        self.insert_into_disk(Column::KycData, user.as_bytes(), &kyc_data)?;

        // Update metadata with new committee_id, topoheight and tx_hash
        let metadata = KycMetadata {
            committee_id: new_committee_id.clone(),
            topoheight,
            tx_hash: tx_hash.clone(),
        };
        self.insert_into_disk(Column::KycMetadata, user.as_bytes(), &metadata)?;

        Ok(())
    }

    async fn revoke_kyc(
        &mut self,
        user: &PublicKey,
        _reason_hash: &Hash,
        _topoheight: TopoHeight,
        _tx_hash: &Hash,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "revoking KYC for user {}",
                user.as_address(self.is_mainnet())
            );
        }

        let mut kyc_data: KycData = self
            .load_optional_from_disk(Column::KycData, user.as_bytes())?
            .ok_or(BlockchainError::KycNotFound)?;

        kyc_data.status = KycStatus::Revoked;
        self.insert_into_disk(Column::KycData, user.as_bytes(), &kyc_data)?;

        Ok(())
    }

    async fn get_effective_level(
        &self,
        user: &PublicKey,
        current_time: u64,
    ) -> Result<u16, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "getting effective KYC level for user {}",
                user.as_address(self.is_mainnet())
            );
        }

        let kyc_data: Option<KycData> =
            self.load_optional_from_disk(Column::KycData, user.as_bytes())?;

        Ok(kyc_data
            .map(|d| d.effective_level(current_time))
            .unwrap_or(0))
    }

    async fn get_effective_tier(
        &self,
        user: &PublicKey,
        current_time: u64,
    ) -> Result<u8, BlockchainError> {
        let level = self.get_effective_level(user, current_time).await?;
        Ok(level_to_tier(level))
    }

    async fn meets_kyc_level(
        &self,
        user: &PublicKey,
        required_level: u16,
        current_time: u64,
    ) -> Result<bool, BlockchainError> {
        let effective_level = self.get_effective_level(user, current_time).await?;
        Ok(effective_level >= required_level)
    }

    async fn is_kyc_valid(
        &self,
        user: &PublicKey,
        current_time: u64,
    ) -> Result<bool, BlockchainError> {
        let kyc_data: Option<KycData> =
            self.load_optional_from_disk(Column::KycData, user.as_bytes())?;

        Ok(kyc_data.map(|d| d.is_valid(current_time)).unwrap_or(false))
    }

    async fn get_verifying_committee(
        &self,
        user: &PublicKey,
    ) -> Result<Option<Hash>, BlockchainError> {
        let metadata: Option<KycMetadata> =
            self.load_optional_from_disk(Column::KycMetadata, user.as_bytes())?;
        Ok(metadata.map(|m| m.committee_id))
    }

    async fn get_kyc_topoheight(
        &self,
        user: &PublicKey,
    ) -> Result<Option<TopoHeight>, BlockchainError> {
        let metadata: Option<KycMetadata> =
            self.load_optional_from_disk(Column::KycMetadata, user.as_bytes())?;
        Ok(metadata.map(|m| m.topoheight))
    }

    async fn get_kyc_batch(
        &self,
        users: &[PublicKey],
    ) -> Result<Vec<(PublicKey, Option<KycData>)>, BlockchainError> {
        let mut results = Vec::with_capacity(users.len());
        for user in users {
            let kyc = self.get_kyc(user).await?;
            results.push((user.clone(), kyc));
        }
        Ok(results)
    }

    async fn check_kyc_batch(
        &self,
        users: &[PublicKey],
        required_level: u16,
        current_time: u64,
    ) -> Result<Vec<(PublicKey, bool)>, BlockchainError> {
        let mut results = Vec::with_capacity(users.len());
        for user in users {
            let meets = self
                .meets_kyc_level(user, required_level, current_time)
                .await?;
            results.push((user.clone(), meets));
        }
        Ok(results)
    }

    async fn emergency_suspend(
        &mut self,
        user: &PublicKey,
        reason_hash: &Hash,
        expires_at: u64,
        topoheight: TopoHeight,
        _tx_hash: &Hash,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "emergency suspending user {} until {}",
                user.as_address(self.is_mainnet()),
                expires_at
            );
        }

        // Get current KYC data and save the previous status before suspension
        let mut kyc_data: KycData = self
            .load_optional_from_disk(Column::KycData, user.as_bytes())?
            .ok_or(BlockchainError::KycNotFound)?;

        // Store the previous status before setting to Suspended
        // This allows lift_emergency_suspension to restore the correct status
        // (e.g., if user was Revoked before suspension, they should remain Revoked after lifting)
        self.insert_into_disk(
            Column::KycEmergencyPreviousStatus,
            user.as_bytes(),
            &kyc_data.status,
        )?;

        // Update KYC status to Suspended
        kyc_data.status = KycStatus::Suspended;
        self.insert_into_disk(Column::KycData, user.as_bytes(), &kyc_data)?;

        // Store emergency suspension data
        let suspension = EmergencySuspensionData {
            reason_hash: reason_hash.clone(),
            expires_at,
        };
        self.insert_into_disk(Column::KycEmergencySuspension, user.as_bytes(), &suspension)?;

        // Update metadata
        if let Some(mut metadata) =
            self.load_optional_from_disk::<_, KycMetadata>(Column::KycMetadata, user.as_bytes())?
        {
            metadata.topoheight = topoheight;
            self.insert_into_disk(Column::KycMetadata, user.as_bytes(), &metadata)?;
        }

        Ok(())
    }

    async fn get_emergency_suspension(
        &self,
        user: &PublicKey,
    ) -> Result<Option<(Hash, u64)>, BlockchainError> {
        let suspension: Option<EmergencySuspensionData> =
            self.load_optional_from_disk(Column::KycEmergencySuspension, user.as_bytes())?;
        Ok(suspension.map(|s| (s.reason_hash, s.expires_at)))
    }

    async fn lift_emergency_suspension(
        &mut self,
        user: &PublicKey,
        _topoheight: TopoHeight,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "lifting emergency suspension for user {}",
                user.as_address(self.is_mainnet())
            );
        }

        // Restore KYC status to the previous status before suspension
        let mut kyc_data: KycData = self
            .load_optional_from_disk(Column::KycData, user.as_bytes())?
            .ok_or(BlockchainError::KycNotFound)?;

        // Read the stored previous status from before the suspension
        // For legacy records (before this fix), default to Active for backward compatibility
        let previous_status: KycStatus = self
            .load_optional_from_disk(Column::KycEmergencyPreviousStatus, user.as_bytes())?
            .unwrap_or(KycStatus::Active);

        kyc_data.status = previous_status;
        self.insert_into_disk(Column::KycData, user.as_bytes(), &kyc_data)?;

        // Remove emergency suspension data
        self.remove_from_disk(Column::KycEmergencySuspension, user.as_bytes())?;

        // Remove the stored previous status as it's no longer needed
        self.remove_from_disk(Column::KycEmergencyPreviousStatus, user.as_bytes())?;

        Ok(())
    }

    async fn delete_kyc_record(&mut self, user: &PublicKey) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "deleting KYC record for user {}",
                user.as_address(self.is_mainnet())
            );
        }

        self.remove_from_disk(Column::KycData, user.as_bytes())?;
        self.remove_from_disk(Column::KycMetadata, user.as_bytes())?;
        self.remove_from_disk(Column::KycEmergencySuspension, user.as_bytes())?;
        self.remove_from_disk(Column::KycEmergencyPreviousStatus, user.as_bytes())?;

        Ok(())
    }

    async fn count_users_at_level(
        &self,
        _min_level: u16,
        _current_time: u64,
    ) -> Result<u64, BlockchainError> {
        // This would require iterating all KYC records - expensive operation
        // For now, return 0 as this is a statistics function
        // Could be implemented with a counter or index in the future
        Ok(0)
    }

    async fn submit_appeal(
        &mut self,
        user: &PublicKey,
        original_committee_id: &Hash,
        parent_committee_id: &Hash,
        reason_hash: &Hash,
        documents_hash: &Hash,
        submitted_at: u64,
        topoheight: TopoHeight,
        tx_hash: &Hash,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "submitting KYC appeal for user {} from committee {} to parent {}",
                user.as_address(self.is_mainnet()),
                original_committee_id,
                parent_committee_id
            );
        }

        // Create appeal record
        let appeal = KycAppealRecord::new(
            original_committee_id.clone(),
            parent_committee_id.clone(),
            reason_hash.clone(),
            documents_hash.clone(),
            submitted_at,
        );

        // Store appeal record
        self.insert_into_disk(Column::KycAppeal, user.as_bytes(), &appeal)?;

        // Update metadata
        if let Some(mut metadata) =
            self.load_optional_from_disk::<_, KycMetadata>(Column::KycMetadata, user.as_bytes())?
        {
            metadata.topoheight = topoheight;
            metadata.tx_hash = tx_hash.clone();
            self.insert_into_disk(Column::KycMetadata, user.as_bytes(), &metadata)?;
        }

        Ok(())
    }

    async fn get_appeal(
        &self,
        user: &PublicKey,
    ) -> Result<Option<KycAppealRecord>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "getting KYC appeal for user {}",
                user.as_address(self.is_mainnet())
            );
        }
        self.load_optional_from_disk(Column::KycAppeal, user.as_bytes())
    }
}
