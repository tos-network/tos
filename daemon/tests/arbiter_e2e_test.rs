#![allow(clippy::disallowed_methods)]

mod a2a_apply_state;

use std::sync::Arc;

use a2a_apply_state::{TestApplyState, TestError};
use tos_common::transaction::builder::UnsignedTransaction;
use tos_common::transaction::verify::BlockchainVerificationState;
use tos_common::{
    arbitration::{ArbiterStatus, ExpertiseDomain},
    config::{MIN_ARBITER_STAKE, TOS_ASSET},
    crypto::{Hash, Hashable, KeyPair},
    transaction::{
        FeeType, Reference, RegisterArbiterPayload, TransactionType, TxVersion,
        UpdateArbiterPayload,
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
        Some(ArbiterStatus::Suspended),
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
    assert_eq!(arbiter_state.status, ArbiterStatus::Suspended);

    let deactivate_payload =
        UpdateArbiterPayload::new(None, None, None, None, None, None, None, true);
    let deactivate_tx = UnsignedTransaction::new_with_fee_type(
        TxVersion::T0,
        0,
        arbiter_pub.clone(),
        TransactionType::UpdateArbiter(deactivate_payload),
        0,
        FeeType::TOS,
        2,
        reference,
    )
    .finalize(&arbiter);
    let deactivate_hash = deactivate_tx.hash();
    let deactivate_tx = Arc::new(deactivate_tx);
    deactivate_tx
        .apply_with_partial_verify(&deactivate_hash, &mut state)
        .await
        .map_err(|_| TestError::Unsupported)?;

    let removed = state.get_arbiter(&arbiter_pub).await?;
    assert!(removed.is_none());

    let balance = state
        .get_balance(&arbiter_pub, &TOS_ASSET)
        .expect("arbiter balance");
    assert_eq!(balance, initial_balance);

    Ok(())
}
