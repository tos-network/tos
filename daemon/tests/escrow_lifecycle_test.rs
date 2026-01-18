#![allow(clippy::disallowed_methods)]

mod a2a_apply_state;

use std::sync::Arc;

use a2a_apply_state::{TestApplyState, TestError};
use tos_common::transaction::builder::UnsignedTransaction;
use tos_common::transaction::verify::BlockchainVerificationState;
use tos_common::{
    config::{COIN_VALUE, TOS_ASSET},
    crypto::{Hash, Hashable, KeyPair},
    escrow::EscrowState,
    transaction::{
        CreateEscrowPayload, DepositEscrowPayload, FeeType, Reference, RefundEscrowPayload,
        TransactionType, TxVersion,
    },
};

#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_escrow_lifecycle_create_deposit_refund() -> Result<(), TestError> {
    let payer = KeyPair::new();
    let payee = KeyPair::new();
    let payer_pub = payer.get_public_key().compress();
    let payee_pub = payee.get_public_key().compress();

    let initial_balance = 1_000 * COIN_VALUE;
    let mut state = TestApplyState::new(1);
    state.insert_account(payer_pub.clone(), initial_balance, 0);
    state.insert_account(payee_pub.clone(), 0, 0);

    let reference = Reference {
        topoheight: 0,
        hash: Hash::zero(),
    };

    let create_payload = CreateEscrowPayload {
        task_id: "task-1".to_string(),
        provider: payee_pub.clone(),
        amount: 200 * COIN_VALUE,
        asset: TOS_ASSET.clone(),
        timeout_blocks: 100,
        challenge_window: 5,
        challenge_deposit_bps: 100,
        optimistic_release: false,
        arbitration_config: None,
        metadata: None,
    };

    let create_tx = UnsignedTransaction::new_with_fee_type(
        TxVersion::T0,
        0,
        payer_pub.clone(),
        TransactionType::CreateEscrow(create_payload),
        0,
        FeeType::TOS,
        0,
        reference.clone(),
    )
    .finalize(&payer);
    let create_hash = create_tx.hash();
    let create_tx = Arc::new(create_tx);
    create_tx
        .apply_with_partial_verify(&create_hash, &mut state)
        .await
        .map_err(|_| TestError::Unsupported)?;

    let deposit_payload = DepositEscrowPayload {
        escrow_id: create_hash.clone(),
        amount: 50 * COIN_VALUE,
    };
    let deposit_tx = UnsignedTransaction::new_with_fee_type(
        TxVersion::T0,
        0,
        payer_pub.clone(),
        TransactionType::DepositEscrow(deposit_payload),
        0,
        FeeType::TOS,
        1,
        reference.clone(),
    )
    .finalize(&payer);
    let deposit_hash = deposit_tx.hash();
    let deposit_tx = Arc::new(deposit_tx);
    deposit_tx
        .apply_with_partial_verify(&deposit_hash, &mut state)
        .await
        .map_err(|_| TestError::Unsupported)?;

    let refund_payload = RefundEscrowPayload {
        escrow_id: create_hash.clone(),
        amount: 100 * COIN_VALUE,
        reason: Some("client_cancel".to_string()),
    };
    let refund_tx = UnsignedTransaction::new_with_fee_type(
        TxVersion::T0,
        0,
        payee_pub.clone(),
        TransactionType::RefundEscrow(refund_payload),
        0,
        FeeType::TOS,
        0,
        reference,
    )
    .finalize(&payee);
    let refund_hash = refund_tx.hash();
    let refund_tx = Arc::new(refund_tx);
    refund_tx
        .apply_with_partial_verify(&refund_hash, &mut state)
        .await
        .map_err(|_| TestError::Unsupported)?;

    let escrow = state
        .get_escrow(&create_hash)
        .await?
        .expect("escrow exists");
    assert_eq!(escrow.amount, 150 * COIN_VALUE);
    assert_eq!(escrow.total_amount, 250 * COIN_VALUE);
    assert_eq!(escrow.refunded_amount, 100 * COIN_VALUE);
    assert_eq!(escrow.released_amount, 0);
    assert_eq!(escrow.state, EscrowState::Funded);

    let balance = state
        .get_balance(&payer_pub, &TOS_ASSET)
        .expect("payer balance");
    assert_eq!(balance, initial_balance - 150 * COIN_VALUE);

    Ok(())
}
