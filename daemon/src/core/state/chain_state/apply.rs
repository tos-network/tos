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
    account::{BalanceType, EnergyResource, Nonce, VersionedNonce},
    ai_mining::AIMiningState,
    asset::VersionedAssetData,
    block::{Block, BlockVersion, TopoHeight},
    contract::{
        AssetChanges, ChainState as ContractChainState, ContractCache, ContractEventTracker,
        ContractOutput, ContractProvider as ContractInfoProvider, ScheduledExecution,
        ScheduledExecutionKind, MAX_SCHEDULED_EXECUTIONS_PER_BLOCK,
        MAX_SCHEDULED_EXECUTION_GAS_PER_BLOCK, OFFER_MINER_PERCENT,
    },
    crypto::{elgamal::CompressedPublicKey, Hash, PublicKey},
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
    // Planned executions for the current block
    scheduled_executions: IndexMap<Hash, ScheduledExecution>,
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
        account: &'a CompressedPublicKey,
    ) -> Result<Option<EnergyResource>, BlockchainError> {
        self.inner.storage.get_energy_resource(account).await
    }

    async fn set_energy_resource(
        &mut self,
        account: &'a CompressedPublicKey,
        energy_resource: EnergyResource,
    ) -> Result<(), BlockchainError> {
        self.inner
            .storage
            .set_energy_resource(account, self.inner.topoheight, &energy_resource)
            .await
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
        Self {
            inner: ChainState::with(
                StorageReference::Mutable(storage),
                environment,
                stable_topoheight,
                topoheight,
                block_version,
            ),
            burned_supply,
            contract_manager: ContractManager {
                outputs: HashMap::new(),
                caches: HashMap::new(),
                assets: HashMap::new(),
                tracker: ContractEventTracker::default(),
                scheduled_executions: IndexMap::new(),
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
            trace!(
                "Saving nonce {} for {} at topoheight {}",
                account.nonce,
                key.as_address(self.inner.storage.is_mainnet()),
                self.inner.topoheight
            );
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
                trace!(
                    "Saving multisig for {} at topoheight {}",
                    key.as_address(self.inner.storage.is_mainnet()),
                    self.inner.topoheight
                );
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
                trace!(
                    "{} {} updated for {} at topoheight {}",
                    echange.version,
                    asset,
                    key.as_address(self.inner.storage.is_mainnet()),
                    self.inner.topoheight
                );
                let Echange {
                    mut version,
                    output_sum,
                    output_balance_used,
                    new_version,
                    ..
                } = echange;
                trace!("sender output sum: {}", output_sum);
                match balances.entry(Cow::Borrowed(asset)) {
                    Entry::Occupied(mut o) => {
                        trace!(
                            "{} already has a balance for {} at topoheight {}",
                            key.as_address(self.inner.storage.is_mainnet()),
                            asset,
                            self.inner.topoheight
                        );
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
                        trace!(
                            "{} has no balance for {} at topoheight {}",
                            key.as_address(self.inner.storage.is_mainnet()),
                            asset,
                            self.inner.topoheight
                        );
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
                            trace!(
                                "{} has no balance for {} at topoheight {}, substract output sum",
                                key.as_address(self.inner.storage.is_mainnet()),
                                asset,
                                self.inner.topoheight
                            );
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
        }

        // Apply the assets
        for (asset, changes) in self.contract_manager.assets {
            if let Some(changes) = changes {
                let (state, data) = changes.data;
                if state.should_be_stored() {
                    trace!(
                        "Saving asset {} at topoheight {}",
                        asset,
                        self.inner.topoheight
                    );
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
                        trace!(
                            "Saving supply {} for {} at topoheight {}",
                            supply,
                            asset,
                            self.inner.topoheight
                        );
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
                trace!(
                    "Saving contract {} at topoheight {}",
                    hash,
                    self.inner.topoheight
                );
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
                    trace!(
                        "Saving contract data {} key {} at topoheight {}",
                        contract,
                        key,
                        self.inner.topoheight
                    );
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
                        trace!(
                            "Saving contract balance {} for {} at topoheight {}",
                            balance,
                            asset,
                            self.inner.topoheight
                        );
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
                trace!(
                    "Transfering {} {} to {} at topoheight {}",
                    amount,
                    asset,
                    key.as_address(self.inner.storage.is_mainnet()),
                    self.inner.topoheight
                );
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

        // Apply all scheduled executions at their topoheight
        debug!("applying scheduled executions at topoheights");
        for (_, execution) in self.contract_manager.scheduled_executions {
            if let ScheduledExecutionKind::TopoHeight(execution_topoheight) = execution.kind {
                trace!(
                    "storing scheduled execution of contract {} with caller {} at topoheight {}",
                    execution.contract,
                    execution.hash,
                    self.inner.topoheight
                );
                self.inner
                    .storage
                    .set_contract_scheduled_execution_at_topoheight(
                        &execution.contract,
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
                debug!(
                    "{} has now a balance but without any nonce registered, set default (0) nonce",
                    account.as_address(self.inner.storage.is_mainnet())
                );
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
                trace!(
                    "Saving versioned balance {} for {} at topoheight {}",
                    version,
                    account.as_address(self.inner.storage.is_mainnet()),
                    self.inner.topoheight
                );
                self.inner
                    .storage
                    .set_last_balance_to(&account, &asset, self.inner.topoheight, &version)
                    .await?;
            }
        }

        Ok(())
    }

    // Execute all the block end scheduled executions
    pub async fn process_executions_at_block_end(&mut self) -> Result<(), BlockchainError> {
        trace!("process executions at block end");

        let mut remaining_executions = IndexMap::new();
        let mut block_end_executions = Vec::new();

        // Collect all scheduled executions for block end
        for (hash, execution) in self.contract_manager.scheduled_executions.drain(..) {
            match execution.kind {
                ScheduledExecutionKind::BlockEnd => {
                    block_end_executions.push(execution);
                }
                _ => {
                    remaining_executions.insert(hash, execution);
                }
            }
        }

        self.contract_manager.scheduled_executions = remaining_executions;

        // Process block end executions
        for execution in block_end_executions {
            debug!(
                "processing block end scheduled execution of contract {} with caller {}",
                execution.contract, execution.hash
            );
            // TODO: Implement actual contract execution when VM integration is complete
            // For now, we just log the execution
        }

        Ok(())
    }

    /// Execute all scheduled executions for current topoheight with priority ordering.
    ///
    /// Executions are processed in priority order:
    /// 1. Higher offer amount first
    /// 2. Earlier registration time (FIFO) for equal offers
    /// 3. Contract hash for deterministic ordering
    ///
    /// Per-block limits are enforced:
    /// - MAX_SCHEDULED_EXECUTIONS_PER_BLOCK (100)
    /// - MAX_SCHEDULED_EXECUTION_GAS_PER_BLOCK (100M CU)
    ///
    /// Executions that exceed limits are deferred to the next topoheight.
    pub async fn process_scheduled_executions(&mut self) -> Result<(), BlockchainError> {
        let topoheight = self.inner.topoheight;

        if log::log_enabled!(log::Level::Trace) {
            trace!("process scheduled executions at topoheight {}", topoheight);
        }

        // Fetch all executions sorted by priority (higher offer first)
        let executions: Vec<ScheduledExecution> = self
            .inner
            .storage
            .get_priority_sorted_scheduled_executions_at_topoheight(topoheight)
            .await?
            .collect::<Result<Vec<_>, _>>()?;

        if executions.is_empty() {
            return Ok(());
        }

        // Track limits
        let mut exec_count = 0usize;
        let mut gas_budget = MAX_SCHEDULED_EXECUTION_GAS_PER_BLOCK;
        let mut executed = Vec::new();
        let mut deferred = Vec::new();

        for execution in executions {
            // Check per-block limits
            if exec_count >= MAX_SCHEDULED_EXECUTIONS_PER_BLOCK {
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "Deferring execution {} - max executions per block reached",
                        execution.hash
                    );
                }
                deferred.push(execution);
                continue;
            }

            if gas_budget < execution.max_gas {
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "Deferring execution {} - insufficient gas budget ({} < {})",
                        execution.hash, gas_budget, execution.max_gas
                    );
                }
                deferred.push(execution);
                continue;
            }

            // Execute the scheduled call
            let result = self.execute_scheduled_call(&execution).await;

            match result {
                Ok(gas_used) => {
                    // Successful execution
                    gas_budget = gas_budget.saturating_sub(gas_used);
                    exec_count = exec_count.saturating_add(1);

                    // Pay 70% of offer to miner
                    let miner_reward = execution
                        .offer_amount
                        .saturating_mul(OFFER_MINER_PERCENT)
                        .saturating_div(100);
                    self.gas_fee = self.gas_fee.saturating_add(miner_reward);

                    // Refund unused gas to scheduler contract
                    let gas_refund = execution.max_gas.saturating_sub(gas_used);
                    if gas_refund > 0 {
                        self.refund_gas_to_contract(&execution.scheduler_contract, gas_refund)
                            .await?;
                    }

                    executed.push(execution);
                }
                Err(e) => {
                    // Execution failed - still count gas, no refund
                    if log::log_enabled!(log::Level::Warn) {
                        log::warn!("Scheduled execution {} failed: {}", execution.hash, e);
                    }
                    gas_budget = gas_budget.saturating_sub(execution.max_gas);
                    exec_count = exec_count.saturating_add(1);

                    // Pay 70% of offer to miner even on failure
                    let miner_reward = execution
                        .offer_amount
                        .saturating_mul(OFFER_MINER_PERCENT)
                        .saturating_div(100);
                    self.gas_fee = self.gas_fee.saturating_add(miner_reward);

                    // Mark as failed but still remove from pending
                    executed.push(execution);
                }
            }
        }

        // Delete executed entries from storage
        for execution in &executed {
            self.inner
                .storage
                .delete_contract_scheduled_execution(&execution.contract, execution)
                .await?;
        }

        // Defer overflow executions to next topoheight
        if !deferred.is_empty() {
            let next_topoheight = topoheight.saturating_add(1);
            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "Deferring {} executions to topoheight {}",
                    deferred.len(),
                    next_topoheight
                );
            }

            for mut execution in deferred {
                // Increment defer count
                execution.defer_count = execution.defer_count.saturating_add(1);

                // Update kind to next topoheight
                execution.kind = ScheduledExecutionKind::TopoHeight(next_topoheight);

                // Re-register for next topoheight
                self.inner
                    .storage
                    .set_contract_scheduled_execution_at_topoheight(
                        &execution.contract,
                        execution.registration_topoheight,
                        &execution,
                        next_topoheight,
                    )
                    .await?;
            }
        }

        if log::log_enabled!(log::Level::Debug) && exec_count > 0 {
            debug!(
                "Processed {} scheduled executions at topoheight {} (gas used: {})",
                exec_count,
                topoheight,
                MAX_SCHEDULED_EXECUTION_GAS_PER_BLOCK.saturating_sub(gas_budget)
            );
        }

        Ok(())
    }

    /// Execute a single scheduled contract call.
    /// Returns the gas used on success.
    async fn execute_scheduled_call(
        &mut self,
        execution: &ScheduledExecution,
    ) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "Executing scheduled call: contract={}, chunk={}, max_gas={}, offer={}",
                execution.contract, execution.chunk_id, execution.max_gas, execution.offer_amount
            );
        }

        // Load contract module
        let contract_data = self
            .inner
            .storage
            .get_contract_at_maximum_topoheight_for(&execution.contract, self.inner.topoheight)
            .await?;

        let bytecode = match contract_data {
            Some((_, versioned_contract)) => {
                let module = versioned_contract
                    .get()
                    .as_ref()
                    .ok_or_else(|| BlockchainError::ContractNotFound(execution.contract.clone()))?;
                module
                    .get_bytecode()
                    .ok_or_else(|| {
                        BlockchainError::ModuleError("Contract does not have bytecode".to_string())
                    })?
                    .to_vec()
            }
            None => {
                return Err(BlockchainError::ContractNotFound(
                    execution.contract.clone(),
                ));
            }
        };

        // Get block info for execution context
        // Convert timestamp from milliseconds to seconds for contract execution
        let block_timestamp = self.block.get_timestamp() / 1000;
        let block_height = self.block.get_height();

        // Get mutable storage as ContractProvider for executor
        // StorageReference<S> implements DerefMut to S, and S: Storage implies S: ContractInfoProvider
        let provider: &mut (dyn ContractInfoProvider + Send) = self.inner.storage.as_mut();

        // Execute using the contract executor
        let result = self
            .executor
            .execute(
                &bytecode,
                provider,
                self.inner.topoheight,
                &execution.contract,
                self.block_hash,
                block_height,
                block_timestamp,
                &execution.hash, // Use execution hash as "tx_hash" for scheduled calls
                &execution.scheduler_contract,
                execution.max_gas,
                Some(execution.input_data.clone()),
            )
            .await
            .map_err(BlockchainError::Any)?;

        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "Scheduled execution {} completed: gas_used={}, exit_code={:?}",
                execution.hash, result.gas_used, result.exit_code
            );
        }

        Ok(result.gas_used)
    }

    /// Refund unused gas to a contract's balance.
    async fn refund_gas_to_contract(
        &mut self,
        contract: &Hash,
        amount: u64,
    ) -> Result<(), BlockchainError> {
        if amount == 0 {
            return Ok(());
        }

        // Get current contract balance for native asset (Hash::zero())
        let native_asset = Hash::zero();
        let current_balance = self
            .inner
            .storage
            .get_contract_balance_at_maximum_topoheight(
                contract,
                &native_asset,
                self.inner.topoheight,
            )
            .await?
            .map(|(_, balance)| balance.take())
            .unwrap_or(0);

        // Add refund to balance
        let new_balance = current_balance.saturating_add(amount);

        // Update balance using versioned storage
        let versioned_balance =
            VersionedContractBalance::new(new_balance, Some(self.inner.topoheight));
        self.inner
            .storage
            .set_last_contract_balance_to(
                contract,
                &native_asset,
                self.inner.topoheight,
                versioned_balance,
            )
            .await?;

        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "Refunded {} gas to contract {}: {} -> {}",
                amount,
                contract,
                current_balance,
                new_balance
            );
        }

        Ok(())
    }
}
