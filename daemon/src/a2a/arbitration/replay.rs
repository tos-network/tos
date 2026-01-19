use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{arbitration_root, ensure_dir, write_atomic, ArbitrationError};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ReplayState {
    entries: HashMap<String, u64>,
}

pub struct ReplayCache {
    path: PathBuf,
    state: ReplayState,
}

impl ReplayCache {
    pub fn load(name: &str) -> Result<Self, ArbitrationError> {
        let Some(root) = arbitration_root() else {
            return Err(ArbitrationError::Storage("no base dir".to_string()));
        };
        let dir = root.join("replay");
        ensure_dir(&dir)?;
        let path = dir.join(format!("{name}.json"));
        let state = if path.exists() {
            let bytes =
                std::fs::read(&path).map_err(|e| ArbitrationError::Storage(e.to_string()))?;
            serde_json::from_slice(&bytes).unwrap_or_default()
        } else {
            ReplayState::default()
        };
        Ok(Self { path, state })
    }

    pub fn check_and_insert(
        &mut self,
        key: &str,
        expires_at: u64,
        now: u64,
    ) -> Result<bool, ArbitrationError> {
        self.state.entries.retain(|_, exp| *exp > now);
        if self.state.entries.contains_key(key) {
            return Ok(true);
        }
        self.state.entries.insert(key.to_string(), expires_at);
        self.flush()?;
        Ok(false)
    }

    fn flush(&self) -> Result<(), ArbitrationError> {
        let bytes = serde_json::to_vec_pretty(&self.state)
            .map_err(|e| ArbitrationError::Storage(e.to_string()))?;
        write_atomic(&self.path, &bytes)
    }
}
