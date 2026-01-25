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
    account::{
        AgentAccountMeta, EnergyResource, Nonce, SessionKey, VersionedBalance, VersionedNonce,
        VersionedUnoBalance,
    },
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

    // Get the right balance to use for TX verification.
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

struct Account {
    // Account nonce used to verify valid transaction
    nonce: VersionedNonce,
    // Assets ready as source for any transfer/transaction
    // It will be added by next change at each TX
    // This is necessary to easily build the final user balance
    assets: HashMap<Hash, Echange>,
    // UNO (encrypted) assets for privacy-preserving transactions
    uno_assets: HashMap<Hash, UnoEchange>,
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
    accounts: HashMap<Cow<'a, PublicKey>, Account>,
    // Cached energy resources
    energy_resources: HashMap<Cow<'a, PublicKey>, EnergyResource>,
    // Agent account metadata updates (None = delete)
    agent_account_meta: HashMap<Cow<'a, PublicKey>, Option<AgentAccountMeta>>,
    // Agent session key updates (None = delete)
    agent_session_keys: HashMap<(PublicKey, u64), Option<SessionKey>>,
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
            energy_resources: HashMap::new(),
            agent_account_meta: HashMap::new(),
            agent_session_keys: HashMap::new(),
            stable_topoheight,
            topoheight,
            contracts: HashMap::new(),
            block_version,
            gas_fee: 0,
            block_timestamp,
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
    ) -> Option<HashMap<Hash, &'b VersionedBalance>> {
        let account = self.accounts.get(key)?;
        Some(
            account
                .assets
                .iter()
                .map(|(k, v)| (k.clone(), &v.version))
                .collect(),
        )
    }

    // Create a sender echange
    async fn create_sender_echange(
        storage: &S,
        key: &PublicKey,
        asset: &Hash,
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
    ) -> Result<Account, BlockchainError> {
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

                let mut version = match storage.get_nonce_at_exact_topoheight(key, scan_topo).await
                {
                    Ok(version) => version,
                    Err(BlockchainError::NotFoundOnDisk(_)) => continue,
                    Err(err) => return Err(err),
                };
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

        // Fallback: account has balances but no registration/nonce pointer recorded.
        // Treat as existing account with default nonce.
        let has_balance = storage.has_balance_for(key, &TOS_ASSET).await?
            || storage.has_balance_for(key, &UNO_ASSET).await?;
        if has_balance {
            if log::log_enabled!(log::Level::Debug) {
                debug!(
                    "Account {} has balances but no registration, creating with nonce=0",
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
        key: Cow<'a, PublicKey>,
        asset: &Hash,
        reference: &Reference,
    ) -> Result<&'b mut u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "getting sender verification balance for {} at topoheight {}, reference: {}",
                key.as_ref().as_address(self.storage.is_mainnet()),
                self.topoheight,
                reference.topoheight
            );
        }
        match self.accounts.entry(key.clone()) {
            Entry::Occupied(o) => {
                let account = o.into_mut();
                match account.assets.entry(asset.clone()) {
                    Entry::Occupied(o) => Ok(o.into_mut().get_balance()),
                    Entry::Vacant(e) => {
                        let echange = Self::create_sender_echange(
                            &self.storage,
                            key.as_ref(),
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
                    key.as_ref(),
                    &self.storage,
                    self.topoheight,
                    &self.receiver_balances,
                )
                .await?;

                // Create a new echange for the asset
                let echange = Self::create_sender_echange(
                    &self.storage,
                    key.as_ref(),
                    asset,
                    self.topoheight,
                    reference,
                )
                .await?;

                Ok(e.insert(account)
                    .assets
                    .entry(asset.clone())
                    .or_insert(echange)
                    .get_balance())
            }
        }
    }

    async fn internal_get_energy_resource(
        &mut self,
        account: Cow<'a, PublicKey>,
    ) -> Result<Option<EnergyResource>, BlockchainError> {
        if let Some(resource) = self.energy_resources.get(&account) {
            return Ok(Some(resource.clone()));
        }

        let resource = self.storage.get_energy_resource(&account).await?;
        if let Some(ref energy_resource) = resource {
            self.energy_resources
                .insert(account.clone(), energy_resource.clone());
        }

        Ok(resource)
    }

    fn cache_energy_resource(
        &mut self,
        account: Cow<'a, PublicKey>,
        energy_resource: EnergyResource,
    ) {
        self.energy_resources.insert(account, energy_resource);
    }

    // Update the output echanges of an account
    // Account must have been fetched before calling this function
    async fn internal_update_sender_echange(
        &mut self,
        key: Cow<'a, PublicKey>,
        asset: &Hash,
        new_ct: u64,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("update sender echange: {}", new_ct);
        }
        let change = self
            .accounts
            .get_mut(key.as_ref())
            .and_then(|a| a.assets.get_mut(asset))
            .ok_or_else(|| {
                BlockchainError::NoTxSender(key.as_ref().as_address(self.storage.is_mainnet()))
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

        match self.accounts.entry(Cow::Borrowed(key)) {
            Entry::Occupied(o) => {
                let account = o.into_mut();
                match account.uno_assets.entry(asset.clone()) {
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
                    .entry(asset.clone())
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
    ) -> Result<&mut Account, BlockchainError> {
        match self.accounts.entry(Cow::Borrowed(key)) {
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
        account: Cow<'a, PublicKey>,
        asset: Cow<'a, Hash>,
        reference: &Reference,
    ) -> Result<&'b mut u64, BlockchainError> {
        let asset = asset.as_ref();
        Ok(self
            .internal_get_sender_verification_balance(account, asset, reference)
            .await?)
    }

    /// Apply new output to a sender account
    async fn add_sender_output(
        &mut self,
        account: Cow<'a, PublicKey>,
        asset: Cow<'a, Hash>,
        output: u64,
    ) -> Result<(), BlockchainError> {
        let asset = asset.as_ref();
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

    async fn account_exists(&mut self, account: &'a PublicKey) -> Result<bool, BlockchainError> {
        if self.receiver_balances.contains_key(account)
            || self.receiver_uno_balances.contains_key(account)
            || self.accounts.contains_key(account)
        {
            return Ok(true);
        }
        self.storage
            .is_account_registered_for_topoheight(account, self.topoheight)
            .await
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

    async fn get_agent_account_meta(
        &mut self,
        account: &'a CompressedPublicKey,
    ) -> Result<Option<AgentAccountMeta>, BlockchainError> {
        if let Some(meta) = self.agent_account_meta.get(account) {
            return Ok(meta.clone());
        }
        self.storage.get_agent_account_meta(account).await
    }

    async fn set_agent_account_meta(
        &mut self,
        account: &'a CompressedPublicKey,
        meta: &AgentAccountMeta,
    ) -> Result<(), BlockchainError> {
        self.agent_account_meta
            .insert(Cow::Borrowed(account), Some(meta.clone()));
        Ok(())
    }

    async fn delete_agent_account_meta(
        &mut self,
        account: &'a CompressedPublicKey,
    ) -> Result<(), BlockchainError> {
        self.agent_account_meta.insert(Cow::Borrowed(account), None);
        Ok(())
    }

    async fn get_session_key(
        &mut self,
        account: &'a CompressedPublicKey,
        key_id: u64,
    ) -> Result<Option<SessionKey>, BlockchainError> {
        let key = (account.clone(), key_id);
        if let Some(session_key) = self.agent_session_keys.get(&key) {
            return Ok(session_key.clone());
        }
        self.storage.get_session_key(account, key_id).await
    }

    async fn set_session_key(
        &mut self,
        account: &'a CompressedPublicKey,
        session_key: &SessionKey,
    ) -> Result<(), BlockchainError> {
        let key = (account.clone(), session_key.key_id);
        self.agent_session_keys
            .insert(key, Some(session_key.clone()));
        Ok(())
    }

    async fn delete_session_key(
        &mut self,
        account: &'a CompressedPublicKey,
        key_id: u64,
    ) -> Result<(), BlockchainError> {
        let key = (account.clone(), key_id);
        self.agent_session_keys.insert(key, None);
        Ok(())
    }

    async fn get_session_keys_for_account(
        &mut self,
        account: &'a CompressedPublicKey,
    ) -> Result<Vec<SessionKey>, BlockchainError> {
        let mut keys: HashMap<u64, SessionKey> = self
            .storage
            .get_session_keys_for_account(account)
            .await?
            .into_iter()
            .map(|key| (key.key_id, key))
            .collect();

        for ((cached_account, key_id), entry) in &self.agent_session_keys {
            if cached_account != account {
                continue;
            }
            match entry {
                Some(key) => {
                    keys.insert(*key_id, key.clone());
                }
                None => {
                    keys.remove(key_id);
                }
            }
        }

        Ok(keys.into_values().collect())
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

    /// Get the topoheight to use for verification
    fn get_verification_topoheight(&self) -> u64 {
        self.topoheight
    }

    /// Get the recyclable TOS amount from expired freeze records
    async fn get_recyclable_tos(&mut self, account: &'a PublicKey) -> Result<u64, BlockchainError> {
        let energy_resource = self
            .internal_get_energy_resource(Cow::Borrowed(account))
            .await?;
        let recyclable = match energy_resource {
            Some(resource) => resource
                .get_recyclable_tos(self.topoheight)
                .map_err(|_| BlockchainError::Overflow)?,
            None => 0,
        };
        Ok(recyclable)
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

    // ===== TNS (TOS Name Service) Verification Methods =====

    async fn is_name_registered(&self, name_hash: &Hash) -> Result<bool, BlockchainError> {
        self.storage.is_name_registered(name_hash).await
    }

    async fn account_has_name(
        &self,
        account: &'a CompressedPublicKey,
    ) -> Result<bool, BlockchainError> {
        self.storage.account_has_name(account).await
    }

    async fn get_account_name_hash(
        &self,
        account: &'a CompressedPublicKey,
    ) -> Result<Option<Hash>, BlockchainError> {
        self.storage.get_account_name(account).await
    }

    async fn is_message_id_used(&self, message_id: &Hash) -> Result<bool, BlockchainError> {
        self.storage.is_message_id_used(message_id).await
    }

    async fn get_escrow(
        &mut self,
        escrow_id: &Hash,
    ) -> Result<Option<tos_common::escrow::EscrowAccount>, BlockchainError> {
        self.storage.get_escrow(escrow_id).await
    }

    async fn get_arbiter(
        &mut self,
        arbiter: &'a CompressedPublicKey,
    ) -> Result<Option<tos_common::arbitration::ArbiterAccount>, BlockchainError> {
        self.storage.get_arbiter(arbiter).await
    }

    async fn get_commit_arbitration_open(
        &mut self,
        escrow_id: &Hash,
        dispute_id: &Hash,
        round: u32,
    ) -> Result<Option<tos_common::transaction::CommitArbitrationOpenPayload>, BlockchainError>
    {
        let key = tos_common::arbitration::ArbitrationRoundKey {
            escrow_id: escrow_id.clone(),
            dispute_id: dispute_id.clone(),
            round,
        };
        self.storage.get_commit_arbitration_open(&key).await
    }

    async fn get_commit_arbitration_open_by_request(
        &mut self,
        request_id: &Hash,
    ) -> Result<Option<tos_common::transaction::CommitArbitrationOpenPayload>, BlockchainError>
    {
        let key = tos_common::arbitration::ArbitrationRequestKey {
            request_id: request_id.clone(),
        };
        self.storage
            .get_commit_arbitration_open_by_request(&key)
            .await
    }

    async fn set_commit_arbitration_open(
        &mut self,
        escrow_id: &Hash,
        dispute_id: &Hash,
        round: u32,
        payload: &tos_common::transaction::CommitArbitrationOpenPayload,
    ) -> Result<(), BlockchainError> {
        let round_key = tos_common::arbitration::ArbitrationRoundKey {
            escrow_id: escrow_id.clone(),
            dispute_id: dispute_id.clone(),
            round,
        };
        let request_key = tos_common::arbitration::ArbitrationRequestKey {
            request_id: payload.request_id.clone(),
        };
        self.storage
            .set_commit_arbitration_open(&round_key, &request_key, payload)
            .await
    }

    async fn get_commit_vote_request(
        &mut self,
        request_id: &Hash,
    ) -> Result<Option<tos_common::transaction::CommitVoteRequestPayload>, BlockchainError> {
        let key = tos_common::arbitration::ArbitrationRequestKey {
            request_id: request_id.clone(),
        };
        self.storage.get_commit_vote_request(&key).await
    }

    async fn set_commit_vote_request(
        &mut self,
        request_id: &Hash,
        payload: &tos_common::transaction::CommitVoteRequestPayload,
    ) -> Result<(), BlockchainError> {
        let key = tos_common::arbitration::ArbitrationRequestKey {
            request_id: request_id.clone(),
        };
        self.storage.set_commit_vote_request(&key, payload).await
    }

    async fn get_commit_selection_commitment(
        &mut self,
        request_id: &Hash,
    ) -> Result<Option<tos_common::transaction::CommitSelectionCommitmentPayload>, BlockchainError>
    {
        let key = tos_common::arbitration::ArbitrationRequestKey {
            request_id: request_id.clone(),
        };
        self.storage.get_commit_selection_commitment(&key).await
    }

    async fn set_commit_selection_commitment(
        &mut self,
        request_id: &Hash,
        payload: &tos_common::transaction::CommitSelectionCommitmentPayload,
    ) -> Result<(), BlockchainError> {
        let key = tos_common::arbitration::ArbitrationRequestKey {
            request_id: request_id.clone(),
        };
        self.storage
            .set_commit_selection_commitment(&key, payload)
            .await
    }

    async fn get_commit_juror_vote(
        &mut self,
        request_id: &Hash,
        juror_pubkey: &PublicKey,
    ) -> Result<Option<tos_common::transaction::CommitJurorVotePayload>, BlockchainError> {
        let key = tos_common::arbitration::ArbitrationJurorVoteKey {
            request_id: request_id.clone(),
            juror_pubkey: juror_pubkey.clone(),
        };
        self.storage.get_commit_juror_vote(&key).await
    }

    async fn set_commit_juror_vote(
        &mut self,
        request_id: &Hash,
        juror_pubkey: &PublicKey,
        payload: &tos_common::transaction::CommitJurorVotePayload,
    ) -> Result<(), BlockchainError> {
        let key = tos_common::arbitration::ArbitrationJurorVoteKey {
            request_id: request_id.clone(),
            juror_pubkey: juror_pubkey.clone(),
        };
        self.storage.set_commit_juror_vote(&key, payload).await
    }

    async fn list_commit_juror_votes(
        &mut self,
        request_id: &Hash,
    ) -> Result<Vec<tos_common::transaction::CommitJurorVotePayload>, BlockchainError> {
        self.storage.list_commit_juror_votes(request_id).await
    }
}
