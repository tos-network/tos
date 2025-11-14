#![allow(clippy::disallowed_methods)]

// RPC modules use json! macro which internally uses unwrap
// This is acceptable for JSON construction which should never fail for valid literals

pub mod getwork;
pub mod rpc;
pub mod websocket;
mod websocket_wrapper;

use crate::core::{
    blockchain::Blockchain, config::RPCConfig, error::BlockchainError, storage::Storage,
};
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
use std::{collections::HashSet, sync::Arc};
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
use websocket::{WebSocketSecurity, WebSocketSecurityConfig};

pub type SharedDaemonRpcServer<S> = Arc<DaemonRpcServer<S>>;

pub struct DaemonRpcServer<S: Storage> {
    handle: Mutex<Option<ServerHandle>>,
    websocket: WebSocketServerShared<EventWebSocketHandler<Arc<Blockchain<S>>, NotifyEvent>>,
    getwork: Option<WebSocketServerShared<GetWorkServer<S>>>,
    websocket_security: Arc<WebSocketSecurity>,
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
        let mut rpc_handler = RPCHandler::new(blockchain);
        rpc::register_methods(&mut rpc_handler, !config.getwork.disable);

        // create the default websocket server (support event & rpc methods)
        let ws = WebSocketServer::new(EventWebSocketHandler::new(
            rpc_handler,
            config.notify_events_concurrency,
        ));

        // Configure WebSocket security
        let allowed_origins = config
            .ws_allowed_origins
            .as_ref()
            .map(|origins| origins.split(',').map(|s| s.trim().to_string()).collect())
            .unwrap_or_else(|| vec!["http://localhost:3000".to_string()]);

        let ws_security_config = WebSocketSecurityConfig {
            allowed_origins,
            require_auth: config.ws_require_auth,
            max_message_size: config.ws_max_message_size,
            max_subscriptions_per_connection: config.ws_max_subscriptions,
            max_connections_per_ip_per_minute: config.ws_max_connections_per_minute,
            max_messages_per_connection_per_second: config.ws_max_messages_per_second,
        };

        let websocket_security = Arc::new(WebSocketSecurity::new(ws_security_config));

        if log::log_enabled!(log::Level::Info) {
            info!("WebSocket security initialized with:");
            info!(
                "  - Origin validation: {:?}",
                websocket_security.config().allowed_origins
            );
            info!(
                "  - Require auth: {}",
                websocket_security.config().require_auth
            );
            info!(
                "  - Max message size: {} bytes",
                websocket_security.config().max_message_size
            );
            info!(
                "  - Max subscriptions: {}",
                websocket_security.config().max_subscriptions_per_connection
            );
            info!(
                "  - Connection rate limit: {}/min",
                websocket_security
                    .config()
                    .max_connections_per_ip_per_minute
            );
            info!(
                "  - Message rate limit: {}/sec",
                websocket_security
                    .config()
                    .max_messages_per_connection_per_second
            );
        }

        let server = Arc::new(Self {
            handle: Mutex::new(None),
            websocket: ws,
            getwork,
            websocket_security,
        });

        let prometheus = if config.prometheus.enable {
            let (recorder, _) = PrometheusBuilder::new()
                .build()
                .context("Failed to create Prometheus handler")?;

            let handle = recorder.handle();
            metrics::set_global_recorder(Box::new(recorder))
                .context("Failed to set global recorder for Prometheus")?;

            if log::log_enabled!(log::Level::Info) {
                info!(
                    "Prometheus metrics enabled on route: {}",
                    config.prometheus.route
                );
            }
            Some((config.prometheus.route, handle))
        } else {
            None
        };

        // SECURITY WARNING: Check if RPC is exposed to network
        if config.bind_address.starts_with("0.0.0.0") {
            warn!("⚠️  SECURITY WARNING: RPC server is bound to 0.0.0.0 (all interfaces)");
            warn!(
                "⚠️  This exposes administrative endpoints to the network WITHOUT authentication!"
            );
            warn!("⚠️  Attackers can:");
            warn!("⚠️    - Submit malicious blocks");
            warn!("⚠️    - Manipulate mempool");
            warn!("⚠️    - Tamper with peer list");
            warn!("⚠️    - Cause DoS via resource exhaustion");
            warn!("⚠️  ");
            warn!("⚠️  RECOMMENDED: Use 127.0.0.1:8080 for localhost-only access");
            warn!("⚠️  If remote access is required, use a firewall to restrict access");
            warn!("⚠️  ");
        }

        if log::log_enabled!(log::Level::Info) {
            info!("Starting RPC server on {}", config.bind_address);
        }

        {
            let clone = Arc::clone(&server);
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
                    // WebSocket support with security wrapper
                    .route("/json_rpc", web::get().to(secure_websocket_endpoint::<S>))
                    .route(
                        "/getwork/{address}/{worker}",
                        web::get().to(getwork_endpoint::<S>),
                    )
                    .service(index);

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

        // Spawn cleanup task for WebSocket security rate limiters
        {
            let security = Arc::clone(&server.websocket_security);
            spawn_task("websocket-security-cleanup", async move {
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(120)).await;
                    security.cleanup().await;
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
        info!("Stopping RPC Server...");
        let mut handle = self.handle.lock().await;
        if let Some(handle) = handle.take() {
            handle.stop(false).await;
            info!("RPC Server is now stopped!");
        } else {
            warn!("RPC Server is not running!");
        }
    }

    pub fn getwork_server(&self) -> &Option<WebSocketServerShared<GetWorkServer<S>>> {
        &self.getwork
    }

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

async fn secure_websocket_endpoint<S: Storage>(
    server: Data<DaemonRpcServer<S>>,
    request: HttpRequest,
    stream: Payload,
) -> Result<HttpResponse, Error> {
    websocket_wrapper::secure_websocket_handler(
        &server.websocket,
        &server.websocket_security,
        request,
        stream,
    )
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
