// TOS Stratum Protocol Support
// Provides Stratum-compatible mining protocol for pool compatibility

use serde::{Deserialize, Serialize};
use tos_common::{
    block::BlockHeader,
    crypto::Hashable,
    difficulty::Difficulty,
};

/// Stratum mining job for mining pools
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StratumJob {
    /// Unique job ID
    pub job_id: String,

    /// Block header hash (for mining)
    pub header_hash: String,

    /// Previous block hash (hex)
    pub prev_hash: String,

    /// Coinbase transaction (hex)
    pub coinbase: String,

    /// Merkle branches for coinbase validation
    pub merkle_branches: Vec<String>,

    /// Block version
    pub version: u8,

    /// Network difficulty bits
    pub nbits: String,

    /// Network difficulty target
    pub target: String,

    /// Current block height
    pub height: u64,

    /// Current topological height
    pub topoheight: u64,

    /// Timestamp
    pub timestamp: u64,

    /// Clean jobs flag (true = abandon old work)
    pub clean_jobs: bool,
}

/// Stratum mining notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StratumNotification {
    /// Notification ID
    pub id: Option<u64>,

    /// Method name
    pub method: String,

    /// Job parameters
    pub params: StratumJob,
}

/// Stratum share submission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StratumShare {
    /// Worker name
    pub worker: String,

    /// Job ID
    pub job_id: String,

    /// Extra nonce 2
    pub extra_nonce2: String,

    /// Nonce time
    pub ntime: String,

    /// Nonce
    pub nonce: String,
}

/// Stratum share response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StratumShareResponse {
    /// Response ID
    pub id: u64,

    /// Result (null if rejected)
    pub result: Option<bool>,

    /// Error (null if accepted)
    pub error: Option<StratumError>,
}

/// Stratum error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StratumError {
    /// Error code
    pub code: i32,

    /// Error message
    pub message: String,

    /// Optional additional data
    pub data: Option<String>,
}

impl StratumError {
    /// Create a new Stratum error
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// Create error with additional data
    pub fn with_data(code: i32, message: impl Into<String>, data: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: Some(data.into()),
        }
    }
}

/// Common Stratum error codes
impl StratumError {
    /// Invalid job ID
    pub fn invalid_job_id() -> Self {
        Self::new(21, "Job not found")
    }

    /// Duplicate share
    pub fn duplicate_share() -> Self {
        Self::new(22, "Duplicate share")
    }

    /// Low difficulty share
    pub fn low_difficulty() -> Self {
        Self::new(23, "Low difficulty share")
    }

    /// Invalid solution
    pub fn invalid_solution() -> Self {
        Self::new(24, "Invalid solution")
    }

    /// Stale job
    pub fn stale_job() -> Self {
        Self::new(25, "Stale job")
    }
}

/// Convert TOS block header to Stratum job
pub fn block_header_to_stratum_job(
    header: &BlockHeader,
    job_id: String,
    height: u64,
    topoheight: u64,
    difficulty: &Difficulty,
    clean_jobs: bool,
) -> StratumJob {
    let header_hash = header.hash().to_string();

    // Get previous block hash (first parent)
    let prev_hash = header.get_parents()
        .first()
        .map(|h| h.to_string())
        .unwrap_or_else(|| "0".repeat(64));

    // Convert difficulty to target (simplified)
    let target = format!("{:064x}", difficulty.as_ref());

    // Network bits (simplified representation)
    let nbits = format!("{:08x}", difficulty.as_ref().bits());

    StratumJob {
        job_id,
        header_hash,
        prev_hash,
        coinbase: String::new(), // TOS doesn't use separate coinbase
        merkle_branches: Vec::new(), // TOS doesn't need merkle branches
        version: header.get_version() as u8,
        nbits,
        target,
        height,
        topoheight,
        timestamp: header.get_timestamp(),
        clean_jobs,
    }
}

/// Convert Stratum job to mining notification
pub fn create_stratum_notification(job: StratumJob, notification_id: Option<u64>) -> StratumNotification {
    StratumNotification {
        id: notification_id,
        method: "mining.notify".to_string(),
        params: job,
    }
}

/// Validate Stratum share submission
pub fn validate_stratum_share(share: &StratumShare) -> Result<(), StratumError> {
    // Validate worker name
    if share.worker.is_empty() || share.worker.len() > 32 {
        return Err(StratumError::new(20, "Invalid worker name"));
    }

    // Validate job ID
    if share.job_id.is_empty() {
        return Err(StratumError::invalid_job_id());
    }

    // Validate nonce format (hex string)
    if hex::decode(&share.nonce).is_err() {
        return Err(StratumError::new(20, "Invalid nonce format"));
    }

    // Validate extra nonce format
    if !share.extra_nonce2.is_empty() {
        // SECURITY FIX: Hard limit on input string length to prevent memory exhaustion DoS
        const MAX_EXTRA_NONCE2_HEX_LENGTH: usize = 128;
        if share.extra_nonce2.len() > MAX_EXTRA_NONCE2_HEX_LENGTH {
            return Err(StratumError::new(20, "Invalid extra nonce2: hex string too long"));
        }

        if hex::decode(&share.extra_nonce2).is_err() {
            return Err(StratumError::new(20, "Invalid extra nonce format"));
        }
    }

    Ok(())
}

/// Create success response for share submission
pub fn create_share_success(id: u64) -> StratumShareResponse {
    StratumShareResponse {
        id,
        result: Some(true),
        error: None,
    }
}

/// Create error response for share submission
pub fn create_share_error(id: u64, error: StratumError) -> StratumShareResponse {
    StratumShareResponse {
        id,
        result: None,
        error: Some(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tos_common::{
        crypto::{PublicKey, Hash},
        block::{EXTRA_NONCE_SIZE, BlockHeader, BlockVersion},
    };

    #[test]
    fn test_stratum_error_codes() {
        let err = StratumError::invalid_job_id();
        assert_eq!(err.code, 21);
        assert_eq!(err.message, "Job not found");

        let err = StratumError::duplicate_share();
        assert_eq!(err.code, 22);

        let err = StratumError::with_data(99, "Custom error", "Additional info");
        assert_eq!(err.code, 99);
        assert_eq!(err.data, Some("Additional info".to_string()));
    }

    #[test]
    fn test_block_header_to_stratum_job() {
        // Create a test public key - read from bytes using Serializer trait
        use tos_common::serializer::{Serializer, Reader};
        let mut reader = Reader::new(&[1u8; 32]);
        let address = PublicKey::read(&mut reader).expect("Failed to read public key");

        let header = BlockHeader::new_simple(
            BlockVersion::V1,
            vec![Hash::new([0u8; 32])],
            1234567890,
            [0u8; EXTRA_NONCE_SIZE],
            address,
            Hash::zero(),
        );

        let difficulty = Difficulty::from(1000u64);
        let job = block_header_to_stratum_job(
            &header,
            "job_001".to_string(),
            100,
            150,
            &difficulty,
            false,
        );

        assert_eq!(job.job_id, "job_001");
        assert_eq!(job.height, 100);
        assert_eq!(job.topoheight, 150);
        assert_eq!(job.version, 1);
        assert!(!job.clean_jobs);
    }

    #[test]
    fn test_validate_stratum_share() {
        let valid_share = StratumShare {
            worker: "worker1".to_string(),
            job_id: "job_001".to_string(),
            extra_nonce2: "".to_string(),
            ntime: "5f5e1234".to_string(),
            nonce: "deadbeef".to_string(),
        };

        assert!(validate_stratum_share(&valid_share).is_ok());

        let invalid_worker = StratumShare {
            worker: "".to_string(),
            job_id: "job_001".to_string(),
            extra_nonce2: "".to_string(),
            ntime: "5f5e1234".to_string(),
            nonce: "deadbeef".to_string(),
        };

        assert!(validate_stratum_share(&invalid_worker).is_err());

        let invalid_nonce = StratumShare {
            worker: "worker1".to_string(),
            job_id: "job_001".to_string(),
            extra_nonce2: "".to_string(),
            ntime: "5f5e1234".to_string(),
            nonce: "not_hex".to_string(),
        };

        assert!(validate_stratum_share(&invalid_nonce).is_err());
    }

    #[test]
    fn test_create_stratum_notification() {
        let job = StratumJob {
            job_id: "job_001".to_string(),
            header_hash: "abc123".to_string(),
            prev_hash: "def456".to_string(),
            coinbase: String::new(),
            merkle_branches: Vec::new(),
            version: 1,
            nbits: "1e0ffff0".to_string(),
            target: "0".repeat(64),
            height: 100,
            topoheight: 150,
            timestamp: 1234567890,
            clean_jobs: false,
        };

        let notification = create_stratum_notification(job.clone(), Some(1));

        assert_eq!(notification.id, Some(1));
        assert_eq!(notification.method, "mining.notify");
        assert_eq!(notification.params.job_id, "job_001");
    }

    #[test]
    fn test_share_responses() {
        let success = create_share_success(1);
        assert_eq!(success.id, 1);
        assert_eq!(success.result, Some(true));
        assert!(success.error.is_none());

        let error = create_share_error(2, StratumError::duplicate_share());
        assert_eq!(error.id, 2);
        assert!(error.result.is_none());
        assert!(error.error.is_some());
        assert_eq!(error.error.unwrap().code, 22);
    }
}
