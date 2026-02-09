// TNS (TOS Name Service) storage provider trait

use crate::core::error::BlockchainError;
use async_trait::async_trait;
use tos_common::{
    crypto::{Hash, PublicKey},
    serializer::Serializer,
};

/// Storage provider for TNS (TOS Name Service)
#[async_trait]
pub trait TnsProvider: Send + Sync {
    // ===== Bootstrap Sync =====

    /// List all TNS name registrations with skip/limit pagination
    /// Returns (name_hash, owner) pairs
    async fn list_all_tns_names(
        &self,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<(Hash, PublicKey)>, BlockchainError>;

    // ===== Name Registration =====

    /// Check if a name hash is already registered
    async fn is_name_registered(&self, name_hash: &Hash) -> Result<bool, BlockchainError>;

    /// Get the owner of a registered name
    /// Returns None if name is not registered
    async fn get_name_owner(&self, name_hash: &Hash) -> Result<Option<PublicKey>, BlockchainError>;

    /// Check if an account already has a registered name
    async fn account_has_name(&self, owner: &PublicKey) -> Result<bool, BlockchainError>;

    /// Get the name hash registered by an account
    /// Returns None if account has no registered name
    async fn get_account_name(&self, owner: &PublicKey) -> Result<Option<Hash>, BlockchainError>;

    /// Register a name for an account
    ///
    /// # Arguments
    /// * `name_hash` - The blake3 hash of the normalized name
    /// * `owner` - The public key of the owner
    ///
    /// # Errors
    /// * Returns error if name is already registered
    /// * Returns error if account already has a name
    async fn register_name(
        &mut self,
        name_hash: Hash,
        owner: PublicKey,
    ) -> Result<(), BlockchainError>;

    // ===== Administrative Operations =====

    /// Delete a name registration (for rollback scenarios)
    async fn delete_name_registration(&mut self, name_hash: &Hash) -> Result<(), BlockchainError>;

    /// Delete an account's name mapping (for rollback scenarios)
    async fn delete_account_name(&mut self, owner: &PublicKey) -> Result<(), BlockchainError>;
}

// ============================================================================
// ConfigurableTnsProvider - Test Infrastructure
// ============================================================================

/// Configurable in-memory TNS provider for testing
#[derive(Default)]
pub struct ConfigurableTnsProvider {
    // ===== Name Registration State =====
    /// name_hash -> owner (PublicKey/CompressedPublicKey as bytes)
    name_to_owner: std::collections::HashMap<Hash, [u8; 32]>,
    /// owner (PublicKey/CompressedPublicKey as bytes) -> name_hash
    owner_to_name: std::collections::HashMap<[u8; 32], Hash>,

    // ===== Fault Injection Flags =====
    /// Fail on name registration
    fail_on_register: bool,
    /// Fail on name lookup
    fail_on_lookup: bool,
    /// Fail on delete operations
    fail_on_delete: bool,

    // ===== Configuration =====
    /// Simulated mainnet flag (for address formatting in tests)
    is_mainnet: bool,
}

impl ConfigurableTnsProvider {
    /// Create a new empty provider
    pub fn new() -> Self {
        Self::default()
    }

    // ===== Builder Methods for Initial State =====

    /// Register a name with an owner
    pub fn with_registered_name(mut self, name_hash: Hash, owner: &PublicKey) -> Self {
        let owner_bytes = *owner.as_bytes();
        self.name_to_owner.insert(name_hash.clone(), owner_bytes);
        self.owner_to_name.insert(owner_bytes, name_hash);
        self
    }

    /// Set mainnet flag
    pub fn with_mainnet(mut self, is_mainnet: bool) -> Self {
        self.is_mainnet = is_mainnet;
        self
    }

    // ===== Builder Methods for Fault Injection =====

    /// Enable fault injection: name registration will fail
    pub fn fail_on_register(mut self) -> Self {
        self.fail_on_register = true;
        self
    }

    /// Enable fault injection: name lookups will fail
    pub fn fail_on_lookup(mut self) -> Self {
        self.fail_on_lookup = true;
        self
    }

    /// Enable fault injection: delete operations will fail
    pub fn fail_on_delete(mut self) -> Self {
        self.fail_on_delete = true;
        self
    }

    // ===== Helper Methods =====

    /// Get current registered name count (for testing)
    pub fn name_count(&self) -> usize {
        self.name_to_owner.len()
    }

    /// Check if provider is configured for mainnet
    pub fn is_mainnet(&self) -> bool {
        self.is_mainnet
    }
}

#[async_trait]
impl TnsProvider for ConfigurableTnsProvider {
    // ===== Bootstrap Sync =====

    async fn list_all_tns_names(
        &self,
        skip: usize,
        limit: usize,
    ) -> Result<Vec<(Hash, PublicKey)>, BlockchainError> {
        if self.fail_on_lookup {
            return Err(BlockchainError::Unknown);
        }
        let mut out = Vec::new();
        let mut skipped = 0usize;
        for (name_hash, owner_bytes) in &self.name_to_owner {
            if skipped < skip {
                skipped += 1;
                continue;
            }
            let pubkey = PublicKey::from_bytes(owner_bytes)
                .map_err(|_| BlockchainError::InvalidPublicKey)?;
            out.push((name_hash.clone(), pubkey));
            if out.len() >= limit {
                break;
            }
        }
        Ok(out)
    }

    // ===== Name Registration =====

    async fn is_name_registered(&self, name_hash: &Hash) -> Result<bool, BlockchainError> {
        if self.fail_on_lookup {
            return Err(BlockchainError::Unknown);
        }
        Ok(self.name_to_owner.contains_key(name_hash))
    }

    async fn get_name_owner(&self, name_hash: &Hash) -> Result<Option<PublicKey>, BlockchainError> {
        if self.fail_on_lookup {
            return Err(BlockchainError::Unknown);
        }
        match self.name_to_owner.get(name_hash) {
            Some(bytes) => {
                let pubkey =
                    PublicKey::from_bytes(bytes).map_err(|_| BlockchainError::InvalidPublicKey)?;
                Ok(Some(pubkey))
            }
            None => Ok(None),
        }
    }

    async fn account_has_name(&self, owner: &PublicKey) -> Result<bool, BlockchainError> {
        if self.fail_on_lookup {
            return Err(BlockchainError::Unknown);
        }
        let owner_bytes = *owner.as_bytes();
        Ok(self.owner_to_name.contains_key(&owner_bytes))
    }

    async fn get_account_name(&self, owner: &PublicKey) -> Result<Option<Hash>, BlockchainError> {
        if self.fail_on_lookup {
            return Err(BlockchainError::Unknown);
        }
        let owner_bytes = *owner.as_bytes();
        Ok(self.owner_to_name.get(&owner_bytes).cloned())
    }

    async fn register_name(
        &mut self,
        name_hash: Hash,
        owner: PublicKey,
    ) -> Result<(), BlockchainError> {
        if self.fail_on_register {
            return Err(BlockchainError::Unknown);
        }

        if self.name_to_owner.contains_key(&name_hash) {
            return Err(BlockchainError::TnsNameAlreadyRegistered);
        }

        let owner_bytes = *owner.as_bytes();
        if self.owner_to_name.contains_key(&owner_bytes) {
            return Err(BlockchainError::TnsAccountAlreadyHasName);
        }

        self.name_to_owner.insert(name_hash.clone(), owner_bytes);
        self.owner_to_name.insert(owner_bytes, name_hash);

        Ok(())
    }

    // ===== Administrative Operations =====

    async fn delete_name_registration(&mut self, name_hash: &Hash) -> Result<(), BlockchainError> {
        if self.fail_on_delete {
            return Err(BlockchainError::Unknown);
        }

        if let Some(owner_bytes) = self.name_to_owner.remove(name_hash) {
            self.owner_to_name.remove(&owner_bytes);
        }

        Ok(())
    }

    async fn delete_account_name(&mut self, owner: &PublicKey) -> Result<(), BlockchainError> {
        if self.fail_on_delete {
            return Err(BlockchainError::Unknown);
        }

        let owner_bytes = *owner.as_bytes();

        if let Some(name_hash) = self.owner_to_name.remove(&owner_bytes) {
            self.name_to_owner.remove(&name_hash);
        }

        Ok(())
    }
}

// ============================================================================
// Test Helper Functions
// ============================================================================

/// Create a test Hash from a single byte value
pub fn test_hash(value: u8) -> Hash {
    let mut bytes = [0u8; 32];
    bytes[0] = value;
    Hash::new(bytes)
}

/// Create a test PublicKey (CompressedPublicKey) from a seed value
pub fn test_public_key(seed: u8) -> PublicKey {
    use tos_common::crypto::elgamal::KeyPair;

    let mut key_bytes = [0u8; 32];
    for i in 0..32 {
        key_bytes[i] = seed.wrapping_add(i as u8).wrapping_mul(17);
    }

    match PublicKey::from_bytes(&key_bytes) {
        Ok(key) => key,
        Err(_) => {
            let keypair = KeyPair::new();
            keypair.get_public_key().compress()
        }
    }
}
