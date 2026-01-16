use std::cmp::Ordering;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::sync::Arc;

use thiserror::Error;

use tos_common::a2a::AgentCard;
use tos_common::time::get_current_time_in_seconds;

use super::{AgentRegistry, AgentStatus, RegisteredAgent};

const DEFAULT_HEARTBEAT_TIMEOUT_SECS: u64 = 120;

#[derive(Clone, Copy, Debug)]
pub enum RoutingStrategy {
    FirstMatch,
    LowestLatency,
    HighestReputation,
    RoundRobin,
}

#[derive(Debug, Error)]
pub enum RouterError {
    #[error("no available agents for skill")]
    NoAvailableAgents,
}

pub struct AgentRouter {
    registry: Arc<AgentRegistry>,
    rr_counter: AtomicUsize,
    heartbeat_timeout_secs: u64,
}

impl AgentRouter {
    /// Create a router bound to the provided registry.
    pub fn new(registry: Arc<AgentRegistry>) -> Self {
        Self {
            registry,
            rr_counter: AtomicUsize::new(0),
            heartbeat_timeout_secs: DEFAULT_HEARTBEAT_TIMEOUT_SECS,
        }
    }

    /// Set custom heartbeat timeout for health filtering.
    pub fn with_heartbeat_timeout(mut self, timeout_secs: u64) -> Self {
        self.heartbeat_timeout_secs = timeout_secs;
        self
    }

    /// Select an agent card for the given skill using a routing strategy.
    pub async fn route_request(
        &self,
        skill: &str,
        strategy: RoutingStrategy,
    ) -> Result<AgentCard, RouterError> {
        let mut candidates = self.registry.filter_by_skill(skill).await;
        candidates.retain(|agent| self.is_healthy(agent));

        if candidates.is_empty() {
            return Err(RouterError::NoAvailableAgents);
        }

        let selected = match strategy {
            RoutingStrategy::FirstMatch => candidates.into_iter().next(),
            RoutingStrategy::LowestLatency => candidates
                .into_iter()
                .max_by(|a, b| a.last_heartbeat.cmp(&b.last_heartbeat)),
            RoutingStrategy::HighestReputation => candidates
                .into_iter()
                .max_by(|a, b| compare_reputation(a, b)),
            RoutingStrategy::RoundRobin => {
                candidates.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));
                let idx = self.rr_counter.fetch_add(1, AtomicOrdering::Relaxed) % candidates.len();
                candidates.into_iter().nth(idx)
            }
        };

        selected
            .map(|agent| agent.agent_card)
            .ok_or(RouterError::NoAvailableAgents)
    }

    fn is_healthy(&self, agent: &RegisteredAgent) -> bool {
        if agent.status != AgentStatus::Active {
            return false;
        }
        let now = get_current_time_in_seconds();
        let last = u64::try_from(agent.last_heartbeat).unwrap_or(0);
        now.saturating_sub(last) <= self.heartbeat_timeout_secs
    }
}

fn compare_reputation(a: &RegisteredAgent, b: &RegisteredAgent) -> Ordering {
    let rep_a = a
        .agent_card
        .tos_identity
        .as_ref()
        .and_then(|id| id.reputation_score_bps)
        .unwrap_or(0);
    let rep_b = b
        .agent_card
        .tos_identity
        .as_ref()
        .and_then(|id| id.reputation_score_bps)
        .unwrap_or(0);
    rep_a.cmp(&rep_b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a2a::registry::AgentRegistry;
    use tos_common::a2a::{AgentCapabilities, AgentInterface, AgentSkill};
    use tos_common::serializer::Serializer;

    fn sample_card(
        name: &str,
        skill_id: &str,
        rep: Option<u32>,
    ) -> Result<AgentCard, Box<dyn std::error::Error>> {
        let agent_account = tos_common::crypto::PublicKey::from_bytes(&[1u8; 32])?;
        let controller = tos_common::crypto::PublicKey::from_bytes(&[2u8; 32])?;
        Ok(AgentCard {
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
            security_schemes: std::collections::HashMap::new(),
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
            tos_identity: rep.map(|score| tos_common::a2a::TosAgentIdentity {
                agent_account,
                controller,
                reputation_score_bps: Some(score),
                identity_proof: None,
            }),
        })
    }

    #[tokio::test]
    async fn router_selects_highest_reputation() -> Result<(), Box<dyn std::error::Error>> {
        let registry = Arc::new(AgentRegistry::new());
        let card_a = sample_card("agent-a", "skill:a", Some(100))?;
        let card_b = sample_card("agent-b", "skill:a", Some(500))?;

        let _ = registry
            .register(card_a, "http://a.test".to_string())
            .await?;
        let _ = registry
            .register(card_b, "http://b.test".to_string())
            .await?;

        let router = AgentRouter::new(Arc::clone(&registry));
        let selected = router
            .route_request("skill:a", RoutingStrategy::HighestReputation)
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        assert_eq!(selected.name, "agent-b");
        Ok(())
    }

    #[tokio::test]
    async fn router_round_robin() -> Result<(), Box<dyn std::error::Error>> {
        let registry = Arc::new(AgentRegistry::new());
        let card_a = sample_card("agent-a", "skill:a", None)?;
        let card_b = sample_card("agent-b", "skill:a", None)?;

        let _ = registry
            .register(card_a, "http://a.test".to_string())
            .await?;
        let _ = registry
            .register(card_b, "http://b.test".to_string())
            .await?;

        let router = AgentRouter::new(Arc::clone(&registry));
        let first = router
            .route_request("skill:a", RoutingStrategy::RoundRobin)
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        let second = router
            .route_request("skill:a", RoutingStrategy::RoundRobin)
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        assert_ne!(first.name, second.name);
        Ok(())
    }
}
