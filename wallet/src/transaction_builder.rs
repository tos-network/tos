use crate::{error::WalletError, storage::EncryptedStorage};
use log::trace;
use std::collections::{HashMap, HashSet};
use tos_common::{
    account::CiphertextCache,
    crypto::{
        elgamal::Ciphertext,
        Hash, Hashable, PublicKey,
    },
    transaction::{
        builder::{AccountState, FeeHelper, UnoAccountState},
        Reference, Transaction,
    },
};

// Simple balance container for transaction building
// Holds the amount available for a given asset
#[derive(Debug, Clone)]
pub struct Balance {
    pub amount: u64,
}

impl Balance {
    pub fn new(amount: u64) -> Self {
        Self { amount }
    }
}

/// UNO (privacy) balance container
/// Holds both the encrypted ciphertext and the decrypted plaintext amount
#[derive(Debug, Clone)]
pub struct UnoBalance {
    /// The encrypted balance ciphertext (for ZK proof generation)
    pub ciphertext: CiphertextCache,
    /// The decrypted plaintext amount (known only to wallet owner)
    pub amount: u64,
}

impl UnoBalance {
    pub fn new(ciphertext: CiphertextCache, amount: u64) -> Self {
        Self { ciphertext, amount }
    }
}

// State used to estimate fees for a transaction
// Because fees can be higher if a destination account is not registered
// We need to give this information during the estimation of fees
#[derive(Default)]
pub struct EstimateFeesState {
    // this is containing the registered keys that we are aware of
    registered_keys: HashSet<PublicKey>,
}

impl EstimateFeesState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_registered_keys(&mut self, registered_keys: HashSet<PublicKey>) {
        self.registered_keys = registered_keys;
    }

    pub fn add_registered_key(&mut self, key: PublicKey) {
        self.registered_keys.insert(key);
    }
}

impl FeeHelper for EstimateFeesState {
    type Error = WalletError;

    fn account_exists(&self, key: &PublicKey) -> Result<bool, Self::Error> {
        Ok(self.registered_keys.contains(key))
    }
}

// State used to build a transaction
// It contains the balances of the wallet and the registered keys
pub struct TransactionBuilderState {
    // Inner state used to estimate fees
    inner: EstimateFeesState,
    // If we are on mainnet or not
    mainnet: bool,
    // Balances of the wallet (plaintext)
    balances: HashMap<Hash, Balance>,
    // UNO balances of the wallet (encrypted)
    uno_balances: HashMap<Hash, UnoBalance>,
    // Reference at which the transaction is built
    reference: Reference,
    // Nonce of the transaction
    nonce: u64,
    // The hash of the transaction that has been built
    tx_hash_built: Option<Hash>,
    // The stable topoheight detected during the TX building
    // This is used to update the last coinbase reward topoheight
    stable_topoheight: Option<u64>,
}

impl TransactionBuilderState {
    pub fn new(mainnet: bool, reference: Reference, nonce: u64) -> Self {
        Self {
            inner: EstimateFeesState {
                registered_keys: HashSet::new(),
            },
            mainnet,
            balances: HashMap::new(),
            uno_balances: HashMap::new(),
            reference,
            nonce,
            tx_hash_built: None,
            stable_topoheight: None,
        }
    }

    pub fn get_reference(&self) -> &Reference {
        &self.reference
    }

    pub fn set_reference(&mut self, reference: Reference) {
        self.reference = reference;
    }

    pub fn get_nonce(&self) -> u64 {
        self.nonce
    }

    pub fn has_balance_for(&self, asset: &Hash) -> bool {
        self.balances.contains_key(asset)
    }

    // Create state from an existing transaction
    // Stateless wallet: We don't track balances locally, so this just captures
    // the transaction reference and nonce for apply_changes (which is a no-op)
    #[allow(unused_variables)]
    pub async fn from_tx(
        storage: &EncryptedStorage,
        transaction: &Transaction,
        mainnet: bool,
    ) -> Result<Self, WalletError> {
        let mut state = Self::new(
            mainnet,
            transaction.get_reference().clone(),
            transaction.get_nonce(),
        );

        // Stateless wallet: Don't track balances locally
        // Balances are queried fresh from daemon before each transaction
        state.set_tx_hash_built(transaction.hash());

        Ok(state)
    }

    pub fn set_balances(&mut self, balances: HashMap<Hash, Balance>) {
        self.balances = balances;
    }

    pub fn add_balance(&mut self, asset: Hash, balance: Balance) {
        self.balances.insert(asset, balance);
    }

    /// Set all UNO (encrypted) balances
    pub fn set_uno_balances(&mut self, uno_balances: HashMap<Hash, UnoBalance>) {
        self.uno_balances = uno_balances;
    }

    /// Add a single UNO balance
    pub fn add_uno_balance(&mut self, asset: Hash, balance: UnoBalance) {
        self.uno_balances.insert(asset, balance);
    }

    /// Check if UNO balance exists for an asset
    pub fn has_uno_balance_for(&self, asset: &Hash) -> bool {
        self.uno_balances.contains_key(asset)
    }

    pub fn set_registered_keys(&mut self, registered_keys: HashSet<PublicKey>) {
        self.inner.registered_keys = registered_keys;
    }

    pub fn add_registered_key(&mut self, key: PublicKey) {
        self.inner.registered_keys.insert(key);
    }

    // This must be called once the TX has been built
    pub fn set_tx_hash_built(&mut self, tx_hash: Hash) {
        self.tx_hash_built = Some(tx_hash);
    }

    // Set the stable topoheight detected during the TX building
    pub fn set_stable_topoheight(&mut self, stable_topoheight: u64) {
        self.stable_topoheight = Some(stable_topoheight);
    }

    // Apply the changes to the storage
    // Stateless wallet: This is a no-op since we don't cache balances or nonces locally
    // All state is queried fresh from daemon before each transaction
    #[allow(unused_variables)]
    pub async fn apply_changes(
        &mut self,
        storage: &mut EncryptedStorage,
    ) -> Result<(), WalletError> {
        // Stateless wallet: No local cache to update
        // - Balances are queried from daemon before each TX
        // - Nonces are queried from daemon before each TX
        // - No tx_cache needed since we don't track pending transactions locally
        if log::log_enabled!(log::Level::Trace) {
            trace!("Stateless wallet: apply_changes is no-op");
        }
        Ok(())
    }
}

impl FeeHelper for TransactionBuilderState {
    type Error = WalletError;

    fn account_exists(&self, key: &PublicKey) -> Result<bool, Self::Error> {
        self.inner.account_exists(key)
    }
}

impl AccountState for TransactionBuilderState {
    fn is_mainnet(&self) -> bool {
        self.mainnet
    }

    fn get_reference(&self) -> Reference {
        self.reference.clone()
    }

    fn get_account_balance(&self, asset: &Hash) -> Result<u64, Self::Error> {
        self.balances
            .get(asset)
            .map(|b| b.amount)
            .ok_or_else(|| WalletError::BalanceNotFound(asset.clone()))
    }

    fn update_account_balance(
        &mut self,
        asset: &Hash,
        new_balance: u64,
    ) -> Result<(), Self::Error> {
        self.balances.insert(
            asset.clone(),
            Balance {
                amount: new_balance,
            },
        );
        Ok(())
    }

    fn get_nonce(&self) -> Result<u64, Self::Error> {
        Ok(self.nonce)
    }

    fn update_nonce(&mut self, new_nonce: u64) -> Result<(), Self::Error> {
        self.nonce = new_nonce;
        Ok(())
    }

    fn is_account_registered(&self, key: &PublicKey) -> Result<bool, Self::Error> {
        // Use the same logic as account_exists for consistency
        Ok(self.inner.registered_keys.contains(key))
    }
}

impl UnoAccountState for TransactionBuilderState {
    fn get_uno_ciphertext(&self, asset: &Hash) -> Result<CiphertextCache, Self::Error> {
        self.uno_balances
            .get(asset)
            .map(|b| b.ciphertext.clone())
            .ok_or_else(|| WalletError::UnoBalanceNotFound(asset.clone()))
    }

    fn get_uno_balance(&self, asset: &Hash) -> Result<u64, Self::Error> {
        self.uno_balances
            .get(asset)
            .map(|b| b.amount)
            .ok_or_else(|| WalletError::UnoBalanceNotFound(asset.clone()))
    }

    fn update_uno_balance(
        &mut self,
        asset: &Hash,
        new_balance: u64,
        new_ciphertext: Ciphertext,
    ) -> Result<(), Self::Error> {
        self.uno_balances.insert(
            asset.clone(),
            UnoBalance {
                ciphertext: CiphertextCache::Decompressed(new_ciphertext),
                amount: new_balance,
            },
        );
        Ok(())
    }
}

impl AsMut<EstimateFeesState> for TransactionBuilderState {
    fn as_mut(&mut self) -> &mut EstimateFeesState {
        &mut self.inner
    }
}
