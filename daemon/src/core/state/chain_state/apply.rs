use crate::core::{
    error::BlockchainError,
    storage::{
        Storage, VersionedContract, VersionedContractBalance, VersionedContractData,
        VersionedMultiSig, VersionedSupply,
    },
    BlockScheduledExecutionResults,
};
use async_trait::async_trait;
use indexmap::IndexMap;
use log::{debug, trace};
use std::{
    borrow::Cow,
    collections::{hash_map::Entry, HashMap, HashSet},
    ops::{Deref, DerefMut},
};
use tos_common::{
    account::{AgentAccountMeta, BalanceType, EnergyResource, Nonce, SessionKey, VersionedNonce},
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
    // Results from processing scheduled executions at this block
    // Contains aggregated transfers and miner rewards
    scheduled_execution_results: BlockScheduledExecutionResults,
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
        account: Cow<'a, PublicKey>,
        asset: Cow<'a, Hash>,
        reference: &Reference,
    ) -> Result<&'b mut u64, BlockchainError> {
        self.inner
            .get_sender_balance(account, asset, reference)
            .await
    }

    /// Apply new output to a sender account
    async fn add_sender_output(
        &mut self,
        account: Cow<'a, PublicKey>,
        asset: Cow<'a, Hash>,
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

    async fn account_exists(&mut self, account: &'a PublicKey) -> Result<bool, BlockchainError> {
        self.inner.account_exists(account).await
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

    async fn get_agent_account_meta(
        &mut self,
        account: &'a CompressedPublicKey,
    ) -> Result<Option<AgentAccountMeta>, BlockchainError> {
        self.inner.get_agent_account_meta(account).await
    }

    async fn set_agent_account_meta(
        &mut self,
        account: &'a CompressedPublicKey,
        meta: &AgentAccountMeta,
    ) -> Result<(), BlockchainError> {
        self.inner.set_agent_account_meta(account, meta).await
    }

    async fn delete_agent_account_meta(
        &mut self,
        account: &'a CompressedPublicKey,
    ) -> Result<(), BlockchainError> {
        self.inner.delete_agent_account_meta(account).await
    }

    async fn get_session_key(
        &mut self,
        account: &'a CompressedPublicKey,
        key_id: u64,
    ) -> Result<Option<SessionKey>, BlockchainError> {
        self.inner.get_session_key(account, key_id).await
    }

    async fn set_session_key(
        &mut self,
        account: &'a CompressedPublicKey,
        session_key: &SessionKey,
    ) -> Result<(), BlockchainError> {
        self.inner.set_session_key(account, session_key).await
    }

    async fn delete_session_key(
        &mut self,
        account: &'a CompressedPublicKey,
        key_id: u64,
    ) -> Result<(), BlockchainError> {
        self.inner.delete_session_key(account, key_id).await
    }

    /// Get the block version
    fn get_block_version(&self) -> BlockVersion {
        self.block_version
    }

    /// Get the timestamp to use for verification (delegates to inner)
    fn get_verification_timestamp(&self) -> u64 {
        self.inner.get_verification_timestamp()
    }

    /// Get the topoheight to use for verification (delegates to inner)
    fn get_verification_topoheight(&self) -> u64 {
        self.inner.get_verification_topoheight()
    }

    /// Get the recyclable TOS amount from expired freeze records (delegates to inner)
    async fn get_recyclable_tos(&mut self, account: &'a PublicKey) -> Result<u64, BlockchainError> {
        self.inner.get_recyclable_tos(account).await
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

    // ===== TNS (TOS Name Service) Verification Methods =====

    async fn is_name_registered(&self, name_hash: &Hash) -> Result<bool, BlockchainError> {
        self.inner.is_name_registered(name_hash).await
    }

    async fn account_has_name(
        &self,
        account: &'a CompressedPublicKey,
    ) -> Result<bool, BlockchainError> {
        self.inner.account_has_name(account).await
    }

    async fn get_account_name_hash(
        &self,
        account: &'a CompressedPublicKey,
    ) -> Result<Option<Hash>, BlockchainError> {
        self.inner.get_account_name_hash(account).await
    }

    async fn is_message_id_used(&self, message_id: &Hash) -> Result<bool, BlockchainError> {
        self.inner.is_message_id_used(message_id).await
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

    async fn get_energy_resource(
        &mut self,
        account: Cow<'a, CompressedPublicKey>,
    ) -> Result<Option<EnergyResource>, BlockchainError> {
        self.inner.internal_get_energy_resource(account).await
    }

    async fn set_energy_resource(
        &mut self,
        account: Cow<'a, CompressedPublicKey>,
        energy_resource: EnergyResource,
    ) -> Result<(), BlockchainError> {
        self.inner
            .cache_energy_resource(account.clone(), energy_resource.clone());
        self.inner
            .storage
            .set_energy_resource(&account, self.inner.topoheight, &energy_resource)
            .await
    }

    fn get_contract_executor(&self) -> std::sync::Arc<dyn tos_common::contract::ContractExecutor> {
        self.executor.clone()
    }

    async fn add_contract_events(
        &mut self,
        events: Vec<tos_common::contract::ContractEvent>,
        contract: &Hash,
        tx_hash: &'a Hash,
    ) -> Result<(), BlockchainError> {
        // Persist contract events to storage
        // This enables event filtering and querying
        use crate::core::storage::StoredContractEvent;
        use tos_common::crypto::Hashable;

        let event_count = events.len();
        if event_count == 0 {
            return Ok(());
        }

        let block_hash = self.block.hash();
        let topoheight = self.inner.topoheight;

        // Convert common::ContractEvent to storage::StoredContractEvent
        let stored_events: Vec<StoredContractEvent> = events
            .into_iter()
            .enumerate()
            .map(|(idx, event)| StoredContractEvent {
                contract: contract.clone(),
                tx_hash: tx_hash.clone(),
                block_hash: block_hash.clone(),
                topoheight,
                log_index: idx as u32,
                topics: event.topics,
                data: event.data,
            })
            .collect();

        // Store events to persistent storage
        self.inner
            .storage
            .store_contract_events(stored_events)
            .await?;

        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "Contract {} emitted {} events in TX {} (persisted at topoheight {})",
                contract, event_count, tx_hash, topoheight
            );
        }

        Ok(())
    }

    // ===== TNS (TOS Name Service) Apply Methods =====

    async fn register_name(
        &mut self,
        name_hash: Hash,
        owner: &'a CompressedPublicKey,
    ) -> Result<(), BlockchainError> {
        self.inner
            .storage
            .register_name(name_hash, owner.clone())
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
                scheduled_execution_results: BlockScheduledExecutionResults::default(),
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

    /// Set the results from processing scheduled executions at this block
    ///
    /// This should be called after `process_scheduled_executions` to store the results
    /// which will be applied during `apply_changes`.
    pub fn set_scheduled_execution_results(&mut self, results: BlockScheduledExecutionResults) {
        self.contract_manager.scheduled_execution_results = results;
    }

    /// Get the total miner rewards from scheduled executions
    ///
    /// This returns the sum of all miner rewards from processing scheduled executions
    /// in this block. These should be added to the block miner's reward.
    pub fn get_scheduled_execution_miner_rewards(&self) -> u64 {
        self.contract_manager
            .scheduled_execution_results
            .total_miner_rewards
    }

    /// Get the scheduled execution results for this block
    ///
    /// Returns a reference to all scheduled execution results for firing events.
    pub fn get_scheduled_execution_results(&self) -> &BlockScheduledExecutionResults {
        &self.contract_manager.scheduled_execution_results
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
        let accounts = std::mem::take(&mut self.inner.accounts);
        let account_keys: HashSet<PublicKey> =
            accounts.keys().map(|key| key.as_ref().clone()).collect();
        for (key, mut account) in accounts {
            let key = key.into_owned();
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
                .set_last_nonce_to(&key, self.inner.topoheight, &account.nonce)
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
                    .set_last_multisig_to(&key, self.inner.topoheight, versioned)
                    .await?;
            }

            let balances = self
                .inner
                .receiver_balances
                .entry(Cow::Owned(key.clone()))
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
                match balances.entry(Cow::Owned(asset.clone())) {
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
                                .get_new_versioned_balance(
                                    &key,
                                    asset.as_ref(),
                                    self.inner.topoheight,
                                )
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
                .entry(Cow::Owned(key.clone()))
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

                match uno_balances.entry(Cow::Owned(asset.clone())) {
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
                                .get_new_versioned_uno_balance(
                                    &key,
                                    asset.as_ref(),
                                    self.inner.topoheight,
                                )
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
        if log::log_enabled!(log::Level::Debug) {
            debug!("Storing contracts");
        }
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

        if log::log_enabled!(log::Level::Debug) {
            debug!("Storing contract storage changes");
        }
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

        if log::log_enabled!(log::Level::Debug) {
            debug!("applying external transfers");
        }
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

        // Apply transfers from scheduled executions
        if !self
            .contract_manager
            .scheduled_execution_results
            .aggregated_transfers
            .is_empty()
        {
            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "applying {} scheduled execution transfer destinations",
                    self.contract_manager
                        .scheduled_execution_results
                        .aggregated_transfers
                        .len()
                );
            }
            for (key, assets) in self
                .contract_manager
                .scheduled_execution_results
                .aggregated_transfers
            {
                for (asset, amount) in assets {
                    if log::log_enabled!(log::Level::Trace) {
                        trace!(
                            "Applying scheduled execution transfer: {} {} to {} at topoheight {}",
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
        }

        // Apply all the contract outputs
        if log::log_enabled!(log::Level::Debug) {
            debug!("storing contract outputs");
        }
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
            if !account_keys.contains(account.as_ref())
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
            if !account_keys.contains(account.as_ref())
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

        // Apply agent account metadata changes
        for (account, meta) in self.inner.agent_account_meta {
            match meta {
                Some(meta) => {
                    self.inner
                        .storage
                        .set_agent_account_meta(&account, &meta)
                        .await?;
                }
                None => {
                    self.inner
                        .storage
                        .delete_agent_account_meta(&account)
                        .await?;
                }
            }
        }

        // Apply agent session key changes
        for ((account, key_id), session_key) in self.inner.agent_session_keys {
            match session_key {
                Some(session_key) => {
                    self.inner
                        .storage
                        .set_session_key(&account, &session_key)
                        .await?;
                }
                None => {
                    self.inner
                        .storage
                        .delete_session_key(&account, key_id)
                        .await?;
                }
            }
        }

        Ok(())
    }
}
