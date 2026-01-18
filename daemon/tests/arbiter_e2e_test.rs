#![allow(clippy::disallowed_methods)]

mod a2a_apply_state;

use std::sync::Arc;

use a2a_apply_state::{TestApplyState, TestError};
use tos_common::transaction::builder::UnsignedTransaction;
use tos_common::transaction::verify::BlockchainVerificationState;
use tos_common::{
    arbitration::{ArbiterStatus, ExpertiseDomain, ARBITER_COOLDOWN_TOPOHEIGHT},
    config::{MIN_ARBITER_STAKE, TOS_ASSET},
    crypto::{Hash, Hashable, KeyPair},
    kyc::{CommitteeApproval, CommitteeMember, MemberRole, SecurityCommittee},
    network::Network,
    transaction::{
        FeeType, Reference, RegisterArbiterPayload, RequestArbiterExitPayload, SlashArbiterPayload,
        TransactionType, TxVersion, UpdateArbiterPayload, WithdrawArbiterStakePayload,
    },
};

#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_arbiter_register_update_and_withdraw() -> Result<(), TestError> {
    let arbiter = KeyPair::new();
    let arbiter_pub = arbiter.get_public_key().compress();

    let initial_balance = MIN_ARBITER_STAKE + 1_000;
    let mut state = TestApplyState::new(1);
    state.insert_account(arbiter_pub.clone(), initial_balance, 0);

    let reference = Reference {
        topoheight: 0,
        hash: Hash::zero(),
    };

    let register_payload = RegisterArbiterPayload::new(
        "arbiter-one".to_string(),
        vec![ExpertiseDomain::General],
        MIN_ARBITER_STAKE,
        10,
        1_000_000,
        200,
    );

    let register_tx = UnsignedTransaction::new_with_fee_type(
        TxVersion::T0,
        0,
        arbiter_pub.clone(),
        TransactionType::RegisterArbiter(register_payload),
        0,
        FeeType::TOS,
        0,
        reference.clone(),
    )
    .finalize(&arbiter);
    let register_hash = register_tx.hash();
    let register_tx = Arc::new(register_tx);
    register_tx
        .apply_with_partial_verify(&register_hash, &mut state)
        .await
        .map_err(|_| TestError::Unsupported)?;

    let update_payload = UpdateArbiterPayload::new(
        Some("arbiter-updated".to_string()),
        Some(vec![ExpertiseDomain::Payment]),
        Some(250),
        Some(20),
        Some(2_000_000),
        Some(500),
        None,
        false,
    );

    let update_tx = UnsignedTransaction::new_with_fee_type(
        TxVersion::T0,
        0,
        arbiter_pub.clone(),
        TransactionType::UpdateArbiter(update_payload),
        0,
        FeeType::TOS,
        1,
        reference.clone(),
    )
    .finalize(&arbiter);
    let update_hash = update_tx.hash();
    let update_tx = Arc::new(update_tx);
    update_tx
        .apply_with_partial_verify(&update_hash, &mut state)
        .await
        .map_err(|_| TestError::Unsupported)?;

    let arbiter_state = state
        .get_arbiter(&arbiter_pub)
        .await?
        .expect("arbiter exists");
    assert_eq!(arbiter_state.name, "arbiter-updated");
    assert_eq!(arbiter_state.stake_amount, MIN_ARBITER_STAKE + 500);
    assert_eq!(arbiter_state.fee_basis_points, 250);
    assert_eq!(arbiter_state.status, ArbiterStatus::Active);

    let exit_payload = RequestArbiterExitPayload::new();
    let exit_tx = UnsignedTransaction::new_with_fee_type(
        TxVersion::T0,
        0,
        arbiter_pub.clone(),
        TransactionType::RequestArbiterExit(exit_payload),
        0,
        FeeType::TOS,
        2,
        reference.clone(),
    )
    .finalize(&arbiter);
    let exit_hash = exit_tx.hash();
    let exit_tx = Arc::new(exit_tx);
    exit_tx
        .apply_with_partial_verify(&exit_hash, &mut state)
        .await
        .map_err(|_| TestError::Unsupported)?;

    let exiting = state
        .get_arbiter(&arbiter_pub)
        .await?
        .expect("arbiter exists");
    assert_eq!(exiting.status, ArbiterStatus::Exiting);

    state.set_topoheight(1 + ARBITER_COOLDOWN_TOPOHEIGHT + 1);

    let withdraw_payload = WithdrawArbiterStakePayload::new(0);
    let withdraw_tx = UnsignedTransaction::new_with_fee_type(
        TxVersion::T0,
        0,
        arbiter_pub.clone(),
        TransactionType::WithdrawArbiterStake(withdraw_payload),
        0,
        FeeType::TOS,
        3,
        reference,
    )
    .finalize(&arbiter);
    let withdraw_hash = withdraw_tx.hash();
    let withdraw_tx = Arc::new(withdraw_tx);
    withdraw_tx
        .apply_with_partial_verify(&withdraw_hash, &mut state)
        .await
        .map_err(|_| TestError::Unsupported)?;

    let removed = state
        .get_arbiter(&arbiter_pub)
        .await?
        .expect("arbiter exists");
    assert_eq!(removed.status, ArbiterStatus::Removed);
    assert_eq!(removed.stake_amount, 0);

    let balance = state
        .get_balance(&arbiter_pub, &TOS_ASSET)
        .expect("arbiter balance");
    assert_eq!(balance, initial_balance);

    Ok(())
}

#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_arbiter_slash() -> Result<(), TestError> {
    let arbiter = KeyPair::new();
    let arbiter_pub = arbiter.get_public_key().compress();

    let member = KeyPair::new();
    let member_pub = member.get_public_key().compress();

    let mut state = TestApplyState::new(1);
    state.insert_account(arbiter_pub.clone(), MIN_ARBITER_STAKE, 0);
    state.insert_account(member_pub.clone(), 1_000, 0);

    let committee_member = CommitteeMember::new(
        member_pub.clone(),
        Some("member-1".to_string()),
        MemberRole::Chair,
        1,
    );
    let committee = SecurityCommittee::new_global(
        "Global Committee".to_string(),
        vec![committee_member],
        1,
        1,
        32767,
        1,
    );
    state.insert_committee(committee.clone());

    let reference = Reference {
        topoheight: 0,
        hash: Hash::zero(),
    };

    let register_payload = RegisterArbiterPayload::new(
        "arbiter-one".to_string(),
        vec![ExpertiseDomain::General],
        MIN_ARBITER_STAKE,
        10,
        1_000_000,
        200,
    );

    let register_tx = UnsignedTransaction::new_with_fee_type(
        TxVersion::T0,
        0,
        arbiter_pub.clone(),
        TransactionType::RegisterArbiter(register_payload),
        0,
        FeeType::TOS,
        0,
        reference.clone(),
    )
    .finalize(&arbiter);
    let register_hash = register_tx.hash();
    let register_tx = Arc::new(register_tx);
    register_tx
        .apply_with_partial_verify(&register_hash, &mut state)
        .await
        .map_err(|_| TestError::Unsupported)?;

    let amount = MIN_ARBITER_STAKE / 2;
    let reason_hash = Hash::new([7u8; 32]);
    let timestamp = 10u64;
    let message = CommitteeApproval::build_slash_arbiter_message(
        &Network::Devnet,
        &committee.id,
        &arbiter_pub,
        amount,
        &reason_hash,
        timestamp,
    );
    let approval = CommitteeApproval::new(member_pub.clone(), member.sign(&message), timestamp);

    let slash_payload = SlashArbiterPayload::new(
        committee.id.clone(),
        arbiter_pub.clone(),
        amount,
        reason_hash,
        vec![approval],
    );

    let slash_tx = UnsignedTransaction::new_with_fee_type(
        TxVersion::T0,
        0,
        member_pub.clone(),
        TransactionType::SlashArbiter(slash_payload),
        0,
        FeeType::TOS,
        1,
        reference,
    )
    .finalize(&member);
    let slash_hash = slash_tx.hash();
    let slash_tx = Arc::new(slash_tx);
    slash_tx
        .apply_with_partial_verify(&slash_hash, &mut state)
        .await
        .map_err(|_| TestError::Unsupported)?;

    let arbiter_state = state
        .get_arbiter(&arbiter_pub)
        .await?
        .expect("arbiter exists");
    assert_eq!(arbiter_state.stake_amount, MIN_ARBITER_STAKE - amount);
    assert_eq!(arbiter_state.total_slashed, amount);
    assert_eq!(arbiter_state.slash_count, 1);
    assert_eq!(arbiter_state.status, ArbiterStatus::Active);

    Ok(())
}
