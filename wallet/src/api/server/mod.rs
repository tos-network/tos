mod rpc_server;
mod xswd_server;

pub use rpc_server::{
    WalletRpcServer,
    WalletRpcServerShared,
    AuthConfig
};

use serde::Serialize;
use serde_json::json;
use tos_common::{
    api::wallet::NotifyEvent,
    rpc::server::WebSocketServerHandler
};
pub use xswd_server::{
    XSWDServer,
    XSWDWebSocketHandler
};

use crate::api::XSWDHandler;


pub enum APIServer<W>
where
    W: Clone + Send + Sync + XSWDHandler + 'static
{
    RPCServer(WalletRpcServerShared<W>),
    XSWD(XSWDServer<W>)
}

impl<W> APIServer<W>
where
    W: Clone + Send + Sync + XSWDHandler + 'static
{
    pub async fn notify_event<V: Serialize>(&self, event: &NotifyEvent, value: &V) {
        let json = json!(value);
        match self {
            APIServer::RPCServer(server) => {
                server.get_websocket().get_handler().notify(event, json).await;
            },
            APIServer::XSWD(xswd) => {
                xswd.get_handler().notify(event, json).await;
            }
        }
    }

    pub async fn stop(self) {
        match self {
            APIServer::RPCServer(server) => {
                server.stop().await;
            },
            APIServer::XSWD(xswd) => {
                xswd.stop().await;
            }
        }
    }
}