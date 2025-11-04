mod apply;
mod storage;

use crate::core::{error::BlockchainError, storage::Storage};
use async_trait::async_trait;
use log::{debug, trace};
use std::{
    borrow::Cow,
    collections::{hash_map::Entry, HashMap},
};
use tos_common::{
    account::{Nonce, VersionedBalance, VersionedNonce},
    block::{BlockVersion, TopoHeight},
    config::TOS_ASSET,
    crypto::{elgamal::CompressedPublicKey, Hash, PublicKey},
    transaction::{verify::BlockchainVerificationState, MultiSigPayload, Reference, Transaction},
    utils::format_tos,
    versioned_type::VersionedState,
};
use tos_environment::Environment;
use tos_vm::Module;

pub use apply::*;
pub use storage::*;

// Sender changes
// This contains its expected next balance for next outgoing transactions
// But also contains the ciphertext changes happening (so a sum of each spendings for transactions)
// This is necessary to easily build the final user balance
struct Echange {
    // If we are allowed to use the output balance for verification
    allow_output_balance: bool,
    // if the versioned balance below is new for the current topoheight
    new_version: bool,
    // Version balance of the account used for the verification
    version: VersionedBalance,
    // Sum of all transactions output
    output_sum: u64,
    // If we used the output balance or not
    output_balance_used: bool,
}

impl Echange {
    fn new(allow_output_balance: bool, new_version: bool, version: VersionedBalance) -> Self {
        Self {
            allow_output_balance,
            new_version,
            version,
            output_sum: 0,
            output_balance_used: false,
        }
    }

    // Get the right balance to use for TX verification
    // TODO we may need to check previous balances and up to the last output balance made
    // So if in block A we spent TX A, and block B we got some funds, then we spent TX B in block C
    // We are still able to use it even if it was built at same time as TX A
    fn get_balance(&mut self) -> &mut u64 {
        let output = self.output_balance_used || self.allow_output_balance;
        let (balance, used) = self.version.select_balance(output);
        if !self.output_balance_used {
            self.output_balance_used = used;
        }
        balance
    }

    // Add a change to the account
    fn add_output_to_sum(&mut self, output: u64) {
        self.output_sum = self.output_sum.saturating_add(output);
    }
}

struct Account<'a> {
    // Account nonce used to verify valid transaction
    nonce: VersionedNonce,
    // Assets ready as source for any transfer/transaction
    // TODO: they must store also the ciphertext change
    // It will be added by next change at each TX
    // This is necessary to easily build the final user balance
    assets: HashMap<&'a Hash, Echange>,
    // Multisig configured
    // This is used to verify the validity of the multisig setup
    multisig: Option<(VersionedState, Option<MultiSigPayload>)>,
}

// This struct is used to verify the transactions executed at a snapshot of the blockchain
// It is read-only but write in memory the changes to the balances and nonces
// Once the verification is done, the changes are written to the storage
pub struct ChainState<'a, S: Storage> {
    // Storage to read and write the balances and nonces
    storage: StorageReference<'a, S>,
    environment: &'a Environment,
    // Balances of the receiver accounts
    receiver_balances: HashMap<Cow<'a, PublicKey>, HashMap<Cow<'a, Hash>, VersionedBalance>>,
    // Sender accounts
    // This is used to verify ZK Proofs and store/update nonces
    accounts: HashMap<&'a PublicKey, Account<'a>>,
    // Current stable topoheight of the snapshot
    stable_topoheight: TopoHeight,
    // Current topoheight of the snapshot
    topoheight: TopoHeight,
    // All contracts updated
    contracts: HashMap<Hash, (VersionedState, Option<Cow<'a, Module>>)>,
    // Block header version
    block_version: BlockVersion,
    // All gas fees tracked
    gas_fee: u64,
}

impl<'a, S: Storage> ChainState<'a, S> {
    fn with(
        storage: StorageReference<'a, S>,
        environment: &'a Environment,
        stable_topoheight: TopoHeight,
        topoheight: TopoHeight,
        block_version: BlockVersion,
    ) -> Self {
        Self {
            storage,
            environment,
            receiver_balances: HashMap::new(),
            accounts: HashMap::new(),
            stable_topoheight,
            topoheight,
            contracts: HashMap::new(),
            block_version,
            gas_fee: 0,
        }
    }

    pub fn new(
        storage: &'a S,
        environment: &'a Environment,
        stable_topoheight: TopoHeight,
        topoheight: TopoHeight,
        block_version: BlockVersion,
    ) -> Self {
        Self::with(
            StorageReference::Immutable(storage),
            environment,
            stable_topoheight,
            topoheight,
            block_version,
        )
    }

    // Get all the gas fees
    pub fn get_gas_fee(&self) -> u64 {
        self.gas_fee
    }

    // Get the storage used by the chain state
    pub fn get_storage(&self) -> &S {
        self.storage.as_ref()
    }

    pub fn get_sender_balances<'b>(
        &'b self,
        key: &'b PublicKey,
    ) -> Option<HashMap<&'b Hash, &'b VersionedBalance>> {
        let account = self.accounts.get(key)?;
        Some(
            account
                .assets
                .iter()
                .map(|(k, v)| (*k, &v.version))
                .collect(),
        )
    }

    // Create a sender echange
    async fn create_sender_echange(
        storage: &S,
        key: &'a PublicKey,
        asset: &'a Hash,
        current_topoheight: TopoHeight,
        reference: &Reference,
    ) -> Result<Echange, BlockchainError> {
        let (use_output_balance, new_version, version) =
            super::search_versioned_balance_for_reference(
                storage,
                key,
                asset,
                current_topoheight,
                reference,
                true,
            )
            .await?;
        Ok(Echange::new(use_output_balance, new_version, version))
    }

    // Create a sender account by fetching its nonce and create a empty HashMap for balances,
    // those will be fetched lazily
    async fn create_sender_account(
        key: &PublicKey,
        storage: &S,
        topoheight: TopoHeight,
        receiver_balances: &HashMap<Cow<'a, PublicKey>, HashMap<Cow<'a, Hash>, VersionedBalance>>,
    ) -> Result<Account<'a>, BlockchainError> {
        use log::{debug, trace};

        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "create_sender_account for {} at topoheight {}",
                key.as_address(storage.is_mainnet()),
                topoheight
            );
        }

        // Try to get nonce at maximum topoheight
        let nonce_result = storage
            .get_nonce_at_maximum_topoheight(key, topoheight)
            .await?;
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "Step 1: get_nonce_at_maximum_topoheight returned: {:?}",
                nonce_result.as_ref().map(|(topo, _)| topo)
            );
        }

        if let Some((topo, mut version)) = nonce_result {
            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "Found nonce for {} at topoheight {} (current: {})",
                    key.as_address(storage.is_mainnet()),
                    topo,
                    topoheight
                );
            }
            version.set_previous_topoheight(Some(topo));

            let multisig = storage
                .get_multisig_at_maximum_topoheight_for(key, topoheight)
                .await?
                .map(|(topo, multisig)| {
                    multisig
                        .take()
                        .map(|m| (VersionedState::FetchedAt(topo), Some(m.into_owned())))
                })
                .flatten();

            return Ok(Account {
                nonce: version,
                assets: HashMap::new(),
                multisig,
            });
        }

        // If nonce not found, check if account is being registered in this block's receiver_balances
        // This handles DAG concurrency where registration is pending but not yet in storage
        let in_receiver_balances = receiver_balances.contains_key(key);
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "Step 2: receiver_balances contains key: {}",
                in_receiver_balances
            );
        }

        if in_receiver_balances {
            use tos_common::account::VersionedNonce;
            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "Account {} found in receiver_balances, creating with nonce=0",
                    key.as_address(storage.is_mainnet())
                );
            }

            let multisig = storage
                .get_multisig_at_maximum_topoheight_for(key, topoheight)
                .await?
                .map(|(topo, multisig)| {
                    multisig
                        .take()
                        .map(|m| (VersionedState::FetchedAt(topo), Some(m.into_owned())))
                })
                .flatten();

            return Ok(Account {
                nonce: VersionedNonce::new(0, None), // Default nonce = 0, no previous topoheight
                assets: HashMap::new(),
                multisig,
            });
        }

        // Step 3: Scan backwards from current topoheight to find any existing nonce (bypass snapshot)
        // This handles the case where storage snapshot (at base_topo_height) is older than when account nonce was set
        // Snapshot might have account.nonce_pointer = None, but RocksDB VersionedNonces column has the data
        // We scan backwards from topoheight to find the most recent nonce for this account
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "Step 3: Scanning backwards from topoheight {} to find nonce (bypass snapshot)",
                topoheight
            );
        }

        // Scan backwards from topoheight to 0 to find most recent nonce
        // Limit scan to last 1000 topoheights for performance (in practice nonce should be found quickly)
        let scan_start = topoheight.saturating_sub(1000);
        for scan_topo in (scan_start..=topoheight).rev() {
            if storage
                .has_nonce_at_exact_topoheight(key, scan_topo)
                .await?
            {
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "Step 3: Found nonce at topoheight {} (bypass snapshot, current block: {})",
                        scan_topo, topoheight
                    );
                }

                let mut version = storage
                    .get_nonce_at_exact_topoheight(key, scan_topo)
                    .await?;
                version.set_previous_topoheight(Some(scan_topo));

                let multisig = storage
                    .get_multisig_at_maximum_topoheight_for(key, topoheight)
                    .await?
                    .map(|(topo, multisig)| {
                        multisig
                            .take()
                            .map(|m| (VersionedState::FetchedAt(topo), Some(m.into_owned())))
                    })
                    .flatten();

                return Ok(Account {
                    nonce: version,
                    assets: HashMap::new(),
                    multisig,
                });
            }
        }

        // Step 4: If nonce not found, check if account is registered in storage but has default nonce (0)
        // This handles cases where account was registered in a previous block but hasn't sent any tx yet
        let is_registered = storage
            .is_account_registered_for_topoheight(key, topoheight)
            .await?;
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "Step 4: is_account_registered_for_topoheight returned: {}",
                is_registered
            );
        }

        if is_registered {
            use tos_common::account::VersionedNonce;
            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "Account {} is registered but no nonce, creating with nonce=0",
                    key.as_address(storage.is_mainnet())
                );
            }

            let multisig = storage
                .get_multisig_at_maximum_topoheight_for(key, topoheight)
                .await?
                .map(|(topo, multisig)| {
                    multisig
                        .take()
                        .map(|m| (VersionedState::FetchedAt(topo), Some(m.into_owned())))
                })
                .flatten();

            return Ok(Account {
                nonce: VersionedNonce::new(0, None), // Default nonce = 0, no previous topoheight
                assets: HashMap::new(),
                multisig,
            });
        }

        // Account truly does not exist
        if log::log_enabled!(log::Level::Debug) {
            debug!("Account {} not found: all checks failed (nonce query, receiver_balances, registration)",
                   key.as_address(storage.is_mainnet()));
        }
        Err(BlockchainError::AccountNotFound(
            key.as_address(storage.is_mainnet()),
        ))
    }

    // Retrieve the receiver balance of an account
    // This is mostly the final balance where everything is added (outputs and inputs)
    async fn internal_get_receiver_balance<'b>(
        &'b mut self,
        key: Cow<'a, PublicKey>,
        asset: Cow<'a, Hash>,
    ) -> Result<&'b mut u64, BlockchainError> {
        match self
            .receiver_balances
            .entry(key.clone())
            .or_insert_with(HashMap::new)
            .entry(asset.clone())
        {
            Entry::Occupied(o) => Ok(o.into_mut().get_mut_balance()),
            Entry::Vacant(e) => {
                let (version, _) = self
                    .storage
                    .get_new_versioned_balance(&key, &asset, self.topoheight)
                    .await?;
                Ok(e.insert(version).get_mut_balance())
            }
        }
    }

    // Retrieve the sender balance of an account
    // This is used for TX outputs verification
    // This depends on the transaction and can be final balance or output balance
    async fn internal_get_sender_verification_balance<'b>(
        &'b mut self,
        key: &'a PublicKey,
        asset: &'a Hash,
        reference: &Reference,
    ) -> Result<&'b mut u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "getting sender verification balance for {} at topoheight {}, reference: {}",
                key.as_address(self.storage.is_mainnet()),
                self.topoheight,
                reference.topoheight
            );
        }
        match self.accounts.entry(key) {
            Entry::Occupied(o) => {
                let account = o.into_mut();
                match account.assets.entry(asset) {
                    Entry::Occupied(o) => Ok(o.into_mut().get_balance()),
                    Entry::Vacant(e) => {
                        let echange = Self::create_sender_echange(
                            &self.storage,
                            key,
                            asset,
                            self.topoheight,
                            reference,
                        )
                        .await?;
                        Ok(e.insert(echange).get_balance())
                    }
                }
            }
            Entry::Vacant(e) => {
                // Create a new account for the sender
                let account = Self::create_sender_account(
                    key,
                    &self.storage,
                    self.topoheight,
                    &self.receiver_balances,
                )
                .await?;

                // Create a new echange for the asset
                let echange = Self::create_sender_echange(
                    &self.storage,
                    key,
                    asset,
                    self.topoheight,
                    reference,
                )
                .await?;

                Ok(e.insert(account)
                    .assets
                    .entry(asset)
                    .or_insert(echange)
                    .get_balance())
            }
        }
    }

    // Update the output echanges of an account
    // Account must have been fetched before calling this function
    // If the asset echange doesn't exist yet, it will be loaded from storage
    async fn internal_update_sender_echange(
        &mut self,
        key: &'a PublicKey,
        asset: &'a Hash,
        new_amount: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "update sender echange for asset {} with amount {}",
                asset,
                new_amount
            );
        }

        // Check if the account exists
        let account = self.accounts.get_mut(key).ok_or_else(|| {
            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "Account {} not found in accounts HashMap when updating sender echange",
                    key.as_address(self.storage.is_mainnet())
                );
            }
            BlockchainError::NoTxSender(key.as_address(self.storage.is_mainnet()))
        })?;

        // If the asset echange doesn't exist, we need to load it from storage first
        // This handles the case where create_sender_account was called (which creates account with empty assets HashMap)
        // but the transaction execution requires updating the sender's asset balance
        if !account.assets.contains_key(asset) {
            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "Asset {} not found in account {}, loading from storage",
                    asset,
                    key.as_address(self.storage.is_mainnet())
                );
            }

            // We need the reference to create the echange
            // Since we're in the middle of transaction execution and don't have access to the reference here,
            // this is a bug - the asset should have been loaded by calling get_sender_verification_balance first
            // For now, return a clear error message
            return Err(BlockchainError::NoTxSender(
                key.as_address(self.storage.is_mainnet()),
            ));
        }

        let change = account.assets.get_mut(asset).ok_or_else(|| {
            BlockchainError::NoTxSender(key.as_address(self.storage.is_mainnet()))
        })?;

        // Increase the total output
        change.add_output_to_sum(new_amount);

        Ok(())
    }

    // Get or create account for sender
    async fn get_internal_account(
        &mut self,
        key: &'a PublicKey,
    ) -> Result<&mut Account<'a>, BlockchainError> {
        match self.accounts.entry(key) {
            Entry::Occupied(o) => Ok(o.into_mut()),
            Entry::Vacant(e) => {
                let account = Self::create_sender_account(
                    key,
                    &self.storage,
                    self.topoheight,
                    &self.receiver_balances,
                )
                .await?;
                Ok(e.insert(account))
            }
        }
    }

    // Retrieve the account nonce
    // Only sender accounts should be used here
    async fn internal_get_account_nonce(
        &mut self,
        key: &'a PublicKey,
    ) -> Result<Nonce, BlockchainError> {
        self.get_internal_account(key)
            .await
            .map(|a| a.nonce.get_nonce())
    }

    // Update the account nonce
    // Only sender accounts should be used here
    // For each TX, we must update the nonce by one
    async fn internal_update_account_nonce(
        &mut self,
        account: &'a PublicKey,
        new_nonce: Nonce,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "Updating nonce for {} to {} at topoheight {}",
                account.as_address(self.storage.is_mainnet()),
                new_nonce,
                self.topoheight
            );
        }
        let account = self.get_internal_account(account).await?;
        account.nonce.set_nonce(new_nonce);
        Ok(())
    }

    // Search for a contract versioned state
    // if not found, fetch it from the storage
    // if not found in storage, create a new one
    async fn internal_get_versioned_contract(
        &mut self,
        hash: &Hash,
    ) -> Result<&mut (VersionedState, Option<Cow<'a, Module>>), BlockchainError> {
        // Use Entry API for efficient lookup/insert (no memory leak!)
        match self.contracts.entry(hash.clone()) {
            Entry::Occupied(entry) => Ok(entry.into_mut()),
            Entry::Vacant(entry) => {
                let contract = self
                    .storage
                    .get_contract_at_maximum_topoheight_for(hash, self.topoheight)
                    .await?
                    .map(|(topo, contract)| (VersionedState::FetchedAt(topo), contract.take()))
                    .unwrap_or((VersionedState::New, None));

                Ok(entry.insert(contract))
            }
        }
    }

    // Load a contract from the storage if its not already loaded
    async fn load_versioned_contract(&mut self, hash: &Hash) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "Loading contract {} at topoheight {}",
                hash,
                self.topoheight
            );
        }

        // First check if already exists
        if let Some((_, module_opt)) = self.contracts.get(hash) {
            return Ok(module_opt.is_some());
        }

        // Not found, load from storage and insert (no memory leak!)
        let contract = self
            .storage
            .get_contract_at_maximum_topoheight_for(hash, self.topoheight)
            .await?
            .map(|(topo, contract)| (VersionedState::FetchedAt(topo), contract.take()))
            .unwrap_or((VersionedState::New, None));

        let has_module = contract.1.is_some();
        self.contracts.insert(hash.clone(), contract);
        Ok(has_module)
    }

    // Get the contract module from our cache
    async fn internal_get_contract_module(&self, hash: &Hash) -> Result<&Module, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("Getting contract module {}", hash);
        }
        self.contracts
            .get(hash)
            .ok_or_else(|| BlockchainError::ContractNotFound(hash.clone()))
            .and_then(|(_, module)| {
                module
                    .as_ref()
                    .map(|m| m.as_ref())
                    .ok_or_else(|| BlockchainError::ContractNotFound(hash.clone()))
            })
    }

    // Reward a miner for the block mined
    pub async fn reward_miner(
        &mut self,
        miner: &'a PublicKey,
        reward: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "Rewarding miner {} with {} TOS at topoheight {}",
                miner.as_address(self.storage.is_mainnet()),
                format_tos(reward),
                self.topoheight
            );
        }
        let miner_balance = self
            .internal_get_receiver_balance(Cow::Borrowed(miner), Cow::Borrowed(&TOS_ASSET))
            .await?;
        *miner_balance = miner_balance
            .checked_add(reward)
            .ok_or(BlockchainError::Overflow)?;

        Ok(())
    }

    /// Add balance to a receiver account
    /// Used to add additional rewards (fees and gas_fee) to the miner after transaction execution
    pub async fn add_receiver_balance(
        &mut self,
        key: &'a PublicKey,
        asset: &'a Hash,
        amount: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "Adding {} to receiver {} for asset {} at topoheight {}",
                amount,
                key.as_address(self.storage.is_mainnet()),
                asset,
                self.topoheight
            );
        }
        let balance = self
            .internal_get_receiver_balance(Cow::Borrowed(key), Cow::Borrowed(asset))
            .await?;
        *balance = balance
            .checked_add(amount)
            .ok_or(BlockchainError::Overflow)?;
        Ok(())
    }
}

#[async_trait]
impl<'a, S: Storage> BlockchainVerificationState<'a, BlockchainError> for ChainState<'a, S> {
    /// Verify the TX version and reference
    async fn pre_verify_tx<'b>(&'b mut self, tx: &Transaction) -> Result<(), BlockchainError> {
        super::pre_verify_tx(
            self.storage.as_ref(),
            tx,
            self.stable_topoheight,
            self.topoheight,
            self.get_block_version(),
        )
        .await
    }

    /// Get the balance for a receiver account
    async fn get_receiver_balance<'b>(
        &'b mut self,
        account: Cow<'a, PublicKey>,
        asset: Cow<'a, Hash>,
    ) -> Result<&'b mut u64, BlockchainError> {
        let balance = self.internal_get_receiver_balance(account, asset).await?;
        Ok(balance)
    }

    /// Get the balance for a sender account
    async fn get_sender_balance<'b>(
        &'b mut self,
        account: &'a PublicKey,
        asset: &'a Hash,
        reference: &Reference,
    ) -> Result<&'b mut u64, BlockchainError> {
        Ok(self
            .internal_get_sender_verification_balance(account, asset, reference)
            .await?)
    }

    /// Apply new output to a sender account
    async fn add_sender_output(
        &mut self,
        account: &'a PublicKey,
        asset: &'a Hash,
        output: u64,
    ) -> Result<(), BlockchainError> {
        self.internal_update_sender_echange(account, asset, output)
            .await
    }

    /// Get the nonce of an account
    async fn get_account_nonce(
        &mut self,
        account: &'a PublicKey,
    ) -> Result<Nonce, BlockchainError> {
        self.internal_get_account_nonce(account).await
    }

    /// Apply a new nonce to an account
    async fn update_account_nonce(
        &mut self,
        account: &'a PublicKey,
        new_nonce: Nonce,
    ) -> Result<(), BlockchainError> {
        self.internal_update_account_nonce(account, new_nonce).await
    }

    /// SECURITY FIX V-11: Atomic compare-and-swap for nonce updates
    /// Returns true if the nonce matched expected value and was updated
    /// Returns false if the current nonce didn't match expected value
    async fn compare_and_swap_nonce(
        &mut self,
        account: &'a CompressedPublicKey,
        expected: Nonce,
        new_value: Nonce,
    ) -> Result<bool, BlockchainError> {
        // For chain state, we don't have true atomicity
        // Protection happens at blockchain level with per-account locks
        let current = self.get_account_nonce(account).await?;
        if current == expected {
            self.update_account_nonce(account, new_value).await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Get the block version
    fn get_block_version(&self) -> BlockVersion {
        self.block_version
    }

    /// Set the multisig state for an account
    async fn set_multisig_state(
        &mut self,
        account: &'a PublicKey,
        payload: &MultiSigPayload,
    ) -> Result<(), BlockchainError> {
        let account = self.get_internal_account(account).await?;
        if let Some((state, multisig)) = account.multisig.as_mut() {
            state.mark_updated();
            *multisig = if payload.is_delete() {
                None
            } else {
                Some(payload.clone())
            };
        } else {
            let multisig = if payload.is_delete() {
                None
            } else {
                Some(payload.clone())
            };
            account.multisig = Some((VersionedState::New, multisig));
        }

        Ok(())
    }

    /// Get the multisig state for an account
    /// If the account is not a multisig account, return None
    async fn get_multisig_state(
        &mut self,
        account: &'a PublicKey,
    ) -> Result<Option<&MultiSigPayload>, BlockchainError> {
        let account = self.get_internal_account(account).await?;
        Ok(account
            .multisig
            .as_ref()
            .and_then(|(_, multisig)| multisig.as_ref()))
    }

    /// Get the contract environment
    async fn get_environment(&mut self) -> Result<&Environment, BlockchainError> {
        Ok(self.environment)
    }

    /// Set the contract module
    async fn set_contract_module(
        &mut self,
        hash: &Hash,
        module: &'a Module,
    ) -> Result<(), BlockchainError> {
        let (state, m) = self.internal_get_versioned_contract(&hash).await?;
        if !state.is_new() {
            return Err(BlockchainError::ContractAlreadyExists);
        }

        state.mark_updated();
        *m = Some(Cow::Borrowed(module));

        Ok(())
    }

    async fn load_contract_module(&mut self, hash: &Hash) -> Result<bool, BlockchainError> {
        self.load_versioned_contract(hash).await
    }

    /// Get the contract module with the environment
    async fn get_contract_module_with_environment(
        &self,
        hash: &Hash,
    ) -> Result<(&Module, &Environment), BlockchainError> {
        let module = self.internal_get_contract_module(hash).await?;
        Ok((module, self.environment))
    }
}
