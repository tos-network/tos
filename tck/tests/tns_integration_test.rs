mod common;

use common::create_test_storage;
use tos_common::crypto::KeyPair;
use tos_common::tns::{normalize_name, tns_name_hash};
use tos_daemon::core::storage::TnsProvider;

fn make_public_key() -> tos_common::crypto::PublicKey {
    KeyPair::new().get_public_key().compress()
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
