use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;

use tos_common::{
    a2a::AgentCard,
    crypto::{hash, Hash, PublicKey},
    time::get_current_time_in_seconds,
};

pub mod router;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentStatus {
    Active,
    Inactive,
    Suspended,
    Unregistered,
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
    pub status: AgentStatus,
    pub health_failures: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentFilter {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_mode: Option<String>,
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
    #[error("failed to serialize agent card")]
    SerializeAgentCard,
    #[error("timestamp overflow")]
    TimestampOverflow,
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
        if endpoint_url.trim().is_empty() {
            return Err(RegistryError::InvalidEndpointUrl);
        }

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
            status: AgentStatus::Active,
            health_failures: 0,
        };

        {
            let mut agents = self.agents.write().await;
            if agents.contains_key(&agent_id) {
                return Err(RegistryError::AgentAlreadyRegistered);
            }
            agents.insert(agent_id.clone(), registered.clone());
        }

        self.index_skills(agent_id.clone(), &registered.agent_card)
            .await;

        Ok(registered)
    }

    /// Unregister an agent by ID.
    pub async fn unregister(&self, agent_id: &Hash) -> Result<(), RegistryError> {
        let removed = {
            let mut agents = self.agents.write().await;
            agents.remove(agent_id)
        };
        let removed = removed.ok_or(RegistryError::AgentNotFound)?;
        self.remove_skills(agent_id, &removed.agent_card).await;
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
        let mut candidates = if let Some(skill) = filter.skill.as_ref() {
            self.filter_by_skill(skill).await
        } else {
            self.list().await
        };

        candidates.retain(|agent| agent.status == AgentStatus::Active);

        if let Some(input_mode) = filter.input_mode.as_ref() {
            candidates.retain(|agent| supports_input_mode(agent, input_mode));
        }
        if let Some(output_mode) = filter.output_mode.as_ref() {
            candidates.retain(|agent| supports_output_mode(agent, output_mode));
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

    /// Update heartbeat timestamp for an agent.
    pub async fn heartbeat(&self, agent_id: &Hash) -> Result<i64, RegistryError> {
        let now = current_timestamp_i64()?;
        let mut agents = self.agents.write().await;
        let agent = agents
            .get_mut(agent_id)
            .ok_or(RegistryError::AgentNotFound)?;
        agent.last_heartbeat = now;
        if agent.status == AgentStatus::Inactive {
            agent.status = AgentStatus::Active;
        }
        Ok(now)
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
pub fn compute_agent_id(card: &AgentCard, endpoint_url: &str) -> Result<Hash, RegistryError> {
    let card_bytes = serde_json::to_vec(card).map_err(|_| RegistryError::SerializeAgentCard)?;
    let mut material = Vec::with_capacity(card_bytes.len() + endpoint_url.len() + 8);
    material.extend_from_slice(endpoint_url.as_bytes());
    material.extend_from_slice(&card_bytes);
    Ok(hash(&material))
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

fn extract_skill_ids(card: &AgentCard) -> Vec<String> {
    card.skills.iter().map(|skill| skill.id.clone()).collect()
}

fn current_timestamp_i64() -> Result<i64, RegistryError> {
    i64::try_from(get_current_time_in_seconds()).map_err(|_| RegistryError::TimestampOverflow)
}

static GLOBAL_REGISTRY: Lazy<Arc<AgentRegistry>> = Lazy::new(|| Arc::new(AgentRegistry::new()));

/// Get the process-wide shared agent registry.
pub fn global_registry() -> Arc<AgentRegistry> {
    Arc::clone(&GLOBAL_REGISTRY)
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
        }
    }

    #[tokio::test]
    async fn registry_register_get_and_unreg() -> Result<(), Box<dyn std::error::Error>> {
        let registry = AgentRegistry::new();
        let card = sample_card("agent", "skill:a");
        let registered = registry
            .register(card, "http://agent.test".to_string())
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
            .register(card_a, "http://a.test".to_string())
            .await?;
        let _ = registry
            .register(card_b, "http://b.test".to_string())
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
            .register(card, "http://agent.test".to_string())
            .await?;

        let before = registered.last_heartbeat;
        let now = registry.heartbeat(&registered.agent_id).await?;
        assert!(now >= before);
        Ok(())
    }
}
