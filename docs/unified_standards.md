# AI Mining System Unified Standards and Specifications

## 1. Data Structure Unified Standards

### 1.1 Core Data Types

#### Identifier Specifications
```rust
// Unified use of UUID v4 format strings as IDs
pub type TaskId = String;          // "550e8400-e29b-41d4-a716-446655440000"
pub type MinerId = String;         // "550e8400-e29b-41d4-a716-446655440001"
pub type SubmissionId = String;    // "550e8400-e29b-41d4-a716-446655440002"
pub type ValidationId = String;    // "550e8400-e29b-41d4-a716-446655440003"

// Addresses unified using TOS address format
pub type Address = String;         // "tos1abc123def456..."

// Hashes unified using 32-byte arrays
pub type Hash = [u8; 32];
```

#### Timestamp Standards
```rust
// Unified use of Unix timestamps (u64 seconds)
pub type Timestamp = u64;

// Time-related utility functions
pub fn current_timestamp() -> Timestamp {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

pub fn timestamp_to_datetime(timestamp: Timestamp) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(timestamp as i64, 0)
        .unwrap_or_else(|| chrono::Utc::now())
}
```

#### Currency Amount Standards
```rust
// Unified use of u128 to represent TOS quantities (in Wei units, 1 TOS = 10^18 Wei)
pub type TOSAmount = u128;

pub const TOS_DECIMALS: u32 = 18;
pub const TOS_WEI: u128 = 10u128.pow(TOS_DECIMALS);

// Utility functions
pub fn tos_to_wei(tos: f64) -> TOSAmount {
    (tos * TOS_WEI as f64) as u128
}

pub fn wei_to_tos(wei: TOSAmount) -> f64 {
    wei as f64 / TOS_WEI as f64
}
```

### 1.2 Unified Reputation Calculation Formula

```rust
/// Unified Reputation Calculation System
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedReputationCalculator {
    pub base_weights: ReputationWeights,
    pub decay_config: DecayConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationWeights {
    pub task_completion: f64,      // 0.4
    pub quality_score: f64,        // 0.3
    pub validation_accuracy: f64,  // 0.15
    pub peer_rating: f64,          // 0.1
    pub fraud_penalty: f64,        // -1.0 (multiplier)
    pub innovation_bonus: f64,     // 0.05
}

impl Default for ReputationWeights {
    fn default() -> Self {
        Self {
            task_completion: 0.4,
            quality_score: 0.3,
            validation_accuracy: 0.15,
            peer_rating: 0.1,
            fraud_penalty: -1.0,
            innovation_bonus: 0.05,
        }
    }
}

impl UnifiedReputationCalculator {
    /// Unified reputation calculation formula
    /// reputation_change = Σ(weight_i × score_i) × fraud_multiplier × innovation_multiplier
    pub fn calculate_reputation_change(
        &self,
        current_reputation: u32,
        performance_data: &PerformanceData,
    ) -> i32 {
        let weights = &self.base_weights;

        // Base score calculation
        let base_score =
            weights.task_completion * performance_data.task_completion_rate +
            weights.quality_score * performance_data.average_quality_score +
            weights.validation_accuracy * performance_data.validation_accuracy +
            weights.peer_rating * performance_data.peer_rating_score;

        // Fraud penalty multiplier
        let fraud_multiplier = if performance_data.fraud_detected {
            weights.fraud_penalty
        } else {
            1.0
        };

        // Innovation reward multiplier
        let innovation_multiplier = 1.0 + (weights.innovation_bonus * performance_data.innovation_score);

        // Reputation decay factor (prevent infinite growth)
        let decay_factor = self.calculate_decay_factor(current_reputation);

        // Final calculation
        let raw_change = base_score * fraud_multiplier * innovation_multiplier * decay_factor;

        // Convert to integer and apply bounds
        (raw_change * 100.0).round() as i32
    }

    fn calculate_decay_factor(&self, current_reputation: u32) -> f64 {
        let max_reputation = 10000.0;
        let decay_rate = self.decay_config.decay_rate;

        // Linear decay: higher reputation, slower growth
        1.0 - (current_reputation as f64 / max_reputation) * decay_rate
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceData {
    pub task_completion_rate: f64,    // 0.0-1.0
    pub average_quality_score: f64,   // 0.0-1.0
    pub validation_accuracy: f64,     // 0.0-1.0
    pub peer_rating_score: f64,       // 0.0-1.0
    pub fraud_detected: bool,
    pub innovation_score: f64,        // 0.0-1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecayConfig {
    pub decay_rate: f64,      // 0.0-1.0, default 0.1
    pub min_decay: f64,       // Minimum decay factor, default 0.5
}

impl Default for DecayConfig {
    fn default() -> Self {
        Self {
            decay_rate: 0.1,
            min_decay: 0.5,
        }
    }
}
```

## 2. API Interface Unified Standards

### 2.1 HTTP Response Format Standards

```rust
/// Unified API Response Format
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<ApiError>,
    pub timestamp: Timestamp,
    pub request_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    pub details: Option<serde_json::Value>,
}

// Success response constructor
impl<T> ApiResponse<T> {
    pub fn success(data: T, request_id: String) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            timestamp: current_timestamp(),
            request_id,
        }
    }

    pub fn error(error: ApiError, request_id: String) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(error),
            timestamp: current_timestamp(),
            request_id,
        }
    }
}

/// Paginated response format
#[derive(Debug, Serialize, Deserialize)]
pub struct PaginatedResponse<T> {
    pub items: Vec<T>,
    pub pagination: PaginationInfo,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PaginationInfo {
    pub current_page: u32,
    pub total_pages: u32,
    pub page_size: u32,
    pub total_items: u64,
    pub has_next: bool,
    pub has_prev: bool,
}
```

### 2.2 Error Code Standards

```rust
/// Unified error code definitions
pub mod error_codes {
    // 1000-1999: General errors
    pub const INVALID_REQUEST: &str = "1000";
    pub const UNAUTHORIZED: &str = "1001";
    pub const FORBIDDEN: &str = "1002";
    pub const NOT_FOUND: &str = "1004";
    pub const INTERNAL_ERROR: &str = "1500";

    // 2000-2999: Task-related errors
    pub const TASK_NOT_FOUND: &str = "2000";
    pub const TASK_EXPIRED: &str = "2001";
    pub const TASK_FULL: &str = "2002";
    pub const INVALID_TASK_TYPE: &str = "2003";
    pub const INSUFFICIENT_REWARD: &str = "2004";

    // 3000-3999: Miner-related errors
    pub const MINER_NOT_REGISTERED: &str = "3000";
    pub const MINER_ALREADY_EXISTS: &str = "3001";
    pub const INSUFFICIENT_STAKE: &str = "3002";
    pub const LOW_REPUTATION: &str = "3003";
    pub const MINER_SUSPENDED: &str = "3004";

    // 4000-4999: Validation-related errors
    pub const VALIDATION_FAILED: &str = "4000";
    pub const VALIDATOR_NOT_QUALIFIED: &str = "4001";
    pub const VALIDATION_TIMEOUT: &str = "4002";
    pub const CONSENSUS_NOT_REACHED: &str = "4003";

    // 5000-5999: Network-related errors
    pub const NETWORK_ERROR: &str = "5000";
    pub const SYNC_FAILED: &str = "5001";
    pub const PEER_UNREACHABLE: &str = "5002";
}

/// Error response utility functions
pub fn create_error_response(code: &str, message: &str, request_id: String) -> ApiResponse<()> {
    ApiResponse::error(
        ApiError {
            code: code.to_string(),
            message: message.to_string(),
            details: None,
        },
        request_id,
    )
}
```

### 2.3 HTTP Status Code Mapping Standards

```rust
pub fn error_code_to_http_status(error_code: &str) -> u16 {
    match error_code {
        // 400 Bad Request
        "1000" | "2003" | "2004" => 400,

        // 401 Unauthorized
        "1001" => 401,

        // 403 Forbidden
        "1002" | "3004" => 403,

        // 404 Not Found
        "1004" | "2000" | "3000" => 404,

        // 409 Conflict
        "2002" | "3001" => 409,

        // 422 Unprocessable Entity
        "2001" | "3002" | "3003" | "4000" | "4001" => 422,

        // 408 Request Timeout
        "4002" => 408,

        // 424 Failed Dependency
        "4003" => 424,

        // 502 Bad Gateway
        "5000" | "5002" => 502,

        // 503 Service Unavailable
        "5001" => 503,

        // 500 Internal Server Error
        _ => 500,
    }
}
```

## 3. Configuration Parameter Unified Standards

### 3.1 System Configuration Structure

```toml
# unified_config.toml - Unified configuration file format

[network]
listen_addr = "0.0.0.0:8000"
max_peers = 100                    # Unified to 100
connection_timeout = 30            # 30 seconds
sync_interval = 15                 # 15 second network sync interval

[ai_mining]
enabled = true
max_concurrent_tasks = 1000
task_timeout_hours = 24            # Unified to 24 hours
validation_timeout_hours = 48      # Unified to 48 hours
min_stake_amount = 100             # 100 TOS Wei

[validation]
consensus_threshold = 0.75         # Unified consensus threshold
fraud_detection_threshold = 0.8    # Unified fraud detection threshold
automatic_validation_enabled = true
peer_validation_required = true
expert_validation_threshold = 3    # Threshold requiring 3 expert validations

[rewards]
network_fee_percentage = 0.05      # 5% network fee
winner_share = 0.65               # 65% to winner
participant_share = 0.15          # 15% to participants
validator_share = 0.15            # 15% to validators

[reputation]
max_reputation = 10000
decay_rate = 0.1
min_decay_factor = 0.5
task_completion_weight = 0.4
quality_score_weight = 0.3
validation_accuracy_weight = 0.15
peer_rating_weight = 0.1
innovation_bonus_weight = 0.05

[storage]
data_dir = "~/.tos/ai"
cache_size_mb = 1024              # 1GB cache
max_log_size_mb = 100
log_rotation_count = 10

[retry]
max_retries = 5                   # Unified retry count
base_delay_ms = 1000             # Base delay 1 second
max_delay_ms = 30000             # Max delay 30 seconds
exponential_backoff = true

[performance]
batch_size = 100
worker_threads = 0               # 0 means use CPU core count
task_queue_size = 10000
validation_queue_size = 5000
```

### 3.2 Configuration Loader

```rust
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UnifiedConfig {
    pub network: NetworkConfig,
    pub ai_mining: AIMiningConfig,
    pub validation: ValidationConfig,
    pub rewards: RewardsConfig,
    pub reputation: ReputationConfig,
    pub storage: StorageConfig,
    pub retry: RetryConfig,
    pub performance: PerformanceConfig,
}

impl UnifiedConfig {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::FileError(e.to_string()))?;

        toml::from_str(&content)
            .map_err(|e| ConfigError::ParseError(e.to_string()))
    }

    pub fn load_with_defaults() -> Self {
        Self {
            network: NetworkConfig::default(),
            ai_mining: AIMiningConfig::default(),
            validation: ValidationConfig::default(),
            rewards: RewardsConfig::default(),
            reputation: ReputationConfig::default(),
            storage: StorageConfig::default(),
            retry: RetryConfig::default(),
            performance: PerformanceConfig::default(),
        }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        // Validate configuration reasonableness
        if self.validation.consensus_threshold < 0.5 || self.validation.consensus_threshold > 1.0 {
            return Err(ConfigError::InvalidValue("consensus_threshold must be between 0.5 and 1.0".to_string()));
        }

        if self.rewards.network_fee_percentage + self.rewards.winner_share +
           self.rewards.participant_share + self.rewards.validator_share != 1.0 {
            return Err(ConfigError::InvalidValue("reward shares must sum to 1.0".to_string()));
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ValidationConfig {
    pub consensus_threshold: f64,
    pub fraud_detection_threshold: f64,
    pub automatic_validation_enabled: bool,
    pub peer_validation_required: bool,
    pub expert_validation_threshold: u32,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            consensus_threshold: 0.75,
            fraud_detection_threshold: 0.8,
            automatic_validation_enabled: true,
            peer_validation_required: true,
            expert_validation_threshold: 3,
        }
    }
}

#[derive(Debug)]
pub enum ConfigError {
    FileError(String),
    ParseError(String),
    InvalidValue(String),
}
```

## 4. Database Architecture Unified Standards

### 4.1 RocksDB Column Family Definitions

```rust
/// Unified RocksDB column family definitions
pub mod column_families {
    pub const TASKS: &str = "tasks";                    // Task data
    pub const MINERS: &str = "miners";                  // Miner data
    pub const SUBMISSIONS: &str = "submissions";        // Submission data
    pub const VALIDATIONS: &str = "validations";        // Validation data
    pub const REWARDS: &str = "rewards";                // Reward records
    pub const REPUTATION: &str = "reputation";          // Reputation data
    pub const NETWORK_STATE: &str = "network_state";    // Network state
    pub const FRAUD_RECORDS: &str = "fraud_records";    // Fraud records
    pub const METRICS: &str = "metrics";                // Performance metrics
    pub const CONFIG: &str = "config";                  // Configuration data
}

/// Key-value pair format standards
pub mod key_formats {
    use super::*;

    // Task-related key formats
    pub fn task_key(task_id: &TaskId) -> String {
        format!("task:{}", task_id)
    }

    pub fn task_submissions_key(task_id: &TaskId) -> String {
        format!("task:{}:submissions", task_id)
    }

    pub fn task_validations_key(task_id: &TaskId) -> String {
        format!("task:{}:validations", task_id)
    }

    // Miner-related key formats
    pub fn miner_key(miner_id: &MinerId) -> String {
        format!("miner:{}", miner_id)
    }

    pub fn miner_reputation_key(miner_id: &MinerId) -> String {
        format!("miner:{}:reputation", miner_id)
    }

    pub fn miner_performance_key(miner_id: &MinerId) -> String {
        format!("miner:{}:performance", miner_id)
    }

    // Time series key formats
    pub fn time_series_key(metric_name: &str, timestamp: Timestamp) -> String {
        format!("metric:{}:{:016x}", metric_name, timestamp)
    }
}

/// Database initialization configuration
pub fn init_rocksdb_with_column_families(path: &Path) -> Result<rocksdb::DB, rocksdb::Error> {
    use rocksdb::{DB, Options, ColumnFamilyDescriptor, SliceTransform};

    let mut db_opts = Options::default();
    db_opts.create_if_missing(true);
    db_opts.create_missing_column_families(true);

    // Set different optimization options for different column families
    let cfs = vec![
        // Task column family - mainly read operations
        ColumnFamilyDescriptor::new(
            column_families::TASKS,
            Options::default()
        ),

        // Miner column family - frequent updates
        {
            let mut opts = Options::default();
            opts.set_write_buffer_size(64 * 1024 * 1024); // 64MB
            ColumnFamilyDescriptor::new(column_families::MINERS, opts)
        },

        // Submission column family - heavy write operations
        {
            let mut opts = Options::default();
            opts.set_max_write_buffer_number(4);
            opts.set_write_buffer_size(32 * 1024 * 1024); // 32MB
            ColumnFamilyDescriptor::new(column_families::SUBMISSIONS, opts)
        },

        // Validation column family
        ColumnFamilyDescriptor::new(
            column_families::VALIDATIONS,
            Options::default()
        ),

        // Reward column family - write once, read often
        {
            let mut opts = Options::default();
            opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
            ColumnFamilyDescriptor::new(column_families::REWARDS, opts)
        },

        // Reputation column family - frequent updates and queries
        {
            let mut opts = Options::default();
            opts.set_prefix_extractor(SliceTransform::create_fixed_prefix(10));
            ColumnFamilyDescriptor::new(column_families::REPUTATION, opts)
        },

        // Network state column family
        ColumnFamilyDescriptor::new(
            column_families::NETWORK_STATE,
            Options::default()
        ),

        // Fraud records column family
        ColumnFamilyDescriptor::new(
            column_families::FRAUD_RECORDS,
            Options::default()
        ),

        // Metrics column family - time series data
        {
            let mut opts = Options::default();
            opts.set_compression_type(rocksdb::DBCompressionType::Snappy);
            opts.set_compaction_style(rocksdb::DBCompactionStyle::Level);
            ColumnFamilyDescriptor::new(column_families::METRICS, opts)
        },

        // Configuration column family
        ColumnFamilyDescriptor::new(
            column_families::CONFIG,
            Options::default()
        ),
    ];

    DB::open_cf_descriptors(&db_opts, path, cfs)
}
```

## 5. Error Handling Unified Standards

### 5.1 Error Type Hierarchy

```rust
/// Unified error handling system
#[derive(Debug, thiserror::Error)]
pub enum AISystemError {
    #[error("Task error: {0}")]
    Task(#[from] TaskError),

    #[error("Miner error: {0}")]
    Miner(#[from] MinerError),

    #[error("Validation error: {0}")]
    Validation(#[from] ValidationError),

    #[error("Network error: {0}")]
    Network(#[from] NetworkError),

    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("Internal system error: {0}")]
    Internal(String),
}

/// Error recovery strategy
#[derive(Debug, Clone)]
pub enum RecoveryStrategy {
    Retry { max_attempts: u32, delay_ms: u64 },
    Fallback { alternative_action: String },
    Escalate { to_human: bool },
    Ignore,
    Shutdown,
}

/// Error handler
pub struct ErrorHandler {
    strategies: HashMap<String, RecoveryStrategy>,
    metrics: ErrorMetrics,
}

impl ErrorHandler {
    pub fn handle_error(&mut self, error: &AISystemError) -> Result<(), AISystemError> {
        // Record error metrics
        self.metrics.record_error(error);

        // Get recovery strategy
        let error_type = self.get_error_type(error);
        let strategy = self.strategies.get(&error_type)
            .cloned()
            .unwrap_or(RecoveryStrategy::Escalate { to_human: true });

        // Execute recovery strategy
        match strategy {
            RecoveryStrategy::Retry { max_attempts, delay_ms } => {
                self.execute_retry_strategy(error, max_attempts, delay_ms)
            },
            RecoveryStrategy::Fallback { alternative_action } => {
                self.execute_fallback_strategy(&alternative_action)
            },
            RecoveryStrategy::Escalate { to_human } => {
                self.escalate_error(error, to_human)
            },
            RecoveryStrategy::Ignore => {
                log::warn!("Ignoring error: {}", error);
                Ok(())
            },
            RecoveryStrategy::Shutdown => {
                log::error!("Critical error, initiating shutdown: {}", error);
                Err(error.clone())
            },
        }
    }

    fn get_error_type(&self, error: &AISystemError) -> String {
        match error {
            AISystemError::Task(_) => "task".to_string(),
            AISystemError::Miner(_) => "miner".to_string(),
            AISystemError::Validation(_) => "validation".to_string(),
            AISystemError::Network(_) => "network".to_string(),
            AISystemError::Storage(_) => "storage".to_string(),
            AISystemError::Config(_) => "config".to_string(),
            AISystemError::Internal(_) => "internal".to_string(),
        }
    }
}
```

This unified standards document ensures consistency and maintainability throughout the entire AI mining system, providing clear specification guidance for all developers.