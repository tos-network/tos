#![allow(clippy::disallowed_methods)]
#![allow(clippy::too_many_arguments)]

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    sync::Arc,
};

use async_trait::async_trait;
use indexmap::IndexMap;
use tos_common::{
    account::{EnergyResource, Nonce},
    arbitration::ArbiterAccount,
    block::{Block, BlockHeader, BlockVersion, EXTRA_NONCE_SIZE},
    contract::{
        AssetChanges, ChainState as ContractChainState, ContractCache, ContractEventTracker,
        ContractOutput, ContractProvider, ContractStorage,
    },
    crypto::{
        elgamal::{Ciphertext, CompressedPublicKey},
        Hash, Hashable, PublicKey,
    },
    escrow::EscrowAccount,
    immutable::Immutable,
    network::Network,
    nft::{NftCache, NftStorageProvider},
    serializer::Serializer,
    transaction::{
        verify::{BlockchainApplyState, BlockchainVerificationState, ContractEnvironment},
        ContractDeposit, MultiSigPayload, Reference, Transaction,
    },
};
use tos_kernel::{Environment, Module, ValueCell};

#[derive(Debug, Clone)]
pub enum TestError {
    Unsupported,
    MissingAccount,
    MissingAsset,
}

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for TestError {}

#[derive(Default)]
pub struct DummyContractProvider;

impl ContractStorage for DummyContractProvider {
    fn load_data(
        &self,
        _contract: &Hash,
        _key: &ValueCell,
        _topoheight: u64,
    ) -> Result<Option<(u64, Option<ValueCell>)>, anyhow::Error> {
        Ok(None)
    }

    fn load_data_latest_topoheight(
        &self,
        _contract: &Hash,
        _key: &ValueCell,
        _topoheight: u64,
    ) -> Result<Option<u64>, anyhow::Error> {
        Ok(None)
    }

    fn has_data(
        &self,
        _contract: &Hash,
        _key: &ValueCell,
        _topoheight: u64,
    ) -> Result<bool, anyhow::Error> {
        Ok(false)
    }

    fn has_contract(&self, _contract: &Hash, _topoheight: u64) -> Result<bool, anyhow::Error> {
        Ok(false)
    }
}

impl ContractProvider for DummyContractProvider {
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
        Ok(false)
    }

    fn load_contract_module(
        &self,
        _contract: &Hash,
        _topoheight: u64,
    ) -> Result<Option<Vec<u8>>, anyhow::Error> {
        Ok(None)
    }
}

impl NftStorageProvider for DummyContractProvider {
    fn get_collection(
        &self,
        _id: &Hash,
        _topoheight: u64,
    ) -> Result<Option<(u64, tos_common::nft::NftCollection)>, anyhow::Error> {
        Ok(None)
    }

    fn get_token(
        &self,
        _collection: &Hash,
        _token_id: u64,
        _topoheight: u64,
    ) -> Result<Option<(u64, tos_common::nft::Nft)>, anyhow::Error> {
        Ok(None)
    }

    fn get_owner_balance(
        &self,
        _collection: &Hash,
        _owner: &PublicKey,
        _topoheight: u64,
    ) -> Result<Option<(u64, u64)>, anyhow::Error> {
        Ok(None)
    }

    fn get_operator_approval(
        &self,
        _owner: &PublicKey,
        _collection: &Hash,
        _operator: &PublicKey,
        _topoheight: u64,
    ) -> Result<Option<(u64, bool)>, anyhow::Error> {
        Ok(None)
    }

    fn get_mint_count(
        &self,
        _collection: &Hash,
        _user: &PublicKey,
        _topoheight: u64,
    ) -> Result<Option<(u64, u64)>, anyhow::Error> {
        Ok(None)
    }

    fn get_collection_nonce(&self, _topoheight: u64) -> Result<Option<(u64, u64)>, anyhow::Error> {
        Ok(None)
    }

    fn get_tba(
        &self,
        _collection: &Hash,
        _token_id: u64,
        _topoheight: u64,
    ) -> Result<Option<(u64, tos_common::nft::TokenBoundAccount)>, anyhow::Error> {
        Ok(None)
    }

    fn get_rental_listing(
        &self,
        _listing_id: &Hash,
        _topoheight: u64,
    ) -> Result<Option<(u64, tos_common::nft::RentalListing)>, anyhow::Error> {
        Ok(None)
    }

    fn get_active_rental(
        &self,
        _collection: &Hash,
        _token_id: u64,
        _topoheight: u64,
    ) -> Result<Option<(u64, tos_common::nft::NftRental)>, anyhow::Error> {
        Ok(None)
    }
}

pub struct TestApplyState {
    balances: HashMap<PublicKey, HashMap<Hash, u64>>,
    nonces: HashMap<PublicKey, Nonce>,
    multisig: HashMap<PublicKey, MultiSigPayload>,
    escrows: HashMap<Hash, EscrowAccount>,
    arbiters: HashMap<PublicKey, ArbiterAccount>,
    escrow_history: HashMap<Hash, Vec<(u64, Hash)>>,
    pending_releases: HashSet<(u64, Hash)>,
    committees: HashMap<Hash, tos_common::kyc::SecurityCommittee>,
    env: Environment,
    block: Block,
    block_hash: Hash,
    block_version: BlockVersion,
    topoheight: u64,
    network: Network,
    executor: Arc<dyn tos_common::contract::ContractExecutor>,
    gas_fee: u64,
    burned: u64,
}

impl TestApplyState {
    pub fn new(topoheight: u64) -> Self {
        let (block, block_hash) = create_dummy_block();
        Self {
            balances: HashMap::new(),
            nonces: HashMap::new(),
            multisig: HashMap::new(),
            escrows: HashMap::new(),
            arbiters: HashMap::new(),
            escrow_history: HashMap::new(),
            pending_releases: HashSet::new(),
            committees: HashMap::new(),
            env: Environment::new(),
            block,
            block_hash,
            block_version: BlockVersion::Nobunaga,
            topoheight,
            network: Network::Devnet,
            executor: Arc::new(tos_daemon::tako_integration::TakoContractExecutor::new()),
            gas_fee: 0,
            burned: 0,
        }
    }

    pub fn insert_account(&mut self, key: PublicKey, balance: u64, nonce: Nonce) {
        self.balances
            .entry(key.clone())
            .or_default()
            .insert(tos_common::config::TOS_ASSET, balance);
        self.nonces.insert(key, nonce);
    }

    #[allow(dead_code)]
    pub fn set_topoheight(&mut self, topoheight: u64) {
        self.topoheight = topoheight;
    }

    #[allow(dead_code)]
    pub fn insert_committee(&mut self, committee: tos_common::kyc::SecurityCommittee) {
        self.committees.insert(committee.id.clone(), committee);
    }

    #[allow(dead_code)]
    pub fn list_pending_releases(&self, up_to: u64, limit: usize) -> Vec<(u64, Hash)> {
        let mut entries: Vec<(u64, Hash)> = self
            .pending_releases
            .iter()
            .filter(|(release_at, _)| *release_at <= up_to)
            .cloned()
            .collect();
        entries.sort_by_key(|(release_at, _)| *release_at);
        if entries.len() > limit {
            entries.truncate(limit);
        }
        entries
    }

    pub fn get_balance(&self, key: &PublicKey, asset: &Hash) -> Option<u64> {
        self.balances
            .get(key)
            .and_then(|assets| assets.get(asset).copied())
    }
}

#[async_trait]
impl<'a> BlockchainVerificationState<'a, TestError> for TestApplyState {
    async fn pre_verify_tx<'b>(&'b mut self, _tx: &Transaction) -> Result<(), TestError> {
        Ok(())
    }

    async fn get_receiver_balance<'b>(
        &'b mut self,
        account: Cow<'a, CompressedPublicKey>,
        asset: Cow<'a, Hash>,
    ) -> Result<&'b mut u64, TestError> {
        let account_entry = self.balances.entry(account.into_owned()).or_default();
        Ok(account_entry.entry(asset.into_owned()).or_insert(0))
    }

    async fn get_sender_balance<'b>(
        &'b mut self,
        account: Cow<'a, CompressedPublicKey>,
        asset: Cow<'a, Hash>,
        _reference: &Reference,
    ) -> Result<&'b mut u64, TestError> {
        let assets = self
            .balances
            .get_mut(account.as_ref())
            .ok_or(TestError::MissingAccount)?;
        assets
            .get_mut(asset.as_ref())
            .ok_or(TestError::MissingAsset)
    }

    async fn add_sender_output(
        &mut self,
        _account: Cow<'a, CompressedPublicKey>,
        _asset: Cow<'a, Hash>,
        _output: u64,
    ) -> Result<(), TestError> {
        Ok(())
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

    async fn get_account_nonce(
        &mut self,
        account: &'a CompressedPublicKey,
    ) -> Result<Nonce, TestError> {
        self.nonces
            .get(account)
            .copied()
            .ok_or(TestError::MissingAccount)
    }

    async fn account_exists(
        &mut self,
        account: &'a CompressedPublicKey,
    ) -> Result<bool, TestError> {
        Ok(self.balances.contains_key(account))
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
        let current = self
            .nonces
            .get(account)
            .copied()
            .ok_or(TestError::MissingAccount)?;
        if current == expected {
            self.nonces.insert(account.clone(), new_value);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn get_escrow(&mut self, escrow_id: &Hash) -> Result<Option<EscrowAccount>, TestError> {
        Ok(self.escrows.get(escrow_id).cloned())
    }

    async fn get_arbiter(
        &mut self,
        arbiter: &'a CompressedPublicKey,
    ) -> Result<Option<ArbiterAccount>, TestError> {
        Ok(self.arbiters.get(arbiter).cloned())
    }

    fn get_block_version(&self) -> BlockVersion {
        self.block_version
    }

    fn get_verification_timestamp(&self) -> u64 {
        0
    }

    fn get_verification_topoheight(&self) -> u64 {
        self.topoheight
    }

    async fn get_recyclable_tos(
        &mut self,
        _account: &'a CompressedPublicKey,
    ) -> Result<u64, TestError> {
        Ok(0)
    }

    async fn set_multisig_state(
        &mut self,
        account: &'a CompressedPublicKey,
        config: &MultiSigPayload,
    ) -> Result<(), TestError> {
        self.multisig.insert(account.clone(), config.clone());
        Ok(())
    }

    async fn get_multisig_state(
        &mut self,
        account: &'a CompressedPublicKey,
    ) -> Result<Option<&MultiSigPayload>, TestError> {
        Ok(self.multisig.get(account))
    }

    async fn get_environment(&mut self) -> Result<&Environment, TestError> {
        Ok(&self.env)
    }

    fn get_network(&self) -> Network {
        self.network
    }

    async fn set_contract_module(
        &mut self,
        _hash: &Hash,
        _module: &'a Module,
    ) -> Result<(), TestError> {
        Err(TestError::Unsupported)
    }

    async fn load_contract_module(&mut self, _hash: &Hash) -> Result<bool, TestError> {
        Err(TestError::Unsupported)
    }

    async fn get_contract_module_with_environment(
        &self,
        _hash: &Hash,
    ) -> Result<(&Module, &Environment), TestError> {
        Err(TestError::Unsupported)
    }

    async fn is_name_registered(&self, _name_hash: &Hash) -> Result<bool, TestError> {
        Ok(false)
    }

    async fn account_has_name(&self, _account: &'a CompressedPublicKey) -> Result<bool, TestError> {
        Ok(false)
    }

    async fn get_account_name_hash(
        &self,
        _account: &'a CompressedPublicKey,
    ) -> Result<Option<Hash>, TestError> {
        Ok(None)
    }

    async fn is_message_id_used(&self, _message_id: &Hash) -> Result<bool, TestError> {
        Ok(false)
    }
}

#[async_trait]
impl<'a> BlockchainApplyState<'a, DummyContractProvider, TestError> for TestApplyState {
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
        Err(TestError::Unsupported)
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
        _nft_cache: NftCache,
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

    fn get_contract_executor(&self) -> Arc<dyn tos_common::contract::ContractExecutor> {
        Arc::clone(&self.executor)
    }

    async fn add_contract_events(
        &mut self,
        _events: Vec<tos_common::contract::ContractEvent>,
        _contract: &Hash,
        _tx_hash: &'a Hash,
    ) -> Result<(), TestError> {
        Ok(())
    }

    async fn set_escrow(&mut self, escrow: &EscrowAccount) -> Result<(), TestError> {
        self.escrows.insert(escrow.id.clone(), escrow.clone());
        Ok(())
    }

    async fn add_pending_release(
        &mut self,
        release_at: u64,
        escrow_id: &Hash,
    ) -> Result<(), TestError> {
        self.pending_releases
            .insert((release_at, escrow_id.clone()));
        Ok(())
    }

    async fn remove_pending_release(
        &mut self,
        release_at: u64,
        escrow_id: &Hash,
    ) -> Result<(), TestError> {
        self.pending_releases
            .remove(&(release_at, escrow_id.clone()));
        Ok(())
    }

    async fn add_escrow_history(
        &mut self,
        escrow_id: &Hash,
        topoheight: u64,
        tx_hash: &Hash,
    ) -> Result<(), TestError> {
        self.escrow_history
            .entry(escrow_id.clone())
            .or_default()
            .push((topoheight, tx_hash.clone()));
        Ok(())
    }

    async fn set_arbiter(&mut self, arbiter: &ArbiterAccount) -> Result<(), TestError> {
        self.arbiters
            .insert(arbiter.public_key.clone(), arbiter.clone());
        Ok(())
    }

    async fn remove_arbiter(&mut self, arbiter: &CompressedPublicKey) -> Result<(), TestError> {
        self.arbiters.remove(arbiter);
        Ok(())
    }

    async fn get_committee(
        &self,
        committee_id: &'a Hash,
    ) -> Result<Option<tos_common::kyc::SecurityCommittee>, TestError> {
        Ok(self.committees.get(committee_id).cloned())
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

    async fn set_kyc(
        &mut self,
        _user: &'a CompressedPublicKey,
        _level: u16,
        _verified_at: u64,
        _data_hash: &'a Hash,
        _committee_id: &'a Hash,
        _tx_hash: &'a Hash,
    ) -> Result<(), TestError> {
        Err(TestError::Unsupported)
    }

    async fn revoke_kyc(
        &mut self,
        _user: &'a CompressedPublicKey,
        _reason_hash: &'a Hash,
        _tx_hash: &'a Hash,
    ) -> Result<(), TestError> {
        Err(TestError::Unsupported)
    }

    async fn renew_kyc(
        &mut self,
        _user: &'a CompressedPublicKey,
        _verified_at: u64,
        _data_hash: &'a Hash,
        _tx_hash: &'a Hash,
    ) -> Result<(), TestError> {
        Err(TestError::Unsupported)
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
        Err(TestError::Unsupported)
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
        Err(TestError::Unsupported)
    }

    async fn emergency_suspend_kyc(
        &mut self,
        _user: &'a CompressedPublicKey,
        _reason_hash: &'a Hash,
        _expires_at: u64,
        _tx_hash: &'a Hash,
    ) -> Result<(), TestError> {
        Err(TestError::Unsupported)
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
        Err(TestError::Unsupported)
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
        Err(TestError::Unsupported)
    }

    async fn update_committee(
        &mut self,
        _committee_id: &'a Hash,
        _update: &tos_common::transaction::CommitteeUpdateData,
    ) -> Result<(), TestError> {
        Err(TestError::Unsupported)
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
    ) -> Result<tos_common::referral::DistributionResult, TestError> {
        Err(TestError::Unsupported)
    }

    async fn register_name(
        &mut self,
        _name_hash: Hash,
        _owner: &'a CompressedPublicKey,
    ) -> Result<(), TestError> {
        Err(TestError::Unsupported)
    }

    async fn store_ephemeral_message(
        &mut self,
        _message_id: Hash,
        _sender_name_hash: Hash,
        _recipient_name_hash: Hash,
        _message_nonce: u64,
        _ttl_blocks: u32,
        _encrypted_content: Vec<u8>,
        _receiver_handle: [u8; 32],
        _current_topoheight: u64,
    ) -> Result<(), TestError> {
        Err(TestError::Unsupported)
    }
}

fn create_dummy_block() -> (Block, Hash) {
    let mut buffer = Vec::new();
    let mut writer = tos_common::serializer::Writer::new(&mut buffer);
    writer.write_bytes(&[0u8; 32]);
    let data = writer.as_bytes();

    let mut reader = tos_common::serializer::Reader::new(data);
    let miner = CompressedPublicKey::read(&mut reader).expect("create dummy block pubkey");

    let header = BlockHeader::new(
        BlockVersion::Nobunaga,
        0,
        0,
        indexmap::IndexSet::new(),
        [0u8; EXTRA_NONCE_SIZE],
        miner,
        indexmap::IndexSet::new(),
    );

    let block = Block::new(Immutable::Owned(header), vec![]);
    let hash = block.hash();
    (block, hash)
}
