use std::collections::{HashMap, HashSet};
use std::fs;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use once_cell::sync::Lazy;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};
use url::Url;

use tos_common::{
    a2a::AgentCard,
    a2a::{MAX_EXTENSIONS, MAX_INTERFACES, MAX_SECURITY_SCHEMES, MAX_SIGNATURES, MAX_SKILLS},
    crypto::{hash, Hash, PublicKey},
    time::get_current_time_in_seconds,
};

mod cache;
pub mod router;
mod snapshot;
mod store;

use snapshot::SnapshotGuard;
use store::RegistryStore;

const DEFAULT_HEALTH_CHECK_INTERVAL_SECS: u64 = 300;
const DEFAULT_HEARTBEAT_TIMEOUT_SECS: u64 = 120;
const DEFAULT_INACTIVE_FAILURES: u32 = 3;

// Security limits
const MAX_ENDPOINT_URL_LENGTH: usize = 2048;
const MAX_FILTER_SKILLS: usize = 32;
const MAX_FILTER_INPUT_MODES: usize = 16;
const MAX_FILTER_OUTPUT_MODES: usize = 16;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentStatus {
    Active,
    Inactive,
    Suspended,
    Unregistered,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentHealthStatus {
    pub active_tasks: u32,
    pub queue_depth: u32,
    pub avg_latency_ms: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredAgent {
    pub agent_id: Hash,
    pub agent_card: AgentCard,
    pub endpoint_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_account: Option<PublicKey>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub controller: Option<PublicKey>,
    pub registered_at: i64,
    pub last_heartbeat: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_health: Option<AgentHealthStatus>,
    pub status: AgentStatus,
    pub health_failures: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentFilter {
    #[serde(
        default,
        deserialize_with = "deserialize_opt_string_vec",
        skip_serializing_if = "Option::is_none"
    )]
    pub skills: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "deserialize_opt_string_vec",
        skip_serializing_if = "Option::is_none"
    )]
    pub input_modes: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "deserialize_opt_string_vec",
        skip_serializing_if = "Option::is_none"
    )]
    pub output_modes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_settlement: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_tos_identity: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("agent already registered")]
    AgentAlreadyRegistered,
    #[error("agent not found")]
    AgentNotFound,
    #[error("invalid endpoint url")]
    InvalidEndpointUrl,
    #[error("endpoint url blocked: {0}")]
    EndpointUrlBlocked(String),
    #[error("filter input exceeds limit")]
    FilterInputTooLarge,
    #[error("invalid agent card: {0}")]
    InvalidAgentCard(String),
    #[error("failed to serialize agent card")]
    SerializeAgentCard,
    #[error("timestamp overflow")]
    TimestampOverflow,
    #[error("storage error: {0}")]
    Storage(String),
    #[error("cannot remove TOS identity once set")]
    CannotRemoveTosIdentity,
    #[error("cannot add TOS identity to anonymous agent")]
    CannotAddTosIdentity,
    #[error("agent account already registered")]
    AgentAccountAlreadyRegistered,
    #[error("cannot change agent account once set")]
    CannotChangeAgentAccount,
    #[error("snapshot already active")]
    SnapshotAlreadyActive,
    #[error("snapshot not active")]
    SnapshotNotActive,
}

pub struct AgentRegistry {
    store: RwLock<RegistryStore>,
}

impl AgentRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            store: RwLock::new(RegistryStore::in_memory()),
        }
    }

    /// Register a new agent and return its registry record.
    pub async fn register(
        &self,
        agent_card: AgentCard,
        endpoint_url: String,
    ) -> Result<RegisteredAgent, RegistryError> {
        let mut store = self.store.write().await;
        let mut guard = SnapshotGuard::new(&mut store)?;
        let registered = Self::do_register(guard.store_mut(), agent_card, endpoint_url)?;
        guard.commit()?;
        Ok(registered)
    }

    fn do_register(
        store: &mut RegistryStore,
        agent_card: AgentCard,
        endpoint_url: String,
    ) -> Result<RegisteredAgent, RegistryError> {
        // Validate endpoint URL (SSRF protection)
        validate_endpoint_url(&endpoint_url)?;
        validate_agent_card(&agent_card)?;

        let agent_id = compute_agent_id(&agent_card, &endpoint_url)?;
        let now = current_timestamp_i64()?;

        let agent_account = agent_card
            .tos_identity
            .as_ref()
            .map(|id| id.agent_account.clone());
        let controller = agent_card
            .tos_identity
            .as_ref()
            .map(|id| id.controller.clone());

        if store.cache().agents.contains_key(&agent_id) {
            return Err(RegistryError::AgentAlreadyRegistered);
        }

        if let Some(ref account) = agent_account {
            if store.cache().index_by_account.contains_key(account) {
                return Err(RegistryError::AgentAccountAlreadyRegistered);
            }
        }

        let registered = RegisteredAgent {
            agent_id: agent_id.clone(),
            agent_card,
            endpoint_url,
            agent_account,
            controller,
            registered_at: now,
            last_heartbeat: now,
            last_health: None,
            status: AgentStatus::Active,
            health_failures: 0,
        };

        store.insert_agent(registered.clone())?;
        Ok(registered)
    }

    /// Unregister an agent by ID.
    pub async fn unregister(&self, agent_id: &Hash) -> Result<(), RegistryError> {
        let mut store = self.store.write().await;
        let mut guard = SnapshotGuard::new(&mut store)?;
        guard
            .store_mut()
            .remove_agent(agent_id)?
            .ok_or(RegistryError::AgentNotFound)?;
        guard.commit()?;
        Ok(())
    }

    /// Fetch a registered agent by ID.
    pub async fn get(&self, agent_id: &Hash) -> Option<RegisteredAgent> {
        let store = self.store.read().await;
        store.cache().agents.get(agent_id).cloned()
    }

    /// List all registered agents (including inactive).
    pub async fn list(&self) -> Vec<RegisteredAgent> {
        let store = self.store.read().await;
        store.cache().agents.values().cloned().collect()
    }

    /// List all active agents.
    pub async fn list_active(&self) -> Vec<RegisteredAgent> {
        let store = self.store.read().await;
        store
            .cache()
            .agents
            .values()
            .filter(|agent| agent.status == AgentStatus::Active)
            .cloned()
            .collect()
    }

    /// Fetch a registered agent by on-chain account.
    pub async fn get_by_account(&self, account: &PublicKey) -> Option<RegisteredAgent> {
        let store = self.store.read().await;
        if let Some(agent_id) = store.cache().index_by_account.get(account) {
            return store.cache().agents.get(agent_id).cloned();
        }
        store
            .cache()
            .agents
            .values()
            .find(|agent| agent.agent_account.as_ref() == Some(account))
            .cloned()
    }

    /// Update an existing agent's card.
    ///
    /// Note: The agent_id is assigned at registration and remains stable.
    /// The endpoint_url is immutable. TOS identity cannot be removed once set
    /// (to prevent disabling ownership verification).
    pub async fn update(
        &self,
        agent_id: &Hash,
        agent_card: AgentCard,
    ) -> Result<RegisteredAgent, RegistryError> {
        validate_agent_card(&agent_card)?;
        let mut store = self.store.write().await;
        let mut guard = SnapshotGuard::new(&mut store)?;
        let existing = guard
            .store_mut()
            .cache()
            .agents
            .get(agent_id)
            .cloned()
            .ok_or(RegistryError::AgentNotFound)?;

        // Prevent removing TOS identity once set (security: prevents disabling ownership checks)
        if existing.agent_account.is_some() && agent_card.tos_identity.is_none() {
            return Err(RegistryError::CannotRemoveTosIdentity);
        }

        // Prevent adding TOS identity to anonymous agents (security: prevents hijacking)
        // Agents registered without TOS identity must remain anonymous
        if existing.agent_account.is_none() && agent_card.tos_identity.is_some() {
            return Err(RegistryError::CannotAddTosIdentity);
        }

        // Prevent changing agent_account once set (security: prevents squatting on another account)
        // The agent_account is immutable; to change it, unregister and re-register
        if let (Some(existing_account), Some(new_identity)) =
            (&existing.agent_account, &agent_card.tos_identity)
        {
            if existing_account != &new_identity.agent_account {
                return Err(RegistryError::CannotChangeAgentAccount);
            }
        }

        let updated = RegisteredAgent {
            agent_id: existing.agent_id.clone(),
            endpoint_url: existing.endpoint_url.clone(),
            agent_account: agent_card
                .tos_identity
                .as_ref()
                .map(|id| id.agent_account.clone()),
            controller: agent_card
                .tos_identity
                .as_ref()
                .map(|id| id.controller.clone()),
            registered_at: existing.registered_at,
            last_heartbeat: existing.last_heartbeat,
            last_health: existing.last_health.clone(),
            status: existing.status,
            health_failures: existing.health_failures,
            agent_card,
        };

        guard.store_mut().update_agent(&existing, updated.clone())?;
        guard.commit()?;
        Ok(updated)
    }

    /// Fetch agents that match a given skill ID.
    pub async fn filter_by_skill(&self, skill: &str) -> Vec<RegisteredAgent> {
        let store = self.store.read().await;
        let cache = store.cache();
        cache
            .index_by_skill
            .get(skill)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| cache.agents.get(id).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Filter agents by skill and capability constraints.
    /// Returns error if filter input exceeds size limits.
    pub async fn filter(
        &self,
        filter: &AgentFilter,
    ) -> Result<Vec<RegisteredAgent>, RegistryError> {
        // Validate filter input sizes to prevent DoS
        Self::validate_filter(filter)?;

        let mut candidates = if let Some(skills) = filter.skills.as_ref() {
            self.filter_by_any_skill(skills).await?
        } else {
            self.list().await
        };

        candidates.retain(|agent| agent.status == AgentStatus::Active);

        if let Some(input_modes) = filter.input_modes.as_ref() {
            candidates.retain(|agent| supports_any_input_mode(agent, input_modes));
        }
        if let Some(output_modes) = filter.output_modes.as_ref() {
            candidates.retain(|agent| supports_any_output_mode(agent, output_modes));
        }
        if let Some(require_settlement) = filter.require_settlement {
            candidates.retain(|agent| {
                agent
                    .agent_card
                    .capabilities
                    .tos_on_chain_settlement
                    .unwrap_or(false)
                    == require_settlement
            });
        }
        if let Some(require_tos_identity) = filter.require_tos_identity {
            candidates
                .retain(|agent| agent.agent_card.tos_identity.is_some() == require_tos_identity);
        }

        if let Some(limit) = filter.limit {
            let limit = limit as usize;
            if candidates.len() > limit {
                candidates.truncate(limit);
            }
        }

        Ok(candidates)
    }

    /// Validate filter input sizes to prevent DoS attacks.
    fn validate_filter(filter: &AgentFilter) -> Result<(), RegistryError> {
        if let Some(ref skills) = filter.skills {
            if skills.len() > MAX_FILTER_SKILLS {
                return Err(RegistryError::FilterInputTooLarge);
            }
        }
        if let Some(ref input_modes) = filter.input_modes {
            if input_modes.len() > MAX_FILTER_INPUT_MODES {
                return Err(RegistryError::FilterInputTooLarge);
            }
        }
        if let Some(ref output_modes) = filter.output_modes {
            if output_modes.len() > MAX_FILTER_OUTPUT_MODES {
                return Err(RegistryError::FilterInputTooLarge);
            }
        }
        Ok(())
    }

    /// Fetch agents that match any of the provided skill IDs.
    /// Returns error if skills list exceeds size limit.
    pub async fn filter_by_any_skill(
        &self,
        skills: &[String],
    ) -> Result<Vec<RegisteredAgent>, RegistryError> {
        if skills.len() > MAX_FILTER_SKILLS {
            return Err(RegistryError::FilterInputTooLarge);
        }

        let store = self.store.read().await;
        let cache = store.cache();
        let mut agent_ids = HashSet::new();
        for skill in skills {
            if let Some(ids) = cache.index_by_skill.get(skill) {
                agent_ids.extend(ids.iter().cloned());
            }
        }
        Ok(agent_ids
            .into_iter()
            .filter_map(|id| cache.agents.get(&id).cloned())
            .collect())
    }

    /// Update heartbeat timestamp for an agent.
    pub async fn heartbeat(
        &self,
        agent_id: &Hash,
        status: Option<AgentHealthStatus>,
    ) -> Result<i64, RegistryError> {
        let now = current_timestamp_i64()?;
        let mut store = self.store.write().await;
        let mut guard = SnapshotGuard::new(&mut store)?;
        let existing = guard
            .store_mut()
            .cache()
            .agents
            .get(agent_id)
            .cloned()
            .ok_or(RegistryError::AgentNotFound)?;

        let mut updated = existing.clone();
        updated.last_heartbeat = now;
        if status.is_some() {
            updated.last_health = status;
        }
        if updated.status == AgentStatus::Inactive {
            updated.status = AgentStatus::Active;
        }

        guard.store_mut().update_agent(&existing, updated)?;
        guard.commit()?;

        Ok(now)
    }

    /// Mark an agent as inactive and increment failure count.
    pub async fn mark_inactive(&self, agent_id: &Hash) -> Result<(), RegistryError> {
        let mut store = self.store.write().await;
        let mut guard = SnapshotGuard::new(&mut store)?;
        let existing = guard
            .store_mut()
            .cache()
            .agents
            .get(agent_id)
            .cloned()
            .ok_or(RegistryError::AgentNotFound)?;
        let mut updated = existing.clone();
        updated.status = AgentStatus::Inactive;
        updated.health_failures = updated.health_failures.saturating_add(1);
        guard.store_mut().update_agent(&existing, updated)?;
        guard.commit()?;
        Ok(())
    }

    /// Run health checks and mark stale agents as inactive.
    pub async fn run_health_checks(
        &self,
        timeout_secs: u64,
        failure_threshold: u32,
    ) -> Result<usize, RegistryError> {
        let now = get_current_time_in_seconds();
        let mut store = self.store.write().await;
        let mut guard = SnapshotGuard::new(&mut store)?;

        let existing_agents: Vec<RegisteredAgent> =
            guard.store_mut().cache().agents.values().cloned().collect();
        let mut updates: Vec<(RegisteredAgent, RegisteredAgent)> = Vec::new();

        for agent in existing_agents {
            if agent.status != AgentStatus::Active {
                continue;
            }
            let last = u64::try_from(agent.last_heartbeat).unwrap_or(0);
            if now.saturating_sub(last) > timeout_secs {
                let mut updated = agent.clone();
                updated.health_failures = updated.health_failures.saturating_add(1);
                if updated.health_failures >= failure_threshold {
                    updated.status = AgentStatus::Inactive;
                }
                updates.push((agent, updated));
            }
        }

        for (existing, updated) in &updates {
            guard.store_mut().update_agent(existing, updated.clone())?;
        }

        guard.commit()?;
        Ok(updates.len())
    }
}

/// Compute deterministic agent ID from card + endpoint URL.
/// Uses canonical JSON serialization to ensure consistent IDs across processes.
pub fn compute_agent_id(card: &AgentCard, endpoint_url: &str) -> Result<Hash, RegistryError> {
    let card_bytes = canonical_serialize_card(card)?;
    let mut material = Vec::with_capacity(card_bytes.len() + endpoint_url.len() + 8);
    material.extend_from_slice(endpoint_url.as_bytes());
    material.extend_from_slice(&card_bytes);
    Ok(hash(&material))
}

/// Serialize an AgentCard in a canonical (deterministic) format.
/// This ensures that HashMap fields are serialized with sorted keys.
fn canonical_serialize_card(card: &AgentCard) -> Result<Vec<u8>, RegistryError> {
    // Convert to serde_json::Value first
    let mut value = serde_json::to_value(card).map_err(|_| RegistryError::SerializeAgentCard)?;

    // Recursively sort all object keys to ensure deterministic serialization
    canonicalize_json_value(&mut value);

    // Serialize to bytes
    serde_json::to_vec(&value).map_err(|_| RegistryError::SerializeAgentCard)
}

/// Recursively sort all object keys in a JSON value for deterministic serialization.
fn canonicalize_json_value(value: &mut Value) {
    match value {
        Value::Object(map) => {
            // Sort the keys by extracting, sorting, and reinserting
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

fn supports_input_mode(agent: &RegisteredAgent, input_mode: &str) -> bool {
    if agent
        .agent_card
        .default_input_modes
        .iter()
        .any(|mode| mode == input_mode)
    {
        return true;
    }
    agent
        .agent_card
        .skills
        .iter()
        .any(|skill| skill.input_modes.iter().any(|mode| mode == input_mode))
}

fn supports_any_input_mode(agent: &RegisteredAgent, input_modes: &[String]) -> bool {
    input_modes
        .iter()
        .any(|mode| supports_input_mode(agent, mode))
}

fn supports_output_mode(agent: &RegisteredAgent, output_mode: &str) -> bool {
    if agent
        .agent_card
        .default_output_modes
        .iter()
        .any(|mode| mode == output_mode)
    {
        return true;
    }
    agent
        .agent_card
        .skills
        .iter()
        .any(|skill| skill.output_modes.iter().any(|mode| mode == output_mode))
}

fn supports_any_output_mode(agent: &RegisteredAgent, output_modes: &[String]) -> bool {
    output_modes
        .iter()
        .any(|mode| supports_output_mode(agent, mode))
}

fn deserialize_opt_string_vec<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    match value {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(vec![s])),
        Some(Value::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Value::String(s) => out.push(s),
                    _ => {
                        return Err(serde::de::Error::custom(
                            "expected string or list of strings",
                        ))
                    }
                }
            }
            Ok(Some(out))
        }
        _ => Err(serde::de::Error::custom(
            "expected string or list of strings",
        )),
    }
}

fn current_timestamp_i64() -> Result<i64, RegistryError> {
    i64::try_from(get_current_time_in_seconds()).map_err(|_| RegistryError::TimestampOverflow)
}

fn validate_agent_card(card: &AgentCard) -> Result<(), RegistryError> {
    if card.skills.len() > MAX_SKILLS {
        return Err(RegistryError::InvalidAgentCard(
            "too many skills".to_string(),
        ));
    }
    if card.supported_interfaces.len() > MAX_INTERFACES {
        return Err(RegistryError::InvalidAgentCard(
            "too many interfaces".to_string(),
        ));
    }
    if card.security_schemes.len() > MAX_SECURITY_SCHEMES {
        return Err(RegistryError::InvalidAgentCard(
            "too many security schemes".to_string(),
        ));
    }
    if card.signatures.len() > MAX_SIGNATURES {
        return Err(RegistryError::InvalidAgentCard(
            "too many signatures".to_string(),
        ));
    }
    if card.capabilities.extensions.len() > MAX_EXTENSIONS {
        return Err(RegistryError::InvalidAgentCard(
            "too many capabilities extensions".to_string(),
        ));
    }
    Ok(())
}

/// Validate endpoint URL to prevent SSRF attacks.
/// Blocks private IP ranges, localhost, and non-HTTPS URLs.
fn validate_endpoint_url(endpoint_url: &str) -> Result<(), RegistryError> {
    // Check length limit
    if endpoint_url.len() > MAX_ENDPOINT_URL_LENGTH {
        return Err(RegistryError::EndpointUrlBlocked(
            "URL too long".to_string(),
        ));
    }

    // Parse URL
    let url = Url::parse(endpoint_url).map_err(|_| RegistryError::InvalidEndpointUrl)?;

    // Require HTTPS
    if url.scheme() != "https" {
        return Err(RegistryError::InvalidEndpointUrl);
    }

    // Get host
    let host = url.host_str().ok_or(RegistryError::InvalidEndpointUrl)?;

    // Block localhost variants
    if host == "localhost" || host == "127.0.0.1" || host == "::1" || host == "[::1]" {
        return Err(RegistryError::EndpointUrlBlocked(
            "localhost not allowed".to_string(),
        ));
    }

    // Try to parse as IP address
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_private_ip(&ip) {
            return Err(RegistryError::EndpointUrlBlocked(
                "private IP not allowed".to_string(),
            ));
        }
    }

    // Block common internal hostnames
    let host_lower = host.to_lowercase();
    if host_lower.ends_with(".local")
        || host_lower.ends_with(".internal")
        || host_lower.ends_with(".localhost")
        || host_lower == "metadata.google.internal"
        || host_lower == "169.254.169.254"
    {
        return Err(RegistryError::EndpointUrlBlocked(
            "internal hostname not allowed".to_string(),
        ));
    }

    Ok(())
}

/// Check if an IP address is in a private/reserved range.
fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => is_private_ipv4(ipv4),
        IpAddr::V6(ipv6) => is_private_ipv6(ipv6),
    }
}

fn is_private_ipv4(ip: &Ipv4Addr) -> bool {
    let octets = ip.octets();
    // 10.0.0.0/8
    if octets[0] == 10 {
        return true;
    }
    // 172.16.0.0/12
    if octets[0] == 172 && (16..=31).contains(&octets[1]) {
        return true;
    }
    // 192.168.0.0/16
    if octets[0] == 192 && octets[1] == 168 {
        return true;
    }
    // 127.0.0.0/8 (loopback)
    if octets[0] == 127 {
        return true;
    }
    // 169.254.0.0/16 (link-local, includes AWS metadata endpoint)
    if octets[0] == 169 && octets[1] == 254 {
        return true;
    }
    // 0.0.0.0/8
    if octets[0] == 0 {
        return true;
    }
    false
}

fn is_private_ipv6(ip: &Ipv6Addr) -> bool {
    // ::1 loopback
    if ip.is_loopback() {
        return true;
    }
    // :: unspecified
    if ip.is_unspecified() {
        return true;
    }
    let segments = ip.segments();
    // fe80::/10 link-local
    if (segments[0] & 0xffc0) == 0xfe80 {
        return true;
    }
    // fc00::/7 unique local
    if (segments[0] & 0xfe00) == 0xfc00 {
        return true;
    }
    // ::ffff:0:0/96 IPv4-mapped (check the embedded IPv4)
    if segments[0] == 0
        && segments[1] == 0
        && segments[2] == 0
        && segments[3] == 0
        && segments[4] == 0
        && segments[5] == 0xffff
    {
        let ipv4 = Ipv4Addr::new(
            (segments[6] >> 8) as u8,
            segments[6] as u8,
            (segments[7] >> 8) as u8,
            segments[7] as u8,
        );
        return is_private_ipv4(&ipv4);
    }
    false
}

#[derive(Debug, Serialize, Deserialize)]
struct RegistryIndex {
    agent_ids: Vec<String>,
}

static REGISTRY_BASE_DIR: OnceCell<PathBuf> = OnceCell::new();
static REGISTRY_LOADED: OnceCell<()> = OnceCell::new();
static GLOBAL_REGISTRY: Lazy<Arc<AgentRegistry>> = Lazy::new(|| Arc::new(AgentRegistry::new()));

/// Set base directory for registry persistence.
pub fn set_base_dir(dir: &str) {
    let _ = REGISTRY_BASE_DIR.set(PathBuf::from(dir));
}

/// Get the process-wide shared agent registry.
pub fn global_registry() -> Arc<AgentRegistry> {
    if REGISTRY_LOADED.get().is_none() {
        // Only mark as loaded if we successfully load (including runtime assignment)
        if load_registry_snapshot(&GLOBAL_REGISTRY) {
            let _ = REGISTRY_LOADED.set(());
        }
    }
    Arc::clone(&GLOBAL_REGISTRY)
}

/// Spawn background health check task using default settings.
pub fn spawn_health_checks() -> tokio::task::JoinHandle<()> {
    let registry = global_registry();
    tokio::spawn(async move {
        let interval = Duration::from_secs(DEFAULT_HEALTH_CHECK_INTERVAL_SECS);
        loop {
            let _ = registry
                .run_health_checks(DEFAULT_HEARTBEAT_TIMEOUT_SECS, DEFAULT_INACTIVE_FAILURES)
                .await;
            sleep(interval).await;
        }
    })
}

fn registry_root() -> Option<PathBuf> {
    let base = REGISTRY_BASE_DIR.get_or_init(|| PathBuf::from(""));
    if base.as_os_str().is_empty() {
        return None;
    }
    let mut path = base.clone();
    path.push("a2a");
    path.push("agents");
    Some(path)
}

fn index_path(root: &Path) -> PathBuf {
    let mut path = root.to_path_buf();
    path.push("index.json");
    path
}

fn agent_path(root: &Path, agent_id: &Hash) -> PathBuf {
    let mut path = root.to_path_buf();
    path.push(format!("{}.json", agent_id.to_hex()));
    path
}

/// Load registry from disk. Returns true if successfully loaded (or no persistence configured).
fn load_registry_snapshot(registry: &Arc<AgentRegistry>) -> bool {
    let base = REGISTRY_BASE_DIR.get_or_init(|| PathBuf::from(""));
    if base.as_os_str().is_empty() {
        // No persistence configured, consider this a success (in-memory only mode)
        return true;
    }

    let mut db_path = base.clone();
    db_path.push("a2a");
    db_path.push("registry");

    let mut store = match RegistryStore::open(&db_path) {
        Ok(store) => store,
        Err(_) => return false,
    };

    if store.is_empty() {
        let agents = load_agents_from_files();
        if !agents.is_empty() {
            let mut guard = match SnapshotGuard::new(&mut store) {
                Ok(guard) => guard,
                Err(_) => return false,
            };
            for agent in agents {
                if guard.store_mut().insert_agent(agent).is_err() {
                    let _ = guard.rollback();
                    return false;
                }
            }
            if guard.commit().is_err() {
                return false;
            }
        }
    }

    let registry_store = Arc::clone(registry);
    let registry_inner = async move {
        *registry_store.store.write().await = store;
    };

    // Only mark as loaded if we have a runtime to actually apply the store
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.block_on(registry_inner);
        true
    } else {
        // No runtime available, don't mark as loaded so we can retry later
        false
    }
}

fn load_agents_from_files() -> Vec<RegisteredAgent> {
    let Some(root) = registry_root() else {
        return Vec::new();
    };

    let mut agents = HashMap::new();
    let mut seen_accounts: HashSet<PublicKey> = HashSet::new();
    let index_path = index_path(&root);

    // Helper to validate and insert agent
    let mut try_insert_agent = |agent: RegisteredAgent| {
        // Validate endpoint URL (SSRF protection)
        if validate_endpoint_url(&agent.endpoint_url).is_err() {
            if log::log_enabled!(log::Level::Warn) {
                log::warn!(
                    "Skipping agent {} with invalid endpoint URL: {}",
                    agent.agent_id.to_hex(),
                    agent.endpoint_url
                );
            }
            return;
        }

        // Validate agent card
        if validate_agent_card(&agent.agent_card).is_err() {
            if log::log_enabled!(log::Level::Warn) {
                log::warn!(
                    "Skipping agent {} with invalid card",
                    agent.agent_id.to_hex()
                );
            }
            return;
        }

        // Check for duplicate accounts
        if let Some(ref account) = agent.agent_account {
            if seen_accounts.contains(account) {
                if log::log_enabled!(log::Level::Warn) {
                    log::warn!(
                        "Skipping agent {} with duplicate account",
                        agent.agent_id.to_hex()
                    );
                }
                return;
            }
            seen_accounts.insert(account.clone());
        }

        agents.insert(agent.agent_id.clone(), agent);
    };

    if let Ok(raw) = fs::read(&index_path) {
        if let Ok(index) = serde_json::from_slice::<RegistryIndex>(&raw) {
            for id in index.agent_ids {
                if let Ok(hash) = id.parse::<Hash>() {
                    let path = agent_path(&root, &hash);
                    if let Ok(bytes) = fs::read(&path) {
                        if let Ok(agent) = serde_json::from_slice::<RegisteredAgent>(&bytes) {
                            try_insert_agent(agent);
                        }
                    }
                }
            }
        }
    } else if let Ok(entries) = fs::read_dir(&root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.file_name().and_then(|s| s.to_str()) == Some("index.json") {
                continue;
            }
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            if let Ok(bytes) = fs::read(&path) {
                if let Ok(agent) = serde_json::from_slice::<RegisteredAgent>(&bytes) {
                    try_insert_agent(agent);
                }
            }
        }
    }

    agents.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tos_common::a2a::{AgentCapabilities, AgentInterface, AgentSkill};

    fn sample_card(name: &str, skill_id: &str) -> AgentCard {
        AgentCard {
            name: name.to_string(),
            description: "test".to_string(),
            version: "0.0.1".to_string(),
            supported_interfaces: vec![AgentInterface {
                protocol_version: "1.0".to_string(),
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
                extended_agent_card: None,
                extensions: Vec::new(),
                tos_on_chain_settlement: Some(false),
            },
            security_schemes: HashMap::new(),
            security_requirements: Vec::new(),
            default_input_modes: vec!["text/plain".to_string()],
            default_output_modes: vec!["text/plain".to_string()],
            skills: vec![AgentSkill {
                id: skill_id.to_string(),
                name: "skill".to_string(),
                description: "skill desc".to_string(),
                tags: Vec::new(),
                examples: Vec::new(),
                input_modes: vec!["text/plain".to_string()],
                output_modes: vec!["text/plain".to_string()],
                security_requirements: Vec::new(),
                tos_base_cost: None,
            }],
            signatures: Vec::new(),
            tos_identity: None,
            arbitration: None,
        }
    }

    fn build_registered_agent(name: &str, skill_id: &str, endpoint: &str) -> RegisteredAgent {
        let card = sample_card(name, skill_id);
        let agent_id = compute_agent_id(&card, endpoint).expect("agent id");
        let now = current_timestamp_i64().expect("timestamp");
        RegisteredAgent {
            agent_id,
            agent_card: card,
            endpoint_url: endpoint.to_string(),
            agent_account: None,
            controller: None,
            registered_at: now,
            last_heartbeat: now,
            last_health: None,
            status: AgentStatus::Active,
            health_failures: 0,
        }
    }

    #[tokio::test]
    async fn registry_register_get_and_unreg() -> Result<(), Box<dyn std::error::Error>> {
        let registry = AgentRegistry::new();
        let card = sample_card("agent", "skill:a");
        let registered = registry
            .register(card, "https://agent.test".to_string())
            .await?;

        let fetched = registry.get(&registered.agent_id).await;
        assert!(fetched.is_some());

        registry.unregister(&registered.agent_id).await?;

        let fetched = registry.get(&registered.agent_id).await;
        assert!(fetched.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn registry_filter_by_skill() -> Result<(), Box<dyn std::error::Error>> {
        let registry = AgentRegistry::new();
        let card_a = sample_card("agent-a", "skill:a");
        let card_b = sample_card("agent-b", "skill:b");

        let _ = registry
            .register(card_a, "https://a.test".to_string())
            .await?;
        let _ = registry
            .register(card_b, "https://b.test".to_string())
            .await?;

        let filtered = registry.filter_by_skill("skill:a").await;
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].agent_card.name, "agent-a");
        Ok(())
    }

    #[tokio::test]
    async fn registry_heartbeat_updates_timestamp() -> Result<(), Box<dyn std::error::Error>> {
        let registry = AgentRegistry::new();
        let card = sample_card("agent", "skill:a");
        let registered = registry
            .register(card, "https://agent.test".to_string())
            .await?;

        let before = registered.last_heartbeat;
        let now = registry.heartbeat(&registered.agent_id, None).await?;
        assert!(now >= before);
        Ok(())
    }

    #[test]
    fn snapshot_read_your_writes() {
        let mut store = RegistryStore::in_memory();
        store.start_snapshot().expect("start snapshot");

        let agent_a = build_registered_agent("agent-a", "skill:shared", "https://a.test");
        let agent_b = build_registered_agent("agent-b", "skill:shared", "https://b.test");

        store.insert_agent(agent_a.clone()).expect("insert a");
        store.insert_agent(agent_b.clone()).expect("insert b");

        let agents = store
            .cache()
            .index_by_skill
            .get("skill:shared")
            .expect("index");
        assert_eq!(agents.len(), 2);
        assert!(agents.contains(&agent_a.agent_id));
        assert!(agents.contains(&agent_b.agent_id));

        store.end_snapshot(true).expect("commit");

        let agents = store
            .cache()
            .index_by_skill
            .get("skill:shared")
            .expect("index");
        assert_eq!(agents.len(), 2);
        assert!(agents.contains(&agent_a.agent_id));
        assert!(agents.contains(&agent_b.agent_id));
    }

    #[test]
    fn snapshot_rollback_discards_changes() {
        let mut store = RegistryStore::in_memory();
        {
            let mut guard = SnapshotGuard::new(&mut store).expect("guard");
            let agent = build_registered_agent("agent-a", "skill:a", "https://a.test");
            guard.store_mut().insert_agent(agent).expect("insert");
            guard.rollback().expect("rollback");
        }

        assert!(store.cache().agents.is_empty());
        assert!(store.cache().index_by_skill.is_empty());
    }

    #[test]
    fn snapshot_commit_retry_after_failure() {
        let mut store = RegistryStore::in_memory();
        let mut guard = SnapshotGuard::new(&mut store).expect("guard");

        let agent = build_registered_agent("agent-a", "skill:a", "https://a.test");
        guard
            .store_mut()
            .insert_agent(agent.clone())
            .expect("insert");

        guard.store_mut().set_fail_commit(true);
        assert!(guard.commit().is_err());
        assert!(guard.store_mut().has_snapshot());

        guard.store_mut().set_fail_commit(false);
        guard.commit().expect("retry commit");
        drop(guard);

        assert!(store.cache().agents.contains_key(&agent.agent_id));
    }
}
