use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use tos_common::arbitration::{ArbitrationOpen, JurorVote, VerdictBundle, VoteRequest};
use tos_common::crypto::Hash;

use super::{arbitration_root, ensure_dir, write_atomic, ArbitrationError};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinatorCase {
    pub request_id: Hash,
    pub open: ArbitrationOpen,
    pub vote_request: VoteRequest,
    pub votes: Vec<JurorVote>,
    pub verdict: Option<VerdictBundle>,
    #[serde(default)]
    pub verdict_submitted: bool,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JurorCase {
    pub request_id: Hash,
    pub vote_request_hash: Hash,
    pub vote: Option<JurorVote>,
    pub submitted: bool,
    pub updated_at: u64,
}

pub fn coordinator_case_path(request_id: &Hash) -> Result<PathBuf, ArbitrationError> {
    let root =
        arbitration_root().ok_or_else(|| ArbitrationError::Storage("no base dir".to_string()))?;
    let dir = root.join("coordinator");
    ensure_dir(&dir)?;
    Ok(dir.join(format!("{}.json", request_id.to_hex())))
}

pub fn juror_case_path(request_id: &Hash) -> Result<PathBuf, ArbitrationError> {
    let root =
        arbitration_root().ok_or_else(|| ArbitrationError::Storage("no base dir".to_string()))?;
    let dir = root.join("juror");
    ensure_dir(&dir)?;
    Ok(dir.join(format!("{}.json", request_id.to_hex())))
}

pub fn load_coordinator_case(
    request_id: &Hash,
) -> Result<Option<CoordinatorCase>, ArbitrationError> {
    let path = coordinator_case_path(request_id)?;
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path).map_err(|e| ArbitrationError::Storage(e.to_string()))?;
    let case =
        serde_json::from_slice(&bytes).map_err(|e| ArbitrationError::Storage(e.to_string()))?;
    Ok(Some(case))
}

pub fn save_coordinator_case(case: &CoordinatorCase) -> Result<(), ArbitrationError> {
    let path = coordinator_case_path(&case.request_id)?;
    let bytes =
        serde_json::to_vec_pretty(case).map_err(|e| ArbitrationError::Storage(e.to_string()))?;
    write_atomic(&path, &bytes)
}

pub fn load_juror_case(request_id: &Hash) -> Result<Option<JurorCase>, ArbitrationError> {
    let path = juror_case_path(request_id)?;
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(&path).map_err(|e| ArbitrationError::Storage(e.to_string()))?;
    let case =
        serde_json::from_slice(&bytes).map_err(|e| ArbitrationError::Storage(e.to_string()))?;
    Ok(Some(case))
}

pub fn save_juror_case(case: &JurorCase) -> Result<(), ArbitrationError> {
    let path = juror_case_path(&case.request_id)?;
    let bytes =
        serde_json::to_vec_pretty(case).map_err(|e| ArbitrationError::Storage(e.to_string()))?;
    write_atomic(&path, &bytes)
}
