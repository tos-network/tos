use std::collections::{HashMap, VecDeque};
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use actix_web::{
    error::{
        ErrorBadRequest, ErrorInternalServerError, ErrorNotFound, ErrorTooManyRequests,
        ErrorUnauthorized,
    },
    web, Error as ActixError, HttpRequest, HttpResponse,
};
use log::warn;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::sync::Mutex;

use tos_common::{
    a2a::{verify_tos_signature, AgentCard, TosSignature, TosSignerType},
    a2a::{HEADER_VERSION, PROTOCOL_VERSION},
    arbitration::{expertise_domains_to_skill_tags, ArbiterAccount},
    async_handler,
    context::Context,
    crypto::{hash, Address, Hash, PublicKey},
    rpc::{
        parse_params,
        server::{RPCServerHandler, RequestMetadata},
        InternalRpcError, RPCHandler,
    },
};

use crate::{
    a2a::registry::{
        global_registry, AgentFilter, AgentHealthStatus, AgentStatus, RegisteredAgent,
        RegistryError,
    },
    core::{blockchain::Blockchain, storage::Storage},
    rpc::DaemonRpcServer,
};

const REGISTRY_SIGNATURE_DOMAIN: &[u8] = b"TOS_AGENT_REGISTRY_V2";
const DEFAULT_HEARTBEAT_INTERVAL_SECS: u32 = 30;
const DEFAULT_REGISTRY_RATE_LIMIT_WINDOW_SECS: u64 = 60;
const DEFAULT_REGISTRY_RATE_LIMIT_MAX: u32 = 10;
/// Maximum age of a signature in seconds (5 minutes).
const SIGNATURE_VALIDITY_WINDOW_SECS: u64 = 300;
/// Maximum time drift allowed for future timestamps (30 seconds).
const SIGNATURE_MAX_FUTURE_SECS: u64 = 30;
/// Maximum number of keys in the rate limiter map to prevent memory exhaustion.
const RATE_LIMITER_MAX_KEYS: usize = 10_000;
/// Maximum number of accounts in the nonce tracker to prevent memory exhaustion.
const NONCE_TRACKER_MAX_ACCOUNTS: usize = 10_000;
/// Maximum number of nonces per account to prevent memory exhaustion from nonce spam.
/// Since nonces expire after SIGNATURE_VALIDITY_WINDOW_SECS, this limits concurrent operations.
const NONCE_TRACKER_MAX_PER_ACCOUNT: usize = 100;

#[derive(Clone, Copy)]
pub struct RegistrationRateLimitConfig {
    pub window_secs: u64,
    pub max_requests: u32,
}

struct RegistrationRateLimiter {
    window: Duration,
    max_requests: u32,
    entries: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl RegistrationRateLimiter {
    fn new(config: RegistrationRateLimitConfig) -> Self {
        Self {
            window: Duration::from_secs(config.window_secs),
            max_requests: config.max_requests,
            entries: Mutex::new(HashMap::new()),
        }
    }

    async fn check(&self, key: &str) -> Result<(), AgentRegistryRpcError> {
        if self.max_requests == 0 || self.window.is_zero() {
            return Ok(());
        }
        let now = Instant::now();
        let mut entries = self.entries.lock().await;

        // Evict stale entries to prevent unbounded memory growth
        // Do this periodically when map grows large
        if entries.len() > RATE_LIMITER_MAX_KEYS / 2 {
            entries.retain(|_, bucket| {
                // Remove entries that have no recent activity
                if let Some(back) = bucket.back() {
                    now.duration_since(*back) <= self.window
                } else {
                    false
                }
            });
        }

        // If still at capacity after cleanup, reject new keys (DoS protection)
        if entries.len() >= RATE_LIMITER_MAX_KEYS && !entries.contains_key(key) {
            return Err(AgentRegistryRpcError::RateLimitExceeded {
                window_secs: self.window.as_secs(),
                max_requests: self.max_requests,
            });
        }

        let bucket = entries.entry(key.to_string()).or_default();
        while let Some(front) = bucket.front() {
            if now.duration_since(*front) > self.window {
                bucket.pop_front();
            } else {
                break;
            }
        }
        if bucket.len() >= self.max_requests as usize {
            return Err(AgentRegistryRpcError::RateLimitExceeded {
                window_secs: self.window.as_secs(),
                max_requests: self.max_requests,
            });
        }
        bucket.push_back(now);
        Ok(())
    }
}

static REGISTRY_RATE_LIMIT_CONFIG: OnceCell<RegistrationRateLimitConfig> = OnceCell::new();
static REGISTRY_RATE_LIMITER: OnceCell<RegistrationRateLimiter> = OnceCell::new();
static SIGNATURE_NONCE_TRACKER: OnceCell<SignatureNonceTracker> = OnceCell::new();

pub fn set_registration_rate_limit_config(config: RegistrationRateLimitConfig) {
    if REGISTRY_RATE_LIMIT_CONFIG.set(config).is_err() {
        if log::log_enabled!(log::Level::Warn) {
            warn!("Registration rate limit config already set");
        }
    }
    if REGISTRY_RATE_LIMITER
        .set(RegistrationRateLimiter::new(config))
        .is_err()
    {
        if log::log_enabled!(log::Level::Warn) {
            warn!("Registration rate limiter already set");
        }
    }
    if SIGNATURE_NONCE_TRACKER
        .set(SignatureNonceTracker::new())
        .is_err()
    {
        if log::log_enabled!(log::Level::Warn) {
            warn!("Signature nonce tracker already set");
        }
    }
}

/// Tracks used signature nonces per account to prevent replay attacks.
/// Nonces are stored with their timestamps and cleaned up when they expire.
struct SignatureNonceTracker {
    /// Maps account -> (nonce -> timestamp)
    entries: Mutex<HashMap<PublicKey, HashMap<u64, u64>>>,
}

impl SignatureNonceTracker {
    fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Atomically check and reserve a nonce before signature verification.
    /// If the nonce is not used, it is reserved (recorded) to prevent concurrent use.
    /// The caller should verify the signature after this succeeds.
    /// Note: If signature verification fails, the nonce remains reserved, which is safe
    /// since it just prevents the same nonce from being used again (attackers can't
    /// waste nonces without knowing the correct signature).
    async fn check_and_reserve_nonce(
        &self,
        account: &PublicKey,
        nonce: u64,
        timestamp: u64,
    ) -> Result<(), AgentRegistryRpcError> {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let expiry_threshold = current_time.saturating_sub(SIGNATURE_VALIDITY_WINDOW_SECS);

        let mut entries = self.entries.lock().await;

        // Check if nonce was already used (and not expired)
        if let Some(account_nonces) = entries.get(account) {
            if let Some(&ts) = account_nonces.get(&nonce) {
                if ts > expiry_threshold {
                    return Err(AgentRegistryRpcError::SignatureNonceReused);
                }
            }
        }

        // Nonce is available - reserve it atomically (while still holding the lock)
        // First, clean up expired nonces
        entries.retain(|_, nonces| {
            nonces.retain(|_, ts| *ts > expiry_threshold);
            !nonces.is_empty()
        });

        // If still at capacity and this is a new account, force eviction of oldest accounts
        if entries.len() >= NONCE_TRACKER_MAX_ACCOUNTS && !entries.contains_key(account) {
            let oldest_account = entries
                .iter()
                .map(|(k, nonces)| {
                    let max_ts = nonces.values().max().copied().unwrap_or(0);
                    (k.clone(), max_ts)
                })
                .min_by_key(|(_, ts)| *ts)
                .map(|(k, _)| k);

            if let Some(old_account) = oldest_account {
                entries.remove(&old_account);
            }
        }

        // Record the nonce with per-account limit enforcement
        let account_nonces = entries.entry(account.clone()).or_default();

        // If at per-account capacity, evict the oldest nonce
        if account_nonces.len() >= NONCE_TRACKER_MAX_PER_ACCOUNT {
            // Find and remove the oldest nonce (lowest timestamp)
            let oldest_nonce = account_nonces
                .iter()
                .min_by_key(|(_, ts)| *ts)
                .map(|(n, _)| *n);
            if let Some(old_nonce) = oldest_nonce {
                account_nonces.remove(&old_nonce);
            }
        }

        account_nonces.insert(nonce, timestamp);

        Ok(())
    }
}

/// Verify agent ownership by checking TOS signature against agent's owner or controller.
/// If the agent has no TOS identity, no verification is performed.
async fn verify_agent_ownership<S: Storage>(
    blockchain: &Arc<Blockchain<S>>,
    agent: &crate::a2a::registry::RegisteredAgent,
    signature: Option<&TosSignature>,
    operation: &str,
    payload_hash: &Hash,
) -> Result<(), AgentRegistryRpcError> {
    // If agent has TOS identity (owner or controller), signature is required
    let requires_signature = agent.agent_account.is_some() || agent.controller.is_some();

    if !requires_signature {
        // Agent has no TOS identity, no ownership verification needed
        return Ok(());
    }

    let signature = signature.ok_or(AgentRegistryRpcError::AgentOwnershipRequired)?;

    // Validate timestamp freshness
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let min_valid_time = current_time.saturating_sub(SIGNATURE_VALIDITY_WINDOW_SECS);
    if signature.timestamp < min_valid_time {
        return Err(AgentRegistryRpcError::SignatureExpired {
            timestamp: signature.timestamp,
            max_age_secs: SIGNATURE_VALIDITY_WINDOW_SECS,
        });
    }

    let max_valid_time = current_time.saturating_add(SIGNATURE_MAX_FUTURE_SECS);
    if signature.timestamp > max_valid_time {
        return Err(AgentRegistryRpcError::SignatureTimestampFuture {
            timestamp: signature.timestamp,
            max_future_secs: SIGNATURE_MAX_FUTURE_SECS,
        });
    }

    // Get agent_account - required for on-chain meta lookup and signature verification
    // verify_tos_signature uses agent_account to fetch meta and extract the signing key
    let agent_account = agent
        .agent_account
        .as_ref()
        .ok_or(AgentRegistryRpcError::AgentOwnershipRequired)?;

    // Fetch account meta FIRST for both signature verification and nonce key determination.
    // We need on-chain meta to get the authoritative controller key for nonce tracking.
    let (meta, session_key) = {
        let storage = blockchain.get_storage().read().await;
        let meta = storage
            .get_agent_account_meta(agent_account)
            .await
            .map_err(|e| AgentRegistryRpcError::StorageError(e.to_string()))?;
        let session_key = if signature.signer == TosSignerType::SessionKey {
            let key_id = signature
                .session_key_id
                .ok_or(AgentRegistryRpcError::MissingTosSignature)?;
            storage
                .get_session_key(agent_account, key_id)
                .await
                .map_err(|e| AgentRegistryRpcError::StorageError(e.to_string()))?
        } else {
            None
        };
        (meta, session_key)
    };

    // Determine nonce tracking key using on-chain data to prevent replay attacks.
    // IMPORTANT: We use the on-chain controller (from meta) not the registry-stored
    // controller, because the registry controller could be set to an arbitrary value
    // by the owner, bypassing nonce tracking.
    let nonce_tracking_key: PublicKey = match signature.signer {
        TosSignerType::Owner | TosSignerType::SessionKey => {
            // Owner and session key use agent_account for nonce tracking
            agent_account.clone()
        }
        TosSignerType::Controller => {
            // Use on-chain controller for nonce tracking (not registry-stored value)
            // This prevents bypass via bogus controller in registration
            meta.as_ref()
                .ok_or(AgentRegistryRpcError::AgentOwnershipRequired)?
                .controller
                .clone()
        }
    };

    // Build the message to verify (includes chain_id to prevent cross-chain replay)
    let chain_id = blockchain.get_network().chain_id();
    let mut message = Vec::new();
    message.extend_from_slice(b"TOS_AGENT_OWNERSHIP_V2");
    message.extend_from_slice(&chain_id.to_le_bytes());
    message.extend_from_slice(operation.as_bytes());
    message.extend_from_slice(agent.agent_id.as_bytes());
    message.extend_from_slice(payload_hash.as_bytes());
    message.extend_from_slice(&signature.timestamp.to_le_bytes());
    message.extend_from_slice(&signature.nonce.to_le_bytes());

    let reader = PreloadedAgentAccountReader {
        agent_account: agent_account.clone(),
        meta,
        session_key,
        topoheight: blockchain.get_topo_height(),
    };

    // Verify signature BEFORE reserving nonce. This prevents attackers from
    // burning nonces for victim accounts with invalid signatures.
    verify_tos_signature(signature, agent_account, &message, &reader)
        .map_err(|e| AgentRegistryRpcError::SignatureVerification(e.to_string()))?;

    // Reserve nonce AFTER signature verification to prevent DoS via invalid signatures.
    // The nonce check prevents replay attacks with the same valid signature.
    let nonce_tracker = SIGNATURE_NONCE_TRACKER.get_or_init(SignatureNonceTracker::new);
    nonce_tracker
        .check_and_reserve_nonce(&nonce_tracking_key, signature.nonce, signature.timestamp)
        .await?;

    Ok(())
}

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
pub struct UpdateAgentRequest {
    pub agent_id: String,
    pub agent_card: AgentCard,
    /// TOS signature for ownership verification (required when agent has TOS identity)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tos_signature: Option<TosSignature>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnregisterAgentRequest {
    pub agent_id: String,
    /// TOS signature for ownership verification (required when agent has TOS identity)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tos_signature: Option<TosSignature>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAgentRequest {
    pub agent_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAgentByAccountRequest {
    pub agent_account: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatRequest {
    pub agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<AgentHealthStatus>,
    /// TOS signature for ownership verification (required when agent has TOS identity)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tos_signature: Option<TosSignature>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatResponse {
    pub agent_id: String,
    pub last_seen: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSummary {
    pub agent_id: String,
    pub name: String,
    pub description: String,
    pub endpoint_url: String,
    pub skills: Vec<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tos_identity: Option<tos_common::a2a::TosAgentIdentity>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverAgentsResponse {
    pub agents: Vec<AgentSummary>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverCommitteeMembersRequest {
    pub committee_id: String,
    #[serde(default)]
    pub active_only: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverCommitteeMembersResponse {
    pub committee_name: String,
    pub region: String,
    pub members: Vec<CommitteeMemberInfo>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitteeMemberInfo {
    pub pubkey: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint_url: Option<String>,
    pub role: String,
    pub reputation_score: u16,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AgentPath {
    pub id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AgentListQuery {
    pub skills: Option<String>,
    pub input_modes: Option<String>,
    pub output_modes: Option<String>,
    pub require_settlement: Option<bool>,
    pub require_tos_identity: Option<bool>,
    pub limit: Option<u32>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AgentByAccountQuery {
    pub account: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CommitteeMembersQuery {
    pub active_only: Option<bool>,
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
    #[error("invalid agent account")]
    InvalidAgentAccount,
    #[error("invalid committee id")]
    InvalidCommitteeId,
    #[error("committee not found")]
    CommitteeNotFound,
    #[error("failed to serialize agent card")]
    SerializeAgentCard,
    #[error("arbiter requires TOS identity")]
    ArbiterRequiresTosIdentity,
    #[error("arbiter not registered on chain")]
    ArbiterNotRegisteredOnChain,
    #[error("arbiter not active")]
    ArbiterNotActive,
    #[error("arbiter stake too low: required {required}, found {found}")]
    ArbiterStakeTooLow { required: u64, found: u64 },
    #[error("registration rate limit exceeded: {max_requests} per {window_secs}s")]
    RateLimitExceeded { window_secs: u64, max_requests: u32 },
    #[error("signature expired: timestamp {timestamp} is older than {max_age_secs} seconds")]
    SignatureExpired { timestamp: u64, max_age_secs: u64 },
    #[error(
        "signature timestamp in future: {timestamp} is more than {max_future_secs} seconds ahead"
    )]
    SignatureTimestampFuture {
        timestamp: u64,
        max_future_secs: u64,
    },
    #[error("signature nonce already used")]
    SignatureNonceReused,
    #[error("signature verification failed: {0}")]
    SignatureVerification(String),
    #[error("agent ownership verification required")]
    AgentOwnershipRequired,
    #[error("agent ownership verification failed: signer is not agent owner or controller")]
    AgentOwnershipFailed,
    #[error("storage error: {0}")]
    StorageError(String),
    #[error(transparent)]
    Registry(#[from] RegistryError),
}

/// Register agent registry JSON-RPC methods.
pub fn register_agent_registry_methods<S: Storage>(handler: &mut RPCHandler<Arc<Blockchain<S>>>) {
    handler.register_method("register_agent", async_handler!(register_agent::<S>));
    handler.register_method("RegisterAgent", async_handler!(register_agent::<S>));
    handler.register_method("update_agent", async_handler!(update_agent::<S>));
    handler.register_method("UpdateAgent", async_handler!(update_agent::<S>));
    handler.register_method("unregister_agent", async_handler!(unregister_agent::<S>));
    handler.register_method("UnregisterAgent", async_handler!(unregister_agent::<S>));
    handler.register_method("get_agent", async_handler!(get_agent::<S>));
    handler.register_method("GetRegisteredAgent", async_handler!(get_agent::<S>));
    handler.register_method(
        "get_agent_by_account",
        async_handler!(get_agent_by_account::<S>),
    );
    handler.register_method(
        "GetAgentByAccount",
        async_handler!(get_agent_by_account::<S>),
    );
    handler.register_method("discover_agents", async_handler!(discover_agents::<S>));
    handler.register_method("DiscoverAgents", async_handler!(discover_agents::<S>));
    handler.register_method("list_agents", async_handler!(list_agents::<S>));
    handler.register_method("ListRegisteredAgents", async_handler!(list_agents::<S>));
    handler.register_method("heartbeat", async_handler!(heartbeat::<S>));
    handler.register_method("AgentHeartbeat", async_handler!(heartbeat::<S>));
    handler.register_method(
        "discover_committee_members",
        async_handler!(discover_committee_members::<S>),
    );
    handler.register_method(
        "DiscoverCommitteeMembers",
        async_handler!(discover_committee_members::<S>),
    );
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

async fn update_agent<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_registry_auth_context(context).await?;
    let request: UpdateAgentRequest = parse_params(body)?;
    let agent_id = parse_agent_id(&request.agent_id)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;

    // Verify ownership before allowing update
    let registry = global_registry();
    let existing = registry
        .get(&agent_id)
        .await
        .ok_or(AgentRegistryRpcError::Registry(
            RegistryError::AgentNotFound,
        ))?;

    // Hash the new agent card as payload for signature verification (using canonical serialization)
    let payload_hash = canonical_card_hash(&request.agent_card)?;

    verify_agent_ownership(
        blockchain,
        &existing,
        request.tos_signature.as_ref(),
        "update",
        &payload_hash,
    )
    .await?;

    // Reconcile arbiter fields if this is an arbiter agent
    // This ensures on-chain data remains authoritative even on updates
    let mut agent_card = request.agent_card;
    let is_arbiter = agent_card
        .skills
        .iter()
        .any(|skill| skill.id.starts_with("arbitration"));

    if is_arbiter {
        let tos_identity = agent_card
            .tos_identity
            .as_ref()
            .ok_or(AgentRegistryRpcError::ArbiterRequiresTosIdentity)?;
        let storage = blockchain.get_storage().read().await;
        let arbiter = storage
            .get_arbiter(&tos_identity.agent_account)
            .await
            .map_err(|e| AgentRegistryRpcError::StorageError(e.to_string()))?
            .ok_or(AgentRegistryRpcError::ArbiterNotRegisteredOnChain)?;
        if arbiter.status != tos_common::arbitration::ArbiterStatus::Active {
            return Err(AgentRegistryRpcError::ArbiterNotActive.into());
        }
        let min_stake = tos_common::config::MIN_ARBITER_STAKE;
        if arbiter.stake_amount < min_stake {
            return Err(AgentRegistryRpcError::ArbiterStakeTooLow {
                required: min_stake,
                found: arbiter.stake_amount,
            }
            .into());
        }
        reconcile_arbiter_card_fields(&mut agent_card, &arbiter);

        // Update reputation score
        if let Some(identity) = agent_card.tos_identity.as_mut() {
            identity.reputation_score_bps = Some(u32::from(arbiter.reputation_score));
        }
    }

    let updated = registry
        .update(&agent_id, agent_card)
        .await
        .map_err(AgentRegistryRpcError::from)?;
    serde_json::to_value(to_agent_summary(&updated)).map_err(InternalRpcError::SerializeResponse)
}

async fn unregister_agent<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_registry_auth_context(context).await?;
    let request: UnregisterAgentRequest = parse_params(body)?;
    let agent_id = parse_agent_id(&request.agent_id)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;

    // Verify ownership before allowing unregister
    let registry = global_registry();
    let existing = registry
        .get(&agent_id)
        .await
        .ok_or(AgentRegistryRpcError::Registry(
            RegistryError::AgentNotFound,
        ))?;

    // Use agent_id as payload hash for unregister
    verify_agent_ownership(
        blockchain,
        &existing,
        request.tos_signature.as_ref(),
        "unregister",
        &agent_id,
    )
    .await?;

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
    let response = if let Some(agent) = agent {
        let blockchain: &Arc<Blockchain<S>> = context.get()?;
        let storage = blockchain.get_storage().read().await;
        Some(enrich_agent_summary(&*storage, &agent).await)
    } else {
        None
    };
    serde_json::to_value(response).map_err(InternalRpcError::SerializeResponse)
}

async fn get_agent_by_account<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_registry_auth_context(context).await?;
    let request: GetAgentByAccountRequest = parse_params(body)?;
    let agent_account = parse_agent_account(&request.agent_account).map_err(map_error)?;

    let registry = global_registry();
    let agent = registry.get_by_account(&agent_account).await;
    let response = if let Some(agent) = agent {
        let blockchain: &Arc<Blockchain<S>> = context.get()?;
        let storage = blockchain.get_storage().read().await;
        Some(enrich_agent_summary(&*storage, &agent).await)
    } else {
        None
    };
    serde_json::to_value(response).map_err(InternalRpcError::SerializeResponse)
}

async fn discover_agents<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_registry_auth_context(context).await?;
    let filter: AgentFilter = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let registry = global_registry();
    let agents = registry
        .filter(&filter)
        .await
        .map_err(AgentRegistryRpcError::Registry)?;
    let response = DiscoverAgentsResponse {
        agents: enrich_agent_summaries(&*storage, &agents).await,
    };
    serde_json::to_value(response).map_err(InternalRpcError::SerializeResponse)
}

async fn discover_committee_members<S: Storage>(
    context: &Context,
    body: Value,
) -> Result<Value, InternalRpcError> {
    require_registry_auth_context(context).await?;
    let request: DiscoverCommitteeMembersRequest = parse_params(body)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let response = discover_committee_members_impl(Arc::clone(blockchain), request).await?;
    serde_json::to_value(response).map_err(InternalRpcError::SerializeResponse)
}

async fn list_agents<S: Storage>(
    context: &Context,
    _body: Value,
) -> Result<Value, InternalRpcError> {
    require_registry_auth_context(context).await?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;
    let storage = blockchain.get_storage().read().await;
    let registry = global_registry();
    let agents = registry.list().await;
    let response = DiscoverAgentsResponse {
        agents: enrich_agent_summaries(&*storage, &agents).await,
    };
    serde_json::to_value(response).map_err(InternalRpcError::SerializeResponse)
}

async fn heartbeat<S: Storage>(context: &Context, body: Value) -> Result<Value, InternalRpcError> {
    require_registry_auth_context(context).await?;
    let request: HeartbeatRequest = parse_params(body)?;
    let agent_id = parse_agent_id(&request.agent_id)?;
    let blockchain: &Arc<Blockchain<S>> = context.get()?;

    // Verify ownership before allowing heartbeat
    let registry = global_registry();
    let existing = registry
        .get(&agent_id)
        .await
        .ok_or(AgentRegistryRpcError::Registry(
            RegistryError::AgentNotFound,
        ))?;

    // Include status in payload hash to prevent tampering with health metrics
    let payload_hash = compute_heartbeat_payload_hash(&agent_id, request.status.as_ref())?;
    verify_agent_ownership(
        blockchain,
        &existing,
        request.tos_signature.as_ref(),
        "heartbeat",
        &payload_hash,
    )
    .await?;

    let last_seen = registry
        .heartbeat(&agent_id, request.status)
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

/// HTTP endpoint: PATCH /agents/{id}
pub async fn update_agent_http<S: Storage>(
    server: web::Data<DaemonRpcServer<S>>,
    request: HttpRequest,
    path: web::Path<AgentPath>,
    body: web::Bytes,
) -> Result<HttpResponse, ActixError> {
    require_registry_auth_http(&request, &body).await?;
    let agent_id = parse_agent_id_http(&path.id).map_err(map_http_error)?;
    let update_request: UpdateAgentRequest =
        serde_json::from_slice(&body).map_err(|e| ErrorBadRequest(e.to_string()))?;
    if update_request.agent_id != path.id {
        return Err(ErrorBadRequest("agent id mismatch"));
    }
    let blockchain = server.get_rpc_handler().get_data().clone();

    // Verify ownership before allowing update
    let registry = global_registry();
    let existing = registry.get(&agent_id).await.ok_or_else(|| {
        map_http_error(AgentRegistryRpcError::Registry(
            RegistryError::AgentNotFound,
        ))
    })?;

    // Hash the new agent card as payload for signature verification (using canonical serialization)
    let payload_hash = canonical_card_hash(&update_request.agent_card).map_err(map_http_error)?;

    verify_agent_ownership(
        &blockchain,
        &existing,
        update_request.tos_signature.as_ref(),
        "update",
        &payload_hash,
    )
    .await
    .map_err(map_http_error)?;

    // Reconcile arbiter fields if this is an arbiter agent
    // This ensures on-chain data remains authoritative even on updates
    let mut agent_card = update_request.agent_card;
    let is_arbiter = agent_card
        .skills
        .iter()
        .any(|skill| skill.id.starts_with("arbitration"));

    if is_arbiter {
        let tos_identity = agent_card
            .tos_identity
            .as_ref()
            .ok_or_else(|| map_http_error(AgentRegistryRpcError::ArbiterRequiresTosIdentity))?;
        let storage = blockchain.get_storage().read().await;
        let arbiter = storage
            .get_arbiter(&tos_identity.agent_account)
            .await
            .map_err(|e| map_http_error(AgentRegistryRpcError::StorageError(e.to_string())))?
            .ok_or_else(|| map_http_error(AgentRegistryRpcError::ArbiterNotRegisteredOnChain))?;
        if arbiter.status != tos_common::arbitration::ArbiterStatus::Active {
            return Err(map_http_error(AgentRegistryRpcError::ArbiterNotActive));
        }
        let min_stake = tos_common::config::MIN_ARBITER_STAKE;
        if arbiter.stake_amount < min_stake {
            return Err(map_http_error(AgentRegistryRpcError::ArbiterStakeTooLow {
                required: min_stake,
                found: arbiter.stake_amount,
            }));
        }
        reconcile_arbiter_card_fields(&mut agent_card, &arbiter);

        // Update reputation score
        if let Some(identity) = agent_card.tos_identity.as_mut() {
            identity.reputation_score_bps = Some(u32::from(arbiter.reputation_score));
        }
    }

    let updated = registry
        .update(&agent_id, agent_card)
        .await
        .map_err(AgentRegistryRpcError::from)
        .map_err(map_http_error)?;
    let storage = server
        .get_rpc_handler()
        .get_data()
        .get_storage()
        .read()
        .await;
    Ok(HttpResponse::Ok().json(enrich_agent_summary(&*storage, &updated).await))
}

/// HTTP endpoint: GET /agents/{id}
pub async fn get_agent_http<S: Storage>(
    server: web::Data<DaemonRpcServer<S>>,
    request: HttpRequest,
    path: web::Path<AgentPath>,
) -> Result<HttpResponse, ActixError> {
    require_registry_auth_http(&request, &[]).await?;
    let agent_id = parse_agent_id_http(&path.id).map_err(map_http_error)?;
    let registry = global_registry();
    let agent = registry.get(&agent_id).await;
    match agent {
        Some(agent) => {
            let storage = server
                .get_rpc_handler()
                .get_data()
                .get_storage()
                .read()
                .await;
            Ok(HttpResponse::Ok().json(enrich_agent_summary(&*storage, &agent).await))
        }
        None => Err(ErrorNotFound("Agent not found")),
    }
}

/// HTTP endpoint: GET /agents:by-account?account=tos1...
pub async fn get_agent_by_account_http<S: Storage>(
    server: web::Data<DaemonRpcServer<S>>,
    request: HttpRequest,
    query: web::Query<AgentByAccountQuery>,
) -> Result<HttpResponse, ActixError> {
    require_registry_auth_http(&request, &[]).await?;
    let agent_account = parse_agent_account(&query.account).map_err(map_http_error)?;
    let registry = global_registry();
    let agent = registry.get_by_account(&agent_account).await;
    match agent {
        Some(agent) => {
            let storage = server
                .get_rpc_handler()
                .get_data()
                .get_storage()
                .read()
                .await;
            Ok(HttpResponse::Ok().json(enrich_agent_summary(&*storage, &agent).await))
        }
        None => Err(ErrorNotFound("Agent not found")),
    }
}

/// HTTP endpoint: GET /agents
pub async fn list_agents_http<S: Storage>(
    server: web::Data<DaemonRpcServer<S>>,
    request: HttpRequest,
) -> Result<HttpResponse, ActixError> {
    require_registry_auth_http(&request, &[]).await?;
    let registry = global_registry();
    let agents = registry.list().await;
    let storage = server
        .get_rpc_handler()
        .get_data()
        .get_storage()
        .read()
        .await;
    let response = DiscoverAgentsResponse {
        agents: enrich_agent_summaries(&*storage, &agents).await,
    };
    Ok(HttpResponse::Ok().json(response))
}

/// HTTP endpoint: DELETE /agents/{id}
/// Body can optionally contain TOS signature for ownership verification.
pub async fn unregister_agent_http<S: Storage>(
    server: web::Data<DaemonRpcServer<S>>,
    request: HttpRequest,
    path: web::Path<AgentPath>,
    body: web::Bytes,
) -> Result<HttpResponse, ActixError> {
    require_registry_auth_http(&request, &body).await?;
    let agent_id = parse_agent_id_http(&path.id).map_err(map_http_error)?;
    let blockchain = server.get_rpc_handler().get_data().clone();

    // Parse optional unregister request with signature
    let tos_signature = if !body.is_empty() {
        let unregister_request: UnregisterAgentRequest =
            serde_json::from_slice(&body).map_err(|e| ErrorBadRequest(e.to_string()))?;
        if unregister_request.agent_id != path.id {
            return Err(ErrorBadRequest("agent id mismatch"));
        }
        unregister_request.tos_signature
    } else {
        None
    };

    // Verify ownership before allowing unregister
    let registry = global_registry();
    let existing = registry.get(&agent_id).await.ok_or_else(|| {
        map_http_error(AgentRegistryRpcError::Registry(
            RegistryError::AgentNotFound,
        ))
    })?;

    verify_agent_ownership(
        &blockchain,
        &existing,
        tos_signature.as_ref(),
        "unregister",
        &agent_id,
    )
    .await
    .map_err(map_http_error)?;

    registry
        .unregister(&agent_id)
        .await
        .map_err(AgentRegistryRpcError::from)
        .map_err(map_http_error)?;
    Ok(HttpResponse::NoContent().finish())
}

/// HTTP endpoint: POST /agents:discover
pub async fn discover_agents_http<S: Storage>(
    server: web::Data<DaemonRpcServer<S>>,
    request: HttpRequest,
    body: web::Bytes,
) -> Result<HttpResponse, ActixError> {
    require_registry_auth_http(&request, &body).await?;
    let filter: AgentFilter =
        serde_json::from_slice(&body).map_err(|e| ErrorBadRequest(e.to_string()))?;
    let registry = global_registry();
    let agents = registry
        .filter(&filter)
        .await
        .map_err(|e| ErrorBadRequest(e.to_string()))?;
    let storage = server
        .get_rpc_handler()
        .get_data()
        .get_storage()
        .read()
        .await;
    let response = DiscoverAgentsResponse {
        agents: enrich_agent_summaries(&*storage, &agents).await,
    };
    Ok(HttpResponse::Ok().json(response))
}

/// HTTP endpoint: POST /committees:members
pub async fn discover_committee_members_http<S: Storage>(
    server: web::Data<DaemonRpcServer<S>>,
    request: HttpRequest,
    body: web::Bytes,
) -> Result<HttpResponse, ActixError> {
    require_registry_auth_http(&request, &body).await?;
    let request: DiscoverCommitteeMembersRequest =
        serde_json::from_slice(&body).map_err(|e| ErrorBadRequest(e.to_string()))?;
    let blockchain = server.get_rpc_handler().get_data().clone();
    let response = discover_committee_members_impl(blockchain, request)
        .await
        .map_err(map_http_error)?;
    Ok(HttpResponse::Ok().json(response))
}

/// HTTP endpoint: GET /agents:discover
pub async fn discover_agents_http_get<S: Storage>(
    server: web::Data<DaemonRpcServer<S>>,
    request: HttpRequest,
    query: web::Query<AgentListQuery>,
) -> Result<HttpResponse, ActixError> {
    require_registry_auth_http(&request, &[]).await?;
    let filter = filter_from_query(&query);
    let registry = global_registry();
    let agents = registry
        .filter(&filter)
        .await
        .map_err(|e| ErrorBadRequest(e.to_string()))?;
    let storage = server
        .get_rpc_handler()
        .get_data()
        .get_storage()
        .read()
        .await;
    let response = DiscoverAgentsResponse {
        agents: enrich_agent_summaries(&*storage, &agents).await,
    };
    Ok(HttpResponse::Ok().json(response))
}

/// HTTP endpoint: GET /committees/{id}:members
pub async fn discover_committee_members_http_get<S: Storage>(
    server: web::Data<DaemonRpcServer<S>>,
    request: HttpRequest,
    path: web::Path<AgentPath>,
    query: web::Query<CommitteeMembersQuery>,
) -> Result<HttpResponse, ActixError> {
    require_registry_auth_http(&request, &[]).await?;
    let request = DiscoverCommitteeMembersRequest {
        committee_id: path.id.clone(),
        active_only: query.active_only.unwrap_or(true),
    };
    let blockchain = server.get_rpc_handler().get_data().clone();
    let response = discover_committee_members_impl(blockchain, request)
        .await
        .map_err(map_http_error)?;
    Ok(HttpResponse::Ok().json(response))
}

/// HTTP endpoint: POST /agents/{id}:heartbeat
pub async fn heartbeat_http<S: Storage>(
    server: web::Data<DaemonRpcServer<S>>,
    request: HttpRequest,
    path: web::Path<AgentPath>,
    body: web::Bytes,
) -> Result<HttpResponse, ActixError> {
    require_registry_auth_http(&request, &body).await?;
    let agent_id = parse_agent_id_http(&path.id).map_err(map_http_error)?;
    let blockchain = server.get_rpc_handler().get_data().clone();

    let (status, tos_signature) = if !body.is_empty() {
        let payload: HeartbeatRequest =
            serde_json::from_slice(&body).map_err(|e| ErrorBadRequest(e.to_string()))?;
        if payload.agent_id != path.id {
            return Err(ErrorBadRequest("agent id mismatch"));
        }
        (payload.status, payload.tos_signature)
    } else {
        (None, None)
    };

    // Verify ownership before allowing heartbeat
    let registry = global_registry();
    let existing = registry.get(&agent_id).await.ok_or_else(|| {
        map_http_error(AgentRegistryRpcError::Registry(
            RegistryError::AgentNotFound,
        ))
    })?;

    // Include status in payload hash to prevent tampering with health metrics
    let payload_hash =
        compute_heartbeat_payload_hash(&agent_id, status.as_ref()).map_err(map_http_error)?;
    verify_agent_ownership(
        &blockchain,
        &existing,
        tos_signature.as_ref(),
        "heartbeat",
        &payload_hash,
    )
    .await
    .map_err(map_http_error)?;

    let last_seen = registry
        .heartbeat(&agent_id, status)
        .await
        .map_err(AgentRegistryRpcError::from)
        .map_err(map_http_error)?;
    let response = HeartbeatResponse {
        agent_id: agent_id.to_hex(),
        last_seen,
    };
    Ok(HttpResponse::Ok().json(response))
}

fn parse_agent_id(value: &str) -> Result<Hash, InternalRpcError> {
    Hash::from_str(value).map_err(|_| map_error(AgentRegistryRpcError::InvalidAgentId))
}

fn parse_agent_id_http(value: &str) -> Result<Hash, AgentRegistryRpcError> {
    Hash::from_str(value).map_err(|_| AgentRegistryRpcError::InvalidAgentId)
}

fn parse_agent_account(value: &str) -> Result<PublicKey, AgentRegistryRpcError> {
    Address::from_string(value)
        .map(|address| address.to_public_key())
        .map_err(|_| AgentRegistryRpcError::InvalidAgentAccount)
}

fn parse_committee_id(value: &str) -> Result<Hash, AgentRegistryRpcError> {
    let value = value.strip_prefix("0x").unwrap_or(value);
    Hash::from_str(value).map_err(|_| AgentRegistryRpcError::InvalidCommitteeId)
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

fn registry_rate_limit_config() -> RegistrationRateLimitConfig {
    *REGISTRY_RATE_LIMIT_CONFIG.get_or_init(|| RegistrationRateLimitConfig {
        window_secs: DEFAULT_REGISTRY_RATE_LIMIT_WINDOW_SECS,
        max_requests: DEFAULT_REGISTRY_RATE_LIMIT_MAX,
    })
}

/// Apply rate limit based on endpoint URL (for agents without TOS identity).
/// Safe to call before signature verification since there's no account to spoof.
async fn enforce_endpoint_rate_limit(endpoint_url: &str) -> Result<(), AgentRegistryRpcError> {
    let config = registry_rate_limit_config();
    if config.window_secs == 0 || config.max_requests == 0 {
        return Ok(());
    }
    let limiter = REGISTRY_RATE_LIMITER.get_or_init(|| RegistrationRateLimiter::new(config));
    let key = format!("endpoint:{}", endpoint_url);
    limiter.check(&key).await
}

/// Apply rate limit based on account (for agents with TOS identity).
/// MUST be called AFTER signature verification to prevent attackers from
/// burning rate limits with fake signatures.
async fn enforce_account_rate_limit(account: &PublicKey) -> Result<(), AgentRegistryRpcError> {
    let config = registry_rate_limit_config();
    if config.window_secs == 0 || config.max_requests == 0 {
        return Ok(());
    }
    let limiter = REGISTRY_RATE_LIMITER.get_or_init(|| RegistrationRateLimiter::new(config));
    let key = format!("account:{}", hex::encode(account.as_bytes()));
    limiter.check(&key).await
}

async fn register_agent_impl<S: Storage>(
    blockchain: Arc<Blockchain<S>>,
    request: RegisterAgentRequest,
) -> Result<RegisterAgentResponse, AgentRegistryRpcError> {
    let mut request = request;

    let is_arbiter = request
        .agent_card
        .skills
        .iter()
        .any(|skill| skill.id.starts_with("arbitration"));

    if is_arbiter && request.agent_card.tos_identity.is_none() {
        return Err(AgentRegistryRpcError::ArbiterRequiresTosIdentity);
    }

    // For agents without TOS identity, apply endpoint-based rate limit early
    // (safe since there's no account to spoof)
    if request.agent_card.tos_identity.is_none() {
        enforce_endpoint_rate_limit(&request.endpoint_url).await?;
    }

    // Signature verification happens BEFORE arbiter reconciliation.
    // The signature proves the owner authorized this registration with the submitted card.
    // Arbiter reconciliation then enforces on-chain data (expertise, fees) which is
    // authoritative and server-controlled. The agent_id is computed from the final
    // reconciled card for consistency.
    //
    // Security note: The signature covers the submitted card, not the reconciled card.
    // This is acceptable because:
    // 1. The signature proves ownership/authorization for the registration
    // 2. Reconciliation only adds/updates fields from on-chain authoritative data
    // 3. An attacker cannot forge on-chain arbiter data
    if let Some(signature) = request.tos_signature.as_ref() {
        let tos_identity = request
            .agent_card
            .tos_identity
            .as_ref()
            .ok_or(AgentRegistryRpcError::MissingTosIdentity)?;

        // Validate signature timestamp to prevent replay attacks
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Reject signatures that are too old
        let min_valid_time = current_time.saturating_sub(SIGNATURE_VALIDITY_WINDOW_SECS);
        if signature.timestamp < min_valid_time {
            return Err(AgentRegistryRpcError::SignatureExpired {
                timestamp: signature.timestamp,
                max_age_secs: SIGNATURE_VALIDITY_WINDOW_SECS,
            });
        }

        // Reject signatures with future timestamps (with small tolerance for clock drift)
        let max_valid_time = current_time.saturating_add(SIGNATURE_MAX_FUTURE_SECS);
        if signature.timestamp > max_valid_time {
            return Err(AgentRegistryRpcError::SignatureTimestampFuture {
                timestamp: signature.timestamp,
                max_future_secs: SIGNATURE_MAX_FUTURE_SECS,
            });
        }

        let chain_id = blockchain.get_network().chain_id();
        // Build message using the final (reconciled) card
        let message = build_registration_message(
            chain_id,
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

        // Verify signature BEFORE reserving nonce. This prevents attackers from
        // burning nonces for victim accounts with invalid signatures.
        verify_tos_signature(signature, &tos_identity.agent_account, &message, &reader)
            .map_err(|e| AgentRegistryRpcError::SignatureVerification(e.to_string()))?;

        // Reserve nonce AFTER signature verification to prevent DoS via invalid signatures.
        // The nonce check prevents replay attacks with the same valid signature.
        let nonce_tracker = SIGNATURE_NONCE_TRACKER.get_or_init(SignatureNonceTracker::new);
        nonce_tracker
            .check_and_reserve_nonce(
                &tos_identity.agent_account,
                signature.nonce,
                signature.timestamp,
            )
            .await?;

        // Apply account-based rate limit AFTER signature verification
        // This prevents attackers from burning rate limits with fake signatures
        enforce_account_rate_limit(&tos_identity.agent_account).await?;
    } else if request.agent_card.tos_identity.is_some() {
        return Err(AgentRegistryRpcError::MissingTosSignature);
    } else if is_arbiter {
        return Err(AgentRegistryRpcError::MissingTosSignature);
    }

    // Arbiter validation and card reconciliation happens AFTER signature verification.
    // This ensures on-chain data (expertise domains, fees, etc.) is authoritative.
    let mut arbiter_reputation: Option<u16> = None;
    if is_arbiter {
        let tos_identity = request
            .agent_card
            .tos_identity
            .as_ref()
            .ok_or(AgentRegistryRpcError::MissingTosIdentity)?;
        let storage = blockchain.get_storage().read().await;
        let arbiter = storage
            .get_arbiter(&tos_identity.agent_account)
            .await
            .map_err(|e| AgentRegistryRpcError::StorageError(e.to_string()))?
            .ok_or(AgentRegistryRpcError::ArbiterNotRegisteredOnChain)?;
        if arbiter.status != tos_common::arbitration::ArbiterStatus::Active {
            return Err(AgentRegistryRpcError::ArbiterNotActive);
        }
        let min_stake = tos_common::config::MIN_ARBITER_STAKE;
        if arbiter.stake_amount < min_stake {
            return Err(AgentRegistryRpcError::ArbiterStakeTooLow {
                required: min_stake,
                found: arbiter.stake_amount,
            });
        }
        reconcile_arbiter_card_fields(&mut request.agent_card, &arbiter);
        arbiter_reputation = Some(arbiter.reputation_score);
    }

    // Set reputation score if available
    if let (Some(rep), Some(identity)) =
        (arbiter_reputation, request.agent_card.tos_identity.as_mut())
    {
        identity.reputation_score_bps = Some(u32::from(rep));
    }

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

async fn discover_committee_members_impl<S: Storage>(
    blockchain: Arc<Blockchain<S>>,
    request: DiscoverCommitteeMembersRequest,
) -> Result<DiscoverCommitteeMembersResponse, AgentRegistryRpcError> {
    let storage = blockchain.get_storage().read().await;
    let committee_id = if request.committee_id.eq_ignore_ascii_case("global") {
        storage
            .get_global_committee_id()
            .await
            .map_err(|e| AgentRegistryRpcError::StorageError(e.to_string()))?
            .ok_or(AgentRegistryRpcError::CommitteeNotFound)?
    } else {
        parse_committee_id(&request.committee_id)?
    };
    let committee = storage
        .get_committee(&committee_id)
        .await
        .map_err(|e| AgentRegistryRpcError::StorageError(e.to_string()))?
        .ok_or(AgentRegistryRpcError::CommitteeNotFound)?;

    let registry = global_registry();
    let mut members = Vec::new();
    for member in &committee.members {
        if request.active_only && member.status != tos_common::kyc::MemberStatus::Active {
            continue;
        }
        let (agent_id, endpoint_url) = match registry.get_by_account(&member.public_key).await {
            Some(agent) if agent.status == AgentStatus::Active => (
                Some(agent.agent_id.to_hex()),
                Some(agent.endpoint_url.clone()),
            ),
            _ => (None, None),
        };
        let key = hex::encode(member.public_key.as_bytes());
        let reputation_score = storage
            .get_arbiter(&member.public_key)
            .await
            .map_err(|e| AgentRegistryRpcError::StorageError(e.to_string()))?
            .map(|arbiter| arbiter.reputation_score)
            .unwrap_or(0);
        members.push(CommitteeMemberInfo {
            pubkey: format!("0x{key}"),
            agent_id,
            endpoint_url,
            role: member.role.as_str().to_string(),
            reputation_score,
        });
    }

    Ok(DiscoverCommitteeMembersResponse {
        committee_name: committee.name,
        region: committee.region.as_str().to_string(),
        members,
    })
}

fn build_registration_message(
    chain_id: u64,
    agent_account: &PublicKey,
    endpoint_url: &str,
    agent_card: &AgentCard,
    signature: &TosSignature,
) -> Result<Vec<u8>, AgentRegistryRpcError> {
    // Use canonical serialization for deterministic card hash
    let card_hash = canonical_card_hash(agent_card)?;
    let mut message = Vec::with_capacity(
        REGISTRY_SIGNATURE_DOMAIN.len()
            + 8  // chain_id
            + agent_account.as_bytes().len()
            + endpoint_url.len()
            + card_hash.as_bytes().len()
            + 16,
    );
    message.extend_from_slice(REGISTRY_SIGNATURE_DOMAIN);
    message.extend_from_slice(&chain_id.to_le_bytes());
    message.extend_from_slice(agent_account.as_bytes());
    message.extend_from_slice(endpoint_url.as_bytes());
    message.extend_from_slice(card_hash.as_bytes());
    message.extend_from_slice(&signature.timestamp.to_le_bytes());
    message.extend_from_slice(&signature.nonce.to_le_bytes());
    Ok(message)
}

/// Compute a canonical hash of an AgentCard for signature verification.
/// Uses sorted JSON keys to ensure deterministic hashing across processes.
fn canonical_card_hash(card: &AgentCard) -> Result<Hash, AgentRegistryRpcError> {
    // Convert to Value and canonicalize
    let mut value =
        serde_json::to_value(card).map_err(|_| AgentRegistryRpcError::SerializeAgentCard)?;
    canonicalize_json_value(&mut value);
    let card_bytes =
        serde_json::to_vec(&value).map_err(|_| AgentRegistryRpcError::SerializeAgentCard)?;
    Ok(hash(&card_bytes))
}

/// Recursively sort all object keys in a JSON value for deterministic serialization.
fn canonicalize_json_value(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<_> = std::mem::take(map).into_iter().collect();
            entries.sort_by(|(a, _), (b, _)| a.cmp(b));
            for (k, mut v) in entries {
                canonicalize_json_value(&mut v);
                map.insert(k, v);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                canonicalize_json_value(item);
            }
        }
        _ => {}
    }
}

/// Compute a hash of the heartbeat payload including status.
/// This prevents attackers from replaying a valid signature with modified status.
fn compute_heartbeat_payload_hash(
    agent_id: &Hash,
    status: Option<&AgentHealthStatus>,
) -> Result<Hash, AgentRegistryRpcError> {
    let mut data = Vec::with_capacity(64);
    data.extend_from_slice(agent_id.as_bytes());
    if let Some(health) = status {
        // Include status fields in a deterministic order
        data.extend_from_slice(&health.active_tasks.to_le_bytes());
        data.extend_from_slice(&health.queue_depth.to_le_bytes());
        data.extend_from_slice(&health.avg_latency_ms.to_le_bytes());
    }
    Ok(hash(&data))
}

fn map_error(err: AgentRegistryRpcError) -> InternalRpcError {
    match err {
        AgentRegistryRpcError::MissingTosSignature
        | AgentRegistryRpcError::MissingTosIdentity
        | AgentRegistryRpcError::InvalidAgentId
        | AgentRegistryRpcError::InvalidAgentAccount
        | AgentRegistryRpcError::InvalidCommitteeId
        | AgentRegistryRpcError::CommitteeNotFound
        | AgentRegistryRpcError::InvalidVersion(_)
        | AgentRegistryRpcError::SerializeAgentCard
        | AgentRegistryRpcError::ArbiterRequiresTosIdentity
        | AgentRegistryRpcError::ArbiterNotRegisteredOnChain
        | AgentRegistryRpcError::ArbiterNotActive
        | AgentRegistryRpcError::ArbiterStakeTooLow { .. } => {
            InternalRpcError::Custom(-32602, err.to_string())
        }
        AgentRegistryRpcError::RateLimitExceeded { .. } => {
            InternalRpcError::Custom(-32083, err.to_string())
        }
        AgentRegistryRpcError::SignatureExpired { .. }
        | AgentRegistryRpcError::SignatureTimestampFuture { .. }
        | AgentRegistryRpcError::SignatureNonceReused => {
            InternalRpcError::Custom(-32084, err.to_string())
        }
        AgentRegistryRpcError::SignatureVerification(message) => {
            InternalRpcError::Custom(-32080, message)
        }
        AgentRegistryRpcError::AgentOwnershipRequired
        | AgentRegistryRpcError::AgentOwnershipFailed => {
            InternalRpcError::Custom(-32085, err.to_string())
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
        | AgentRegistryRpcError::InvalidAgentAccount
        | AgentRegistryRpcError::InvalidCommitteeId
        | AgentRegistryRpcError::InvalidVersion(_)
        | AgentRegistryRpcError::SerializeAgentCard
        | AgentRegistryRpcError::ArbiterRequiresTosIdentity
        | AgentRegistryRpcError::ArbiterNotRegisteredOnChain
        | AgentRegistryRpcError::ArbiterNotActive
        | AgentRegistryRpcError::ArbiterStakeTooLow { .. } => ErrorBadRequest(err.to_string()),
        AgentRegistryRpcError::CommitteeNotFound => ErrorNotFound(err.to_string()),
        AgentRegistryRpcError::RateLimitExceeded { .. } => ErrorTooManyRequests(err.to_string()),
        AgentRegistryRpcError::SignatureExpired { .. }
        | AgentRegistryRpcError::SignatureTimestampFuture { .. }
        | AgentRegistryRpcError::SignatureNonceReused
        | AgentRegistryRpcError::SignatureVerification(_)
        | AgentRegistryRpcError::AgentOwnershipRequired
        | AgentRegistryRpcError::AgentOwnershipFailed => ErrorUnauthorized(err.to_string()),
        AgentRegistryRpcError::StorageError(message) => ErrorInternalServerError(message),
        AgentRegistryRpcError::Registry(registry_err) => match registry_err {
            RegistryError::AgentNotFound => ErrorNotFound(registry_err.to_string()),
            RegistryError::AgentAlreadyRegistered
            | RegistryError::AgentAccountAlreadyRegistered
            | RegistryError::InvalidEndpointUrl
            | RegistryError::EndpointUrlBlocked(_)
            | RegistryError::FilterInputTooLarge
            | RegistryError::InvalidAgentCard(_)
            | RegistryError::CannotRemoveTosIdentity
            | RegistryError::CannotAddTosIdentity
            | RegistryError::CannotChangeAgentAccount => ErrorBadRequest(registry_err.to_string()),
            RegistryError::SerializeAgentCard
            | RegistryError::TimestampOverflow
            | RegistryError::Storage(_)
            | RegistryError::SnapshotAlreadyActive
            | RegistryError::SnapshotNotActive => {
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

fn to_agent_summary(agent: &RegisteredAgent) -> AgentSummary {
    AgentSummary {
        agent_id: agent.agent_id.to_hex(),
        name: agent.agent_card.name.clone(),
        description: agent.agent_card.description.clone(),
        endpoint_url: agent.endpoint_url.clone(),
        skills: agent
            .agent_card
            .skills
            .iter()
            .map(|skill| skill.id.clone())
            .collect(),
        status: match agent.status {
            AgentStatus::Active => "active",
            AgentStatus::Inactive => "inactive",
            AgentStatus::Suspended => "suspended",
            AgentStatus::Unregistered => "unregistered",
        }
        .to_string(),
        tos_identity: agent.agent_card.tos_identity.clone(),
    }
}

async fn enrich_agent_summary<S: Storage>(storage: &S, agent: &RegisteredAgent) -> AgentSummary {
    let mut summary = to_agent_summary(agent);
    if let Some(mut identity) = summary.tos_identity.clone() {
        if let Ok(Some(arbiter)) = storage.get_arbiter(&identity.agent_account).await {
            identity.reputation_score_bps = Some(u32::from(arbiter.reputation_score));
            summary.tos_identity = Some(identity);
        }
    }
    summary
}

async fn enrich_agent_summaries<S: Storage>(
    storage: &S,
    agents: &[RegisteredAgent],
) -> Vec<AgentSummary> {
    let mut out = Vec::with_capacity(agents.len());
    for agent in agents {
        out.push(enrich_agent_summary(storage, agent).await);
    }
    out
}

fn arbitration_expertise_domains(arbiter: &ArbiterAccount) -> Vec<String> {
    arbiter
        .expertise
        .iter()
        .map(|domain| domain.as_str().to_string())
        .collect()
}

fn reconcile_arbiter_card_fields(card: &mut AgentCard, arbiter: &ArbiterAccount) {
    let tags = expertise_domains_to_skill_tags(&arbiter.expertise);
    let existing: std::collections::HashSet<String> =
        card.skills.iter().map(|skill| skill.id.clone()).collect();
    for tag in tags {
        if !existing.contains(tag) {
            card.skills.push(tos_common::a2a::AgentSkill {
                id: tag.to_string(),
                name: tag.to_string(),
                description: "Arbitration expertise".to_string(),
                tags: Vec::new(),
                examples: Vec::new(),
                input_modes: Vec::new(),
                output_modes: Vec::new(),
                security: Vec::new(),
                tos_base_cost: None,
            });
        }
    }

    if let Some(extension) = card.arbitration.as_mut() {
        extension.expertise_domains = arbitration_expertise_domains(arbiter);
        extension.fee_basis_points = arbiter.fee_basis_points;
        extension.min_escrow_value = arbiter.min_escrow_value;
        extension.max_escrow_value = arbiter.max_escrow_value;
    }
}

fn filter_from_query(query: &AgentListQuery) -> AgentFilter {
    AgentFilter {
        skills: query.skills.as_ref().map(|value| split_csv(value)),
        input_modes: query.input_modes.as_ref().map(|value| split_csv(value)),
        output_modes: query.output_modes.as_ref().map(|value| split_csv(value)),
        require_settlement: query.require_settlement,
        require_tos_identity: query.require_tos_identity,
        limit: query.limit,
    }
}

fn split_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|entry| entry.trim())
        .filter(|entry| !entry.is_empty())
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::test::TestRequest;
    use std::collections::HashSet;
    use std::sync::Once;
    use tos_common::a2a::{AgentCapabilities, AgentInterface, AgentSkill, TosSignerType};
    use tos_common::crypto::AddressType;
    use tos_common::serializer::Serializer;

    static AUTH_INIT: Once = Once::new();

    fn init_auth() {
        AUTH_INIT.call_once(|| {
            crate::a2a::auth::set_auth_config(
                crate::a2a::auth::A2AAuthConfig {
                    api_keys: HashSet::new(),
                    oauth_issuer: None,
                    oauth_jwks_url: None,
                    oauth_audience: None,
                    tos_skew_secs: 0,
                    tos_nonce_ttl_secs: 0,
                },
                None,
            );
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
            arbitration: None,
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
        let chain_id = 3; // devnet
        let msg = build_registration_message(
            chain_id,
            &agent_account,
            "http://example.com",
            &card,
            &sig,
        )?;
        assert!(!msg.is_empty());
        Ok(())
    }

    #[test]
    fn parse_agent_account_accepts_address() -> Result<(), Box<dyn std::error::Error>> {
        let pubkey = PublicKey::from_bytes(&[7u8; 32])?;
        let address = Address::new(true, AddressType::Normal, pubkey.clone())
            .as_string()
            .expect("address");
        let parsed = parse_agent_account(&address)?;
        assert_eq!(parsed, pubkey);
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
