use std::sync::Arc;

use actix_web::{
    error::{ErrorBadRequest, ErrorInternalServerError},
    web, Error as ActixError, HttpResponse,
};
use anyhow::Context as AnyhowContext;
use serde::Deserialize;
use serde_json::{json, Value};
use tos_common::{
    api::daemon::{
        AppealStatusResult, DisputeDetailsResult, EscrowHistoryResult, EscrowListResult,
        GetAppealStatusParams, GetDisputeDetailsParams, GetEscrowHistoryParams, GetEscrowParams,
        GetEscrowsByClientParams, GetEscrowsByProviderParams, GetEscrowsByTaskParams,
        GetPendingReleasesParams, PendingReleaseEntry, PendingReleasesResult,
    },
    async_handler,
    context::Context,
    rpc::server::RPCServerHandler,
    rpc::{parse_params, InternalRpcError, RPCHandler},
};

use crate::core::{blockchain::Blockchain, storage::Storage};

use super::DaemonRpcServer;

const MAX_ESCROWS: usize = 200;

pub fn register_methods<S: Storage>(handler: &mut RPCHandler<Arc<Blockchain<S>>>) {
    handler.register_method("get_escrow", async_handler!(get_escrow::<S>));
    handler.register_method(
        "get_escrows_by_client",
        async_handler!(get_escrows_by_client::<S>),
    );
    handler.register_method(
        "get_escrows_by_provider",
        async_handler!(get_escrows_by_provider::<S>),
    );
    handler.register_method(
        "get_escrows_by_task",
        async_handler!(get_escrows_by_task::<S>),
    );
    handler.register_method(
        "get_escrow_history",
        async_handler!(get_escrow_history::<S>),
    );
    handler.register_method(
        "get_pending_releases",
        async_handler!(get_pending_releases::<S>),
    );
    handler.register_method(
        "get_dispute_details",
        async_handler!(get_dispute_details::<S>),
    );
    handler.register_method("get_appeal_status", async_handler!(get_appeal_status::<S>));
}

async fn get_escrow<S: Storage>(context: &Context, body: Value) -> Result<Value, InternalRpcError> {
    let params: GetEscrowParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let escrow = storage
        .get_escrow(&params.escrow_id)
        .await
        .context("Escrow not found")?
        .context("Escrow not found")?;
    Ok(json!(escrow))
}

async fn get_escrows_by_client<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetEscrowsByClientParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    if params.address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            crate::core::error::BlockchainError::InvalidNetwork.into(),
        ));
    }
    let maximum = params.maximum.unwrap_or(MAX_ESCROWS).min(MAX_ESCROWS);
    let skip = params.skip.unwrap_or(0);
    let storage = blockchain.get_storage().read().await;
    let escrows = storage
        .get_escrows_by_payer(params.address.get_public_key(), skip, maximum)
        .await
        .context("Failed to list escrows by client")?;
    Ok(json!(EscrowListResult { escrows }))
}

async fn get_escrows_by_provider<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetEscrowsByProviderParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    if params.address.is_mainnet() != blockchain.get_network().is_mainnet() {
        return Err(InternalRpcError::InvalidParamsAny(
            crate::core::error::BlockchainError::InvalidNetwork.into(),
        ));
    }
    let maximum = params.maximum.unwrap_or(MAX_ESCROWS).min(MAX_ESCROWS);
    let skip = params.skip.unwrap_or(0);
    let storage = blockchain.get_storage().read().await;
    let escrows = storage
        .get_escrows_by_payee(params.address.get_public_key(), skip, maximum)
        .await
        .context("Failed to list escrows by provider")?;
    Ok(json!(EscrowListResult { escrows }))
}

async fn get_escrows_by_task<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetEscrowsByTaskParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let maximum = params.maximum.unwrap_or(MAX_ESCROWS).min(MAX_ESCROWS);
    let skip = params.skip.unwrap_or(0);
    let storage = blockchain.get_storage().read().await;
    let escrows = storage
        .get_escrows_by_task_id(&params.task_id, skip, maximum)
        .await
        .context("Failed to list escrows by task")?;
    Ok(json!(EscrowListResult { escrows }))
}

async fn get_escrow_history<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetEscrowHistoryParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let maximum = params.maximum.unwrap_or(MAX_ESCROWS).min(MAX_ESCROWS);
    let skip = params.skip.unwrap_or(0);
    let entries = if params.descending {
        storage
            .list_escrow_history_desc(&params.escrow_id, skip, maximum)
            .await
            .context("Failed to list escrow history")?
    } else {
        storage
            .list_escrow_history(&params.escrow_id, skip, maximum)
            .await
            .context("Failed to list escrow history")?
    }
    .into_iter()
    .map(
        |(topoheight, tx_hash)| tos_common::api::daemon::EscrowHistoryEntry {
            topoheight,
            tx_hash,
        },
    )
    .collect::<Vec<_>>();
    Ok(json!(EscrowHistoryResult {
        escrow_id: params.escrow_id.into_owned(),
        entries,
    }))
}

async fn get_pending_releases<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetPendingReleasesParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let limit = params.limit.unwrap_or(MAX_ESCROWS).min(MAX_ESCROWS);
    let up_to = params.up_to.unwrap_or_else(|| blockchain.get_topo_height());
    let storage = blockchain.get_storage().read().await;
    let entries = storage
        .list_pending_releases(up_to, limit)
        .await
        .context("Failed to list pending releases")?
        .into_iter()
        .map(|(release_at, escrow_id)| PendingReleaseEntry {
            release_at,
            escrow_id,
        })
        .collect::<Vec<_>>();
    Ok(json!(PendingReleasesResult { entries }))
}

async fn get_dispute_details<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetDisputeDetailsParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let escrow = storage
        .get_escrow(&params.escrow_id)
        .await
        .context("Escrow not found")?
        .context("Escrow not found")?;
    Ok(json!(DisputeDetailsResult {
        dispute: escrow.dispute,
        dispute_id: escrow.dispute_id,
        dispute_round: escrow.dispute_round,
    }))
}

async fn get_appeal_status<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    let params: GetAppealStatusParams = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let escrow = storage
        .get_escrow(&params.escrow_id)
        .await
        .context("Escrow not found")?
        .context("Escrow not found")?;
    Ok(json!(AppealStatusResult {
        appeal: escrow.appeal
    }))
}

#[derive(Debug, Deserialize)]
pub(crate) struct PendingReleasesQuery {
    up_to: Option<u64>,
    limit: Option<usize>,
}

pub(crate) async fn get_pending_releases_http<S: Storage>(
    server: web::Data<DaemonRpcServer<S>>,
    body: web::Bytes,
) -> Result<HttpResponse, ActixError> {
    let params: GetPendingReleasesParams =
        serde_json::from_slice(&body).map_err(|e| ErrorBadRequest(e.to_string()))?;
    let blockchain = server.get_rpc_handler().get_data().clone();
    let limit = params.limit.unwrap_or(MAX_ESCROWS).min(MAX_ESCROWS);
    let up_to = params.up_to.unwrap_or_else(|| blockchain.get_topo_height());
    let storage = blockchain.get_storage().read().await;
    let entries = storage
        .list_pending_releases(up_to, limit)
        .await
        .map_err(|e| ErrorInternalServerError(e.to_string()))?
        .into_iter()
        .map(|(release_at, escrow_id)| PendingReleaseEntry {
            release_at,
            escrow_id,
        })
        .collect::<Vec<_>>();
    Ok(HttpResponse::Ok().json(PendingReleasesResult { entries }))
}

pub(crate) async fn get_pending_releases_http_get<S: Storage>(
    server: web::Data<DaemonRpcServer<S>>,
    query: web::Query<PendingReleasesQuery>,
) -> Result<HttpResponse, ActixError> {
    let blockchain = server.get_rpc_handler().get_data().clone();
    let limit = query.limit.unwrap_or(MAX_ESCROWS).min(MAX_ESCROWS);
    let up_to = query.up_to.unwrap_or_else(|| blockchain.get_topo_height());
    let storage = blockchain.get_storage().read().await;
    let entries = storage
        .list_pending_releases(up_to, limit)
        .await
        .map_err(|e| ErrorInternalServerError(e.to_string()))?
        .into_iter()
        .map(|(release_at, escrow_id)| PendingReleaseEntry {
            release_at,
            escrow_id,
        })
        .collect::<Vec<_>>();
    Ok(HttpResponse::Ok().json(PendingReleasesResult { entries }))
}
