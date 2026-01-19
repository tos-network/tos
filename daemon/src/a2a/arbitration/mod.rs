use std::path::{Path, PathBuf};

use once_cell::sync::OnceCell;
use thiserror::Error;

pub mod audit;
pub mod coordinator;
pub mod evidence;
pub mod juror;
pub mod keys;
pub mod persistence;
pub mod replay;

pub const MAX_EVIDENCE_TOTAL_BYTES: usize = 50 * 1024 * 1024;
pub const MAX_EVIDENCE_FILE_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_EVIDENCE_FILES: usize = 100;
pub const MAX_EVIDENCE_FETCH_SECS: u64 = 30;
pub const MAX_MANIFEST_BYTES: usize = 1 * 1024 * 1024;
pub const MAX_MANIFEST_ENTRIES: usize = 1000;
pub const MAX_MANIFEST_PATH_BYTES: usize = 256;
pub const MAX_MANIFEST_MIME_BYTES: usize = 128;

pub const MAX_VOTE_REQUEST_BYTES: usize = 64 * 1024;
pub const MAX_VERDICT_BUNDLE_BYTES: usize = 128 * 1024;
pub const MAX_JUROR_VOTE_BYTES: usize = 8 * 1024;
pub const MAX_JUROR_COUNT: usize = 256;

pub const MAX_CLOCK_DRIFT_SECS: u64 = 300;
pub const COORDINATOR_GRACE_PERIOD: u64 = 3600;

static BASE_DIR: OnceCell<PathBuf> = OnceCell::new();

#[derive(Debug, Error)]
pub enum ArbitrationError {
    #[error("invalid message: {0}")]
    InvalidMessage(String),
    #[error("invalid signature")]
    InvalidSignature,
    #[error("expired message")]
    Expired,
    #[error("replay detected")]
    Replay,
    #[error("committee not found")]
    CommitteeNotFound,
    #[error("committee inactive")]
    CommitteeInactive,
    #[error("insufficient jurors (required {required}, available {available})")]
    InsufficientJurors { required: usize, available: usize },
    #[error("not selected juror")]
    NotSelectedJuror,
    #[error("storage error: {0}")]
    Storage(String),
    #[error("evidence error: {0}")]
    Evidence(String),
    #[error("coordinator key not configured")]
    CoordinatorKeyMissing,
    #[error("coordinator key mismatch")]
    CoordinatorKeyMismatch,
    #[error("transaction error: {0}")]
    Transaction(String),
    #[error("quorum not met")]
    QuorumNotMet,
}

pub fn set_base_dir(dir: &str) {
    let _ = BASE_DIR.set(PathBuf::from(dir));
}

pub fn arbitration_root() -> Option<PathBuf> {
    let base = BASE_DIR.get_or_init(|| PathBuf::from(""));
    if base.as_os_str().is_empty() {
        return None;
    }
    let mut path = base.clone();
    path.push("a2a");
    path.push("arbitration");
    Some(path)
}

pub fn ensure_dir(path: &Path) -> Result<(), ArbitrationError> {
    std::fs::create_dir_all(path).map_err(|e| ArbitrationError::Storage(e.to_string()))
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), ArbitrationError> {
    let mut tmp = path.to_path_buf();
    tmp.set_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| ArbitrationError::Storage(e.to_string()))?;
    std::fs::rename(&tmp, path).map_err(|e| ArbitrationError::Storage(e.to_string()))
}
