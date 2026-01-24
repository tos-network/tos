mod common;

use common::create_test_storage;
use tos_common::crypto::{Hash, KeyPair, PublicKey};
use tos_common::tns::{normalize_name, tns_name_hash};
use tos_daemon::core::storage::{
    test_message, test_message_id, StoredEphemeralMessage, TnsProvider,
};

fn make_public_key() -> PublicKey {
    KeyPair::new().get_public_key().compress()
}

fn make_hash(seed: u8) -> Hash {
    Hash::new([seed; 32])
}

#[tokio::test]
async fn test_register_and_query_name() {
    let storage = create_test_storage().await;
    let owner = make_public_key();
    let name = normalize_name("alice").unwrap();
    let name_hash = tns_name_hash(&name);

    {
        let mut storage_write = storage.write().await;
        storage_write
            .register_name(name_hash.clone(), owner.clone())
            .await
            .unwrap();
    }

    let registered = storage
        .read()
        .await
        .is_name_registered(&name_hash)
        .await
        .unwrap();
    assert!(registered);

    let fetched_owner = storage
        .read()
        .await
        .get_name_owner(&name_hash)
        .await
        .unwrap();
    assert_eq!(fetched_owner, Some(owner.clone()));

    let has_name = storage.read().await.account_has_name(&owner).await.unwrap();
    assert!(has_name);

    let account_name = storage.read().await.get_account_name(&owner).await.unwrap();
    assert_eq!(account_name, Some(name_hash));
}

#[tokio::test]
async fn test_ephemeral_message_lifecycle() {
    let storage = create_test_storage().await;
    let sender_hash = make_hash(2);
    let recipient_hash = make_hash(3);

    let message_id = test_message_id(&sender_hash, &recipient_hash, 1);
    let message: StoredEphemeralMessage = test_message(sender_hash, recipient_hash, 1, 5, 100);

    {
        let mut storage_write = storage.write().await;
        storage_write
            .store_ephemeral_message(message_id.clone(), message.clone())
            .await
            .unwrap();
    }

    let found = storage
        .read()
        .await
        .get_ephemeral_message(&message_id)
        .await
        .unwrap();
    assert!(found.is_some());

    let count = storage
        .read()
        .await
        .count_messages_for_recipient(&message.recipient_name_hash, 104)
        .await
        .unwrap();
    assert_eq!(count, 1);

    let messages = storage
        .read()
        .await
        .get_messages_for_recipient(&message.recipient_name_hash, 0, 10, 104)
        .await
        .unwrap();
    assert_eq!(messages.len(), 1);

    // Cleanup at topoheight before expiry should keep message
    {
        let mut storage_write = storage.write().await;
        storage_write.cleanup_expired_messages(104).await.unwrap();
    }
    let count = storage
        .read()
        .await
        .count_messages_for_recipient(&message.recipient_name_hash, 104)
        .await
        .unwrap();
    assert_eq!(count, 1);

    // Cleanup at expiry removes message
    {
        let mut storage_write = storage.write().await;
        storage_write.cleanup_expired_messages(105).await.unwrap();
    }

    let count = storage
        .read()
        .await
        .count_messages_for_recipient(&message.recipient_name_hash, 105)
        .await
        .unwrap();
    assert_eq!(count, 0);
}
