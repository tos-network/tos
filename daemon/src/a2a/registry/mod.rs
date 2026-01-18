use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use once_cell::sync::Lazy;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};

use tos_common::{
    a2a::AgentCard,
    a2a::{MAX_EXTENSIONS, MAX_INTERFACES, MAX_SECURITY_SCHEMES, MAX_SIGNATURES, MAX_SKILLS},
    crypto::{hash, Hash, PublicKey},
    time::get_current_time_in_seconds,
};

pub mod router;
mod store;

use store::A2ARegistryStore;

const DEFAULT_HEALTH_CHECK_INTERVAL_SECS: u64 = 300;
const DEFAULT_HEARTBEAT_TIMEOUT_SECS: u64 = 120;
const DEFAULT_INACTIVE_FAILURES: u32 = 3;

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
}

pub struct AgentRegistry {
    agents: RwLock<HashMap<Hash, RegisteredAgent>>,
    index_by_skill: RwLock<HashMap<String, HashSet<Hash>>>,
}

impl AgentRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            agents: RwLock::new(HashMap::new()),
            index_by_skill: RwLock::new(HashMap::new()),
        }
    }

    /// Register a new agent and return its registry record.
    pub async fn register(
        &self,
        agent_card: AgentCard,
        endpoint_url: String,
    ) -> Result<RegisteredAgent, RegistryError> {
        if endpoint_url.trim().is_empty() || !endpoint_url.starts_with("https://") {
            return Err(RegistryError::InvalidEndpointUrl);
        }
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

        {
            let mut agents = self.agents.write().await;

            // Check for duplicate agent_id
            if agents.contains_key(&agent_id) {
                return Err(RegistryError::AgentAlreadyRegistered);
            }

            // Enforce 1:1 mapping between agent_account and agent (inside write lock to prevent race)
            // An account can only have one registered agent at a time
            if let Some(ref account) = registered.agent_account {
                let account_exists = agents
                    .values()
                    .any(|a| a.agent_account.as_ref() == Some(account));
                if account_exists {
                    return Err(RegistryError::AgentAccountAlreadyRegistered);
                }
            }

            // Persist BEFORE updating in-memory state to prevent divergence on failure.
            // This ensures that if persistence fails, the in-memory state is unchanged.
            persist_agent_with_indexes(&registered)?;

            // Only insert into in-memory state after successful persistence
            agents.insert(agent_id.clone(), registered.clone());
        }

        // Index skills after registration is fully committed
        self.index_skills(agent_id.clone(), &registered.agent_card)
            .await;

        // Persist the full index (this is optional/optimization, not critical for consistency)
        let _ = persist_index(&self.agents).await;

        Ok(registered)
    }

    /// Unregister an agent by ID.
    pub async fn unregister(&self, agent_id: &Hash) -> Result<(), RegistryError> {
        let mut agents = self.agents.write().await;
        let removed = agents
            .get(agent_id)
            .cloned()
            .ok_or(RegistryError::AgentNotFound)?;

        // Persist removal BEFORE updating in-memory state to prevent divergence on failure
        remove_agent_with_indexes(&removed)?;

        // Only remove from in-memory state after successful persistence
        agents.remove(agent_id);
        drop(agents);

        // Remove skill indexes after successful unregistration
        self.remove_skills(agent_id, &removed.agent_card).await;

        // Persist the full index (optional optimization)
        let _ = persist_index(&self.agents).await;
        Ok(())
    }

    /// Fetch a registered agent by ID.
    pub async fn get(&self, agent_id: &Hash) -> Option<RegisteredAgent> {
        let agents = self.agents.read().await;
        agents.get(agent_id).cloned()
    }

    /// List all registered agents (including inactive).
    pub async fn list(&self) -> Vec<RegisteredAgent> {
        let agents = self.agents.read().await;
        agents.values().cloned().collect()
    }

    /// List all active agents.
    pub async fn list_active(&self) -> Vec<RegisteredAgent> {
        let agents = self.agents.read().await;
        agents
            .values()
            .filter(|agent| agent.status == AgentStatus::Active)
            .cloned()
            .collect()
    }

    /// Fetch a registered agent by on-chain account.
    pub async fn get_by_account(&self, account: &PublicKey) -> Option<RegisteredAgent> {
        if let Ok(Some(store)) = registry_store() {
            if let Ok(Some(agent_id)) = store.get_agent_id_by_account(account) {
                if let Some(agent) = self.get(&agent_id).await {
                    return Some(agent);
                }
            }
        }

        let agents = self.agents.read().await;
        agents
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
        let mut agents = self.agents.write().await;
        let existing = agents
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

        // Persist BEFORE updating in-memory state to prevent divergence on failure.
        // Update store indexes and record atomically (if either fails, in-memory state remains unchanged).
        update_agent_with_indexes(&existing, &updated)?;

        // Only update in-memory state after successful persistence
        agents.insert(existing.agent_id.clone(), updated.clone());
        drop(agents);

        // Update skill indexes (best-effort, in-memory only)
        self.remove_skills(agent_id, &existing.agent_card).await;
        self.index_skills(updated.agent_id.clone(), &updated.agent_card)
            .await;

        // Persist the full index (optional optimization)
        let _ = persist_index(&self.agents).await;

        Ok(updated)
    }

    /// Fetch agents that match a given skill ID.
    pub async fn filter_by_skill(&self, skill: &str) -> Vec<RegisteredAgent> {
        let agent_ids = {
            let index = self.index_by_skill.read().await;
            index.get(skill).cloned().unwrap_or_else(HashSet::new)
        };

        let agents = self.agents.read().await;
        agent_ids
            .into_iter()
            .filter_map(|id| agents.get(&id).cloned())
            .collect()
    }

    /// Filter agents by skill and capability constraints.
    pub async fn filter(&self, filter: &AgentFilter) -> Vec<RegisteredAgent> {
        let mut candidates = if let Some(skills) = filter.skills.as_ref() {
            self.filter_by_any_skill(skills).await
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

        candidates
    }

    /// Fetch agents that match any of the provided skill IDs.
    pub async fn filter_by_any_skill(&self, skills: &[String]) -> Vec<RegisteredAgent> {
        let mut agent_ids = HashSet::new();
        let index = self.index_by_skill.read().await;
        for skill in skills {
            if let Some(ids) = index.get(skill) {
                agent_ids.extend(ids.iter().cloned());
            }
        }
        drop(index);

        let agents = self.agents.read().await;
        agent_ids
            .into_iter()
            .filter_map(|id| agents.get(&id).cloned())
            .collect()
    }

    /// Update heartbeat timestamp for an agent.
    pub async fn heartbeat(
        &self,
        agent_id: &Hash,
        status: Option<AgentHealthStatus>,
    ) -> Result<i64, RegistryError> {
        let now = current_timestamp_i64()?;

        // Build updated agent record without modifying in-memory state yet
        let updated = {
            let agents = self.agents.read().await;
            let agent = agents.get(agent_id).ok_or(RegistryError::AgentNotFound)?;
            let mut updated = agent.clone();
            updated.last_heartbeat = now;
            if status.is_some() {
                updated.last_health = status;
            }
            if updated.status == AgentStatus::Inactive {
                updated.status = AgentStatus::Active;
            }
            updated
        };

        // Persist BEFORE updating in-memory state to prevent divergence on failure
        persist_agent_record(&updated)?;

        // Only update in-memory state after successful persistence
        let mut agents = self.agents.write().await;
        agents.insert(agent_id.clone(), updated);

        Ok(now)
    }

    /// Mark an agent as inactive and increment failure count.
    pub async fn mark_inactive(&self, agent_id: &Hash) -> Result<(), RegistryError> {
        // Build updated agent record without modifying in-memory state yet
        let updated = {
            let agents = self.agents.read().await;
            let agent = agents.get(agent_id).ok_or(RegistryError::AgentNotFound)?;
            let mut updated = agent.clone();
            updated.status = AgentStatus::Inactive;
            updated.health_failures = updated.health_failures.saturating_add(1);
            updated
        };

        // Persist BEFORE updating in-memory state to prevent divergence on failure
        persist_agent_record(&updated)?;

        // Only update in-memory state after successful persistence
        let mut agents = self.agents.write().await;
        agents.insert(agent_id.clone(), updated);

        Ok(())
    }

    /// Run health checks and mark stale agents as inactive.
    pub async fn run_health_checks(
        &self,
        timeout_secs: u64,
        failure_threshold: u32,
    ) -> Result<usize, RegistryError> {
        let now = get_current_time_in_seconds();

        // Build list of updated agent records without modifying in-memory state yet
        let updated: Vec<RegisteredAgent> = {
            let agents = self.agents.read().await;
            agents
                .values()
                .filter_map(|agent| {
                    if agent.status != AgentStatus::Active {
                        return None;
                    }
                    let last = u64::try_from(agent.last_heartbeat).unwrap_or(0);
                    if now.saturating_sub(last) > timeout_secs {
                        let mut updated = agent.clone();
                        updated.health_failures = updated.health_failures.saturating_add(1);
                        if updated.health_failures >= failure_threshold {
                            updated.status = AgentStatus::Inactive;
                        }
                        Some(updated)
                    } else {
                        None
                    }
                })
                .collect()
        };

        // Persist BEFORE updating in-memory state to prevent divergence on failure
        for agent in &updated {
            persist_agent_record(agent)?;
        }

        // Only update in-memory state after ALL persistence succeeds
        {
            let mut agents = self.agents.write().await;
            for agent in &updated {
                agents.insert(agent.agent_id.clone(), agent.clone());
            }
        }

        Ok(updated.len())
    }

    async fn index_skills(&self, agent_id: Hash, card: &AgentCard) {
        let skills = extract_skill_ids(card);
        if skills.is_empty() {
            return;
        }
        let mut index = self.index_by_skill.write().await;
        for skill in skills {
            index.entry(skill).or_default().insert(agent_id.clone());
        }
    }

    async fn remove_skills(&self, agent_id: &Hash, card: &AgentCard) {
        let skills = extract_skill_ids(card);
        if skills.is_empty() {
            return;
        }
        let mut index = self.index_by_skill.write().await;
        for skill in skills {
            if let Some(agent_ids) = index.get_mut(&skill) {
                agent_ids.remove(agent_id);
                if agent_ids.is_empty() {
                    index.remove(&skill);
                }
            }
        }
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

fn extract_skill_ids(card: &AgentCard) -> Vec<String> {
    card.skills.iter().map(|skill| skill.id.clone()).collect()
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

#[derive(Debug, Serialize, Deserialize)]
struct RegistryIndex {
    agent_ids: Vec<String>,
}

static REGISTRY_BASE_DIR: OnceCell<PathBuf> = OnceCell::new();
static REGISTRY_LOADED: OnceCell<()> = OnceCell::new();
static REGISTRY_STORE: OnceCell<A2ARegistryStore> = OnceCell::new();
static GLOBAL_REGISTRY: Lazy<Arc<AgentRegistry>> = Lazy::new(|| Arc::new(AgentRegistry::new()));

/// Set base directory for registry persistence.
pub fn set_base_dir(dir: &str) {
    let _ = REGISTRY_BASE_DIR.set(PathBuf::from(dir));
}

/// Get the process-wide shared agent registry.
pub fn global_registry() -> Arc<AgentRegistry> {
    if REGISTRY_LOADED.get().is_none() {
        let _ = REGISTRY_LOADED.set(());
        load_registry_snapshot(&GLOBAL_REGISTRY);
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

fn registry_store() -> Result<Option<&'static A2ARegistryStore>, RegistryError> {
    let base = REGISTRY_BASE_DIR.get_or_init(|| PathBuf::from(""));
    if base.as_os_str().is_empty() {
        return Ok(None);
    }
    let mut path = base.clone();
    path.push("a2a");
    path.push("registry");
    let store = REGISTRY_STORE.get_or_try_init(|| A2ARegistryStore::open(&path))?;
    Ok(Some(store))
}

fn ensure_registry_dir(path: &Path) -> Result<(), RegistryError> {
    fs::create_dir_all(path).map_err(|e| RegistryError::Storage(e.to_string()))
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

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), RegistryError> {
    let mut tmp = path.to_path_buf();
    tmp.set_extension("tmp");
    fs::write(&tmp, bytes).map_err(|e| RegistryError::Storage(e.to_string()))?;
    fs::rename(&tmp, path).map_err(|e| RegistryError::Storage(e.to_string()))
}

fn persist_agent_record(agent: &RegisteredAgent) -> Result<(), RegistryError> {
    if let Some(store) = registry_store()? {
        store.save_agent(agent)?;
        return Ok(());
    }

    let Some(root) = registry_root() else {
        return Ok(());
    };
    ensure_registry_dir(&root)?;
    let path = agent_path(&root, &agent.agent_id);
    let bytes =
        serde_json::to_vec_pretty(agent).map_err(|e| RegistryError::Storage(e.to_string()))?;
    write_atomic(&path, &bytes)?;
    Ok(())
}

fn remove_agent_record(agent_id: &Hash) -> Result<(), RegistryError> {
    if let Some(store) = registry_store()? {
        store.remove_agent(agent_id)?;
        return Ok(());
    }

    let Some(root) = registry_root() else {
        return Ok(());
    };
    let path = agent_path(&root, agent_id);
    if path.exists() {
        fs::remove_file(&path).map_err(|e| RegistryError::Storage(e.to_string()))?;
    }
    Ok(())
}

async fn persist_index(
    agents: &RwLock<HashMap<Hash, RegisteredAgent>>,
) -> Result<(), RegistryError> {
    if registry_store()?.is_some() {
        return Ok(());
    }

    let Some(root) = registry_root() else {
        return Ok(());
    };
    ensure_registry_dir(&root)?;
    let agents = agents.read().await;
    let mut ids: Vec<String> = agents.keys().map(|id| id.to_hex()).collect();
    ids.sort();
    let index = RegistryIndex { agent_ids: ids };
    let bytes =
        serde_json::to_vec_pretty(&index).map_err(|e| RegistryError::Storage(e.to_string()))?;
    write_atomic(&index_path(&root), &bytes)?;
    Ok(())
}

fn add_store_indexes(agent: &RegisteredAgent) -> Result<(), RegistryError> {
    let Some(store) = registry_store()? else {
        return Ok(());
    };
    for skill in extract_skill_ids(&agent.agent_card) {
        store.add_agent_to_skill(&agent.agent_id, &skill)?;
    }
    if let Some(account) = agent.agent_account.as_ref() {
        store.set_account_index(account, &agent.agent_id)?;
    }
    Ok(())
}

fn persist_agent_with_indexes(agent: &RegisteredAgent) -> Result<(), RegistryError> {
    if let Some(store) = registry_store()? {
        let skills = extract_skill_ids(&agent.agent_card);
        store.save_agent_with_indexes(agent, &skills)?;
        return Ok(());
    }
    persist_agent_record(agent)?;
    add_store_indexes(agent)?;
    Ok(())
}

fn remove_agent_with_indexes(agent: &RegisteredAgent) -> Result<(), RegistryError> {
    if let Some(store) = registry_store()? {
        let skills = extract_skill_ids(&agent.agent_card);
        store.remove_agent_with_indexes(agent, &skills)?;
        return Ok(());
    }
    remove_store_indexes(agent)?;
    remove_agent_record(&agent.agent_id)?;
    Ok(())
}

fn update_agent_with_indexes(
    existing: &RegisteredAgent,
    updated: &RegisteredAgent,
) -> Result<(), RegistryError> {
    if let Some(store) = registry_store()? {
        let existing_skills = extract_skill_ids(&existing.agent_card);
        let updated_skills = extract_skill_ids(&updated.agent_card);
        store.update_agent_with_indexes(existing, updated, &existing_skills, &updated_skills)?;
        return Ok(());
    }
    remove_store_indexes(existing)?;
    persist_agent_record(updated)?;
    add_store_indexes(updated)?;
    Ok(())
}

fn remove_store_indexes(agent: &RegisteredAgent) -> Result<(), RegistryError> {
    let Some(store) = registry_store()? else {
        return Ok(());
    };
    for skill in extract_skill_ids(&agent.agent_card) {
        store.remove_agent_from_skill(&agent.agent_id, &skill)?;
    }
    if let Some(account) = agent.agent_account.as_ref() {
        store.remove_account_index(account)?;
    }
    Ok(())
}

fn load_registry_snapshot(registry: &Arc<AgentRegistry>) {
    let mut agents = HashMap::new();
    let mut loaded_from_store = false;
    if let Ok(Some(store)) = registry_store() {
        if let Ok(store_agents) = store.load_agents() {
            if !store_agents.is_empty() {
                for agent in store_agents {
                    agents.insert(agent.agent_id.clone(), agent);
                }
                loaded_from_store = true;
            }
        }
    }

    if !loaded_from_store {
        let Some(root) = registry_root() else {
            return;
        };
        let index_path = index_path(&root);
        if let Ok(raw) = fs::read(&index_path) {
            if let Ok(index) = serde_json::from_slice::<RegistryIndex>(&raw) {
                for id in index.agent_ids {
                    if let Ok(hash) = id.parse::<Hash>() {
                        let path = agent_path(&root, &hash);
                        if let Ok(bytes) = fs::read(&path) {
                            if let Ok(agent) = serde_json::from_slice::<RegisteredAgent>(&bytes) {
                                agents.insert(hash, agent);
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
                        agents.insert(agent.agent_id.clone(), agent);
                    }
                }
            }
        }

        if let Ok(Some(_store)) = registry_store() {
            for agent in agents.values() {
                let _ = persist_agent_with_indexes(agent);
            }
        }
    }

    let index = agents.iter().fold(
        HashMap::<String, HashSet<Hash>>::new(),
        |mut acc, (id, agent)| {
            for skill in extract_skill_ids(&agent.agent_card) {
                acc.entry(skill).or_default().insert(id.clone());
            }
            acc
        },
    );

    let registry_agents = Arc::clone(registry);
    let registry_index = index;
    let agents_map = agents;

    let registry_inner = async move {
        *registry_agents.agents.write().await = agents_map;
        *registry_agents.index_by_skill.write().await = registry_index;
    };

    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.block_on(registry_inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tos_common::a2a::{AgentCapabilities, AgentInterface, AgentSkill};

    fn sample_card(name: &str, skill_id: &str) -> AgentCard {
        AgentCard {
            protocol_version: "1.0".to_string(),
            name: name.to_string(),
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
            security_schemes: HashMap::new(),
            security: Vec::new(),
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
                security: Vec::new(),
                tos_base_cost: None,
            }],
            supports_extended_agent_card: Some(false),
            signatures: Vec::new(),
            tos_identity: None,
            arbitration: None,
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
}
