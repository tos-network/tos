mod common;

use common::create_test_storage;
use tos_common::arbitration::{
    ArbiterAccount, ArbiterStatus, ArbitrationJurorVoteKey, ArbitrationRequestKey,
    ArbitrationRoundKey, ExpertiseDomain,
};
use tos_common::crypto::{Hash, KeyPair, PublicKey, Signature};
use tos_common::transaction::{
    CommitArbitrationOpenPayload, CommitJurorVotePayload, CommitSelectionCommitmentPayload,
    CommitVoteRequestPayload,
};
use tos_crypto::curve25519_dalek::Scalar;
use tos_daemon::core::storage::{ArbiterProvider, ArbitrationCommitProvider};

fn make_public_key() -> PublicKey {
    KeyPair::new().get_public_key().compress()
}

fn make_hash(seed: u8) -> Hash {
    Hash::new([seed; 32])
}

fn zero_signature() -> Signature {
    Signature::new(Scalar::ZERO, Scalar::ZERO)
}

fn make_arbiter(public_key: PublicKey, name: &str) -> ArbiterAccount {
    ArbiterAccount {
        public_key,
        name: name.to_string(),
        status: ArbiterStatus::Active,
        expertise: vec![ExpertiseDomain::General],
        stake_amount: 1_000,
        fee_basis_points: 50,
        min_escrow_value: 100,
        max_escrow_value: 10_000,
        reputation_score: 9_000,
        total_cases: 0,
        cases_overturned: 0,
        registered_at: 1,
        last_active_at: 1,
        pending_withdrawal: 0,
        deactivated_at: None,
        active_cases: 0,
        total_slashed: 0,
        slash_count: 0,
    }
}

#[tokio::test]
async fn test_arbiter_storage_roundtrip() {
    let storage = create_test_storage().await;
    let arbiter_key = make_public_key();
    let arbiter = make_arbiter(arbiter_key.clone(), "arbiter-one");

    {
        let mut storage_write = storage.write().await;
        storage_write.set_arbiter(&arbiter).await.unwrap();
    }

    let fetched = storage
        .read()
        .await
        .get_arbiter(&arbiter_key)
        .await
        .unwrap();
    assert!(fetched.is_some());
    assert_eq!(fetched.unwrap().name, "arbiter-one");

    let listed = storage.read().await.list_all_arbiters(0, 10).await.unwrap();
    assert_eq!(listed.len(), 1);

    {
        let mut storage_write = storage.write().await;
        storage_write.remove_arbiter(&arbiter_key).await.unwrap();
    }

    let fetched = storage
        .read()
        .await
        .get_arbiter(&arbiter_key)
        .await
        .unwrap();
    assert!(fetched.is_none());
}

#[tokio::test]
async fn test_arbitration_commit_payloads() {
    let storage = create_test_storage().await;
    let escrow_id = make_hash(1);
    let dispute_id = make_hash(2);
    let request_id = make_hash(3);
    let round_key = ArbitrationRoundKey {
        escrow_id: escrow_id.clone(),
        dispute_id: dispute_id.clone(),
        round: 1,
    };
    let request_key = ArbitrationRequestKey {
        request_id: request_id.clone(),
    };

    let open_payload = CommitArbitrationOpenPayload {
        escrow_id: escrow_id.clone(),
        dispute_id: dispute_id.clone(),
        round: 1,
        request_id: request_id.clone(),
        arbitration_open_hash: make_hash(4),
        opener_signature: zero_signature(),
        arbitration_open_payload: vec![1, 2, 3],
    };

    let vote_request = CommitVoteRequestPayload {
        request_id: request_id.clone(),
        vote_request_hash: make_hash(5),
        coordinator_signature: zero_signature(),
        vote_request_payload: vec![4, 5, 6],
    };

    let selection_commitment = CommitSelectionCommitmentPayload {
        request_id: request_id.clone(),
        selection_commitment_id: make_hash(6),
        selection_commitment_payload: vec![7, 8, 9],
    };

    let juror_key = make_public_key();
    let juror_vote_key = ArbitrationJurorVoteKey {
        request_id: request_id.clone(),
        juror_pubkey: juror_key.clone(),
    };

    let juror_vote = CommitJurorVotePayload {
        request_id: request_id.clone(),
        juror_pubkey: juror_key.clone(),
        vote_hash: make_hash(7),
        juror_signature: zero_signature(),
        vote_payload: vec![10, 11, 12],
    };

    {
        let mut storage_write = storage.write().await;
        storage_write
            .set_commit_arbitration_open(&round_key, &request_key, &open_payload)
            .await
            .unwrap();
        storage_write
            .set_commit_vote_request(&request_key, &vote_request)
            .await
            .unwrap();
        storage_write
            .set_commit_selection_commitment(&request_key, &selection_commitment)
            .await
            .unwrap();
        storage_write
            .set_commit_juror_vote(&juror_vote_key, &juror_vote)
            .await
            .unwrap();
    }

    let fetched_open = storage
        .read()
        .await
        .get_commit_arbitration_open(&round_key)
        .await
        .unwrap();
    assert!(fetched_open.is_some());

    let fetched_by_request = storage
        .read()
        .await
        .get_commit_arbitration_open_by_request(&request_key)
        .await
        .unwrap();
    assert!(fetched_by_request.is_some());

    let fetched_vote_request = storage
        .read()
        .await
        .get_commit_vote_request(&request_key)
        .await
        .unwrap();
    assert!(fetched_vote_request.is_some());

    let fetched_commitment = storage
        .read()
        .await
        .get_commit_selection_commitment(&request_key)
        .await
        .unwrap();
    assert!(fetched_commitment.is_some());

    let fetched_vote = storage
        .read()
        .await
        .get_commit_juror_vote(&juror_vote_key)
        .await
        .unwrap();
    assert!(fetched_vote.is_some());

    let votes = storage
        .read()
        .await
        .list_commit_juror_votes(&request_id)
        .await
        .unwrap();
    assert_eq!(votes.len(), 1);

    let all_opens = storage
        .read()
        .await
        .list_all_arbitration_opens(0, 10)
        .await
        .unwrap();
    assert_eq!(all_opens.len(), 1);
}
