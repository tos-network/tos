mod common;

use common::create_test_storage;
use tos_common::crypto::{Hash, KeyPair, PublicKey};
use tos_common::kyc::{get_validity_period_seconds, KycData};
use tos_daemon::core::storage::KycProvider;

fn make_public_key() -> PublicKey {
    KeyPair::new().get_public_key().compress()
}

fn make_hash(seed: u8) -> Hash {
    Hash::new([seed; 32])
}

#[tokio::test]
async fn test_effective_level_expired() {
    let storage = create_test_storage().await;
    let user = make_public_key();

    // Tier 1 = level 7, validity 1 year
    let verified_at = 1_000u64;
    let kyc = KycData::new(7, verified_at, make_hash(1));

    {
        let mut storage_write = storage.write().await;
        storage_write
            .set_kyc(&user, kyc, &make_hash(2), 10, &make_hash(3))
            .await
            .unwrap();
    }

    let validity = get_validity_period_seconds(1);
    let expired_at = verified_at + validity;

    let effective_level = storage
        .read()
        .await
        .get_effective_level(&user, expired_at)
        .await
        .unwrap();

    assert_eq!(effective_level, 0);
}

#[tokio::test]
async fn test_get_kyc_batch_empty() {
    let storage = create_test_storage().await;
    let user = make_public_key();

    let batch = storage
        .read()
        .await
        .get_kyc_batch(std::slice::from_ref(&user))
        .await
        .unwrap();

    assert_eq!(batch.len(), 1);
    assert!(batch[0].1.is_none());
}
