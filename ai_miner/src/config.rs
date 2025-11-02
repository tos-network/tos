use anyhow::{anyhow, Result};
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use url::Url;

use tos_common::{crypto::Address, network::Network, prompt::LogLevel};

/// Network-specific address characteristics for validation
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct NetworkAddressCharacteristics {
    pub prefix: Option<&'static str>,
    pub format_hint: Option<&'static str>,
    pub min_length: usize,
    pub max_length: usize,
}

use crate::daemon_client::DaemonClientConfig;

/// Default values for configuration
pub mod defaults {
    use super::*;

    pub const LOG_LEVEL: LogLevel = LogLevel::Info;
    pub const FILENAME_LOG: &str = "tos-ai-miner.log";
    pub const LOGS_PATH: &str = "logs/";
    pub const STORAGE_PATH: &str = "storage/";
    pub const DAEMON_ADDRESS: &str = "http://127.0.0.1:18080";
    pub const NETWORK: &str = "mainnet";

    // Daemon client defaults
    pub const REQUEST_TIMEOUT_SECS: u64 = 30;
    pub const CONNECTION_TIMEOUT_SECS: u64 = 10;
    pub const MAX_RETRIES: u32 = 3;
    pub const RETRY_DELAY_MS: u64 = 1000;

    // Validation limits
    pub const MIN_TIMEOUT_SECS: u64 = 1;
    pub const MAX_TIMEOUT_SECS: u64 = 300;
    pub const MAX_RETRIES_LIMIT: u32 = 10;
    pub const MIN_RETRY_DELAY_MS: u64 = 100;
    pub const MAX_RETRY_DELAY_MS: u64 = 30000;
}

/// Enhanced configuration with validation
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidatedConfig {
    /// Log level configuration
    #[serde(default = "default_log_level")]
    pub log_level: LogLevel,

    /// File logging settings
    #[serde(default)]
    pub disable_file_logging: bool,

    #[serde(default)]
    pub disable_log_color: bool,

    #[serde(default)]
    pub disable_interactive_mode: bool,

    #[serde(default = "default_filename_log")]
    pub filename_log: String,

    #[serde(default = "default_logs_path")]
    pub logs_path: String,

    /// Storage configuration
    #[serde(default = "default_storage_path")]
    pub storage_path: String,

    /// Daemon connection settings
    #[serde(default = "default_daemon_address")]
    pub daemon_address: String,

    pub miner_address: Option<Address>,

    #[serde(default = "default_network")]
    pub network: String,

    /// Advanced daemon client configuration
    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,

    #[serde(default = "default_connection_timeout_secs")]
    pub connection_timeout_secs: u64,

    #[serde(default = "default_max_retries")]
    pub max_retries: u32,

    #[serde(default = "default_retry_delay_ms")]
    pub retry_delay_ms: u64,

    /// Auto-fix configuration issues
    #[serde(default)]
    pub auto_fix_config: bool,

    /// Validation settings
    #[serde(default)]
    pub strict_validation: bool,
}

// Default functions for serde
fn default_log_level() -> LogLevel {
    defaults::LOG_LEVEL
}
fn default_filename_log() -> String {
    defaults::FILENAME_LOG.to_string()
}
fn default_logs_path() -> String {
    defaults::LOGS_PATH.to_string()
}
fn default_storage_path() -> String {
    defaults::STORAGE_PATH.to_string()
}
fn default_daemon_address() -> String {
    defaults::DAEMON_ADDRESS.to_string()
}
fn default_network() -> String {
    defaults::NETWORK.to_string()
}
fn default_request_timeout_secs() -> u64 {
    defaults::REQUEST_TIMEOUT_SECS
}
fn default_connection_timeout_secs() -> u64 {
    defaults::CONNECTION_TIMEOUT_SECS
}
fn default_max_retries() -> u32 {
    defaults::MAX_RETRIES
}
fn default_retry_delay_ms() -> u64 {
    defaults::RETRY_DELAY_MS
}

impl Default for ValidatedConfig {
    fn default() -> Self {
        Self {
            log_level: default_log_level(),
            disable_file_logging: false,
            disable_log_color: false,
            disable_interactive_mode: false,
            filename_log: default_filename_log(),
            logs_path: default_logs_path(),
            storage_path: default_storage_path(),
            daemon_address: default_daemon_address(),
            miner_address: None,
            network: default_network(),
            request_timeout_secs: default_request_timeout_secs(),
            connection_timeout_secs: default_connection_timeout_secs(),
            max_retries: default_max_retries(),
            retry_delay_ms: default_retry_delay_ms(),
            auto_fix_config: true,
            strict_validation: false,
        }
    }
}

/// Configuration validation errors
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum ConfigValidationError {
    InvalidDaemonAddress(String),
    InvalidNetworkType(String),
    InvalidMinerAddress(String),
    InvalidTimeout {
        field: String,
        value: u64,
        min: u64,
        max: u64,
    },
    InvalidRetrySettings {
        field: String,
        value: u32,
        max: u32,
    },
    InvalidPath {
        field: String,
        path: String,
        reason: String,
    },
    DuplicateLogFile {
        logs_path: String,
        filename: String,
    },
    InsufficientPermissions {
        path: String,
        operation: String,
    },
}

impl std::fmt::Display for ConfigValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigValidationError::InvalidDaemonAddress(addr) => write!(
                f,
                "Invalid daemon address: '{}' - must be a valid HTTP/HTTPS URL",
                addr
            ),
            ConfigValidationError::InvalidNetworkType(network) => write!(
                f,
                "Invalid network type: '{}' - must be one of: mainnet, testnet, devnet, stagenet",
                network
            ),
            ConfigValidationError::InvalidMinerAddress(addr) => write!(
                f,
                "Invalid miner address: '{}' - must be a valid TOS address",
                addr
            ),
            ConfigValidationError::InvalidTimeout {
                field,
                value,
                min,
                max,
            } => write!(
                f,
                "Invalid {}: {} seconds - must be between {} and {} seconds",
                field, value, min, max
            ),
            ConfigValidationError::InvalidRetrySettings { field, value, max } => write!(
                f,
                "Invalid {}: {} - must be between 0 and {}",
                field, value, max
            ),
            ConfigValidationError::InvalidPath {
                field,
                path,
                reason,
            } => write!(f, "Invalid {}: '{}' - {}", field, path, reason),
            ConfigValidationError::DuplicateLogFile {
                logs_path,
                filename,
            } => write!(
                f,
                "Log file conflict: '{}' already exists in directory '{}'",
                filename, logs_path
            ),
            ConfigValidationError::InsufficientPermissions { path, operation } => write!(
                f,
                "Insufficient permissions for {}: '{}' - check directory permissions",
                operation, path
            ),
        }
    }
}

impl std::error::Error for ConfigValidationError {}

/// Configuration validation result
pub type ValidationResult<T> = std::result::Result<T, ConfigValidationError>;

/// Configuration validator
pub struct ConfigValidator {
    strict_mode: bool,
    auto_fix: bool,
}

impl ConfigValidator {
    pub fn new(strict_mode: bool, auto_fix: bool) -> Self {
        Self {
            strict_mode,
            auto_fix,
        }
    }

    /// Validate the entire configuration
    pub fn validate(&self, config: &mut ValidatedConfig) -> Result<Vec<String>> {
        let mut warnings = Vec::new();
        let mut fixed_issues = Vec::new();

        info!("🔍 Validating configuration...");

        // Validate daemon address
        if let Err(e) = self.validate_daemon_address(&config.daemon_address) {
            if self.auto_fix && !self.strict_mode {
                if log::log_enabled!(log::Level::Warn) {
                    warn!("Auto-fixing daemon address: {}", e);
                }
                config.daemon_address = defaults::DAEMON_ADDRESS.to_string();
                fixed_issues.push(format!(
                    "Fixed daemon address to default: {}",
                    config.daemon_address
                ));
            } else {
                return Err(anyhow!("Configuration validation failed: {}", e));
            }
        }

        // Validate network type
        if let Err(e) = self.validate_network(&config.network) {
            if self.auto_fix && !self.strict_mode {
                if log::log_enabled!(log::Level::Warn) {
                    warn!("Auto-fixing network type: {}", e);
                }
                config.network = defaults::NETWORK.to_string();
                fixed_issues.push(format!("Fixed network type to default: {}", config.network));
            } else {
                return Err(anyhow!("Configuration validation failed: {}", e));
            }
        }

        // Validate miner address if provided
        if let Some(ref addr) = config.miner_address {
            if let Err(e) = self.validate_miner_address(addr) {
                if self.strict_mode {
                    return Err(anyhow!("Configuration validation failed: {}", e));
                } else {
                    warnings.push(format!("Warning: {}", e));
                }
            }
        }

        // Validate timeout settings
        if let Err(e) = self.validate_timeout("request_timeout", config.request_timeout_secs) {
            if self.auto_fix && !self.strict_mode {
                if log::log_enabled!(log::Level::Warn) {
                    warn!("Auto-fixing request timeout: {}", e);
                }
                config.request_timeout_secs = defaults::REQUEST_TIMEOUT_SECS;
                fixed_issues.push(format!(
                    "Fixed request timeout to {} seconds",
                    config.request_timeout_secs
                ));
            } else {
                return Err(anyhow!("Configuration validation failed: {}", e));
            }
        }

        if let Err(e) = self.validate_timeout("connection_timeout", config.connection_timeout_secs)
        {
            if self.auto_fix && !self.strict_mode {
                if log::log_enabled!(log::Level::Warn) {
                    warn!("Auto-fixing connection timeout: {}", e);
                }
                config.connection_timeout_secs = defaults::CONNECTION_TIMEOUT_SECS;
                fixed_issues.push(format!(
                    "Fixed connection timeout to {} seconds",
                    config.connection_timeout_secs
                ));
            } else {
                return Err(anyhow!("Configuration validation failed: {}", e));
            }
        }

        // Validate retry settings
        if let Err(e) = self.validate_retry_count(config.max_retries) {
            if self.auto_fix && !self.strict_mode {
                if log::log_enabled!(log::Level::Warn) {
                    warn!("Auto-fixing max retries: {}", e);
                }
                config.max_retries = defaults::MAX_RETRIES;
                fixed_issues.push(format!("Fixed max retries to {}", config.max_retries));
            } else {
                return Err(anyhow!("Configuration validation failed: {}", e));
            }
        }

        if let Err(e) = self.validate_retry_delay(config.retry_delay_ms) {
            if self.auto_fix && !self.strict_mode {
                if log::log_enabled!(log::Level::Warn) {
                    warn!("Auto-fixing retry delay: {}", e);
                }
                config.retry_delay_ms = defaults::RETRY_DELAY_MS;
                fixed_issues.push(format!("Fixed retry delay to {} ms", config.retry_delay_ms));
            } else {
                return Err(anyhow!("Configuration validation failed: {}", e));
            }
        }

        // Validate and create directories
        self.validate_and_create_paths(config, &mut warnings, &mut fixed_issues)?;

        // Report results
        if !fixed_issues.is_empty() {
            if log::log_enabled!(log::Level::Info) {
                info!(
                    "🔧 Auto-fixed {} configuration issue(s):",
                    fixed_issues.len()
                );
            }
            for fix in &fixed_issues {
                if log::log_enabled!(log::Level::Info) {
                    info!("  ✅ {}", fix);
                }
            }
        }

        if !warnings.is_empty() {
            warn!("⚠️  Configuration warnings:");
            for warning in &warnings {
                if log::log_enabled!(log::Level::Warn) {
                    warn!("  • {}", warning);
                }
            }
        }

        let mut all_messages = fixed_issues;
        all_messages.extend(warnings);

        info!("✅ Configuration validation completed successfully");
        Ok(all_messages)
    }

    fn validate_daemon_address(&self, address: &str) -> ValidationResult<()> {
        let url_str = if address.starts_with("http://") || address.starts_with("https://") {
            address.to_string()
        } else {
            format!("http://{}", address)
        };

        Url::parse(&url_str)
            .map_err(|_| ConfigValidationError::InvalidDaemonAddress(address.to_string()))?;

        Ok(())
    }

    fn validate_network(&self, network: &str) -> ValidationResult<()> {
        match network.to_lowercase().as_str() {
            "mainnet" | "testnet" | "devnet" | "dev" | "stagenet" => Ok(()),
            _ => Err(ConfigValidationError::InvalidNetworkType(
                network.to_string(),
            )),
        }
    }

    fn validate_miner_address(&self, address: &Address) -> ValidationResult<()> {
        if log::log_enabled!(log::Level::Debug) {
            debug!("Validating miner address: {}", address);
        }

        // Comprehensive address validation
        self.validate_address_format(address)?;
        self.validate_address_network_compatibility(address)?;
        self.validate_address_type_compatibility(address)?;

        if log::log_enabled!(log::Level::Info) {
            info!("Miner address validation passed: {}", address);
        }
        Ok(())
    }

    fn validate_address_format(&self, address: &Address) -> ValidationResult<()> {
        let address_str = address.to_string();

        // Check address length (TOS addresses should be specific length)
        if address_str.len() < 20 || address_str.len() > 150 {
            return Err(ConfigValidationError::InvalidMinerAddress(format!(
                "Address length {} is invalid (expected 20-150 characters)",
                address_str.len()
            )));
        }

        // Check address format (should contain valid characters)
        if !address_str
            .chars()
            .all(|c| c.is_alphanumeric() || "._-".contains(c))
        {
            return Err(ConfigValidationError::InvalidMinerAddress(
                "Address contains invalid characters".to_string(),
            ));
        }

        if log::log_enabled!(log::Level::Debug) {
            debug!("Address format validation passed for: {}", address);
        }
        Ok(())
    }

    fn validate_address_network_compatibility(&self, address: &Address) -> ValidationResult<()> {
        // Get network-specific address prefixes/formats
        let expected_characteristics = self.get_network_address_characteristics();

        // Convert address to string for pattern matching
        let address_str = address.to_string();

        // Validate network-specific patterns
        if let Some(prefix) = expected_characteristics.prefix {
            if !address_str.starts_with(prefix) {
                if log::log_enabled!(log::Level::Warn) {
                    warn!(
                        "Address {} doesn't have expected network prefix '{}' for network type",
                        address, prefix
                    );
                }
                // Note: This is a warning, not an error, as address formats may vary
            }
        }

        // Check address format compatibility
        if let Some(expected_format) = expected_characteristics.format_hint {
            if !self.matches_address_format(&address_str, expected_format) {
                if log::log_enabled!(log::Level::Warn) {
                    warn!(
                        "Address {} may not be compatible with {} network format",
                        address, expected_format
                    );
                }
            }
        }

        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "Address network compatibility validation passed for: {}",
                address
            );
        }
        Ok(())
    }

    fn validate_address_type_compatibility(&self, address: &Address) -> ValidationResult<()> {
        // Validate that the address type is suitable for AI mining operations

        // Check if address appears to be a standard transaction address
        // (not a smart contract or special address type)
        let address_str = address.to_string();

        // Basic heuristics for address type validation
        if address_str.starts_with("contract_") || address_str.contains("::") {
            if log::log_enabled!(log::Level::Warn) {
                warn!("Address {} appears to be a contract address, which may not be suitable for mining rewards",
                      address);
            }
        }

        // Additional validation can be added based on TOS address specifications
        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "Address type compatibility validation passed for: {}",
                address
            );
        }
        Ok(())
    }

    fn get_network_address_characteristics(&self) -> NetworkAddressCharacteristics {
        // This would be expanded based on actual TOS network specifications
        NetworkAddressCharacteristics {
            prefix: None, // TOS may not use prefixes like Bitcoin
            format_hint: Some("standard"),
            min_length: 20,
            max_length: 150,
        }
    }

    fn matches_address_format(&self, address: &str, format_hint: &str) -> bool {
        match format_hint {
            "standard" => {
                // Standard TOS address format validation
                address.len() >= 20
                    && address.len() <= 150
                    && address
                        .chars()
                        .all(|c| c.is_alphanumeric() || "._-".contains(c))
            }
            "compressed" => {
                // Compressed address format (if supported)
                address.len() >= 20 && address.len() <= 50
            }
            _ => true, // Unknown format, assume valid
        }
    }

    fn validate_timeout(&self, field: &str, value: u64) -> ValidationResult<()> {
        if value < defaults::MIN_TIMEOUT_SECS || value > defaults::MAX_TIMEOUT_SECS {
            return Err(ConfigValidationError::InvalidTimeout {
                field: field.to_string(),
                value,
                min: defaults::MIN_TIMEOUT_SECS,
                max: defaults::MAX_TIMEOUT_SECS,
            });
        }
        Ok(())
    }

    fn validate_retry_count(&self, value: u32) -> ValidationResult<()> {
        if value > defaults::MAX_RETRIES_LIMIT {
            return Err(ConfigValidationError::InvalidRetrySettings {
                field: "max_retries".to_string(),
                value,
                max: defaults::MAX_RETRIES_LIMIT,
            });
        }
        Ok(())
    }

    fn validate_retry_delay(&self, value: u64) -> ValidationResult<()> {
        if value < defaults::MIN_RETRY_DELAY_MS || value > defaults::MAX_RETRY_DELAY_MS {
            return Err(ConfigValidationError::InvalidTimeout {
                field: "retry_delay".to_string(),
                value,
                min: defaults::MIN_RETRY_DELAY_MS,
                max: defaults::MAX_RETRY_DELAY_MS,
            });
        }
        Ok(())
    }

    fn validate_and_create_paths(
        &self,
        config: &ValidatedConfig,
        warnings: &mut Vec<String>,
        fixed_issues: &mut Vec<String>,
    ) -> Result<()> {
        // Validate and create logs directory
        self.ensure_directory_exists(&config.logs_path, "logs", fixed_issues)?;

        // Validate and create storage directory
        self.ensure_directory_exists(&config.storage_path, "storage", fixed_issues)?;

        // Check for potential log file conflicts
        let log_path = Path::new(&config.logs_path).join(&config.filename_log);
        if log_path.exists() && log_path.metadata()?.len() > 0 {
            warnings.push(format!(
                "Log file '{}' already exists and is not empty - logs will be appended",
                log_path.display()
            ));
        }

        Ok(())
    }

    fn ensure_directory_exists(
        &self,
        path: &str,
        dir_type: &str,
        fixed_issues: &mut Vec<String>,
    ) -> Result<()> {
        let path_buf = PathBuf::from(path);

        if !path_buf.exists() {
            if log::log_enabled!(log::Level::Info) {
                info!("Creating {} directory: {}", dir_type, path);
            }
            std::fs::create_dir_all(&path_buf).map_err(|e| {
                anyhow!("Failed to create {} directory '{}': {}", dir_type, path, e)
            })?;
            fixed_issues.push(format!("Created {} directory: {}", dir_type, path));
        } else if !path_buf.is_dir() {
            return Err(anyhow!("Path '{}' exists but is not a directory", path));
        }

        // Check write permissions
        let test_file = path_buf.join(".write_test");
        match std::fs::write(&test_file, "test") {
            Ok(_) => {
                let _ = std::fs::remove_file(test_file);
            }
            Err(e) => {
                return Err(anyhow!(
                    "Insufficient write permissions for {} directory '{}': {}",
                    dir_type,
                    path,
                    e
                ));
            }
        }

        Ok(())
    }
}

impl ValidatedConfig {
    /// Create DaemonClientConfig from validated settings
    pub fn to_daemon_client_config(&self) -> DaemonClientConfig {
        DaemonClientConfig {
            request_timeout: Duration::from_secs(self.request_timeout_secs),
            connection_timeout: Duration::from_secs(self.connection_timeout_secs),
            max_retries: self.max_retries,
            retry_delay: Duration::from_millis(self.retry_delay_ms),
        }
    }

    /// Parse network string to Network enum
    pub fn get_network(&self) -> Network {
        match self.network.to_lowercase().as_str() {
            "mainnet" => Network::Mainnet,
            "testnet" => Network::Testnet,
            "dev" | "devnet" => Network::Devnet,
            "stagenet" => Network::Stagenet,
            _ => {
                if log::log_enabled!(log::Level::Warn) {
                    warn!("Unknown network '{}', defaulting to mainnet", self.network);
                }
                Network::Mainnet
            }
        }
    }

    /// Validate and load configuration from file
    pub fn from_file<P: AsRef<Path>>(path: P, strict_mode: bool, auto_fix: bool) -> Result<Self> {
        let content = std::fs::read_to_string(&path).map_err(|e| {
            anyhow!(
                "Failed to read config file '{}': {}",
                path.as_ref().display(),
                e
            )
        })?;

        let mut config: ValidatedConfig = serde_json::from_str(&content).map_err(|e| {
            anyhow!(
                "Failed to parse config file '{}': {}",
                path.as_ref().display(),
                e
            )
        })?;

        // Validate the loaded configuration
        let validator = ConfigValidator::new(strict_mode, auto_fix);
        let messages = validator.validate(&mut config)?;

        if !messages.is_empty() {
            if log::log_enabled!(log::Level::Info) {
                info!(
                    "Configuration loaded with {} adjustments/warnings",
                    messages.len()
                );
            }
        }

        Ok(config)
    }

    /// Save validated configuration to file
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| anyhow!("Failed to serialize config: {}", e))?;

        std::fs::write(&path, content).map_err(|e| {
            anyhow!(
                "Failed to write config file '{}': {}",
                path.as_ref().display(),
                e
            )
        })?;

        if log::log_enabled!(log::Level::Info) {
            info!("Configuration saved to: {}", path.as_ref().display());
        }
        Ok(())
    }

    /// Generate a configuration template with descriptive structure
    pub fn generate_template<P: AsRef<Path>>(path: P) -> Result<()> {
        let template_config = ValidatedConfig::default();
        let _json_content = serde_json::to_string_pretty(&template_config)?;

        // Add descriptive header
        let template = format!(
            r#"{{
  "_info": {{
    "description": "TOS AI Mining Configuration",
    "version": "1.0",
    "sections": {{
      "logging": "Controls log output and file generation",
      "storage": "Persistent storage for AI mining state",
      "daemon": "Connection settings for TOS daemon",
      "network": "Advanced network timeouts and retry settings",
      "validation": "Configuration validation behavior"
    }}
  }},
  "log_level": "info",
  "disable_file_logging": false,
  "disable_log_color": false,
  "disable_interactive_mode": false,
  "filename_log": "tos-ai-miner.log",
  "logs_path": "logs/",
  "storage_path": "storage/",
  "daemon_address": "http://127.0.0.1:18080",
  "network": "mainnet",
  "request_timeout_secs": 30,
  "connection_timeout_secs": 10,
  "max_retries": 3,
  "retry_delay_ms": 1000,
  "auto_fix_config": true,
  "strict_validation": false
}}"#
        );

        std::fs::write(&path, template).map_err(|e| {
            anyhow!(
                "Failed to write template to '{}': {}",
                path.as_ref().display(),
                e
            )
        })?;

        if log::log_enabled!(log::Level::Info) {
            info!(
                "Configuration template generated at: {}",
                path.as_ref().display()
            );
        }
        Ok(())
    }
}
