use crate::core::{error::BlockchainError, mempool::Mempool, storage::Storage};
use async_trait::async_trait;
use std::{
    borrow::Cow,
    collections::{hash_map::Entry, HashMap},
};
use tos_common::{
    account::Nonce,
    block::{BlockVersion, TopoHeight},
    config::COIN_VALUE,
    crypto::{
        elgamal::{Ciphertext, CompressedPublicKey},
        Hash, PublicKey,
    },
    transaction::{verify::BlockchainVerificationState, MultiSigPayload, Reference, Transaction},
};
use tos_environment::Environment;
use tos_kernel::Module;

struct Account {
    // Account nonce used to verify valid transaction
    nonce: u64,
    // Assets ready as source for any transfer/transaction
    assets: HashMap<Hash, u64>,
    // UNO (encrypted) assets for privacy-preserving transactions
    uno_assets: HashMap<Hash, Ciphertext>,
    // Multisig configured
    // This is used to verify the validity of the multisig setup
    multisig: Option<MultiSigPayload>,
}

const _: () = assert!(COIN_VALUE > 0);

pub struct MempoolState<'a, S: Storage> {
    // If the provider is mainnet or not
    mainnet: bool,
    // Mempool from which it's backed
    mempool: &'a Mempool,
    // Storage in case sender balances aren't in mempool cache
    storage: &'a S,
    // Contract environment
    environment: &'a Environment,
    // Receiver balances
    receiver_balances: HashMap<Cow<'a, PublicKey>, HashMap<Cow<'a, Hash>, u64>>,
    // UNO (encrypted) receiver balances
    receiver_uno_balances: HashMap<Cow<'a, PublicKey>, HashMap<Cow<'a, Hash>, Ciphertext>>,
    // Sender accounts
    // This is used to verify ZK Proofs and store/update nonces
    accounts: HashMap<Cow<'a, PublicKey>, Account>,
    // Contract modules
    contracts: HashMap<Hash, Cow<'a, Module>>,
    // The current stable topoheight of the chain
    stable_topoheight: TopoHeight,
    // The current topoheight of the chain
    topoheight: TopoHeight,
    // Block header version
    block_version: BlockVersion,
    // Optional timestamp override (seconds) used for deterministic verification (e.g. conformance).
    // When None, mempool verification uses the current system time.
    verification_timestamp_override: Option<u64>,
}

impl<'a, S: Storage> MempoolState<'a, S> {
    pub fn new(
        mempool: &'a Mempool,
        storage: &'a S,
        environment: &'a Environment,
        stable_topoheight: TopoHeight,
        topoheight: TopoHeight,
        block_version: BlockVersion,
        mainnet: bool,
        verification_timestamp_override: Option<u64>,
    ) -> Self {
        Self {
            mainnet,
            mempool,
            storage,
            environment,
            receiver_balances: HashMap::new(),
            receiver_uno_balances: HashMap::new(),
            accounts: HashMap::new(),
            contracts: HashMap::new(),
            stable_topoheight,
            topoheight,
            block_version,
            verification_timestamp_override,
        }
    }

    // Retrieve the sender cache (inclunding balances and multisig)
    pub fn get_sender_cache(
        &mut self,
        key: &PublicKey,
    ) -> Option<(HashMap<Hash, u64>, Option<MultiSigPayload>)> {
        let account = self.accounts.remove(key)?;

        Some((account.assets, account.multisig))
    }

    // Retrieve the receiver balance
    // We never store the receiver balance in mempool, only outgoing balances
    // So we just get it from our internal cache or from storage
    async fn internal_get_receiver_balance<'b>(
        &'b mut self,
        account: Cow<'a, PublicKey>,
        asset: Cow<'a, Hash>,
    ) -> Result<&'b mut u64, BlockchainError> {
        // If its borrowed, it cost nothing to clone the Cow as it's just the reference being cloned
        match self
            .receiver_balances
            .entry(account.clone())
            .or_insert_with(HashMap::new)
            .entry(asset.clone())
        {
            Entry::Occupied(entry) => Ok(entry.into_mut()),
            Entry::Vacant(entry) => {
                let (version, _) = self
                    .storage
                    .get_new_versioned_balance(&account, &asset, self.topoheight)
                    .await?;
                Ok(entry.insert(version.take_balance()))
            }
        }
    }

    // Retrieve the versioned balance based on the TX reference
    async fn get_versioned_balance_for_reference(
        storage: &S,
        key: &PublicKey,
        asset: &Hash,
        current_topoheight: TopoHeight,
        reference: &Reference,
    ) -> Result<u64, BlockchainError> {
        let (output, _, version) = super::search_versioned_balance_for_reference(
            storage,
            key,
            asset,
            current_topoheight,
            reference,
            false,
        )
        .await?;

        Ok(version.take_balance_with(output))
    }

    // Retrieve the nonce & the multisig state for a sender account
    async fn create_sender_account(
        mempool: &Mempool,
        storage: &S,
        key: &PublicKey,
        topoheight: TopoHeight,
    ) -> Result<Account, BlockchainError> {
        let (nonce, multisig) = if let Some(cache) = mempool.get_cache_for(key) {
            let nonce = cache.get_next_nonce();
            let multisig = if let Some(multisig) = cache.get_multisig() {
                Some(multisig.clone())
            } else {
                storage
                    .get_multisig_at_maximum_topoheight_for(key, topoheight)
                    .await?
                    .map(|(_, v)| v.take().map(|v| v.into_owned()))
                    .flatten()
            };

            (nonce, multisig)
        } else {
            let nonce = storage
                .get_nonce_at_maximum_topoheight(key, topoheight)
                .await?
                .map(|(_, v)| v.get_nonce())
                .unwrap_or(0);

            let multisig = storage
                .get_multisig_at_maximum_topoheight_for(key, topoheight)
                .await?
                .map(|(_, v)| v.take().map(|v| v.into_owned()))
                .flatten();

            (nonce, multisig)
        };

        Ok(Account {
            nonce,
            assets: HashMap::new(),
            uno_assets: HashMap::new(),
            multisig,
        })
    }

    // Retrieve the sender balance
    // For this, we first look in our internal cache,
    // If not found, we check in mempool cache,
    // If still not present, we check in storage and determine using reference
    // Which version to use
    async fn internal_get_sender_balance<'b>(
        &'b mut self,
        key: Cow<'a, PublicKey>,
        asset: &Hash,
        reference: &Reference,
    ) -> Result<&'b mut u64, BlockchainError> {
        match self.accounts.entry(key.clone()) {
            Entry::Occupied(o) => {
                let account = o.into_mut();
                match account.assets.entry(asset.clone()) {
                    Entry::Occupied(entry) => Ok(entry.into_mut()),
                    Entry::Vacant(entry) => match self.mempool.get_cache_for(key.as_ref()) {
                        Some(cache) => {
                            if let Some(version) = cache.get_balances().get(asset) {
                                Ok(entry.insert(version.clone()))
                            } else {
                                let ct = Self::get_versioned_balance_for_reference(
                                    &self.storage,
                                    key.as_ref(),
                                    asset,
                                    self.topoheight,
                                    reference,
                                )
                                .await?;
                                Ok(entry.insert(ct))
                            }
                        }
                        None => {
                            let ct = Self::get_versioned_balance_for_reference(
                                &self.storage,
                                key.as_ref(),
                                asset,
                                self.topoheight,
                                reference,
                            )
                            .await?;
                            Ok(entry.insert(ct))
                        }
                    },
                }
            }
            Entry::Vacant(e) => {
                let account = e.insert(
                    Self::create_sender_account(
                        &self.mempool,
                        &self.storage,
                        key.as_ref(),
                        self.topoheight,
                    )
                    .await?,
                );

                match account.assets.entry(asset.clone()) {
                    Entry::Occupied(entry) => Ok(entry.into_mut()),
                    Entry::Vacant(entry) => {
                        let (version, new) = self
                            .storage
                            .get_new_versioned_balance(key.as_ref(), asset, self.topoheight)
                            .await?;
                        if new {
                            return Err(BlockchainError::NoPreviousBalanceFound);
                        }

                        Ok(entry.insert(version.take_balance()))
                    }
                }
            }
        }
    }

    // Retrieve the account nonce
    // Only sender accounts should be used here
    async fn internal_get_account_nonce(
        &mut self,
        key: &'a PublicKey,
    ) -> Result<Nonce, BlockchainError> {
        match self.accounts.entry(Cow::Borrowed(key)) {
            Entry::Occupied(o) => Ok(o.get().nonce),
            Entry::Vacant(e) => {
                let account =
                    Self::create_sender_account(&self.mempool, &self.storage, key, self.topoheight)
                        .await?;
                Ok(e.insert(account).nonce)
            }
        }
    }

    // Update the account nonce
    // Only sender accounts should be used here
    // For each TX, we must update the nonce by one
    async fn internal_update_account_nonce(
        &mut self,
        account: &'a PublicKey,
        new_nonce: u64,
    ) -> Result<(), BlockchainError> {
        let account = self.accounts.get_mut(account).ok_or_else(|| {
            BlockchainError::AccountNotFound(account.as_address(self.storage.is_mainnet()))
        })?;
        account.nonce = new_nonce;

        Ok(())
    }
}

#[async_trait]
impl<'a, S: Storage> BlockchainVerificationState<'a, BlockchainError> for MempoolState<'a, S> {
    /// Verify the TX version and reference
    async fn pre_verify_tx<'b>(&'b mut self, tx: &Transaction) -> Result<(), BlockchainError> {
        super::pre_verify_tx(
            self.storage,
            tx,
            self.stable_topoheight,
            self.topoheight,
            self.get_block_version(),
            self.get_network(),
        )
        .await
    }

    /// Get the balance for a receiver account
    async fn get_receiver_balance<'b>(
        &'b mut self,
        account: Cow<'a, PublicKey>,
        asset: Cow<'a, Hash>,
    ) -> Result<&'b mut u64, BlockchainError> {
        self.internal_get_receiver_balance(account, asset).await
    }

    /// Get the balance used for verification of funds for the sender account
    async fn get_sender_balance<'b>(
        &'b mut self,
        account: Cow<'a, PublicKey>,
        asset: Cow<'a, Hash>,
        reference: &Reference,
    ) -> Result<&'b mut u64, BlockchainError> {
        let asset_ref = asset.as_ref();
        self.internal_get_sender_balance(account, asset_ref, reference)
            .await
    }

    /// Apply new output to a sender account
    /// In this state, we don't need to store the output
    async fn add_sender_output(
        &mut self,
        _: Cow<'a, PublicKey>,
        _: Cow<'a, Hash>,
        _: u64,
    ) -> Result<(), BlockchainError> {
        Ok(())
    }

    // ===== UNO (Privacy Balance) Methods =====

    /// Get the UNO (encrypted) balance for a receiver account
    async fn get_receiver_uno_balance<'b>(
        &'b mut self,
        account: Cow<'a, PublicKey>,
        asset: Cow<'a, Hash>,
    ) -> Result<&'b mut Ciphertext, BlockchainError> {
        // Check if we already have this balance in our internal cache
        match self
            .receiver_uno_balances
            .entry(account.clone())
            .or_insert_with(HashMap::new)
            .entry(asset.clone())
        {
            Entry::Occupied(o) => Ok(o.into_mut()),
            Entry::Vacant(e) => {
                // Try to get from storage
                let balance = if let Some((_, version)) = self
                    .storage
                    .get_uno_balance_at_maximum_topoheight(&account, &asset, self.topoheight)
                    .await?
                {
                    // Decompress for computation
                    let mut version = version;
                    version
                        .get_mut_balance()
                        .computable()
                        .map_err(BlockchainError::from)?
                        .clone()
                } else {
                    // Default to zero ciphertext
                    Ciphertext::zero()
                };
                Ok(e.insert(balance))
            }
        }
    }

    /// Get the UNO (encrypted) balance used for verification of funds for the sender account
    async fn get_sender_uno_balance<'b>(
        &'b mut self,
        account: &'a PublicKey,
        asset: &'a Hash,
        _reference: &Reference,
    ) -> Result<&'b mut Ciphertext, BlockchainError> {
        // Get or create account
        let acc = match self.accounts.entry(Cow::Borrowed(account)) {
            Entry::Occupied(o) => o.into_mut(),
            Entry::Vacant(e) => {
                let acc = Self::create_sender_account(
                    self.mempool,
                    self.storage,
                    account,
                    self.topoheight,
                )
                .await?;
                e.insert(acc)
            }
        };

        // Check if we already have this UNO asset balance
        match acc.uno_assets.entry(asset.clone()) {
            Entry::Occupied(o) => Ok(o.into_mut()),
            Entry::Vacant(e) => {
                // Try to get from storage
                let balance = if let Some((_, version)) = self
                    .storage
                    .get_uno_balance_at_maximum_topoheight(account, asset, self.topoheight)
                    .await?
                {
                    // Decompress for computation
                    let mut version = version;
                    version
                        .get_mut_balance()
                        .computable()
                        .map_err(BlockchainError::from)?
                        .clone()
                } else {
                    // Default to zero ciphertext
                    Ciphertext::zero()
                };
                Ok(e.insert(balance))
            }
        }
    }

    /// Apply new output ciphertext to a sender's UNO account
    async fn add_sender_uno_output(
        &mut self,
        _account: &'a PublicKey,
        _asset: &'a Hash,
        _output: Ciphertext,
    ) -> Result<(), BlockchainError> {
        // In mempool state, we don't need to track outputs since
        // balances are already verified - this is a no-op
        Ok(())
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
        // For mempool state, we don't have atomic operations
        // This is acceptable because mempool only validates individual txs
        // The actual ordering protection happens at blockchain level
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

    /// Get the timestamp to use for verification (uses current system time for mempool)
    fn get_verification_timestamp(&self) -> u64 {
        if let Some(ts) = self.verification_timestamp_override {
            return ts;
        }
        std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Get the topoheight to use for verification (uses current chain topoheight for mempool)
    fn get_verification_topoheight(&self) -> u64 {
        self.topoheight
    }

    /// Get the recyclable TOS amount from expired freeze records
    async fn get_recyclable_tos(&mut self, account: &'a PublicKey) -> Result<u64, BlockchainError> {
        let _ = account;
        Ok(0)
    }

    /// Set the multisig state for an account
    async fn set_multisig_state(
        &mut self,
        account: &'a PublicKey,
        payload: &MultiSigPayload,
    ) -> Result<(), BlockchainError> {
        let account = self
            .accounts
            .get_mut(account)
            .ok_or_else(|| BlockchainError::AccountNotFound(account.as_address(self.mainnet)))?;
        if payload.is_delete() {
            account.multisig = None;
        } else {
            account.multisig = Some(payload.clone());
        }

        Ok(())
    }

    /// Get the multisig state for an account
    /// If the account is not a multisig account, return None
    async fn get_multisig_state(
        &mut self,
        account: &'a PublicKey,
    ) -> Result<Option<&MultiSigPayload>, BlockchainError> {
        self.accounts
            .get(account)
            .map(|a| a.multisig.as_ref())
            .ok_or_else(|| {
                BlockchainError::AccountNotFound(account.as_address(self.storage.is_mainnet()))
            })
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
        // Insert contract with owned hash (no memory leak!)
        if self
            .contracts
            .insert(hash.clone(), Cow::Borrowed(module))
            .is_some()
        {
            return Err(BlockchainError::ContractAlreadyExists);
        }

        Ok(())
    }

    async fn load_contract_module(&mut self, hash: &Hash) -> Result<bool, BlockchainError> {
        // Check if already loaded
        if self.contracts.contains_key(hash) {
            return Ok(true);
        }

        // Load from storage - return Ok(false) if not found
        let module_opt = self
            .storage
            .get_contract_at_maximum_topoheight_for(hash, self.topoheight)
            .await?
            .map(|(_, v)| v.take().map(|v| v.into_owned()))
            .flatten();

        match module_opt {
            Some(module) => {
                // Insert contract with owned hash (no memory leak!)
                self.contracts.insert(hash.clone(), Cow::Owned(module));
                Ok(true)
            }
            None => {
                // Contract doesn't exist - this is OK for existence checks
                Ok(false)
            }
        }
    }

    async fn get_contract_module_with_environment(
        &self,
        hash: &Hash,
    ) -> Result<(&Module, &Environment), BlockchainError> {
        let module = self
            .contracts
            .get(hash)
            .ok_or_else(|| BlockchainError::ContractNotFound(hash.clone()))?;

        Ok((module, self.environment))
    }

    fn get_network(&self) -> tos_common::network::Network {
        self.storage
            .get_network()
            .unwrap_or(tos_common::network::Network::Mainnet)
    }
}
