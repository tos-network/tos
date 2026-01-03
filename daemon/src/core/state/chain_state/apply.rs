use crate::core::{
    error::BlockchainError,
    storage::{
        Storage, VersionedContract, VersionedContractBalance, VersionedContractData,
        VersionedMultiSig, VersionedSupply,
    },
};
use async_trait::async_trait;
use indexmap::IndexMap;
use log::{debug, trace};
use std::{
    borrow::Cow,
    collections::{hash_map::Entry, HashMap},
    ops::{Deref, DerefMut},
};
use tos_common::{
    account::{AccountEnergy, BalanceType, Nonce, VersionedNonce},
    ai_mining::AIMiningState,
    asset::VersionedAssetData,
    block::{Block, BlockVersion, TopoHeight},
    contract::{
        AssetChanges, ChainState as ContractChainState, ContractCache, ContractEventTracker,
        ContractOutput, ScheduledExecution,
    },
    crypto::{
        elgamal::{Ciphertext, CompressedPublicKey},
        Hash, PublicKey,
    },
    transaction::{
        verify::{BlockchainApplyState, BlockchainVerificationState, ContractEnvironment},
        ContractDeposit, MultiSigPayload, Reference,
    },
    versioned_type::VersionedState,
};
use tos_kernel::{Environment, Module};

use super::{ChainState, Echange, StorageReference};

struct ContractManager {
    outputs: HashMap<Hash, Vec<ContractOutput>>,
    caches: HashMap<Hash, ContractCache>,
    // global assets cache
    assets: HashMap<Hash, Option<AssetChanges>>,
    tracker: ContractEventTracker,
    // Scheduled executions registered during this block
    // Key: (contract_hash, execution_topoheight)
    scheduled_executions: Vec<(Hash, TopoHeight, ScheduledExecution)>,
}

// Chain State that can be applied to the mutable storage
pub struct ApplicableChainState<'a, S: Storage> {
    inner: ChainState<'a, S>,
    block_hash: &'a Hash,
    block: &'a Block,
    contract_manager: ContractManager,
    burned_supply: u64,
    gas_fee: u64,
    executor: std::sync::Arc<dyn tos_common::contract::ContractExecutor>,
}

#[async_trait]
impl<'a, S: Storage> BlockchainVerificationState<'a, BlockchainError>
    for ApplicableChainState<'a, S>
{
    /// Pre-verify the TX
    async fn pre_verify_tx<'b>(
        &'b mut self,
        tx: &tos_common::transaction::Transaction,
    ) -> Result<(), BlockchainError> {
        self.inner.pre_verify_tx(tx).await
    }

    /// Get the balance for a receiver account
    async fn get_receiver_balance<'b>(
        &'b mut self,
        account: Cow<'a, PublicKey>,
        asset: Cow<'a, Hash>,
    ) -> Result<&'b mut u64, BlockchainError> {
        self.inner.get_receiver_balance(account, asset).await
    }

    /// Get the balance used for verification of funds for the sender account
    async fn get_sender_balance<'b>(
        &'b mut self,
        account: &'a PublicKey,
        asset: &'a Hash,
        reference: &Reference,
    ) -> Result<&'b mut u64, BlockchainError> {
        self.inner
            .get_sender_balance(account, asset, reference)
            .await
    }

    /// Apply new output to a sender account
    async fn add_sender_output(
        &mut self,
        account: &'a PublicKey,
        asset: &'a Hash,
        output: u64,
    ) -> Result<(), BlockchainError> {
        self.inner.add_sender_output(account, asset, output).await
    }

    // ===== UNO (Privacy Balance) Methods =====
    // UNO balance storage implemented in apply_changes() method

    /// Get the UNO (encrypted) balance for a receiver account
    async fn get_receiver_uno_balance<'b>(
        &'b mut self,
        account: Cow<'a, PublicKey>,
        asset: Cow<'a, Hash>,
    ) -> Result<&'b mut Ciphertext, BlockchainError> {
        self.inner.get_receiver_uno_balance(account, asset).await
    }

    /// Get the UNO (encrypted) balance used for verification of funds for the sender account
    async fn get_sender_uno_balance<'b>(
        &'b mut self,
        account: &'a PublicKey,
        asset: &'a Hash,
        reference: &Reference,
    ) -> Result<&'b mut Ciphertext, BlockchainError> {
        self.inner
            .get_sender_uno_balance(account, asset, reference)
            .await
    }

    /// Apply new output ciphertext to a sender's UNO account
    async fn add_sender_uno_output(
        &mut self,
        account: &'a PublicKey,
        asset: &'a Hash,
        output: Ciphertext,
    ) -> Result<(), BlockchainError> {
        self.inner
            .add_sender_uno_output(account, asset, output)
            .await
    }

    /// Get the nonce of an account
    async fn get_account_nonce(
        &mut self,
        account: &'a PublicKey,
    ) -> Result<Nonce, BlockchainError> {
        self.inner.get_account_nonce(account).await
    }

    async fn update_account_nonce(
        &mut self,
        account: &'a PublicKey,
        new_nonce: Nonce,
    ) -> Result<(), BlockchainError> {
        self.inner.update_account_nonce(account, new_nonce).await
    }

    /// SECURITY FIX V-11: Atomic compare-and-swap for nonce updates
    async fn compare_and_swap_nonce(
        &mut self,
        account: &'a CompressedPublicKey,
        expected: Nonce,
        new_value: Nonce,
    ) -> Result<bool, BlockchainError> {
        self.inner
            .compare_and_swap_nonce(account, expected, new_value)
            .await
    }

    /// Get the block version
    fn get_block_version(&self) -> BlockVersion {
        self.block_version
    }

    /// Get the timestamp to use for verification (delegates to inner)
    fn get_verification_timestamp(&self) -> u64 {
        self.inner.get_verification_timestamp()
    }

    async fn set_multisig_state(
        &mut self,
        account: &'a PublicKey,
        config: &MultiSigPayload,
    ) -> Result<(), BlockchainError> {
        self.inner.set_multisig_state(account, config).await
    }

    async fn get_multisig_state(
        &mut self,
        account: &'a PublicKey,
    ) -> Result<Option<&MultiSigPayload>, BlockchainError> {
        self.inner.get_multisig_state(account).await
    }

    async fn get_environment(&mut self) -> Result<&Environment, BlockchainError> {
        self.inner.get_environment().await
    }

    async fn set_contract_module(
        &mut self,
        hash: &Hash,
        module: &'a Module,
    ) -> Result<(), BlockchainError> {
        self.inner.set_contract_module(hash, module).await
    }

    async fn load_contract_module(&mut self, hash: &Hash) -> Result<bool, BlockchainError> {
        self.inner.load_contract_module(hash).await
    }

    async fn get_contract_module_with_environment(
        &self,
        hash: &Hash,
    ) -> Result<(&Module, &Environment), BlockchainError> {
        self.inner.get_contract_module_with_environment(hash).await
    }

    fn get_network(&self) -> tos_common::network::Network {
        self.inner
            .storage
            .get_network()
            .unwrap_or(tos_common::network::Network::Mainnet)
    }

    /// Check if an account is registered (exists) on the blockchain
    async fn is_account_registered(
        &self,
        account: &CompressedPublicKey,
    ) -> Result<bool, BlockchainError> {
        // Delegate to inner ChainState implementation
        self.inner.is_account_registered(account).await
    }

    /// Get account energy for stake 2.0 validation
    async fn get_account_energy(
        &mut self,
        account: &'a CompressedPublicKey,
    ) -> Result<Option<tos_common::account::AccountEnergy>, BlockchainError> {
        self.inner.get_account_energy(account).await
    }

    /// Get a specific delegation from one account to another
    async fn get_delegated_resource(
        &mut self,
        from: &'a CompressedPublicKey,
        to: &'a CompressedPublicKey,
    ) -> Result<Option<tos_common::account::DelegatedResource>, BlockchainError> {
        self.inner.get_delegated_resource(from, to).await
    }

    /// Record a pending undelegation (delegates to inner)
    async fn record_pending_undelegation(
        &mut self,
        from: &'a CompressedPublicKey,
        to: &'a CompressedPublicKey,
        amount: u64,
    ) -> Result<(), BlockchainError> {
        self.inner
            .record_pending_undelegation(from, to, amount)
            .await
    }
}

#[async_trait]
impl<'a, S: Storage> BlockchainApplyState<'a, S, BlockchainError> for ApplicableChainState<'a, S> {
    /// Track burned supply
    async fn add_burned_coins(&mut self, amount: u64) -> Result<(), BlockchainError> {
        self.burned_supply = self
            .burned_supply
            .checked_add(amount)
            .ok_or(BlockchainError::BalanceOverflow)?;
        Ok(())
    }

    /// Track miner fees
    async fn add_gas_fee(&mut self, amount: u64) -> Result<(), BlockchainError> {
        self.gas_fee = self
            .gas_fee
            .checked_add(amount)
            .ok_or(BlockchainError::BalanceOverflow)?;
        Ok(())
    }

    fn get_block_hash(&self) -> &Hash {
        &self.block_hash
    }

    fn get_block(&self) -> &Block {
        self.block
    }

    fn is_mainnet(&self) -> bool {
        self.inner.storage.is_mainnet()
    }

    async fn set_contract_outputs(
        &mut self,
        tx_hash: &'a Hash,
        outputs: Vec<ContractOutput>,
    ) -> Result<(), BlockchainError> {
        match self.contract_manager.outputs.entry(tx_hash.clone()) {
            Entry::Occupied(mut o) => {
                o.get_mut().extend(outputs);
            }
            Entry::Vacant(e) => {
                e.insert(outputs);
            }
        };

        Ok(())
    }

    async fn get_contract_environment_for<'b>(
        &'b mut self,
        contract: &'b Hash,
        deposits: &'b IndexMap<Hash, ContractDeposit>,
        tx_hash: &'b Hash,
    ) -> Result<(ContractEnvironment<'b, S>, ContractChainState<'b>), BlockchainError> {
        // Find the contract module in our cache
        // We don't use the function `get_contract_module_with_environment` because we need to return the mutable storage
        let module = self
            .inner
            .contracts
            .get(contract)
            .ok_or_else(|| BlockchainError::ContractNotFound(contract.clone()))
            .and_then(|(_, module)| {
                module
                    .as_ref()
                    .map(|m| m.as_ref())
                    .ok_or_else(|| BlockchainError::ContractNotFound(contract.clone()))
            })?;

        // Find the contract cache in our cache map
        let mut cache = self
            .contract_manager
            .caches
            .get(contract)
            .cloned()
            .unwrap_or_default();

        // Balance simplification: Add plaintext deposits to contract balances
        for (asset, deposit) in deposits.iter() {
            let amount = deposit.amount();
            match cache.balances.entry(asset.clone()) {
                Entry::Occupied(mut o) => match o.get_mut() {
                    Some((mut state, balance)) => {
                        state.mark_updated();
                        *balance = balance
                            .checked_add(amount)
                            .ok_or(BlockchainError::BalanceOverflow)?;
                    }
                    None => {
                        // Balance was already fetched and we didn't had any balance before
                        o.insert(Some((VersionedState::New, amount)));
                    }
                },
                Entry::Vacant(e) => {
                    let (mut state, balance) = self
                        .storage
                        .get_contract_balance_at_maximum_topoheight(
                            contract,
                            asset,
                            self.topoheight,
                        )
                        .await?
                        .map(|(topo, balance)| (VersionedState::FetchedAt(topo), balance.take()))
                        .unwrap_or((VersionedState::New, 0));

                    state.mark_updated();
                    let new_balance = balance
                        .checked_add(amount)
                        .ok_or(BlockchainError::BalanceOverflow)?;
                    e.insert(Some((state, new_balance)));
                }
            }
        }

        let state = ContractChainState {
            debug_mode: true,
            mainnet: self.inner.storage.is_mainnet(),
            contract,
            topoheight: self.inner.topoheight,
            block_hash: self.block_hash,
            block: self.block,
            deposits,
            random: None,
            tx_hash,
            cache,
            outputs: Vec::new(),
            // Event trackers
            tracker: self.contract_manager.tracker.clone(),
            // Assets cache owned by this contract
            assets: self.contract_manager.assets.clone(),
            // Global caches (all contracts)
            global_caches: &self.contract_manager.caches,
        };

        let contract_environment = ContractEnvironment {
            environment: self.inner.environment,
            module,
            provider: self.inner.storage.as_mut(),
        };

        Ok((contract_environment, state))
    }

    async fn merge_contract_changes(
        &mut self,
        hash: &Hash,
        cache: ContractCache,
        tracker: ContractEventTracker,
        assets: HashMap<Hash, Option<AssetChanges>>,
    ) -> Result<(), BlockchainError> {
        // Insert or update cache
        self.contract_manager.caches.insert(hash.clone(), cache);

        self.contract_manager.tracker = tracker;
        self.contract_manager.assets = assets;

        Ok(())
    }

    async fn remove_contract_module(&mut self, hash: &Hash) -> Result<(), BlockchainError> {
        self.remove_contract_module_internal(hash).await
    }

    // Note: get_account_energy is inherited from BlockchainVerificationState

    async fn set_account_energy(
        &mut self,
        account: &'a CompressedPublicKey,
        account_energy: AccountEnergy,
    ) -> Result<(), BlockchainError> {
        self.inner
            .storage
            .set_account_energy(account, self.inner.topoheight, &account_energy)
            .await
    }

    async fn get_global_energy_state(
        &mut self,
    ) -> Result<tos_common::account::GlobalEnergyState, BlockchainError> {
        self.inner.storage.get_global_energy_state().await
    }

    async fn set_global_energy_state(
        &mut self,
        mut state: tos_common::account::GlobalEnergyState,
    ) -> Result<(), BlockchainError> {
        // Automatically update last_update to current topoheight
        state.last_update = self.inner.topoheight;
        self.inner.storage.set_global_energy_state(&state).await
    }

    // Note: get_delegated_resource is inherited from BlockchainVerificationState

    async fn set_delegated_resource(
        &mut self,
        delegation: &tos_common::account::DelegatedResource,
    ) -> Result<(), BlockchainError> {
        self.inner.storage.set_delegated_resource(delegation).await
    }

    async fn delete_delegated_resource(
        &mut self,
        from: &'a CompressedPublicKey,
        to: &'a CompressedPublicKey,
    ) -> Result<(), BlockchainError> {
        self.inner.storage.delete_delegated_resource(from, to).await
    }

    async fn get_ai_mining_state(&mut self) -> Result<Option<AIMiningState>, BlockchainError> {
        self.inner.storage.get_ai_mining_state().await
    }

    async fn set_ai_mining_state(&mut self, state: &AIMiningState) -> Result<(), BlockchainError> {
        self.inner
            .storage
            .set_ai_mining_state(self.inner.topoheight, state)
            .await
    }

    fn get_contract_executor(&self) -> std::sync::Arc<dyn tos_common::contract::ContractExecutor> {
        self.executor.clone()
    }

    async fn bind_referrer(
        &mut self,
        user: &'a CompressedPublicKey,
        referrer: &'a CompressedPublicKey,
        tx_hash: &'a Hash,
    ) -> Result<(), BlockchainError> {
        // Note: PublicKey is an alias for CompressedPublicKey, so no conversion needed
        // Get current timestamp from block
        let timestamp = self.block.get_timestamp() / 1000; // Convert ms to seconds

        // Call the ReferralProvider implementation
        self.inner
            .storage
            .bind_referrer(
                user,
                referrer,
                self.inner.topoheight,
                tx_hash.clone(),
                timestamp,
            )
            .await
    }

    async fn distribute_referral_rewards(
        &mut self,
        from_user: &'a CompressedPublicKey,
        asset: &'a Hash,
        total_amount: u64,
        ratios: &[u16],
    ) -> Result<tos_common::referral::DistributionResult, BlockchainError> {
        // Note: PublicKey is an alias for CompressedPublicKey, so no conversion needed
        // Build ReferralRewardRatios from the slice
        let reward_ratios = tos_common::referral::ReferralRewardRatios {
            ratios: ratios.to_vec(),
        };

        // Call the ReferralProvider implementation
        self.inner
            .storage
            .distribute_to_uplines(from_user, asset.clone(), total_amount, &reward_ratios)
            .await
    }

    async fn add_contract_events(
        &mut self,
        events: Vec<tos_common::contract::ContractEvent>,
        contract: &Hash,
        tx_hash: &'a Hash,
    ) -> Result<(), BlockchainError> {
        // Contract events are logged but not persisted in this simplified version
        // In the full version, these would be stored for event filtering
        let event_count = events.len();
        if event_count > 0 {
            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "Contract {} emitted {} events in TX {} (not persisted)",
                    contract, event_count, tx_hash
                );
            }
        }
        Ok(())
    }

    // ===== KYC System Operations =====

    async fn set_kyc(
        &mut self,
        user: &'a CompressedPublicKey,
        level: u16,
        verified_at: u64,
        data_hash: &'a Hash,
        committee_id: &'a Hash,
        tx_hash: &'a Hash,
    ) -> Result<(), BlockchainError> {
        use tos_common::kyc::KycData;

        // Create KYC data (KycData::new takes level, verified_at, data_hash)
        let kyc_data = KycData::new(level, verified_at, data_hash.clone());

        // Call the KycProvider implementation
        self.inner
            .storage
            .set_kyc(user, kyc_data, committee_id, self.inner.topoheight, tx_hash)
            .await
    }

    async fn revoke_kyc(
        &mut self,
        user: &'a CompressedPublicKey,
        reason_hash: &'a Hash,
        tx_hash: &'a Hash,
    ) -> Result<(), BlockchainError> {
        // Call the KycProvider implementation
        self.inner
            .storage
            .revoke_kyc(user, reason_hash, self.inner.topoheight, tx_hash)
            .await
    }

    async fn renew_kyc(
        &mut self,
        user: &'a CompressedPublicKey,
        verified_at: u64,
        data_hash: &'a Hash,
        tx_hash: &'a Hash,
    ) -> Result<(), BlockchainError> {
        // Call the KycProvider implementation
        self.inner
            .storage
            .renew_kyc(
                user,
                verified_at,
                data_hash.clone(),
                self.inner.topoheight,
                tx_hash,
            )
            .await
    }

    async fn transfer_kyc(
        &mut self,
        user: &'a CompressedPublicKey,
        _source_committee_id: &'a Hash,
        dest_committee_id: &'a Hash,
        new_data_hash: &'a Hash,
        transferred_at: u64,
        tx_hash: &'a Hash,
        dest_max_kyc_level: u16,
        verification_timestamp: u64,
    ) -> Result<(), BlockchainError> {
        // Transfer KYC to a new committee
        // This updates the committee_id while preserving the user's KYC level
        // The source_committee validation is done at verification time
        self.inner
            .storage
            .transfer_kyc(
                user,
                dest_committee_id,
                new_data_hash.clone(),
                transferred_at,
                self.inner.topoheight,
                tx_hash,
                dest_max_kyc_level,
                verification_timestamp,
            )
            .await
    }

    async fn emergency_suspend_kyc(
        &mut self,
        user: &'a CompressedPublicKey,
        reason_hash: &'a Hash,
        expires_at: u64,
        tx_hash: &'a Hash,
    ) -> Result<(), BlockchainError> {
        // Call the KycProvider implementation
        self.inner
            .storage
            .emergency_suspend(
                user,
                reason_hash,
                expires_at,
                self.inner.topoheight,
                tx_hash,
            )
            .await
    }

    async fn submit_kyc_appeal(
        &mut self,
        user: &'a CompressedPublicKey,
        original_committee_id: &'a Hash,
        parent_committee_id: &'a Hash,
        reason_hash: &'a Hash,
        documents_hash: &'a Hash,
        submitted_at: u64,
        tx_hash: &'a Hash,
    ) -> Result<(), BlockchainError> {
        // Call the KycProvider implementation
        self.inner
            .storage
            .submit_appeal(
                user,
                original_committee_id,
                parent_committee_id,
                reason_hash,
                documents_hash,
                submitted_at,
                self.inner.topoheight,
                tx_hash,
            )
            .await
    }

    async fn bootstrap_global_committee(
        &mut self,
        name: String,
        members: Vec<tos_common::kyc::CommitteeMemberInfo>,
        threshold: u8,
        kyc_threshold: u8,
        max_kyc_level: u16,
        tx_hash: &'a Hash,
    ) -> Result<Hash, BlockchainError> {
        use tos_common::kyc::SecurityCommittee;

        // Get current timestamp from block
        let timestamp = self.block.get_timestamp() / 1000;

        // Convert member info to full committee members
        let committee_members: Vec<_> = members
            .into_iter()
            .map(|m| m.into_member(timestamp))
            .collect();

        // Create the Global Committee
        let committee = SecurityCommittee::new_global(
            name,
            committee_members,
            threshold,
            kyc_threshold,
            max_kyc_level,
            timestamp,
        );

        // Call the CommitteeProvider implementation
        self.inner
            .storage
            .bootstrap_global_committee(committee, self.inner.topoheight, tx_hash)
            .await
    }

    async fn register_committee(
        &mut self,
        name: String,
        region: tos_common::kyc::KycRegion,
        members: Vec<tos_common::kyc::CommitteeMemberInfo>,
        threshold: u8,
        kyc_threshold: u8,
        max_kyc_level: u16,
        parent_id: &'a Hash,
        tx_hash: &'a Hash,
    ) -> Result<Hash, BlockchainError> {
        use tos_common::kyc::SecurityCommittee;

        // Get current timestamp from block
        let timestamp = self.block.get_timestamp() / 1000;

        // Convert member info to full committee members
        let committee_members: Vec<_> = members
            .into_iter()
            .map(|m| m.into_member(timestamp))
            .collect();

        // Create the regional committee
        let committee = SecurityCommittee::new_regional(
            name,
            region,
            committee_members,
            threshold,
            kyc_threshold,
            max_kyc_level,
            parent_id.clone(),
            timestamp,
        );

        // Call the CommitteeProvider implementation
        self.inner
            .storage
            .register_committee(committee, parent_id, self.inner.topoheight, tx_hash)
            .await
    }

    async fn update_committee(
        &mut self,
        committee_id: &'a Hash,
        update: &tos_common::transaction::CommitteeUpdateData,
    ) -> Result<(), BlockchainError> {
        use tos_common::kyc::CommitteeStatus;
        use tos_common::transaction::CommitteeUpdateData;

        match update {
            CommitteeUpdateData::AddMember {
                public_key,
                name,
                role,
            } => {
                self.inner
                    .storage
                    .add_committee_member(
                        committee_id,
                        public_key,
                        name.clone(),
                        *role,
                        self.inner.topoheight,
                    )
                    .await
            }
            CommitteeUpdateData::RemoveMember { public_key } => {
                self.inner
                    .storage
                    .remove_committee_member(committee_id, public_key, self.inner.topoheight)
                    .await
            }
            CommitteeUpdateData::UpdateMemberRole {
                public_key,
                new_role,
            } => {
                self.inner
                    .storage
                    .update_member_role(committee_id, public_key, *new_role, self.inner.topoheight)
                    .await
            }
            CommitteeUpdateData::UpdateMemberStatus {
                public_key,
                new_status,
            } => {
                self.inner
                    .storage
                    .update_member_status(
                        committee_id,
                        public_key,
                        *new_status,
                        self.inner.topoheight,
                    )
                    .await
            }
            CommitteeUpdateData::UpdateThreshold { new_threshold } => {
                self.inner
                    .storage
                    .update_committee_threshold(committee_id, *new_threshold, self.inner.topoheight)
                    .await
            }
            CommitteeUpdateData::UpdateKycThreshold { new_kyc_threshold } => {
                self.inner
                    .storage
                    .update_committee_kyc_threshold(
                        committee_id,
                        *new_kyc_threshold,
                        self.inner.topoheight,
                    )
                    .await
            }
            CommitteeUpdateData::UpdateName { new_name } => {
                self.inner
                    .storage
                    .update_committee_name(committee_id, new_name.clone(), self.inner.topoheight)
                    .await
            }
            CommitteeUpdateData::SuspendCommittee => {
                self.inner
                    .storage
                    .update_committee_status(
                        committee_id,
                        CommitteeStatus::Suspended,
                        self.inner.topoheight,
                    )
                    .await
            }
            CommitteeUpdateData::ActivateCommittee => {
                self.inner
                    .storage
                    .update_committee_status(
                        committee_id,
                        CommitteeStatus::Active,
                        self.inner.topoheight,
                    )
                    .await
            }
        }
    }

    async fn get_committee(
        &self,
        committee_id: &'a Hash,
    ) -> Result<Option<tos_common::kyc::SecurityCommittee>, BlockchainError> {
        self.inner.storage.get_committee(committee_id).await
    }

    async fn get_verifying_committee(
        &self,
        user: &'a CompressedPublicKey,
    ) -> Result<Option<Hash>, BlockchainError> {
        self.inner.storage.get_verifying_committee(user).await
    }

    async fn get_kyc_status(
        &self,
        user: &'a CompressedPublicKey,
    ) -> Result<Option<tos_common::kyc::KycStatus>, BlockchainError> {
        let kyc_data = self.inner.storage.get_kyc(user).await?;
        Ok(kyc_data.map(|d| d.status))
    }

    async fn get_kyc_level(
        &self,
        user: &'a CompressedPublicKey,
    ) -> Result<Option<u16>, BlockchainError> {
        let kyc_data = self.inner.storage.get_kyc(user).await?;
        Ok(kyc_data.map(|d| d.level))
    }

    async fn is_global_committee_bootstrapped(&self) -> Result<bool, BlockchainError> {
        self.inner.storage.is_global_committee_bootstrapped().await
    }

    // ===== Transaction Result Storage (Stake 2.0) =====

    async fn set_transaction_result(
        &mut self,
        tx_hash: &'a Hash,
        result: &tos_common::transaction::TransactionResult,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "set transaction result {}: fee={}, energy_used={}",
                tx_hash,
                result.fee,
                result.energy_used
            );
        }
        self.inner
            .storage
            .set_transaction_result(tx_hash, result)
            .await
    }
}

impl<'a, S: Storage> Deref for ApplicableChainState<'a, S> {
    type Target = ChainState<'a, S>;

    fn deref(&self) -> &ChainState<'a, S> {
        &self.inner
    }
}

impl<'a, S: Storage> DerefMut for ApplicableChainState<'a, S> {
    fn deref_mut(&mut self) -> &mut ChainState<'a, S> {
        &mut self.inner
    }
}

impl<'a, S: Storage> AsRef<ChainState<'a, S>> for ApplicableChainState<'a, S> {
    fn as_ref(&self) -> &ChainState<'a, S> {
        &self.inner
    }
}

impl<'a, S: Storage> AsMut<ChainState<'a, S>> for ApplicableChainState<'a, S> {
    fn as_mut(&mut self) -> &mut ChainState<'a, S> {
        &mut self.inner
    }
}

impl<'a, S: Storage> ApplicableChainState<'a, S> {
    pub fn new(
        storage: &'a mut S,
        environment: &'a Environment,
        stable_topoheight: TopoHeight,
        topoheight: TopoHeight,
        block_version: BlockVersion,
        burned_supply: u64,
        block_hash: &'a Hash,
        block: &'a Block,
        executor: std::sync::Arc<dyn tos_common::contract::ContractExecutor>,
    ) -> Self {
        // Use block timestamp for deterministic consensus validation
        let block_timestamp_secs = block.get_header().get_timestamp() / 1000;
        Self {
            inner: ChainState::with(
                StorageReference::Mutable(storage),
                environment,
                stable_topoheight,
                topoheight,
                block_version,
                Some(block_timestamp_secs),
            ),
            burned_supply,
            contract_manager: ContractManager {
                outputs: HashMap::new(),
                caches: HashMap::new(),
                assets: HashMap::new(),
                tracker: ContractEventTracker::default(),
                scheduled_executions: Vec::new(),
            },
            block_hash,
            block,
            gas_fee: 0,
            executor,
        }
    }

    // Get the storage used by the chain state
    pub fn get_mut_storage(&mut self) -> &mut S {
        self.inner.storage.as_mut()
    }

    // Get the contracts cache
    pub fn get_contracts_cache(&self) -> &HashMap<Hash, ContractCache> {
        &self.contract_manager.caches
    }

    // Get the contract tracker
    pub fn get_contract_tracker(&self) -> &ContractEventTracker {
        &self.contract_manager.tracker
    }

    // Get the contract outputs for TX
    pub fn get_contract_outputs_for_tx(&self, tx_hash: &Hash) -> Option<&Vec<ContractOutput>> {
        self.contract_manager.outputs.get(tx_hash)
    }

    // Get the total amount of burned coins
    pub fn get_burned_supply(&self) -> u64 {
        self.burned_supply
    }

    /// Register a scheduled execution to be stored when apply_changes is called
    ///
    /// This is used during OFFERCALL syscall processing to schedule future contract executions.
    /// The execution will be stored in the scheduled execution storage and processed at the
    /// target topoheight.
    ///
    /// # Arguments
    ///
    /// * `contract` - The contract that scheduled this execution
    /// * `execution_topoheight` - The topoheight at which the execution should run
    /// * `execution` - The scheduled execution details
    pub fn register_scheduled_execution(
        &mut self,
        contract: Hash,
        execution_topoheight: TopoHeight,
        execution: ScheduledExecution,
    ) {
        self.contract_manager.scheduled_executions.push((
            contract,
            execution_topoheight,
            execution,
        ));
    }

    /// Get the number of scheduled executions registered in this block
    pub fn get_scheduled_execution_count(&self) -> usize {
        self.contract_manager.scheduled_executions.len()
    }

    async fn remove_contract_module_internal(
        &mut self,
        hash: &Hash,
    ) -> Result<(), BlockchainError> {
        let (state, contract) = self
            .inner
            .contracts
            .get_mut(hash)
            .ok_or_else(|| BlockchainError::ContractNotFound(hash.clone()))?;

        state.mark_updated();
        *contract = None;

        Ok(())
    }

    // This function is called after the verification of all needed transactions
    // This will consume ChainState and apply all changes to the storage
    // In case of incoming and outgoing transactions in same state, the final balance will be computed
    pub async fn apply_changes(mut self) -> Result<(), BlockchainError> {
        // Apply changes for sender accounts
        for (key, account) in &mut self.inner.accounts {
            if log::log_enabled!(log::Level::Trace) {
                trace!(
                    "Saving nonce {} for {} at topoheight {}",
                    account.nonce,
                    key.as_address(self.inner.storage.is_mainnet()),
                    self.inner.topoheight
                );
            }
            self.inner
                .storage
                .set_last_nonce_to(key, self.inner.topoheight, &account.nonce)
                .await?;

            // Save the multisig state if needed
            if let Some((state, multisig)) = account
                .multisig
                .as_ref()
                .filter(|(state, _)| state.should_be_stored())
            {
                if log::log_enabled!(log::Level::Trace) {
                    trace!(
                        "Saving multisig for {} at topoheight {}",
                        key.as_address(self.inner.storage.is_mainnet()),
                        self.inner.topoheight
                    );
                }
                let multisig = multisig.as_ref().map(|v| Cow::Borrowed(v));
                let versioned = VersionedMultiSig::new(multisig, state.get_topoheight());
                self.inner
                    .storage
                    .set_last_multisig_to(key, self.inner.topoheight, versioned)
                    .await?;
            }

            let balances = self
                .inner
                .receiver_balances
                .entry(Cow::Borrowed(key))
                .or_insert_with(HashMap::new);
            // Because account balances are only used to verify the validity of ZK Proofs, we can't store them
            // We have to recompute the final balance for each asset using the existing current balance
            // Otherwise, we could have a front running problem
            // Example: Alice sends 100 to Bob, Bob sends 100 to Charlie
            // But Bob built its ZK Proof with the balance before Alice's transaction
            for (asset, echange) in account.assets.drain() {
                if log::log_enabled!(log::Level::Trace) {
                    trace!(
                        "{} {} updated for {} at topoheight {}",
                        echange.version,
                        asset,
                        key.as_address(self.inner.storage.is_mainnet()),
                        self.inner.topoheight
                    );
                }
                let Echange {
                    mut version,
                    output_sum,
                    output_balance_used,
                    new_version,
                    ..
                } = echange;
                if log::log_enabled!(log::Level::Trace) {
                    trace!("sender output sum: {}", output_sum);
                }
                match balances.entry(Cow::Borrowed(asset)) {
                    Entry::Occupied(mut o) => {
                        if log::log_enabled!(log::Level::Trace) {
                            trace!(
                                "{} already has a balance for {} at topoheight {}",
                                key.as_address(self.inner.storage.is_mainnet()),
                                asset,
                                self.inner.topoheight
                            );
                        }
                        // We got incoming funds while spending some
                        // We need to split the version in two
                        // Output balance is the balance after outputs spent without incoming funds
                        // Final balance is the balance after incoming funds + outputs spent
                        // This is a necessary process for the following case:
                        // Alice sends 100 to Bob in block 1000
                        // But Bob build 2 txs before Alice, one to Charlie and one to David
                        // First Tx of Blob is in block 1000, it will be valid
                        // But because of Alice incoming, the second Tx of Bob will be invalid
                        let final_version = o.get_mut();

                        // We got input and output funds, mark it
                        final_version.set_balance_type(BalanceType::Both);

                        // We must build output balance correctly
                        // For that, we use the same balance before any inputs
                        // And deduct outputs
                        // let clean_version = self.storage.get_new_versioned_balance(key, asset, self.topoheight).await?;
                        // let mut output_balance = clean_version.take_balance();
                        // *output_balance.computable()? -= &output_sum;

                        // Determine which balance to use as next output balance
                        // This is used in case TXs that are built at same reference, but
                        // executed in differents topoheights have the output balance reported
                        // to the next topoheight each time to stay valid during ZK Proof verification
                        let output_balance = version.take_balance_with(output_balance_used);

                        // Set to our final version the new output balance
                        final_version.set_output_balance(Some(output_balance));

                        // Build the final balance
                        // All inputs are already added, we just need to substract the outputs
                        let final_balance = final_version.get_mut_balance();
                        *final_balance -= output_sum;
                    }
                    Entry::Vacant(e) => {
                        if log::log_enabled!(log::Level::Trace) {
                            trace!(
                                "{} has no balance for {} at topoheight {}",
                                key.as_address(self.inner.storage.is_mainnet()),
                                asset,
                                self.inner.topoheight
                            );
                        }
                        // We have no incoming update for this key
                        // Select the right final version
                        // For that, we must check if we used the output balance and/or if we are not on the last version
                        let version = if output_balance_used || !new_version {
                            // We must fetch again the version to sum it with the output
                            // This is necessary to build the final balance
                            let (mut new_version, _) = self
                                .inner
                                .storage
                                .get_new_versioned_balance(key, asset, self.inner.topoheight)
                                .await?;
                            // Substract the output sum
                            if log::log_enabled!(log::Level::Trace) {
                                trace!(
                                    "{} has no balance for {} at topoheight {}, substract output sum",
                                    key.as_address(self.inner.storage.is_mainnet()),
                                    asset,
                                    self.inner.topoheight
                                );
                            }
                            *new_version.get_mut_balance() = new_version
                                .get_mut_balance()
                                .checked_sub(output_sum)
                                .ok_or(BlockchainError::Overflow)?;

                            // Report the output balance to the next topoheight
                            // So the edge case where:
                            // Balance at topo 1000 is referenced
                            // Balance updated at topo 1001 as input
                            // TX A is built with reference 1000 but executed at topo 1002
                            // TX B reference 1000 but output balance is at topo 1002 and it include the final balance of (TX A + input at 1001)
                            // So we report the output balance for next TX verification
                            new_version.set_output_balance(Some(
                                version.take_balance_with(output_balance_used),
                            ));
                            new_version.set_balance_type(BalanceType::Both);

                            new_version
                        } else {
                            // BLOCKDAG alignment: Balance deduction now happens in apply_without_verify,
                            // so version.balance already has the correct deducted balance.
                            // We just need to set the balance type for storage.
                            //
                            // Original BLOCKDAG behavior: apply_without_verify subtracts output before
                            // calling add_sender_output, so the version stored here is already correct.
                            version.set_balance_type(BalanceType::Output);
                            version
                        };

                        // We have some output, mark it

                        e.insert(version);
                    }
                }
            }

            // Process UNO (encrypted) sender assets
            // Similar to plaintext assets but with homomorphic ciphertext operations
            let uno_balances = self
                .inner
                .receiver_uno_balances
                .entry(Cow::Borrowed(key))
                .or_insert_with(HashMap::new);

            for (asset, uno_echange) in account.uno_assets.drain() {
                if log::log_enabled!(log::Level::Trace) {
                    trace!(
                        "UNO {} updated for {} at topoheight {}",
                        asset,
                        key.as_address(self.inner.storage.is_mainnet()),
                        self.inner.topoheight
                    );
                }
                let super::UnoEchange {
                    mut version,
                    output_sum,
                    output_balance_used,
                    new_version,
                    ..
                } = uno_echange;

                match uno_balances.entry(Cow::Borrowed(asset)) {
                    Entry::Occupied(mut o) => {
                        if log::log_enabled!(log::Level::Trace) {
                            trace!(
                                "{} already has UNO balance for {} at topoheight {}",
                                key.as_address(self.inner.storage.is_mainnet()),
                                asset,
                                self.inner.topoheight
                            );
                        }
                        // We got incoming funds while spending some
                        let final_version = o.get_mut();
                        final_version.set_balance_type(BalanceType::Both);

                        // Set output balance from sender's version
                        let output_balance = version.take_balance_with(output_balance_used);
                        final_version.set_output_balance(Some(output_balance));

                        // Subtract output_sum from final balance (homomorphic subtraction)
                        final_version.sub_ciphertext_from_balance(&output_sum)?;
                    }
                    Entry::Vacant(e) => {
                        if log::log_enabled!(log::Level::Trace) {
                            trace!(
                                "{} has no UNO balance for {} at topoheight {}",
                                key.as_address(self.inner.storage.is_mainnet()),
                                asset,
                                self.inner.topoheight
                            );
                        }
                        let version = if output_balance_used || !new_version {
                            let (mut new_version, _) = self
                                .inner
                                .storage
                                .get_new_versioned_uno_balance(key, asset, self.inner.topoheight)
                                .await?;
                            // Subtract output_sum (homomorphic subtraction)
                            new_version.sub_ciphertext_from_balance(&output_sum)?;

                            // Report output balance for next TX verification
                            new_version.set_output_balance(Some(
                                version.take_balance_with(output_balance_used),
                            ));
                            new_version.set_balance_type(BalanceType::Both);
                            new_version
                        } else {
                            version.set_balance_type(BalanceType::Output);
                            version
                        };
                        e.insert(version);
                    }
                }
            }
        }

        // Apply the assets
        for (asset, changes) in self.contract_manager.assets {
            if let Some(changes) = changes {
                let (state, data) = changes.data;
                if state.should_be_stored() {
                    if log::log_enabled!(log::Level::Trace) {
                        trace!(
                            "Saving asset {} at topoheight {}",
                            asset,
                            self.inner.topoheight
                        );
                    }
                    self.inner
                        .storage
                        .add_asset(
                            &asset,
                            self.inner.topoheight,
                            VersionedAssetData::new(data, state.get_topoheight()),
                        )
                        .await?;
                }

                if let Some((state, supply)) = changes.supply {
                    if state.should_be_stored() {
                        if log::log_enabled!(log::Level::Trace) {
                            trace!(
                                "Saving supply {} for {} at topoheight {}",
                                supply,
                                asset,
                                self.inner.topoheight
                            );
                        }
                        self.inner
                            .storage
                            .set_last_supply_for_asset(
                                &asset,
                                self.inner.topoheight,
                                &VersionedSupply::new(supply, state.get_topoheight()),
                            )
                            .await?;
                    }
                }
            }
        }

        // Start by storing the contracts
        debug!("Storing contracts");
        for (hash, (state, module)) in self.inner.contracts.iter() {
            if state.should_be_stored() {
                if log::log_enabled!(log::Level::Trace) {
                    trace!(
                        "Saving contract {} at topoheight {}",
                        hash,
                        self.inner.topoheight
                    );
                }
                // Prevent cloning the value
                let module = module.as_ref().map(|v| Cow::Borrowed(v.as_ref()));
                self.inner
                    .storage
                    .set_last_contract_to(
                        &hash,
                        self.inner.topoheight,
                        &VersionedContract::new(module, state.get_topoheight()),
                    )
                    .await?;
            }
        }

        debug!("Storing contract storage changes");
        // Apply all the contract storage changes
        for (contract, cache) in self.contract_manager.caches {
            // Apply all storage changes
            for (key, (state, value)) in cache.storage {
                if state.should_be_stored() {
                    if log::log_enabled!(log::Level::Trace) {
                        trace!(
                            "Saving contract data {} key {} at topoheight {}",
                            contract,
                            key,
                            self.inner.topoheight
                        );
                    }
                    self.inner
                        .storage
                        .set_last_contract_data_to(
                            &contract,
                            &key,
                            self.inner.topoheight,
                            &VersionedContractData::new(value, state.get_topoheight()),
                        )
                        .await?;
                }
            }

            for (asset, data) in cache.balances {
                if let Some((state, balance)) = data {
                    if state.should_be_stored() {
                        if log::log_enabled!(log::Level::Trace) {
                            trace!(
                                "Saving contract balance {} for {} at topoheight {}",
                                balance,
                                asset,
                                self.inner.topoheight
                            );
                        }
                        self.inner
                            .storage
                            .set_last_contract_balance_to(
                                &contract,
                                &asset,
                                self.inner.topoheight,
                                VersionedContractBalance::new(balance, state.get_topoheight()),
                            )
                            .await?;
                    }
                }
            }
        }

        debug!("applying external transfers");
        // Apply all the transfers to the receiver accounts
        for (key, assets) in self.contract_manager.tracker.transfers {
            for (asset, amount) in assets {
                if log::log_enabled!(log::Level::Trace) {
                    trace!(
                        "Transfering {} {} to {} at topoheight {}",
                        amount,
                        asset,
                        key.as_address(self.inner.storage.is_mainnet()),
                        self.inner.topoheight
                    );
                }
                let receiver_balance = self
                    .inner
                    .internal_get_receiver_balance(Cow::Owned(key.clone()), Cow::Owned(asset))
                    .await?;
                *receiver_balance += amount;
            }
        }

        // Apply all the contract outputs
        debug!("storing contract outputs");
        for (key, outputs) in self.contract_manager.outputs.drain() {
            self.inner
                .storage
                .set_contract_outputs_for_tx(&key, &outputs)
                .await?;
        }

        // Store scheduled executions registered during this block
        if !self.contract_manager.scheduled_executions.is_empty() {
            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "storing {} scheduled executions",
                    self.contract_manager.scheduled_executions.len()
                );
            }
            for (contract, execution_topoheight, execution) in
                self.contract_manager.scheduled_executions.drain(..)
            {
                if log::log_enabled!(log::Level::Trace) {
                    trace!(
                        "Storing scheduled execution for contract {} at topoheight {}",
                        contract,
                        execution_topoheight
                    );
                }
                self.inner
                    .storage
                    .set_contract_scheduled_execution_at_topoheight(
                        &contract,
                        self.inner.topoheight,
                        &execution,
                        execution_topoheight,
                    )
                    .await?;
            }
        }

        // Apply all balances changes at topoheight
        // We injected the sender balances in the receiver balances previously
        for (account, balances) in self.inner.receiver_balances {
            // If the account has no nonce set, set it to 0
            if !self.inner.accounts.contains_key(account.as_ref())
                && !self.inner.storage.has_nonce(&account).await?
            {
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "{} has now a balance but without any nonce registered, set default (0) nonce",
                        account.as_address(self.inner.storage.is_mainnet())
                    );
                }
                self.inner
                    .storage
                    .set_last_nonce_to(
                        &account,
                        self.inner.topoheight,
                        &VersionedNonce::new(0, None),
                    )
                    .await?;
            }

            // Mark it as registered at this topoheight
            if !self
                .inner
                .storage
                .is_account_registered_for_topoheight(&account, self.inner.topoheight)
                .await?
            {
                self.inner
                    .storage
                    .set_account_registration_topoheight(&account, self.inner.topoheight)
                    .await?;
            }

            for (asset, version) in balances {
                if log::log_enabled!(log::Level::Trace) {
                    trace!(
                        "Saving versioned balance {} for {} at topoheight {}",
                        version,
                        account.as_address(self.inner.storage.is_mainnet()),
                        self.inner.topoheight
                    );
                }
                self.inner
                    .storage
                    .set_last_balance_to(&account, &asset, self.inner.topoheight, &version)
                    .await?;
            }
        }

        // Apply all UNO (encrypted) balance changes at topoheight
        // Similar to plaintext balances but stored in UNO-specific columns
        for (account, uno_balances) in self.inner.receiver_uno_balances {
            // UNO balances: If account has no nonce, set default nonce
            if !self.inner.accounts.contains_key(account.as_ref())
                && !self.inner.storage.has_nonce(&account).await?
            {
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "{} has now a UNO balance but without any nonce registered, set default (0) nonce",
                        account.as_address(self.inner.storage.is_mainnet())
                    );
                }
                self.inner
                    .storage
                    .set_last_nonce_to(
                        &account,
                        self.inner.topoheight,
                        &VersionedNonce::new(0, None),
                    )
                    .await?;
            }

            // Mark as registered at this topoheight
            if !self
                .inner
                .storage
                .is_account_registered_for_topoheight(&account, self.inner.topoheight)
                .await?
            {
                self.inner
                    .storage
                    .set_account_registration_topoheight(&account, self.inner.topoheight)
                    .await?;
            }

            // UNO is a single asset, but we iterate to support the data structure
            for (asset, version) in uno_balances {
                if log::log_enabled!(log::Level::Trace) {
                    trace!(
                        "Saving versioned UNO balance for {} at topoheight {}",
                        account.as_address(self.inner.storage.is_mainnet()),
                        self.inner.topoheight
                    );
                }
                self.inner
                    .storage
                    .set_last_uno_balance_to(&account, &asset, self.inner.topoheight, &version)
                    .await?;
            }
        }

        Ok(())
    }
}
