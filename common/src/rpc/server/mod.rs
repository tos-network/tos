pub mod websocket;

use actix_web::{
    web::{self, Data, Payload},
    HttpRequest, HttpResponse, Responder,
};
use std::net::IpAddr;

use self::websocket::{WebSocketHandler, WebSocketServerShared};
use super::{RPCHandler, RpcResponseError};
use crate::context::Context;

/// Client address information for RPC requests.
/// Used to enforce localhost-only access for admin methods.
#[derive(Debug, Clone)]
pub struct ClientAddr(pub Option<IpAddr>);

impl ClientAddr {
    /// Check if the client is connecting from localhost (loopback address)
    pub fn is_loopback(&self) -> bool {
        self.0.is_some_and(|ip| ip.is_loopback())
    }
}

// trait to retrieve easily a JSON RPC handler for registered route
pub trait RPCServerHandler<T: Send + Clone> {
    fn get_rpc_handler(&self) -> &RPCHandler<T>;
}

// JSON RPC handler endpoint
pub async fn json_rpc<T, H>(
    server: Data<H>,
    request: HttpRequest,
    body: web::Bytes,
) -> Result<impl Responder, RpcResponseError>
where
    T: Send + Sync + Clone + 'static,
    H: RPCServerHandler<T>,
{
    // Extract client IP address from the request
    let client_addr = ClientAddr(request.peer_addr().map(|addr| addr.ip()));

    // Create context with client address for admin method verification
    let mut context = Context::new();
    context.store(server.get_rpc_handler().get_data().clone());
    context.store(client_addr);

    let result = server
        .get_rpc_handler()
        .handle_request_with_context(context, &body)
        .await?;
    Ok(HttpResponse::Ok().json(result))
}

// trait to retrieve easily a websocket handler for registered route
pub trait WebSocketServerHandler<H: WebSocketHandler> {
    fn get_websocket(&self) -> &WebSocketServerShared<H>;
}

// WebSocket JSON RPC handler endpoint
pub async fn websocket<H, S>(
    server: Data<S>,
    request: HttpRequest,
    body: Payload,
) -> Result<impl Responder, actix_web::Error>
where
    H: WebSocketHandler + 'static,
    S: WebSocketServerHandler<H>,
{
    let response = server
        .get_websocket()
        .handle_connection(request, body)
        .await?;
    Ok(response)
}
