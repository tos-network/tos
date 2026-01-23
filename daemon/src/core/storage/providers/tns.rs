// TNS (TOS Name Service) storage provider trait

use crate::core::error::BlockchainError;
use async_trait::async_trait;
use tos_common::{
    block::TopoHeight,
    crypto::{Hash, PublicKey},
    serializer::{Reader, ReaderError, Serializer, Writer},
};

/// Ephemeral message stored in the database
#[derive(Clone, Debug)]
pub struct StoredEphemeralMessage {
    /// Sender's name hash
    pub sender_name_hash: Hash,
    /// Recipient's name hash
    pub recipient_name_hash: Hash,
    /// Message nonce for replay protection
    pub message_nonce: u64,
    /// TTL in blocks
    pub ttl_blocks: u32,
    /// Encrypted message content
    pub encrypted_content: Vec<u8>,
    /// Receiver handle for decryption
    pub receiver_handle: [u8; 32],
    /// Topoheight when message was stored
    pub stored_topoheight: TopoHeight,
    /// Expiry topoheight (stored_topoheight + ttl_blocks)
    pub expiry_topoheight: TopoHeight,
}

/// Maximum encrypted content size (same as MAX_ENCRYPTED_SIZE from tns constants)
const MAX_ENCRYPTED_SIZE: usize = 188;

/// Message index entry for O(1) lookup by message_id
/// Stores recipient_hash to allow direct key construction
#[derive(Clone, Debug)]
pub struct MessageIndexEntry {
    /// Recipient's name hash (needed to construct the message key)
    pub recipient_hash: Hash,
    /// Expiry topoheight (for quick expiry checks without loading full message)
    pub expiry_topoheight: TopoHeight,
}

impl Serializer for MessageIndexEntry {
    fn write(&self, writer: &mut Writer) {
        self.recipient_hash.write(writer);
        writer.write_u64(&self.expiry_topoheight);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let recipient_hash = Hash::read(reader)?;
        let expiry_topoheight = reader.read_u64()?;
        Ok(Self {
            recipient_hash,
            expiry_topoheight,
        })
    }

    fn size(&self) -> usize {
        32 + 8 // Hash (32) + TopoHeight (8)
    }
}

impl Serializer for StoredEphemeralMessage {
    fn write(&self, writer: &mut Writer) {
        self.sender_name_hash.write(writer);
        self.recipient_name_hash.write(writer);
        writer.write_u64(&self.message_nonce);
        writer.write_u32(&self.ttl_blocks);
        // Content length as u16 (max 188 bytes)
        // Debug assert to catch programming errors - content should be validated before storage
        debug_assert!(
            self.encrypted_content.len() <= MAX_ENCRYPTED_SIZE,
            "Encrypted content length {} exceeds max {}",
            self.encrypted_content.len(),
            MAX_ENCRYPTED_SIZE
        );
        // Saturating cast to prevent overflow/truncation in release builds
        let len = self.encrypted_content.len().min(u16::MAX as usize) as u16;
        writer.write_u16(len);
        writer.write_bytes(&self.encrypted_content[..len as usize]);
        writer.write_bytes(&self.receiver_handle);
        writer.write_u64(&self.stored_topoheight);
        writer.write_u64(&self.expiry_topoheight);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let sender_name_hash = Hash::read(reader)?;
        let recipient_name_hash = Hash::read(reader)?;
        let message_nonce = reader.read_u64()?;
        let ttl_blocks = reader.read_u32()?;
        let content_len = reader.read_u16()? as usize;

        // Validate content length to prevent large allocations from corrupted data
        if content_len > MAX_ENCRYPTED_SIZE {
            return Err(ReaderError::InvalidSize);
        }

        let encrypted_content = reader.read_bytes(content_len)?;
        let receiver_handle = reader.read_bytes_32()?;
        let stored_topoheight = reader.read_u64()?;
        let expiry_topoheight = reader.read_u64()?;

        Ok(Self {
            sender_name_hash,
            recipient_name_hash,
            message_nonce,
            ttl_blocks,
            encrypted_content,
            receiver_handle,
            stored_topoheight,
            expiry_topoheight,
        })
    }

    fn size(&self) -> usize {
        // 32 + 32 + 8 + 4 + 2 + content_len + 32 + 8 + 8 = 126 + content_len
        32 + 32 + 8 + 4 + 2 + self.encrypted_content.len() + 32 + 8 + 8
    }
}

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

    // ===== Ephemeral Messages =====

    /// Check if a message ID has been used (for replay protection)
    async fn is_message_id_used(&self, message_id: &Hash) -> Result<bool, BlockchainError>;

    /// Store an ephemeral message
    ///
    /// # Arguments
    /// * `message_id` - Unique message identifier (hash of sender + recipient + nonce)
    /// * `message` - The stored ephemeral message data
    ///
    /// # Errors
    /// * Returns error if message_id is already used
    async fn store_ephemeral_message(
        &mut self,
        message_id: Hash,
        message: StoredEphemeralMessage,
    ) -> Result<(), BlockchainError>;

    /// Get an ephemeral message by ID
    async fn get_ephemeral_message(
        &self,
        message_id: &Hash,
    ) -> Result<Option<StoredEphemeralMessage>, BlockchainError>;

    /// Get messages for a recipient (paginated, filtered by expiry)
    ///
    /// # Arguments
    /// * `recipient_name_hash` - The recipient's name hash
    /// * `offset` - Pagination offset
    /// * `limit` - Maximum number of results
    /// * `current_topoheight` - Current topoheight for filtering expired messages
    ///
    /// # Returns
    /// Vector of (message_id, message) tuples for non-expired messages only
    async fn get_messages_for_recipient(
        &self,
        recipient_name_hash: &Hash,
        offset: u32,
        limit: u32,
        current_topoheight: TopoHeight,
    ) -> Result<Vec<(Hash, StoredEphemeralMessage)>, BlockchainError>;

    /// Count messages for a recipient (efficient count without loading all data)
    ///
    /// # Arguments
    /// * `recipient_name_hash` - The recipient's name hash
    /// * `current_topoheight` - Current topoheight for filtering expired messages
    ///
    /// # Returns
    /// Number of non-expired messages for the recipient
    async fn count_messages_for_recipient(
        &self,
        recipient_name_hash: &Hash,
        current_topoheight: TopoHeight,
    ) -> Result<u64, BlockchainError>;

    /// Delete an ephemeral message (for cleanup after expiry)
    async fn delete_ephemeral_message(&mut self, message_id: &Hash) -> Result<(), BlockchainError>;

    /// Delete expired messages up to a given topoheight
    ///
    /// # Arguments
    /// * `current_topoheight` - Current chain topoheight
    ///
    /// # Returns
    /// Number of messages deleted
    async fn cleanup_expired_messages(
        &mut self,
        current_topoheight: TopoHeight,
    ) -> Result<u64, BlockchainError>;

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
///
/// This provider allows:
/// - Pre-configuring initial state with builder methods
/// - Fault injection for testing error paths
/// - In-memory storage for fast test execution
///
/// # Example
/// ```rust
/// use tos_common::crypto::{Hash, KeyPair};
/// use tos_daemon::core::storage::ConfigurableTnsProvider;
///
/// let name_hash = Hash::zero();
/// let keypair = KeyPair::new();
/// let owner = keypair.get_public_key().compress();
///
/// let provider = ConfigurableTnsProvider::new()
///     .with_registered_name(name_hash, &owner)
///     .fail_on_register();
/// ```
#[derive(Default)]
pub struct ConfigurableTnsProvider {
    // ===== Name Registration State =====
    /// name_hash -> owner (PublicKey/CompressedPublicKey as bytes)
    name_to_owner: std::collections::HashMap<Hash, [u8; 32]>,
    /// owner (PublicKey/CompressedPublicKey as bytes) -> name_hash
    owner_to_name: std::collections::HashMap<[u8; 32], Hash>,

    // ===== Ephemeral Message State =====
    /// message_id -> StoredEphemeralMessage
    messages: std::collections::HashMap<Hash, StoredEphemeralMessage>,
    /// message_id -> MessageIndexEntry (for replay protection)
    message_index: std::collections::HashMap<Hash, MessageIndexEntry>,

    // ===== Fault Injection Flags =====
    /// Fail on name registration
    fail_on_register: bool,
    /// Fail on name lookup
    fail_on_lookup: bool,
    /// Fail on message storage
    fail_on_store_message: bool,
    /// Fail on message retrieval
    fail_on_get_message: bool,
    /// Fail on message cleanup
    fail_on_cleanup: bool,
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
    /// Note: PublicKey is an alias for CompressedPublicKey in this codebase
    pub fn with_registered_name(mut self, name_hash: Hash, owner: &PublicKey) -> Self {
        let owner_bytes = *owner.as_bytes();
        self.name_to_owner.insert(name_hash.clone(), owner_bytes);
        self.owner_to_name.insert(owner_bytes, name_hash);
        self
    }

    /// Store an ephemeral message
    pub fn with_stored_message(
        mut self,
        message_id: Hash,
        message: StoredEphemeralMessage,
    ) -> Self {
        let index_entry = MessageIndexEntry {
            recipient_hash: message.recipient_name_hash.clone(),
            expiry_topoheight: message.expiry_topoheight,
        };
        self.message_index.insert(message_id.clone(), index_entry);
        self.messages.insert(message_id, message);
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

    /// Enable fault injection: message storage will fail
    pub fn fail_on_store_message(mut self) -> Self {
        self.fail_on_store_message = true;
        self
    }

    /// Enable fault injection: message retrieval will fail
    pub fn fail_on_get_message(mut self) -> Self {
        self.fail_on_get_message = true;
        self
    }

    /// Enable fault injection: cleanup will fail
    pub fn fail_on_cleanup(mut self) -> Self {
        self.fail_on_cleanup = true;
        self
    }

    /// Enable fault injection: delete operations will fail
    pub fn fail_on_delete(mut self) -> Self {
        self.fail_on_delete = true;
        self
    }

    // ===== Helper Methods =====

    /// Get current message count (for testing)
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

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
                // PublicKey is alias for CompressedPublicKey
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

        // Check if name is already registered
        if self.name_to_owner.contains_key(&name_hash) {
            return Err(BlockchainError::TnsNameAlreadyRegistered);
        }

        // Check if account already has a name
        let owner_bytes = *owner.as_bytes();
        if self.owner_to_name.contains_key(&owner_bytes) {
            return Err(BlockchainError::TnsAccountAlreadyHasName);
        }

        // Store bidirectional mapping
        self.name_to_owner.insert(name_hash.clone(), owner_bytes);
        self.owner_to_name.insert(owner_bytes, name_hash);

        Ok(())
    }

    // ===== Ephemeral Messages =====

    async fn is_message_id_used(&self, message_id: &Hash) -> Result<bool, BlockchainError> {
        if self.fail_on_lookup {
            return Err(BlockchainError::Unknown);
        }
        Ok(self.message_index.contains_key(message_id))
    }

    async fn store_ephemeral_message(
        &mut self,
        message_id: Hash,
        message: StoredEphemeralMessage,
    ) -> Result<(), BlockchainError> {
        if self.fail_on_store_message {
            return Err(BlockchainError::Unknown);
        }

        // Check for replay attack
        if self.message_index.contains_key(&message_id) {
            return Err(BlockchainError::TnsMessageIdAlreadyUsed);
        }

        // Store index entry
        let index_entry = MessageIndexEntry {
            recipient_hash: message.recipient_name_hash.clone(),
            expiry_topoheight: message.expiry_topoheight,
        };
        self.message_index.insert(message_id.clone(), index_entry);

        // Store message
        self.messages.insert(message_id, message);

        Ok(())
    }

    async fn get_ephemeral_message(
        &self,
        message_id: &Hash,
    ) -> Result<Option<StoredEphemeralMessage>, BlockchainError> {
        if self.fail_on_get_message {
            return Err(BlockchainError::Unknown);
        }
        Ok(self.messages.get(message_id).cloned())
    }

    async fn get_messages_for_recipient(
        &self,
        recipient_name_hash: &Hash,
        offset: u32,
        limit: u32,
        current_topoheight: TopoHeight,
    ) -> Result<Vec<(Hash, StoredEphemeralMessage)>, BlockchainError> {
        if self.fail_on_get_message {
            return Err(BlockchainError::Unknown);
        }

        if limit == 0 {
            return Ok(Vec::new());
        }

        let mut results = Vec::new();
        let mut skipped = 0u32;

        for (msg_id, msg) in &self.messages {
            // Filter by recipient
            if &msg.recipient_name_hash != recipient_name_hash {
                continue;
            }

            // Filter out expired messages
            if msg.expiry_topoheight <= current_topoheight {
                continue;
            }

            // Handle offset
            if skipped < offset {
                skipped = skipped.saturating_add(1);
                continue;
            }

            results.push((msg_id.clone(), msg.clone()));

            if results.len() >= limit as usize {
                break;
            }
        }

        Ok(results)
    }

    async fn count_messages_for_recipient(
        &self,
        recipient_name_hash: &Hash,
        current_topoheight: TopoHeight,
    ) -> Result<u64, BlockchainError> {
        if self.fail_on_get_message {
            return Err(BlockchainError::Unknown);
        }

        const MAX_COUNT: u64 = 10100;
        let mut count = 0u64;

        for msg in self.messages.values() {
            if &msg.recipient_name_hash == recipient_name_hash
                && msg.expiry_topoheight > current_topoheight
            {
                count = count.saturating_add(1);
                if count >= MAX_COUNT {
                    break;
                }
            }
        }

        Ok(count)
    }

    async fn delete_ephemeral_message(&mut self, message_id: &Hash) -> Result<(), BlockchainError> {
        if self.fail_on_delete {
            return Err(BlockchainError::Unknown);
        }

        self.messages.remove(message_id);
        self.message_index.remove(message_id);

        Ok(())
    }

    async fn cleanup_expired_messages(
        &mut self,
        current_topoheight: TopoHeight,
    ) -> Result<u64, BlockchainError> {
        if self.fail_on_cleanup {
            return Err(BlockchainError::Unknown);
        }

        const MAX_CLEANUP_PER_CYCLE: u64 = 1000;
        let mut deleted = 0u64;

        // Collect expired message IDs
        let expired_ids: Vec<Hash> = self
            .message_index
            .iter()
            .filter(|(_, entry)| entry.expiry_topoheight <= current_topoheight)
            .map(|(id, _)| id.clone())
            .take(MAX_CLEANUP_PER_CYCLE as usize)
            .collect();

        // Delete expired messages
        for msg_id in expired_ids {
            self.messages.remove(&msg_id);
            self.message_index.remove(&msg_id);
            deleted = deleted.saturating_add(1);
        }

        Ok(deleted)
    }

    // ===== Administrative Operations =====

    async fn delete_name_registration(&mut self, name_hash: &Hash) -> Result<(), BlockchainError> {
        if self.fail_on_delete {
            return Err(BlockchainError::Unknown);
        }

        // Remove owner -> name mapping if exists
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

        // Remove name -> owner mapping if exists
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
/// Note: This creates a deterministic but valid compressed curve point
pub fn test_public_key(seed: u8) -> PublicKey {
    use tos_common::crypto::elgamal::KeyPair;

    // For testing, we generate deterministic keys by seeding a pseudo-random generator
    // Note: KeyPair::new() uses OsRng, so for deterministic testing we use from_bytes
    // For simplicity, we just create different keys for different seeds
    // by using the seed to modify a base key

    // Create a deterministic 32-byte array from the seed
    let mut key_bytes = [0u8; 32];
    for i in 0..32 {
        key_bytes[i] = seed.wrapping_add(i as u8).wrapping_mul(17);
    }

    // Use CompressedPublicKey::from_bytes which may fail for invalid points
    // If it fails, we fall back to a known valid key
    match PublicKey::from_bytes(&key_bytes) {
        Ok(key) => key,
        Err(_) => {
            // Generate a valid key using KeyPair (this is random, but works for testing)
            let keypair = KeyPair::new();
            keypair.get_public_key().compress()
        }
    }
}

/// Create a test StoredEphemeralMessage
pub fn test_message(
    sender_hash: Hash,
    recipient_hash: Hash,
    nonce: u64,
    ttl: u32,
    stored_at: TopoHeight,
) -> StoredEphemeralMessage {
    StoredEphemeralMessage {
        sender_name_hash: sender_hash,
        recipient_name_hash: recipient_hash,
        message_nonce: nonce,
        ttl_blocks: ttl,
        encrypted_content: vec![1, 2, 3, 4],
        receiver_handle: [0u8; 32],
        stored_topoheight: stored_at,
        expiry_topoheight: stored_at.saturating_add(ttl as u64),
    }
}

/// Compute test message ID (similar to compute_message_id in verify/tns.rs)
/// Uses a deterministic hash computation for testing purposes
pub fn test_message_id(sender_hash: &Hash, recipient_hash: &Hash, nonce: u64) -> Hash {
    // Create input data (same as actual implementation: sender || recipient || nonce)
    let mut data = Vec::with_capacity(72);
    data.extend_from_slice(sender_hash.as_bytes());
    data.extend_from_slice(recipient_hash.as_bytes());
    data.extend_from_slice(&nonce.to_le_bytes());

    // Create a deterministic hash from the input data for testing purposes
    let mut hash_bytes = [0u8; 32];
    // XOR the data into the hash to create a deterministic output
    for (i, byte) in data.iter().enumerate() {
        hash_bytes[i % 32] ^= byte;
    }
    // Add some entropy based on position
    for i in 0..32 {
        hash_bytes[i] = hash_bytes[i].wrapping_add(i as u8);
    }
    Hash::new(hash_bytes)
}
