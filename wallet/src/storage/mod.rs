mod backend;

use crate::{cipher::Cipher, config::SALT_SIZE};
use anyhow::{anyhow, Context, Result};
use indexmap::IndexMap;
use log::{debug, trace};
use tos_common::{
    api::{
        query::{Query, QueryResult},
        DataElement, DataValue,
    },
    asset::AssetData,
    config::{COIN_DECIMALS, TOS_ASSET},
    crypto::{Hash, PrivateKey},
    network::Network,
    serializer::{Reader, Serializer},
    transaction::TxVersion,
};

use backend::{Db, Tree};

// Keys used to retrieve from storage

// Security critical keys
const SALT_KEY: &[u8] = b"SALT";
// Password + salt is necessary to decrypt master key
const PASSWORD_SALT_KEY: &[u8] = b"PSALT";
// Master key to encrypt/decrypt while interacting with the storage
const MASTER_KEY: &[u8] = b"MKEY";
const PRIVATE_KEY: &[u8] = b"PKEY";

// Network and TX version needed for address generation and transaction building
const NETWORK: &[u8] = b"NET";
const TX_VERSION: &[u8] = b"TXV";

// Use this struct to get access to non-encrypted keys (such as salt for KDF and encrypted master key)
pub struct Storage {
    db: Db,
}

// Implement an encrypted storage system
// Stateless wallet: Only stores essential data (keys, network, tx_version)
// All balances, transactions, nonce, etc. are queried from daemon on-demand
pub struct EncryptedStorage {
    // cipher used to encrypt/decrypt/hash data
    cipher: Cipher,

    // Extra data tree for network, tx_version, private key, and custom data
    extra: Tree,

    // The inner storage
    inner: Storage,

    // Transaction version to use for building transactions
    tx_version: TxVersion,
}

impl EncryptedStorage {
    pub fn new(
        inner: Storage,
        key: &[u8],
        salt: [u8; SALT_SIZE],
        network: Network,
    ) -> Result<Self> {
        let cipher = Cipher::new(key, Some(salt))?;
        let mut storage = Self {
            extra: inner.db.open_tree(cipher.hash_key("extra"))?,
            cipher,
            inner,
            tx_version: TxVersion::T1,
        };

        if storage.has_network()? {
            let storage_network = storage.get_network()?;
            if storage_network != network {
                return Err(anyhow!(
                    "Network mismatch for this wallet storage (stored: {})!",
                    storage_network
                ));
            }
        } else {
            storage.set_network(&network)?;
        }

        // Load transaction version if exists
        if storage.contains_data(&storage.extra, TX_VERSION)? {
            storage.tx_version = storage.load_from_disk(&storage.extra, TX_VERSION)?;
        }

        Ok(storage)
    }

    // Flush on disk to make sure it is saved
    pub async fn flush(&mut self) -> Result<()> {
        self.inner.db.flush_async().await?;
        Ok(())
    }

    // Await for the storage to be flushed
    pub async fn stop(&mut self) {
        let _ = self.flush().await;
    }

    // Internal helper methods for encryption/decryption

    fn load_from_disk_optional<V: Serializer>(&self, tree: &Tree, key: &[u8]) -> Result<Option<V>> {
        trace!("load from disk optional");
        let hashed_key = self.cipher.hash_key(key);
        self.internal_load(tree, &hashed_key)
    }

    fn load_from_disk<V: Serializer>(&self, tree: &Tree, key: &[u8]) -> Result<V> {
        trace!("load from disk");
        self.load_from_disk_optional(tree, key)?.context(format!(
            "Error while loading data with hashed key {} from disk",
            hex::encode(self.cipher.hash_key(key))
        ))
    }

    fn load_from_disk_with_key<V: Serializer>(&self, tree: &Tree, key: &[u8]) -> Result<V> {
        trace!("load from disk with key");
        self.internal_load(tree, key)?.context(format!(
            "Error while loading data with key {} from disk",
            hex::encode(key)
        ))
    }

    fn internal_load<V: Serializer>(&self, tree: &Tree, key: &[u8]) -> Result<Option<V>> {
        if let Some(data) = tree.get(key)? {
            let bytes = self.cipher.decrypt_value(&data)?;
            let mut reader = Reader::new(&bytes);
            let value = V::read(&mut reader)?;
            return Ok(Some(value));
        }

        Ok(None)
    }

    fn create_encrypted_key(&self, key: &[u8]) -> Result<Vec<u8>> {
        Ok(self.cipher.encrypt_value(key)?)
    }

    fn load_from_disk_with_encrypted_key<V: Serializer>(
        &self,
        tree: &Tree,
        key: &[u8],
    ) -> Result<V> {
        let encrypted_key = self.create_encrypted_key(key)?;
        self.load_from_disk_with_key(tree, &encrypted_key)
    }

    #[allow(dead_code)]
    fn load_from_disk_optional_with_encrypted_key<V: Serializer>(
        &self,
        tree: &Tree,
        key: &[u8],
    ) -> Result<Option<V>> {
        let encrypted_key = self.create_encrypted_key(key)?;
        self.internal_load(tree, &encrypted_key)
    }

    fn save_to_disk_with_encrypted_key(&self, tree: &Tree, key: &[u8], value: &[u8]) -> Result<()> {
        trace!("save to disk with encrypted key");
        let encrypted_key = self.create_encrypted_key(key)?;
        self.save_to_disk_with_key(tree, &encrypted_key, value)
    }

    fn save_to_disk(&self, tree: &Tree, key: &[u8], value: &[u8]) -> Result<()> {
        trace!("save to disk");
        let hashed_key = self.cipher.hash_key(key);
        self.save_to_disk_with_key(tree, &hashed_key, value)
    }

    fn save_to_disk_with_key(&self, tree: &Tree, key: &[u8], value: &[u8]) -> Result<()> {
        trace!("save to disk with key");
        tree.insert(key, self.cipher.encrypt_value(value)?)?;
        Ok(())
    }

    #[allow(dead_code)]
    fn delete_from_disk(&self, tree: &Tree, key: &[u8]) -> Result<()> {
        trace!("delete from disk");
        let hashed_key = self.cipher.hash_key(key);
        tree.remove(hashed_key)?;
        Ok(())
    }

    #[allow(dead_code)]
    fn delete_from_disk_with_key(&self, tree: &Tree, key: &[u8]) -> Result<()> {
        trace!("delete from disk with key");
        tree.remove(key)?;
        Ok(())
    }

    fn delete_from_disk_with_encrypted_key(&self, tree: &Tree, key: &[u8]) -> Result<()> {
        trace!("delete from disk with encrypted key");
        let encrypted_key = self.create_encrypted_key(key)?;
        tree.remove(encrypted_key)?;
        Ok(())
    }

    fn contains_data(&self, tree: &Tree, key: &[u8]) -> Result<bool> {
        trace!("contains data");
        let hashed_key = self.cipher.hash_key(key);
        Ok(tree.contains_key(hashed_key)?)
    }

    fn contains_with_encrypted_key(&self, tree: &Tree, key: &[u8]) -> Result<bool> {
        trace!("contains encrypted data");
        let encrypted_key = self.create_encrypted_key(key)?;
        Ok(tree.contains_key(encrypted_key)?)
    }

    // Custom data tree methods (for extensibility)

    // Clear all entries from the custom tree
    pub fn clear_custom_tree(&mut self, name: impl Into<String>) -> Result<()> {
        let tree_name = name.into();
        let tree = self.inner.db.open_tree(self.cipher.hash_key(&tree_name))?;
        tree.clear()?;
        Ok(())
    }

    // Store a custom serializable data
    pub fn set_custom_data(
        &mut self,
        tree: impl Into<String>,
        key: &DataValue,
        value: &DataElement,
    ) -> Result<()> {
        let tree_name = tree.into();
        let tree = self.inner.db.open_tree(self.cipher.hash_key(&tree_name))?;
        self.save_to_disk_with_encrypted_key(&tree, &key.to_bytes(), &value.to_bytes())
    }

    // Delete a custom data using its key
    pub fn delete_custom_data(&mut self, tree: impl Into<String>, key: &DataValue) -> Result<()> {
        let tree_name = tree.into();
        let tree = self.inner.db.open_tree(self.cipher.hash_key(&tree_name))?;
        self.delete_from_disk_with_encrypted_key(&tree, &key.to_bytes())
    }

    // Retrieve a custom data in the selected format
    pub fn get_custom_data(&self, tree: impl Into<String>, key: &DataValue) -> Result<DataElement> {
        let tree_name = tree.into();
        let tree = self.inner.db.open_tree(self.cipher.hash_key(&tree_name))?;
        self.load_from_disk_with_encrypted_key(&tree, &key.to_bytes())
    }

    // Verify if the key is present in the DB
    pub fn has_custom_data(&self, tree: impl Into<String>, key: &DataValue) -> Result<bool> {
        let tree_name = tree.into();
        let tree = self.inner.db.open_tree(self.cipher.hash_key(&tree_name))?;
        self.contains_with_encrypted_key(&tree, &key.to_bytes())
    }

    // Search all entries with requested query_key/query_value
    pub fn query_db(
        &self,
        tree_name: impl Into<String>,
        query_key: Option<Query>,
        query_value: Option<Query>,
        maximum: Option<usize>,
    ) -> Result<QueryResult> {
        let tree_name = tree_name.into();
        let tree = self.inner.db.open_tree(self.cipher.hash_key(&tree_name))?;
        let mut result: IndexMap<DataValue, DataElement> = IndexMap::new();

        for res in tree.iter() {
            if let Some(max) = maximum {
                if result.len() >= max {
                    break;
                }
            }

            let (key, value) = res?;
            let decrypted_key = self.cipher.decrypt_value(&key)?;
            let key = {
                let mut reader = Reader::new(&decrypted_key);
                DataValue::read(&mut reader)?
            };

            if let Some(query_key) = &query_key {
                if !query_key.verify_value(&key) {
                    continue;
                }
            }

            let decrypted_value = self.cipher.decrypt_value(&value)?;
            let value = {
                let mut reader = Reader::new(&decrypted_value);
                DataElement::read(&mut reader)?
            };

            if let Some(query_value) = &query_value {
                if !query_value.verify_element(&value) {
                    continue;
                }
            }

            result.insert(key, value);
        }

        Ok(QueryResult {
            entries: result,
            next: None,
        })
    }

    // Get all keys from the custom tree
    pub fn get_custom_tree_keys(
        &self,
        tree_name: impl Into<String>,
        query: Option<Query>,
        maximum: Option<usize>,
    ) -> Result<Vec<DataValue>> {
        let tree_name = tree_name.into();
        let tree = self.inner.db.open_tree(self.cipher.hash_key(&tree_name))?;
        let mut result: Vec<DataValue> = Vec::new();

        for res in tree.iter() {
            if let Some(max) = maximum {
                if result.len() >= max {
                    break;
                }
            }

            let (key, _) = res?;
            let decrypted_key = self.cipher.decrypt_value(&key)?;
            let key = {
                let mut reader = Reader::new(&decrypted_key);
                DataValue::read(&mut reader)?
            };

            if let Some(query) = &query {
                if !query.verify_value(&key) {
                    continue;
                }
            }

            result.push(key);
        }

        Ok(result)
    }

    // Count entries from a tree
    pub fn count_custom_tree_entries(
        &self,
        tree_name: impl Into<String>,
        query: Option<Query>,
    ) -> Result<usize> {
        let tree_name = tree_name.into();
        let tree = self.inner.db.open_tree(self.cipher.hash_key(&tree_name))?;
        let mut count = 0;

        if query.is_none() {
            return Ok(tree.len());
        }

        for res in tree.iter() {
            let (key, _) = res?;
            let decrypted_key = self.cipher.decrypt_value(&key)?;
            let key = {
                let mut reader = Reader::new(&decrypted_key);
                DataValue::read(&mut reader)?
            };

            if let Some(query) = &query {
                if !query.verify_value(&key) {
                    continue;
                }
            }

            count += 1;
        }

        Ok(count)
    }

    // TX Version methods

    // Set the TX Version
    pub async fn set_tx_version(&mut self, version: TxVersion) -> Result<()> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("set tx version to {:?}", version);
        }
        self.save_to_disk(&self.extra, TX_VERSION, &version.to_bytes())?;
        self.tx_version = version;
        Ok(())
    }

    // Get the TX Version
    pub async fn get_tx_version(&self) -> Result<TxVersion> {
        trace!("get tx version");
        Ok(self.tx_version)
    }

    // Private key methods

    // Store the private key
    pub fn set_private_key(&mut self, private_key: &PrivateKey) -> Result<()> {
        self.save_to_disk(&self.extra, PRIVATE_KEY, &private_key.to_bytes())
    }

    // Retrieve the private key of this wallet
    pub fn get_private_key(&self) -> Result<PrivateKey> {
        self.load_from_disk(&self.extra, PRIVATE_KEY)
    }

    // Storage accessor methods

    pub fn get_public_storage(&self) -> &Storage {
        &self.inner
    }

    pub fn get_mutable_public_storage(&mut self) -> &mut Storage {
        &mut self.inner
    }

    // Network methods

    fn set_network(&mut self, network: &Network) -> Result<()> {
        if log::log_enabled!(log::Level::Debug) {
            debug!("Storing network {} in wallet", network);
        }
        self.save_to_disk(&self.extra, NETWORK, &network.to_bytes())
    }

    fn get_network(&self) -> Result<Network> {
        debug!("Retrieving network from wallet");
        self.load_from_disk(&self.extra, NETWORK)
    }

    fn has_network(&self) -> Result<bool> {
        self.contains_data(&self.extra, NETWORK)
    }

    // Stateless wallet: Asset tracking methods are no-ops
    // All assets are considered "tracked" since we query daemon on-demand

    // Check if an asset is tracked (always returns true in stateless mode)
    pub fn is_asset_tracked(&self, _asset: &tos_common::crypto::Hash) -> Result<bool> {
        Ok(true)
    }

    // Track an asset (no-op in stateless mode)
    pub fn track_asset(&mut self, _asset: &tos_common::crypto::Hash) -> Result<()> {
        Ok(())
    }

    // Untrack an asset (no-op in stateless mode)
    pub fn untrack_asset(&mut self, _asset: &tos_common::crypto::Hash) -> Result<()> {
        Ok(())
    }

    // Get the number of tracked assets (returns 0 in stateless mode)
    pub fn get_assets_count(&self) -> Result<usize> {
        Ok(0)
    }

    // Set the name for an asset (no-op in stateless mode)
    pub async fn set_asset_name(&mut self, _asset: &Hash, _name: String) -> Result<()> {
        Ok(())
    }

    // Get asset data (stateless mode: returns TOS asset data or error)
    // For non-TOS assets, callers should use daemon API to get asset data
    pub async fn get_asset(&self, asset: &Hash) -> Result<AssetData> {
        if *asset == TOS_ASSET {
            Ok(AssetData::new(
                COIN_DECIMALS,
                "TOS".to_string(),
                "TOS".to_string(),
                None,
                None,
            ))
        } else {
            Err(anyhow!(
                "Stateless wallet: use daemon API to get asset data for {}",
                asset
            ))
        }
    }

    // Add asset data (no-op in stateless mode)
    pub async fn add_asset(&mut self, _asset: &Hash, _data: AssetData) -> Result<()> {
        Ok(())
    }

    // Get all tracked assets (returns empty vec in stateless mode)
    pub fn get_tracked_assets(&self) -> Result<Vec<Hash>> {
        Ok(Vec::new())
    }

    // Get assets with data (returns empty map in stateless mode)
    pub async fn get_assets_with_data(&self) -> Result<std::collections::HashMap<Hash, AssetData>> {
        Ok(std::collections::HashMap::new())
    }

    // Get filtered transactions (returns empty vec in stateless mode)
    // In stateless wallet, transactions should be queried from daemon
    pub fn get_filtered_transactions(
        &self,
        _address: Option<&tos_common::crypto::PublicKey>,
        _asset: Option<&Hash>,
        _min_topoheight: Option<u64>,
        _max_topoheight: Option<u64>,
        _accept_incoming: bool,
        _accept_outgoing: bool,
        _accept_coinbase: bool,
        _accept_burn: bool,
        _query: Option<&Query>,
        _limit: Option<usize>,
        _skip: Option<usize>,
    ) -> Result<Vec<crate::entry::TransactionEntry>> {
        Ok(Vec::new())
    }
}

impl Storage {
    pub fn new(name: &str) -> Result<Self> {
        Ok(Self {
            db: backend::open(name)?,
        })
    }

    // save the encrypted form of the master key
    // it can only be decrypted using the password-based key
    pub fn set_encrypted_master_key(&mut self, encrypted_key: &[u8]) -> Result<()> {
        self.db
            .insert(MASTER_KEY, encrypted_key)
            .context("Error while setting encrypted master key")?;
        Ok(())
    }

    // retrieve the encrypted form of the master key
    pub fn get_encrypted_master_key(&self) -> Result<Vec<u8>> {
        self.db
            .get(MASTER_KEY)?
            .map(|v| v.to_vec())
            .context("Error while getting encrypted master key: not found")
    }

    // set password salt used to derive the password-based key
    pub fn set_password_salt(&mut self, salt: &[u8]) -> Result<()> {
        self.db
            .insert(PASSWORD_SALT_KEY, salt)
            .context("Error while setting password salt")?;
        Ok(())
    }

    // retrieve password salt used to derive the password-based key
    pub fn get_password_salt(&self) -> Result<[u8; SALT_SIZE]> {
        let salt = self
            .db
            .get(PASSWORD_SALT_KEY)?
            .context("Error while getting password salt")?;
        let mut salt_bytes = [0u8; SALT_SIZE];
        if salt.len() != SALT_SIZE {
            return Err(anyhow!(
                "Invalid password salt size: expected {}, got {}",
                SALT_SIZE,
                salt.len()
            ));
        }
        salt_bytes.copy_from_slice(&salt);
        Ok(salt_bytes)
    }

    // get the salt used for encrypted storage
    pub fn get_encrypted_storage_salt(&self) -> Result<Vec<u8>> {
        self.db
            .get(SALT_KEY)?
            .map(|v| v.to_vec())
            .context("Error while getting encrypted storage salt")
    }

    // set the salt used for encrypted storage
    pub fn set_encrypted_storage_salt(&mut self, salt: &[u8]) -> Result<()> {
        self.db
            .insert(SALT_KEY, salt)
            .context("Error while setting encrypted storage salt")?;
        Ok(())
    }
}
