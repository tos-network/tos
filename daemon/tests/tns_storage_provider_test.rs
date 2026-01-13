#![allow(clippy::disallowed_methods)]

//! TNS Storage Provider Unit Tests
//!
//! Tests for the ConfigurableTnsProvider implementation.
//! These tests verify all storage provider operations including:
//! - Name registration (CRUD operations)
//! - Ephemeral message storage (CRUD operations)
//! - Error conditions and fault injection
//! - Invariant verification

use tos_daemon::core::storage::{
    test_hash, test_message, test_message_id, test_public_key, ConfigurableTnsProvider, TnsProvider,
};

// ============================================================================
// Name Registration Tests - Happy Path
// ============================================================================

#[tokio::test]
async fn test_register_name_success() {
    let mut provider = ConfigurableTnsProvider::new();
    let name_hash = test_hash(1);
    let owner = test_public_key(1);

    let result = provider
        .register_name(name_hash.clone(), owner.clone())
        .await;
    assert!(result.is_ok());

    // Verify registration
    assert!(provider.is_name_registered(&name_hash).await.unwrap());
    assert!(provider.account_has_name(&owner).await.unwrap());
    assert_eq!(provider.name_count(), 1);
}

#[tokio::test]
async fn test_register_name_multiple_different_names() {
    let mut provider = ConfigurableTnsProvider::new();

    // Register multiple different names for different owners
    for i in 1..=5 {
        let name_hash = test_hash(i);
        let owner = test_public_key(i);
        provider.register_name(name_hash, owner).await.unwrap();
    }

    assert_eq!(provider.name_count(), 5);
}

#[tokio::test]
async fn test_get_name_owner_exists() {
    let mut provider = ConfigurableTnsProvider::new();
    let name_hash = test_hash(1);
    let owner = test_public_key(1);

    provider
        .register_name(name_hash.clone(), owner.clone())
        .await
        .unwrap();

    let result = provider.get_name_owner(&name_hash).await.unwrap();
    assert!(result.is_some());
    assert_eq!(result.unwrap().as_bytes(), owner.as_bytes());
}

#[tokio::test]
async fn test_get_name_owner_not_exists() {
    let provider = ConfigurableTnsProvider::new();
    let name_hash = test_hash(1);

    let result = provider.get_name_owner(&name_hash).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_account_name_exists() {
    let mut provider = ConfigurableTnsProvider::new();
    let name_hash = test_hash(1);
    let owner = test_public_key(1);

    provider
        .register_name(name_hash.clone(), owner.clone())
        .await
        .unwrap();

    let result = provider.get_account_name(&owner).await.unwrap();
    assert!(result.is_some());
    assert_eq!(result.unwrap(), name_hash);
}

#[tokio::test]
async fn test_get_account_name_not_exists() {
    let provider = ConfigurableTnsProvider::new();
    let owner = test_public_key(1);

    let result = provider.get_account_name(&owner).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_is_name_registered_true() {
    let mut provider = ConfigurableTnsProvider::new();
    let name_hash = test_hash(1);
    let owner = test_public_key(1);

    provider
        .register_name(name_hash.clone(), owner)
        .await
        .unwrap();

    assert!(provider.is_name_registered(&name_hash).await.unwrap());
}

#[tokio::test]
async fn test_is_name_registered_false() {
    let provider = ConfigurableTnsProvider::new();
    let name_hash = test_hash(1);

    assert!(!provider.is_name_registered(&name_hash).await.unwrap());
}

#[tokio::test]
async fn test_account_has_name_true() {
    let mut provider = ConfigurableTnsProvider::new();
    let name_hash = test_hash(1);
    let owner = test_public_key(1);

    provider
        .register_name(name_hash, owner.clone())
        .await
        .unwrap();

    assert!(provider.account_has_name(&owner).await.unwrap());
}

#[tokio::test]
async fn test_account_has_name_false() {
    let provider = ConfigurableTnsProvider::new();
    let owner = test_public_key(1);

    assert!(!provider.account_has_name(&owner).await.unwrap());
}

// ============================================================================
// Name Registration Tests - Error Cases
// ============================================================================

#[tokio::test]
async fn test_register_name_already_registered() {
    let mut provider = ConfigurableTnsProvider::new();
    let name_hash = test_hash(1);
    let owner1 = test_public_key(1);
    let owner2 = test_public_key(2);

    // First registration should succeed
    provider
        .register_name(name_hash.clone(), owner1)
        .await
        .unwrap();

    // Second registration with same name should fail
    let result = provider.register_name(name_hash, owner2).await;
    assert!(result.is_err());
    // Check error type
    let err = result.unwrap_err();
    assert!(format!("{:?}", err).contains("TnsNameAlreadyRegistered"));
}

#[tokio::test]
async fn test_register_name_account_already_has_name() {
    let mut provider = ConfigurableTnsProvider::new();
    let name_hash1 = test_hash(1);
    let name_hash2 = test_hash(2);
    let owner = test_public_key(1);

    // First registration should succeed
    provider
        .register_name(name_hash1, owner.clone())
        .await
        .unwrap();

    // Second registration with same owner should fail
    let result = provider.register_name(name_hash2, owner).await;
    assert!(result.is_err());
    // Check error type
    let err = result.unwrap_err();
    assert!(format!("{:?}", err).contains("TnsAccountAlreadyHasName"));
}

// ============================================================================
// Name Registration Tests - Delete Operations
// ============================================================================

#[tokio::test]
async fn test_delete_name_registration_exists() {
    let mut provider = ConfigurableTnsProvider::new();
    let name_hash = test_hash(1);
    let owner = test_public_key(1);

    provider
        .register_name(name_hash.clone(), owner.clone())
        .await
        .unwrap();
    assert_eq!(provider.name_count(), 1);

    // Delete the registration
    provider.delete_name_registration(&name_hash).await.unwrap();

    // Verify deletion
    assert!(!provider.is_name_registered(&name_hash).await.unwrap());
    assert!(!provider.account_has_name(&owner).await.unwrap());
    assert_eq!(provider.name_count(), 0);
}

#[tokio::test]
async fn test_delete_name_registration_not_exists() {
    let mut provider = ConfigurableTnsProvider::new();
    let name_hash = test_hash(1);

    // Deleting non-existent registration should not error (idempotent)
    let result = provider.delete_name_registration(&name_hash).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_delete_account_name_exists() {
    let mut provider = ConfigurableTnsProvider::new();
    let name_hash = test_hash(1);
    let owner = test_public_key(1);

    provider
        .register_name(name_hash.clone(), owner.clone())
        .await
        .unwrap();
    assert_eq!(provider.name_count(), 1);

    // Delete by account
    provider.delete_account_name(&owner).await.unwrap();

    // Verify deletion
    assert!(!provider.is_name_registered(&name_hash).await.unwrap());
    assert!(!provider.account_has_name(&owner).await.unwrap());
    assert_eq!(provider.name_count(), 0);
}

#[tokio::test]
async fn test_delete_account_name_not_exists() {
    let mut provider = ConfigurableTnsProvider::new();
    let owner = test_public_key(1);

    // Deleting non-existent account name should not error (idempotent)
    let result = provider.delete_account_name(&owner).await;
    assert!(result.is_ok());
}

// ============================================================================
// Ephemeral Message Tests - Happy Path
// ============================================================================

#[tokio::test]
async fn test_store_message_success() {
    let mut provider = ConfigurableTnsProvider::new();
    let sender_hash = test_hash(1);
    let recipient_hash = test_hash(2);
    let message = test_message(sender_hash.clone(), recipient_hash.clone(), 1, 1000, 100);
    let message_id = test_message_id(&sender_hash, &recipient_hash, 1);

    let result = provider
        .store_ephemeral_message(message_id.clone(), message)
        .await;
    assert!(result.is_ok());
    assert_eq!(provider.message_count(), 1);
}

#[tokio::test]
async fn test_store_message_multiple() {
    let mut provider = ConfigurableTnsProvider::new();
    let sender_hash = test_hash(1);
    let recipient_hash = test_hash(2);

    for nonce in 1..=5 {
        let message = test_message(
            sender_hash.clone(),
            recipient_hash.clone(),
            nonce,
            1000,
            100,
        );
        let message_id = test_message_id(&sender_hash, &recipient_hash, nonce);
        provider
            .store_ephemeral_message(message_id, message)
            .await
            .unwrap();
    }

    assert_eq!(provider.message_count(), 5);
}

#[tokio::test]
async fn test_is_message_id_used_true() {
    let mut provider = ConfigurableTnsProvider::new();
    let sender_hash = test_hash(1);
    let recipient_hash = test_hash(2);
    let message = test_message(sender_hash.clone(), recipient_hash.clone(), 1, 1000, 100);
    let message_id = test_message_id(&sender_hash, &recipient_hash, 1);

    provider
        .store_ephemeral_message(message_id.clone(), message)
        .await
        .unwrap();

    assert!(provider.is_message_id_used(&message_id).await.unwrap());
}

#[tokio::test]
async fn test_is_message_id_used_false() {
    let provider = ConfigurableTnsProvider::new();
    let message_id = test_hash(1);

    assert!(!provider.is_message_id_used(&message_id).await.unwrap());
}

#[tokio::test]
async fn test_get_ephemeral_message_exists() {
    let mut provider = ConfigurableTnsProvider::new();
    let sender_hash = test_hash(1);
    let recipient_hash = test_hash(2);
    let message = test_message(sender_hash.clone(), recipient_hash.clone(), 1, 1000, 100);
    let message_id = test_message_id(&sender_hash, &recipient_hash, 1);

    provider
        .store_ephemeral_message(message_id.clone(), message.clone())
        .await
        .unwrap();

    let result = provider.get_ephemeral_message(&message_id).await.unwrap();
    assert!(result.is_some());
    let retrieved = result.unwrap();
    assert_eq!(retrieved.sender_name_hash, sender_hash);
    assert_eq!(retrieved.recipient_name_hash, recipient_hash);
    assert_eq!(retrieved.message_nonce, 1);
}

#[tokio::test]
async fn test_get_ephemeral_message_not_exists() {
    let provider = ConfigurableTnsProvider::new();
    let message_id = test_hash(1);

    let result = provider.get_ephemeral_message(&message_id).await.unwrap();
    assert!(result.is_none());
}

// ============================================================================
// Ephemeral Message Tests - Error Cases
// ============================================================================

#[tokio::test]
async fn test_store_message_duplicate_id() {
    let mut provider = ConfigurableTnsProvider::new();
    let sender_hash = test_hash(1);
    let recipient_hash = test_hash(2);
    let message1 = test_message(sender_hash.clone(), recipient_hash.clone(), 1, 1000, 100);
    let message2 = test_message(sender_hash.clone(), recipient_hash.clone(), 1, 2000, 200);
    let message_id = test_message_id(&sender_hash, &recipient_hash, 1);

    // First store should succeed
    provider
        .store_ephemeral_message(message_id.clone(), message1)
        .await
        .unwrap();

    // Second store with same ID should fail (replay attack)
    let result = provider.store_ephemeral_message(message_id, message2).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(format!("{:?}", err).contains("TnsMessageIdAlreadyUsed"));
}

// ============================================================================
// Ephemeral Message Tests - Query Operations
// ============================================================================

#[tokio::test]
async fn test_get_messages_for_recipient_basic() {
    let mut provider = ConfigurableTnsProvider::new();
    let sender_hash = test_hash(1);
    let recipient_hash = test_hash(2);
    let current_topoheight = 100u64;

    // Store 3 messages for the recipient
    for nonce in 1..=3 {
        let message = test_message(
            sender_hash.clone(),
            recipient_hash.clone(),
            nonce,
            1000,
            current_topoheight,
        );
        let message_id = test_message_id(&sender_hash, &recipient_hash, nonce);
        provider
            .store_ephemeral_message(message_id, message)
            .await
            .unwrap();
    }

    let results = provider
        .get_messages_for_recipient(&recipient_hash, 0, 10, current_topoheight)
        .await
        .unwrap();

    assert_eq!(results.len(), 3);
}

#[tokio::test]
async fn test_get_messages_for_recipient_pagination() {
    let mut provider = ConfigurableTnsProvider::new();
    let sender_hash = test_hash(1);
    let recipient_hash = test_hash(2);
    let current_topoheight = 100u64;

    // Store 5 messages
    for nonce in 1..=5 {
        let message = test_message(
            sender_hash.clone(),
            recipient_hash.clone(),
            nonce,
            1000,
            current_topoheight,
        );
        let message_id = test_message_id(&sender_hash, &recipient_hash, nonce);
        provider
            .store_ephemeral_message(message_id, message)
            .await
            .unwrap();
    }

    // Get first 2 messages
    let page1 = provider
        .get_messages_for_recipient(&recipient_hash, 0, 2, current_topoheight)
        .await
        .unwrap();
    assert_eq!(page1.len(), 2);

    // Get next 2 messages
    let page2 = provider
        .get_messages_for_recipient(&recipient_hash, 2, 2, current_topoheight)
        .await
        .unwrap();
    assert_eq!(page2.len(), 2);

    // Get last message
    let page3 = provider
        .get_messages_for_recipient(&recipient_hash, 4, 2, current_topoheight)
        .await
        .unwrap();
    assert_eq!(page3.len(), 1);
}

#[tokio::test]
async fn test_get_messages_for_recipient_filters_expired() {
    let mut provider = ConfigurableTnsProvider::new();
    let sender_hash = test_hash(1);
    let recipient_hash = test_hash(2);

    // Store message that expires at topoheight 200 (stored at 100, TTL 100)
    let message = test_message(sender_hash.clone(), recipient_hash.clone(), 1, 100, 100);
    let message_id = test_message_id(&sender_hash, &recipient_hash, 1);
    provider
        .store_ephemeral_message(message_id, message)
        .await
        .unwrap();

    // Query at topoheight 150 (message not expired)
    let results = provider
        .get_messages_for_recipient(&recipient_hash, 0, 10, 150)
        .await
        .unwrap();
    assert_eq!(results.len(), 1);

    // Query at topoheight 200 (message expired - expiry is <= current)
    let results = provider
        .get_messages_for_recipient(&recipient_hash, 0, 10, 200)
        .await
        .unwrap();
    assert_eq!(results.len(), 0);
}

#[tokio::test]
async fn test_get_messages_for_recipient_empty() {
    let provider = ConfigurableTnsProvider::new();
    let recipient_hash = test_hash(1);

    let results = provider
        .get_messages_for_recipient(&recipient_hash, 0, 10, 100)
        .await
        .unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_get_messages_for_recipient_limit_zero() {
    let mut provider = ConfigurableTnsProvider::new();
    let sender_hash = test_hash(1);
    let recipient_hash = test_hash(2);
    let message = test_message(sender_hash.clone(), recipient_hash.clone(), 1, 1000, 100);
    let message_id = test_message_id(&sender_hash, &recipient_hash, 1);
    provider
        .store_ephemeral_message(message_id, message)
        .await
        .unwrap();

    // Limit 0 should return empty
    let results = provider
        .get_messages_for_recipient(&recipient_hash, 0, 0, 100)
        .await
        .unwrap();
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_count_messages_for_recipient_basic() {
    let mut provider = ConfigurableTnsProvider::new();
    let sender_hash = test_hash(1);
    let recipient_hash = test_hash(2);
    let current_topoheight = 100u64;

    // Store 5 messages
    for nonce in 1..=5 {
        let message = test_message(
            sender_hash.clone(),
            recipient_hash.clone(),
            nonce,
            1000,
            current_topoheight,
        );
        let message_id = test_message_id(&sender_hash, &recipient_hash, nonce);
        provider
            .store_ephemeral_message(message_id, message)
            .await
            .unwrap();
    }

    let count = provider
        .count_messages_for_recipient(&recipient_hash, current_topoheight)
        .await
        .unwrap();
    assert_eq!(count, 5);
}

#[tokio::test]
async fn test_count_messages_for_recipient_filters_expired() {
    let mut provider = ConfigurableTnsProvider::new();
    let sender_hash = test_hash(1);
    let recipient_hash = test_hash(2);

    // Store 2 messages: one that expires at 200, one at 300
    let message1 = test_message(sender_hash.clone(), recipient_hash.clone(), 1, 100, 100);
    let message2 = test_message(sender_hash.clone(), recipient_hash.clone(), 2, 200, 100);
    let message_id1 = test_message_id(&sender_hash, &recipient_hash, 1);
    let message_id2 = test_message_id(&sender_hash, &recipient_hash, 2);
    provider
        .store_ephemeral_message(message_id1, message1)
        .await
        .unwrap();
    provider
        .store_ephemeral_message(message_id2, message2)
        .await
        .unwrap();

    // At topoheight 150, both are valid
    let count = provider
        .count_messages_for_recipient(&recipient_hash, 150)
        .await
        .unwrap();
    assert_eq!(count, 2);

    // At topoheight 200, only message2 is valid (expiry > 200)
    let count = provider
        .count_messages_for_recipient(&recipient_hash, 200)
        .await
        .unwrap();
    assert_eq!(count, 1);

    // At topoheight 300, none are valid
    let count = provider
        .count_messages_for_recipient(&recipient_hash, 300)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

// ============================================================================
// Ephemeral Message Tests - Delete and Cleanup
// ============================================================================

#[tokio::test]
async fn test_delete_ephemeral_message_exists() {
    let mut provider = ConfigurableTnsProvider::new();
    let sender_hash = test_hash(1);
    let recipient_hash = test_hash(2);
    let message = test_message(sender_hash.clone(), recipient_hash.clone(), 1, 1000, 100);
    let message_id = test_message_id(&sender_hash, &recipient_hash, 1);

    provider
        .store_ephemeral_message(message_id.clone(), message)
        .await
        .unwrap();
    assert_eq!(provider.message_count(), 1);

    provider
        .delete_ephemeral_message(&message_id)
        .await
        .unwrap();

    assert_eq!(provider.message_count(), 0);
    assert!(!provider.is_message_id_used(&message_id).await.unwrap());
}

#[tokio::test]
async fn test_delete_ephemeral_message_not_exists() {
    let mut provider = ConfigurableTnsProvider::new();
    let message_id = test_hash(1);

    // Should not error (idempotent)
    let result = provider.delete_ephemeral_message(&message_id).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_cleanup_expired_messages_basic() {
    let mut provider = ConfigurableTnsProvider::new();
    let sender_hash = test_hash(1);
    let recipient_hash = test_hash(2);

    // Store 3 messages with different expiry times
    // Message 1: expires at 200 (stored at 100, TTL 100)
    let msg1 = test_message(sender_hash.clone(), recipient_hash.clone(), 1, 100, 100);
    let id1 = test_message_id(&sender_hash, &recipient_hash, 1);
    provider
        .store_ephemeral_message(id1.clone(), msg1)
        .await
        .unwrap();

    // Message 2: expires at 300 (stored at 100, TTL 200)
    let msg2 = test_message(sender_hash.clone(), recipient_hash.clone(), 2, 200, 100);
    let id2 = test_message_id(&sender_hash, &recipient_hash, 2);
    provider
        .store_ephemeral_message(id2.clone(), msg2)
        .await
        .unwrap();

    // Message 3: expires at 400 (stored at 100, TTL 300)
    let msg3 = test_message(sender_hash.clone(), recipient_hash.clone(), 3, 300, 100);
    let id3 = test_message_id(&sender_hash, &recipient_hash, 3);
    provider
        .store_ephemeral_message(id3.clone(), msg3)
        .await
        .unwrap();

    assert_eq!(provider.message_count(), 3);

    // Cleanup at topoheight 200 should delete message 1 (expiry <= 200)
    let deleted = provider.cleanup_expired_messages(200).await.unwrap();
    assert_eq!(deleted, 1);
    assert_eq!(provider.message_count(), 2);

    // Cleanup at topoheight 400 should delete messages 2 and 3
    let deleted = provider.cleanup_expired_messages(400).await.unwrap();
    assert_eq!(deleted, 2);
    assert_eq!(provider.message_count(), 0);
}

#[tokio::test]
async fn test_cleanup_expired_messages_none_expired() {
    let mut provider = ConfigurableTnsProvider::new();
    let sender_hash = test_hash(1);
    let recipient_hash = test_hash(2);

    // Store message that expires at 200
    let message = test_message(sender_hash.clone(), recipient_hash.clone(), 1, 100, 100);
    let message_id = test_message_id(&sender_hash, &recipient_hash, 1);
    provider
        .store_ephemeral_message(message_id, message)
        .await
        .unwrap();

    // Cleanup at topoheight 150 should delete nothing
    let deleted = provider.cleanup_expired_messages(150).await.unwrap();
    assert_eq!(deleted, 0);
    assert_eq!(provider.message_count(), 1);
}

// ============================================================================
// Fault Injection Tests
// ============================================================================

#[tokio::test]
async fn test_fault_injection_register() {
    let mut provider = ConfigurableTnsProvider::new().fail_on_register();
    let name_hash = test_hash(1);
    let owner = test_public_key(1);

    let result = provider.register_name(name_hash, owner).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_fault_injection_lookup() {
    let provider = ConfigurableTnsProvider::new().fail_on_lookup();
    let name_hash = test_hash(1);

    let result = provider.is_name_registered(&name_hash).await;
    assert!(result.is_err());

    let result = provider.get_name_owner(&name_hash).await;
    assert!(result.is_err());

    let owner = test_public_key(1);
    let result = provider.account_has_name(&owner).await;
    assert!(result.is_err());

    let result = provider.get_account_name(&owner).await;
    assert!(result.is_err());

    let message_id = test_hash(1);
    let result = provider.is_message_id_used(&message_id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_fault_injection_store_message() {
    let mut provider = ConfigurableTnsProvider::new().fail_on_store_message();
    let sender_hash = test_hash(1);
    let recipient_hash = test_hash(2);
    let message = test_message(sender_hash.clone(), recipient_hash.clone(), 1, 1000, 100);
    let message_id = test_message_id(&sender_hash, &recipient_hash, 1);

    let result = provider.store_ephemeral_message(message_id, message).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_fault_injection_get_message() {
    let provider = ConfigurableTnsProvider::new().fail_on_get_message();
    let message_id = test_hash(1);
    let recipient_hash = test_hash(2);

    let result = provider.get_ephemeral_message(&message_id).await;
    assert!(result.is_err());

    let result = provider
        .get_messages_for_recipient(&recipient_hash, 0, 10, 100)
        .await;
    assert!(result.is_err());

    let result = provider
        .count_messages_for_recipient(&recipient_hash, 100)
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_fault_injection_delete() {
    let mut provider = ConfigurableTnsProvider::new().fail_on_delete();
    let name_hash = test_hash(1);
    let owner = test_public_key(1);
    let message_id = test_hash(2);

    let result = provider.delete_name_registration(&name_hash).await;
    assert!(result.is_err());

    let result = provider.delete_account_name(&owner).await;
    assert!(result.is_err());

    let result = provider.delete_ephemeral_message(&message_id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_fault_injection_cleanup() {
    let mut provider = ConfigurableTnsProvider::new().fail_on_cleanup();

    let result = provider.cleanup_expired_messages(100).await;
    assert!(result.is_err());
}

// ============================================================================
// Builder Pattern Tests
// ============================================================================

#[tokio::test]
async fn test_builder_with_registered_name() {
    let name_hash = test_hash(1);
    let owner = test_public_key(1);

    let provider = ConfigurableTnsProvider::new().with_registered_name(name_hash.clone(), &owner);

    assert!(provider.is_name_registered(&name_hash).await.unwrap());
    assert!(provider.account_has_name(&owner).await.unwrap());
    assert_eq!(provider.name_count(), 1);
}

#[tokio::test]
async fn test_builder_with_stored_message() {
    let sender_hash = test_hash(1);
    let recipient_hash = test_hash(2);
    let message = test_message(sender_hash.clone(), recipient_hash.clone(), 1, 1000, 100);
    let message_id = test_message_id(&sender_hash, &recipient_hash, 1);

    let provider = ConfigurableTnsProvider::new().with_stored_message(message_id.clone(), message);

    assert!(provider.is_message_id_used(&message_id).await.unwrap());
    assert_eq!(provider.message_count(), 1);
}

#[tokio::test]
async fn test_builder_chaining() {
    let name_hash = test_hash(1);
    let owner = test_public_key(1);
    let sender_hash = test_hash(2);
    let recipient_hash = test_hash(3);
    let message = test_message(sender_hash.clone(), recipient_hash.clone(), 1, 1000, 100);
    let message_id = test_message_id(&sender_hash, &recipient_hash, 1);

    let provider = ConfigurableTnsProvider::new()
        .with_registered_name(name_hash.clone(), &owner)
        .with_stored_message(message_id.clone(), message)
        .with_mainnet(true);

    assert!(provider.is_name_registered(&name_hash).await.unwrap());
    assert!(provider.is_message_id_used(&message_id).await.unwrap());
    assert!(provider.is_mainnet());
}

// ============================================================================
// Invariant Tests
// ============================================================================

#[tokio::test]
async fn test_invariant_one_name_per_account() {
    let mut provider = ConfigurableTnsProvider::new();
    let name_hash1 = test_hash(1);
    let name_hash2 = test_hash(2);
    let owner = test_public_key(1);

    // Register first name
    provider
        .register_name(name_hash1.clone(), owner.clone())
        .await
        .unwrap();

    // Attempt to register second name for same account should fail
    let result = provider.register_name(name_hash2, owner).await;
    assert!(result.is_err());

    // Original registration should still exist
    assert!(provider.is_name_registered(&name_hash1).await.unwrap());
    assert_eq!(provider.name_count(), 1);
}

#[tokio::test]
async fn test_invariant_one_account_per_name() {
    let mut provider = ConfigurableTnsProvider::new();
    let name_hash = test_hash(1);
    let owner1 = test_public_key(1);
    let owner2 = test_public_key(2);

    // Register name for first owner
    provider
        .register_name(name_hash.clone(), owner1.clone())
        .await
        .unwrap();

    // Attempt to register same name for different owner should fail
    let result = provider.register_name(name_hash.clone(), owner2).await;
    assert!(result.is_err());

    // Original owner should still own the name
    let retrieved_owner = provider.get_name_owner(&name_hash).await.unwrap().unwrap();
    assert_eq!(retrieved_owner.as_bytes(), owner1.as_bytes());
}

#[tokio::test]
async fn test_invariant_bidirectional_consistency() {
    let mut provider = ConfigurableTnsProvider::new();
    let name_hash = test_hash(1);
    let owner = test_public_key(1);

    // Register
    provider
        .register_name(name_hash.clone(), owner.clone())
        .await
        .unwrap();

    // Verify bidirectional lookup
    let owner_from_name = provider.get_name_owner(&name_hash).await.unwrap().unwrap();
    let name_from_owner = provider.get_account_name(&owner).await.unwrap().unwrap();

    assert_eq!(owner_from_name.as_bytes(), owner.as_bytes());
    assert_eq!(name_from_owner, name_hash);

    // Delete and verify both directions are cleared
    provider.delete_name_registration(&name_hash).await.unwrap();

    assert!(provider.get_name_owner(&name_hash).await.unwrap().is_none());
    assert!(provider.get_account_name(&owner).await.unwrap().is_none());
}

#[tokio::test]
async fn test_invariant_message_expiry_calculation() {
    let mut provider = ConfigurableTnsProvider::new();
    let sender_hash = test_hash(1);
    let recipient_hash = test_hash(2);
    let stored_at = 100u64;
    let ttl = 500u32;

    let message = test_message(
        sender_hash.clone(),
        recipient_hash.clone(),
        1,
        ttl,
        stored_at,
    );
    let message_id = test_message_id(&sender_hash, &recipient_hash, 1);

    provider
        .store_ephemeral_message(message_id.clone(), message)
        .await
        .unwrap();

    let retrieved = provider
        .get_ephemeral_message(&message_id)
        .await
        .unwrap()
        .unwrap();

    // Verify expiry calculation
    assert_eq!(retrieved.stored_topoheight, stored_at);
    assert_eq!(retrieved.expiry_topoheight, stored_at + ttl as u64);
}

#[tokio::test]
async fn test_invariant_replay_protection() {
    let mut provider = ConfigurableTnsProvider::new();
    let sender_hash = test_hash(1);
    let recipient_hash = test_hash(2);
    let message = test_message(sender_hash.clone(), recipient_hash.clone(), 1, 1000, 100);
    let message_id = test_message_id(&sender_hash, &recipient_hash, 1);

    // Store message
    provider
        .store_ephemeral_message(message_id.clone(), message.clone())
        .await
        .unwrap();

    // Verify message ID is marked as used
    assert!(provider.is_message_id_used(&message_id).await.unwrap());

    // Attempt to store with same ID should fail
    let result = provider
        .store_ephemeral_message(message_id.clone(), message)
        .await;
    assert!(result.is_err());

    // Delete message
    provider
        .delete_ephemeral_message(&message_id)
        .await
        .unwrap();

    // After deletion, message ID should no longer be marked as used
    assert!(!provider.is_message_id_used(&message_id).await.unwrap());
}
