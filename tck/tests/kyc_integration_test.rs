mod common;

use common::create_test_storage;
use tos_common::crypto::{Hash, KeyPair, PublicKey};
use tos_common::kyc::{KycData, KycStatus};
use tos_daemon::core::storage::KycProvider;

fn make_public_key() -> PublicKey {
    KeyPair::new().get_public_key().compress()
}

fn make_hash(seed: u8) -> Hash {
    Hash::new([seed; 32])
}

#[tokio::test]
async fn test_set_get_and_validate_kyc() {
    let storage = create_test_storage().await;
    let user = make_public_key();
    let committee_id = make_hash(1);
    let tx_hash = make_hash(2);

    let kyc = KycData::new(31, 1_000, make_hash(3));

    {
        let mut storage_write = storage.write().await;
        storage_write
            .set_kyc(&user, kyc.clone(), &committee_id, 10, &tx_hash)
            .await
            .unwrap();
    }

    let has_kyc = storage.read().await.has_kyc(&user).await.unwrap();
    assert!(has_kyc);

    let stored = storage.read().await.get_kyc(&user).await.unwrap();
    assert!(stored.is_some());
    assert_eq!(stored.unwrap(), kyc);

    let is_valid = storage
        .read()
        .await
        .is_kyc_valid(&user, 1_000 + 10)
        .await
        .unwrap();
    assert!(is_valid);

    let meets_level = storage
        .read()
        .await
        .meets_kyc_level(&user, 7, 1_000 + 10)
        .await
        .unwrap();
    assert!(meets_level);
}

#[tokio::test]
async fn test_kyc_status_update_and_revoke() {
    let storage = create_test_storage().await;
    let user = make_public_key();
    let committee_id = make_hash(4);
    let tx_hash = make_hash(5);

    let kyc = KycData::new(63, 1_000, make_hash(6));

    {
        let mut storage_write = storage.write().await;
        storage_write
            .set_kyc(&user, kyc.clone(), &committee_id, 10, &tx_hash)
            .await
            .unwrap();
        storage_write
            .update_kyc_status(&user, KycStatus::Suspended, 11)
            .await
            .unwrap();
    }

    let is_valid = storage
        .read()
        .await
        .is_kyc_valid(&user, 2_000)
        .await
        .unwrap();
    assert!(!is_valid);

    {
        let mut storage_write = storage.write().await;
        storage_write
            .revoke_kyc(&user, &make_hash(7), 12, &make_hash(13))
            .await
            .unwrap();
    }

    let stored = storage.read().await.get_kyc(&user).await.unwrap();
    assert!(stored.is_some());
    assert_eq!(stored.unwrap().status, KycStatus::Revoked);
}

#[tokio::test]
async fn test_kyc_renew() {
    let storage = create_test_storage().await;
    let user = make_public_key();
    let committee_id = make_hash(8);
    let tx_hash = make_hash(9);

    let kyc = KycData::new(31, 1_000, make_hash(10));

    {
        let mut storage_write = storage.write().await;
        storage_write
            .set_kyc(&user, kyc, &committee_id, 10, &tx_hash)
            .await
            .unwrap();
        storage_write
            .renew_kyc(&user, 5_000, make_hash(11), 20, &make_hash(12))
            .await
            .unwrap();
    }

    let stored = storage.read().await.get_kyc(&user).await.unwrap().unwrap();
    assert_eq!(stored.verified_at, 5_000);
    assert_eq!(stored.data_hash, make_hash(11));
}
