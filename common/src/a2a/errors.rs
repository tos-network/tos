use thiserror::Error;

pub type A2AResult<T> = Result<T, A2AError>;

#[derive(Debug, Error)]
pub enum A2AError {
    #[error("parse error: {message}")]
    ParseError { message: String },
    #[error("invalid request: {message}")]
    InvalidRequest { message: String },
    #[error("method not found: {method}")]
    MethodNotFound { method: String },
    #[error("invalid params: {message}")]
    InvalidParams { message: String },
    #[error("internal error: {message}")]
    InternalError { message: String },

    #[error("task not found: {task_id}")]
    TaskNotFoundError { task_id: String },
    #[error("task not cancelable: {task_id}")]
    TaskNotCancelableError { task_id: String },
    #[error("push notifications not supported")]
    PushNotificationNotSupportedError,
    #[error("unsupported operation: {reason}")]
    UnsupportedOperationError { reason: String },
    #[error("content type not supported: {content_type}")]
    ContentTypeNotSupportedError { content_type: String },
    #[error("invalid agent response: {message}")]
    InvalidAgentResponseError { message: String },
    #[error("extended agent card not configured")]
    ExtendedAgentCardNotConfiguredError,
    #[error("extension support required: {extension_uri}")]
    ExtensionSupportRequiredError { extension_uri: String },
    #[error("version not supported: {version}")]
    VersionNotSupportedError { version: String },

    #[error("TOS identity verification failed: {agent_account}")]
    TosIdentityVerificationFailed { agent_account: String },
    #[error("TOS signature invalid")]
    TosSignatureInvalid,
    #[error("TOS session key expired")]
    TosSessionKeyExpired,
    #[error("TOS account frozen")]
    TosAccountFrozen,
    #[error("TOS escrow failed: {reason}")]
    TosEscrowFailed { reason: String },
}

impl A2AError {
    pub fn code(&self) -> i32 {
        match self {
            Self::ParseError { .. } => -32700,
            Self::InvalidRequest { .. } => -32600,
            Self::MethodNotFound { .. } => -32601,
            Self::InvalidParams { .. } => -32602,
            Self::InternalError { .. } => -32603,
            Self::TaskNotFoundError { .. } => -32001,
            Self::TaskNotCancelableError { .. } => -32002,
            Self::PushNotificationNotSupportedError => -32003,
            Self::UnsupportedOperationError { .. } => -32004,
            Self::ContentTypeNotSupportedError { .. } => -32005,
            Self::InvalidAgentResponseError { .. } => -32006,
            Self::ExtendedAgentCardNotConfiguredError => -32007,
            Self::ExtensionSupportRequiredError { .. } => -32008,
            Self::VersionNotSupportedError { .. } => -32009,
            Self::TosIdentityVerificationFailed { .. } => -32100,
            Self::TosSignatureInvalid => -32101,
            Self::TosSessionKeyExpired => -32102,
            Self::TosAccountFrozen => -32103,
            Self::TosEscrowFailed { .. } => -32104,
        }
    }
}
