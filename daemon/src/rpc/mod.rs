pub mod a2a;
pub mod agent_registry;
pub mod arbitration;
pub mod callback;
pub mod escrow;
pub mod getwork;
pub mod rpc;
pub mod ws_security;

use crate::a2a::registry::router::{RouterConfig, RoutingStrategy};
use crate::a2a::router_executor::AgentRouterExecutor;
use crate::core::{
    blockchain::Blockchain, config::RPCConfig, error::BlockchainError, storage::Storage,
};
use crate::rpc::agent_registry::RegistrationRateLimitConfig;
use actix_web::{
    dev::ServerHandle,
    error::Error,
    get,
    web::{self, Data, Payload},
    App, HttpRequest, HttpResponse, HttpServer, Responder,
};
use anyhow::Context;
use getwork::GetWorkServer;
use log::{error, info, warn};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use serde_json::{json, Value};
use std::{collections::HashSet, net::SocketAddr, sync::Arc, time::Duration};
use tokio::sync::oneshot;
use tonic::transport::Server;
use tonic_reflection::server::Builder as ReflectionBuilder;
use tos_common::{
    api::daemon::NotifyEvent,
    config,
    rpc::{
        server::{
            json_rpc,
            websocket::{EventWebSocketHandler, WebSocketServer, WebSocketServerShared},
            RPCServerHandler, WebSocketServerHandler,
        },
        InternalRpcError, RPCHandler,
    },
    tokio::spawn_task,
    tokio::sync::Mutex,
};
use ws_security::{WebSocketSecurity, WebSocketSecurityConfig};

pub type SharedDaemonRpcServer<S> = Arc<DaemonRpcServer<S>>;

pub struct DaemonRpcServer<S: Storage> {
    handle: Mutex<Option<ServerHandle>>,
    websocket: WebSocketServerShared<EventWebSocketHandler<Arc<Blockchain<S>>, NotifyEvent>>,
    getwork: Option<WebSocketServerShared<GetWorkServer<S>>>,
    websocket_security: Arc<WebSocketSecurity>,
    a2a_grpc_shutdown: Mutex<Option<oneshot::Sender<()>>>,
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("client not registered")]
    ClientNotRegistered,
    #[error("invalid address")]
    ExpectedNormalAddress,
    #[error("P2p engine is not running")]
    NoP2p,
    #[error("WebSocket server is not started")]
    NoWebSocketServer,
}

impl<S: Storage> DaemonRpcServer<S> {
    pub async fn new(
        blockchain: Arc<Blockchain<S>>,
        config: RPCConfig,
    ) -> Result<SharedDaemonRpcServer<S>, BlockchainError> {
        // SECURITY WARNING: Check for insecure bind address
        if config.bind_address.starts_with("0.0.0.0") {
            warn!(
                "RPC server binding to 0.0.0.0 exposes the API to ALL network interfaces. \
                 For production, consider using 127.0.0.1 to restrict access to localhost only. \
                 Ensure proper firewall rules and authentication are in place."
            );
        }

        // Create WebSocket security configuration from RPC config
        let ws_security_config = WebSocketSecurityConfig {
            allowed_origins: config
                .ws_allowed_origins
                .as_ref()
                .map(|s| s.split(',').map(|s| s.trim().to_string()).collect())
                .unwrap_or_else(|| vec!["*".to_string()]),
            require_auth: config.ws_require_auth,
            max_message_size: config.ws_max_message_size,
            max_subscriptions_per_connection: config.ws_max_subscriptions,
            max_connections_per_ip_per_minute: config.ws_max_connections_per_minute,
            max_messages_per_connection_per_second: config.ws_max_messages_per_second,
        };

        if log::log_enabled!(log::Level::Info) {
            info!(
                "WebSocket security: max_message_size={}, max_subscriptions={}, \
                 max_connections_per_minute={}, max_messages_per_second={}",
                ws_security_config.max_message_size,
                ws_security_config.max_subscriptions_per_connection,
                ws_security_config.max_connections_per_ip_per_minute,
                ws_security_config.max_messages_per_connection_per_second
            );
        }
        info!("A2A service enabled: {}", config.enable_a2a);
        if config.enable_a2a {
            let _ = crate::a2a::registry::spawn_health_checks();
            crate::a2a::auth::set_auth_config(crate::a2a::auth::A2AAuthConfig::from_rpc_config(
                &config,
            ));
            crate::a2a::set_settlement_validation_config(crate::a2a::SettlementValidationConfig {
                validate_states: config.a2a_escrow_validate_states,
                allowed_states: config.a2a_escrow_allowed_states.clone(),
                validate_timeout: config.a2a_escrow_validate_timeout,
                validate_amounts: config.a2a_escrow_validate_amounts,
            });
            crate::rpc::agent_registry::set_registration_rate_limit_config(
                RegistrationRateLimitConfig {
                    window_secs: config.a2a_registry_rate_limit_window_secs,
                    max_requests: config.a2a_registry_rate_limit_max,
                },
            );
            let local_executor =
                crate::a2a::executor::default_executor(config.a2a_executor_concurrency);
            let router_config = RouterConfig {
                strategy: match config.a2a_router_strategy {
                    crate::core::config::A2ARoutingStrategy::FirstMatch => {
                        RoutingStrategy::FirstMatch
                    }
                    crate::core::config::A2ARoutingStrategy::LowestLatency => {
                        RoutingStrategy::LowestLatency
                    }
                    crate::core::config::A2ARoutingStrategy::HighestReputation => {
                        RoutingStrategy::HighestReputation
                    }
                    crate::core::config::A2ARoutingStrategy::RoundRobin => {
                        RoutingStrategy::RoundRobin
                    }
                    crate::core::config::A2ARoutingStrategy::WeightedRandom => {
                        RoutingStrategy::WeightedRandom
                    }
                },
                timeout_ms: config.a2a_router_timeout_ms,
                retry_count: config.a2a_router_retry_count,
                fallback_to_local: config.a2a_router_fallback_to_local,
            };
            let router_executor = AgentRouterExecutor::new(local_executor, router_config);
            crate::a2a::executor::set_executor(Arc::new(router_executor));
        }

        let websocket_security = Arc::new(WebSocketSecurity::new(ws_security_config));

        let getwork = if !config.getwork.disable {
            info!("Creating GetWork server...");
            Some(WebSocketServer::new(GetWorkServer::new(
                blockchain.clone(),
                config.getwork.rate_limit_ms,
                config.getwork.notify_job_concurrency,
            )))
        } else {
            None
        };

        // create the RPC Handler which will register and contains all available methods
        let mut rpc_handler = RPCHandler::new(Arc::clone(&blockchain));
        rpc::register_methods(
            &mut rpc_handler,
            !config.getwork.disable,
            config.enable_admin_rpc,
            config.enable_a2a,
        );

        // create the default websocket server (support event & rpc methods)
        let ws = WebSocketServer::new(EventWebSocketHandler::new(
            rpc_handler,
            config.notify_events_concurrency,
        ));

        let server = Arc::new(Self {
            handle: Mutex::new(None),
            websocket: ws,
            getwork,
            websocket_security: Arc::clone(&websocket_security),
            a2a_grpc_shutdown: Mutex::new(None),
        });

        // Spawn a background task to periodically clean up rate limiter entries
        {
            let security = Arc::clone(&websocket_security);
            spawn_task("ws-security-cleanup", async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    security.cleanup().await;
                }
            });
        }

        let prometheus = if config.prometheus.enable {
            let (recorder, _) = PrometheusBuilder::new()
                .build()
                .context("Failed to create Prometheus handler")?;

            let handle = recorder.handle();
            metrics::set_global_recorder(Box::new(recorder))
                .context("Failed to set global recorder for Prometheus")?;

            info!(
                "Prometheus metrics enabled on route: {}",
                config.prometheus.route
            );
            Some((config.prometheus.route, handle))
        } else {
            None
        };

        {
            let clone = Arc::clone(&server);
            let enable_a2a = config.enable_a2a;
            let builder = HttpServer::new(move || {
                let server = Arc::clone(&clone);
                let mut app = App::new()
                    .app_data(web::Data::from(server))
                    .app_data(web::Data::new(
                        prometheus.as_ref().map(|(_, handle)| handle.clone()),
                    ))
                    // Traditional HTTP
                    .route(
                        "/json_rpc",
                        web::post().to(json_rpc::<Arc<Blockchain<S>>, DaemonRpcServer<S>>),
                    )
                    // WebSocket support with security enforcement
                    .route("/json_rpc", web::get().to(secure_websocket::<S>))
                    .route(
                        "/getwork/{address}/{worker}",
                        web::get().to(getwork_endpoint::<S>),
                    )
                    .service(index);

                if enable_a2a {
                    // Unversioned A2A endpoints
                    app = app
                        .route(
                            "/.well-known/agent-card.json",
                            web::get().to(a2a::agent_card::<S>),
                        )
                        .route(
                            "/agents:register",
                            web::post().to(agent_registry::register_agent_http::<S>),
                        )
                        .route(
                            "/agents:discover",
                            web::post().to(agent_registry::discover_agents_http::<S>),
                        )
                        .route(
                            "/agents:discover",
                            web::get().to(agent_registry::discover_agents_http_get::<S>),
                        )
                        .route(
                            "/agents:by-account",
                            web::get().to(agent_registry::get_agent_by_account_http::<S>),
                        )
                        .route(
                            "/committees:members",
                            web::post().to(agent_registry::discover_committee_members_http::<S>),
                        )
                        .route(
                            "/committees/{id}:members",
                            web::get().to(agent_registry::discover_committee_members_http_get::<S>),
                        )
                        .route(
                            "/escrows:pending",
                            web::post().to(escrow::get_pending_releases_http::<S>),
                        )
                        .route(
                            "/escrows:pending",
                            web::get().to(escrow::get_pending_releases_http_get::<S>),
                        )
                        .route(
                            "/agents",
                            web::get().to(agent_registry::list_agents_http::<S>),
                        )
                        .route(
                            "/agents/{id}",
                            web::get().to(agent_registry::get_agent_http::<S>),
                        )
                        .route(
                            "/agents/{id}",
                            web::patch().to(agent_registry::update_agent_http::<S>),
                        )
                        .route(
                            "/agents/{id}",
                            web::delete().to(agent_registry::unregister_agent_http::<S>),
                        )
                        .route(
                            "/agents/{id}:heartbeat",
                            web::post().to(agent_registry::heartbeat_http::<S>),
                        )
                        .route("/message:send", web::post().to(a2a::send_message_http::<S>))
                        .route(
                            "/message:stream",
                            web::post().to(a2a::send_streaming_message_http::<S>),
                        )
                        .route("/tasks", web::get().to(a2a::list_tasks_http::<S>))
                        .route("/tasks/{id}", web::get().to(a2a::get_task_http::<S>))
                        .route(
                            "/tasks/{id}:cancel",
                            web::post().to(a2a::cancel_task_http::<S>),
                        )
                        .route(
                            "/tasks/{id}:subscribe",
                            web::post().to(a2a::subscribe_task_http::<S>),
                        )
                        .route(
                            "/tasks/{id}/pushNotificationConfigs",
                            web::post().to(a2a::set_task_push_config_http::<S>),
                        )
                        .route(
                            "/tasks/{id}/pushNotificationConfigs",
                            web::get().to(a2a::list_task_push_config_http::<S>),
                        )
                        .route(
                            "/tasks/{id}/pushNotificationConfigs/{configId}",
                            web::get().to(a2a::get_task_push_config_http::<S>),
                        )
                        .route(
                            "/tasks/{id}/pushNotificationConfigs/{configId}",
                            web::delete().to(a2a::delete_task_push_config_http::<S>),
                        )
                        .route(
                            "/extendedAgentCard",
                            web::get().to(a2a::get_extended_agent_card_http::<S>),
                        );
                    // Versioned A2A endpoints (/v1/...)
                    app = app
                        .route(
                            "/v1/agents:register",
                            web::post().to(agent_registry::register_agent_http::<S>),
                        )
                        .route(
                            "/v1/agents:discover",
                            web::post().to(agent_registry::discover_agents_http::<S>),
                        )
                        .route(
                            "/v1/agents:discover",
                            web::get().to(agent_registry::discover_agents_http_get::<S>),
                        )
                        .route(
                            "/v1/agents:by-account",
                            web::get().to(agent_registry::get_agent_by_account_http::<S>),
                        )
                        .route(
                            "/v1/committees:members",
                            web::post().to(agent_registry::discover_committee_members_http::<S>),
                        )
                        .route(
                            "/v1/committees/{id}:members",
                            web::get().to(agent_registry::discover_committee_members_http_get::<S>),
                        )
                        .route(
                            "/v1/escrows:pending",
                            web::post().to(escrow::get_pending_releases_http::<S>),
                        )
                        .route(
                            "/v1/escrows:pending",
                            web::get().to(escrow::get_pending_releases_http_get::<S>),
                        )
                        .route(
                            "/v1/agents",
                            web::get().to(agent_registry::list_agents_http::<S>),
                        )
                        .route(
                            "/v1/agents/{id}",
                            web::get().to(agent_registry::get_agent_http::<S>),
                        )
                        .route(
                            "/v1/agents/{id}",
                            web::patch().to(agent_registry::update_agent_http::<S>),
                        )
                        .route(
                            "/v1/agents/{id}",
                            web::delete().to(agent_registry::unregister_agent_http::<S>),
                        )
                        .route(
                            "/v1/agents/{id}:heartbeat",
                            web::post().to(agent_registry::heartbeat_http::<S>),
                        )
                        .route(
                            "/v1/message:send",
                            web::post().to(a2a::send_message_http::<S>),
                        )
                        .route(
                            "/v1/message:stream",
                            web::post().to(a2a::send_streaming_message_http::<S>),
                        )
                        .route("/v1/tasks", web::get().to(a2a::list_tasks_http::<S>))
                        .route("/v1/tasks/{id}", web::get().to(a2a::get_task_http::<S>))
                        .route(
                            "/v1/tasks/{id}:cancel",
                            web::post().to(a2a::cancel_task_http::<S>),
                        )
                        .route(
                            "/v1/tasks/{id}:subscribe",
                            web::post().to(a2a::subscribe_task_http::<S>),
                        )
                        .route(
                            "/v1/tasks/{id}/pushNotificationConfigs",
                            web::post().to(a2a::set_task_push_config_http::<S>),
                        )
                        .route(
                            "/v1/tasks/{id}/pushNotificationConfigs",
                            web::get().to(a2a::list_task_push_config_http::<S>),
                        )
                        .route(
                            "/v1/tasks/{id}/pushNotificationConfigs/{configId}",
                            web::get().to(a2a::get_task_push_config_http::<S>),
                        )
                        .route(
                            "/v1/tasks/{id}/pushNotificationConfigs/{configId}",
                            web::delete().to(a2a::delete_task_push_config_http::<S>),
                        )
                        .route(
                            "/v1/extendedAgentCard",
                            web::get().to(a2a::get_extended_agent_card_http::<S>),
                        );
                    app = app.route("/a2a/ws", web::get().to(a2a::a2a_websocket::<S>));
                }
                if let Some((route, _)) = &prometheus {
                    app = app.route(route, web::get().to(prometheus_metrics));
                }
                app
            })
            .disable_signals()
            .bind(&config.bind_address)?;

            let http_server = builder.workers(config.threads).run();

            {
                // save the server handle to be able to stop it later
                let handle = http_server.handle();
                let mut lock = server.handle.lock().await;
                *lock = Some(handle);
            }
            spawn_task("rpc-server", http_server);
        }
        if config.enable_a2a {
            let addr: SocketAddr = config
                .a2a_grpc_bind_address
                .parse::<SocketAddr>()
                .map_err(|e: std::net::AddrParseError| BlockchainError::Any(e.into()))?;
            let (tx, rx) = oneshot::channel::<()>();
            {
                let mut lock = server.a2a_grpc_shutdown.lock().await;
                *lock = Some(tx);
            }
            let blockchain = Arc::clone(&blockchain);
            spawn_task("a2a-grpc", async move {
                let service = crate::a2a::grpc::service::A2AGrpcService::new(blockchain);
                let svc =
                    crate::a2a::grpc::proto::a2a_service_server::A2aServiceServer::new(service);
                let reflection = match ReflectionBuilder::configure()
                    .register_encoded_file_descriptor_set(
                        crate::a2a::grpc::proto::FILE_DESCRIPTOR_SET,
                    )
                    .build_v1()
                {
                    Ok(r) => r,
                    Err(err) => {
                        log::error!("Failed to build a2a grpc reflection: {err}");
                        return;
                    }
                };
                if let Err(err) = Server::builder()
                    .add_service(svc)
                    .add_service(reflection)
                    .serve_with_shutdown(addr, async move {
                        let _ = rx.await;
                    })
                    .await
                {
                    if log::log_enabled!(log::Level::Error) {
                        error!("A2A gRPC server error: {}", err);
                    }
                }
            });
        }
        Ok(server)
    }

    pub async fn get_tracked_events(&self) -> HashSet<NotifyEvent> {
        self.get_websocket()
            .get_handler()
            .get_tracked_events()
            .await
    }

    pub async fn is_event_tracked(&self, event: &NotifyEvent) -> bool {
        self.get_websocket()
            .get_handler()
            .is_event_tracked(event)
            .await
    }

    pub async fn notify_clients_with<V: serde::Serialize>(&self, event: &NotifyEvent, value: V) {
        if let Err(e) = self.notify_clients(event, json!(value)).await {
            if log::log_enabled!(log::Level::Error) {
                error!("Error while notifying event {:?}: {}", event, e);
            }
        }
    }

    pub async fn notify_clients(
        &self,
        event: &NotifyEvent,
        value: Value,
    ) -> Result<(), anyhow::Error> {
        self.get_websocket()
            .get_handler()
            .notify(event, value)
            .await;
        Ok(())
    }

    pub async fn stop(&self) {
        if log::log_enabled!(log::Level::Info) {
            info!("Stopping RPC Server...");
        }
        let mut grpc = self.a2a_grpc_shutdown.lock().await;
        if let Some(shutdown) = grpc.take() {
            let _ = shutdown.send(());
        }
        let mut handle = self.handle.lock().await;
        if let Some(handle) = handle.take() {
            handle.stop(false).await;
            if log::log_enabled!(log::Level::Info) {
                info!("RPC Server is now stopped!");
            }
        } else {
            if log::log_enabled!(log::Level::Warn) {
                warn!("RPC Server is not running!");
            }
        }
    }

    pub fn getwork_server(&self) -> &Option<WebSocketServerShared<GetWorkServer<S>>> {
        &self.getwork
    }

    /// Get the WebSocket security instance
    pub fn websocket_security(&self) -> &Arc<WebSocketSecurity> {
        &self.websocket_security
    }
}

impl<S: Storage> WebSocketServerHandler<EventWebSocketHandler<Arc<Blockchain<S>>, NotifyEvent>>
    for DaemonRpcServer<S>
{
    fn get_websocket(
        &self,
    ) -> &WebSocketServerShared<EventWebSocketHandler<Arc<Blockchain<S>>, NotifyEvent>> {
        &self.websocket
    }
}

impl<S: Storage> RPCServerHandler<Arc<Blockchain<S>>> for DaemonRpcServer<S> {
    fn get_rpc_handler(&self) -> &RPCHandler<Arc<Blockchain<S>>> {
        self.get_websocket().get_handler().get_rpc_handler()
    }
}

#[get("/")]
async fn index() -> impl Responder {
    HttpResponse::Ok().body(format!("Hello, world!\nRunning on: {}", config::VERSION))
}

async fn prometheus_metrics(handle: Data<Option<PrometheusHandle>>) -> Result<HttpResponse, Error> {
    Ok(match handle.as_ref() {
        Some(handle) => {
            let metrics = handle.render();
            HttpResponse::Ok()
                .content_type("text/plain; version=0.0.4")
                .body(metrics)
        }
        None => HttpResponse::NotFound().body("Prometheus metrics are not enabled"),
    })
}

/// Secure WebSocket endpoint with origin validation and rate limiting
/// This endpoint enforces the following security checks before establishing the connection:
/// 1. Origin validation (CORS protection)
/// 2. Connection rate limiting (per-IP DoS protection)
async fn secure_websocket<S: Storage>(
    server: Data<DaemonRpcServer<S>>,
    request: HttpRequest,
    stream: Payload,
) -> Result<HttpResponse, Error> {
    let security = server.websocket_security();

    // 1. Validate Origin header (CORS protection)
    let origin = request
        .headers()
        .get("Origin")
        .and_then(|v| v.to_str().ok());

    if let Err(e) = security.validate_origin(origin) {
        if log::log_enabled!(log::Level::Warn) {
            warn!("WebSocket connection rejected: {}", e);
        }
        return Ok(HttpResponse::Forbidden().body(e.to_string()));
    }

    // 2. Check connection rate limit (per-IP DoS protection)
    if let Some(peer_addr) = request.peer_addr() {
        if let Err(e) = security.check_connection_rate(peer_addr.ip()).await {
            if log::log_enabled!(log::Level::Warn) {
                warn!("WebSocket connection rejected: {}", e);
            }
            return Ok(HttpResponse::TooManyRequests().body(e.to_string()));
        }
    }

    // Security checks passed, delegate to the WebSocket handler
    server
        .get_websocket()
        .handle_connection(request, stream)
        .await
}

async fn getwork_endpoint<S: Storage>(
    server: Data<DaemonRpcServer<S>>,
    request: HttpRequest,
    stream: Payload,
) -> Result<HttpResponse, Error> {
    match &server.getwork {
        Some(getwork) => getwork.handle_connection(request, stream).await,
        None => Ok(HttpResponse::NotFound()
            .reason("GetWork server is not enabled")
            .finish()), // getwork server is not started
    }
}
