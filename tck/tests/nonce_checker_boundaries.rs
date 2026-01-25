use tempdir::TempDir;

use tos_common::account::VersionedNonce;
use tos_common::crypto::KeyPair;
use tos_common::network::Network;
use tos_daemon::core::config::RocksDBConfig;
use tos_daemon::core::nonce_checker::NonceChecker;
use tos_daemon::core::storage::{AccountProvider, NonceProvider, RocksStorage};

#[tokio::test]
async fn test_nonce_checker_sequence_and_undo() {
    let temp_dir = TempDir::new("tck_nonce_checker").expect("tempdir");
    let mut storage = RocksStorage::new(
        &temp_dir.path().to_string_lossy(),
        Network::Devnet,
        &RocksDBConfig::default(),
    );

    let keypair = KeyPair::new();
    let public_key = keypair.get_public_key().compress();

    storage
        .set_account_registration_topoheight(&public_key, 0)
        .await
        .expect("register account");
    storage
        .set_last_nonce_to(&public_key, 0, &VersionedNonce::new(5, Some(0)))
        .await
        .expect("seed nonce");

    let mut checker = NonceChecker::new();

    assert!(checker
        .use_nonce(&storage, &public_key, 5, 0)
        .await
        .expect("use nonce 5"));
    assert!(checker
        .use_nonce(&storage, &public_key, 6, 0)
        .await
        .expect("use nonce 6"));
    assert!(!checker
        .use_nonce(&storage, &public_key, 6, 0)
        .await
        .expect("reject duplicate nonce"));
    assert!(!checker
        .use_nonce(&storage, &public_key, 8, 0)
        .await
        .expect("reject skipped nonce"));

    checker.undo_nonce(&public_key, 6);
    assert_eq!(checker.get_new_nonce(&public_key, false).unwrap(), 6);
}

#[tokio::test]
async fn test_nonce_checker_rejects_lower_than_initial() {
    let temp_dir = TempDir::new("tck_nonce_checker_low").expect("tempdir");
    let mut storage = RocksStorage::new(
        &temp_dir.path().to_string_lossy(),
        Network::Devnet,
        &RocksDBConfig::default(),
    );

    let keypair = KeyPair::new();
    let public_key = keypair.get_public_key().compress();

    storage
        .set_account_registration_topoheight(&public_key, 0)
        .await
        .expect("register account");
    storage
        .set_last_nonce_to(&public_key, 0, &VersionedNonce::new(3, Some(0)))
        .await
        .expect("seed nonce");

    let mut checker = NonceChecker::new();
    assert!(!checker
        .use_nonce(&storage, &public_key, 2, 0)
        .await
        .expect("reject lower nonce"));
}
