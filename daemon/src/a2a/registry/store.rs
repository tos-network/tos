use std::{collections::HashSet, fs, path::Path, sync::Arc};

use rocksdb::{IteratorMode, Options, WriteBatch, DB};
use serde::de::DeserializeOwned;
use serde::Serialize;

use tos_common::crypto::{Hash, PublicKey};

use super::{RegisteredAgent, RegistryError};

const AGENT_PREFIX: &[u8] = b"agent:";
const SKILL_PREFIX: &[u8] = b"skill:";
const ACCOUNT_PREFIX: &[u8] = b"account:";

#[derive(Clone)]
pub(super) struct A2ARegistryStore {
    db: Arc<DB>,
}

impl A2ARegistryStore {
    pub fn open(path: &Path) -> Result<Self, RegistryError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| RegistryError::Storage(e.to_string()))?;
        } else {
            fs::create_dir_all(path).map_err(|e| RegistryError::Storage(e.to_string()))?;
        }

        let mut opts = Options::default();
        opts.create_if_missing(true);

        let db = DB::open(&opts, path).map_err(|e| RegistryError::Storage(e.to_string()))?;
        Ok(Self { db: Arc::new(db) })
    }

    pub fn save_agent(&self, agent: &RegisteredAgent) -> Result<(), RegistryError> {
        let key = agent_key(&agent.agent_id);
        let bytes = to_json(agent)?;
        self.db
            .put(key, bytes)
            .map_err(|e| RegistryError::Storage(e.to_string()))?;
        Ok(())
    }

    pub fn save_agent_with_indexes(
        &self,
        agent: &RegisteredAgent,
        skill_ids: &[String],
    ) -> Result<(), RegistryError> {
        let mut batch = WriteBatch::default();
        let key = agent_key(&agent.agent_id);
        let bytes = to_json(agent)?;
        batch.put(key, bytes);

        let id_hex = agent.agent_id.to_hex();
        for skill in skill_ids {
            let key = skill_key(skill);
            let mut agents = self.load_skill_agents(&key)?;
            if !agents.iter().any(|id| id == &id_hex) {
                agents.push(id_hex.clone());
                agents.sort();
            }
            let bytes = to_json(&agents)?;
            batch.put(key, bytes);
        }

        if let Some(account) = agent.agent_account.as_ref() {
            let key = account_key(account);
            let bytes = to_json(&id_hex)?;
            batch.put(key, bytes);
        }

        self.db
            .write(batch)
            .map_err(|e| RegistryError::Storage(e.to_string()))?;
        Ok(())
    }

    pub fn remove_agent(&self, agent_id: &Hash) -> Result<(), RegistryError> {
        let key = agent_key(agent_id);
        self.db
            .delete(key)
            .map_err(|e| RegistryError::Storage(e.to_string()))?;
        Ok(())
    }

    pub fn remove_agent_with_indexes(
        &self,
        agent: &RegisteredAgent,
        skill_ids: &[String],
    ) -> Result<(), RegistryError> {
        let mut batch = WriteBatch::default();
        let key = agent_key(&agent.agent_id);
        batch.delete(key);

        let id_hex = agent.agent_id.to_hex();
        for skill in skill_ids {
            let key = skill_key(skill);
            let mut agents = self.load_skill_agents(&key)?;
            let original_len = agents.len();
            agents.retain(|id| id != &id_hex);
            if agents.is_empty() {
                batch.delete(key);
            } else if agents.len() != original_len {
                let bytes = to_json(&agents)?;
                batch.put(key, bytes);
            }
        }

        if let Some(account) = agent.agent_account.as_ref() {
            let key = account_key(account);
            batch.delete(key);
        }

        self.db
            .write(batch)
            .map_err(|e| RegistryError::Storage(e.to_string()))?;
        Ok(())
    }

    pub fn update_agent_with_indexes(
        &self,
        existing: &RegisteredAgent,
        updated: &RegisteredAgent,
        existing_skills: &[String],
        updated_skills: &[String],
    ) -> Result<(), RegistryError> {
        let mut batch = WriteBatch::default();
        let key = agent_key(&updated.agent_id);
        let bytes = to_json(updated)?;
        batch.put(key, bytes);

        let existing_set: HashSet<String> = existing_skills.iter().cloned().collect();
        let updated_set: HashSet<String> = updated_skills.iter().cloned().collect();
        let mut union: HashSet<String> = existing_set.union(&updated_set).cloned().collect();

        let id_hex = updated.agent_id.to_hex();
        for skill in union.drain() {
            let key = skill_key(&skill);
            let mut agents = self.load_skill_agents(&key)?;
            let original_len = agents.len();

            if existing_set.contains(&skill) && !updated_set.contains(&skill) {
                agents.retain(|id| id != &id_hex);
            } else if updated_set.contains(&skill) && !agents.iter().any(|id| id == &id_hex) {
                agents.push(id_hex.clone());
                agents.sort();
            }

            if agents.is_empty() {
                batch.delete(key);
            } else if agents.len() != original_len {
                let bytes = to_json(&agents)?;
                batch.put(key, bytes);
            }
        }

        let existing_account = existing.agent_account.as_ref();
        let updated_account = updated.agent_account.as_ref();
        match (existing_account, updated_account) {
            (Some(old), Some(new)) if old != new => {
                batch.delete(account_key(old));
                let bytes = to_json(&id_hex)?;
                batch.put(account_key(new), bytes);
            }
            (Some(account), Some(_)) => {
                let bytes = to_json(&id_hex)?;
                batch.put(account_key(account), bytes);
            }
            (Some(account), None) => {
                batch.delete(account_key(account));
            }
            (None, Some(account)) => {
                let bytes = to_json(&id_hex)?;
                batch.put(account_key(account), bytes);
            }
            (None, None) => {}
        }

        self.db
            .write(batch)
            .map_err(|e| RegistryError::Storage(e.to_string()))?;
        Ok(())
    }

    pub fn load_agents(&self) -> Result<Vec<RegisteredAgent>, RegistryError> {
        let mut agents = Vec::new();
        let iter = self.db.iterator(IteratorMode::Start);
        for item in iter {
            let (key, value) = item.map_err(|e| RegistryError::Storage(e.to_string()))?;
            if !key.starts_with(AGENT_PREFIX) {
                continue;
            }
            let agent: RegisteredAgent = from_json(&value)?;
            agents.push(agent);
        }
        Ok(agents)
    }

    pub fn add_agent_to_skill(&self, agent_id: &Hash, skill_id: &str) -> Result<(), RegistryError> {
        let key = skill_key(skill_id);
        let mut agents = self.load_skill_agents(&key)?;
        let id_hex = agent_id.to_hex();
        if !agents.iter().any(|id| id == &id_hex) {
            agents.push(id_hex);
            agents.sort();
        }
        let bytes = to_json(&agents)?;
        self.db
            .put(key, bytes)
            .map_err(|e| RegistryError::Storage(e.to_string()))?;
        Ok(())
    }

    pub fn remove_agent_from_skill(
        &self,
        agent_id: &Hash,
        skill_id: &str,
    ) -> Result<(), RegistryError> {
        let key = skill_key(skill_id);
        let mut agents = self.load_skill_agents(&key)?;
        let id_hex = agent_id.to_hex();
        let original_len = agents.len();
        agents.retain(|id| id != &id_hex);
        if agents.is_empty() {
            self.db
                .delete(key)
                .map_err(|e| RegistryError::Storage(e.to_string()))?;
            return Ok(());
        }
        if agents.len() != original_len {
            let bytes = to_json(&agents)?;
            self.db
                .put(key, bytes)
                .map_err(|e| RegistryError::Storage(e.to_string()))?;
        }
        Ok(())
    }

    pub fn set_account_index(
        &self,
        account: &PublicKey,
        agent_id: &Hash,
    ) -> Result<(), RegistryError> {
        let key = account_key(account);
        let bytes = to_json(&agent_id.to_hex())?;
        self.db
            .put(key, bytes)
            .map_err(|e| RegistryError::Storage(e.to_string()))?;
        Ok(())
    }

    pub fn remove_account_index(&self, account: &PublicKey) -> Result<(), RegistryError> {
        let key = account_key(account);
        self.db
            .delete(key)
            .map_err(|e| RegistryError::Storage(e.to_string()))?;
        Ok(())
    }

    pub fn get_agent_id_by_account(
        &self,
        account: &PublicKey,
    ) -> Result<Option<Hash>, RegistryError> {
        let key = account_key(account);
        let Some(raw) = self
            .db
            .get(key)
            .map_err(|e| RegistryError::Storage(e.to_string()))?
        else {
            return Ok(None);
        };
        let id_hex: String = from_json(&raw)?;
        id_hex
            .parse::<Hash>()
            .map(Some)
            .map_err(|_| RegistryError::Storage("invalid agent id".to_string()))
    }

    fn load_skill_agents(&self, key: &[u8]) -> Result<Vec<String>, RegistryError> {
        let Some(raw) = self
            .db
            .get(key)
            .map_err(|e| RegistryError::Storage(e.to_string()))?
        else {
            return Ok(Vec::new());
        };
        from_json(&raw)
    }
}

fn to_json<T: Serialize>(value: &T) -> Result<Vec<u8>, RegistryError> {
    serde_json::to_vec(value).map_err(|e| RegistryError::Storage(e.to_string()))
}

fn from_json<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, RegistryError> {
    serde_json::from_slice(bytes).map_err(|e| RegistryError::Storage(e.to_string()))
}

fn agent_key(agent_id: &Hash) -> Vec<u8> {
    let mut key = Vec::with_capacity(AGENT_PREFIX.len() + 32);
    key.extend_from_slice(AGENT_PREFIX);
    key.extend_from_slice(agent_id.as_bytes());
    key
}

fn skill_key(skill_id: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(SKILL_PREFIX.len() + skill_id.len());
    key.extend_from_slice(SKILL_PREFIX);
    key.extend_from_slice(skill_id.as_bytes());
    key
}

fn account_key(account: &PublicKey) -> Vec<u8> {
    let mut key = Vec::with_capacity(ACCOUNT_PREFIX.len() + 32);
    key.extend_from_slice(ACCOUNT_PREFIX);
    key.extend_from_slice(account.as_bytes());
    key
}
