#![allow(clippy::disallowed_methods)]

//! KYC E2E Tests
//!
//! Comprehensive tests for the KYC (Know Your Customer) system operations including:
//! - Global committee bootstrap
//! - Regional committee registration
//! - KYC set/renew/revoke operations
//! - KYC transfer across regions
//! - Emergency suspend operations
//! - Committee updates

use std::{borrow::Cow, collections::HashMap, sync::Arc};

use async_trait::async_trait;
use indexmap::{IndexMap, IndexSet};
use tos_common::{
    account::{AccountEnergy, Nonce},
    block::{Block, BlockHeader, BlockVersion, EXTRA_NONCE_SIZE},
    contract::{
        AssetChanges, ChainState as ContractChainState, ContractCache, ContractEvent,
        ContractEventTracker, ContractExecutor, ContractOutput, ContractStorage,
    },
    crypto::{
        elgamal::{Ciphertext, CompressedPublicKey},
        Hash, Hashable, KeyPair, PublicKey,
    },
    immutable::Immutable,
    kyc::{CommitteeMember, CommitteeStatus, KycRegion, KycStatus, MemberRole, SecurityCommittee},
    network::Network,
    referral::DistributionResult,
    transaction::{
        verify::{BlockchainApplyState, BlockchainVerificationState, ContractEnvironment},
        CommitteeUpdateData, ContractDeposit, MultiSigPayload, Reference, Transaction,
    },
};
use tos_daemon::tako_integration::TakoContractExecutor;
use tos_environment::Environment;
use tos_kernel::Module;

// ============================================================================
// Test Error Type
// ============================================================================

#[derive(Debug)]
enum TestError {
    Unsupported,
    Overflow,
    KycNotFound,
    CommitteeNotFound,
    GlobalCommitteeAlreadyBootstrapped,
    InvalidStatus,
    KycLevelExceedsMax,
}

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestError::Unsupported => write!(f, "Unsupported"),
            TestError::Overflow => write!(f, "Overflow"),
            TestError::KycNotFound => write!(f, "KYC not found"),
            TestError::CommitteeNotFound => write!(f, "Committee not found"),
            TestError::GlobalCommitteeAlreadyBootstrapped => {
                write!(f, "Global committee already bootstrapped")
            }
            TestError::InvalidStatus => write!(f, "Invalid status"),
            TestError::KycLevelExceedsMax => {
                write!(f, "KYC level exceeds destination committee max")
            }
        }
    }
}

impl std::error::Error for TestError {}

// ============================================================================
// Dummy Contract Provider
// ============================================================================

#[derive(Default)]
struct DummyContractProvider;

impl ContractStorage for DummyContractProvider {
    fn load_data(
        &self,
        _contract: &Hash,
        _key: &tos_kernel::ValueCell,
        _topoheight: u64,
    ) -> Result<Option<(u64, Option<tos_kernel::ValueCell>)>, anyhow::Error> {
        Ok(None)
    }

    fn load_data_latest_topoheight(
        &self,
        _contract: &Hash,
        _key: &tos_kernel::ValueCell,
        _topoheight: u64,
    ) -> Result<Option<u64>, anyhow::Error> {
        Ok(None)
    }

    fn has_data(
        &self,
        _contract: &Hash,
        _key: &tos_kernel::ValueCell,
        _topoheight: u64,
    ) -> Result<bool, anyhow::Error> {
        Ok(false)
    }

    fn has_contract(&self, _contract: &Hash, _topoheight: u64) -> Result<bool, anyhow::Error> {
        Ok(false)
    }
}

impl tos_common::contract::ContractProvider for DummyContractProvider {
    fn get_contract_balance_for_asset(
        &self,
        _contract: &Hash,
        _asset: &Hash,
        _topoheight: u64,
    ) -> Result<Option<(u64, u64)>, anyhow::Error> {
        Ok(None)
    }

    fn get_account_balance_for_asset(
        &self,
        _key: &PublicKey,
        _asset: &Hash,
        _topoheight: u64,
    ) -> Result<Option<(u64, u64)>, anyhow::Error> {
        Ok(None)
    }

    fn asset_exists(&self, _asset: &Hash, _topoheight: u64) -> Result<bool, anyhow::Error> {
        Ok(false)
    }

    fn load_asset_data(
        &self,
        _asset: &Hash,
        _topoheight: u64,
    ) -> Result<Option<(u64, tos_common::asset::AssetData)>, anyhow::Error> {
        Ok(None)
    }

    fn load_asset_supply(
        &self,
        _asset: &Hash,
        _topoheight: u64,
    ) -> Result<Option<(u64, u64)>, anyhow::Error> {
        Ok(None)
    }

    fn account_exists(&self, _key: &PublicKey, _topoheight: u64) -> Result<bool, anyhow::Error> {
        Ok(true)
    }

    fn load_contract_module(
        &self,
        _contract: &Hash,
        _topoheight: u64,
    ) -> Result<Option<Vec<u8>>, anyhow::Error> {
        Ok(None)
    }
}

// ============================================================================
// KYC Data Structure for Tests
// ============================================================================

#[derive(Clone, Debug)]
struct KycData {
    level: u16,
    status: KycStatus,
    verified_at: u64,
    data_hash: Hash,
    committee_id: Hash,
    previous_status: Option<KycStatus>,
}

// ============================================================================
// Test Chain State with KYC Support
// ============================================================================

struct KycTestChainState {
    sender_balances: HashMap<CompressedPublicKey, u64>,
    receiver_balances: HashMap<CompressedPublicKey, u64>,
    nonces: HashMap<CompressedPublicKey, Nonce>,
    referrers: HashMap<CompressedPublicKey, CompressedPublicKey>,
    environment: Environment,
    block: Block,
    block_hash: Hash,
    burned: u64,
    gas_fee: u64,
    executor: Arc<dyn ContractExecutor>,
    _contract_provider: DummyContractProvider,
    current_time: u64,

    // KYC-specific state
    kyc_data: HashMap<CompressedPublicKey, KycData>,
    committees: HashMap<Hash, SecurityCommittee>,
    global_committee_id: Option<Hash>,
}

impl KycTestChainState {
    fn new() -> Self {
        let miner = KeyPair::new().get_public_key().compress();
        let header = BlockHeader::new(
            BlockVersion::Nobunaga,
            0,
            0,
            IndexSet::new(),
            [0u8; EXTRA_NONCE_SIZE],
            miner,
            IndexSet::new(),
        );
        let block = Block::new(Immutable::Owned(header), vec![]);
        let block_hash = block.hash();

        Self {
            sender_balances: HashMap::new(),
            receiver_balances: HashMap::new(),
            nonces: HashMap::new(),
            referrers: HashMap::new(),
            environment: Environment::new(),
            block,
            block_hash,
            burned: 0,
            gas_fee: 0,
            executor: Arc::new(TakoContractExecutor::new()),
            _contract_provider: DummyContractProvider,
            current_time: std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            kyc_data: HashMap::new(),
            committees: HashMap::new(),
            global_committee_id: None,
        }
    }

    fn get_kyc(&self, account: &CompressedPublicKey) -> Option<&KycData> {
        self.kyc_data.get(account)
    }

    fn get_committee_by_id(&self, committee_id: &Hash) -> Option<&SecurityCommittee> {
        self.committees.get(committee_id)
    }

    #[allow(dead_code)]
    fn set_time(&mut self, time: u64) {
        self.current_time = time;
    }
}

// ============================================================================
// BlockchainVerificationState Implementation
// ============================================================================

#[async_trait]
impl<'a> BlockchainVerificationState<'a, TestError> for KycTestChainState {
    async fn pre_verify_tx<'b>(&'b mut self, _tx: &Transaction) -> Result<(), TestError> {
        Ok(())
    }

    async fn get_receiver_balance<'b>(
        &'b mut self,
        account: Cow<'a, CompressedPublicKey>,
        _asset: Cow<'a, Hash>,
    ) -> Result<&'b mut u64, TestError> {
        let entry = self
            .receiver_balances
            .entry(account.into_owned())
            .or_insert(0);
        Ok(entry)
    }

    async fn get_sender_balance<'b>(
        &'b mut self,
        account: &'a CompressedPublicKey,
        _asset: &'a Hash,
        _reference: &Reference,
    ) -> Result<&'b mut u64, TestError> {
        let entry = self.sender_balances.entry(account.clone()).or_insert(0);
        Ok(entry)
    }

    async fn add_sender_output(
        &mut self,
        account: &'a CompressedPublicKey,
        _asset: &'a Hash,
        output: u64,
    ) -> Result<(), TestError> {
        let balance = self.sender_balances.entry(account.clone()).or_insert(0);
        *balance = balance.checked_add(output).ok_or(TestError::Overflow)?;
        Ok(())
    }

    async fn get_account_nonce(
        &mut self,
        account: &'a CompressedPublicKey,
    ) -> Result<Nonce, TestError> {
        Ok(*self.nonces.get(account).unwrap_or(&0))
    }

    async fn update_account_nonce(
        &mut self,
        account: &'a CompressedPublicKey,
        new_nonce: Nonce,
    ) -> Result<(), TestError> {
        self.nonces.insert(account.clone(), new_nonce);
        Ok(())
    }

    async fn compare_and_swap_nonce(
        &mut self,
        account: &'a CompressedPublicKey,
        expected: Nonce,
        new_value: Nonce,
    ) -> Result<bool, TestError> {
        let current = self.get_account_nonce(account).await?;
        if current == expected {
            self.nonces.insert(account.clone(), new_value);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn get_block_version(&self) -> BlockVersion {
        BlockVersion::Nobunaga
    }

    fn get_verification_timestamp(&self) -> u64 {
        self.current_time
    }

    async fn set_multisig_state(
        &mut self,
        _account: &'a CompressedPublicKey,
        _config: &MultiSigPayload,
    ) -> Result<(), TestError> {
        Ok(())
    }

    async fn get_multisig_state(
        &mut self,
        _account: &'a CompressedPublicKey,
    ) -> Result<Option<&MultiSigPayload>, TestError> {
        Ok(None)
    }

    async fn get_environment(&mut self) -> Result<&Environment, TestError> {
        Ok(&self.environment)
    }

    async fn set_contract_module(
        &mut self,
        _hash: &Hash,
        _module: &'a Module,
    ) -> Result<(), TestError> {
        Err(TestError::Unsupported)
    }

    async fn load_contract_module(&mut self, _hash: &Hash) -> Result<bool, TestError> {
        Ok(false)
    }

    async fn get_contract_module_with_environment(
        &self,
        _hash: &Hash,
    ) -> Result<(&Module, &Environment), TestError> {
        Err(TestError::Unsupported)
    }

    fn get_network(&self) -> Network {
        Network::Devnet
    }

    async fn is_account_registered(
        &self,
        _account: &CompressedPublicKey,
    ) -> Result<bool, TestError> {
        // For testing, assume all accounts are registered
        Ok(true)
    }

    async fn get_receiver_uno_balance<'b>(
        &'b mut self,
        _account: Cow<'a, CompressedPublicKey>,
        _asset: Cow<'a, Hash>,
    ) -> Result<&'b mut Ciphertext, TestError> {
        Err(TestError::Unsupported)
    }

    async fn get_sender_uno_balance<'b>(
        &'b mut self,
        _account: &'a CompressedPublicKey,
        _asset: &'a Hash,
        _reference: &Reference,
    ) -> Result<&'b mut Ciphertext, TestError> {
        Err(TestError::Unsupported)
    }

    async fn add_sender_uno_output(
        &mut self,
        _account: &'a CompressedPublicKey,
        _asset: &'a Hash,
        _output: Ciphertext,
    ) -> Result<(), TestError> {
        Err(TestError::Unsupported)
    }

    async fn get_account_energy(
        &mut self,
        _account: &'a CompressedPublicKey,
    ) -> Result<Option<tos_common::account::AccountEnergy>, TestError> {
        Ok(None)
    }

    async fn get_delegated_resource(
        &mut self,
        _from: &'a CompressedPublicKey,
        _to: &'a CompressedPublicKey,
    ) -> Result<Option<tos_common::account::DelegatedResource>, TestError> {
        Ok(None)
    }

    async fn record_pending_undelegation(
        &mut self,
        _from: &'a CompressedPublicKey,
        _to: &'a CompressedPublicKey,
        _amount: u64,
    ) -> Result<(), TestError> {
        Ok(())
    }

    fn is_pending_registration(&self, _account: &CompressedPublicKey) -> bool {
        false
    }

    fn record_pending_registration(&mut self, _account: &CompressedPublicKey) {}

    // Stub implementations for test
    async fn record_pending_delegation(
        &mut self,
        _sender: &'a CompressedPublicKey,
        _amount: u64,
    ) -> Result<(), TestError> {
        Ok(())
    }

    fn get_pending_delegation(&self, _sender: &CompressedPublicKey) -> u64 {
        0
    }

    // Stub implementations for test
    async fn record_pending_energy(
        &mut self,
        _sender: &'a CompressedPublicKey,
        _amount: u64,
    ) -> Result<(), TestError> {
        Ok(())
    }

    fn get_pending_energy(&self, _sender: &CompressedPublicKey) -> u64 {
        0
    }
}

// ============================================================================
// BlockchainApplyState Implementation
// ============================================================================

#[async_trait]
impl<'a> BlockchainApplyState<'a, DummyContractProvider, TestError> for KycTestChainState {
    async fn add_burned_coins(&mut self, amount: u64) -> Result<(), TestError> {
        self.burned = self.burned.saturating_add(amount);
        Ok(())
    }

    async fn add_gas_fee(&mut self, amount: u64) -> Result<(), TestError> {
        self.gas_fee = self.gas_fee.saturating_add(amount);
        Ok(())
    }

    fn get_block_hash(&self) -> &Hash {
        &self.block_hash
    }

    fn get_block(&self) -> &Block {
        &self.block
    }

    fn is_mainnet(&self) -> bool {
        false
    }

    async fn set_contract_outputs(
        &mut self,
        _tx_hash: &'a Hash,
        _outputs: Vec<ContractOutput>,
    ) -> Result<(), TestError> {
        Ok(())
    }

    async fn get_contract_environment_for<'b>(
        &'b mut self,
        _contract: &'b Hash,
        _deposits: &'b IndexMap<Hash, ContractDeposit>,
        _tx_hash: &'b Hash,
    ) -> Result<
        (
            ContractEnvironment<'b, DummyContractProvider>,
            ContractChainState<'b>,
        ),
        TestError,
    > {
        Err(TestError::Unsupported)
    }

    async fn merge_contract_changes(
        &mut self,
        _hash: &Hash,
        _cache: ContractCache,
        _tracker: ContractEventTracker,
        _assets: HashMap<Hash, Option<AssetChanges>>,
    ) -> Result<(), TestError> {
        Err(TestError::Unsupported)
    }

    async fn remove_contract_module(&mut self, _hash: &Hash) -> Result<(), TestError> {
        Err(TestError::Unsupported)
    }

    // Note: get_account_energy is inherited from BlockchainVerificationState

    async fn set_account_energy(
        &mut self,
        _account: &'a CompressedPublicKey,
        _energy: AccountEnergy,
    ) -> Result<(), TestError> {
        Ok(())
    }

    async fn get_global_energy_state(
        &mut self,
    ) -> Result<tos_common::account::GlobalEnergyState, TestError> {
        Ok(tos_common::account::GlobalEnergyState::default())
    }

    async fn set_global_energy_state(
        &mut self,
        _state: tos_common::account::GlobalEnergyState,
    ) -> Result<(), TestError> {
        Ok(())
    }

    // Note: get_delegated_resource is inherited from BlockchainVerificationState

    async fn set_delegated_resource(
        &mut self,
        _delegation: &tos_common::account::DelegatedResource,
    ) -> Result<(), TestError> {
        Ok(())
    }

    async fn delete_delegated_resource(
        &mut self,
        _from: &'a CompressedPublicKey,
        _to: &'a CompressedPublicKey,
    ) -> Result<(), TestError> {
        Ok(())
    }

    async fn get_ai_mining_state(
        &mut self,
    ) -> Result<Option<tos_common::ai_mining::AIMiningState>, TestError> {
        Ok(None)
    }

    async fn set_ai_mining_state(
        &mut self,
        _state: &tos_common::ai_mining::AIMiningState,
    ) -> Result<(), TestError> {
        Ok(())
    }

    fn get_contract_executor(&self) -> Arc<dyn ContractExecutor> {
        self.executor.clone()
    }

    async fn add_contract_events(
        &mut self,
        _events: Vec<ContractEvent>,
        _contract: &Hash,
        _tx_hash: &'a Hash,
    ) -> Result<(), TestError> {
        Ok(())
    }

    // ===== Referral Operations =====

    async fn bind_referrer(
        &mut self,
        user: &'a CompressedPublicKey,
        referrer: &'a CompressedPublicKey,
        _tx_hash: &'a Hash,
    ) -> Result<(), TestError> {
        self.referrers.insert(user.clone(), referrer.clone());
        Ok(())
    }

    async fn distribute_referral_rewards(
        &mut self,
        _from_user: &'a CompressedPublicKey,
        _asset: &'a Hash,
        _total_amount: u64,
        _ratios: &[u16],
    ) -> Result<DistributionResult, TestError> {
        Ok(DistributionResult::new(vec![]))
    }

    // ===== KYC System Operations =====

    async fn get_committee(
        &self,
        committee_id: &'a Hash,
    ) -> Result<Option<SecurityCommittee>, TestError> {
        Ok(self.committees.get(committee_id).cloned())
    }

    async fn get_verifying_committee(
        &self,
        user: &'a CompressedPublicKey,
    ) -> Result<Option<Hash>, TestError> {
        Ok(self.kyc_data.get(user).map(|kyc| kyc.committee_id.clone()))
    }

    async fn get_kyc_status(
        &self,
        user: &'a CompressedPublicKey,
    ) -> Result<Option<tos_common::kyc::KycStatus>, TestError> {
        Ok(self.kyc_data.get(user).map(|kyc| kyc.status))
    }

    async fn get_kyc_level(&self, user: &'a CompressedPublicKey) -> Result<Option<u16>, TestError> {
        Ok(self.kyc_data.get(user).map(|kyc| kyc.level))
    }

    async fn is_global_committee_bootstrapped(&self) -> Result<bool, TestError> {
        Ok(self.global_committee_id.is_some())
    }

    async fn set_kyc(
        &mut self,
        user: &'a CompressedPublicKey,
        level: u16,
        verified_at: u64,
        data_hash: &'a Hash,
        committee_id: &'a Hash,
        _tx_hash: &'a Hash,
    ) -> Result<(), TestError> {
        let kyc = KycData {
            level,
            status: KycStatus::Active,
            verified_at,
            data_hash: data_hash.clone(),
            committee_id: committee_id.clone(),
            previous_status: None,
        };
        self.kyc_data.insert(user.clone(), kyc);
        Ok(())
    }

    async fn revoke_kyc(
        &mut self,
        user: &'a CompressedPublicKey,
        _reason_hash: &'a Hash,
        _tx_hash: &'a Hash,
    ) -> Result<(), TestError> {
        let kyc = self.kyc_data.get_mut(user).ok_or(TestError::KycNotFound)?;
        kyc.previous_status = Some(kyc.status);
        kyc.status = KycStatus::Revoked;
        Ok(())
    }

    async fn renew_kyc(
        &mut self,
        user: &'a CompressedPublicKey,
        verified_at: u64,
        data_hash: &'a Hash,
        _tx_hash: &'a Hash,
    ) -> Result<(), TestError> {
        let kyc = self.kyc_data.get_mut(user).ok_or(TestError::KycNotFound)?;
        if kyc.status == KycStatus::Revoked {
            return Err(TestError::InvalidStatus);
        }
        kyc.verified_at = verified_at;
        kyc.data_hash = data_hash.clone();
        Ok(())
    }

    async fn transfer_kyc(
        &mut self,
        user: &'a CompressedPublicKey,
        _source_committee_id: &'a Hash,
        dest_committee_id: &'a Hash,
        new_data_hash: &'a Hash,
        transferred_at: u64,
        _tx_hash: &'a Hash,
        dest_max_kyc_level: u16,
        _verification_timestamp: u64,
    ) -> Result<(), TestError> {
        let kyc = self.kyc_data.get_mut(user).ok_or(TestError::KycNotFound)?;
        if kyc.status == KycStatus::Revoked || kyc.status == KycStatus::Suspended {
            return Err(TestError::InvalidStatus);
        }
        // Verify KYC level doesn't exceed destination committee's max level
        if kyc.level > dest_max_kyc_level {
            return Err(TestError::KycLevelExceedsMax);
        }
        kyc.committee_id = dest_committee_id.clone();
        kyc.data_hash = new_data_hash.clone();
        kyc.verified_at = transferred_at;
        Ok(())
    }

    async fn submit_kyc_appeal(
        &mut self,
        _user: &'a CompressedPublicKey,
        _original_committee_id: &'a Hash,
        _parent_committee_id: &'a Hash,
        _reason_hash: &'a Hash,
        _documents_hash: &'a Hash,
        _submitted_at: u64,
        _tx_hash: &'a Hash,
    ) -> Result<(), TestError> {
        Ok(())
    }

    async fn emergency_suspend_kyc(
        &mut self,
        user: &'a CompressedPublicKey,
        _reason_hash: &'a Hash,
        _expires_at: u64,
        _tx_hash: &'a Hash,
    ) -> Result<(), TestError> {
        let kyc = self.kyc_data.get_mut(user).ok_or(TestError::KycNotFound)?;
        kyc.previous_status = Some(kyc.status);
        kyc.status = KycStatus::Suspended;
        Ok(())
    }

    async fn bootstrap_global_committee(
        &mut self,
        name: String,
        members: Vec<tos_common::kyc::CommitteeMemberInfo>,
        threshold: u8,
        kyc_threshold: u8,
        max_kyc_level: u16,
        tx_hash: &'a Hash,
    ) -> Result<Hash, TestError> {
        if self.global_committee_id.is_some() {
            return Err(TestError::GlobalCommitteeAlreadyBootstrapped);
        }

        let committee_members: Vec<CommitteeMember> = members
            .into_iter()
            .map(|m| m.into_member(self.current_time))
            .collect();

        let mut committee = SecurityCommittee::new_global(
            name,
            committee_members,
            threshold,
            kyc_threshold,
            max_kyc_level,
            self.current_time,
        );
        // Override ID with tx_hash for test predictability
        committee.id = tx_hash.clone();

        let committee_id = committee.id.clone();
        self.committees.insert(committee_id.clone(), committee);
        self.global_committee_id = Some(committee_id.clone());
        Ok(committee_id)
    }

    async fn register_committee(
        &mut self,
        name: String,
        region: KycRegion,
        members: Vec<tos_common::kyc::CommitteeMemberInfo>,
        threshold: u8,
        kyc_threshold: u8,
        max_kyc_level: u16,
        parent_id: &'a Hash,
        tx_hash: &'a Hash,
    ) -> Result<Hash, TestError> {
        if !self.committees.contains_key(parent_id) {
            return Err(TestError::CommitteeNotFound);
        }

        let committee_members: Vec<CommitteeMember> = members
            .into_iter()
            .map(|m| m.into_member(self.current_time))
            .collect();

        let mut committee = SecurityCommittee::new_regional(
            name,
            region,
            committee_members,
            threshold,
            kyc_threshold,
            max_kyc_level,
            parent_id.clone(),
            self.current_time,
        );
        // Override ID with tx_hash for test predictability
        committee.id = tx_hash.clone();

        let committee_id = committee.id.clone();
        self.committees.insert(committee_id.clone(), committee);
        Ok(committee_id)
    }

    async fn update_committee(
        &mut self,
        committee_id: &'a Hash,
        update: &CommitteeUpdateData,
    ) -> Result<(), TestError> {
        let committee = self
            .committees
            .get_mut(committee_id)
            .ok_or(TestError::CommitteeNotFound)?;

        match update {
            CommitteeUpdateData::AddMember {
                public_key,
                name,
                role,
            } => {
                committee.add_member(public_key.clone(), name.clone(), *role);
            }
            CommitteeUpdateData::RemoveMember { public_key } => {
                committee.remove_member(public_key);
            }
            CommitteeUpdateData::UpdateThreshold { new_threshold } => {
                committee.threshold = *new_threshold;
            }
            CommitteeUpdateData::UpdateKycThreshold { new_kyc_threshold } => {
                committee.kyc_threshold = *new_kyc_threshold;
            }
            CommitteeUpdateData::SuspendCommittee => {
                committee.status = CommitteeStatus::Suspended;
            }
            CommitteeUpdateData::ActivateCommittee => {
                committee.status = CommitteeStatus::Active;
            }
            _ => {}
        }
        Ok(())
    }

    // ===== Transaction Result Storage (Stake 2.0) =====

    async fn set_transaction_result(
        &mut self,
        _tx_hash: &'a Hash,
        _result: &tos_common::transaction::TransactionResult,
    ) -> Result<(), TestError> {
        // Test stub - no-op for now
        Ok(())
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn create_test_members(keys: &[KeyPair]) -> Vec<tos_common::kyc::CommitteeMemberInfo> {
    keys.iter()
        .enumerate()
        .map(|(i, k)| {
            tos_common::kyc::CommitteeMemberInfo::new(
                k.get_public_key().compress(),
                Some(format!("Member-{}", i)),
                MemberRole::Member,
            )
        })
        .collect()
}

// ============================================================================
// Test Cases
// ============================================================================

#[tokio::test]
async fn test_kyc_bootstrap_global_committee() {
    let mut state = KycTestChainState::new();

    // Create 5 committee members
    let members: Vec<KeyPair> = (0..5).map(|_| KeyPair::new()).collect();
    let member_infos = create_test_members(&members);
    let tx_hash = Hash::new([1u8; 32]);

    // Bootstrap global committee
    let committee_id = state
        .bootstrap_global_committee(
            "Global Security Committee".to_string(),
            member_infos,
            3,   // threshold
            2,   // kyc_threshold
            255, // max_kyc_level
            &tx_hash,
        )
        .await
        .expect("Should bootstrap global committee");

    // Verify committee was created
    let committee = state
        .get_committee_by_id(&committee_id)
        .expect("Committee should exist");
    assert_eq!(committee.name, "Global Security Committee");
    assert_eq!(committee.members.len(), 5);
    assert_eq!(committee.threshold, 3);
    assert_eq!(committee.kyc_threshold, 2);
    assert!(committee.is_active());

    // Verify global committee is bootstrapped
    let bootstrapped = state
        .is_global_committee_bootstrapped()
        .await
        .expect("Should check bootstrap status");
    assert!(bootstrapped);
}

#[tokio::test]
async fn test_kyc_bootstrap_global_committee_only_once() {
    let mut state = KycTestChainState::new();

    let members: Vec<KeyPair> = (0..3).map(|_| KeyPair::new()).collect();
    let member_infos = create_test_members(&members);
    let tx_hash1 = Hash::new([1u8; 32]);
    let tx_hash2 = Hash::new([2u8; 32]);

    // First bootstrap should succeed
    state
        .bootstrap_global_committee(
            "Global".to_string(),
            member_infos.clone(),
            2,
            2,
            255,
            &tx_hash1,
        )
        .await
        .expect("First bootstrap should succeed");

    // Second bootstrap should fail
    let result = state
        .bootstrap_global_committee("Global2".to_string(), member_infos, 2, 2, 255, &tx_hash2)
        .await;
    assert!(result.is_err(), "Second bootstrap should fail");
}

#[tokio::test]
async fn test_kyc_register_regional_committee() {
    let mut state = KycTestChainState::new();

    // Bootstrap global committee first
    let global_members: Vec<KeyPair> = (0..3).map(|_| KeyPair::new()).collect();
    let global_member_infos = create_test_members(&global_members);
    let global_tx_hash = Hash::new([1u8; 32]);

    let global_id = state
        .bootstrap_global_committee(
            "Global".to_string(),
            global_member_infos,
            2,
            2,
            255,
            &global_tx_hash,
        )
        .await
        .expect("Should bootstrap global committee");

    // Register regional committee
    let regional_members: Vec<KeyPair> = (0..3).map(|_| KeyPair::new()).collect();
    let regional_member_infos = create_test_members(&regional_members);
    let regional_tx_hash = Hash::new([2u8; 32]);

    let regional_id = state
        .register_committee(
            "Asia Pacific".to_string(),
            KycRegion::AsiaPacific,
            regional_member_infos,
            2,
            2,
            100, // max_kyc_level
            &global_id,
            &regional_tx_hash,
        )
        .await
        .expect("Should register regional committee");

    // Verify regional committee
    let committee = state
        .get_committee_by_id(&regional_id)
        .expect("Regional committee should exist");
    assert_eq!(committee.name, "Asia Pacific");
    assert_eq!(committee.region, KycRegion::AsiaPacific);
    assert_eq!(committee.parent_id, Some(global_id));
}

#[tokio::test]
async fn test_kyc_set_and_get() {
    let mut state = KycTestChainState::new();

    // Bootstrap global committee
    let members: Vec<KeyPair> = (0..3).map(|_| KeyPair::new()).collect();
    let member_infos = create_test_members(&members);
    let tx_hash = Hash::new([1u8; 32]);

    let committee_id = state
        .bootstrap_global_committee("Global".to_string(), member_infos, 2, 2, 255, &tx_hash)
        .await
        .expect("Should bootstrap");

    // Create user and set KYC
    let user = KeyPair::new();
    let user_pk = user.get_public_key().compress();
    let data_hash = Hash::new([3u8; 32]);
    let set_tx_hash = Hash::new([4u8; 32]);

    state
        .set_kyc(&user_pk, 50, 1000, &data_hash, &committee_id, &set_tx_hash)
        .await
        .expect("Should set KYC");

    // Verify KYC data
    let kyc = state.get_kyc(&user_pk).expect("KYC should exist");
    assert_eq!(kyc.level, 50);
    assert_eq!(kyc.status, KycStatus::Active);
    assert_eq!(kyc.verified_at, 1000);
    assert_eq!(kyc.data_hash, data_hash);
    assert_eq!(kyc.committee_id, committee_id);
}

#[tokio::test]
async fn test_kyc_renew() {
    let mut state = KycTestChainState::new();

    // Setup
    let members: Vec<KeyPair> = (0..3).map(|_| KeyPair::new()).collect();
    let member_infos = create_test_members(&members);
    let committee_id = state
        .bootstrap_global_committee(
            "Global".to_string(),
            member_infos,
            2,
            2,
            255,
            &Hash::new([1u8; 32]),
        )
        .await
        .expect("Should bootstrap");

    // Set initial KYC
    let user = KeyPair::new();
    let user_pk = user.get_public_key().compress();
    let data_hash = Hash::new([3u8; 32]);
    state
        .set_kyc(
            &user_pk,
            50,
            1000,
            &data_hash,
            &committee_id,
            &Hash::new([4u8; 32]),
        )
        .await
        .expect("Should set KYC");

    // Renew KYC
    let new_data_hash = Hash::new([5u8; 32]);
    state
        .renew_kyc(&user_pk, 2000, &new_data_hash, &Hash::new([6u8; 32]))
        .await
        .expect("Should renew KYC");

    // Verify renewal
    let kyc = state.get_kyc(&user_pk).expect("KYC should exist");
    assert_eq!(kyc.verified_at, 2000);
    assert_eq!(kyc.data_hash, new_data_hash);
    assert_eq!(kyc.status, KycStatus::Active);
}

#[tokio::test]
async fn test_kyc_revoke() {
    let mut state = KycTestChainState::new();

    // Setup
    let members: Vec<KeyPair> = (0..3).map(|_| KeyPair::new()).collect();
    let member_infos = create_test_members(&members);
    let committee_id = state
        .bootstrap_global_committee(
            "Global".to_string(),
            member_infos,
            2,
            2,
            255,
            &Hash::new([1u8; 32]),
        )
        .await
        .expect("Should bootstrap");

    // Set initial KYC
    let user = KeyPair::new();
    let user_pk = user.get_public_key().compress();
    state
        .set_kyc(
            &user_pk,
            50,
            1000,
            &Hash::new([3u8; 32]),
            &committee_id,
            &Hash::new([4u8; 32]),
        )
        .await
        .expect("Should set KYC");

    // Revoke KYC
    let reason_hash = Hash::new([5u8; 32]);
    state
        .revoke_kyc(&user_pk, &reason_hash, &Hash::new([6u8; 32]))
        .await
        .expect("Should revoke KYC");

    // Verify revocation
    let kyc = state.get_kyc(&user_pk).expect("KYC should exist");
    assert_eq!(kyc.status, KycStatus::Revoked);
    assert_eq!(kyc.previous_status, Some(KycStatus::Active));
}

#[tokio::test]
async fn test_kyc_revoked_cannot_renew() {
    let mut state = KycTestChainState::new();

    // Setup
    let members: Vec<KeyPair> = (0..3).map(|_| KeyPair::new()).collect();
    let member_infos = create_test_members(&members);
    let committee_id = state
        .bootstrap_global_committee(
            "Global".to_string(),
            member_infos,
            2,
            2,
            255,
            &Hash::new([1u8; 32]),
        )
        .await
        .expect("Should bootstrap");

    // Set and revoke KYC
    let user = KeyPair::new();
    let user_pk = user.get_public_key().compress();
    state
        .set_kyc(
            &user_pk,
            50,
            1000,
            &Hash::new([3u8; 32]),
            &committee_id,
            &Hash::new([4u8; 32]),
        )
        .await
        .expect("Should set KYC");

    state
        .revoke_kyc(&user_pk, &Hash::new([5u8; 32]), &Hash::new([6u8; 32]))
        .await
        .expect("Should revoke");

    // Try to renew revoked KYC
    let result = state
        .renew_kyc(&user_pk, 2000, &Hash::new([7u8; 32]), &Hash::new([8u8; 32]))
        .await;
    assert!(result.is_err(), "Should not renew revoked KYC");
}

#[tokio::test]
async fn test_kyc_emergency_suspend() {
    let mut state = KycTestChainState::new();

    // Setup
    let members: Vec<KeyPair> = (0..3).map(|_| KeyPair::new()).collect();
    let member_infos = create_test_members(&members);
    let committee_id = state
        .bootstrap_global_committee(
            "Global".to_string(),
            member_infos,
            2,
            2,
            255,
            &Hash::new([1u8; 32]),
        )
        .await
        .expect("Should bootstrap");

    // Set initial KYC
    let user = KeyPair::new();
    let user_pk = user.get_public_key().compress();
    state
        .set_kyc(
            &user_pk,
            50,
            1000,
            &Hash::new([3u8; 32]),
            &committee_id,
            &Hash::new([4u8; 32]),
        )
        .await
        .expect("Should set KYC");

    // Emergency suspend
    let reason_hash = Hash::new([5u8; 32]);
    let expires_at = state.current_time + 86400; // 24 hours
    state
        .emergency_suspend_kyc(&user_pk, &reason_hash, expires_at, &Hash::new([6u8; 32]))
        .await
        .expect("Should suspend KYC");

    // Verify suspension
    let kyc = state.get_kyc(&user_pk).expect("KYC should exist");
    assert_eq!(kyc.status, KycStatus::Suspended);
    assert_eq!(kyc.previous_status, Some(KycStatus::Active));
}

#[tokio::test]
async fn test_kyc_transfer() {
    let mut state = KycTestChainState::new();

    // Bootstrap global committee
    let global_members: Vec<KeyPair> = (0..3).map(|_| KeyPair::new()).collect();
    let global_id = state
        .bootstrap_global_committee(
            "Global".to_string(),
            create_test_members(&global_members),
            2,
            2,
            255,
            &Hash::new([1u8; 32]),
        )
        .await
        .expect("Should bootstrap");

    // Register source committee (Asia)
    let asia_members: Vec<KeyPair> = (0..3).map(|_| KeyPair::new()).collect();
    let asia_id = state
        .register_committee(
            "Asia".to_string(),
            KycRegion::AsiaPacific,
            create_test_members(&asia_members),
            2,
            2,
            100,
            &global_id,
            &Hash::new([2u8; 32]),
        )
        .await
        .expect("Should register Asia committee");

    // Register destination committee (Europe)
    let europe_members: Vec<KeyPair> = (0..3).map(|_| KeyPair::new()).collect();
    let europe_id = state
        .register_committee(
            "Europe".to_string(),
            KycRegion::Europe,
            create_test_members(&europe_members),
            2,
            2,
            100,
            &global_id,
            &Hash::new([3u8; 32]),
        )
        .await
        .expect("Should register Europe committee");

    // Set user KYC in Asia committee
    let user = KeyPair::new();
    let user_pk = user.get_public_key().compress();
    state
        .set_kyc(
            &user_pk,
            50,
            1000,
            &Hash::new([4u8; 32]),
            &asia_id,
            &Hash::new([5u8; 32]),
        )
        .await
        .expect("Should set KYC");

    // Verify initial committee
    let kyc = state.get_kyc(&user_pk).expect("KYC should exist");
    assert_eq!(kyc.committee_id, asia_id);

    // Transfer KYC to Europe
    let new_data_hash = Hash::new([6u8; 32]);
    state
        .transfer_kyc(
            &user_pk,
            &asia_id,
            &europe_id,
            &new_data_hash,
            2000,
            &Hash::new([7u8; 32]),
            100,  // dest committee max_kyc_level
            2000, // verification_timestamp
        )
        .await
        .expect("Should transfer KYC");

    // Verify transfer
    let kyc = state.get_kyc(&user_pk).expect("KYC should exist");
    assert_eq!(kyc.committee_id, europe_id);
    assert_eq!(kyc.data_hash, new_data_hash);
    assert_eq!(kyc.verified_at, 2000);
    assert_eq!(kyc.status, KycStatus::Active);
}

#[tokio::test]
async fn test_kyc_suspended_cannot_transfer() {
    let mut state = KycTestChainState::new();

    // Setup committees
    let global_members: Vec<KeyPair> = (0..3).map(|_| KeyPair::new()).collect();
    let global_id = state
        .bootstrap_global_committee(
            "Global".to_string(),
            create_test_members(&global_members),
            2,
            2,
            255,
            &Hash::new([1u8; 32]),
        )
        .await
        .expect("Should bootstrap");

    let asia_id = state
        .register_committee(
            "Asia".to_string(),
            KycRegion::AsiaPacific,
            create_test_members(&(0..3).map(|_| KeyPair::new()).collect::<Vec<_>>()),
            2,
            2,
            100,
            &global_id,
            &Hash::new([2u8; 32]),
        )
        .await
        .expect("Should register");

    let europe_id = state
        .register_committee(
            "Europe".to_string(),
            KycRegion::Europe,
            create_test_members(&(0..3).map(|_| KeyPair::new()).collect::<Vec<_>>()),
            2,
            2,
            100,
            &global_id,
            &Hash::new([3u8; 32]),
        )
        .await
        .expect("Should register");

    // Set and suspend KYC
    let user = KeyPair::new();
    let user_pk = user.get_public_key().compress();
    state
        .set_kyc(
            &user_pk,
            50,
            1000,
            &Hash::new([4u8; 32]),
            &asia_id,
            &Hash::new([5u8; 32]),
        )
        .await
        .expect("Should set KYC");

    state
        .emergency_suspend_kyc(
            &user_pk,
            &Hash::new([6u8; 32]),
            state.current_time + 86400,
            &Hash::new([7u8; 32]),
        )
        .await
        .expect("Should suspend");

    // Try to transfer suspended KYC
    let result = state
        .transfer_kyc(
            &user_pk,
            &asia_id,
            &europe_id,
            &Hash::new([8u8; 32]),
            2000,
            &Hash::new([9u8; 32]),
            100,  // dest committee max_kyc_level
            2000, // verification_timestamp
        )
        .await;
    assert!(result.is_err(), "Should not transfer suspended KYC");
}

#[tokio::test]
async fn test_committee_update_add_remove_member() {
    let mut state = KycTestChainState::new();

    // Bootstrap with 3 members
    let members: Vec<KeyPair> = (0..3).map(|_| KeyPair::new()).collect();
    let committee_id = state
        .bootstrap_global_committee(
            "Global".to_string(),
            create_test_members(&members),
            2,
            2,
            255,
            &Hash::new([1u8; 32]),
        )
        .await
        .expect("Should bootstrap");

    // Verify initial member count
    let committee = state
        .get_committee_by_id(&committee_id)
        .expect("Should exist");
    assert_eq!(committee.members.len(), 3);

    // Add new member
    let new_member = KeyPair::new();
    state
        .update_committee(
            &committee_id,
            &CommitteeUpdateData::AddMember {
                public_key: new_member.get_public_key().compress(),
                name: Some("New Member".to_string()),
                role: MemberRole::Member,
            },
        )
        .await
        .expect("Should add member");

    // Verify member was added
    let committee = state
        .get_committee_by_id(&committee_id)
        .expect("Should exist");
    assert_eq!(committee.members.len(), 4);

    // Remove a member
    let member_to_remove = members[0].get_public_key().compress();
    state
        .update_committee(
            &committee_id,
            &CommitteeUpdateData::RemoveMember {
                public_key: member_to_remove,
            },
        )
        .await
        .expect("Should remove member");

    // Verify member was removed
    let committee = state
        .get_committee_by_id(&committee_id)
        .expect("Should exist");
    assert_eq!(committee.members.len(), 3);
}

#[tokio::test]
async fn test_committee_update_threshold() {
    let mut state = KycTestChainState::new();

    // Bootstrap with threshold 2
    let members: Vec<KeyPair> = (0..5).map(|_| KeyPair::new()).collect();
    let committee_id = state
        .bootstrap_global_committee(
            "Global".to_string(),
            create_test_members(&members),
            2,
            2,
            255,
            &Hash::new([1u8; 32]),
        )
        .await
        .expect("Should bootstrap");

    // Verify initial threshold
    let committee = state
        .get_committee_by_id(&committee_id)
        .expect("Should exist");
    assert_eq!(committee.threshold, 2);

    // Update threshold to 3
    state
        .update_committee(
            &committee_id,
            &CommitteeUpdateData::UpdateThreshold { new_threshold: 3 },
        )
        .await
        .expect("Should update threshold");

    // Verify threshold was updated
    let committee = state
        .get_committee_by_id(&committee_id)
        .expect("Should exist");
    assert_eq!(committee.threshold, 3);
}

#[tokio::test]
async fn test_committee_suspend_and_activate() {
    let mut state = KycTestChainState::new();

    // Bootstrap committee
    let members: Vec<KeyPair> = (0..3).map(|_| KeyPair::new()).collect();
    let committee_id = state
        .bootstrap_global_committee(
            "Global".to_string(),
            create_test_members(&members),
            2,
            2,
            255,
            &Hash::new([1u8; 32]),
        )
        .await
        .expect("Should bootstrap");

    // Verify initially active
    let committee = state
        .get_committee_by_id(&committee_id)
        .expect("Should exist");
    assert!(committee.is_active());

    // Suspend committee
    state
        .update_committee(&committee_id, &CommitteeUpdateData::SuspendCommittee)
        .await
        .expect("Should suspend");

    // Verify suspended
    let committee = state
        .get_committee_by_id(&committee_id)
        .expect("Should exist");
    assert!(!committee.is_active());

    // Activate committee
    state
        .update_committee(&committee_id, &CommitteeUpdateData::ActivateCommittee)
        .await
        .expect("Should activate");

    // Verify active again
    let committee = state
        .get_committee_by_id(&committee_id)
        .expect("Should exist");
    assert!(committee.is_active());
}

#[tokio::test]
async fn test_kyc_full_lifecycle() {
    let mut state = KycTestChainState::new();

    // 1. Bootstrap global committee
    let global_members: Vec<KeyPair> = (0..5).map(|_| KeyPair::new()).collect();
    let global_id = state
        .bootstrap_global_committee(
            "Global Security Committee".to_string(),
            create_test_members(&global_members),
            3,
            2,
            255,
            &Hash::new([1u8; 32]),
        )
        .await
        .expect("Should bootstrap");

    // 2. Register regional committee
    let regional_members: Vec<KeyPair> = (0..3).map(|_| KeyPair::new()).collect();
    let regional_id = state
        .register_committee(
            "North America".to_string(),
            KycRegion::NorthAmerica,
            create_test_members(&regional_members),
            2,
            2,
            100,
            &global_id,
            &Hash::new([2u8; 32]),
        )
        .await
        .expect("Should register regional committee");

    // 3. Set KYC for user
    let user = KeyPair::new();
    let user_pk = user.get_public_key().compress();
    state
        .set_kyc(
            &user_pk,
            50,
            1000,
            &Hash::new([3u8; 32]),
            &regional_id,
            &Hash::new([4u8; 32]),
        )
        .await
        .expect("Should set KYC");

    // Verify Active status
    let kyc = state.get_kyc(&user_pk).expect("KYC should exist");
    assert_eq!(kyc.status, KycStatus::Active);
    assert_eq!(kyc.level, 50);

    // 4. Renew KYC
    state
        .renew_kyc(&user_pk, 2000, &Hash::new([5u8; 32]), &Hash::new([6u8; 32]))
        .await
        .expect("Should renew KYC");

    let kyc = state.get_kyc(&user_pk).expect("KYC should exist");
    assert_eq!(kyc.verified_at, 2000);

    // 5. Emergency suspend
    state
        .emergency_suspend_kyc(
            &user_pk,
            &Hash::new([7u8; 32]),
            state.current_time + 86400,
            &Hash::new([8u8; 32]),
        )
        .await
        .expect("Should suspend");

    let kyc = state.get_kyc(&user_pk).expect("KYC should exist");
    assert_eq!(kyc.status, KycStatus::Suspended);
    assert_eq!(kyc.previous_status, Some(KycStatus::Active));

    // 6. Create another user and revoke their KYC
    let user2 = KeyPair::new();
    let user2_pk = user2.get_public_key().compress();
    state
        .set_kyc(
            &user2_pk,
            30,
            1000,
            &Hash::new([9u8; 32]),
            &regional_id,
            &Hash::new([10u8; 32]),
        )
        .await
        .expect("Should set KYC for user2");

    state
        .revoke_kyc(&user2_pk, &Hash::new([11u8; 32]), &Hash::new([12u8; 32]))
        .await
        .expect("Should revoke");

    let kyc2 = state.get_kyc(&user2_pk).expect("KYC should exist");
    assert_eq!(kyc2.status, KycStatus::Revoked);

    // 7. Verify revoked user cannot renew
    let result = state
        .renew_kyc(
            &user2_pk,
            3000,
            &Hash::new([13u8; 32]),
            &Hash::new([14u8; 32]),
        )
        .await;
    assert!(result.is_err(), "Revoked user should not renew");

    // 8. Update committee threshold
    state
        .update_committee(
            &regional_id,
            &CommitteeUpdateData::UpdateThreshold { new_threshold: 3 },
        )
        .await
        .expect("Should update threshold");

    let committee = state
        .get_committee_by_id(&regional_id)
        .expect("Should exist");
    assert_eq!(committee.threshold, 3);
}
