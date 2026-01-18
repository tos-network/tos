use std::borrow::Cow;

use crate::core::{error::BlockchainError, state::ApplicableChainState, storage::Storage};
use metrics::counter;
use tos_common::crypto::{Hash, PublicKey};
use tos_common::escrow::EscrowState;
use tos_common::transaction::verify::{BlockchainApplyState, BlockchainVerificationState};

pub const DEFAULT_AUTO_RELEASE_BATCH: usize = 256;

#[derive(Clone, Debug)]
pub struct AutoReleaseRecord {
    pub release_at: u64,
    pub escrow_id: Hash,
    pub amount: u64,
    pub asset: Hash,
    pub payee: PublicKey,
}

pub async fn apply_auto_release<'a, S: Storage>(
    state: &mut ApplicableChainState<'a, S>,
    current_topoheight: u64,
) -> Result<Vec<AutoReleaseRecord>, BlockchainError> {
    let pending = state
        .get_mut_storage()
        .list_pending_releases(current_topoheight, DEFAULT_AUTO_RELEASE_BATCH)
        .await?;
    if pending.is_empty() {
        return Ok(Vec::new());
    }

    counter!("tos_escrow_auto_release_pending").increment(pending.len() as u64);
    let mut released = Vec::new();

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

        counter!("tos_escrow_auto_release_total").increment(1);
        counter!("tos_escrow_auto_release_amount").increment(release_amount);
        released.push(AutoReleaseRecord {
            release_at,
            escrow_id,
            amount: release_amount,
            asset: escrow.asset.clone(),
            payee: escrow.payee.clone(),
        });
    }

    Ok(released)
}
