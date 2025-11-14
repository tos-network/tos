use std::fmt::{Display, Formatter};

#[cfg(feature = "rpc-server")]
use actix_web::{HttpResponse, ResponseError};

use crate::{
    rpc::{Id, JSON_RPC_VERSION},
    serializer::ReaderError,
};
use anyhow::Error as AnyError;
use serde_json::{Error as SerdeError, Value};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum InternalRpcError {
    #[error("Internal error: {}", _0)]
    InternalError(&'static str),
    #[error("Invalid context")]
    InvalidContext,
    #[error("Invalid body in request")]
    ParseBodyError,
    #[error("Invalid JSON request")]
    InvalidJSONRequest,
    #[error("Invalid request: {}", _0)]
    InvalidRequestStr(&'static str),
    #[error("Invalid params: {}", _0)]
    InvalidJSONParams(#[from] SerdeError),
    #[error("Invalid params: {}", _0)]
    InvalidParams(&'static str),
    #[error("Invalid params: {:#}", _0)]
    InvalidParamsAny(AnyError),
    #[error("Expected parameters for this method but was not present")]
    ExpectedParams,
    #[error("Unexpected parameters for this method")]
    UnexpectedParams,
    #[error("Expected json_rpc set to '2.0'")]
    InvalidVersion,
    #[error("Method '{}' in request was not found", _0)]
    MethodNotFound(String),
    #[error(transparent)]
    DeserializerError(#[from] ReaderError),
    #[error(transparent)]
    AnyError(#[from] AnyError),
    #[error("Websocket client was not found")]
    ClientNotFound,
    #[error("Event is not subscribed")]
    EventNotSubscribed,
    #[error("Event is already subscribed")]
    EventAlreadySubscribed,
    #[error(transparent)]
    SerializeResponse(SerdeError),
    // Custom errors must have a code between -3 and -31999
    #[error("{:#}", _1)]
    CustomAny(i16, AnyError),
    #[error("{}", _1)]
    Custom(i16, String),
    #[error("{}", _1)]
    CustomStr(i16, &'static str),
    #[error("batch limit exceeded")]
    BatchLimitExceeded,
}

impl InternalRpcError {
    pub fn get_code(&self) -> i16 {
        match self {
            // JSON RPC errors
            Self::ParseBodyError => -32700,
            Self::InvalidJSONRequest
            | Self::InvalidRequestStr(_)
            | Self::InvalidVersion
            | Self::BatchLimitExceeded => -32600,
            Self::MethodNotFound(_) => -32601,
            Self::InvalidJSONParams(_)
            | Self::InvalidParams(_)
            | Self::InvalidParamsAny(_)
            | Self::UnexpectedParams
            | Self::ExpectedParams => -32602,
            // Internal errors
            Self::InternalError(_) => -32603,
            // 32000 to -32099	Server error (Reserved for implementation-defined server-errors)
            Self::DeserializerError(_) => -32000,
            Self::InvalidContext => -32001,
            Self::ClientNotFound => -32002,
            Self::SerializeResponse(_) => -32003,
            Self::AnyError(_) => -32004,
            // Events invalid requests
            Self::EventNotSubscribed => -1,
            Self::EventAlreadySubscribed => -2,
            // Custom errors
            Self::Custom(code, _) | Self::CustomStr(code, _) | Self::CustomAny(code, _) => *code,
        }
    }
}

#[derive(Debug)]
pub struct RpcResponseError {
    id: Option<Id>,
    error: InternalRpcError,
}

impl RpcResponseError {
    pub fn new<T: Into<InternalRpcError>>(id: Option<Id>, error: T) -> Self {
        Self {
            id,
            error: error.into(),
        }
    }

    pub fn get_id(&self) -> Value {
        match &self.id {
            // SAFE: Converting simple types to JSON Value - use direct conversion instead of json!()
            Some(id) => serde_json::to_value(id).unwrap_or(Value::Null),
            None => Value::Null,
        }
    }

    pub fn to_json(&self) -> Value {
        // SAFE: Build JSON manually instead of using json!() macro to avoid unwrap
        let mut obj = serde_json::Map::new();
        obj.insert(
            "jsonrpc".to_string(),
            Value::String(JSON_RPC_VERSION.to_string()),
        );
        obj.insert("id".to_string(), self.get_id());

        let mut error_obj = serde_json::Map::new();
        error_obj.insert(
            "code".to_string(),
            Value::Number(self.error.get_code().into()),
        );
        error_obj.insert(
            "message".to_string(),
            Value::String(format!("{:#}", self.error)),
        );
        obj.insert("error".to_string(), Value::Object(error_obj));

        Value::Object(obj)
    }
}

impl Display for RpcResponseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "RpcError[id: {}, error: {:#}]",
            self.get_id(),
            self.error
        )
    }
}

#[cfg(feature = "rpc-server")]
impl ResponseError for RpcResponseError {
    fn error_response(&self) -> HttpResponse {
        HttpResponse::Ok().json(self.to_json())
    }
}
