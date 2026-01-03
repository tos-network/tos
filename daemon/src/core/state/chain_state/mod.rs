mod apply;
mod storage;

use crate::core::{error::BlockchainError, storage::Storage};
use async_trait::async_trait;
use log::{debug, trace};
use std::{
    borrow::Cow,
    collections::{hash_map::Entry, HashMap, HashSet},
};
use tos_common::{
    account::{Nonce, VersionedBalance, VersionedNonce, VersionedUnoBalance},
    block::{BlockVersion, TopoHeight},
    config::{TOS_ASSET, UNO_ASSET},
    crypto::{
        elgamal::{Ciphertext, CompressedPublicKey},
        Hash, PublicKey,
    },
    transaction::{verify::BlockchainVerificationState, MultiSigPayload, Reference, Transaction},
    utils::format_tos,
    versioned_type::VersionedState,
};
use tos_environment::Environment;
use tos_kernel::Module;

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

// UNO (encrypted) sender changes
// This tracks encrypted balance changes for privacy-preserving transactions
struct UnoEchange {
    // If we are allowed to use the output balance for verification
    allow_output_balance: bool,
    // if the versioned balance below is new for the current topoheight
    // (used during storage finalization, matches plaintext Echange pattern)
    #[allow(dead_code)]
    new_version: bool,
    // Version balance of the account used for the verification
    version: VersionedUnoBalance,
    // Sum of all transactions output (encrypted ciphertext)
    output_sum: Ciphertext,
    // If we used the output balance or not
    output_balance_used: bool,
}

impl UnoEchange {
    fn new(allow_output_balance: bool, new_version: bool, version: VersionedUnoBalance) -> Self {
        Self {
            allow_output_balance,
            new_version,
            version,
            output_sum: Ciphertext::zero(),
            output_balance_used: false,
        }
    }

    // Get the right balance to use for TX verification
    // Returns a decompressed Ciphertext for ZK proof operations
    fn get_balance(&mut self) -> Result<&mut Ciphertext, BlockchainError> {
        let output = self.output_balance_used || self.allow_output_balance;
        let (cache, used) = self.version.select_balance(output);
        if !self.output_balance_used {
            self.output_balance_used = used;
        }
        // Decompress the ciphertext for computation
        cache.computable().map_err(BlockchainError::from)
    }

    // Add a change to the account
    fn add_output_to_sum(&mut self, output: Ciphertext) {
        self.output_sum += output;
    }
}

struct Account<'a> {
    // Account nonce used to verify valid transaction
    nonce: VersionedNonce,
    // Assets ready as source for any transfer/transaction
    // It will be added by next change at each TX
    // This is necessary to easily build the final user balance
    assets: HashMap<&'a Hash, Echange>,
    // UNO (encrypted) assets for privacy-preserving transactions
    uno_assets: HashMap<&'a Hash, UnoEchange>,
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
    // UNO (encrypted) balances of the receiver accounts
    receiver_uno_balances: HashMap<Cow<'a, PublicKey>, HashMap<Cow<'a, Hash>, VersionedUnoBalance>>,
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
    // Block timestamp for deterministic verification (in seconds)
    // None for mempool verification (uses system time)
    block_timestamp: Option<u64>,
    // Pending registrations: accounts that will be registered by earlier TXs in this block
    // Enables same-block visibility for fee calculation
    pending_registrations: HashSet<CompressedPublicKey>,
}

impl<'a, S: Storage> ChainState<'a, S> {
    fn with(
        storage: StorageReference<'a, S>,
        environment: &'a Environment,
        stable_topoheight: TopoHeight,
        topoheight: TopoHeight,
        block_version: BlockVersion,
        block_timestamp: Option<u64>,
    ) -> Self {
        Self {
            storage,
            environment,
            receiver_balances: HashMap::new(),
            receiver_uno_balances: HashMap::new(),
            accounts: HashMap::new(),
            stable_topoheight,
            topoheight,
            contracts: HashMap::new(),
            block_version,
            gas_fee: 0,
            block_timestamp,
            pending_registrations: HashSet::new(),
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
            None, // No block timestamp for mempool verification
        )
    }

    /// Create a new ChainState with block timestamp for deterministic consensus validation
    ///
    /// Use this when verifying transactions during block validation.
    /// The block_timestamp_secs should be the block's timestamp in seconds.
    pub fn new_with_timestamp(
        storage: &'a S,
        environment: &'a Environment,
        stable_topoheight: TopoHeight,
        topoheight: TopoHeight,
        block_version: BlockVersion,
        block_timestamp_secs: u64,
    ) -> Self {
        Self::with(
            StorageReference::Immutable(storage),
            environment,
            stable_topoheight,
            topoheight,
            block_version,
            Some(block_timestamp_secs),
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
                uno_assets: HashMap::new(),
                multisig,
            });
        }

        // If nonce not found, check if account is being registered in this block's receiver_balances
        // This handles DAG concurrency where registration is pending but not yet in storage
        let in_receiver_balances = receiver_balances.contains_key(key);

        if in_receiver_balances {
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
                nonce: VersionedNonce::new(0, None), // Default nonce = 0
                assets: HashMap::new(),
                uno_assets: HashMap::new(),
                multisig,
            });
        }

        // Scan backwards from topoheight to find most recent nonce
        let scan_start = topoheight.saturating_sub(1000);
        for scan_topo in (scan_start..=topoheight).rev() {
            if storage
                .has_nonce_at_exact_topoheight(key, scan_topo)
                .await?
            {
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "Found nonce at topoheight {} (bypass snapshot, current block: {})",
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
                    uno_assets: HashMap::new(),
                    multisig,
                });
            }
        }

        // Check if account is registered but has default nonce (0)
        let is_registered = storage
            .is_account_registered_for_topoheight(key, topoheight)
            .await?;

        if is_registered {
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
                nonce: VersionedNonce::new(0, None),
                assets: HashMap::new(),
                uno_assets: HashMap::new(),
                multisig,
            });
        }

        // Account truly does not exist
        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "Account {} not found: all checks failed",
                key.as_address(storage.is_mainnet())
            );
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
    async fn internal_update_sender_echange(
        &mut self,
        key: &'a PublicKey,
        asset: &'a Hash,
        new_ct: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("update sender echange: {}", new_ct);
        }
        let change = self
            .accounts
            .get_mut(key)
            .and_then(|a| a.assets.get_mut(asset))
            .ok_or_else(|| {
                BlockchainError::NoTxSender(key.as_address(self.storage.is_mainnet()))
            })?;

        // Increase the total output
        change.add_output_to_sum(new_ct);

        Ok(())
    }

    // ===== UNO (Privacy Balance) Internal Methods =====

    // Retrieve the UNO receiver balance of an account
    async fn internal_get_receiver_uno_balance<'b>(
        &'b mut self,
        key: Cow<'a, PublicKey>,
        asset: Cow<'a, Hash>,
    ) -> Result<&'b mut Ciphertext, BlockchainError> {
        match self
            .receiver_uno_balances
            .entry(key.clone())
            .or_insert_with(HashMap::new)
            .entry(asset.clone())
        {
            Entry::Occupied(o) => {
                // Decompress for computation
                o.into_mut()
                    .get_mut_balance()
                    .computable()
                    .map_err(BlockchainError::from)
            }
            Entry::Vacant(e) => {
                let (version, _) = self
                    .storage
                    .get_new_versioned_uno_balance(&key, &asset, self.topoheight)
                    .await?;
                // Decompress for computation
                e.insert(version)
                    .get_mut_balance()
                    .computable()
                    .map_err(BlockchainError::from)
            }
        }
    }

    // Create a UNO echange for a sender account
    async fn create_sender_uno_echange(
        storage: &StorageReference<'a, S>,
        key: &PublicKey,
        asset: &Hash,
        topoheight: TopoHeight,
        reference: &Reference,
    ) -> Result<UnoEchange, BlockchainError> {
        // Check if we should use output balance (based on reference topoheight)
        let allow_output_balance = reference.topoheight < topoheight;

        // Get the versioned UNO balance from storage
        let (version, new_version) = storage
            .get_new_versioned_uno_balance(key, asset, topoheight)
            .await?;

        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "create_sender_uno_echange: key={}, asset={}, topoheight={}, allow_output={}, new_version={}",
                key.as_address(storage.is_mainnet()),
                asset,
                topoheight,
                allow_output_balance,
                new_version
            );
        }

        Ok(UnoEchange::new(allow_output_balance, new_version, version))
    }

    // Retrieve the UNO sender balance of an account for verification
    async fn internal_get_sender_uno_balance<'b>(
        &'b mut self,
        key: &'a PublicKey,
        asset: &'a Hash,
        reference: &Reference,
    ) -> Result<&'b mut Ciphertext, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "getting sender UNO balance for {} at topoheight {}, reference: {}",
                key.as_address(self.storage.is_mainnet()),
                self.topoheight,
                reference.topoheight
            );
        }
        // First check if this account has received UNO in this block (receiver_uno_balances)
        // This handles the case where Shield and Unshield are in the same block template
        let receiver_uno = self
            .receiver_uno_balances
            .get(key)
            .and_then(|assets| assets.get(asset))
            .map(|v| v.clone());

        match self.accounts.entry(key) {
            Entry::Occupied(o) => {
                let account = o.into_mut();
                match account.uno_assets.entry(asset) {
                    Entry::Occupied(o) => o.into_mut().get_balance(),
                    Entry::Vacant(e) => {
                        // Check receiver_uno_balances first, then storage
                        let echange = if let Some(receiver_version) = receiver_uno {
                            if log::log_enabled!(log::Level::Trace) {
                                trace!(
                                    "Using receiver UNO balance for sender {} (same block Shield/Unshield)",
                                    key.as_address(self.storage.is_mainnet())
                                );
                            }
                            // Create UnoEchange from receiver balance
                            let allow_output_balance = reference.topoheight < self.topoheight;
                            UnoEchange::new(allow_output_balance, false, receiver_version)
                        } else {
                            Self::create_sender_uno_echange(
                                &self.storage,
                                key,
                                asset,
                                self.topoheight,
                                reference,
                            )
                            .await?
                        };
                        e.insert(echange).get_balance()
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

                // Check receiver_uno_balances first, then storage
                let echange = if let Some(receiver_version) = receiver_uno {
                    if log::log_enabled!(log::Level::Trace) {
                        trace!(
                            "Using receiver UNO balance for new sender account {} (same block Shield/Unshield)",
                            key.as_address(self.storage.is_mainnet())
                        );
                    }
                    // Create UnoEchange from receiver balance
                    let allow_output_balance = reference.topoheight < self.topoheight;
                    UnoEchange::new(allow_output_balance, false, receiver_version)
                } else {
                    Self::create_sender_uno_echange(
                        &self.storage,
                        key,
                        asset,
                        self.topoheight,
                        reference,
                    )
                    .await?
                };

                e.insert(account)
                    .uno_assets
                    .entry(asset)
                    .or_insert(echange)
                    .get_balance()
            }
        }
    }

    // Update the UNO output echanges of an account
    async fn internal_update_sender_uno_echange(
        &mut self,
        key: &'a PublicKey,
        asset: &'a Hash,
        output_ct: Ciphertext,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "update sender UNO echange for {}",
                key.as_address(self.storage.is_mainnet())
            );
        }
        let change = self
            .accounts
            .get_mut(key)
            .and_then(|a| a.uno_assets.get_mut(asset))
            .ok_or_else(|| {
                BlockchainError::NoTxSender(key.as_address(self.storage.is_mainnet()))
            })?;

        // Increase the total output
        change.add_output_to_sum(output_ct);

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
        // Use Entry API for efficient lookup/insert
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

        // Not found, load from storage and insert
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

    // ===== UNO (Privacy Balance) Methods =====

    /// Get the UNO (encrypted) balance for a receiver account
    async fn get_receiver_uno_balance<'b>(
        &'b mut self,
        account: Cow<'a, PublicKey>,
        asset: Cow<'a, Hash>,
    ) -> Result<&'b mut Ciphertext, BlockchainError> {
        self.internal_get_receiver_uno_balance(account, asset).await
    }

    /// Get the UNO (encrypted) balance used for verification of funds for the sender account
    async fn get_sender_uno_balance<'b>(
        &'b mut self,
        account: &'a PublicKey,
        asset: &'a Hash,
        reference: &Reference,
    ) -> Result<&'b mut Ciphertext, BlockchainError> {
        self.internal_get_sender_uno_balance(account, asset, reference)
            .await
    }

    /// Apply new output ciphertext to a sender's UNO account
    async fn add_sender_uno_output(
        &mut self,
        account: &'a PublicKey,
        asset: &'a Hash,
        output: Ciphertext,
    ) -> Result<(), BlockchainError> {
        self.internal_update_sender_uno_echange(account, asset, output)
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

    /// Get the timestamp to use for verification
    ///
    /// For block validation: returns the block timestamp (deterministic)
    /// For mempool verification: returns current system time
    fn get_verification_timestamp(&self) -> u64 {
        self.block_timestamp.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        })
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

    fn get_network(&self) -> tos_common::network::Network {
        self.storage
            .get_network()
            .unwrap_or(tos_common::network::Network::Mainnet)
    }

    /// Check if an account is registered (exists) on the blockchain
    ///
    /// An account is considered registered if it has ever had a nonce
    /// (i.e., has sent transactions before) or has any balance record (even zero).
    /// This is used for TOS-Only account creation fee calculation.
    ///
    /// Note: Zero-balance accounts created by ActivateAccounts are considered
    /// registered because they have a balance record in the receiver cache.
    async fn is_account_registered(
        &self,
        account: &CompressedPublicKey,
    ) -> Result<bool, BlockchainError> {
        // Note: tos_common::crypto::PublicKey = CompressedPublicKey
        // So we can use account directly

        // Check if account has nonce (has sent transactions)
        let has_nonce = self.storage.has_nonce(account).await?;
        if has_nonce {
            return Ok(true);
        }

        // Check if account has any balance record in receiver cache (any asset)
        // Note: We check for record existence, not balance > 0, because
        // ActivateAccounts creates zero-balance accounts that should be registered
        if let Some(balances) = self.receiver_balances.get(&Cow::Borrowed(account)) {
            if !balances.is_empty() {
                return Ok(true);
            }
        }

        // Check if account has any UNO balance in receiver cache (any asset)
        if self
            .receiver_uno_balances
            .contains_key(&Cow::Borrowed(account))
        {
            return Ok(true);
        }

        // Check storage for any asset balance record at current topoheight
        // Collect into Vec to avoid holding iterator across await (Send requirement)
        let assets: Vec<_> = self.storage.get_assets_for(account).await?.collect();
        for asset in assets {
            let asset = asset?;
            let balance_result = self
                .storage
                .get_balance_at_maximum_topoheight(account, &asset, self.topoheight)
                .await?;
            // Check for record existence, not balance > 0
            if balance_result.is_some() {
                return Ok(true);
            }
        }

        // Check storage for UNO balance (privacy)
        if self
            .storage
            .has_uno_balance_for(account, &UNO_ASSET)
            .await?
        {
            return Ok(true);
        }

        // Account not registered
        Ok(false)
    }

    /// Get account energy for stake 2.0 validation
    async fn get_account_energy(
        &mut self,
        account: &'a CompressedPublicKey,
    ) -> Result<Option<tos_common::account::AccountEnergy>, BlockchainError> {
        self.storage.get_account_energy(account).await
    }

    /// Get a specific delegation from one account to another
    async fn get_delegated_resource(
        &mut self,
        from: &'a CompressedPublicKey,
        to: &'a CompressedPublicKey,
    ) -> Result<Option<tos_common::account::DelegatedResource>, BlockchainError> {
        self.storage.get_delegated_resource(from, to).await
    }

    /// Record a pending undelegation (no-op for ChainState)
    ///
    /// ChainState is used during block verification where transactions
    /// are processed sequentially in the apply phase. Pending undelegation
    /// tracking is only needed for mempool verification.
    async fn record_pending_undelegation(
        &mut self,
        _from: &'a CompressedPublicKey,
        _to: &'a CompressedPublicKey,
        _amount: u64,
    ) -> Result<(), BlockchainError> {
        // No-op for block verification - apply phase handles state changes
        Ok(())
    }

    /// Check if account is pending registration in current block
    fn is_pending_registration(&self, account: &CompressedPublicKey) -> bool {
        self.pending_registrations.contains(account)
    }

    /// Record that account will be registered by an earlier TX in this block
    fn record_pending_registration(&mut self, account: &CompressedPublicKey) {
        self.pending_registrations.insert(account.clone());
    }

    /// Record pending delegation (no-op for ChainState)
    ///
    /// ChainState is used during block verification where transactions
    /// are processed sequentially. Pending delegation tracking is only
    /// needed for mempool verification.
    async fn record_pending_delegation(
        &mut self,
        _sender: &'a CompressedPublicKey,
        _amount: u64,
    ) -> Result<(), BlockchainError> {
        // No-op for block verification - apply phase handles state changes
        Ok(())
    }

    /// Get pending delegation (always 0 for ChainState)
    fn get_pending_delegation(&self, _sender: &CompressedPublicKey) -> u64 {
        0
    }

    /// Record pending unfreeze (no-op for ChainState)
    ///
    /// ChainState is used during block verification where transactions
    /// are processed sequentially. Pending unfreeze tracking is only
    /// needed for mempool verification.
    async fn record_pending_unfreeze(
        &mut self,
        _sender: &'a CompressedPublicKey,
        _amount: u64,
    ) -> Result<(), BlockchainError> {
        // No-op for block verification - apply phase handles state changes
        Ok(())
    }

    /// Get pending unfreeze count (always 0 for ChainState)
    fn get_pending_unfreeze_count(&self, _sender: &CompressedPublicKey) -> usize {
        0
    }

    /// Get pending unfreeze amount (always 0 for ChainState)
    fn get_pending_unfreeze_amount(&self, _sender: &CompressedPublicKey) -> u64 {
        0
    }

    /// Record pending energy (no-op for ChainState)
    ///
    /// ChainState is used during block verification where transactions
    /// are processed sequentially. Pending energy tracking is only
    /// needed for mempool verification.
    async fn record_pending_energy(
        &mut self,
        _sender: &'a CompressedPublicKey,
        _amount: u64,
    ) -> Result<(), BlockchainError> {
        // No-op for block verification - apply phase handles state changes
        Ok(())
    }

    /// Get pending energy (always 0 for ChainState)
    fn get_pending_energy(&self, _sender: &CompressedPublicKey) -> u64 {
        0
    }

    /// Get the global energy state (Stake 2.0)
    async fn get_global_energy_state(
        &mut self,
    ) -> Result<tos_common::account::GlobalEnergyState, BlockchainError> {
        self.storage.get_global_energy_state().await
    }
}
