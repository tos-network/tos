mod common;

use common::create_test_storage;
use tos_common::arbitration::{ArbitrationRequestKey, ArbitrationRoundKey};
use tos_common::crypto::{Hash, Signature};
use tos_common::transaction::CommitArbitrationOpenPayload;
use tos_crypto::curve25519_dalek::Scalar;
use tos_daemon::core::storage::ArbitrationCommitProvider;

fn make_hash(seed: u8) -> Hash {
    Hash::new([seed; 32])
}

fn zero_signature() -> Signature {
    Signature::new(Scalar::ZERO, Scalar::ZERO)
}

fn make_open_payload(round: u32, seed: u8) -> CommitArbitrationOpenPayload {
    CommitArbitrationOpenPayload {
        escrow_id: make_hash(seed),
        dispute_id: make_hash(seed + 1),
        round,
        request_id: make_hash(seed + 2),
        arbitration_open_hash: make_hash(seed + 3),
        opener_signature: zero_signature(),
        arbitration_open_payload: vec![seed],
    }
}

#[tokio::test]
async fn test_list_arbitration_opens_pagination() {
    let storage = create_test_storage().await;

    let open1 = make_open_payload(1, 10);
    let open2 = make_open_payload(1, 20);

    let round_key1 = ArbitrationRoundKey {
        escrow_id: open1.escrow_id.clone(),
        dispute_id: open1.dispute_id.clone(),
        round: open1.round,
    };
    let request_key1 = ArbitrationRequestKey {
        request_id: open1.request_id.clone(),
    };

    let round_key2 = ArbitrationRoundKey {
        escrow_id: open2.escrow_id.clone(),
        dispute_id: open2.dispute_id.clone(),
        round: open2.round,
    };
    let request_key2 = ArbitrationRequestKey {
        request_id: open2.request_id.clone(),
    };

    {
        let mut storage_write = storage.write().await;
        storage_write
            .set_commit_arbitration_open(&round_key1, &request_key1, &open1)
            .await
            .unwrap();
        storage_write
            .set_commit_arbitration_open(&round_key2, &request_key2, &open2)
            .await
            .unwrap();
    }

    let first_page = storage
        .read()
        .await
        .list_all_arbitration_opens(0, 1)
        .await
        .unwrap();
    assert_eq!(first_page.len(), 1);

    let second_page = storage
        .read()
        .await
        .list_all_arbitration_opens(1, 1)
        .await
        .unwrap();
    assert_eq!(second_page.len(), 1);
}

#[tokio::test]
async fn test_list_votes_empty() {
    let storage = create_test_storage().await;
    let request_id = make_hash(99);

    let votes = storage
        .read()
        .await
        .list_commit_juror_votes(&request_id)
        .await
        .unwrap();
    assert!(votes.is_empty());
}
