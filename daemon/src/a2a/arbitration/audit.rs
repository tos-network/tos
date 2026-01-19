use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use tos_common::crypto::Hash;

use super::{arbitration_root, ensure_dir, ArbitrationError};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuditEntry {
    ts: u64,
    event: String,
    request_id: Hash,
    payload_hash: Hash,
    prev_hash: Hash,
    entry_hash: Hash,
}

pub fn append_event(
    event: &str,
    request_id: &Hash,
    payload_hash: &Hash,
    ts: u64,
) -> Result<(), ArbitrationError> {
    let path = audit_log_path()?;
    let prev_hash = last_entry_hash(&path)?;

    let entry_hash = compute_entry_hash(&prev_hash, event, request_id, payload_hash, ts);
    let entry = AuditEntry {
        ts,
        event: event.to_string(),
        request_id: request_id.clone(),
        payload_hash: payload_hash.clone(),
        prev_hash,
        entry_hash,
    };
    let line =
        serde_json::to_string(&entry).map_err(|e| ArbitrationError::Storage(e.to_string()))?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| ArbitrationError::Storage(e.to_string()))?;
    writeln!(file, "{line}").map_err(|e| ArbitrationError::Storage(e.to_string()))?;
    Ok(())
}

fn audit_log_path() -> Result<PathBuf, ArbitrationError> {
    let root =
        arbitration_root().ok_or_else(|| ArbitrationError::Storage("no base dir".to_string()))?;
    ensure_dir(&root)?;
    Ok(root.join("audit.log"))
}

fn last_entry_hash(path: &PathBuf) -> Result<Hash, ArbitrationError> {
    if !path.exists() {
        return Ok(Hash::zero());
    }
    let file = File::open(path).map_err(|e| ArbitrationError::Storage(e.to_string()))?;
    let reader = BufReader::new(file);
    let mut last = None;
    for line in reader.lines() {
        if let Ok(line) = line {
            last = Some(line);
        }
    }
    if let Some(line) = last {
        let entry: AuditEntry =
            serde_json::from_str(&line).map_err(|e| ArbitrationError::Storage(e.to_string()))?;
        Ok(entry.entry_hash)
    } else {
        Ok(Hash::zero())
    }
}

fn compute_entry_hash(
    prev: &Hash,
    event: &str,
    request_id: &Hash,
    payload_hash: &Hash,
    ts: u64,
) -> Hash {
    use sha3::{Digest, Sha3_256};
    let mut hasher = Sha3_256::new();
    hasher.update(prev.as_bytes());
    hasher.update(event.as_bytes());
    hasher.update(request_id.as_bytes());
    hasher.update(payload_hash.as_bytes());
    hasher.update(&ts.to_le_bytes());
    Hash::new(hasher.finalize().into())
}
