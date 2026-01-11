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
pub trait TnsProvider {
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
