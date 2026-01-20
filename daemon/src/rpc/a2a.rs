use std::sync::Arc;

use actix_web::{
    error::{ErrorBadRequest, ErrorInternalServerError, ErrorUnauthorized},
    web::{self, Payload},
    Error, HttpRequest, HttpResponse,
};
use actix_ws::Message;
use bytes::Bytes;
use futures::StreamExt;
use log::warn;
use serde::Deserialize;
use serde_json::{json, Value};
use std::borrow::Cow;

use tos_common::{
    a2a::A2AService,
    a2a::{
        A2AError, CancelTaskRequest, GetExtendedAgentCardRequest,
        GetTaskPushNotificationConfigRequest, GetTaskRequest,
        ListTaskPushNotificationConfigRequest, ListTasksRequest, SendMessageRequest,
        SetTaskPushNotificationConfigRequest, SubscribeToTaskRequest, TaskPushNotificationConfig,
        HEADER_VERSION, PROTOCOL_VERSION,
    },
    async_handler,
    context::Context,
    rpc::server::{RPCServerHandler, RequestMetadata},
    rpc::{parse_params, InternalRpcError, RPCHandler, RpcRequest, RpcResponse, RpcResponseError},
};

use crate::{a2a::A2ADaemonService, core::blockchain::Blockchain, core::storage::Storage};

use super::DaemonRpcServer;

fn service_from_context<S: Storage>(
    context: &Context,
) -> Result<A2ADaemonService<S>, InternalRpcError> {
    let blockchain = context
        .get::<Arc<Blockchain<S>>>()
        .map_err(|_| InternalRpcError::InvalidContext)?;
    Ok(A2ADaemonService::new(Arc::clone(blockchain)))
}

fn service_from_server<S: Storage>(server: &DaemonRpcServer<S>) -> A2ADaemonService<S> {
    let blockchain = server.get_rpc_handler().get_data().clone();
    A2ADaemonService::new(blockchain)
}

fn map_a2a_error(err: A2AError) -> InternalRpcError {
    InternalRpcError::Custom(err.code() as i16, err.to_string())
}

fn map_auth_error(_err: crate::a2a::auth::AuthError) -> InternalRpcError {
    InternalRpcError::CustomStr(-32098, "Unauthorized")
}

/// Validate A2A version header
fn validate_a2a_version(
    headers: &std::collections::HashMap<String, String>,
) -> Result<(), A2AError> {
    if let Some(version) = headers.get(HEADER_VERSION) {
        // Check if version is compatible (currently only 1.0)
        if version != PROTOCOL_VERSION && !version.starts_with("1.") {
            return Err(A2AError::VersionNotSupportedError {
                version: version.clone(),
            });
        }
    }
    // Version header is optional - if not present, assume compatible
    Ok(())
}

async fn require_a2a_auth_http(request: &HttpRequest, body: &[u8]) -> Result<(), Error> {
    let meta = RequestMetadata::from_http_request(request, body);
    // Validate A2A version header
    validate_a2a_version(&meta.headers).map_err(|e| ErrorBadRequest(e.to_string()))?;
    crate::a2a::auth::authorize_metadata(&meta)
        .await
        .map_err(|e| ErrorUnauthorized(e.to_string()))?;
    Ok(())
}

async fn require_a2a_auth_context(context: &Context) -> Result<(), InternalRpcError> {
    let meta = context
        .get::<RequestMetadata>()
        .map_err(|_| InternalRpcError::InvalidContext)?;
    // Validate A2A version header
    validate_a2a_version(&meta.headers).map_err(map_a2a_error)?;
    crate::a2a::auth::authorize_metadata(meta)
        .await
        .map_err(map_auth_error)?;
    Ok(())
}

fn http_error(err: A2AError) -> HttpResponse {
    let body = json!({
        "error": {
            "code": err.code(),
            "message": err.to_string(),
        }
    });

    match err {
        A2AError::UnsupportedOperationError { .. } => HttpResponse::NotImplemented().json(body),
        _ => HttpResponse::BadRequest().json(body),
    }
}

// === JSON-RPC handlers ===

pub fn register_a2a_methods<S: Storage>(handler: &mut RPCHandler<Arc<Blockchain<S>>>) {
    handler.register_method("SendMessage", async_handler!(send_message::<S>));
    handler.register_method(
        "SendStreamingMessage",
        async_handler!(send_streaming_message::<S>),
    );
    handler.register_method("GetTask", async_handler!(get_task::<S>));
    handler.register_method("ListTasks", async_handler!(list_tasks::<S>));
    handler.register_method("CancelTask", async_handler!(cancel_task::<S>));
    handler.register_method("SubscribeToTask", async_handler!(subscribe_to_task::<S>));
    handler.register_method(
        "SetTaskPushNotificationConfig",
        async_handler!(set_task_push_notification_config::<S>),
    );
    handler.register_method(
        "GetTaskPushNotificationConfig",
        async_handler!(get_task_push_notification_config::<S>),
    );
    handler.register_method(
        "ListTaskPushNotificationConfig",
        async_handler!(list_task_push_notification_config::<S>),
    );
    handler.register_method(
        "DeleteTaskPushNotificationConfig",
        async_handler!(delete_task_push_notification_config::<S>),
    );
    handler.register_method(
        "GetExtendedAgentCard",
        async_handler!(get_extended_agent_card::<S>),
    );
}

async fn send_message<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_a2a_auth_context(context).await?;
    let request: SendMessageRequest = parse_params(body)?;
    let service = service_from_context::<S>(context)?;
    let response = service.send_message(request).await.map_err(map_a2a_error)?;
    serde_json::to_value(response).map_err(InternalRpcError::SerializeResponse)
}

async fn send_streaming_message<S: Storage>(
    _context: &Context,
    _body: Value,
) -> Result<Value, InternalRpcError> {
    // Streaming is not supported over plain JSON-RPC.
    // Use the HTTP SSE endpoint (POST /a2a/tasks:sendStreamingMessage) or WebSocket instead.
    Err(InternalRpcError::Custom(
        -32600,
        "Streaming not supported over JSON-RPC. Use HTTP SSE endpoint or WebSocket.".to_string(),
    ))
}

async fn get_task<S: Storage>(context: &Context, body: Value) -> Result<Value, InternalRpcError> {
    require_a2a_auth_context(context).await?;
    let request: GetTaskRequest = parse_params(body)?;
    let service = service_from_context::<S>(context)?;
    let response = service.get_task(request).await.map_err(map_a2a_error)?;
    serde_json::to_value(response).map_err(InternalRpcError::SerializeResponse)
}

async fn list_tasks<S: Storage>(context: &Context, body: Value) -> Result<Value, InternalRpcError> {
    require_a2a_auth_context(context).await?;
    let request: ListTasksRequest = parse_params(body)?;
    let service = service_from_context::<S>(context)?;
    let response = service.list_tasks(request).await.map_err(map_a2a_error)?;
    serde_json::to_value(response).map_err(InternalRpcError::SerializeResponse)
}

async fn cancel_task<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_a2a_auth_context(context).await?;
    let request: CancelTaskRequest = parse_params(body)?;
    let service = service_from_context::<S>(context)?;
    let response = service.cancel_task(request).await.map_err(map_a2a_error)?;
    serde_json::to_value(response).map_err(InternalRpcError::SerializeResponse)
}

async fn subscribe_to_task<S: Storage>(
    _context: &Context,
    _body: Value,
) -> Result<Value, InternalRpcError> {
    // Streaming is not supported over plain JSON-RPC.
    // Use the HTTP SSE endpoint (GET /a2a/tasks/{id}:subscribe) or WebSocket instead.
    Err(InternalRpcError::Custom(
        -32600,
        "Streaming not supported over JSON-RPC. Use HTTP SSE endpoint or WebSocket.".to_string(),
    ))
}

async fn set_task_push_notification_config<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_a2a_auth_context(context).await?;
    let request: SetTaskPushNotificationConfigRequest = parse_params(body)?;
    let service = service_from_context::<S>(context)?;
    let response = service
        .set_task_push_notification_config(request)
        .await
        .map_err(map_a2a_error)?;
    serde_json::to_value(response).map_err(InternalRpcError::SerializeResponse)
}

async fn get_task_push_notification_config<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_a2a_auth_context(context).await?;
    let request: GetTaskPushNotificationConfigRequest = parse_params(body)?;
    let service = service_from_context::<S>(context)?;
    let response = service
        .get_task_push_notification_config(request)
        .await
        .map_err(map_a2a_error)?;
    serde_json::to_value(response).map_err(InternalRpcError::SerializeResponse)
}

async fn list_task_push_notification_config<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_a2a_auth_context(context).await?;
    let request: ListTaskPushNotificationConfigRequest = parse_params(body)?;
    let service = service_from_context::<S>(context)?;
    let response = service
        .list_task_push_notification_config(request)
        .await
        .map_err(map_a2a_error)?;
    serde_json::to_value(response).map_err(InternalRpcError::SerializeResponse)
}

async fn delete_task_push_notification_config<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_a2a_auth_context(context).await?;
    let request: tos_common::a2a::DeleteTaskPushNotificationConfigRequest = parse_params(body)?;
    let service = service_from_context::<S>(context)?;
    service
        .delete_task_push_notification_config(request)
        .await
        .map_err(map_a2a_error)?;
    Ok(json!({}))
}

async fn get_extended_agent_card<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_a2a_auth_context(context).await?;
    let request: GetExtendedAgentCardRequest = parse_params(body)?;
    let service = service_from_context::<S>(context)?;
    let response = service
        .get_extended_agent_card(request)
        .await
        .map_err(map_a2a_error)?;
    serde_json::to_value(response).map_err(InternalRpcError::SerializeResponse)
}

// === HTTP+JSON handlers ===

#[derive(Deserialize)]
pub struct TaskPath {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskConfigPath {
    id: String,
    config_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskQuery {
    tenant: Option<String>,
    history_length: Option<i32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTasksQuery {
    tenant: Option<String>,
    context_id: Option<String>,
    status: Option<tos_common::a2a::TaskState>,
    page_size: Option<i32>,
    page_token: Option<String>,
    history_length: Option<i32>,
    last_updated_after: Option<i64>,
    include_artifacts: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListPushConfigQuery {
    tenant: Option<String>,
    page_size: Option<i32>,
    page_token: Option<String>,
}

fn parse_push_name(name: &str) -> Option<(&str, &str)> {
    let mut parts = name.split('/');
    if parts.next()? != "tasks" {
        return None;
    }
    let task_id = parts.next()?;
    if parts.next()? != "pushNotificationConfigs" {
        return None;
    }
    let config_id = parts.next()?;
    Some((task_id, config_id))
}

/// Public agent card discovery endpoint (no authentication required)
/// Per A2A spec, /.well-known/agent-card.json SHOULD be publicly accessible
pub async fn agent_card<S: Storage>(
    server: web::Data<DaemonRpcServer<S>>,
) -> Result<HttpResponse, Error> {
    let service = service_from_server(&server);
    let request = GetExtendedAgentCardRequest { tenant: None };
    match service.get_extended_agent_card(request).await {
        Ok(card) => Ok(HttpResponse::Ok().json(card)),
        Err(err) => Ok(http_error(err)),
    }
}

pub async fn send_message_http<S: Storage>(
    server: web::Data<DaemonRpcServer<S>>,
    request: HttpRequest,
    body: web::Bytes,
) -> Result<HttpResponse, Error> {
    require_a2a_auth_http(&request, &body).await?;
    let request: SendMessageRequest =
        serde_json::from_slice(&body).map_err(|e| ErrorBadRequest(e.to_string()))?;
    let service = service_from_server(&server);
    match service.send_message(request).await {
        Ok(response) => Ok(HttpResponse::Ok().json(response)),
        Err(err) => Ok(http_error(err)),
    }
}

pub async fn send_streaming_message_http<S: Storage>(
    server: web::Data<DaemonRpcServer<S>>,
    request: HttpRequest,
    body: web::Bytes,
) -> Result<HttpResponse, Error> {
    require_a2a_auth_http(&request, &body).await?;
    let request: SendMessageRequest =
        serde_json::from_slice(&body).map_err(|e| ErrorBadRequest(e.to_string()))?;
    let service = service_from_server(&server);
    match service.send_streaming_message(request).await {
        Ok(stream) => {
            let sse_stream = stream.map(|event| {
                let payload = serde_json::to_string(&event).map_err(ErrorInternalServerError)?;
                Ok::<Bytes, Error>(Bytes::from(format!("data: {payload}\n\n")))
            });
            Ok(HttpResponse::Ok()
                .append_header(("Content-Type", "text/event-stream"))
                .append_header(("Cache-Control", "no-cache"))
                .streaming(sse_stream))
        }
        Err(err) => Ok(http_error(err)),
    }
}

pub async fn get_task_http<S: Storage>(
    server: web::Data<DaemonRpcServer<S>>,
    request: HttpRequest,
    path: web::Path<TaskPath>,
    query: web::Query<TaskQuery>,
) -> Result<HttpResponse, Error> {
    require_a2a_auth_http(&request, &[]).await?;
    let service = service_from_server(&server);
    let request = GetTaskRequest {
        tenant: query.tenant.clone(),
        name: format!("tasks/{}", path.id),
        history_length: query.history_length,
    };
    match service.get_task(request).await {
        Ok(task) => Ok(HttpResponse::Ok().json(task)),
        Err(err) => Ok(http_error(err)),
    }
}

pub async fn list_tasks_http<S: Storage>(
    server: web::Data<DaemonRpcServer<S>>,
    request: HttpRequest,
    query: web::Query<ListTasksQuery>,
) -> Result<HttpResponse, Error> {
    require_a2a_auth_http(&request, &[]).await?;
    let service = service_from_server(&server);
    let request = ListTasksRequest {
        tenant: query.tenant.clone(),
        context_id: query.context_id.clone(),
        status: query.status.clone(),
        page_size: query.page_size,
        page_token: query.page_token.clone(),
        history_length: query.history_length,
        last_updated_after: query.last_updated_after,
        include_artifacts: query.include_artifacts,
    };
    match service.list_tasks(request).await {
        Ok(response) => Ok(HttpResponse::Ok().json(response)),
        Err(err) => Ok(http_error(err)),
    }
}

pub async fn cancel_task_http<S: Storage>(
    server: web::Data<DaemonRpcServer<S>>,
    request: HttpRequest,
    path: web::Path<TaskPath>,
) -> Result<HttpResponse, Error> {
    require_a2a_auth_http(&request, &[]).await?;
    let service = service_from_server(&server);
    let request = CancelTaskRequest {
        tenant: None,
        name: format!("tasks/{}", path.id),
    };
    match service.cancel_task(request).await {
        Ok(task) => Ok(HttpResponse::Ok().json(task)),
        Err(err) => Ok(http_error(err)),
    }
}

pub async fn subscribe_task_http<S: Storage>(
    server: web::Data<DaemonRpcServer<S>>,
    request: HttpRequest,
    path: web::Path<TaskPath>,
) -> Result<HttpResponse, Error> {
    require_a2a_auth_http(&request, &[]).await?;
    let service = service_from_server(&server);
    let request = SubscribeToTaskRequest {
        tenant: None,
        name: format!("tasks/{}", path.id),
    };
    match service.subscribe_to_task(request).await {
        Ok(stream) => {
            let sse_stream = stream.map(|event| {
                let payload = serde_json::to_string(&event).map_err(ErrorInternalServerError)?;
                Ok::<Bytes, Error>(Bytes::from(format!("data: {payload}\n\n")))
            });
            Ok(HttpResponse::Ok()
                .append_header(("Content-Type", "text/event-stream"))
                .append_header(("Cache-Control", "no-cache"))
                .streaming(sse_stream))
        }
        Err(err) => Ok(http_error(err)),
    }
}

pub async fn set_task_push_config_http<S: Storage>(
    server: web::Data<DaemonRpcServer<S>>,
    request: HttpRequest,
    path: web::Path<TaskPath>,
    body: web::Bytes,
) -> Result<HttpResponse, Error> {
    require_a2a_auth_http(&request, &body).await?;
    let service = service_from_server(&server);
    let value: Value = serde_json::from_slice(&body).map_err(|e| ErrorBadRequest(e.to_string()))?;
    let request = if value.get("config").is_some() {
        serde_json::from_value::<SetTaskPushNotificationConfigRequest>(value)
            .map_err(|e| ErrorBadRequest(e.to_string()))?
    } else {
        let mut config: TaskPushNotificationConfig =
            serde_json::from_value(value).map_err(|e| ErrorBadRequest(e.to_string()))?;
        let (task_id, config_id) = if let Some((task_id, config_id)) = parse_push_name(&config.name)
        {
            (task_id.to_string(), config_id.to_string())
        } else if let Some(config_id) = config.push_notification_config.id.clone() {
            (path.id.clone(), config_id)
        } else {
            return Err(ErrorBadRequest("missing push config id"));
        };
        if task_id != path.id {
            return Err(ErrorBadRequest("push config task id mismatch"));
        }
        if config.name.is_empty() {
            config.name = format!("tasks/{}/pushNotificationConfigs/{}", task_id, config_id);
        }
        SetTaskPushNotificationConfigRequest {
            tenant: None,
            parent: format!("tasks/{}", task_id),
            config_id,
            config,
        }
    };

    match service.set_task_push_notification_config(request).await {
        Ok(config) => Ok(HttpResponse::Ok().json(config)),
        Err(err) => Ok(http_error(err)),
    }
}

pub async fn get_task_push_config_http<S: Storage>(
    server: web::Data<DaemonRpcServer<S>>,
    request: HttpRequest,
    path: web::Path<TaskConfigPath>,
) -> Result<HttpResponse, Error> {
    require_a2a_auth_http(&request, &[]).await?;
    let service = service_from_server(&server);
    let request = GetTaskPushNotificationConfigRequest {
        tenant: None,
        name: format!(
            "tasks/{}/pushNotificationConfigs/{}",
            path.id, path.config_id
        ),
    };
    match service.get_task_push_notification_config(request).await {
        Ok(config) => Ok(HttpResponse::Ok().json(config)),
        Err(err) => Ok(http_error(err)),
    }
}

pub async fn list_task_push_config_http<S: Storage>(
    server: web::Data<DaemonRpcServer<S>>,
    request: HttpRequest,
    path: web::Path<TaskPath>,
    query: web::Query<ListPushConfigQuery>,
) -> Result<HttpResponse, Error> {
    require_a2a_auth_http(&request, &[]).await?;
    let service = service_from_server(&server);
    let request = ListTaskPushNotificationConfigRequest {
        tenant: query.tenant.clone(),
        parent: format!("tasks/{}", path.id),
        page_size: query.page_size,
        page_token: query.page_token.clone(),
    };
    match service.list_task_push_notification_config(request).await {
        Ok(response) => Ok(HttpResponse::Ok().json(response)),
        Err(err) => Ok(http_error(err)),
    }
}

pub async fn delete_task_push_config_http<S: Storage>(
    server: web::Data<DaemonRpcServer<S>>,
    request: HttpRequest,
    path: web::Path<TaskConfigPath>,
) -> Result<HttpResponse, Error> {
    require_a2a_auth_http(&request, &[]).await?;
    let service = service_from_server(&server);
    let request = tos_common::a2a::DeleteTaskPushNotificationConfigRequest {
        tenant: None,
        name: format!(
            "tasks/{}/pushNotificationConfigs/{}",
            path.id, path.config_id
        ),
    };
    match service.delete_task_push_notification_config(request).await {
        Ok(()) => Ok(HttpResponse::Ok().json(json!({}))),
        Err(err) => Ok(http_error(err)),
    }
}

pub async fn get_extended_agent_card_http<S: Storage>(
    server: web::Data<DaemonRpcServer<S>>,
    request: HttpRequest,
) -> Result<HttpResponse, Error> {
    require_a2a_auth_http(&request, &[]).await?;
    let service = service_from_server(&server);
    let request = GetExtendedAgentCardRequest { tenant: None };
    match service.get_extended_agent_card(request).await {
        Ok(card) => Ok(HttpResponse::Ok().json(card)),
        Err(err) => Ok(http_error(err)),
    }
}

pub async fn a2a_websocket<S: Storage>(
    server: web::Data<DaemonRpcServer<S>>,
    request: HttpRequest,
    body: Payload,
) -> Result<HttpResponse, Error> {
    require_a2a_auth_http(&request, &[]).await?;
    let (response, mut session, msg_stream) = actix_ws::handle(&request, body)?;
    let server = server.clone();
    let security = server.websocket_security().clone();
    // Apply frame size limit at transport layer to prevent DoS via large frames
    // This rejects oversized frames before they are fully buffered into memory
    // Use the configured max_message_size to ensure transport and application limits match
    let mut msg_stream = msg_stream.max_frame_size(security.max_message_size());

    actix_web::rt::spawn(async move {
        while let Some(message) = msg_stream.next().await {
            let message = match message {
                Ok(message) => message,
                Err(err) => {
                    if log::log_enabled!(log::Level::Warn) {
                        warn!("A2A websocket receive error: {}", err);
                    }
                    break;
                }
            };

            match message {
                Message::Text(text) => {
                    if let Err(err) = security.validate_message_size(text.len()) {
                        if log::log_enabled!(log::Level::Warn) {
                            warn!("A2A websocket message rejected: {}", err);
                        }
                        let _ = session.close(None).await;
                        break;
                    }
                    if let Err(err) =
                        handle_a2a_ws_message(&server, &mut session, text.as_bytes()).await
                    {
                        if let Err(send_err) = session.text(err.to_json().to_string()).await {
                            if log::log_enabled!(log::Level::Warn) {
                                warn!("A2A websocket send error: {}", send_err);
                            }
                            break;
                        }
                    }
                }
                Message::Binary(bytes) => {
                    if let Err(err) = security.validate_message_size(bytes.len()) {
                        if log::log_enabled!(log::Level::Warn) {
                            warn!("A2A websocket message rejected: {}", err);
                        }
                        let _ = session.close(None).await;
                        break;
                    }
                    if let Err(err) = handle_a2a_ws_message(&server, &mut session, &bytes).await {
                        if let Err(send_err) = session.text(err.to_json().to_string()).await {
                            if log::log_enabled!(log::Level::Warn) {
                                warn!("A2A websocket send error: {}", send_err);
                            }
                            break;
                        }
                    }
                }
                Message::Close(_) => break,
                Message::Ping(bytes) => {
                    if let Err(err) = session.pong(&bytes).await {
                        if log::log_enabled!(log::Level::Warn) {
                            warn!("A2A websocket pong error: {}", err);
                        }
                        break;
                    }
                }
                Message::Pong(_) => {}
                _ => {}
            }
        }
    });

    Ok(response)
}

async fn handle_a2a_ws_message<S: Storage>(
    server: &web::Data<DaemonRpcServer<S>>,
    session: &mut actix_ws::Session,
    payload: &[u8],
) -> Result<(), RpcResponseError> {
    let request: RpcRequest = serde_json::from_slice(payload)
        .map_err(|_| RpcResponseError::new(None, InternalRpcError::InvalidJSONRequest))?;
    if request.jsonrpc != tos_common::rpc::JSON_RPC_VERSION {
        return Err(RpcResponseError::new(
            request.id.clone(),
            InternalRpcError::InvalidVersion,
        ));
    }

    let service = service_from_server(server);
    let method = request.method.as_str();
    let params = request.params.clone().ok_or_else(|| {
        RpcResponseError::new(request.id.clone(), InternalRpcError::ExpectedParams)
    })?;

    match method {
        "SendMessage" => {
            let send_request: SendMessageRequest = serde_json::from_value(params).map_err(|e| {
                RpcResponseError::new(request.id.clone(), InternalRpcError::InvalidJSONParams(e))
            })?;
            let response = service
                .send_message(send_request)
                .await
                .map_err(|e| RpcResponseError::new(request.id.clone(), map_a2a_error(e)))?;
            if let Some(_) = request.id {
                let result = serde_json::to_value(response).map_err(|e| {
                    RpcResponseError::new(
                        request.id.clone(),
                        InternalRpcError::SerializeResponse(e),
                    )
                })?;
                let response = RpcResponse::new(Cow::Borrowed(&request.id), Cow::Owned(result));
                session
                    .text(serde_json::to_string(&response).map_err(|e| {
                        RpcResponseError::new(
                            request.id.clone(),
                            InternalRpcError::SerializeResponse(e),
                        )
                    })?)
                    .await
                    .map_err(|_| {
                        RpcResponseError::new(
                            request.id.clone(),
                            InternalRpcError::InternalError("ws send failed"),
                        )
                    })?;
            }
        }
        "SendStreamingMessage" | "SubscribeToTask" => {
            if request.id.is_none() {
                return Err(RpcResponseError::new(
                    request.id.clone(),
                    InternalRpcError::InvalidParams("id is required for streaming"),
                ));
            }

            let mut stream = if method == "SendStreamingMessage" {
                let send_request: SendMessageRequest =
                    serde_json::from_value(params).map_err(|e| {
                        RpcResponseError::new(
                            request.id.clone(),
                            InternalRpcError::InvalidJSONParams(e),
                        )
                    })?;
                service
                    .send_streaming_message(send_request)
                    .await
                    .map_err(|e| RpcResponseError::new(request.id.clone(), map_a2a_error(e)))?
            } else {
                let subscribe_request: SubscribeToTaskRequest = serde_json::from_value(params)
                    .map_err(|e| {
                        RpcResponseError::new(
                            request.id.clone(),
                            InternalRpcError::InvalidJSONParams(e),
                        )
                    })?;
                service
                    .subscribe_to_task(subscribe_request)
                    .await
                    .map_err(|e| RpcResponseError::new(request.id.clone(), map_a2a_error(e)))?
            };

            while let Some(event) = stream.next().await {
                let result = serde_json::to_value(event).map_err(|e| {
                    RpcResponseError::new(
                        request.id.clone(),
                        InternalRpcError::SerializeResponse(e),
                    )
                })?;
                let response = RpcResponse::new(Cow::Borrowed(&request.id), Cow::Owned(result));
                session
                    .text(serde_json::to_string(&response).map_err(|e| {
                        RpcResponseError::new(
                            request.id.clone(),
                            InternalRpcError::SerializeResponse(e),
                        )
                    })?)
                    .await
                    .map_err(|_| {
                        RpcResponseError::new(
                            request.id.clone(),
                            InternalRpcError::InternalError("ws send failed"),
                        )
                    })?;
            }
        }
        _ => {
            return Err(RpcResponseError::new(
                request.id.clone(),
                InternalRpcError::MethodNotFound(request.method.clone()),
            ));
        }
    }

    Ok(())
}
