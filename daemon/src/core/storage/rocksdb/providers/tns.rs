// TnsProvider implementation for RocksDB storage

use crate::core::{
    error::BlockchainError,
    storage::{
        rocksdb::{Column, IteratorMode, RocksStorage},
        MessageIndexEntry, NetworkProvider, StoredEphemeralMessage, TnsProvider,
    },
};
use async_trait::async_trait;
use log::trace;
use rocksdb::Direction;
use tos_common::{
    block::TopoHeight,
    crypto::{Hash, PublicKey},
};

#[async_trait]
impl TnsProvider for RocksStorage {
    // ===== Name Registration =====

    async fn is_name_registered(&self, name_hash: &Hash) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("checking if name {} is registered", name_hash);
        }
        self.contains_data(Column::TnsNameToOwner, name_hash.as_bytes())
    }

    async fn get_name_owner(&self, name_hash: &Hash) -> Result<Option<PublicKey>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("getting owner for name {}", name_hash);
        }
        self.load_optional_from_disk(Column::TnsNameToOwner, name_hash.as_bytes())
    }

    async fn account_has_name(&self, owner: &PublicKey) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "checking if account {} has a name",
                owner.as_address(self.is_mainnet())
            );
        }
        self.contains_data(Column::TnsAccountToName, owner.as_bytes())
    }

    async fn get_account_name(&self, owner: &PublicKey) -> Result<Option<Hash>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "getting name for account {}",
                owner.as_address(self.is_mainnet())
            );
        }
        self.load_optional_from_disk(Column::TnsAccountToName, owner.as_bytes())
    }

    async fn register_name(
        &mut self,
        name_hash: Hash,
        owner: PublicKey,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "registering name {} for account {}",
                name_hash,
                owner.as_address(self.is_mainnet())
            );
        }

        // Check if name is already registered
        if self.is_name_registered(&name_hash).await? {
            return Err(BlockchainError::TnsNameAlreadyRegistered);
        }

        // Check if account already has a name
        if self.account_has_name(&owner).await? {
            return Err(BlockchainError::TnsAccountAlreadyHasName);
        }

        // Store name -> owner mapping
        self.insert_into_disk(Column::TnsNameToOwner, name_hash.as_bytes(), &owner)?;

        // Store owner -> name mapping (reverse index)
        self.insert_into_disk(Column::TnsAccountToName, owner.as_bytes(), &name_hash)?;

        Ok(())
    }

    // ===== Ephemeral Messages =====

    async fn is_message_id_used(&self, message_id: &Hash) -> Result<bool, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("checking if message ID {} is used", message_id);
        }
        self.contains_data(Column::TnsMessageIdIndex, message_id.as_bytes())
    }

    async fn store_ephemeral_message(
        &mut self,
        message_id: Hash,
        message: StoredEphemeralMessage,
    ) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "storing ephemeral message {} for recipient {}",
                message_id,
                message.recipient_name_hash
            );
        }

        // Check for replay attack
        if self.is_message_id_used(&message_id).await? {
            return Err(BlockchainError::TnsMessageIdAlreadyUsed);
        }

        // Store in message ID index for replay protection and O(1) lookup
        // Value includes recipient_hash for direct key construction
        let index_entry = MessageIndexEntry {
            recipient_hash: message.recipient_name_hash.clone(),
            expiry_topoheight: message.expiry_topoheight,
        };
        self.insert_into_disk(
            Column::TnsMessageIdIndex,
            message_id.as_bytes(),
            &index_entry,
        )?;

        // Store in recipient-indexed storage
        // Key: {recipient_name_hash (32 bytes)}{message_id (32 bytes)}
        let key = Self::make_ephemeral_message_key(&message.recipient_name_hash, &message_id);
        self.insert_into_disk(Column::TnsEphemeralMessages, &key, &message)?;

        Ok(())
    }

    async fn get_ephemeral_message(
        &self,
        message_id: &Hash,
    ) -> Result<Option<StoredEphemeralMessage>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("getting ephemeral message {}", message_id);
        }

        // Load index entry which contains recipient_hash for O(1) lookup
        let index_entry: Option<MessageIndexEntry> =
            self.load_optional_from_disk(Column::TnsMessageIdIndex, message_id.as_bytes())?;

        let index_entry = match index_entry {
            Some(entry) => entry,
            None => return Ok(None),
        };

        // Direct lookup using recipient_hash from index
        let key = Self::make_ephemeral_message_key(&index_entry.recipient_hash, message_id);
        let message: Option<StoredEphemeralMessage> =
            self.load_optional_from_disk(Column::TnsEphemeralMessages, &key)?;

        Ok(message)
    }

    async fn get_messages_for_recipient(
        &self,
        recipient_name_hash: &Hash,
        offset: u32,
        limit: u32,
        current_topoheight: TopoHeight,
    ) -> Result<Vec<(Hash, StoredEphemeralMessage)>, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "getting messages for recipient {} (offset: {}, limit: {}, topoheight: {})",
                recipient_name_hash,
                offset,
                limit,
                current_topoheight
            );
        }

        // Early return for limit=0 edge case
        if limit == 0 {
            return Ok(Vec::new());
        }

        let mut messages = Vec::new();
        let mut skipped = 0u32;
        let mut collected = 0u32;

        // Iterate with prefix for the recipient
        let prefix = recipient_name_hash.as_bytes().to_vec();
        for result in self.iter::<Vec<u8>, StoredEphemeralMessage>(
            Column::TnsEphemeralMessages,
            IteratorMode::WithPrefix(&prefix, Direction::Forward),
        )? {
            let (key, msg) = result?;

            // Filter out expired messages
            if msg.expiry_topoheight <= current_topoheight {
                continue;
            }

            // Skip messages until we reach the offset
            if skipped < offset {
                skipped = skipped.saturating_add(1);
                continue;
            }

            // Extract message_id from key (last 32 bytes)
            if key.len() >= 64 {
                let mut msg_id_bytes = [0u8; 32];
                msg_id_bytes.copy_from_slice(&key[32..64]);
                let msg_id = Hash::new(msg_id_bytes);
                messages.push((msg_id, msg));
                collected = collected.saturating_add(1);

                if collected >= limit {
                    break;
                }
            }
        }

        Ok(messages)
    }

    async fn count_messages_for_recipient(
        &self,
        recipient_name_hash: &Hash,
        current_topoheight: TopoHeight,
    ) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "counting messages for recipient {} at topoheight {}",
                recipient_name_hash,
                current_topoheight
            );
        }

        let mut count = 0u64;

        // Iterate with prefix for the recipient, only counting non-expired messages
        let prefix = recipient_name_hash.as_bytes().to_vec();
        for result in self.iter::<Vec<u8>, StoredEphemeralMessage>(
            Column::TnsEphemeralMessages,
            IteratorMode::WithPrefix(&prefix, Direction::Forward),
        )? {
            let (_key, msg) = result?;

            // Only count non-expired messages
            if msg.expiry_topoheight > current_topoheight {
                count = count.saturating_add(1);
            }
        }

        Ok(count)
    }

    async fn delete_ephemeral_message(&mut self, message_id: &Hash) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("deleting ephemeral message {}", message_id);
        }

        // First, find the message to get the recipient hash
        if let Some(msg) = self.get_ephemeral_message(message_id).await? {
            // Delete from recipient-indexed storage
            let key = Self::make_ephemeral_message_key(&msg.recipient_name_hash, message_id);
            self.remove_from_disk(Column::TnsEphemeralMessages, &key)?;
        }

        // Delete from message ID index
        self.remove_from_disk(Column::TnsMessageIdIndex, message_id.as_bytes())?;

        Ok(())
    }

    async fn cleanup_expired_messages(
        &mut self,
        current_topoheight: TopoHeight,
    ) -> Result<u64, BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "cleaning up expired messages at topoheight {}",
                current_topoheight
            );
        }

        let mut deleted_count = 0u64;
        let mut to_delete = Vec::new();

        // Scan the message ID index for expired messages
        // Index stores MessageIndexEntry which contains (recipient_hash, expiry_topoheight)
        for result in
            self.iter::<Hash, MessageIndexEntry>(Column::TnsMessageIdIndex, IteratorMode::Start)?
        {
            let (msg_id, entry) = result?;
            if entry.expiry_topoheight <= current_topoheight {
                to_delete.push(msg_id);
            }
        }

        // Delete expired messages
        for msg_id in to_delete {
            self.delete_ephemeral_message(&msg_id).await?;
            deleted_count = deleted_count.saturating_add(1);
        }

        if log::log_enabled!(log::Level::Trace) {
            trace!("deleted {} expired messages", deleted_count);
        }

        Ok(deleted_count)
    }

    // ===== Administrative Operations =====

    async fn delete_name_registration(&mut self, name_hash: &Hash) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("deleting name registration {}", name_hash);
        }

        // Get the owner first so we can delete the reverse mapping
        if let Some(owner) = self.get_name_owner(name_hash).await? {
            // Delete owner -> name mapping
            self.remove_from_disk(Column::TnsAccountToName, owner.as_bytes())?;
        }

        // Delete name -> owner mapping
        self.remove_from_disk(Column::TnsNameToOwner, name_hash.as_bytes())?;

        Ok(())
    }

    async fn delete_account_name(&mut self, owner: &PublicKey) -> Result<(), BlockchainError> {
        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "deleting account name for {}",
                owner.as_address(self.is_mainnet())
            );
        }

        // Get the name hash first so we can delete the forward mapping
        if let Some(name_hash) = self.get_account_name(owner).await? {
            // Delete name -> owner mapping
            self.remove_from_disk(Column::TnsNameToOwner, name_hash.as_bytes())?;
        }

        // Delete owner -> name mapping
        self.remove_from_disk(Column::TnsAccountToName, owner.as_bytes())?;

        Ok(())
    }
}

impl RocksStorage {
    /// Create ephemeral message storage key: {recipient_name_hash (32 bytes)}{message_id (32 bytes)}
    fn make_ephemeral_message_key(recipient_name_hash: &Hash, message_id: &Hash) -> Vec<u8> {
        let mut key = Vec::with_capacity(64);
        key.extend_from_slice(recipient_name_hash.as_bytes());
        key.extend_from_slice(message_id.as_bytes());
        key
    }
}
