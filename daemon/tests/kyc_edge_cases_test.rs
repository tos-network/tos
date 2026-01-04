#![allow(clippy::disallowed_methods)]
#![allow(clippy::too_many_arguments)]
#![allow(dead_code)]

//! KYC Edge Cases and Boundary Tests
//!
//! Comprehensive tests for edge cases, boundary conditions, error handling,
//! and security scenarios in the KYC system.

use std::{borrow::Cow, collections::HashMap, sync::Arc};

use async_trait::async_trait;
use indexmap::{IndexMap, IndexSet};
use tos_common::{
    account::{EnergyResource, Nonce},
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
    kyc::{
        CommitteeMember, CommitteeStatus, KycRegion, KycStatus, MemberRole, MemberStatus,
        SecurityCommittee, APPROVAL_EXPIRY_SECONDS, MIN_COMMITTEE_MEMBERS,
    },
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

#[derive(Debug, Clone)]
enum TestError {
    Unsupported,
    Overflow,
    KycNotFound,
    KycAlreadyExists,
    CommitteeNotFound,
    CommitteeSuspended,
    GlobalCommitteeAlreadyBootstrapped,
    GlobalCommitteeNotBootstrapped,
    InvalidStatus,
    InvalidLevel,
    LevelDowngradeNotAllowed,
    InsufficientApprovals,
    ApprovalExpired,
    InvalidThreshold,
    InsufficientMembers,
    MemberNotActive,
    DuplicateMember,
    MaxMembersExceeded,
    EmptyName,
    ParentNotFound,
    SameCommittee,
    LevelExceedsMax,
}

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for TestError {}

// ============================================================================
// Constants
// ============================================================================

const MAX_COMMITTEE_MEMBERS: usize = 21;
const MAX_APPROVALS: usize = 15;

// Valid KYC levels (cumulative bitmask pattern)
const VALID_LEVELS: [u16; 9] = [0, 7, 31, 63, 255, 2047, 8191, 16383, 32767];

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
struct TestKycData {
    level: u16,
    status: KycStatus,
    verified_at: u64,
    data_hash: Hash,
    committee_id: Hash,
    previous_status: Option<KycStatus>,
}

// ============================================================================
// Enhanced Test Chain State
// ============================================================================

struct EdgeCaseTestState {
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
    kyc_data: HashMap<CompressedPublicKey, TestKycData>,
    committees: HashMap<Hash, SecurityCommittee>,
    global_committee_id: Option<Hash>,
}

impl EdgeCaseTestState {
    fn new() -> Self {
        Self::with_time(
            std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(1704067200), // 2024-01-01 00:00:00 UTC fallback
        )
    }

    fn with_time(current_time: u64) -> Self {
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
            current_time,
            kyc_data: HashMap::new(),
            committees: HashMap::new(),
            global_committee_id: None,
        }
    }

    fn get_kyc(&self, account: &CompressedPublicKey) -> Option<&TestKycData> {
        self.kyc_data.get(account)
    }

    fn get_committee_by_id(&self, committee_id: &Hash) -> Option<&SecurityCommittee> {
        self.committees.get(committee_id)
    }

    fn set_time(&mut self, time: u64) {
        self.current_time = time;
    }

    // Validation helpers
    fn is_valid_level(level: u16) -> bool {
        VALID_LEVELS.contains(&level)
    }

    fn calculate_min_threshold(member_count: usize) -> usize {
        (member_count * 2).div_ceil(3)
    }

    fn is_approval_expired(&self, approval_timestamp: u64) -> bool {
        self.current_time.saturating_sub(approval_timestamp) > APPROVAL_EXPIRY_SECONDS
    }

    fn is_approval_from_future(&self, approval_timestamp: u64) -> bool {
        // Allow 1 hour clock skew
        approval_timestamp > self.current_time.saturating_add(3600)
    }

    // Enhanced set_kyc with validation
    fn set_kyc_validated(
        &mut self,
        user: &CompressedPublicKey,
        level: u16,
        verified_at: u64,
        data_hash: &Hash,
        committee_id: &Hash,
    ) -> Result<(), TestError> {
        // Check if valid level
        if !Self::is_valid_level(level) {
            return Err(TestError::InvalidLevel);
        }

        // Check if committee exists and is active
        let committee = self
            .committees
            .get(committee_id)
            .ok_or(TestError::CommitteeNotFound)?;
        if !committee.is_active() {
            return Err(TestError::CommitteeSuspended);
        }

        // Check if level exceeds committee's max
        if level > committee.max_kyc_level {
            return Err(TestError::LevelExceedsMax);
        }

        // Check for downgrade
        if let Some(existing) = self.kyc_data.get(user) {
            if level < existing.level {
                return Err(TestError::LevelDowngradeNotAllowed);
            }
        }

        // Check data_hash is not zero
        if data_hash == &Hash::zero() {
            return Err(TestError::InvalidLevel); // Reusing error for invalid hash
        }

        let kyc = TestKycData {
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

    // Enhanced bootstrap with validation
    fn bootstrap_validated(
        &mut self,
        name: String,
        members: Vec<CommitteeMember>,
        threshold: u8,
        kyc_threshold: u8,
        max_kyc_level: u16,
    ) -> Result<Hash, TestError> {
        // Check if already bootstrapped
        if self.global_committee_id.is_some() {
            return Err(TestError::GlobalCommitteeAlreadyBootstrapped);
        }

        // Check name not empty
        if name.is_empty() {
            return Err(TestError::EmptyName);
        }

        // Check member count
        if members.len() < MIN_COMMITTEE_MEMBERS {
            return Err(TestError::InsufficientMembers);
        }
        if members.len() > MAX_COMMITTEE_MEMBERS {
            return Err(TestError::MaxMembersExceeded);
        }

        // Check for duplicate members
        let mut seen_keys = std::collections::HashSet::new();
        for member in &members {
            if !seen_keys.insert(member.public_key.clone()) {
                return Err(TestError::DuplicateMember);
            }
        }

        // Check threshold validity
        let min_threshold = Self::calculate_min_threshold(members.len());
        if (threshold as usize) < min_threshold {
            return Err(TestError::InvalidThreshold);
        }
        if (threshold as usize) > members.len() {
            return Err(TestError::InvalidThreshold);
        }

        // Check kyc_threshold
        if kyc_threshold == 0 {
            return Err(TestError::InvalidThreshold);
        }
        if (kyc_threshold as usize) > members.len() {
            return Err(TestError::InvalidThreshold);
        }

        // Check max_kyc_level is valid
        if !Self::is_valid_level(max_kyc_level) {
            return Err(TestError::InvalidLevel);
        }

        let committee = SecurityCommittee::new_global(
            name,
            members,
            threshold,
            kyc_threshold,
            max_kyc_level,
            self.current_time,
        );

        let committee_id = committee.id.clone();
        self.committees.insert(committee_id.clone(), committee);
        self.global_committee_id = Some(committee_id.clone());
        Ok(committee_id)
    }

    // Enhanced register with validation
    fn register_validated(
        &mut self,
        name: String,
        region: KycRegion,
        members: Vec<CommitteeMember>,
        threshold: u8,
        kyc_threshold: u8,
        max_kyc_level: u16,
        parent_id: &Hash,
    ) -> Result<Hash, TestError> {
        // Check global committee is bootstrapped
        if self.global_committee_id.is_none() {
            return Err(TestError::GlobalCommitteeNotBootstrapped);
        }

        // Check parent exists
        let parent = self
            .committees
            .get(parent_id)
            .ok_or(TestError::ParentNotFound)?;

        // Check max_kyc_level doesn't exceed parent's
        if max_kyc_level > parent.max_kyc_level {
            return Err(TestError::LevelExceedsMax);
        }

        // Check name not empty
        if name.is_empty() {
            return Err(TestError::EmptyName);
        }

        // Check member count
        if members.len() < MIN_COMMITTEE_MEMBERS {
            return Err(TestError::InsufficientMembers);
        }
        if members.len() > MAX_COMMITTEE_MEMBERS {
            return Err(TestError::MaxMembersExceeded);
        }

        // Check for duplicate members
        let mut seen_keys = std::collections::HashSet::new();
        for member in &members {
            if !seen_keys.insert(member.public_key.clone()) {
                return Err(TestError::DuplicateMember);
            }
        }

        // Check threshold validity
        let min_threshold = Self::calculate_min_threshold(members.len());
        if (threshold as usize) < min_threshold {
            return Err(TestError::InvalidThreshold);
        }
        if (threshold as usize) > members.len() {
            return Err(TestError::InvalidThreshold);
        }

        // Check kyc_threshold
        if kyc_threshold == 0 {
            return Err(TestError::InvalidThreshold);
        }

        let committee = SecurityCommittee::new_regional(
            name,
            region,
            members,
            threshold,
            kyc_threshold,
            max_kyc_level,
            parent_id.clone(),
            self.current_time,
        );

        let committee_id = committee.id.clone();
        self.committees.insert(committee_id.clone(), committee);
        Ok(committee_id)
    }

    // Transfer with validation
    fn transfer_validated(
        &mut self,
        user: &CompressedPublicKey,
        source_committee_id: &Hash,
        dest_committee_id: &Hash,
        new_data_hash: &Hash,
    ) -> Result<(), TestError> {
        // Check same committee
        if source_committee_id == dest_committee_id {
            return Err(TestError::SameCommittee);
        }

        // Check committees exist
        if !self.committees.contains_key(source_committee_id) {
            return Err(TestError::CommitteeNotFound);
        }
        let dest_committee = self
            .committees
            .get(dest_committee_id)
            .ok_or(TestError::CommitteeNotFound)?;

        // Check KYC exists
        let kyc = self.kyc_data.get(user).ok_or(TestError::KycNotFound)?;

        // Check status allows transfer
        if kyc.status == KycStatus::Revoked || kyc.status == KycStatus::Suspended {
            return Err(TestError::InvalidStatus);
        }

        // Check level doesn't exceed destination committee's max
        if kyc.level > dest_committee.max_kyc_level {
            return Err(TestError::LevelExceedsMax);
        }

        // Perform transfer
        let kyc = self.kyc_data.get_mut(user).ok_or(TestError::KycNotFound)?;
        kyc.committee_id = dest_committee_id.clone();
        kyc.data_hash = new_data_hash.clone();
        kyc.verified_at = self.current_time;
        Ok(())
    }
}

// ============================================================================
// BlockchainVerificationState Implementation
// ============================================================================

#[async_trait]
impl<'a> BlockchainVerificationState<'a, TestError> for EdgeCaseTestState {
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
}

// ============================================================================
// BlockchainApplyState Implementation
// ============================================================================

#[async_trait]
impl<'a> BlockchainApplyState<'a, DummyContractProvider, TestError> for EdgeCaseTestState {
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

    async fn get_energy_resource(
        &mut self,
        _account: Cow<'a, CompressedPublicKey>,
    ) -> Result<Option<EnergyResource>, TestError> {
        Ok(None)
    }

    async fn set_energy_resource(
        &mut self,
        _account: Cow<'a, CompressedPublicKey>,
        _energy_resource: EnergyResource,
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
        self.set_kyc_validated(user, level, verified_at, data_hash, committee_id)
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
        // If suspended, can renew but stay suspended
        // If expired, renew restores to active
        if kyc.status == KycStatus::Expired {
            kyc.status = KycStatus::Active;
        }
        Ok(())
    }

    async fn transfer_kyc(
        &mut self,
        user: &'a CompressedPublicKey,
        source_committee_id: &'a Hash,
        dest_committee_id: &'a Hash,
        new_data_hash: &'a Hash,
        _transferred_at: u64,
        _tx_hash: &'a Hash,
        _dest_max_kyc_level: u16,
        _verification_timestamp: u64,
    ) -> Result<(), TestError> {
        self.transfer_validated(user, source_committee_id, dest_committee_id, new_data_hash)
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
        _tx_hash: &'a Hash,
    ) -> Result<Hash, TestError> {
        let committee_members: Vec<CommitteeMember> = members
            .into_iter()
            .map(|m| m.into_member(self.current_time))
            .collect();
        self.bootstrap_validated(
            name,
            committee_members,
            threshold,
            kyc_threshold,
            max_kyc_level,
        )
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
        _tx_hash: &'a Hash,
    ) -> Result<Hash, TestError> {
        let committee_members: Vec<CommitteeMember> = members
            .into_iter()
            .map(|m| m.into_member(self.current_time))
            .collect();
        self.register_validated(
            name,
            region,
            committee_members,
            threshold,
            kyc_threshold,
            max_kyc_level,
            parent_id,
        )
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
                if committee.members.len() >= MAX_COMMITTEE_MEMBERS {
                    return Err(TestError::MaxMembersExceeded);
                }
                // Check for duplicate
                if committee
                    .members
                    .iter()
                    .any(|m| m.public_key == *public_key)
                {
                    return Err(TestError::DuplicateMember);
                }
                committee.add_member(public_key.clone(), name.clone(), *role);
            }
            CommitteeUpdateData::RemoveMember { public_key } => {
                if committee.members.len() <= MIN_COMMITTEE_MEMBERS {
                    return Err(TestError::InsufficientMembers);
                }
                // Check if removal would break threshold
                let active_after = committee
                    .members
                    .iter()
                    .filter(|m| m.public_key != *public_key && m.status == MemberStatus::Active)
                    .count();
                if active_after < (committee.threshold as usize) {
                    return Err(TestError::InvalidThreshold);
                }
                committee.remove_member(public_key);
            }
            CommitteeUpdateData::UpdateThreshold { new_threshold } => {
                let min_threshold =
                    EdgeCaseTestState::calculate_min_threshold(committee.active_member_count());
                if (*new_threshold as usize) < min_threshold {
                    return Err(TestError::InvalidThreshold);
                }
                if (*new_threshold as usize) > committee.active_member_count() {
                    return Err(TestError::InvalidThreshold);
                }
                committee.threshold = *new_threshold;
            }
            CommitteeUpdateData::UpdateKycThreshold { new_kyc_threshold } => {
                if *new_kyc_threshold == 0 {
                    return Err(TestError::InvalidThreshold);
                }
                if (*new_kyc_threshold as usize) > committee.active_member_count() {
                    return Err(TestError::InvalidThreshold);
                }
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
}

// ============================================================================
// Helper Functions
// ============================================================================

fn create_test_members(count: usize) -> (Vec<KeyPair>, Vec<tos_common::kyc::CommitteeMemberInfo>) {
    let keys: Vec<KeyPair> = (0..count).map(|_| KeyPair::new()).collect();
    let infos: Vec<tos_common::kyc::CommitteeMemberInfo> = keys
        .iter()
        .enumerate()
        .map(|(i, k)| {
            tos_common::kyc::CommitteeMemberInfo::new(
                k.get_public_key().compress(),
                Some(format!("Member-{}", i)),
                MemberRole::Member,
            )
        })
        .collect();
    (keys, infos)
}

async fn setup_global_committee(state: &mut EdgeCaseTestState) -> Hash {
    let (_, members) = create_test_members(5);
    state
        .bootstrap_global_committee(
            "Global".to_string(),
            members,
            4,
            2,
            32767,
            &Hash::new([1u8; 32]),
        )
        .await
        .expect("Should bootstrap")
}

// ============================================================================
// 1. BOUNDARY VALUE TESTS
// ============================================================================

#[tokio::test]
async fn test_all_valid_kyc_levels() {
    let mut state = EdgeCaseTestState::new();
    let global_id = setup_global_committee(&mut state).await;

    // Test all 9 valid levels
    for (i, &level) in VALID_LEVELS.iter().enumerate() {
        let user = KeyPair::new();
        let user_pk = user.get_public_key().compress();
        let data_hash = Hash::new([i as u8 + 1; 32]);

        let result = state.set_kyc_validated(&user_pk, level, 1000, &data_hash, &global_id);
        assert!(result.is_ok(), "Level {} should be valid", level);

        let kyc = state.get_kyc(&user_pk).expect("KYC should exist");
        assert_eq!(kyc.level, level);
    }
}

#[tokio::test]
async fn test_invalid_kyc_levels() {
    let mut state = EdgeCaseTestState::new();
    let global_id = setup_global_committee(&mut state).await;

    // Test various invalid levels
    let invalid_levels: [u16; 12] = [1, 2, 3, 4, 5, 6, 8, 15, 16, 17, 100, 256];

    for &level in &invalid_levels {
        let user = KeyPair::new();
        let user_pk = user.get_public_key().compress();
        let data_hash = Hash::new([1u8; 32]);

        let result = state.set_kyc_validated(&user_pk, level, 1000, &data_hash, &global_id);
        assert!(
            matches!(result, Err(TestError::InvalidLevel)),
            "Level {} should be invalid",
            level
        );
    }
}

#[tokio::test]
async fn test_kyc_level_boundary_values() {
    let mut state = EdgeCaseTestState::new();
    let global_id = setup_global_committee(&mut state).await;

    // Test boundary values around valid levels
    let boundary_tests = [
        (6, false),     // Just below 7
        (7, true),      // Valid Tier 1
        (8, false),     // Just above 7
        (30, false),    // Just below 31
        (31, true),     // Valid Tier 2
        (32, false),    // Just above 31
        (62, false),    // Just below 63
        (63, true),     // Valid Tier 3
        (64, false),    // Just above 63
        (254, false),   // Just below 255
        (255, true),    // Valid Tier 4
        (256, false),   // Just above 255
        (32766, false), // Just below 32767
        (32767, true),  // Valid Tier 8 (max)
        (32768, false), // Just above 32767
        (65535, false), // u16::MAX
    ];

    for (level, should_be_valid) in boundary_tests {
        let user = KeyPair::new();
        let user_pk = user.get_public_key().compress();
        let data_hash = Hash::new([1u8; 32]);

        let result = state.set_kyc_validated(&user_pk, level, 1000, &data_hash, &global_id);

        if should_be_valid {
            assert!(result.is_ok(), "Level {} should be valid", level);
        } else {
            assert!(result.is_err(), "Level {} should be invalid", level);
        }
    }
}

#[tokio::test]
async fn test_committee_member_count_boundaries() {
    let mut state = EdgeCaseTestState::new();

    // Test with exactly MIN_COMMITTEE_MEMBERS (3)
    let (_, members_3) = create_test_members(3);
    let result = state
        .bootstrap_global_committee(
            "Global".to_string(),
            members_3,
            2, // min threshold for 3 members
            1,
            32767,
            &Hash::new([1u8; 32]),
        )
        .await;
    assert!(result.is_ok(), "3 members should be valid");

    // Reset and test with 2 members (below minimum)
    let mut state2 = EdgeCaseTestState::new();
    let (_, members_2) = create_test_members(2);
    let result = state2
        .bootstrap_global_committee(
            "Global".to_string(),
            members_2,
            2,
            1,
            32767,
            &Hash::new([1u8; 32]),
        )
        .await;
    assert!(
        matches!(result, Err(TestError::InsufficientMembers)),
        "2 members should fail"
    );

    // Test with MAX_COMMITTEE_MEMBERS (21)
    let mut state3 = EdgeCaseTestState::new();
    let (_, members_21) = create_test_members(21);
    let result = state3
        .bootstrap_global_committee(
            "Global".to_string(),
            members_21,
            14, // ceil(21 * 2/3) = 14
            1,
            32767,
            &Hash::new([1u8; 32]),
        )
        .await;
    assert!(result.is_ok(), "21 members should be valid");

    // Test with 22 members (above maximum)
    let mut state4 = EdgeCaseTestState::new();
    let (_, members_22) = create_test_members(22);
    let result = state4
        .bootstrap_global_committee(
            "Global".to_string(),
            members_22,
            15,
            1,
            32767,
            &Hash::new([1u8; 32]),
        )
        .await;
    assert!(
        matches!(result, Err(TestError::MaxMembersExceeded)),
        "22 members should fail"
    );
}

#[tokio::test]
async fn test_threshold_two_thirds_rule() {
    // Test the 2/3 rule for various member counts
    let test_cases = [
        (3, 2),   // ceil(6/3) = 2
        (4, 3),   // ceil(8/3) = 3
        (5, 4),   // ceil(10/3) = 4
        (6, 4),   // ceil(12/3) = 4
        (7, 5),   // ceil(14/3) = 5
        (10, 7),  // ceil(20/3) = 7
        (11, 8),  // ceil(22/3) = 8
        (15, 10), // ceil(30/3) = 10
        (21, 14), // ceil(42/3) = 14
    ];

    for (member_count, expected_min_threshold) in test_cases {
        let mut state = EdgeCaseTestState::new();
        let (_, members) = create_test_members(member_count);

        // Test with exactly min threshold (should succeed)
        let result = state
            .bootstrap_global_committee(
                "Global".to_string(),
                members.clone(),
                expected_min_threshold,
                1,
                32767,
                &Hash::new([1u8; 32]),
            )
            .await;
        assert!(
            result.is_ok(),
            "{} members with threshold {} should succeed",
            member_count,
            expected_min_threshold
        );

        // Test with threshold - 1 (should fail)
        let mut state2 = EdgeCaseTestState::new();
        if expected_min_threshold > 1 {
            let result = state2
                .bootstrap_global_committee(
                    "Global".to_string(),
                    members,
                    expected_min_threshold - 1,
                    1,
                    32767,
                    &Hash::new([2u8; 32]),
                )
                .await;
            assert!(
                matches!(result, Err(TestError::InvalidThreshold)),
                "{} members with threshold {} should fail",
                member_count,
                expected_min_threshold - 1
            );
        }
    }
}

// ============================================================================
// 2. ERROR HANDLING TESTS
// ============================================================================

#[tokio::test]
async fn test_empty_committee_name() {
    let mut state = EdgeCaseTestState::new();
    let (_, members) = create_test_members(5);

    let result = state
        .bootstrap_global_committee(
            "".to_string(), // Empty name
            members,
            4,
            2,
            32767,
            &Hash::new([1u8; 32]),
        )
        .await;

    assert!(
        matches!(result, Err(TestError::EmptyName)),
        "Empty name should fail"
    );
}

#[tokio::test]
async fn test_duplicate_committee_members() {
    let mut state = EdgeCaseTestState::new();
    let key = KeyPair::new();
    let pk = key.get_public_key().compress();

    // Create members with duplicate public keys
    let members = vec![
        tos_common::kyc::CommitteeMemberInfo::new(
            pk.clone(),
            Some("Member-1".to_string()),
            MemberRole::Member,
        ),
        tos_common::kyc::CommitteeMemberInfo::new(
            pk.clone(),
            Some("Member-2".to_string()),
            MemberRole::Member,
        ),
        tos_common::kyc::CommitteeMemberInfo::new(
            pk.clone(),
            Some("Member-3".to_string()),
            MemberRole::Member,
        ),
    ];

    let result = state
        .bootstrap_global_committee(
            "Global".to_string(),
            members,
            2,
            1,
            32767,
            &Hash::new([1u8; 32]),
        )
        .await;

    assert!(
        matches!(result, Err(TestError::DuplicateMember)),
        "Duplicate members should fail"
    );
}

#[tokio::test]
async fn test_zero_data_hash() {
    let mut state = EdgeCaseTestState::new();
    let global_id = setup_global_committee(&mut state).await;

    let user = KeyPair::new();
    let user_pk = user.get_public_key().compress();

    let result = state.set_kyc_validated(&user_pk, 7, 1000, &Hash::zero(), &global_id);

    assert!(result.is_err(), "Zero data hash should fail");
}

#[tokio::test]
async fn test_kyc_on_suspended_committee() {
    let mut state = EdgeCaseTestState::new();
    let global_id = setup_global_committee(&mut state).await;

    // Suspend the committee
    state
        .update_committee(&global_id, &CommitteeUpdateData::SuspendCommittee)
        .await
        .expect("Should suspend");

    let user = KeyPair::new();
    let user_pk = user.get_public_key().compress();
    let data_hash = Hash::new([1u8; 32]);

    let result = state.set_kyc_validated(&user_pk, 7, 1000, &data_hash, &global_id);

    assert!(
        matches!(result, Err(TestError::CommitteeSuspended)),
        "KYC on suspended committee should fail"
    );
}

#[tokio::test]
async fn test_kyc_threshold_zero() {
    let mut state = EdgeCaseTestState::new();
    let (_, members) = create_test_members(5);

    let result = state
        .bootstrap_global_committee(
            "Global".to_string(),
            members,
            4,
            0, // kyc_threshold = 0 (invalid)
            32767,
            &Hash::new([1u8; 32]),
        )
        .await;

    assert!(
        matches!(result, Err(TestError::InvalidThreshold)),
        "kyc_threshold = 0 should fail"
    );
}

#[tokio::test]
async fn test_kyc_threshold_exceeds_members() {
    let mut state = EdgeCaseTestState::new();
    let (_, members) = create_test_members(5);

    let result = state
        .bootstrap_global_committee(
            "Global".to_string(),
            members,
            4,
            6, // kyc_threshold > member_count
            32767,
            &Hash::new([1u8; 32]),
        )
        .await;

    assert!(
        matches!(result, Err(TestError::InvalidThreshold)),
        "kyc_threshold > members should fail"
    );
}

#[tokio::test]
async fn test_governance_threshold_exceeds_members() {
    let mut state = EdgeCaseTestState::new();
    let (_, members) = create_test_members(5);

    let result = state
        .bootstrap_global_committee(
            "Global".to_string(),
            members,
            6, // threshold > member_count
            2,
            32767,
            &Hash::new([1u8; 32]),
        )
        .await;

    assert!(
        matches!(result, Err(TestError::InvalidThreshold)),
        "threshold > members should fail"
    );
}

// ============================================================================
// 3. LEVEL DOWNGRADE PREVENTION TESTS
// ============================================================================

#[tokio::test]
async fn test_level_downgrade_prevention() {
    let mut state = EdgeCaseTestState::new();
    let global_id = setup_global_committee(&mut state).await;

    let user = KeyPair::new();
    let user_pk = user.get_public_key().compress();

    // Set initial level to Tier 3 (63)
    state
        .set_kyc_validated(&user_pk, 63, 1000, &Hash::new([1u8; 32]), &global_id)
        .expect("Should set KYC");

    // Try to downgrade to Tier 2 (31)
    let result = state.set_kyc_validated(&user_pk, 31, 2000, &Hash::new([2u8; 32]), &global_id);

    assert!(
        matches!(result, Err(TestError::LevelDowngradeNotAllowed)),
        "Downgrade should fail"
    );

    // Verify level is unchanged
    let kyc = state.get_kyc(&user_pk).expect("KYC should exist");
    assert_eq!(kyc.level, 63);
}

#[tokio::test]
async fn test_level_upgrade_allowed() {
    let mut state = EdgeCaseTestState::new();
    let global_id = setup_global_committee(&mut state).await;

    let user = KeyPair::new();
    let user_pk = user.get_public_key().compress();

    // Set initial level to Tier 1 (7)
    state
        .set_kyc_validated(&user_pk, 7, 1000, &Hash::new([1u8; 32]), &global_id)
        .expect("Should set KYC");

    // Upgrade to Tier 4 (255)
    let result = state.set_kyc_validated(&user_pk, 255, 2000, &Hash::new([2u8; 32]), &global_id);

    assert!(result.is_ok(), "Upgrade should succeed");

    let kyc = state.get_kyc(&user_pk).expect("KYC should exist");
    assert_eq!(kyc.level, 255);
}

#[tokio::test]
async fn test_same_level_update_allowed() {
    let mut state = EdgeCaseTestState::new();
    let global_id = setup_global_committee(&mut state).await;

    let user = KeyPair::new();
    let user_pk = user.get_public_key().compress();

    // Set initial level
    state
        .set_kyc_validated(&user_pk, 63, 1000, &Hash::new([1u8; 32]), &global_id)
        .expect("Should set KYC");

    // Update with same level (different data hash)
    let result = state.set_kyc_validated(&user_pk, 63, 2000, &Hash::new([2u8; 32]), &global_id);

    assert!(result.is_ok(), "Same level update should succeed");
}

// ============================================================================
// 4. STATE TRANSITION TESTS
// ============================================================================

#[tokio::test]
async fn test_revoked_cannot_renew() {
    let mut state = EdgeCaseTestState::new();
    let global_id = setup_global_committee(&mut state).await;

    let user = KeyPair::new();
    let user_pk = user.get_public_key().compress();

    // Set and revoke KYC
    state
        .set_kyc_validated(&user_pk, 31, 1000, &Hash::new([1u8; 32]), &global_id)
        .expect("Should set");
    state
        .revoke_kyc(&user_pk, &Hash::new([2u8; 32]), &Hash::new([3u8; 32]))
        .await
        .expect("Should revoke");

    // Try to renew
    let result = state
        .renew_kyc(&user_pk, 2000, &Hash::new([4u8; 32]), &Hash::new([5u8; 32]))
        .await;

    assert!(
        matches!(result, Err(TestError::InvalidStatus)),
        "Revoked KYC should not renew"
    );
}

#[tokio::test]
async fn test_expired_can_renew() {
    let mut state = EdgeCaseTestState::new();
    let global_id = setup_global_committee(&mut state).await;

    let user = KeyPair::new();
    let user_pk = user.get_public_key().compress();

    // Set KYC
    state
        .set_kyc_validated(&user_pk, 31, 1000, &Hash::new([1u8; 32]), &global_id)
        .expect("Should set");

    // Manually set to expired
    let kyc = state.kyc_data.get_mut(&user_pk).unwrap();
    kyc.status = KycStatus::Expired;

    // Renew should succeed and restore to Active
    let result = state
        .renew_kyc(&user_pk, 2000, &Hash::new([2u8; 32]), &Hash::new([3u8; 32]))
        .await;

    assert!(result.is_ok(), "Expired KYC should renew");

    let kyc = state.get_kyc(&user_pk).expect("KYC should exist");
    assert_eq!(kyc.status, KycStatus::Active);
}

#[tokio::test]
async fn test_suspended_cannot_transfer() {
    let mut state = EdgeCaseTestState::new();
    let global_id = setup_global_committee(&mut state).await;

    // Register regional committee
    let (_, regional_members) = create_test_members(3);
    let regional_id = state
        .register_committee(
            "Regional".to_string(),
            KycRegion::AsiaPacific,
            regional_members,
            2,
            1,
            255,
            &global_id,
            &Hash::new([2u8; 32]),
        )
        .await
        .expect("Should register");

    let user = KeyPair::new();
    let user_pk = user.get_public_key().compress();

    // Set and suspend KYC
    state
        .set_kyc_validated(&user_pk, 31, 1000, &Hash::new([1u8; 32]), &global_id)
        .expect("Should set");
    state
        .emergency_suspend_kyc(
            &user_pk,
            &Hash::new([2u8; 32]),
            state.current_time + 86400,
            &Hash::new([3u8; 32]),
        )
        .await
        .expect("Should suspend");

    // Try to transfer
    let result =
        state.transfer_validated(&user_pk, &global_id, &regional_id, &Hash::new([4u8; 32]));

    assert!(
        matches!(result, Err(TestError::InvalidStatus)),
        "Suspended KYC should not transfer"
    );
}

#[tokio::test]
async fn test_previous_status_preserved() {
    let mut state = EdgeCaseTestState::new();
    let global_id = setup_global_committee(&mut state).await;

    let user = KeyPair::new();
    let user_pk = user.get_public_key().compress();

    // Set KYC to Active
    state
        .set_kyc_validated(&user_pk, 31, 1000, &Hash::new([1u8; 32]), &global_id)
        .expect("Should set");

    // Emergency suspend
    state
        .emergency_suspend_kyc(
            &user_pk,
            &Hash::new([2u8; 32]),
            state.current_time + 86400,
            &Hash::new([3u8; 32]),
        )
        .await
        .expect("Should suspend");

    let kyc = state.get_kyc(&user_pk).expect("KYC should exist");
    assert_eq!(kyc.status, KycStatus::Suspended);
    assert_eq!(kyc.previous_status, Some(KycStatus::Active));
}

// ============================================================================
// 5. TRANSFER EDGE CASES
// ============================================================================

#[tokio::test]
async fn test_transfer_to_same_committee() {
    let mut state = EdgeCaseTestState::new();
    let global_id = setup_global_committee(&mut state).await;

    let user = KeyPair::new();
    let user_pk = user.get_public_key().compress();

    state
        .set_kyc_validated(&user_pk, 31, 1000, &Hash::new([1u8; 32]), &global_id)
        .expect("Should set");

    // Try to transfer to same committee
    let result = state.transfer_validated(&user_pk, &global_id, &global_id, &Hash::new([2u8; 32]));

    assert!(
        matches!(result, Err(TestError::SameCommittee)),
        "Transfer to same committee should fail"
    );
}

#[tokio::test]
async fn test_transfer_level_exceeds_destination_max() {
    let mut state = EdgeCaseTestState::new();
    let global_id = setup_global_committee(&mut state).await;

    // Register regional committee with lower max level
    let (_, regional_members) = create_test_members(3);
    let regional_id = state
        .register_committee(
            "Regional".to_string(),
            KycRegion::AsiaPacific,
            regional_members,
            2,
            1,
            63, // max_kyc_level = 63 (Tier 3)
            &global_id,
            &Hash::new([2u8; 32]),
        )
        .await
        .expect("Should register");

    let user = KeyPair::new();
    let user_pk = user.get_public_key().compress();

    // Set KYC with level 255 (Tier 4) in global committee
    state
        .set_kyc_validated(&user_pk, 255, 1000, &Hash::new([1u8; 32]), &global_id)
        .expect("Should set");

    // Try to transfer to regional with lower max
    let result =
        state.transfer_validated(&user_pk, &global_id, &regional_id, &Hash::new([3u8; 32]));

    assert!(
        matches!(result, Err(TestError::LevelExceedsMax)),
        "Transfer exceeding destination max should fail"
    );
}

#[tokio::test]
async fn test_transfer_nonexistent_user() {
    let mut state = EdgeCaseTestState::new();
    let global_id = setup_global_committee(&mut state).await;

    let (_, regional_members) = create_test_members(3);
    let regional_id = state
        .register_committee(
            "Regional".to_string(),
            KycRegion::AsiaPacific,
            regional_members,
            2,
            1,
            255,
            &global_id,
            &Hash::new([2u8; 32]),
        )
        .await
        .expect("Should register");

    let user = KeyPair::new();
    let user_pk = user.get_public_key().compress();

    // Try to transfer non-existent user
    let result =
        state.transfer_validated(&user_pk, &global_id, &regional_id, &Hash::new([1u8; 32]));

    assert!(
        matches!(result, Err(TestError::KycNotFound)),
        "Transfer non-existent user should fail"
    );
}

// ============================================================================
// 6. COMMITTEE UPDATE EDGE CASES
// ============================================================================

#[tokio::test]
async fn test_remove_member_below_minimum() {
    let mut state = EdgeCaseTestState::new();

    // Bootstrap with exactly 3 members
    let (keys, members) = create_test_members(3);
    let global_id = state
        .bootstrap_global_committee(
            "Global".to_string(),
            members,
            2,
            1,
            32767,
            &Hash::new([1u8; 32]),
        )
        .await
        .expect("Should bootstrap");

    // Try to remove a member
    let result = state
        .update_committee(
            &global_id,
            &CommitteeUpdateData::RemoveMember {
                public_key: keys[0].get_public_key().compress(),
            },
        )
        .await;

    assert!(
        matches!(result, Err(TestError::InsufficientMembers)),
        "Removing below minimum should fail"
    );
}

#[tokio::test]
async fn test_add_member_above_maximum() {
    let mut state = EdgeCaseTestState::new();

    // Bootstrap with 21 members (max)
    let (_, members) = create_test_members(21);
    let global_id = state
        .bootstrap_global_committee(
            "Global".to_string(),
            members,
            14,
            1,
            32767,
            &Hash::new([1u8; 32]),
        )
        .await
        .expect("Should bootstrap");

    // Try to add another member
    let new_member = KeyPair::new();
    let result = state
        .update_committee(
            &global_id,
            &CommitteeUpdateData::AddMember {
                public_key: new_member.get_public_key().compress(),
                name: Some("New".to_string()),
                role: MemberRole::Member,
            },
        )
        .await;

    assert!(
        matches!(result, Err(TestError::MaxMembersExceeded)),
        "Adding above maximum should fail"
    );
}

#[tokio::test]
async fn test_add_duplicate_member() {
    let mut state = EdgeCaseTestState::new();
    let (keys, members) = create_test_members(5);
    let global_id = state
        .bootstrap_global_committee(
            "Global".to_string(),
            members,
            4,
            1,
            32767,
            &Hash::new([1u8; 32]),
        )
        .await
        .expect("Should bootstrap");

    // Try to add existing member
    let result = state
        .update_committee(
            &global_id,
            &CommitteeUpdateData::AddMember {
                public_key: keys[0].get_public_key().compress(),
                name: Some("Duplicate".to_string()),
                role: MemberRole::Member,
            },
        )
        .await;

    assert!(
        matches!(result, Err(TestError::DuplicateMember)),
        "Adding duplicate member should fail"
    );
}

#[tokio::test]
async fn test_update_threshold_below_two_thirds() {
    let mut state = EdgeCaseTestState::new();
    let (_, members) = create_test_members(6);
    let global_id = state
        .bootstrap_global_committee(
            "Global".to_string(),
            members,
            4,
            1,
            32767,
            &Hash::new([1u8; 32]),
        )
        .await
        .expect("Should bootstrap");

    // Try to update threshold below 2/3 (min for 6 is 4)
    let result = state
        .update_committee(
            &global_id,
            &CommitteeUpdateData::UpdateThreshold { new_threshold: 3 },
        )
        .await;

    assert!(
        matches!(result, Err(TestError::InvalidThreshold)),
        "Threshold below 2/3 should fail"
    );
}

#[tokio::test]
async fn test_update_kyc_threshold_to_zero() {
    let mut state = EdgeCaseTestState::new();
    let global_id = setup_global_committee(&mut state).await;

    let result = state
        .update_committee(
            &global_id,
            &CommitteeUpdateData::UpdateKycThreshold {
                new_kyc_threshold: 0,
            },
        )
        .await;

    assert!(
        matches!(result, Err(TestError::InvalidThreshold)),
        "kyc_threshold = 0 should fail"
    );
}

// ============================================================================
// 7. REGIONAL COMMITTEE EDGE CASES
// ============================================================================

#[tokio::test]
async fn test_register_without_global_committee() {
    let mut state = EdgeCaseTestState::new();
    let (_, members) = create_test_members(3);

    let result = state
        .register_committee(
            "Regional".to_string(),
            KycRegion::AsiaPacific,
            members,
            2,
            1,
            255,
            &Hash::new([1u8; 32]), // Non-existent parent
            &Hash::new([2u8; 32]),
        )
        .await;

    assert!(
        matches!(result, Err(TestError::GlobalCommitteeNotBootstrapped)),
        "Register without global should fail"
    );
}

#[tokio::test]
async fn test_register_with_nonexistent_parent() {
    let mut state = EdgeCaseTestState::new();
    let _global_id = setup_global_committee(&mut state).await;

    let (_, members) = create_test_members(3);
    let fake_parent = Hash::new([99u8; 32]);

    let result = state
        .register_committee(
            "Regional".to_string(),
            KycRegion::AsiaPacific,
            members,
            2,
            1,
            255,
            &fake_parent,
            &Hash::new([2u8; 32]),
        )
        .await;

    assert!(
        matches!(result, Err(TestError::ParentNotFound)),
        "Register with fake parent should fail"
    );
}

#[tokio::test]
async fn test_register_with_level_exceeding_parent() {
    let mut state = EdgeCaseTestState::new();

    // Bootstrap global with max level 255
    let (_, global_members) = create_test_members(5);
    let global_id = state
        .bootstrap_global_committee(
            "Global".to_string(),
            global_members,
            4,
            1,
            255, // max_kyc_level = 255
            &Hash::new([1u8; 32]),
        )
        .await
        .expect("Should bootstrap");

    let (_, regional_members) = create_test_members(3);
    let result = state
        .register_committee(
            "Regional".to_string(),
            KycRegion::AsiaPacific,
            regional_members,
            2,
            1,
            2047, // Exceeds parent's 255
            &global_id,
            &Hash::new([2u8; 32]),
        )
        .await;

    assert!(
        matches!(result, Err(TestError::LevelExceedsMax)),
        "Regional max > parent max should fail"
    );
}

// ============================================================================
// 8. TIME-BASED EDGE CASES
// ============================================================================

#[tokio::test]
async fn test_approval_expiry_boundary() {
    let state = EdgeCaseTestState::with_time(1000000);

    // The expiry check uses `>` not `>=`, so at exactly APPROVAL_EXPIRY_SECONDS
    // the approval is still valid (it's the last valid second)

    // Test approval at exactly expiry boundary (age == APPROVAL_EXPIRY_SECONDS)
    // This is still valid because we use > not >=
    let approval_time = 1000000 - APPROVAL_EXPIRY_SECONDS;
    assert!(
        !state.is_approval_expired(approval_time),
        "Approval at exactly expiry boundary should still be valid"
    );

    // Test approval 1 second younger than expiry boundary (within valid window)
    let approval_time = 1000000 - APPROVAL_EXPIRY_SECONDS + 1;
    assert!(
        !state.is_approval_expired(approval_time),
        "Approval within expiry window should be valid"
    );

    // Test approval 1 second older than expiry boundary (past the window)
    let approval_time = 1000000 - APPROVAL_EXPIRY_SECONDS - 1;
    assert!(
        state.is_approval_expired(approval_time),
        "Approval past expiry window should be expired"
    );
}

#[tokio::test]
async fn test_future_approval_detection() {
    let state = EdgeCaseTestState::with_time(1000000);

    // Test approval at exactly 1 hour in future (within tolerance)
    let approval_time = 1000000 + 3600;
    assert!(
        !state.is_approval_from_future(approval_time),
        "Approval at exactly 1h future should be valid"
    );

    // Test approval 1 second beyond tolerance
    let approval_time = 1000000 + 3601;
    assert!(
        state.is_approval_from_future(approval_time),
        "Approval beyond 1h future should be rejected"
    );
}

#[tokio::test]
async fn test_timestamp_overflow_handling() {
    // Test with time near u64::MAX
    let state = EdgeCaseTestState::with_time(u64::MAX - 1000);

    // Approval expiry check should use saturating_sub
    let approval_time = 1000;
    let is_expired = state.is_approval_expired(approval_time);
    // Should not panic and should correctly identify as expired
    assert!(is_expired, "Ancient approval should be expired");

    // Future check should use saturating_add
    // When current_time + 3600 saturates to u64::MAX,
    // checking if u64::MAX > u64::MAX returns false
    // This is the correct overflow protection behavior
    let future_time = u64::MAX;
    let is_future = state.is_approval_from_future(future_time);
    // With saturating_add, u64::MAX is NOT detected as future because
    // (u64::MAX - 1000).saturating_add(3600) = u64::MAX, and u64::MAX > u64::MAX is false
    // This is safe because such extreme timestamps are not realistic
    assert!(
        !is_future,
        "Saturating add prevents overflow - max time equals saturation point"
    );

    // Test case where overflow would occur without saturation
    // But with saturation, future detection still works for realistic values
    let state2 = EdgeCaseTestState::with_time(u64::MAX - 5000);
    let future_time = u64::MAX - 100;
    // (u64::MAX - 5000).saturating_add(3600) = u64::MAX - 1400
    // u64::MAX - 100 > u64::MAX - 1400 = true
    let is_future2 = state2.is_approval_from_future(future_time);
    assert!(
        is_future2,
        "Future detection works near max with saturation"
    );
}

#[tokio::test]
async fn test_zero_timestamp() {
    let mut state = EdgeCaseTestState::with_time(1000000);
    let global_id = setup_global_committee(&mut state).await;

    let user = KeyPair::new();
    let user_pk = user.get_public_key().compress();

    // Set KYC with verified_at = 0
    let result = state.set_kyc_validated(&user_pk, 7, 0, &Hash::new([1u8; 32]), &global_id);

    // Should succeed (verified_at = 0 is genesis time)
    assert!(result.is_ok(), "Zero timestamp should be valid");
}

// ============================================================================
// 9. ALL REGION COVERAGE TESTS
// ============================================================================

#[tokio::test]
async fn test_all_regions() {
    let mut state = EdgeCaseTestState::new();
    let global_id = setup_global_committee(&mut state).await;

    let regions = [
        KycRegion::Global,
        KycRegion::AsiaPacific,
        KycRegion::Europe,
        KycRegion::NorthAmerica,
        KycRegion::LatinAmerica,
        KycRegion::MiddleEast,
        KycRegion::Africa,
        KycRegion::Oceania,
    ];

    for (i, region) in regions.iter().enumerate() {
        if *region == KycRegion::Global {
            continue; // Skip global, already bootstrapped
        }

        let (_, members) = create_test_members(3);
        let result = state
            .register_committee(
                format!("Committee-{:?}", region),
                *region,
                members,
                2,
                1,
                255,
                &global_id,
                &Hash::new([(i + 10) as u8; 32]),
            )
            .await;

        assert!(result.is_ok(), "Region {:?} should register", region);
    }
}

// ============================================================================
// 10. BOOTSTRAP IDEMPOTENCY TEST
// ============================================================================

#[tokio::test]
async fn test_double_bootstrap() {
    let mut state = EdgeCaseTestState::new();
    let (_, members1) = create_test_members(5);
    let (_, members2) = create_test_members(5);

    // First bootstrap
    let result1 = state
        .bootstrap_global_committee(
            "Global1".to_string(),
            members1,
            4,
            1,
            32767,
            &Hash::new([1u8; 32]),
        )
        .await;
    assert!(result1.is_ok(), "First bootstrap should succeed");

    // Second bootstrap
    let result2 = state
        .bootstrap_global_committee(
            "Global2".to_string(),
            members2,
            4,
            1,
            32767,
            &Hash::new([2u8; 32]),
        )
        .await;
    assert!(
        matches!(result2, Err(TestError::GlobalCommitteeAlreadyBootstrapped)),
        "Second bootstrap should fail"
    );
}
