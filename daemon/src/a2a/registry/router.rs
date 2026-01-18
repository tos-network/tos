use std::cmp::Ordering;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;

use rand::distributions::WeightedIndex;
use rand::prelude::Distribution;
use reqwest::Client;
use serde_json;
use tos_common::a2a::{
    A2AError, A2AResult, AgentCard, Message, SendMessageRequest, SendMessageResponse,
    HEADER_VERSION, PROTOCOL_VERSION,
};
use tos_common::time::get_current_time_in_seconds;

use super::{AgentRegistry, AgentStatus, RegisteredAgent};

const DEFAULT_HEARTBEAT_TIMEOUT_SECS: u64 = 120;
const DEFAULT_ROUTER_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_ROUTER_RETRY_COUNT: u32 = 2;
// Maximum response body size (1 MB) to prevent DoS via large responses
const MAX_RESPONSE_BODY_SIZE: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub enum RoutingStrategy {
    FirstMatch,
    LowestLatency,
    HighestReputation,
    RoundRobin,
    WeightedRandom,
}

#[derive(Debug, Error)]
pub enum RouterError {
    #[error("no available agents for skill")]
    NoAvailableAgents,
}

#[derive(Clone, Debug)]
pub struct RouterConfig {
    pub strategy: RoutingStrategy,
    pub timeout_ms: u64,
    pub retry_count: u32,
    pub fallback_to_local: bool,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            strategy: RoutingStrategy::LowestLatency,
            timeout_ms: DEFAULT_ROUTER_TIMEOUT_MS,
            retry_count: DEFAULT_ROUTER_RETRY_COUNT,
            fallback_to_local: true,
        }
    }
}

pub struct AgentRouter {
    registry: Arc<AgentRegistry>,
    rr_counter: AtomicUsize,
    heartbeat_timeout_secs: u64,
    http_client: Client,
    config: RouterConfig,
}

impl AgentRouter {
    /// Create a router bound to the provided registry.
    pub fn new(registry: Arc<AgentRegistry>) -> Self {
        Self {
            registry,
            rr_counter: AtomicUsize::new(0),
            heartbeat_timeout_secs: DEFAULT_HEARTBEAT_TIMEOUT_SECS,
            http_client: Client::new(),
            config: RouterConfig::default(),
        }
    }

    /// Set custom heartbeat timeout for health filtering.
    pub fn with_heartbeat_timeout(mut self, timeout_secs: u64) -> Self {
        self.heartbeat_timeout_secs = timeout_secs;
        self
    }

    /// Set router configuration (strategy, timeout, retries, fallback).
    pub fn with_config(mut self, config: RouterConfig) -> Self {
        self.config = config;
        self
    }

    /// Access router configuration.
    pub fn config(&self) -> &RouterConfig {
        &self.config
    }

    /// Select an agent card for the given skill using a routing strategy.
    pub async fn route_request(
        &self,
        skill: &str,
        strategy: RoutingStrategy,
    ) -> Result<AgentCard, RouterError> {
        let mut candidates = self.registry.filter_by_skill(skill).await;
        candidates.retain(|agent| self.is_healthy(agent));
        let selected = self.select_from_candidates(candidates, strategy);

        selected
            .map(|agent| agent.agent_card)
            .ok_or(RouterError::NoAvailableAgents)
    }

    /// Select a full registered agent for the given skill and strategy.
    pub async fn route_agent(
        &self,
        skill: &str,
        strategy: RoutingStrategy,
    ) -> Result<RegisteredAgent, RouterError> {
        let mut candidates = self.registry.filter_by_skill(skill).await;
        candidates.retain(|agent| self.is_healthy(agent));
        self.select_from_candidates(candidates, strategy)
            .ok_or(RouterError::NoAvailableAgents)
    }

    /// Route using multiple required skills. Uses intersection first, then optionally falls back
    /// to any-skill matching when no intersection candidates are available.
    pub async fn route_agent_by_skills(
        &self,
        skills: &[String],
        strategy: RoutingStrategy,
        fallback_any: bool,
    ) -> Result<RegisteredAgent, RouterError> {
        let Some((first, rest)) = skills.split_first() else {
            return Err(RouterError::NoAvailableAgents);
        };
        let mut candidates = self.registry.filter_by_skill(first).await;
        candidates.retain(|agent| self.is_healthy(agent));
        if !rest.is_empty() {
            candidates.retain(|agent| {
                rest.iter().all(|skill| {
                    agent
                        .agent_card
                        .skills
                        .iter()
                        .any(|agent_skill| agent_skill.id == *skill)
                })
            });
        }

        if candidates.is_empty() && fallback_any {
            // If filter fails due to too many skills, treat as no candidates
            candidates = self
                .registry
                .filter_by_any_skill(skills)
                .await
                .unwrap_or_default();
            candidates.retain(|agent| self.is_healthy(agent));
        }

        self.select_from_candidates(candidates, strategy)
            .ok_or(RouterError::NoAvailableAgents)
    }

    fn select_from_candidates(
        &self,
        mut candidates: Vec<RegisteredAgent>,
        strategy: RoutingStrategy,
    ) -> Option<RegisteredAgent> {
        if candidates.is_empty() {
            return None;
        }
        match strategy {
            RoutingStrategy::FirstMatch => candidates.into_iter().next(),
            RoutingStrategy::LowestLatency => {
                // Prefer agents with lowest reported latency.
                // If no latency data available, fall back to most recent heartbeat.
                candidates.into_iter().min_by(|a, b| {
                    let latency_a = a.last_health.as_ref().map(|h| h.avg_latency_ms);
                    let latency_b = b.last_health.as_ref().map(|h| h.avg_latency_ms);
                    match (latency_a, latency_b) {
                        (Some(la), Some(lb)) => la.cmp(&lb),
                        (Some(_), None) => std::cmp::Ordering::Less,
                        (None, Some(_)) => std::cmp::Ordering::Greater,
                        // Both have no latency data - prefer more recent heartbeat
                        (None, None) => b.last_heartbeat.cmp(&a.last_heartbeat),
                    }
                })
            }
            RoutingStrategy::HighestReputation => candidates
                .into_iter()
                .max_by(|a, b| compare_reputation(a, b)),
            RoutingStrategy::RoundRobin => {
                candidates.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));
                let idx = self.rr_counter.fetch_add(1, AtomicOrdering::Relaxed) % candidates.len();
                candidates.into_iter().nth(idx)
            }
            RoutingStrategy::WeightedRandom => {
                let weights: Vec<u32> = candidates
                    .iter()
                    .map(|agent| reputation_weight(agent))
                    .collect();
                let dist = WeightedIndex::new(&weights).ok();
                dist.and_then(|dist| {
                    let idx = dist.sample(&mut rand::thread_rng());
                    candidates.into_iter().nth(idx)
                })
            }
        }
    }

    /// Forward a SendMessage request to an external agent.
    pub async fn forward_request(
        &self,
        agent: &RegisteredAgent,
        request: SendMessageRequest,
    ) -> A2AResult<SendMessageResponse> {
        let url = format!("{}/message:send", agent.endpoint_url.trim_end_matches('/'));

        let mut last_err: Option<String> = None;
        let attempts = self.config.retry_count.saturating_add(1);

        for _ in 0..attempts {
            let response = self
                .http_client
                .post(&url)
                .header(HEADER_VERSION, PROTOCOL_VERSION)
                .timeout(Duration::from_millis(self.config.timeout_ms))
                .json(&request)
                .send()
                .await;

            match response {
                Ok(resp) => {
                    if !resp.status().is_success() {
                        last_err = Some(format!("status {}", resp.status()));
                        continue;
                    }

                    // Check content-length header if present
                    if let Some(content_length) = resp.content_length() {
                        if content_length as usize > MAX_RESPONSE_BODY_SIZE {
                            return Err(A2AError::InvalidAgentResponseError {
                                message: format!(
                                    "response too large: {} bytes exceeds {} limit",
                                    content_length, MAX_RESPONSE_BODY_SIZE
                                ),
                            });
                        }
                    }

                    // Read body with size limit
                    let bytes =
                        resp.bytes()
                            .await
                            .map_err(|e| A2AError::InvalidAgentResponseError {
                                message: format!("failed to read response body: {}", e),
                            })?;

                    if bytes.len() > MAX_RESPONSE_BODY_SIZE {
                        return Err(A2AError::InvalidAgentResponseError {
                            message: format!(
                                "response too large: {} bytes exceeds {} limit",
                                bytes.len(),
                                MAX_RESPONSE_BODY_SIZE
                            ),
                        });
                    }

                    let response: SendMessageResponse =
                        serde_json::from_slice(&bytes).map_err(|e| {
                            A2AError::InvalidAgentResponseError {
                                message: format!("invalid response from agent: {}", e),
                            }
                        })?;
                    return Ok(response);
                }
                Err(err) => {
                    last_err = Some(err.to_string());
                }
            }
        }

        Err(A2AError::InternalError {
            message: format!(
                "failed to forward request: {}",
                last_err.unwrap_or_else(|| "unknown error".to_string())
            ),
        })
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

fn reputation_weight(agent: &RegisteredAgent) -> u32 {
    agent
        .agent_card
        .tos_identity
        .as_ref()
        .and_then(|id| id.reputation_score_bps)
        .unwrap_or(0)
        .saturating_add(1)
}

pub fn extract_required_skills(message: &Message) -> Vec<String> {
    message
        .metadata
        .as_ref()
        .and_then(|m| m.get("required_skills").or_else(|| m.get("requiredSkills")))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a2a::registry::AgentRegistry;
    use tos_common::a2a::{AgentCapabilities, AgentInterface, AgentSkill};
    use tos_common::serializer::Serializer;

    fn sample_card(
        name: &str,
        skill_ids: &[&str],
        rep: Option<u32>,
        account_seed: u8,
    ) -> Result<AgentCard, Box<dyn std::error::Error>> {
        let agent_account = tos_common::crypto::PublicKey::from_bytes(&[account_seed; 32])?;
        let controller =
            tos_common::crypto::PublicKey::from_bytes(&[account_seed.wrapping_add(1); 32])?;
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
            skills: skill_ids
                .iter()
                .map(|id| AgentSkill {
                    id: (*id).to_string(),
                    name: "skill".to_string(),
                    description: "skill desc".to_string(),
                    tags: Vec::new(),
                    examples: Vec::new(),
                    input_modes: vec!["text/plain".to_string()],
                    output_modes: vec!["text/plain".to_string()],
                    security: Vec::new(),
                    tos_base_cost: None,
                })
                .collect(),
            supports_extended_agent_card: Some(false),
            signatures: Vec::new(),
            tos_identity: rep.map(|score| tos_common::a2a::TosAgentIdentity {
                agent_account,
                controller,
                reputation_score_bps: Some(score),
                identity_proof: None,
            }),
            arbitration: None,
        })
    }

    #[tokio::test]
    async fn router_selects_highest_reputation() -> Result<(), Box<dyn std::error::Error>> {
        let registry = Arc::new(AgentRegistry::new());
        let card_a = sample_card("agent-a", &["skill:a"], Some(100), 1)?;
        let card_b = sample_card("agent-b", &["skill:a"], Some(500), 3)?;

        let _ = registry
            .register(card_a, "https://a.test".to_string())
            .await?;
        let _ = registry
            .register(card_b, "https://b.test".to_string())
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
        let card_a = sample_card("agent-a", &["skill:a"], None, 5)?;
        let card_b = sample_card("agent-b", &["skill:a"], None, 7)?;

        let _ = registry
            .register(card_a, "https://a.test".to_string())
            .await?;
        let _ = registry
            .register(card_b, "https://b.test".to_string())
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

    #[tokio::test]
    async fn router_weighted_random_selects_candidate() -> Result<(), Box<dyn std::error::Error>> {
        let registry = Arc::new(AgentRegistry::new());
        let card_a = sample_card("agent-a", &["skill:a"], Some(0), 9)?;
        let card_b = sample_card("agent-b", &["skill:a"], Some(1000), 11)?;

        let _ = registry
            .register(card_a, "https://a.test".to_string())
            .await?;
        let _ = registry
            .register(card_b, "https://b.test".to_string())
            .await?;

        let router = AgentRouter::new(Arc::clone(&registry));
        let selected = router
            .route_request("skill:a", RoutingStrategy::WeightedRandom)
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        assert!(selected.name == "agent-a" || selected.name == "agent-b");
        Ok(())
    }

    #[tokio::test]
    async fn router_intersection_then_fallback_any() -> Result<(), Box<dyn std::error::Error>> {
        let registry = Arc::new(AgentRegistry::new());
        let card_a = sample_card("agent-a", &["skill:a", "skill:b"], None, 13)?;
        let card_b = sample_card("agent-b", &["skill:a"], None, 15)?;

        let _ = registry
            .register(card_a, "https://a.test".to_string())
            .await?;
        let _ = registry
            .register(card_b, "https://b.test".to_string())
            .await?;

        let router = AgentRouter::new(Arc::clone(&registry));
        let selected = router
            .route_agent_by_skills(
                &vec!["skill:a".to_string(), "skill:b".to_string()],
                RoutingStrategy::FirstMatch,
                true,
            )
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        assert_eq!(selected.agent_card.name, "agent-a");

        let fallback_selected = router
            .route_agent_by_skills(
                &vec!["skill:a".to_string(), "skill:missing".to_string()],
                RoutingStrategy::FirstMatch,
                true,
            )
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        assert!(
            fallback_selected.agent_card.name == "agent-a"
                || fallback_selected.agent_card.name == "agent-b"
        );
        Ok(())
    }
}
