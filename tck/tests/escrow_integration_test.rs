mod common;

use common::create_test_storage;
use tos_common::config::TOS_ASSET;
use tos_common::crypto::{Hash, KeyPair, PublicKey};
use tos_common::escrow::{EscrowAccount, EscrowState};
use tos_daemon::core::storage::EscrowProvider;

fn make_public_key() -> PublicKey {
    KeyPair::new().get_public_key().compress()
}

fn make_hash(seed: u8) -> Hash {
    Hash::new([seed; 32])
}

fn make_escrow(id: Hash, payer: PublicKey, payee: PublicKey, task_id: &str) -> EscrowAccount {
    EscrowAccount {
        id,
        task_id: task_id.to_string(),
        payer,
        payee,
        amount: 1_000,
        total_amount: 1_000,
        released_amount: 0,
        refunded_amount: 0,
        pending_release_amount: None,
        challenge_deposit: 0,
        asset: TOS_ASSET,
        state: EscrowState::Funded,
        dispute_id: None,
        dispute_round: None,
        challenge_window: 10,
        challenge_deposit_bps: 0,
        optimistic_release: false,
        release_requested_at: None,
        created_at: 1,
        updated_at: 1,
        timeout_at: 100,
        timeout_blocks: 100,
        arbitration_config: None,
        dispute: None,
        appeal: None,
        resolutions: Vec::new(),
    }
}

#[tokio::test]
async fn test_escrow_set_get_and_list() {
    let storage = create_test_storage().await;
    let payer = make_public_key();
    let payee = make_public_key();

    let escrow_id = make_hash(1);
    let escrow = make_escrow(escrow_id.clone(), payer.clone(), payee.clone(), "task-1");

    {
        let mut storage_write = storage.write().await;
        storage_write.set_escrow(&escrow).await.unwrap();
    }

    let stored = storage.read().await.get_escrow(&escrow_id).await.unwrap();
    assert!(stored.is_some());
    assert_eq!(stored.unwrap().task_id, "task-1");

    let escrows_by_payer = storage
        .read()
        .await
        .get_escrows_by_payer(&payer, 0, 10)
        .await
        .unwrap();
    assert_eq!(escrows_by_payer.len(), 1);

    let escrows_by_payee = storage
        .read()
        .await
        .get_escrows_by_payee(&payee, 0, 10)
        .await
        .unwrap();
    assert_eq!(escrows_by_payee.len(), 1);

    let escrows_by_task = storage
        .read()
        .await
        .get_escrows_by_task_id("task-1", 0, 10)
        .await
        .unwrap();
    assert_eq!(escrows_by_task.len(), 1);
}

#[tokio::test]
async fn test_escrow_history_and_pending_releases() {
    let storage = create_test_storage().await;
    let payer = make_public_key();
    let payee = make_public_key();

    let escrow_id = make_hash(2);
    let escrow = make_escrow(escrow_id.clone(), payer, payee, "task-2");

    let tx_hash = make_hash(9);
    {
        let mut storage_write = storage.write().await;
        storage_write.set_escrow(&escrow).await.unwrap();
        storage_write
            .add_escrow_history(&escrow_id, 42, &tx_hash)
            .await
            .unwrap();
        storage_write
            .add_pending_release(50, &escrow_id)
            .await
            .unwrap();
    }

    let history = storage
        .read()
        .await
        .list_escrow_history(&escrow_id, 0, 10)
        .await
        .unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].0, 42);
    assert_eq!(history[0].1, tx_hash);

    let pending = storage
        .read()
        .await
        .list_pending_releases(100, 10)
        .await
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].0, 50);
    assert_eq!(pending[0].1, escrow_id);

    {
        let mut storage_write = storage.write().await;
        storage_write
            .remove_pending_release(50, &escrow_id)
            .await
            .unwrap();
        storage_write
            .remove_escrow_history(&escrow_id, 42, &tx_hash)
            .await
            .unwrap();
    }

    let pending = storage
        .read()
        .await
        .list_pending_releases(100, 10)
        .await
        .unwrap();
    assert!(pending.is_empty());

    let history = storage
        .read()
        .await
        .list_escrow_history(&escrow_id, 0, 10)
        .await
        .unwrap();
    assert!(history.is_empty());
}
