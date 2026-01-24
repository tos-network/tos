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

fn make_escrow(
    id: Hash,
    payer: PublicKey,
    payee: PublicKey,
    task_id: &str,
    updated_at: u64,
) -> EscrowAccount {
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
        updated_at,
        timeout_at: 100,
        timeout_blocks: 100,
        arbitration_config: None,
        dispute: None,
        appeal: None,
        resolutions: Vec::new(),
    }
}

#[tokio::test]
async fn test_list_escrows_pagination() {
    let storage = create_test_storage().await;
    let payer = make_public_key();
    let payee = make_public_key();

    let first = make_escrow(make_hash(1), payer.clone(), payee.clone(), "task-1", 1);
    let second = make_escrow(make_hash(2), payer.clone(), payee.clone(), "task-2", 2);

    {
        let mut storage_write = storage.write().await;
        storage_write.set_escrow(&first).await.unwrap();
        storage_write.set_escrow(&second).await.unwrap();
    }

    let page1 = storage.read().await.list_escrows(0, 1).await.unwrap();
    assert_eq!(page1.len(), 1);

    let page2 = storage.read().await.list_escrows(1, 1).await.unwrap();
    assert_eq!(page2.len(), 1);
}

#[tokio::test]
async fn test_history_desc_order() {
    let storage = create_test_storage().await;
    let payer = make_public_key();
    let payee = make_public_key();
    let escrow_id = make_hash(9);
    let escrow = make_escrow(escrow_id.clone(), payer, payee, "task-9", 1);

    {
        let mut storage_write = storage.write().await;
        storage_write.set_escrow(&escrow).await.unwrap();
        storage_write
            .add_escrow_history(&escrow_id, 10, &make_hash(1))
            .await
            .unwrap();
        storage_write
            .add_escrow_history(&escrow_id, 20, &make_hash(2))
            .await
            .unwrap();
    }

    let desc = storage
        .read()
        .await
        .list_escrow_history_desc(&escrow_id, 0, 10)
        .await
        .unwrap();
    assert_eq!(desc.len(), 2);
    assert!(desc[0].0 > desc[1].0);
}
