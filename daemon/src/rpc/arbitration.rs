use std::sync::Arc;

use anyhow::Context as AnyhowContext;
use serde_json::{json, Value};
use tos_common::{
    api::daemon::{
        ArbiterWithdrawStatus, EstimateWithdrawableAmountParams, EstimateWithdrawableAmountResult,
        GetArbiterWithdrawStatusParams,
    },
    arbitration::{ArbitrationOpen, JurorVote},
    async_handler,
    context::Context,
    rpc::{parse_params, InternalRpcError, RPCHandler},
};

use crate::core::{blockchain::Blockchain, error::BlockchainError, storage::Storage};

#[cfg(feature = "a2a")]
use crate::a2a::arbitration::coordinator::CoordinatorService;

pub fn register_methods<S: Storage>(handler: &mut RPCHandler<Arc<Blockchain<S>>>) {
    handler.register_method(
        "get_arbiter_withdraw_status",
        async_handler!(get_arbiter_withdraw_status::<S>),
    );
    handler.register_method(
        "estimate_withdrawable_amount",
        async_handler!(estimate_withdrawable_amount::<S>),
    );
    #[cfg(feature = "a2a")]
    {
        handler.register_method("arbitration_open", async_handler!(arbitration_open::<S>));
        handler.register_method("submit_juror_vote", async_handler!(submit_juror_vote::<S>));
    }
}

async fn get_arbiter_withdraw_status<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetArbiterWithdrawStatusParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    if params.address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }
    let storage = blockchain.get_storage().read().await;
    let arbiter = storage
        .get_arbiter(&params.address.get_public_key())
        .await
        .context("Arbiter not found")?
        .context("Arbiter not found")?;
    let current_topoheight = blockchain.get_topo_height();
    let mut status = ArbiterWithdrawStatus {
        status: arbiter.status.clone(),
        stake_amount: arbiter.stake_amount,
        total_slashed: arbiter.total_slashed,
        can_withdraw: false,
        block_reason: None,
        cooldown_ends_at: None,
        cooldown_remaining: None,
        active_cases: arbiter.active_cases,
    };

    if let Some(deactivated_at) = arbiter.deactivated_at {
        let cooldown_end =
            deactivated_at.saturating_add(tos_common::arbitration::ARBITER_COOLDOWN_TOPOHEIGHT);
        status.cooldown_ends_at = Some(cooldown_end);
        if current_topoheight < cooldown_end {
            status.cooldown_remaining = Some(cooldown_end - current_topoheight);
        } else {
            status.cooldown_remaining = Some(0);
        }
    }

    match arbiter.can_withdraw(current_topoheight) {
        Ok(_) => status.can_withdraw = true,
        Err(err) => {
            status.block_reason = Some(match err {
                tos_common::arbitration::ArbiterWithdrawError::NotActive
                | tos_common::arbitration::ArbiterWithdrawError::NotInExitProcess => {
                    "not_in_exit_process".to_string()
                }
                tos_common::arbitration::ArbiterWithdrawError::NoStakeToWithdraw => {
                    "no_stake_to_withdraw".to_string()
                }
                tos_common::arbitration::ArbiterWithdrawError::CooldownNotComplete { .. } => {
                    "cooldown_not_complete".to_string()
                }
                tos_common::arbitration::ArbiterWithdrawError::HasActiveCases { .. } => {
                    "has_active_cases".to_string()
                }
                tos_common::arbitration::ArbiterWithdrawError::ArbiterAlreadyRemoved => {
                    "arbiter_removed".to_string()
                }
            });
        }
    }

    Ok(json!(status))
}

async fn estimate_withdrawable_amount<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: EstimateWithdrawableAmountParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    if params.address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            BlockchainError::InvalidNetwork.into(),
        ));
    }
    let storage = blockchain.get_storage().read().await;
    let arbiter = storage
        .get_arbiter(&params.address.get_public_key())
        .await
        .context("Arbiter not found")?
        .context("Arbiter not found")?;
    let current_topoheight = blockchain.get_topo_height();
    let available = arbiter.can_withdraw(current_topoheight).unwrap_or(0);
    Ok(json!(EstimateWithdrawableAmountResult { available }))
}

#[cfg(feature = "a2a")]
async fn arbitration_open<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let open: ArbitrationOpen = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let coordinator = CoordinatorService::new();
    let vote_request = coordinator
        .handle_arbitration_open(blockchain, open)
        .await
        .map_err(|e| InternalRpcError::InvalidParamsAny(e.into()))?;
    Ok(json!(vote_request))
}

#[cfg(feature = "a2a")]
async fn submit_juror_vote<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let vote: JurorVote = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let coordinator = CoordinatorService::new();
    let verdict = coordinator
        .handle_juror_vote(blockchain, vote)
        .await
        .map_err(|e| InternalRpcError::InvalidParamsAny(e.into()))?;
    Ok(json!(verdict))
}
