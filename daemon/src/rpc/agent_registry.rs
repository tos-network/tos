use std::str::FromStr;
use std::sync::Arc;

use actix_web::{
    error::{ErrorBadRequest, ErrorInternalServerError, ErrorNotFound, ErrorUnauthorized},
    web, Error as ActixError, HttpRequest, HttpResponse,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use tos_common::{
    a2a::{verify_tos_signature, AgentCard, TosSignature, TosSignerType},
    a2a::{HEADER_VERSION, PROTOCOL_VERSION},
    async_handler,
    context::Context,
    crypto::{hash, Hash, PublicKey},
    rpc::{
        parse_params,
        server::{RPCServerHandler, RequestMetadata},
        InternalRpcError, RPCHandler,
    },
};

use crate::{
    a2a::registry::{global_registry, AgentFilter, RegistryError},
    core::{blockchain::Blockchain, storage::Storage},
    rpc::DaemonRpcServer,
};

const REGISTRY_SIGNATURE_DOMAIN: &[u8] = b"TOS_AGENT_REGISTRY_V1";
const DEFAULT_HEARTBEAT_INTERVAL_SECS: u32 = 30;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterAgentRequest {
    pub agent_card: AgentCard,
    pub endpoint_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tos_signature: Option<TosSignature>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterAgentResponse {
    pub agent_id: String,
    pub registered_at: i64,
    pub heartbeat_interval_secs: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnregisterAgentRequest {
    pub agent_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAgentRequest {
    pub agent_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatRequest {
    pub agent_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatResponse {
    pub agent_id: String,
    pub last_seen: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AgentPath {
    pub id: String,
}

#[derive(Debug, Error)]
pub enum AgentRegistryRpcError {
    #[error("missing TOS signature")]
    MissingTosSignature,
    #[error("missing TOS identity")]
    MissingTosIdentity,
    #[error("unsupported protocol version: {0}")]
    InvalidVersion(String),
    #[error("invalid agent id")]
    InvalidAgentId,
    #[error("failed to serialize agent card")]
    SerializeAgentCard,
    #[error("signature verification failed: {0}")]
    SignatureVerification(String),
    #[error("storage error: {0}")]
    StorageError(String),
    #[error(transparent)]
    Registry(#[from] RegistryError),
}

/// Register agent registry JSON-RPC methods.
pub fn register_agent_registry_methods<S: Storage>(handler: &mut RPCHandler<Arc<Blockchain<S>>>) {
    handler.register_method("register_agent", async_handler!(register_agent::<S>));
    handler.register_method("unregister_agent", async_handler!(unregister_agent::<S>));
    handler.register_method("get_agent", async_handler!(get_agent::<S>));
    handler.register_method("discover_agents", async_handler!(discover_agents::<S>));
    handler.register_method("heartbeat", async_handler!(heartbeat::<S>));
}

async fn register_agent<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_registry_auth_context(context).await?;
    let request: RegisterAgentRequest = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let response = register_agent_impl(Arc::clone(blockchain), request).await?;

    serde_json::to_value(response).map_err(InternalRpcError::SerializeResponse)
}

async fn unregister_agent<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_registry_auth_context(context).await?;
    let request: UnregisterAgentRequest = parse_params(body)?;
    let agent_id = parse_agent_id(&request.agent_id)?;

    let registry = global_registry();
    registry
        .unregister(&agent_id)
        .await
        .map_err(AgentRegistryRpcError::from)?;

    Ok(Value::Null)
}

async fn get_agent<S: Storage>(context: &Context, body: Value) -> Result<Value, InternalRpcError> {
    require_registry_auth_context(context).await?;
    let request: GetAgentRequest = parse_params(body)?;
    let agent_id = parse_agent_id(&request.agent_id)?;

    let registry = global_registry();
    let agent = registry.get(&agent_id).await;
    let response = agent.map(|agent| agent.agent_card);
    serde_json::to_value(response).map_err(InternalRpcError::SerializeResponse)
}

async fn discover_agents<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_registry_auth_context(context).await?;
    let filter: AgentFilter = parse_params(body)?;
    let registry = global_registry();
    let agents = registry.filter(&filter).await;
    let cards: Vec<AgentCard> = agents.into_iter().map(|agent| agent.agent_card).collect();
    serde_json::to_value(cards).map_err(InternalRpcError::SerializeResponse)
}

async fn heartbeat<S: Storage>(context: &Context, body: Value) -> Result<Value, InternalRpcError> {
    require_registry_auth_context(context).await?;
    let request: HeartbeatRequest = parse_params(body)?;
    let agent_id = parse_agent_id(&request.agent_id)?;
    let registry = global_registry();
    let last_seen = registry
        .heartbeat(&agent_id)
        .await
        .map_err(AgentRegistryRpcError::from)?;
    let response = HeartbeatResponse {
        agent_id: agent_id.to_hex(),
        last_seen,
    };
    serde_json::to_value(response).map_err(InternalRpcError::SerializeResponse)
}

/// HTTP endpoint: POST /agents:register
pub async fn register_agent_http<S: Storage>(
    server: web::Data<DaemonRpcServer<S>>,
    request: HttpRequest,
    body: web::Bytes,
) -> Result<HttpResponse, ActixError> {
    require_registry_auth_http(&request, &body).await?;
    let request: RegisterAgentRequest =
        serde_json::from_slice(&body).map_err(|e| ErrorBadRequest(e.to_string()))?;
    let blockchain = server.get_rpc_handler().get_data().clone();
    let response = register_agent_impl(blockchain, request)
        .await
        .map_err(map_http_error)?;
    Ok(HttpResponse::Ok().json(response))
}

/// HTTP endpoint: GET /agents/{id}
pub async fn get_agent_http<S: Storage>(
    _server: web::Data<DaemonRpcServer<S>>,
    request: HttpRequest,
    path: web::Path<AgentPath>,
) -> Result<HttpResponse, ActixError> {
    require_registry_auth_http(&request, &[]).await?;
    let agent_id = parse_agent_id_http(&path.id).map_err(map_http_error)?;
    let registry = global_registry();
    let agent = registry.get(&agent_id).await;
    match agent {
        Some(agent) => Ok(HttpResponse::Ok().json(agent.agent_card)),
        None => Err(ErrorNotFound("Agent not found")),
    }
}

/// HTTP endpoint: DELETE /agents/{id}
pub async fn unregister_agent_http<S: Storage>(
    _server: web::Data<DaemonRpcServer<S>>,
    request: HttpRequest,
    path: web::Path<AgentPath>,
) -> Result<HttpResponse, ActixError> {
    require_registry_auth_http(&request, &[]).await?;
    let agent_id = parse_agent_id_http(&path.id).map_err(map_http_error)?;
    let registry = global_registry();
    registry
        .unregister(&agent_id)
        .await
        .map_err(AgentRegistryRpcError::from)
        .map_err(map_http_error)?;
    Ok(HttpResponse::NoContent().finish())
}

/// HTTP endpoint: POST /agents:discover
pub async fn discover_agents_http<S: Storage>(
    _server: web::Data<DaemonRpcServer<S>>,
    request: HttpRequest,
    body: web::Bytes,
) -> Result<HttpResponse, ActixError> {
    require_registry_auth_http(&request, &body).await?;
    let filter: AgentFilter =
        serde_json::from_slice(&body).map_err(|e| ErrorBadRequest(e.to_string()))?;
    let registry = global_registry();
    let agents = registry.filter(&filter).await;
    let cards: Vec<AgentCard> = agents.into_iter().map(|agent| agent.agent_card).collect();
    Ok(HttpResponse::Ok().json(cards))
}

fn parse_agent_id(value: &str) -> Result<Hash, InternalRpcError> {
    Hash::from_str(value).map_err(|_| map_error(AgentRegistryRpcError::InvalidAgentId))
}

fn parse_agent_id_http(value: &str) -> Result<Hash, AgentRegistryRpcError> {
    Hash::from_str(value).map_err(|_| AgentRegistryRpcError::InvalidAgentId)
}

fn validate_a2a_version(
    headers: &std::collections::HashMap<String, String>,
) -> Result<(), AgentRegistryRpcError> {
    if let Some(version) = headers.get(HEADER_VERSION) {
        if version != PROTOCOL_VERSION && !version.starts_with("1.") {
            return Err(AgentRegistryRpcError::InvalidVersion(version.clone()));
        }
    }
    Ok(())
}

async fn require_registry_auth_http(request: &HttpRequest, body: &[u8]) -> Result<(), ActixError> {
    let meta = RequestMetadata::from_http_request(request, body);
    validate_a2a_version(&meta.headers).map_err(map_http_error)?;
    crate::a2a::auth::authorize_metadata(&meta)
        .await
        .map_err(|e| ErrorUnauthorized(e.to_string()))?;
    Ok(())
}

async fn require_registry_auth_context(context: &Context) -> Result<(), InternalRpcError> {
    let meta = context
        .get::<RequestMetadata>()
        .map_err(|_| InternalRpcError::InvalidContext)?;
    validate_a2a_version(&meta.headers)
        .map_err(|err| InternalRpcError::Custom(-32602, err.to_string()))?;
    crate::a2a::auth::authorize_metadata(meta)
        .await
        .map_err(|e| InternalRpcError::Custom(-32098, e.to_string()))?;
    Ok(())
}

async fn register_agent_impl<S: Storage>(
    blockchain: Arc<Blockchain<S>>,
    request: RegisterAgentRequest,
) -> Result<RegisterAgentResponse, AgentRegistryRpcError> {
    let signature = request
        .tos_signature
        .as_ref()
        .ok_or(AgentRegistryRpcError::MissingTosSignature)?;
    let tos_identity = request
        .agent_card
        .tos_identity
        .as_ref()
        .ok_or(AgentRegistryRpcError::MissingTosIdentity)?;

    let message = build_registration_message(
        &tos_identity.agent_account,
        &request.endpoint_url,
        &request.agent_card,
        signature,
    )?;

    let (meta, session_key) = {
        let storage = blockchain.get_storage().read().await;
        let meta = storage
            .get_agent_account_meta(&tos_identity.agent_account)
            .await
            .map_err(|e| AgentRegistryRpcError::StorageError(e.to_string()))?;
        let session_key = if signature.signer == TosSignerType::SessionKey {
            let key_id = signature
                .session_key_id
                .ok_or(AgentRegistryRpcError::MissingTosSignature)?;
            storage
                .get_session_key(&tos_identity.agent_account, key_id)
                .await
                .map_err(|e| AgentRegistryRpcError::StorageError(e.to_string()))?
        } else {
            None
        };
        (meta, session_key)
    };

    let reader = PreloadedAgentAccountReader {
        agent_account: tos_identity.agent_account.clone(),
        meta,
        session_key,
        topoheight: blockchain.get_topo_height(),
    };

    verify_tos_signature(signature, &tos_identity.agent_account, &message, &reader)
        .map_err(|e| AgentRegistryRpcError::SignatureVerification(e.to_string()))?;

    let registry = global_registry();
    let registered = registry
        .register(request.agent_card, request.endpoint_url)
        .await
        .map_err(AgentRegistryRpcError::from)?;

    Ok(RegisterAgentResponse {
        agent_id: registered.agent_id.to_hex(),
        registered_at: registered.registered_at,
        heartbeat_interval_secs: DEFAULT_HEARTBEAT_INTERVAL_SECS,
    })
}

fn build_registration_message(
    agent_account: &PublicKey,
    endpoint_url: &str,
    agent_card: &AgentCard,
    signature: &TosSignature,
) -> Result<Vec<u8>, AgentRegistryRpcError> {
    let card_bytes =
        serde_json::to_vec(agent_card).map_err(|_| AgentRegistryRpcError::SerializeAgentCard)?;
    let card_hash = hash(&card_bytes);
    let mut message = Vec::with_capacity(
        REGISTRY_SIGNATURE_DOMAIN.len()
            + agent_account.as_bytes().len()
            + endpoint_url.len()
            + card_hash.as_bytes().len()
            + 16,
    );
    message.extend_from_slice(REGISTRY_SIGNATURE_DOMAIN);
    message.extend_from_slice(agent_account.as_bytes());
    message.extend_from_slice(endpoint_url.as_bytes());
    message.extend_from_slice(card_hash.as_bytes());
    message.extend_from_slice(&signature.timestamp.to_le_bytes());
    message.extend_from_slice(&signature.nonce.to_le_bytes());
    Ok(message)
}

fn map_error(err: AgentRegistryRpcError) -> InternalRpcError {
    match err {
        AgentRegistryRpcError::MissingTosSignature
        | AgentRegistryRpcError::MissingTosIdentity
        | AgentRegistryRpcError::InvalidAgentId
        | AgentRegistryRpcError::InvalidVersion(_)
        | AgentRegistryRpcError::SerializeAgentCard => {
            InternalRpcError::Custom(-32602, err.to_string())
        }
        AgentRegistryRpcError::SignatureVerification(message) => {
            InternalRpcError::Custom(-32080, message)
        }
        AgentRegistryRpcError::StorageError(message) => InternalRpcError::Custom(-32081, message),
        AgentRegistryRpcError::Registry(registry_err) => {
            InternalRpcError::Custom(-32082, registry_err.to_string())
        }
    }
}

fn map_http_error(err: AgentRegistryRpcError) -> ActixError {
    match err {
        AgentRegistryRpcError::MissingTosSignature
        | AgentRegistryRpcError::MissingTosIdentity
        | AgentRegistryRpcError::InvalidAgentId
        | AgentRegistryRpcError::InvalidVersion(_)
        | AgentRegistryRpcError::SerializeAgentCard => ErrorBadRequest(err.to_string()),
        AgentRegistryRpcError::SignatureVerification(message) => ErrorUnauthorized(message),
        AgentRegistryRpcError::StorageError(message) => ErrorInternalServerError(message),
        AgentRegistryRpcError::Registry(registry_err) => match registry_err {
            RegistryError::AgentNotFound => ErrorNotFound(registry_err.to_string()),
            RegistryError::AgentAlreadyRegistered | RegistryError::InvalidEndpointUrl => {
                ErrorBadRequest(registry_err.to_string())
            }
            RegistryError::SerializeAgentCard | RegistryError::TimestampOverflow => {
                ErrorInternalServerError(registry_err.to_string())
            }
        },
    }
}

struct PreloadedAgentAccountReader {
    agent_account: PublicKey,
    meta: Option<tos_common::account::AgentAccountMeta>,
    session_key: Option<tos_common::account::SessionKey>,
    topoheight: u64,
}

impl tos_common::a2a::AgentAccountReader for PreloadedAgentAccountReader {
    type Error = &'static str;

    fn get_agent_account_meta(
        &self,
        agent_account: &PublicKey,
    ) -> Result<Option<tos_common::account::AgentAccountMeta>, Self::Error> {
        if agent_account != &self.agent_account {
            return Ok(None);
        }
        Ok(self.meta.clone())
    }

    fn get_session_key(
        &self,
        agent_account: &PublicKey,
        key_id: u64,
    ) -> Result<Option<tos_common::account::SessionKey>, Self::Error> {
        if agent_account != &self.agent_account {
            return Ok(None);
        }
        Ok(self
            .session_key
            .as_ref()
            .filter(|key| key.key_id == key_id)
            .cloned())
    }

    fn get_topoheight(&self) -> Result<u64, Self::Error> {
        Ok(self.topoheight)
    }
}

impl From<AgentRegistryRpcError> for InternalRpcError {
    fn from(err: AgentRegistryRpcError) -> Self {
        map_error(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::test::TestRequest;
    use std::collections::HashSet;
    use std::sync::Once;
    use tos_common::a2a::{AgentCapabilities, AgentInterface, AgentSkill, TosSignerType};
    use tos_common::serializer::Serializer;

    static AUTH_INIT: Once = Once::new();

    fn init_auth() {
        AUTH_INIT.call_once(|| {
            crate::a2a::auth::set_auth_config(crate::a2a::auth::A2AAuthConfig {
                api_keys: HashSet::new(),
                oauth_issuer: None,
                oauth_jwks_url: None,
                oauth_audience: None,
                tos_skew_secs: 0,
                tos_nonce_ttl_secs: 0,
            });
        });
    }

    fn sample_card() -> AgentCard {
        AgentCard {
            protocol_version: "1.0".to_string(),
            name: "agent".to_string(),
            description: "test".to_string(),
            version: "0.0.1".to_string(),
            supported_interfaces: vec![AgentInterface {
                url: "http://example.com".to_string(),
                protocol_binding: "HTTP+JSON".to_string(),
                tenant: None,
            }],
            provider: None,
            icon_url: None,
            documentation_url: None,
            capabilities: AgentCapabilities {
                streaming: None,
                push_notifications: None,
                state_transition_history: None,
                extensions: Vec::new(),
                tos_on_chain_settlement: Some(false),
            },
            security_schemes: std::collections::HashMap::new(),
            security: Vec::new(),
            default_input_modes: vec!["text/plain".to_string()],
            default_output_modes: vec!["text/plain".to_string()],
            skills: vec![AgentSkill {
                id: "skill:a".to_string(),
                name: "skill".to_string(),
                description: "skill desc".to_string(),
                tags: Vec::new(),
                examples: Vec::new(),
                input_modes: vec!["text/plain".to_string()],
                output_modes: vec!["text/plain".to_string()],
                security: Vec::new(),
                tos_base_cost: None,
            }],
            supports_extended_agent_card: Some(false),
            signatures: Vec::new(),
            tos_identity: None,
        }
    }

    #[test]
    fn build_message_is_stable() -> Result<(), Box<dyn std::error::Error>> {
        let agent_account = PublicKey::from_bytes(&[9u8; 32])?;
        let mut card = sample_card();
        card.tos_identity = Some(tos_common::a2a::TosAgentIdentity {
            agent_account: agent_account.clone(),
            controller: PublicKey::from_bytes(&[8u8; 32])?,
            reputation_score_bps: None,
            identity_proof: None,
        });
        let sig = TosSignature {
            signer: TosSignerType::Owner,
            value: "0x00".to_string(),
            timestamp: 42,
            nonce: 7,
            session_key_id: None,
        };
        let msg = build_registration_message(&agent_account, "http://example.com", &card, &sig)?;
        assert!(!msg.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn json_rpc_requires_auth() {
        init_auth();
        let mut context = Context::new();
        let meta = RequestMetadata {
            method: "POST".to_string(),
            path: "/json_rpc".to_string(),
            query: String::new(),
            headers: std::collections::HashMap::new(),
            body: Vec::new(),
        };
        context.store(meta);
        let err = require_registry_auth_context(&context)
            .await
            .expect_err("expected auth error");
        assert_eq!(err.get_code(), -32098);
    }

    #[tokio::test]
    async fn http_requires_auth() {
        init_auth();
        let request = TestRequest::post()
            .uri("/agents:register")
            .to_http_request();
        let err = require_registry_auth_http(&request, &[])
            .await
            .expect_err("expected auth error");
        let message = err.to_string();
        assert!(message.contains("Unauthorized") || message.contains("missing"));
    }

    #[tokio::test]
    async fn http_rejects_invalid_version_header() {
        init_auth();
        let request = TestRequest::post()
            .uri("/agents:register")
            .insert_header((HEADER_VERSION, "2.0"))
            .to_http_request();
        let err = require_registry_auth_http(&request, &[])
            .await
            .expect_err("expected version error");
        let message = err.to_string();
        assert!(message.contains("unsupported") || message.contains("version"));
    }

    #[tokio::test]
    async fn json_rpc_rejects_invalid_version_header() {
        init_auth();
        let mut context = Context::new();
        let mut headers = std::collections::HashMap::new();
        headers.insert(HEADER_VERSION.to_string(), "2.0".to_string());
        let meta = RequestMetadata {
            method: "POST".to_string(),
            path: "/json_rpc".to_string(),
            query: String::new(),
            headers,
            body: Vec::new(),
        };
        context.store(meta);
        let err = require_registry_auth_context(&context)
            .await
            .expect_err("expected version error");
        let message = err.to_string();
        assert!(message.contains("unsupported") || message.contains("version"));
    }
}
