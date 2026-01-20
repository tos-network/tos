#![allow(clippy::disallowed_methods)]

mod a2a_apply_state;

use std::{borrow::Cow, sync::Arc};

use a2a_apply_state::{TestApplyState, TestError};
use tos_common::transaction::builder::UnsignedTransaction;
use tos_common::transaction::verify::{BlockchainApplyState, BlockchainVerificationState};
use tos_common::{
    config::{COIN_VALUE, TOS_ASSET},
    crypto::{Hash, Hashable, KeyPair},
    escrow::EscrowState,
    transaction::{
        CreateEscrowPayload, FeeType, Reference, ReleaseEscrowPayload, TransactionType, TxVersion,
    },
};

#[tokio::test]
#[allow(clippy::result_large_err)]
async fn test_escrow_auto_release_transfers_funds() -> Result<(), TestError> {
    let payer = KeyPair::new();
    let payee = KeyPair::new();
    let payer_pub = payer.get_public_key().compress();
    let payee_pub = payee.get_public_key().compress();

    let initial_balance = 500 * COIN_VALUE;
    let mut state = TestApplyState::new(10);
    state.insert_account(payer_pub.clone(), initial_balance, 0);
    state.insert_account(payee_pub.clone(), 0, 0);

    let reference = Reference {
        topoheight: 0,
        hash: Hash::zero(),
    };

    let create_payload = CreateEscrowPayload {
        task_id: "task-auto-release".to_string(),
        provider: payee_pub.clone(),
        amount: 200 * COIN_VALUE,
        asset: TOS_ASSET.clone(),
        timeout_blocks: 100,
        challenge_window: 5,
        challenge_deposit_bps: 100,
        optimistic_release: true,
        arbitration_config: None,
        metadata: None,
    };

    let create_tx = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        0,
        payer_pub.clone(),
        TransactionType::CreateEscrow(create_payload),
        0,
        FeeType::TOS,
        0,
        reference.clone(),
    )
    .finalize(&payer);
    let escrow_id = create_tx.hash();
    let create_tx = Arc::new(create_tx);
    create_tx
        .apply_with_partial_verify(&escrow_id, &mut state)
        .await
        .map_err(|_| TestError::Unsupported)?;

    let release_payload = ReleaseEscrowPayload {
        escrow_id: escrow_id.clone(),
        amount: 120 * COIN_VALUE,
        completion_proof: None,
    };
    let release_tx = UnsignedTransaction::new_with_fee_type(
        TxVersion::T1,
        0,
        payer_pub.clone(),
        TransactionType::ReleaseEscrow(release_payload),
        0,
        FeeType::TOS,
        1,
        reference.clone(),
    )
    .finalize(&payer);
    let release_hash = release_tx.hash();
    let release_tx = Arc::new(release_tx);
    release_tx
        .apply_with_partial_verify(&release_hash, &mut state)
        .await
        .map_err(|_| TestError::Unsupported)?;

    let escrow = state.get_escrow(&escrow_id).await?.expect("escrow exists");
    assert_eq!(escrow.state, EscrowState::PendingRelease);
    assert_eq!(escrow.pending_release_amount, Some(120 * COIN_VALUE));
    assert_eq!(escrow.release_requested_at, Some(10));

    let released = apply_auto_release_in_memory(&mut state, 15).await?;
    assert_eq!(released, 1);

    let escrow = state.get_escrow(&escrow_id).await?.expect("escrow exists");

    assert_eq!(escrow.amount, 80 * COIN_VALUE);
    assert_eq!(escrow.released_amount, 120 * COIN_VALUE);
    assert_eq!(escrow.state, EscrowState::Funded);

    let balance = state
        .get_balance(&payee_pub, &TOS_ASSET)
        .expect("payee balance");
    assert_eq!(balance, 120 * COIN_VALUE);

    let pending = state.list_pending_releases(20, 10);
    assert!(pending.is_empty());

    Ok(())
}

async fn apply_auto_release_in_memory(
    state: &mut TestApplyState,
    current_topoheight: u64,
) -> Result<usize, TestError> {
    let pending = state.list_pending_releases(current_topoheight, 256);
    if pending.is_empty() {
        return Ok(0);
    }

    let mut released = 0usize;
    for (release_at, escrow_id) in pending {
        let mut escrow = match state.get_escrow(&escrow_id).await? {
            Some(escrow) => escrow,
            None => {
                state.remove_pending_release(release_at, &escrow_id).await?;
                continue;
            }
        };

        if !escrow.optimistic_release || escrow.state != EscrowState::PendingRelease {
            state.remove_pending_release(release_at, &escrow_id).await?;
            continue;
        }

        let mut release_amount = escrow.pending_release_amount.unwrap_or(escrow.amount);
        if release_amount > escrow.amount {
            release_amount = escrow.amount;
        }

        if release_amount > 0 {
            let balance = state
                .get_receiver_balance(
                    Cow::Owned(escrow.payee.clone()),
                    Cow::Owned(escrow.asset.clone()),
                )
                .await?;
            *balance = balance.saturating_add(release_amount);
        }

        escrow.amount = escrow.amount.saturating_sub(release_amount);
        escrow.released_amount = escrow.released_amount.saturating_add(release_amount);
        escrow.pending_release_amount = None;
        escrow.release_requested_at = None;
        escrow.state = if escrow.amount == 0 {
            EscrowState::Released
        } else {
            EscrowState::Funded
        };
        escrow.updated_at = current_topoheight;

        state.set_escrow(&escrow).await?;
        state.remove_pending_release(release_at, &escrow_id).await?;
        released += 1;
    }

    Ok(released)
}
