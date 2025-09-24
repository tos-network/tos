# AI 挖矿系统统一标准和规范

## 1. 数据结构统一标准

### 1.1 核心数据类型

#### 标识符规范
```rust
// 统一使用 UUID v4 格式的字符串作为 ID
pub type TaskId = String;          // "550e8400-e29b-41d4-a716-446655440000"
pub type MinerId = String;         // "550e8400-e29b-41d4-a716-446655440001"
pub type SubmissionId = String;    // "550e8400-e29b-41d4-a716-446655440002"
pub type ValidationId = String;    // "550e8400-e29b-41d4-a716-446655440003"

// 地址统一使用 TOS 地址格式
pub type Address = String;         // "tos1abc123def456..."

// 哈希统一使用 32 字节数组
pub type Hash = [u8; 32];
```

#### 时间戳标准
```rust
// 统一使用 Unix 时间戳（u64 秒）
pub type Timestamp = u64;

// 时间相关工具函数
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

#### 货币金额标准
```rust
// 统一使用 u128 表示 TOS 数量（以 Wei 为单位，1 TOS = 10^18 Wei）
pub type TOSAmount = u128;

pub const TOS_DECIMALS: u32 = 18;
pub const TOS_WEI: u128 = 10u128.pow(TOS_DECIMALS);

// 工具函数
pub fn tos_to_wei(tos: f64) -> TOSAmount {
    (tos * TOS_WEI as f64) as u128
}

pub fn wei_to_tos(wei: TOSAmount) -> f64 {
    wei as f64 / TOS_WEI as f64
}
```

### 1.2 统一的声誉计算公式

```rust
/// 统一声誉计算系统
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
    pub fraud_penalty: f64,        // -1.0 (乘数)
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
    /// 统一声誉计算公式
    /// reputation_change = Σ(weight_i × score_i) × fraud_multiplier × innovation_multiplier
    pub fn calculate_reputation_change(
        &self,
        current_reputation: u32,
        performance_data: &PerformanceData,
    ) -> i32 {
        let weights = &self.base_weights;

        // 基础分数计算
        let base_score =
            weights.task_completion * performance_data.task_completion_rate +
            weights.quality_score * performance_data.average_quality_score +
            weights.validation_accuracy * performance_data.validation_accuracy +
            weights.peer_rating * performance_data.peer_rating_score;

        // 欺诈惩罚倍数
        let fraud_multiplier = if performance_data.fraud_detected {
            weights.fraud_penalty
        } else {
            1.0
        };

        // 创新奖励倍数
        let innovation_multiplier = 1.0 + (weights.innovation_bonus * performance_data.innovation_score);

        // 声誉衰减因子（防止无限增长）
        let decay_factor = self.calculate_decay_factor(current_reputation);

        // 最终计算
        let raw_change = base_score * fraud_multiplier * innovation_multiplier * decay_factor;

        // 转换为整数并应用边界
        (raw_change * 100.0).round() as i32
    }

    fn calculate_decay_factor(&self, current_reputation: u32) -> f64 {
        let max_reputation = 10000.0;
        let decay_rate = self.decay_config.decay_rate;

        // 线性衰减：声誉越高，增长越慢
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
    pub decay_rate: f64,      // 0.0-1.0, 默认 0.1
    pub min_decay: f64,       // 最小衰减因子，默认 0.5
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

## 2. API 接口统一标准

### 2.1 HTTP 响应格式标准

```rust
/// 统一的 API 响应格式
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

// 成功响应构造器
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

/// 分页响应格式
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

### 2.2 错误码标准

```rust
/// 统一错误码定义
pub mod error_codes {
    // 1000-1999: 通用错误
    pub const INVALID_REQUEST: &str = "1000";
    pub const UNAUTHORIZED: &str = "1001";
    pub const FORBIDDEN: &str = "1002";
    pub const NOT_FOUND: &str = "1004";
    pub const INTERNAL_ERROR: &str = "1500";

    // 2000-2999: 任务相关错误
    pub const TASK_NOT_FOUND: &str = "2000";
    pub const TASK_EXPIRED: &str = "2001";
    pub const TASK_FULL: &str = "2002";
    pub const INVALID_TASK_TYPE: &str = "2003";
    pub const INSUFFICIENT_REWARD: &str = "2004";

    // 3000-3999: 矿工相关错误
    pub const MINER_NOT_REGISTERED: &str = "3000";
    pub const MINER_ALREADY_EXISTS: &str = "3001";
    pub const INSUFFICIENT_STAKE: &str = "3002";
    pub const LOW_REPUTATION: &str = "3003";
    pub const MINER_SUSPENDED: &str = "3004";

    // 4000-4999: 验证相关错误
    pub const VALIDATION_FAILED: &str = "4000";
    pub const VALIDATOR_NOT_QUALIFIED: &str = "4001";
    pub const VALIDATION_TIMEOUT: &str = "4002";
    pub const CONSENSUS_NOT_REACHED: &str = "4003";

    // 5000-5999: 网络相关错误
    pub const NETWORK_ERROR: &str = "5000";
    pub const SYNC_FAILED: &str = "5001";
    pub const PEER_UNREACHABLE: &str = "5002";
}

/// 错误响应工具函数
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

### 2.3 HTTP 状态码映射标准

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

## 3. 配置参数统一标准

### 3.1 系统配置结构

```toml
# unified_config.toml - 统一配置文件格式

[network]
listen_addr = "0.0.0.0:8000"
max_peers = 100                    # 统一为100
connection_timeout = 30            # 30秒
sync_interval = 15                 # 15秒网络同步间隔

[ai_mining]
enabled = true
max_concurrent_tasks = 1000
task_timeout_hours = 24            # 统一为24小时
validation_timeout_hours = 48      # 统一为48小时
min_stake_amount = 100             # 100 TOS Wei

[validation]
consensus_threshold = 0.75         # 统一共识阈值
fraud_detection_threshold = 0.8    # 统一欺诈检测阈值
automatic_validation_enabled = true
peer_validation_required = true
expert_validation_threshold = 3    # 需要3个专家验证的阈值

[rewards]
network_fee_percentage = 0.05      # 5% 网络费用
winner_share = 0.65               # 65% 给获胜者
participant_share = 0.15          # 15% 给参与者
validator_share = 0.15            # 15% 给验证者

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
cache_size_mb = 1024              # 1GB 缓存
max_log_size_mb = 100
log_rotation_count = 10

[retry]
max_retries = 5                   # 统一重试次数
base_delay_ms = 1000             # 基础延迟1秒
max_delay_ms = 30000             # 最大延迟30秒
exponential_backoff = true

[performance]
batch_size = 100
worker_threads = 0               # 0表示使用CPU核心数
task_queue_size = 10000
validation_queue_size = 5000
```

### 3.2 配置加载器

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
        // 验证配置的合理性
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

## 4. 数据库架构统一标准

### 4.1 RocksDB 列族定义

```rust
/// 统一的 RocksDB 列族定义
pub mod column_families {
    pub const TASKS: &str = "tasks";                    // 任务数据
    pub const MINERS: &str = "miners";                  // 矿工数据
    pub const SUBMISSIONS: &str = "submissions";        // 提交数据
    pub const VALIDATIONS: &str = "validations";        // 验证数据
    pub const REWARDS: &str = "rewards";                // 奖励记录
    pub const REPUTATION: &str = "reputation";          // 声誉数据
    pub const NETWORK_STATE: &str = "network_state";    // 网络状态
    pub const FRAUD_RECORDS: &str = "fraud_records";    // 欺诈记录
    pub const METRICS: &str = "metrics";                // 性能指标
    pub const CONFIG: &str = "config";                  // 配置数据
}

/// 键值对格式标准
pub mod key_formats {
    use super::*;

    // 任务相关键格式
    pub fn task_key(task_id: &TaskId) -> String {
        format!("task:{}", task_id)
    }

    pub fn task_submissions_key(task_id: &TaskId) -> String {
        format!("task:{}:submissions", task_id)
    }

    pub fn task_validations_key(task_id: &TaskId) -> String {
        format!("task:{}:validations", task_id)
    }

    // 矿工相关键格式
    pub fn miner_key(miner_id: &MinerId) -> String {
        format!("miner:{}", miner_id)
    }

    pub fn miner_reputation_key(miner_id: &MinerId) -> String {
        format!("miner:{}:reputation", miner_id)
    }

    pub fn miner_performance_key(miner_id: &MinerId) -> String {
        format!("miner:{}:performance", miner_id)
    }

    // 时间序列键格式
    pub fn time_series_key(metric_name: &str, timestamp: Timestamp) -> String {
        format!("metric:{}:{:016x}", metric_name, timestamp)
    }
}

/// 数据库初始化配置
pub fn init_rocksdb_with_column_families(path: &Path) -> Result<rocksdb::DB, rocksdb::Error> {
    use rocksdb::{DB, Options, ColumnFamilyDescriptor, SliceTransform};

    let mut db_opts = Options::default();
    db_opts.create_if_missing(true);
    db_opts.create_missing_column_families(true);

    // 为不同列族设置不同的优化选项
    let cfs = vec![
        // 任务列族 - 主要是读取操作
        ColumnFamilyDescriptor::new(
            column_families::TASKS,
            Options::default()
        ),

        // 矿工列族 - 频繁更新
        {
            let mut opts = Options::default();
            opts.set_write_buffer_size(64 * 1024 * 1024); // 64MB
            ColumnFamilyDescriptor::new(column_families::MINERS, opts)
        },

        // 提交列族 - 大量写入
        {
            let mut opts = Options::default();
            opts.set_max_write_buffer_number(4);
            opts.set_write_buffer_size(32 * 1024 * 1024); // 32MB
            ColumnFamilyDescriptor::new(column_families::SUBMISSIONS, opts)
        },

        // 验证列族
        ColumnFamilyDescriptor::new(
            column_families::VALIDATIONS,
            Options::default()
        ),

        // 奖励列族 - 只写入一次，经常读取
        {
            let mut opts = Options::default();
            opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
            ColumnFamilyDescriptor::new(column_families::REWARDS, opts)
        },

        // 声誉列族 - 频繁更新和查询
        {
            let mut opts = Options::default();
            opts.set_prefix_extractor(SliceTransform::create_fixed_prefix(10));
            ColumnFamilyDescriptor::new(column_families::REPUTATION, opts)
        },

        // 网络状态列族
        ColumnFamilyDescriptor::new(
            column_families::NETWORK_STATE,
            Options::default()
        ),

        // 欺诈记录列族
        ColumnFamilyDescriptor::new(
            column_families::FRAUD_RECORDS,
            Options::default()
        ),

        // 指标列族 - 时间序列数据
        {
            let mut opts = Options::default();
            opts.set_compression_type(rocksdb::DBCompressionType::Snappy);
            opts.set_compaction_style(rocksdb::DBCompactionStyle::Level);
            ColumnFamilyDescriptor::new(column_families::METRICS, opts)
        },

        // 配置列族
        ColumnFamilyDescriptor::new(
            column_families::CONFIG,
            Options::default()
        ),
    ];

    DB::open_cf_descriptors(&db_opts, path, cfs)
}
```

## 5. 错误处理统一标准

### 5.1 错误类型层次结构

```rust
/// 统一错误处理系统
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

/// 错误恢复策略
#[derive(Debug, Clone)]
pub enum RecoveryStrategy {
    Retry { max_attempts: u32, delay_ms: u64 },
    Fallback { alternative_action: String },
    Escalate { to_human: bool },
    Ignore,
    Shutdown,
}

/// 错误处理器
pub struct ErrorHandler {
    strategies: HashMap<String, RecoveryStrategy>,
    metrics: ErrorMetrics,
}

impl ErrorHandler {
    pub fn handle_error(&mut self, error: &AISystemError) -> Result<(), AISystemError> {
        // 记录错误指标
        self.metrics.record_error(error);

        // 获取恢复策略
        let error_type = self.get_error_type(error);
        let strategy = self.strategies.get(&error_type)
            .cloned()
            .unwrap_or(RecoveryStrategy::Escalate { to_human: true });

        // 执行恢复策略
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

这个统一标准文档确保了整个 AI 挖矿系统的一致性和可维护性，为所有开发者提供了明确的规范指导。