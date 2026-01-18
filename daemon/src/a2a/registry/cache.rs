use std::collections::{HashMap, HashSet};

use tos_common::a2a::AgentCard;
use tos_common::crypto::{Hash, PublicKey};

use super::RegisteredAgent;

#[derive(Debug, Clone, Default)]
pub struct RegistryCache {
    pub agents: HashMap<Hash, RegisteredAgent>,
    pub index_by_skill: HashMap<String, HashSet<Hash>>,
    pub index_by_account: HashMap<PublicKey, Hash>,
}

impl RegistryCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clone_mut(&self) -> Self {
        self.clone()
    }

    pub fn insert_agent(&mut self, agent: RegisteredAgent) {
        let agent_id = agent.agent_id.clone();

        // Remove old entry first to clean up stale indexes if agent_id already exists
        self.remove_agent(&agent_id);

        for skill in extract_skill_ids(&agent.agent_card) {
            self.index_by_skill
                .entry(skill)
                .or_default()
                .insert(agent_id.clone());
        }

        if let Some(ref account) = agent.agent_account {
            self.index_by_account
                .insert(account.clone(), agent_id.clone());
        }

        self.agents.insert(agent_id, agent);
    }

    pub fn remove_agent(&mut self, agent_id: &Hash) -> Option<RegisteredAgent> {
        let agent = self.agents.remove(agent_id)?;

        for skill in extract_skill_ids(&agent.agent_card) {
            if let Some(agents) = self.index_by_skill.get_mut(&skill) {
                agents.remove(agent_id);
                if agents.is_empty() {
                    self.index_by_skill.remove(&skill);
                }
            }
        }

        if let Some(ref account) = agent.agent_account {
            self.index_by_account.remove(account);
        }

        Some(agent)
    }

    pub fn update_agent(&mut self, existing: &RegisteredAgent, updated: RegisteredAgent) {
        let _ = self.remove_agent(&existing.agent_id);
        self.insert_agent(updated);
    }
}

pub fn extract_skill_ids(card: &AgentCard) -> Vec<String> {
    card.skills.iter().map(|skill| skill.id.clone()).collect()
}
