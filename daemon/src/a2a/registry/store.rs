use std::fs;
use std::path::Path;
use std::sync::Arc;

use rocksdb::{IteratorMode, Options, WriteBatch, DB};
use serde::de::DeserializeOwned;
use serde::Serialize;

use tos_common::crypto::{Hash, PublicKey};

use super::cache::{extract_skill_ids, RegistryCache};
use super::snapshot::RegistrySnapshot;
use super::{RegisteredAgent, RegistryError};

const AGENT_PREFIX: &[u8] = b"agent:";
const SKILL_PREFIX: &[u8] = b"skill:";
const ACCOUNT_PREFIX: &[u8] = b"account:";

pub struct RegistryStore {
    db: Option<Arc<DB>>,
    cache: RegistryCache,
    snapshot: Option<RegistrySnapshot>,
    #[cfg(test)]
    fail_commit: bool,
}

impl RegistryStore {
    pub fn open(path: &Path) -> Result<Self, RegistryError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| RegistryError::Storage(e.to_string()))?;
        } else {
            fs::create_dir_all(path).map_err(|e| RegistryError::Storage(e.to_string()))?;
        }

        let mut opts = Options::default();
        opts.create_if_missing(true);

        let db = DB::open(&opts, path).map_err(|e| RegistryError::Storage(e.to_string()))?;
        let db = Arc::new(db);
        let cache = Self::load_cache_from_rocksdb(&db)?;

        Ok(Self {
            db: Some(db),
            cache,
            snapshot: None,
            #[cfg(test)]
            fail_commit: false,
        })
    }

    pub fn in_memory() -> Self {
        Self {
            db: None,
            cache: RegistryCache::new(),
            snapshot: None,
            #[cfg(test)]
            fail_commit: false,
        }
    }

    pub fn cache(&self) -> &RegistryCache {
        self.snapshot
            .as_ref()
            .map(|snapshot| &snapshot.cache)
            .unwrap_or(&self.cache)
    }

    fn cache_mut(&mut self) -> &mut RegistryCache {
        self.snapshot
            .as_mut()
            .map(|snapshot| &mut snapshot.cache)
            .unwrap_or(&mut self.cache)
    }

    pub fn has_snapshot(&self) -> bool {
        self.snapshot.is_some()
    }

    pub fn start_snapshot(&mut self) -> Result<(), RegistryError> {
        if self.snapshot.is_some() {
            return Err(RegistryError::SnapshotAlreadyActive);
        }
        self.snapshot = Some(RegistrySnapshot::new(self.cache.clone_mut()));
        Ok(())
    }

    pub fn end_snapshot(&mut self, apply: bool) -> Result<(), RegistryError> {
        if self.snapshot.is_none() {
            return Err(RegistryError::SnapshotNotActive);
        }

        if apply {
            self.apply_snapshot_to_disk()?;
            // Safe: we already verified snapshot.is_some() at function start
            if let Some(snapshot) = self.snapshot.take() {
                self.cache = snapshot.cache;
            }
        } else {
            self.snapshot.take();
        }

        Ok(())
    }

    pub fn insert_agent(&mut self, agent: RegisteredAgent) -> Result<(), RegistryError> {
        let snapshot = self
            .snapshot
            .as_mut()
            .ok_or(RegistryError::SnapshotNotActive)?;

        snapshot.deleted_agents.remove(&agent.agent_id);
        snapshot.dirty_agents.insert(agent.agent_id.clone());
        mark_dirty_sets(snapshot, None, Some(&agent));

        self.cache_mut().insert_agent(agent);
        Ok(())
    }

    pub fn update_agent(
        &mut self,
        existing: &RegisteredAgent,
        updated: RegisteredAgent,
    ) -> Result<(), RegistryError> {
        let snapshot = self
            .snapshot
            .as_mut()
            .ok_or(RegistryError::SnapshotNotActive)?;

        snapshot.deleted_agents.remove(&existing.agent_id);
        snapshot.dirty_agents.insert(existing.agent_id.clone());
        mark_dirty_sets(snapshot, Some(existing), Some(&updated));

        self.cache_mut().update_agent(existing, updated);
        Ok(())
    }

    pub fn remove_agent(
        &mut self,
        agent_id: &Hash,
    ) -> Result<Option<RegisteredAgent>, RegistryError> {
        // Check snapshot exists before any mutation
        if self.snapshot.is_none() {
            return Err(RegistryError::SnapshotNotActive);
        }

        let removed = self.cache_mut().remove_agent(agent_id);

        // Safe: we already verified snapshot.is_some() at function start
        if let (Some(snapshot), Some(ref agent)) = (self.snapshot.as_mut(), &removed) {
            snapshot.dirty_agents.remove(agent_id);
            snapshot.deleted_agents.insert(agent_id.clone());
            mark_dirty_sets(snapshot, Some(agent), None);
        }

        Ok(removed)
    }

    pub fn is_empty(&self) -> bool {
        self.cache.agents.is_empty()
    }

    fn apply_snapshot_to_disk(&mut self) -> Result<(), RegistryError> {
        let snapshot = self
            .snapshot
            .as_mut()
            .ok_or(RegistryError::SnapshotNotActive)?;

        #[cfg(test)]
        if self.fail_commit {
            return Err(RegistryError::Storage("forced commit failure".to_string()));
        }

        let Some(db) = self.db.as_ref() else {
            return Ok(());
        };

        if snapshot.has_pending_writes() {
            let batch = rebuild_snapshot_batch(snapshot)?;
            if !batch.is_empty() {
                db.write(batch)
                    .map_err(|e| RegistryError::Storage(e.to_string()))?;
            }
        }

        Ok(())
    }

    fn load_cache_from_rocksdb(db: &Arc<DB>) -> Result<RegistryCache, RegistryError> {
        let mut cache = RegistryCache::new();
        let iter = db.iterator(IteratorMode::Start);
        for item in iter {
            let (key, value) = item.map_err(|e| RegistryError::Storage(e.to_string()))?;
            if !key.starts_with(AGENT_PREFIX) {
                continue;
            }
            let agent: RegisteredAgent = from_json(&value)?;
            cache.insert_agent(agent);
        }
        Ok(cache)
    }

    #[cfg(test)]
    pub fn set_fail_commit(&mut self, fail: bool) {
        self.fail_commit = fail;
    }
}

fn mark_dirty_sets(
    snapshot: &mut RegistrySnapshot,
    existing: Option<&RegisteredAgent>,
    updated: Option<&RegisteredAgent>,
) {
    if let Some(agent) = existing {
        for skill in extract_skill_ids(&agent.agent_card) {
            snapshot.dirty_skills.insert(skill);
        }
        if let Some(ref account) = agent.agent_account {
            snapshot.dirty_accounts.insert(account.clone());
        }
    }

    if let Some(agent) = updated {
        for skill in extract_skill_ids(&agent.agent_card) {
            snapshot.dirty_skills.insert(skill);
        }
        if let Some(ref account) = agent.agent_account {
            snapshot.dirty_accounts.insert(account.clone());
        }
    }
}

fn rebuild_snapshot_batch(snapshot: &RegistrySnapshot) -> Result<WriteBatch, RegistryError> {
    let mut batch = WriteBatch::default();

    for agent_id in snapshot.dirty_agents.iter() {
        if let Some(agent) = snapshot.cache.agents.get(agent_id) {
            let key = agent_key(&agent.agent_id);
            let bytes = to_json(agent)?;
            batch.put(key, bytes);
        }
    }

    for agent_id in snapshot.deleted_agents.iter() {
        let key = agent_key(agent_id);
        batch.delete(key);
    }

    for skill in snapshot.dirty_skills.iter() {
        let key = skill_key(skill);
        if let Some(agent_ids) = snapshot.cache.index_by_skill.get(skill) {
            if agent_ids.is_empty() {
                batch.delete(key);
            } else {
                let mut ids: Vec<String> = agent_ids.iter().map(|id| id.to_hex()).collect();
                ids.sort();
                let bytes = to_json(&ids)?;
                batch.put(key, bytes);
            }
        } else {
            batch.delete(key);
        }
    }

    for account in snapshot.dirty_accounts.iter() {
        let key = account_key(account);
        if let Some(agent_id) = snapshot.cache.index_by_account.get(account) {
            let bytes = to_json(&agent_id.to_hex())?;
            batch.put(key, bytes);
        } else {
            batch.delete(key);
        }
    }

    Ok(batch)
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
