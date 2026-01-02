//! Integration tests for TOS Energy Batch Operations (TOS Innovation)
//!
//! Tests for:
//! - ActivateAccounts: Batch account activation (up to 500 accounts)
//! - BatchDelegateResource: Batch delegation to multiple receivers
//! - ActivateAndDelegate: Combined activation and delegation
//!
//! These operations are designed for exchanges and large dApps.

#![allow(clippy::disallowed_methods)]

use std::{borrow::Cow, collections::HashMap, sync::Arc};

use async_trait::async_trait;
use indexmap::{IndexMap, IndexSet};
use tos_common::{
    account::{AccountEnergy, DelegatedResource, GlobalEnergyState, Nonce},
    block::{Block, BlockHeader, BlockVersion, EXTRA_NONCE_SIZE},
    config::{
        COIN_VALUE, FEE_PER_ACCOUNT_CREATION, MAX_BATCH_ACTIVATE, MAX_BATCH_ACTIVATE_DELEGATE,
        MAX_BATCH_DELEGATE, MAX_DELEGATE_LOCK_DAYS, MIN_DELEGATION_AMOUNT, TOTAL_ENERGY_LIMIT,
    },
    contract::{
        AssetChanges, ChainState as ContractChainState, ContractCache, ContractEvent,
        ContractEventTracker, ContractExecutor, ContractOutput, ContractStorage,
    },
    crypto::{
        elgamal::{Ciphertext, CompressedPublicKey, PublicKey as UncompressedPublicKey},
        Hash, Hashable, KeyPair,
    },
    immutable::Immutable,
    kyc::{CommitteeMemberInfo, KycRegion, SecurityCommittee},
    network::Network,
    referral::DistributionResult,
    transaction::{
        builder::{AccountState, FeeHelper},
        verify::{BlockchainApplyState, BlockchainVerificationState, ContractEnvironment},
        ActivateDelegateItem, BatchDelegationItem, CommitteeUpdateData, ContractDeposit,
        EnergyPayload, MultiSigPayload, Reference, TransactionResult, TxVersion,
    },
};
use tos_daemon::tako_integration::TakoContractExecutor;
use tos_environment::Environment;
use tos_kernel::Module;

// =============================================================================
// Test Error Type
// =============================================================================

#[derive(Debug)]
#[allow(dead_code)]
enum TestError {
    Unsupported,
    Overflow,
    Underflow,
    AccountAlreadyRegistered,
    AccountNotRegistered,
    InsufficientBalance,
    InsufficientFrozenBalance,
}

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestError::Unsupported => write!(f, "Unsupported"),
            TestError::Overflow => write!(f, "Overflow"),
            TestError::Underflow => write!(f, "Underflow"),
            TestError::AccountAlreadyRegistered => write!(f, "Account already registered"),
            TestError::AccountNotRegistered => write!(f, "Account not registered"),
            TestError::InsufficientBalance => write!(f, "Insufficient balance"),
            TestError::InsufficientFrozenBalance => write!(f, "Insufficient frozen balance"),
        }
    }
}

impl std::error::Error for TestError {}

// =============================================================================
// Dummy Contract Provider
// =============================================================================

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
        _key: &CompressedPublicKey,
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

    fn account_exists(
        &self,
        _key: &CompressedPublicKey,
        _topoheight: u64,
    ) -> Result<bool, anyhow::Error> {
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

// =============================================================================
// Test Account State (for TransactionBuilder)
// =============================================================================

#[allow(dead_code)]
struct TestAccountState {
    balances: HashMap<Hash, u64>,
    nonce: u64,
    registered_accounts: HashMap<CompressedPublicKey, bool>,
}

#[allow(dead_code)]
impl TestAccountState {
    fn new() -> Self {
        Self {
            balances: HashMap::new(),
            nonce: 0,
            registered_accounts: HashMap::new(),
        }
    }

    fn set_balance(&mut self, asset: Hash, amount: u64) {
        self.balances.insert(asset, amount);
    }

    fn set_nonce(&mut self, nonce: u64) {
        self.nonce = nonce;
    }
}

impl FeeHelper for TestAccountState {
    type Error = Box<dyn std::error::Error>;

    fn account_exists(&self, account: &CompressedPublicKey) -> Result<bool, Self::Error> {
        Ok(self
            .registered_accounts
            .get(account)
            .copied()
            .unwrap_or(false))
    }
}

impl AccountState for TestAccountState {
    fn is_mainnet(&self) -> bool {
        false
    }

    fn get_account_balance(&self, asset: &Hash) -> Result<u64, Self::Error> {
        Ok(self.balances.get(asset).copied().unwrap_or(0))
    }

    fn get_reference(&self) -> Reference {
        Reference {
            topoheight: 0,
            hash: Hash::zero(),
        }
    }

    fn update_account_balance(
        &mut self,
        asset: &Hash,
        new_balance: u64,
    ) -> Result<(), Self::Error> {
        self.balances.insert(asset.clone(), new_balance);
        Ok(())
    }

    fn get_nonce(&self) -> Result<u64, Self::Error> {
        Ok(self.nonce)
    }

    fn update_nonce(&mut self, new_nonce: u64) -> Result<(), Self::Error> {
        self.nonce = new_nonce;
        Ok(())
    }

    fn is_account_registered(&self, key: &CompressedPublicKey) -> Result<bool, Self::Error> {
        Ok(self.registered_accounts.get(key).copied().unwrap_or(false))
    }
}

// =============================================================================
// Test Chain State
// =============================================================================

struct BatchTestChainState {
    // Balances - use CompressedPublicKey for consistency with trait
    sender_balances: HashMap<CompressedPublicKey, u64>,
    receiver_balances: HashMap<CompressedPublicKey, u64>,
    nonces: HashMap<CompressedPublicKey, Nonce>,

    // Account registration tracking
    registered_accounts: HashMap<CompressedPublicKey, bool>,

    // Energy system
    account_energies: HashMap<CompressedPublicKey, AccountEnergy>,
    global_energy_state: GlobalEnergyState,
    delegated_resources: HashMap<(CompressedPublicKey, CompressedPublicKey), DelegatedResource>,

    // Block context
    environment: Environment,
    block: Block,
    block_hash: Hash,
    burned: u64,
    gas_fee: u64,
    executor: Arc<dyn ContractExecutor>,
    _contract_provider: DummyContractProvider,
}

impl BatchTestChainState {
    fn new() -> Self {
        Self::new_with_timestamp(
            std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        )
    }

    fn new_with_timestamp(timestamp_ms: u64) -> Self {
        let miner = KeyPair::new().get_public_key().compress();
        let header = BlockHeader::new(
            BlockVersion::Nobunaga,
            0,
            timestamp_ms,
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
            registered_accounts: HashMap::new(),
            account_energies: HashMap::new(),
            global_energy_state: GlobalEnergyState {
                total_energy_limit: TOTAL_ENERGY_LIMIT,
                total_energy_weight: 0,
                last_update: 0,
            },
            delegated_resources: HashMap::new(),
            environment: Environment::new(),
            block,
            block_hash,
            burned: 0,
            gas_fee: 0,
            executor: Arc::new(TakoContractExecutor::new()),
            _contract_provider: DummyContractProvider,
        }
    }

    fn set_balance(&mut self, account: &UncompressedPublicKey, amount: u64) {
        self.sender_balances.insert(account.compress(), amount);
    }

    #[allow(dead_code)]
    fn get_balance(&self, account: &UncompressedPublicKey) -> u64 {
        let compressed = account.compress();
        let sender = self.sender_balances.get(&compressed).copied().unwrap_or(0);
        let receiver = self
            .receiver_balances
            .get(&compressed)
            .copied()
            .unwrap_or(0);
        sender.saturating_add(receiver)
    }

    fn set_nonce(&mut self, account: &UncompressedPublicKey, nonce: Nonce) {
        self.nonces.insert(account.compress(), nonce);
    }

    fn register_account(&mut self, account: &CompressedPublicKey) {
        self.registered_accounts.insert(account.clone(), true);
    }

    fn is_registered(&self, account: &CompressedPublicKey) -> bool {
        self.registered_accounts
            .get(account)
            .copied()
            .unwrap_or(false)
    }

    fn set_account_energy_state(&mut self, account: &CompressedPublicKey, energy: AccountEnergy) {
        self.account_energies.insert(account.clone(), energy);
        // Update global weight
        self.global_energy_state.total_energy_weight = self
            .account_energies
            .values()
            .map(|e| e.frozen_balance)
            .sum();
    }

    fn get_account_energy_state(&self, account: &CompressedPublicKey) -> Option<&AccountEnergy> {
        self.account_energies.get(account)
    }

    fn get_delegation(
        &self,
        from: &CompressedPublicKey,
        to: &CompressedPublicKey,
    ) -> Option<&DelegatedResource> {
        self.delegated_resources.get(&(from.clone(), to.clone()))
    }

    fn get_burned(&self) -> u64 {
        self.burned
    }
}

// =============================================================================
// BlockchainVerificationState Implementation
// =============================================================================

#[async_trait]
impl<'a> BlockchainVerificationState<'a, TestError> for BatchTestChainState {
    async fn pre_verify_tx<'b>(&'b mut self, _tx: &Transaction) -> Result<(), TestError> {
        Ok(())
    }

    async fn get_receiver_balance<'b>(
        &'b mut self,
        account: Cow<'a, CompressedPublicKey>,
        _asset: Cow<'a, Hash>,
    ) -> Result<&'b mut u64, TestError> {
        let key = account.into_owned();
        // Implicitly register account when getting receiver balance
        self.registered_accounts.insert(key.clone(), true);
        let entry = self.receiver_balances.entry(key).or_insert(0);
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
        self.block.get_timestamp() / 1000
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
        account: &CompressedPublicKey,
    ) -> Result<bool, TestError> {
        Ok(self
            .registered_accounts
            .get(account)
            .copied()
            .unwrap_or(false))
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

    async fn get_delegated_resource(
        &mut self,
        from: &'a CompressedPublicKey,
        to: &'a CompressedPublicKey,
    ) -> Result<Option<DelegatedResource>, TestError> {
        Ok(self
            .delegated_resources
            .get(&(from.clone(), to.clone()))
            .cloned())
    }
}

// =============================================================================
// BlockchainApplyState Implementation
// =============================================================================

#[async_trait]
impl<'a> BlockchainApplyState<'a, DummyContractProvider, TestError> for BatchTestChainState {
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

    async fn get_account_energy(
        &mut self,
        account: &'a CompressedPublicKey,
    ) -> Result<Option<AccountEnergy>, TestError> {
        Ok(self.account_energies.get(account).cloned())
    }

    async fn set_account_energy(
        &mut self,
        account: &'a CompressedPublicKey,
        energy: AccountEnergy,
    ) -> Result<(), TestError> {
        self.account_energies.insert(account.clone(), energy);
        Ok(())
    }

    async fn get_global_energy_state(&mut self) -> Result<GlobalEnergyState, TestError> {
        Ok(self.global_energy_state.clone())
    }

    async fn set_global_energy_state(&mut self, state: GlobalEnergyState) -> Result<(), TestError> {
        self.global_energy_state = state;
        Ok(())
    }

    // Note: get_delegated_resource is inherited from BlockchainVerificationState

    async fn set_delegated_resource(
        &mut self,
        delegation: &DelegatedResource,
    ) -> Result<(), TestError> {
        self.delegated_resources.insert(
            (delegation.from.clone(), delegation.to.clone()),
            delegation.clone(),
        );
        Ok(())
    }

    async fn delete_delegated_resource(
        &mut self,
        from: &'a CompressedPublicKey,
        to: &'a CompressedPublicKey,
    ) -> Result<(), TestError> {
        self.delegated_resources.remove(&(from.clone(), to.clone()));
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
        _user: &'a CompressedPublicKey,
        _referrer: &'a CompressedPublicKey,
        _tx_hash: &'a Hash,
    ) -> Result<(), TestError> {
        Err(TestError::Unsupported)
    }

    async fn distribute_referral_rewards(
        &mut self,
        _from_user: &'a CompressedPublicKey,
        _asset: &'a Hash,
        _total_amount: u64,
        _ratios: &[u16],
    ) -> Result<DistributionResult, TestError> {
        Err(TestError::Unsupported)
    }

    async fn set_kyc(
        &mut self,
        _user: &'a CompressedPublicKey,
        _level: u16,
        _verified_at: u64,
        _data_hash: &'a Hash,
        _committee_id: &'a Hash,
        _tx_hash: &'a Hash,
    ) -> Result<(), TestError> {
        Ok(())
    }

    async fn revoke_kyc(
        &mut self,
        _user: &'a CompressedPublicKey,
        _reason_hash: &'a Hash,
        _tx_hash: &'a Hash,
    ) -> Result<(), TestError> {
        Ok(())
    }

    async fn renew_kyc(
        &mut self,
        _user: &'a CompressedPublicKey,
        _verified_at: u64,
        _data_hash: &'a Hash,
        _tx_hash: &'a Hash,
    ) -> Result<(), TestError> {
        Ok(())
    }

    async fn transfer_kyc(
        &mut self,
        _user: &'a CompressedPublicKey,
        _source_committee_id: &'a Hash,
        _dest_committee_id: &'a Hash,
        _new_data_hash: &'a Hash,
        _transferred_at: u64,
        _tx_hash: &'a Hash,
        _dest_max_kyc_level: u16,
        _verification_timestamp: u64,
    ) -> Result<(), TestError> {
        Ok(())
    }

    async fn emergency_suspend_kyc(
        &mut self,
        _user: &'a CompressedPublicKey,
        _reason_hash: &'a Hash,
        _expires_at: u64,
        _tx_hash: &'a Hash,
    ) -> Result<(), TestError> {
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

    async fn bootstrap_global_committee(
        &mut self,
        _name: String,
        _members: Vec<CommitteeMemberInfo>,
        _threshold: u8,
        _kyc_threshold: u8,
        _max_kyc_level: u16,
        _tx_hash: &'a Hash,
    ) -> Result<Hash, TestError> {
        Ok(Hash::zero())
    }

    async fn register_committee(
        &mut self,
        _name: String,
        _region: KycRegion,
        _members: Vec<CommitteeMemberInfo>,
        _threshold: u8,
        _kyc_threshold: u8,
        _max_kyc_level: u16,
        _parent_id: &'a Hash,
        _tx_hash: &'a Hash,
    ) -> Result<Hash, TestError> {
        Ok(Hash::zero())
    }

    async fn update_committee(
        &mut self,
        _committee_id: &'a Hash,
        _update: &CommitteeUpdateData,
    ) -> Result<(), TestError> {
        Ok(())
    }

    async fn get_committee(
        &self,
        _committee_id: &'a Hash,
    ) -> Result<Option<SecurityCommittee>, TestError> {
        Ok(None)
    }

    async fn get_verifying_committee(
        &self,
        _user: &'a CompressedPublicKey,
    ) -> Result<Option<Hash>, TestError> {
        Ok(None)
    }

    async fn get_kyc_status(
        &self,
        _user: &'a CompressedPublicKey,
    ) -> Result<Option<tos_common::kyc::KycStatus>, TestError> {
        Ok(None)
    }

    async fn get_kyc_level(
        &self,
        _user: &'a CompressedPublicKey,
    ) -> Result<Option<u16>, TestError> {
        Ok(None)
    }

    async fn is_global_committee_bootstrapped(&self) -> Result<bool, TestError> {
        Ok(true)
    }

    async fn set_transaction_result(
        &mut self,
        _tx_hash: &'a Hash,
        _result: &TransactionResult,
    ) -> Result<(), TestError> {
        Ok(())
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

use tos_common::{
    serializer::{Serializer, Writer},
    transaction::{Transaction, TransactionType},
};

fn create_keypair() -> KeyPair {
    KeyPair::new()
}

fn create_test_accounts(count: usize) -> Vec<CompressedPublicKey> {
    (0..count)
        .map(|_| KeyPair::new().get_public_key().compress())
        .collect()
}

/// Create a signed Energy transaction for testing
fn create_energy_transaction(keypair: &KeyPair, payload: EnergyPayload, nonce: u64) -> Transaction {
    let source = keypair.get_public_key().compress();
    let reference = Reference {
        topoheight: 0,
        hash: Hash::zero(),
    };

    // Create transaction data
    let data = TransactionType::Energy(payload);

    // Sign the transaction - serialize fields for signing
    let mut buffer = Vec::new();
    {
        let mut writer = Writer::new(&mut buffer);
        TxVersion::T1.write(&mut writer);
        3u8.write(&mut writer); // chain_id for devnet
        source.write(&mut writer);
        data.write(&mut writer);
        0u64.write(&mut writer); // fee_limit
        nonce.write(&mut writer);
        reference.write(&mut writer);
    }

    let signature = keypair.sign(&buffer);

    Transaction::new(
        TxVersion::T1,
        3, // Devnet chain_id
        source,
        data,
        0, // fee_limit
        nonce,
        reference,
        None, // no multisig
        signature,
    )
}

// =============================================================================
// ActivateAccounts Tests
// =============================================================================

mod activate_accounts_tests {
    use super::*;

    #[tokio::test]
    async fn test_activate_single_account() {
        let sender_keypair = create_keypair();
        let sender = sender_keypair.get_public_key();
        let sender_compressed = sender.compress();

        let accounts_to_activate = create_test_accounts(1);

        let mut chain_state = BatchTestChainState::new();
        chain_state.register_account(&sender_compressed);
        // 10 TOS for activation fee (0.1 TOS per account) + buffer
        chain_state.set_balance(sender, 10 * COIN_VALUE);
        chain_state.set_nonce(sender, 0);

        // Build transaction
        let tx = create_energy_transaction(
            &sender_keypair,
            EnergyPayload::activate_accounts(accounts_to_activate.clone()),
            0,
        );

        let tx = Arc::new(tx);
        let tx_hash = tx.hash();

        // Apply transaction
        let result = tx.apply_without_verify(&tx_hash, &mut chain_state).await;

        assert!(result.is_ok(), "Transaction should succeed: {:?}", result);

        // Verify account was registered
        assert!(
            chain_state.is_registered(&accounts_to_activate[0]),
            "Account should be registered"
        );

        // Verify activation fee was burned
        assert_eq!(
            chain_state.get_burned(),
            FEE_PER_ACCOUNT_CREATION,
            "Activation fee should be burned"
        );
    }

    #[tokio::test]
    async fn test_activate_multiple_accounts() {
        let sender_keypair = create_keypair();
        let sender = sender_keypair.get_public_key();
        let sender_compressed = sender.compress();

        let num_accounts = 10;
        let accounts_to_activate = create_test_accounts(num_accounts);

        let mut chain_state = BatchTestChainState::new();
        chain_state.register_account(&sender_compressed);
        chain_state.set_balance(sender, 100 * COIN_VALUE);
        chain_state.set_nonce(sender, 0);

        let tx = create_energy_transaction(
            &sender_keypair,
            EnergyPayload::activate_accounts(accounts_to_activate.clone()),
            0,
        );

        let tx = Arc::new(tx);
        let tx_hash = tx.hash();

        let result = tx.apply_without_verify(&tx_hash, &mut chain_state).await;

        assert!(result.is_ok(), "Transaction should succeed: {:?}", result);

        // Verify all accounts were registered
        for account in &accounts_to_activate {
            assert!(
                chain_state.is_registered(account),
                "All accounts should be registered"
            );
        }

        // Verify total activation fee was burned
        let expected_burned = FEE_PER_ACCOUNT_CREATION * num_accounts as u64;
        assert_eq!(
            chain_state.get_burned(),
            expected_burned,
            "Total activation fee should be burned"
        );
    }

    #[tokio::test]
    async fn test_activate_max_accounts() {
        let sender_keypair = create_keypair();
        let sender = sender_keypair.get_public_key();
        let sender_compressed = sender.compress();

        // Use MAX_BATCH_ACTIVATE accounts
        let accounts_to_activate = create_test_accounts(MAX_BATCH_ACTIVATE);

        let mut chain_state = BatchTestChainState::new();
        chain_state.register_account(&sender_compressed);
        // Need enough TOS for 500 accounts * 0.1 TOS = 50 TOS
        chain_state.set_balance(sender, 100 * COIN_VALUE);
        chain_state.set_nonce(sender, 0);

        let tx = create_energy_transaction(
            &sender_keypair,
            EnergyPayload::activate_accounts(accounts_to_activate.clone()),
            0,
        );

        let tx = Arc::new(tx);
        let tx_hash = tx.hash();

        let result = tx.apply_without_verify(&tx_hash, &mut chain_state).await;

        assert!(
            result.is_ok(),
            "Transaction should succeed with max accounts"
        );

        // Verify all accounts were registered
        for account in &accounts_to_activate {
            assert!(chain_state.is_registered(account));
        }

        let expected_burned = FEE_PER_ACCOUNT_CREATION * MAX_BATCH_ACTIVATE as u64;
        assert_eq!(chain_state.get_burned(), expected_burned);
    }
}

// =============================================================================
// BatchDelegateResource Tests
// =============================================================================

mod batch_delegate_resource_tests {
    use super::*;

    #[tokio::test]
    async fn test_batch_delegate_single_receiver() {
        let sender_keypair = create_keypair();
        let sender = sender_keypair.get_public_key();
        let sender_compressed = sender.compress();

        let receiver = KeyPair::new().get_public_key().compress();

        let mut chain_state = BatchTestChainState::new();
        chain_state.register_account(&sender_compressed);
        chain_state.register_account(&receiver);
        chain_state.set_balance(sender, 100 * COIN_VALUE);
        chain_state.set_nonce(sender, 0);

        // Sender needs frozen balance for delegation
        let sender_energy = AccountEnergy {
            frozen_balance: 50 * COIN_VALUE,
            delegated_frozen_balance: 0,
            acquired_delegated_balance: 0,
            ..Default::default()
        };
        chain_state.set_account_energy_state(&sender_compressed, sender_energy);

        let delegations = vec![BatchDelegationItem {
            receiver: receiver.clone(),
            amount: 10 * COIN_VALUE,
            lock: false,
            lock_period: 0,
        }];

        let tx = create_energy_transaction(
            &sender_keypair,
            EnergyPayload::batch_delegate_resource(delegations),
            0,
        );

        let tx = Arc::new(tx);
        let tx_hash = tx.hash();

        let result = tx.apply_without_verify(&tx_hash, &mut chain_state).await;

        assert!(result.is_ok(), "Transaction should succeed: {:?}", result);

        // Verify delegation was created
        let delegation = chain_state.get_delegation(&sender_compressed, &receiver);
        assert!(delegation.is_some(), "Delegation should exist");
        assert_eq!(delegation.unwrap().frozen_balance, 10 * COIN_VALUE);

        // Verify sender's delegated balance was updated
        let sender_energy = chain_state
            .get_account_energy_state(&sender_compressed)
            .unwrap();
        assert_eq!(sender_energy.delegated_frozen_balance, 10 * COIN_VALUE);

        // Verify receiver's acquired balance was updated
        let receiver_energy = chain_state.get_account_energy_state(&receiver).unwrap();
        assert_eq!(receiver_energy.acquired_delegated_balance, 10 * COIN_VALUE);
    }

    #[tokio::test]
    async fn test_batch_delegate_multiple_receivers() {
        let sender_keypair = create_keypair();
        let sender = sender_keypair.get_public_key();
        let sender_compressed = sender.compress();

        let receivers = create_test_accounts(5);

        let mut chain_state = BatchTestChainState::new();
        chain_state.register_account(&sender_compressed);
        for receiver in &receivers {
            chain_state.register_account(receiver);
        }
        chain_state.set_balance(sender, 100 * COIN_VALUE);
        chain_state.set_nonce(sender, 0);

        // Sender needs frozen balance for delegation
        let sender_energy = AccountEnergy {
            frozen_balance: 100 * COIN_VALUE,
            delegated_frozen_balance: 0,
            acquired_delegated_balance: 0,
            ..Default::default()
        };
        chain_state.set_account_energy_state(&sender_compressed, sender_energy);

        let delegations: Vec<BatchDelegationItem> = receivers
            .iter()
            .enumerate()
            .map(|(i, receiver)| BatchDelegationItem {
                receiver: receiver.clone(),
                amount: (i as u64 + 1) * COIN_VALUE, // 1, 2, 3, 4, 5 TOS
                lock: false,
                lock_period: 0,
            })
            .collect();

        let total_delegation: u64 = delegations.iter().map(|d| d.amount).sum();

        let tx = create_energy_transaction(
            &sender_keypair,
            EnergyPayload::batch_delegate_resource(delegations),
            0,
        );

        let tx = Arc::new(tx);
        let tx_hash = tx.hash();

        let result = tx.apply_without_verify(&tx_hash, &mut chain_state).await;

        assert!(result.is_ok(), "Transaction should succeed: {:?}", result);

        // Verify all delegations were created
        for (i, receiver) in receivers.iter().enumerate() {
            let delegation = chain_state.get_delegation(&sender_compressed, receiver);
            assert!(
                delegation.is_some(),
                "Delegation to receiver {} should exist",
                i
            );
            assert_eq!(
                delegation.unwrap().frozen_balance,
                (i as u64 + 1) * COIN_VALUE
            );
        }

        // Verify sender's total delegated balance
        let sender_energy = chain_state
            .get_account_energy_state(&sender_compressed)
            .unwrap();
        assert_eq!(sender_energy.delegated_frozen_balance, total_delegation);
    }

    #[tokio::test]
    async fn test_batch_delegate_with_lock() {
        let sender_keypair = create_keypair();
        let sender = sender_keypair.get_public_key();
        let sender_compressed = sender.compress();

        let receiver = KeyPair::new().get_public_key().compress();

        let mut chain_state = BatchTestChainState::new_with_timestamp(1000000);
        chain_state.register_account(&sender_compressed);
        chain_state.register_account(&receiver);
        chain_state.set_balance(sender, 100 * COIN_VALUE);
        chain_state.set_nonce(sender, 0);

        let sender_energy = AccountEnergy {
            frozen_balance: 50 * COIN_VALUE,
            ..Default::default()
        };
        chain_state.set_account_energy_state(&sender_compressed, sender_energy);

        let delegations = vec![BatchDelegationItem {
            receiver: receiver.clone(),
            amount: 10 * COIN_VALUE,
            lock: true,
            lock_period: 30, // 30 days lock
        }];

        let tx = create_energy_transaction(
            &sender_keypair,
            EnergyPayload::batch_delegate_resource(delegations),
            0,
        );

        let tx = Arc::new(tx);
        let tx_hash = tx.hash();

        let result = tx.apply_without_verify(&tx_hash, &mut chain_state).await;

        assert!(result.is_ok(), "Transaction should succeed: {:?}", result);

        // Verify delegation has lock expiry
        let delegation = chain_state.get_delegation(&sender_compressed, &receiver);
        assert!(delegation.is_some());
        let delegation = delegation.unwrap();
        assert!(delegation.expire_time > 0, "Lock should have expiry time");
        // expire_time = timestamp_ms + lock_period * 86_400_000
        let expected_expiry = 1000000 + 30 * 86_400_000;
        assert_eq!(delegation.expire_time, expected_expiry);
    }
}

// =============================================================================
// ActivateAndDelegate Tests
// =============================================================================

mod activate_and_delegate_tests {
    use super::*;

    #[tokio::test]
    async fn test_activate_and_delegate_single() {
        let sender_keypair = create_keypair();
        let sender = sender_keypair.get_public_key();
        let sender_compressed = sender.compress();

        let new_account = KeyPair::new().get_public_key().compress();

        let mut chain_state = BatchTestChainState::new();
        chain_state.register_account(&sender_compressed);
        chain_state.set_balance(sender, 100 * COIN_VALUE);
        chain_state.set_nonce(sender, 0);

        // Sender needs frozen balance for delegation
        let sender_energy = AccountEnergy {
            frozen_balance: 50 * COIN_VALUE,
            ..Default::default()
        };
        chain_state.set_account_energy_state(&sender_compressed, sender_energy);

        let items = vec![ActivateDelegateItem {
            account: new_account.clone(),
            delegate_amount: 10 * COIN_VALUE,
            lock: false,
            lock_period: 0,
        }];

        let tx = create_energy_transaction(
            &sender_keypair,
            EnergyPayload::activate_and_delegate(items),
            0,
        );

        let tx = Arc::new(tx);
        let tx_hash = tx.hash();

        let result = tx.apply_without_verify(&tx_hash, &mut chain_state).await;

        assert!(result.is_ok(), "Transaction should succeed: {:?}", result);

        // Verify account was registered
        assert!(
            chain_state.is_registered(&new_account),
            "Account should be registered"
        );

        // Verify activation fee was burned
        assert_eq!(
            chain_state.get_burned(),
            FEE_PER_ACCOUNT_CREATION,
            "Activation fee should be burned"
        );

        // Verify delegation was created
        let delegation = chain_state.get_delegation(&sender_compressed, &new_account);
        assert!(delegation.is_some(), "Delegation should exist");
        assert_eq!(delegation.unwrap().frozen_balance, 10 * COIN_VALUE);

        // Verify new account has acquired delegated balance
        let new_account_energy = chain_state.get_account_energy_state(&new_account).unwrap();
        assert_eq!(
            new_account_energy.acquired_delegated_balance,
            10 * COIN_VALUE
        );
    }

    #[tokio::test]
    async fn test_activate_and_delegate_multiple() {
        let sender_keypair = create_keypair();
        let sender = sender_keypair.get_public_key();
        let sender_compressed = sender.compress();

        let new_accounts = create_test_accounts(5);

        let mut chain_state = BatchTestChainState::new();
        chain_state.register_account(&sender_compressed);
        chain_state.set_balance(sender, 200 * COIN_VALUE);
        chain_state.set_nonce(sender, 0);

        // Sender needs frozen balance for delegation
        let sender_energy = AccountEnergy {
            frozen_balance: 100 * COIN_VALUE,
            ..Default::default()
        };
        chain_state.set_account_energy_state(&sender_compressed, sender_energy);

        let items: Vec<ActivateDelegateItem> = new_accounts
            .iter()
            .enumerate()
            .map(|(i, account)| ActivateDelegateItem {
                account: account.clone(),
                delegate_amount: (i as u64 + 1) * COIN_VALUE, // 1, 2, 3, 4, 5 TOS
                lock: false,
                lock_period: 0,
            })
            .collect();

        let total_delegation: u64 = items.iter().map(|i| i.delegate_amount).sum();

        let tx = create_energy_transaction(
            &sender_keypair,
            EnergyPayload::activate_and_delegate(items),
            0,
        );

        let tx = Arc::new(tx);
        let tx_hash = tx.hash();

        let result = tx.apply_without_verify(&tx_hash, &mut chain_state).await;

        assert!(result.is_ok(), "Transaction should succeed: {:?}", result);

        // Verify all accounts were registered
        for account in &new_accounts {
            assert!(
                chain_state.is_registered(account),
                "Account should be registered"
            );
        }

        // Verify total activation fee was burned
        let expected_burned = FEE_PER_ACCOUNT_CREATION * new_accounts.len() as u64;
        assert_eq!(chain_state.get_burned(), expected_burned);

        // Verify all delegations were created
        for (i, account) in new_accounts.iter().enumerate() {
            let delegation = chain_state.get_delegation(&sender_compressed, account);
            assert!(delegation.is_some());
            assert_eq!(
                delegation.unwrap().frozen_balance,
                (i as u64 + 1) * COIN_VALUE
            );
        }

        // Verify sender's total delegated balance
        let sender_energy = chain_state
            .get_account_energy_state(&sender_compressed)
            .unwrap();
        assert_eq!(sender_energy.delegated_frozen_balance, total_delegation);
    }

    #[tokio::test]
    async fn test_activate_and_delegate_with_zero_delegation() {
        let sender_keypair = create_keypair();
        let sender = sender_keypair.get_public_key();
        let sender_compressed = sender.compress();

        let new_account = KeyPair::new().get_public_key().compress();

        let mut chain_state = BatchTestChainState::new();
        chain_state.register_account(&sender_compressed);
        chain_state.set_balance(sender, 100 * COIN_VALUE);
        chain_state.set_nonce(sender, 0);

        // Activation only, no delegation
        let items = vec![ActivateDelegateItem {
            account: new_account.clone(),
            delegate_amount: 0,
            lock: false,
            lock_period: 0,
        }];

        let tx = create_energy_transaction(
            &sender_keypair,
            EnergyPayload::activate_and_delegate(items),
            0,
        );

        let tx = Arc::new(tx);
        let tx_hash = tx.hash();

        let result = tx.apply_without_verify(&tx_hash, &mut chain_state).await;

        assert!(result.is_ok(), "Transaction should succeed: {:?}", result);

        // Verify account was registered
        assert!(chain_state.is_registered(&new_account));

        // Verify activation fee was burned
        assert_eq!(chain_state.get_burned(), FEE_PER_ACCOUNT_CREATION);

        // Verify no delegation was created (amount was 0)
        let delegation = chain_state.get_delegation(&sender_compressed, &new_account);
        assert!(
            delegation.is_none(),
            "No delegation should exist for 0 amount"
        );
    }
}

// =============================================================================
// Validation and Error Case Tests
// =============================================================================

mod validation_tests {
    use super::*;

    #[test]
    fn test_batch_limits_constants() {
        // Verify constants are set correctly
        assert_eq!(MAX_BATCH_ACTIVATE, 500);
        assert_eq!(MAX_BATCH_DELEGATE, 500);
        assert_eq!(MAX_BATCH_ACTIVATE_DELEGATE, 500);
    }

    #[test]
    fn test_activation_fee_constant() {
        // 0.1 TOS = 10,000,000 atomic units
        assert_eq!(FEE_PER_ACCOUNT_CREATION, 10_000_000);
    }

    #[test]
    fn test_validate_batch_limits_activate_accounts() {
        // Valid: 500 accounts
        let accounts = create_test_accounts(MAX_BATCH_ACTIVATE);
        let payload = EnergyPayload::activate_accounts(accounts);
        assert!(payload.validate_batch_limits().is_ok());

        // Invalid: 501 accounts
        let accounts = create_test_accounts(MAX_BATCH_ACTIVATE + 1);
        let payload = EnergyPayload::activate_accounts(accounts);
        assert!(payload.validate_batch_limits().is_err());
    }

    #[test]
    fn test_validate_batch_limits_batch_delegate() {
        let receiver = KeyPair::new().get_public_key().compress();

        // Valid: 500 delegations
        let delegations: Vec<BatchDelegationItem> = (0..MAX_BATCH_DELEGATE)
            .map(|_| BatchDelegationItem {
                receiver: receiver.clone(),
                amount: COIN_VALUE,
                lock: false,
                lock_period: 0,
            })
            .collect();
        let payload = EnergyPayload::batch_delegate_resource(delegations);
        assert!(payload.validate_batch_limits().is_ok());

        // Invalid: 501 delegations
        let delegations: Vec<BatchDelegationItem> = (0..MAX_BATCH_DELEGATE + 1)
            .map(|_| BatchDelegationItem {
                receiver: receiver.clone(),
                amount: COIN_VALUE,
                lock: false,
                lock_period: 0,
            })
            .collect();
        let payload = EnergyPayload::batch_delegate_resource(delegations);
        assert!(payload.validate_batch_limits().is_err());
    }

    #[test]
    fn test_validate_lock_period() {
        let receiver = KeyPair::new().get_public_key().compress();

        // Valid: lock period within limits
        let delegations = vec![BatchDelegationItem {
            receiver: receiver.clone(),
            amount: COIN_VALUE,
            lock: true,
            lock_period: MAX_DELEGATE_LOCK_DAYS,
        }];
        let payload = EnergyPayload::batch_delegate_resource(delegations);
        assert!(payload.validate_batch_limits().is_ok());

        // Note: Lock period validation (> MAX_DELEGATE_LOCK_DAYS) is checked during
        // transaction execution, not in validate_batch_limits which only checks batch size.
        // Invalid lock periods are rejected when the transaction is applied.
        let delegations = vec![BatchDelegationItem {
            receiver: receiver.clone(),
            amount: COIN_VALUE,
            lock: true,
            lock_period: MAX_DELEGATE_LOCK_DAYS + 1,
        }];
        let payload = EnergyPayload::batch_delegate_resource(delegations);
        // validate_batch_limits only checks batch size, not lock period
        assert!(payload.validate_batch_limits().is_ok());
    }

    #[test]
    fn test_validate_minimum_delegation_amount() {
        let receiver = KeyPair::new().get_public_key().compress();

        // Valid: minimum delegation amount
        let delegations = vec![BatchDelegationItem {
            receiver: receiver.clone(),
            amount: MIN_DELEGATION_AMOUNT,
            lock: false,
            lock_period: 0,
        }];
        let payload = EnergyPayload::batch_delegate_resource(delegations);
        assert!(payload.validate_batch_limits().is_ok());

        // Note: Minimum delegation amount validation (< MIN_DELEGATION_AMOUNT) is checked
        // during transaction execution, not in validate_batch_limits which only checks batch size.
        // Amounts below minimum are rejected when the transaction is applied.
        let delegations = vec![BatchDelegationItem {
            receiver: receiver.clone(),
            amount: MIN_DELEGATION_AMOUNT - 1,
            lock: false,
            lock_period: 0,
        }];
        let payload = EnergyPayload::batch_delegate_resource(delegations);
        // validate_batch_limits only checks batch size, not minimum amount
        assert!(payload.validate_batch_limits().is_ok());
    }

    #[test]
    fn test_is_batch_operation() {
        let accounts = create_test_accounts(1);

        assert!(EnergyPayload::activate_accounts(accounts).is_batch_operation());
        assert!(EnergyPayload::batch_delegate_resource(vec![]).is_batch_operation());
        assert!(EnergyPayload::activate_and_delegate(vec![]).is_batch_operation());

        // Non-batch operations
        assert!(!EnergyPayload::FreezeTos { amount: 1 }.is_batch_operation());
        assert!(!EnergyPayload::UnfreezeTos { amount: 1 }.is_batch_operation());
    }

    #[test]
    fn test_batch_size() {
        let accounts = create_test_accounts(10);
        let payload = EnergyPayload::activate_accounts(accounts);
        assert_eq!(payload.batch_size(), Some(10));

        let receiver = KeyPair::new().get_public_key().compress();
        let delegations: Vec<BatchDelegationItem> = (0..5)
            .map(|_| BatchDelegationItem {
                receiver: receiver.clone(),
                amount: COIN_VALUE,
                lock: false,
                lock_period: 0,
            })
            .collect();
        let payload = EnergyPayload::batch_delegate_resource(delegations);
        assert_eq!(payload.batch_size(), Some(5));

        // Non-batch operations return None
        assert_eq!(EnergyPayload::FreezeTos { amount: 1 }.batch_size(), None);
    }
}

// =============================================================================
// Mixed Account Activation Tests (Activated + Non-Activated)
// =============================================================================

mod mixed_activation_tests {
    use super::*;

    /// ActivateAccounts should skip already-activated accounts
    /// instead of returning an error. Only charge fees for newly activated accounts.
    #[tokio::test]
    async fn test_activate_accounts_skips_already_activated() {
        let sender_keypair = create_keypair();
        let sender = sender_keypair.get_public_key();
        let sender_compressed = sender.compress();

        // Create 5 accounts: 2 already activated, 3 new
        let all_accounts = create_test_accounts(5);
        let already_activated = &all_accounts[0..2];
        let new_accounts = &all_accounts[2..5];

        let mut chain_state = BatchTestChainState::new();
        chain_state.register_account(&sender_compressed);
        chain_state.set_balance(sender, 100 * COIN_VALUE);
        chain_state.set_nonce(sender, 0);

        // Pre-register 2 accounts
        for account in already_activated {
            chain_state.register_account(account);
        }

        // Verify pre-conditions
        assert!(chain_state.is_registered(&already_activated[0]));
        assert!(chain_state.is_registered(&already_activated[1]));
        assert!(!chain_state.is_registered(&new_accounts[0]));
        assert!(!chain_state.is_registered(&new_accounts[1]));
        assert!(!chain_state.is_registered(&new_accounts[2]));

        let tx = create_energy_transaction(
            &sender_keypair,
            EnergyPayload::activate_accounts(all_accounts.clone()),
            0,
        );

        let tx = Arc::new(tx);
        let tx_hash = tx.hash();

        // Should succeed even though 2 accounts are already activated
        let result = tx.apply_without_verify(&tx_hash, &mut chain_state).await;

        assert!(
            result.is_ok(),
            "Transaction should succeed with mixed accounts: {:?}",
            result
        );

        // All accounts should now be registered
        for account in &all_accounts {
            assert!(
                chain_state.is_registered(account),
                "Account should be registered"
            );
        }

        // Fee should only be burned for 3 NEW accounts (not 5)
        let expected_burned = FEE_PER_ACCOUNT_CREATION * 3;
        assert_eq!(
            chain_state.get_burned(),
            expected_burned,
            "Fee should only be charged for newly activated accounts"
        );
    }

    /// ActivateAccounts with all accounts already activated
    /// should succeed with zero fee burned.
    #[tokio::test]
    async fn test_activate_accounts_all_already_activated() {
        let sender_keypair = create_keypair();
        let sender = sender_keypair.get_public_key();
        let sender_compressed = sender.compress();

        let accounts = create_test_accounts(3);

        let mut chain_state = BatchTestChainState::new();
        chain_state.register_account(&sender_compressed);
        chain_state.set_balance(sender, 100 * COIN_VALUE);
        chain_state.set_nonce(sender, 0);

        // Pre-register ALL accounts
        for account in &accounts {
            chain_state.register_account(account);
        }

        let tx = create_energy_transaction(
            &sender_keypair,
            EnergyPayload::activate_accounts(accounts.clone()),
            0,
        );

        let tx = Arc::new(tx);
        let tx_hash = tx.hash();

        // Should succeed (no-op for already activated accounts)
        let result = tx.apply_without_verify(&tx_hash, &mut chain_state).await;

        assert!(
            result.is_ok(),
            "Transaction should succeed even if all accounts are already activated: {:?}",
            result
        );

        // Zero fee should be burned (no new activations)
        assert_eq!(
            chain_state.get_burned(),
            0,
            "No fee should be burned when all accounts are already activated"
        );
    }

    /// ActivateAndDelegate should work with mixed new/existing accounts
    /// - New accounts: activate + delegate (charge activation fee)
    /// - Existing accounts: delegate only (no activation fee)
    #[tokio::test]
    async fn test_activate_and_delegate_mixed_accounts() {
        let sender_keypair = create_keypair();
        let sender = sender_keypair.get_public_key();
        let sender_compressed = sender.compress();

        // Create 4 accounts: 2 existing, 2 new
        let all_accounts = create_test_accounts(4);
        let existing_accounts = &all_accounts[0..2];
        let _new_accounts = &all_accounts[2..4];

        let mut chain_state = BatchTestChainState::new();
        chain_state.register_account(&sender_compressed);
        chain_state.set_balance(sender, 200 * COIN_VALUE);
        chain_state.set_nonce(sender, 0);

        // Sender needs frozen balance for delegation
        let sender_energy = AccountEnergy {
            frozen_balance: 100 * COIN_VALUE,
            ..Default::default()
        };
        chain_state.set_account_energy_state(&sender_compressed, sender_energy);

        // Pre-register 2 accounts (existing)
        for account in existing_accounts {
            chain_state.register_account(account);
        }

        // Create items for all 4 accounts
        let items: Vec<ActivateDelegateItem> = all_accounts
            .iter()
            .enumerate()
            .map(|(i, account)| ActivateDelegateItem {
                account: account.clone(),
                delegate_amount: (i as u64 + 1) * COIN_VALUE, // 1, 2, 3, 4 TOS
                lock: false,
                lock_period: 0,
            })
            .collect();

        let total_delegation: u64 = items.iter().map(|i| i.delegate_amount).sum();

        let tx = create_energy_transaction(
            &sender_keypair,
            EnergyPayload::activate_and_delegate(items),
            0,
        );

        let tx = Arc::new(tx);
        let tx_hash = tx.hash();

        // Should succeed
        let result = tx.apply_without_verify(&tx_hash, &mut chain_state).await;

        assert!(
            result.is_ok(),
            "Transaction should succeed with mixed accounts: {:?}",
            result
        );

        // All accounts should be registered
        for account in &all_accounts {
            assert!(chain_state.is_registered(account));
        }

        // Fee should only be burned for 2 NEW accounts (not 4)
        let expected_burned = FEE_PER_ACCOUNT_CREATION * 2;
        assert_eq!(
            chain_state.get_burned(),
            expected_burned,
            "Fee should only be charged for newly activated accounts"
        );

        // All delegations should be created (both existing and new accounts)
        for (i, account) in all_accounts.iter().enumerate() {
            let delegation = chain_state.get_delegation(&sender_compressed, account);
            assert!(
                delegation.is_some(),
                "Delegation should exist for account {}",
                i
            );
            assert_eq!(
                delegation.unwrap().frozen_balance,
                (i as u64 + 1) * COIN_VALUE
            );
        }

        // Verify sender's total delegated balance
        let sender_energy = chain_state
            .get_account_energy_state(&sender_compressed)
            .unwrap();
        assert_eq!(sender_energy.delegated_frozen_balance, total_delegation);
    }

    /// ActivateAndDelegate with all accounts already existing
    /// should succeed with zero activation fee (delegation only).
    #[tokio::test]
    async fn test_activate_and_delegate_all_existing_accounts() {
        let sender_keypair = create_keypair();
        let sender = sender_keypair.get_public_key();
        let sender_compressed = sender.compress();

        let accounts = create_test_accounts(3);

        let mut chain_state = BatchTestChainState::new();
        chain_state.register_account(&sender_compressed);
        chain_state.set_balance(sender, 100 * COIN_VALUE);
        chain_state.set_nonce(sender, 0);

        // Sender needs frozen balance for delegation
        let sender_energy = AccountEnergy {
            frozen_balance: 50 * COIN_VALUE,
            ..Default::default()
        };
        chain_state.set_account_energy_state(&sender_compressed, sender_energy);

        // Pre-register ALL accounts
        for account in &accounts {
            chain_state.register_account(account);
        }

        let items: Vec<ActivateDelegateItem> = accounts
            .iter()
            .map(|account| ActivateDelegateItem {
                account: account.clone(),
                delegate_amount: 5 * COIN_VALUE,
                lock: false,
                lock_period: 0,
            })
            .collect();

        let tx = create_energy_transaction(
            &sender_keypair,
            EnergyPayload::activate_and_delegate(items),
            0,
        );

        let tx = Arc::new(tx);
        let tx_hash = tx.hash();

        let result = tx.apply_without_verify(&tx_hash, &mut chain_state).await;

        assert!(result.is_ok(), "Transaction should succeed: {:?}", result);

        // Zero activation fee (all accounts were already activated)
        assert_eq!(
            chain_state.get_burned(),
            0,
            "No activation fee should be burned for existing accounts"
        );

        // All delegations should exist
        for account in &accounts {
            let delegation = chain_state.get_delegation(&sender_compressed, account);
            assert!(delegation.is_some());
            assert_eq!(delegation.unwrap().frozen_balance, 5 * COIN_VALUE);
        }
    }

    /// ActivateAndDelegate with zero delegation to existing account
    /// should skip delegation but not charge activation fee.
    #[tokio::test]
    async fn test_activate_and_delegate_zero_delegation_existing_account() {
        let sender_keypair = create_keypair();
        let sender = sender_keypair.get_public_key();
        let sender_compressed = sender.compress();

        let existing_account = KeyPair::new().get_public_key().compress();

        let mut chain_state = BatchTestChainState::new();
        chain_state.register_account(&sender_compressed);
        chain_state.register_account(&existing_account);
        chain_state.set_balance(sender, 100 * COIN_VALUE);
        chain_state.set_nonce(sender, 0);

        let items = vec![ActivateDelegateItem {
            account: existing_account.clone(),
            delegate_amount: 0, // No delegation
            lock: false,
            lock_period: 0,
        }];

        let tx = create_energy_transaction(
            &sender_keypair,
            EnergyPayload::activate_and_delegate(items),
            0,
        );

        let tx = Arc::new(tx);
        let tx_hash = tx.hash();

        let result = tx.apply_without_verify(&tx_hash, &mut chain_state).await;

        assert!(result.is_ok(), "Transaction should succeed: {:?}", result);

        // No fee burned (existing account, zero delegation)
        assert_eq!(chain_state.get_burned(), 0);

        // No delegation created
        let delegation = chain_state.get_delegation(&sender_compressed, &existing_account);
        assert!(delegation.is_none());
    }
}

// =============================================================================
// Receiver Registration Check Tests (Verify Phase Validation)
// =============================================================================

mod receiver_registration_tests {
    use super::*;

    /// BatchDelegateResource should reject unregistered receivers.
    /// The transaction should fail during verification/apply.
    #[tokio::test]
    async fn test_batch_delegate_rejects_unregistered_receiver() {
        let sender_keypair = create_keypair();
        let sender = sender_keypair.get_public_key();
        let sender_compressed = sender.compress();

        // Create unregistered receiver
        let unregistered_receiver = KeyPair::new().get_public_key().compress();

        let mut chain_state = BatchTestChainState::new();
        chain_state.register_account(&sender_compressed);
        chain_state.set_balance(sender, 100 * COIN_VALUE);
        chain_state.set_nonce(sender, 0);

        // Sender needs frozen balance for delegation
        let sender_energy = AccountEnergy {
            frozen_balance: 50 * COIN_VALUE,
            ..Default::default()
        };
        chain_state.set_account_energy_state(&sender_compressed, sender_energy);

        // NOTE: We intentionally do NOT register the receiver
        assert!(!chain_state.is_registered(&unregistered_receiver));

        let delegations = vec![BatchDelegationItem {
            receiver: unregistered_receiver.clone(),
            amount: 10 * COIN_VALUE,
            lock: false,
            lock_period: 0,
        }];

        let tx = create_energy_transaction(
            &sender_keypair,
            EnergyPayload::batch_delegate_resource(delegations),
            0,
        );

        let tx = Arc::new(tx);
        let tx_hash = tx.hash();

        // Transaction should fail due to unregistered receiver
        let result = tx.apply_without_verify(&tx_hash, &mut chain_state).await;

        assert!(
            result.is_err(),
            "Transaction should fail for unregistered receiver"
        );

        let error_message = format!("{:?}", result.unwrap_err());
        assert!(
            error_message.contains("not registered"),
            "Error should mention receiver not registered: {}",
            error_message
        );
    }

    /// BatchDelegateResource with mixed registered/unregistered receivers
    /// should fail if ANY receiver is unregistered.
    #[tokio::test]
    async fn test_batch_delegate_rejects_mixed_receivers() {
        let sender_keypair = create_keypair();
        let sender = sender_keypair.get_public_key();
        let sender_compressed = sender.compress();

        let registered_receiver = KeyPair::new().get_public_key().compress();
        let unregistered_receiver = KeyPair::new().get_public_key().compress();

        let mut chain_state = BatchTestChainState::new();
        chain_state.register_account(&sender_compressed);
        chain_state.register_account(&registered_receiver); // Only register one
        chain_state.set_balance(sender, 100 * COIN_VALUE);
        chain_state.set_nonce(sender, 0);

        let sender_energy = AccountEnergy {
            frozen_balance: 50 * COIN_VALUE,
            ..Default::default()
        };
        chain_state.set_account_energy_state(&sender_compressed, sender_energy);

        assert!(chain_state.is_registered(&registered_receiver));
        assert!(!chain_state.is_registered(&unregistered_receiver));

        let delegations = vec![
            BatchDelegationItem {
                receiver: registered_receiver.clone(),
                amount: 10 * COIN_VALUE,
                lock: false,
                lock_period: 0,
            },
            BatchDelegationItem {
                receiver: unregistered_receiver.clone(),
                amount: 10 * COIN_VALUE,
                lock: false,
                lock_period: 0,
            },
        ];

        let tx = create_energy_transaction(
            &sender_keypair,
            EnergyPayload::batch_delegate_resource(delegations),
            0,
        );

        let tx = Arc::new(tx);
        let tx_hash = tx.hash();

        // Transaction should fail due to unregistered receiver
        let result = tx.apply_without_verify(&tx_hash, &mut chain_state).await;

        assert!(
            result.is_err(),
            "Transaction should fail when any receiver is unregistered"
        );
    }

    /// BatchDelegateResource with all registered receivers should succeed.
    #[tokio::test]
    async fn test_batch_delegate_succeeds_all_registered() {
        let sender_keypair = create_keypair();
        let sender = sender_keypair.get_public_key();
        let sender_compressed = sender.compress();

        let receivers = create_test_accounts(3);

        let mut chain_state = BatchTestChainState::new();
        chain_state.register_account(&sender_compressed);
        // Register ALL receivers
        for receiver in &receivers {
            chain_state.register_account(receiver);
        }
        chain_state.set_balance(sender, 100 * COIN_VALUE);
        chain_state.set_nonce(sender, 0);

        let sender_energy = AccountEnergy {
            frozen_balance: 50 * COIN_VALUE,
            ..Default::default()
        };
        chain_state.set_account_energy_state(&sender_compressed, sender_energy);

        let delegations: Vec<BatchDelegationItem> = receivers
            .iter()
            .map(|receiver| BatchDelegationItem {
                receiver: receiver.clone(),
                amount: 5 * COIN_VALUE,
                lock: false,
                lock_period: 0,
            })
            .collect();

        let tx = create_energy_transaction(
            &sender_keypair,
            EnergyPayload::batch_delegate_resource(delegations),
            0,
        );

        let tx = Arc::new(tx);
        let tx_hash = tx.hash();

        // Transaction should succeed for all registered receivers
        let result = tx.apply_without_verify(&tx_hash, &mut chain_state).await;

        assert!(
            result.is_ok(),
            "Transaction should succeed for all registered receivers: {:?}",
            result
        );

        // Verify all delegations were created
        for receiver in &receivers {
            let delegation = chain_state.get_delegation(&sender_compressed, receiver);
            assert!(delegation.is_some(), "Delegation should exist");
            assert_eq!(delegation.unwrap().frozen_balance, 5 * COIN_VALUE);
        }
    }

    /// Verify that unregistered receiver check is performed during apply phase.
    /// When sender also has zero frozen balance, both errors are caught.
    #[tokio::test]
    async fn test_batch_delegate_multiple_error_conditions() {
        let sender_keypair = create_keypair();
        let sender = sender_keypair.get_public_key();
        let sender_compressed = sender.compress();

        let unregistered_receiver = KeyPair::new().get_public_key().compress();

        let mut chain_state = BatchTestChainState::new();
        chain_state.register_account(&sender_compressed);
        chain_state.set_balance(sender, 100 * COIN_VALUE);
        chain_state.set_nonce(sender, 0);

        // Sender has ZERO frozen balance (would also fail)
        let sender_energy = AccountEnergy {
            frozen_balance: 0, // No frozen balance
            ..Default::default()
        };
        chain_state.set_account_energy_state(&sender_compressed, sender_energy);

        let delegations = vec![BatchDelegationItem {
            receiver: unregistered_receiver.clone(),
            amount: 10 * COIN_VALUE,
            lock: false,
            lock_period: 0,
        }];

        let tx = create_energy_transaction(
            &sender_keypair,
            EnergyPayload::batch_delegate_resource(delegations),
            0,
        );

        let tx = Arc::new(tx);
        let tx_hash = tx.hash();

        // Transaction should fail due to one or more error conditions
        let result = tx.apply_without_verify(&tx_hash, &mut chain_state).await;

        // Both conditions are errors - the transaction must fail
        assert!(
            result.is_err(),
            "Transaction should fail with multiple error conditions"
        );
    }
}

// =============================================================================
// Serialization Tests
// =============================================================================

mod serialization_tests {
    use super::*;
    use tos_common::serializer::{Reader, Serializer, Writer};

    #[test]
    fn test_activate_accounts_serialization() {
        let accounts = create_test_accounts(3);
        let payload = EnergyPayload::activate_accounts(accounts.clone());

        let mut buffer = Vec::new();
        {
            let mut writer = Writer::new(&mut buffer);
            payload.write(&mut writer);
        }

        let mut reader = Reader::new(&buffer);
        let deserialized = EnergyPayload::read(&mut reader).unwrap();

        match deserialized {
            EnergyPayload::ActivateAccounts {
                accounts: deser_accounts,
            } => {
                assert_eq!(deser_accounts.len(), accounts.len());
                for (a, b) in deser_accounts.iter().zip(accounts.iter()) {
                    assert_eq!(a, b);
                }
            }
            _ => panic!("Wrong payload type"),
        }
    }

    #[test]
    fn test_batch_delegate_resource_serialization() {
        let receiver = KeyPair::new().get_public_key().compress();
        let delegations = vec![
            BatchDelegationItem {
                receiver: receiver.clone(),
                amount: 10 * COIN_VALUE,
                lock: true,
                lock_period: 30,
            },
            BatchDelegationItem {
                receiver: receiver.clone(),
                amount: 5 * COIN_VALUE,
                lock: false,
                lock_period: 0,
            },
        ];
        let payload = EnergyPayload::batch_delegate_resource(delegations.clone());

        let mut buffer = Vec::new();
        {
            let mut writer = Writer::new(&mut buffer);
            payload.write(&mut writer);
        }

        let mut reader = Reader::new(&buffer);
        let deserialized = EnergyPayload::read(&mut reader).unwrap();

        match deserialized {
            EnergyPayload::BatchDelegateResource {
                delegations: deser_delegations,
            } => {
                assert_eq!(deser_delegations.len(), delegations.len());
                for (a, b) in deser_delegations.iter().zip(delegations.iter()) {
                    assert_eq!(a.receiver, b.receiver);
                    assert_eq!(a.amount, b.amount);
                    assert_eq!(a.lock, b.lock);
                    assert_eq!(a.lock_period, b.lock_period);
                }
            }
            _ => panic!("Wrong payload type"),
        }
    }

    #[test]
    fn test_activate_and_delegate_serialization() {
        let account = KeyPair::new().get_public_key().compress();
        let items = vec![ActivateDelegateItem {
            account: account.clone(),
            delegate_amount: 10 * COIN_VALUE,
            lock: true,
            lock_period: 60,
        }];
        let payload = EnergyPayload::activate_and_delegate(items.clone());

        let mut buffer = Vec::new();
        {
            let mut writer = Writer::new(&mut buffer);
            payload.write(&mut writer);
        }

        let mut reader = Reader::new(&buffer);
        let deserialized = EnergyPayload::read(&mut reader).unwrap();

        match deserialized {
            EnergyPayload::ActivateAndDelegate { items: deser_items } => {
                assert_eq!(deser_items.len(), items.len());
                for (a, b) in deser_items.iter().zip(items.iter()) {
                    assert_eq!(a.account, b.account);
                    assert_eq!(a.delegate_amount, b.delegate_amount);
                    assert_eq!(a.lock, b.lock);
                    assert_eq!(a.lock_period, b.lock_period);
                }
            }
            _ => panic!("Wrong payload type"),
        }
    }
}
