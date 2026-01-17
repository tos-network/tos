use std::borrow::Cow;

use crate::core::{error::BlockchainError, state::ApplicableChainState, storage::Storage};
use tos_common::escrow::EscrowState;
use tos_common::transaction::verify::{BlockchainApplyState, BlockchainVerificationState};

pub const DEFAULT_AUTO_RELEASE_BATCH: usize = 256;

pub async fn apply_auto_release<'a, S: Storage>(
    state: &mut ApplicableChainState<'a, S>,
    current_topoheight: u64,
) -> Result<usize, BlockchainError> {
    let pending = state
        .get_mut_storage()
        .list_pending_releases(current_topoheight, DEFAULT_AUTO_RELEASE_BATCH)
        .await?;
    if pending.is_empty() {
        return Ok(0);
    }

    let mut released = 0usize;

    for (release_at, escrow_id) in pending {
        let mut escrow = match state.get_escrow(&escrow_id).await? {
            Some(escrow) => escrow,
            None => {
                state
                    .get_mut_storage()
                    .remove_pending_release(release_at, &escrow_id)
                    .await?;
                continue;
            }
        };

        if !escrow.optimistic_release || escrow.state != EscrowState::PendingRelease {
            state
                .get_mut_storage()
                .remove_pending_release(release_at, &escrow_id)
                .await?;
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
            *balance = balance
                .checked_add(release_amount)
                .ok_or(BlockchainError::BalanceOverflow)?;
        }

        escrow.amount = escrow
            .amount
            .checked_sub(release_amount)
            .ok_or(BlockchainError::BalanceOverflow)?;
        escrow.released_amount = escrow
            .released_amount
            .checked_add(release_amount)
            .ok_or(BlockchainError::BalanceOverflow)?;

        escrow.pending_release_amount = None;
        escrow.release_requested_at = None;
        escrow.state = if escrow.amount == 0 {
            EscrowState::Released
        } else {
            EscrowState::Funded
        };
        escrow.updated_at = current_topoheight;

        state.set_escrow(&escrow).await?;
        state
            .get_mut_storage()
            .remove_pending_release(release_at, &escrow_id)
            .await?;

        released += 1;
    }

    Ok(released)
}
