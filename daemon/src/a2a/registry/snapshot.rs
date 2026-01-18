use std::collections::HashSet;

use tos_common::crypto::{Hash, PublicKey};

use super::cache::RegistryCache;
use super::{RegistryError, RegistryStore};

#[derive(Debug)]
pub struct RegistrySnapshot {
    pub cache: RegistryCache,
    pub dirty_agents: HashSet<Hash>,
    pub deleted_agents: HashSet<Hash>,
    pub dirty_skills: HashSet<String>,
    pub dirty_accounts: HashSet<PublicKey>,
}

impl RegistrySnapshot {
    pub fn new(cache: RegistryCache) -> Self {
        Self {
            cache,
            dirty_agents: HashSet::new(),
            deleted_agents: HashSet::new(),
            dirty_skills: HashSet::new(),
            dirty_accounts: HashSet::new(),
        }
    }

    pub fn has_pending_writes(&self) -> bool {
        !self.dirty_agents.is_empty()
            || !self.deleted_agents.is_empty()
            || !self.dirty_skills.is_empty()
            || !self.dirty_accounts.is_empty()
    }
}

pub struct SnapshotGuard<'a> {
    store: &'a mut RegistryStore,
    committed: bool,
    commit_attempted: bool,
}

impl<'a> SnapshotGuard<'a> {
    pub fn new(store: &'a mut RegistryStore) -> Result<Self, RegistryError> {
        store.start_snapshot()?;
        Ok(Self {
            store,
            committed: false,
            commit_attempted: false,
        })
    }

    pub fn commit(&mut self) -> Result<(), RegistryError> {
        self.commit_attempted = true;
        match self.store.end_snapshot(true) {
            Ok(()) => {
                self.committed = true;
                Ok(())
            }
            Err(err) => {
                // Preserve snapshot for retry as per design spec
                // Drop will clean up if guard is dropped without successful commit
                Err(err)
            }
        }
    }

    pub fn store_mut(&mut self) -> &mut RegistryStore {
        self.store
    }

    pub fn rollback(&mut self) -> Result<(), RegistryError> {
        if self.committed {
            return Ok(());
        }
        self.store.end_snapshot(false)?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for SnapshotGuard<'_> {
    fn drop(&mut self) {
        // Always rollback if snapshot is still active and not successfully committed
        // This ensures we don't leave the registry wedged after a failed commit
        if !self.committed && self.store.has_snapshot() {
            let _ = self.store.end_snapshot(false);
        }
    }
}
