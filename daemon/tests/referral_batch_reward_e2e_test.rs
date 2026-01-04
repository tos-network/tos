#![allow(clippy::disallowed_methods)]

use std::{borrow::Cow, collections::HashMap, sync::Arc};

use async_trait::async_trait;
use indexmap::{IndexMap, IndexSet};
use tos_common::{
    account::{EnergyResource, Nonce},
    block::{Block, BlockHeader, BlockVersion, EXTRA_NONCE_SIZE},
    config::TOS_ASSET,
    contract::{
        AssetChanges, ChainState as ContractChainState, ContractCache, ContractEvent,
        ContractEventTracker, ContractExecutor, ContractOutput, ContractStorage,
    },
    crypto::{
        elgamal::{Ciphertext, CompressedPublicKey},
        Hash, Hashable, KeyPair, PublicKey,
    },
    immutable::Immutable,
    network::Network,
    referral::{DistributionResult, ReferralRewardRatios, RewardDistribution},
    transaction::{
        builder::{
            AccountState, FeeBuilder, FeeHelper, TransactionBuilder, TransactionTypeBuilder,
        },
        verify::{
            BlockchainApplyState, BlockchainVerificationState, ContractEnvironment, NoZKPCache,
        },
        BatchReferralRewardPayload, ContractDeposit, MultiSigPayload, Reference, Transaction,
        TxVersion,
    },
};
use tos_daemon::tako_integration::TakoContractExecutor;
use tos_environment::Environment;
use tos_kernel::Module;

#[derive(Debug)]
enum TestError {
    Unsupported,
    Overflow,
}

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestError::Unsupported => write!(f, "Unsupported"),
            TestError::Overflow => write!(f, "Overflow"),
        }
    }
}

impl std::error::Error for TestError {}

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

struct TestAccountState {
    balances: HashMap<Hash, u64>,
    nonce: u64,
}

impl TestAccountState {
    fn new() -> Self {
        Self {
            balances: HashMap::new(),
            nonce: 0,
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

    fn account_exists(
        &self,
        _account: &tos_common::crypto::elgamal::CompressedPublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
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

    fn is_account_registered(&self, _key: &PublicKey) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

struct TestChainState {
    sender_balances: HashMap<PublicKey, u64>,
    receiver_balances: HashMap<PublicKey, u64>,
    nonces: HashMap<PublicKey, Nonce>,
    referrers: HashMap<PublicKey, PublicKey>,
    environment: Environment,
    block: Block,
    block_hash: Hash,
    burned: u64,
    gas_fee: u64,
    executor: Arc<dyn ContractExecutor>,
    _contract_provider: DummyContractProvider,
}

impl TestChainState {
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
        }
    }

    fn set_balance(&mut self, account: &PublicKey, amount: u64) {
        self.sender_balances.insert(account.clone(), amount);
    }

    fn get_balance(&self, account: &PublicKey) -> u64 {
        let sender = self.sender_balances.get(account).copied().unwrap_or(0);
        let receiver = self.receiver_balances.get(account).copied().unwrap_or(0);
        sender.saturating_add(receiver)
    }

    fn set_nonce(&mut self, account: &PublicKey, nonce: Nonce) {
        self.nonces.insert(account.clone(), nonce);
    }

    fn bind_referrer(&mut self, user: &PublicKey, referrer: &PublicKey) {
        self.referrers.insert(user.clone(), referrer.clone());
    }

    fn get_uplines(&self, user: &PublicKey, levels: u8) -> Vec<PublicKey> {
        let mut current = user.clone();
        let mut uplines = Vec::new();
        for _ in 0..levels {
            match self.referrers.get(&current) {
                Some(referrer) => {
                    uplines.push(referrer.clone());
                    current = referrer.clone();
                }
                None => break,
            }
        }
        uplines
    }
}

#[async_trait]
impl<'a> BlockchainVerificationState<'a, TestError> for TestChainState {
    async fn pre_verify_tx<'b>(&'b mut self, _tx: &Transaction) -> Result<(), TestError> {
        Ok(())
    }

    async fn get_receiver_balance<'b>(
        &'b mut self,
        account: Cow<'a, PublicKey>,
        asset: Cow<'a, Hash>,
    ) -> Result<&'b mut u64, TestError> {
        let _asset = asset.into_owned();
        let entry = self
            .receiver_balances
            .entry(account.into_owned())
            .or_insert(0);
        Ok(entry)
    }

    async fn get_sender_balance<'b>(
        &'b mut self,
        account: &'a PublicKey,
        asset: &'a Hash,
        _reference: &Reference,
    ) -> Result<&'b mut u64, TestError> {
        let _asset = asset.clone();
        let entry = self.sender_balances.entry(account.clone()).or_insert(0);
        Ok(entry)
    }

    async fn add_sender_output(
        &mut self,
        account: &'a PublicKey,
        asset: &'a Hash,
        output: u64,
    ) -> Result<(), TestError> {
        let _asset = asset.clone();
        let balance = self.sender_balances.entry(account.clone()).or_insert(0);
        *balance = balance.checked_add(output).ok_or(TestError::Overflow)?;
        Ok(())
    }

    async fn get_account_nonce(&mut self, account: &'a PublicKey) -> Result<Nonce, TestError> {
        Ok(*self.nonces.get(account).unwrap_or(&0))
    }

    async fn update_account_nonce(
        &mut self,
        account: &'a PublicKey,
        new_nonce: Nonce,
    ) -> Result<(), TestError> {
        self.nonces.insert(account.clone(), new_nonce);
        Ok(())
    }

    async fn compare_and_swap_nonce(
        &mut self,
        account: &'a PublicKey,
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
        // Use current system time for tests
        std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    async fn set_multisig_state(
        &mut self,
        _account: &'a PublicKey,
        _config: &MultiSigPayload,
    ) -> Result<(), TestError> {
        Ok(())
    }

    async fn get_multisig_state(
        &mut self,
        _account: &'a PublicKey,
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

#[async_trait]
impl<'a> BlockchainApplyState<'a, DummyContractProvider, TestError> for TestChainState {
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
        _user: &'a PublicKey,
        _referrer: &'a PublicKey,
        _tx_hash: &'a Hash,
    ) -> Result<(), TestError> {
        Err(TestError::Unsupported)
    }

    async fn distribute_referral_rewards(
        &mut self,
        from_user: &'a PublicKey,
        _asset: &'a Hash,
        total_amount: u64,
        ratios: &[u16],
    ) -> Result<DistributionResult, TestError> {
        let reward_ratios = ReferralRewardRatios {
            ratios: ratios.to_vec(),
        };
        if !reward_ratios.is_valid() {
            return Err(TestError::Unsupported);
        }

        let uplines = self.get_uplines(from_user, reward_ratios.levels());
        let mut distributions = Vec::new();

        for (i, upline) in uplines.iter().enumerate() {
            if let Some(ratio) = reward_ratios.get_ratio(i) {
                let amount = (total_amount as u128 * ratio as u128 / 10000) as u64;
                if amount > 0 {
                    distributions.push(RewardDistribution {
                        recipient: upline.clone(),
                        amount,
                        level: (i + 1) as u8,
                    });
                }
            }
        }

        Ok(DistributionResult::new(distributions))
    }

    // ===== KYC System Operations (stubs) =====

    async fn set_kyc(
        &mut self,
        _user: &'a PublicKey,
        _level: u16,
        _verified_at: u64,
        _data_hash: &'a Hash,
        _committee_id: &'a Hash,
        _tx_hash: &'a Hash,
    ) -> Result<(), TestError> {
        // Test stub - KYC not needed for referral tests
        Ok(())
    }

    async fn revoke_kyc(
        &mut self,
        _user: &'a PublicKey,
        _reason_hash: &'a Hash,
        _tx_hash: &'a Hash,
    ) -> Result<(), TestError> {
        Ok(())
    }

    async fn renew_kyc(
        &mut self,
        _user: &'a PublicKey,
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
        _user: &'a PublicKey,
        _reason_hash: &'a Hash,
        _expires_at: u64,
        _tx_hash: &'a Hash,
    ) -> Result<(), TestError> {
        Ok(())
    }

    async fn bootstrap_global_committee(
        &mut self,
        _name: String,
        _members: Vec<tos_common::kyc::CommitteeMemberInfo>,
        _threshold: u8,
        _kyc_threshold: u8,
        _max_kyc_level: u16,
        _tx_hash: &'a Hash,
    ) -> Result<Hash, TestError> {
        // Return a dummy committee ID
        Ok(Hash::zero())
    }

    async fn register_committee(
        &mut self,
        _name: String,
        _region: tos_common::kyc::KycRegion,
        _members: Vec<tos_common::kyc::CommitteeMemberInfo>,
        _threshold: u8,
        _kyc_threshold: u8,
        _max_kyc_level: u16,
        _parent_id: &'a Hash,
        _tx_hash: &'a Hash,
    ) -> Result<Hash, TestError> {
        // Return a dummy committee ID
        Ok(Hash::zero())
    }

    async fn update_committee(
        &mut self,
        _committee_id: &'a Hash,
        _update: &tos_common::transaction::CommitteeUpdateData,
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

    async fn get_committee(
        &self,
        _committee_id: &'a Hash,
    ) -> Result<Option<tos_common::kyc::SecurityCommittee>, TestError> {
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
        Ok(false)
    }
}

#[tokio::test]
async fn test_batch_referral_reward_authorization_sender_must_match_from_user() {
    let attacker = KeyPair::new();
    let victim = KeyPair::new();
    let attacker_pk = attacker.get_public_key().compress();
    let victim_pk = victim.get_public_key().compress();

    let payload = BatchReferralRewardPayload::new(TOS_ASSET, victim_pk, 1000, 1, vec![1000]);
    let tx_type = TransactionTypeBuilder::BatchReferralReward(payload);
    let fee_builder = FeeBuilder::Value(0);
    let builder = TransactionBuilder::new(
        TxVersion::T0,
        0, // chain_id: 0 for tests
        attacker_pk.clone(),
        None,
        tx_type,
        fee_builder,
    );

    let mut account_state = TestAccountState::new();
    account_state.set_balance(TOS_ASSET, 10_000);
    account_state.set_nonce(0);

    let tx = builder.build(&mut account_state, &attacker).unwrap();
    let tx_hash = tx.hash();

    let mut chain_state = TestChainState::new();
    chain_state.set_balance(&attacker_pk, 10_000);
    chain_state.set_nonce(&attacker_pk, 0);

    let result = Arc::new(tx)
        .verify(&tx_hash, &mut chain_state, &NoZKPCache)
        .await;
    let err = result.expect_err("Authorization should reject sender != from_user");
    let message = format!("{err}");
    assert!(
        message.contains("from_user"),
        "Expected authorization error, got: {message}"
    );
}

#[tokio::test]
async fn test_batch_referral_reward_refunds_remainder_e2e() {
    let alice = KeyPair::new();
    let bob = KeyPair::new();
    let charlie = KeyPair::new();
    let alice_pk = alice.get_public_key().compress();
    let bob_pk = bob.get_public_key().compress();
    let charlie_pk = charlie.get_public_key().compress();

    let payload =
        BatchReferralRewardPayload::new(TOS_ASSET, alice_pk.clone(), 1000, 2, vec![2500, 1500]);
    let tx_type = TransactionTypeBuilder::BatchReferralReward(payload);
    let fee_builder = FeeBuilder::Value(0);
    let builder = TransactionBuilder::new(
        TxVersion::T0,
        0,
        alice_pk.clone(),
        None,
        tx_type,
        fee_builder,
    );

    let mut account_state = TestAccountState::new();
    account_state.set_balance(TOS_ASSET, 10_000);
    account_state.set_nonce(0);

    let tx = builder.build(&mut account_state, &alice).unwrap();
    let tx_hash = tx.hash();
    if let tos_common::transaction::TransactionType::BatchReferralReward(payload) = tx.get_data() {
        assert_eq!(payload.get_total_amount(), 1000);
    } else {
        panic!("Expected BatchReferralReward transaction");
    }
    assert_eq!(tx.get_source(), &alice_pk);

    let mut chain_state = TestChainState::new();
    chain_state.set_balance(&alice_pk, 10_000);
    chain_state.set_balance(&bob_pk, 0);
    chain_state.set_balance(&charlie_pk, 0);
    chain_state.set_nonce(&alice_pk, 0);
    chain_state.bind_referrer(&alice_pk, &bob_pk);
    chain_state.bind_referrer(&bob_pk, &charlie_pk);

    Arc::new(tx)
        .apply_without_verify(&tx_hash, &mut chain_state)
        .await
        .unwrap();
    let alice_receiver = chain_state
        .receiver_balances
        .get(&alice_pk)
        .copied()
        .unwrap_or(0);
    let bob_balance = chain_state.get_balance(&bob_pk);
    let charlie_balance = chain_state.get_balance(&charlie_pk);

    assert_eq!(
        alice_receiver, 600,
        "Remainder should be credited to sender"
    );
    assert_eq!(bob_balance, 250, "Bob should receive 25%");
    assert_eq!(charlie_balance, 150, "Charlie should receive 15%");
}
