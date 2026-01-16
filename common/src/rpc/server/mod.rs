pub mod websocket;

use actix_web::{
    web::{self, Data, Payload},
    HttpRequest, HttpResponse, Responder,
};
use std::{collections::HashMap, net::IpAddr};

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

/// Basic request metadata available to RPC methods.
#[derive(Debug, Clone)]
pub struct RequestMetadata {
    pub method: String,
    pub path: String,
    pub query: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl RequestMetadata {
    pub fn from_http_request(request: &HttpRequest, body: &[u8]) -> Self {
        let mut headers = HashMap::new();
        for (name, value) in request.headers().iter() {
            if let Ok(value) = value.to_str() {
                headers.insert(name.as_str().to_ascii_lowercase(), value.to_string());
            }
        }

        Self {
            method: request.method().to_string(),
            path: request.uri().path().to_string(),
            query: request.uri().query().unwrap_or("").to_string(),
            headers,
            body: body.to_vec(),
        }
    }

    pub fn from_websocket_request(request: &websocket::HttpRequest) -> Self {
        let mut headers = HashMap::new();
        for (name, value) in request.headers().iter() {
            if let Ok(value) = value.to_str() {
                headers.insert(name.as_str().to_ascii_lowercase(), value.to_string());
            }
        }

        Self {
            method: request.method().to_string(),
            path: request.uri().path().to_string(),
            query: request.uri().query().unwrap_or("").to_string(),
            headers,
            body: Vec::new(),
        }
    }
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
    context.store(RequestMetadata::from_http_request(&request, &body));

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
