# TOS AI Miner API Reference

## Overview

This document provides complete API reference for the TOS AI Mining system, covering all implemented interfaces, transaction types, and integration methods.

**Status**: ✅ **Fully Implemented and Tested**

## Table of Contents

- [Command Line Interface](#command-line-interface)
- [Configuration System](#configuration-system)
- [Transaction Builder API](#transaction-builder-api)
- [Daemon Client API](#daemon-client-api)
- [Storage Manager API](#storage-manager-api)
- [Python Integration](#python-integration)
- [RPC Methods](#rpc-methods)
- [Error Handling](#error-handling)

## Command Line Interface

### Basic Usage

```bash
tos_ai_miner [OPTIONS]
```

### Available Options

| Option | Type | Description | Default |
|--------|------|-------------|---------|
| `--network <NETWORK>` | String | Network to use (mainnet, testnet, devnet, stagenet) | `mainnet` |
| `--daemon-address <ADDRESS>` | String | Daemon address to connect to | `http://127.0.0.1:18080` |
| `-m, --miner-address <ADDRESS>` | String | Wallet address for AI mining operations | None |
| `--storage-path <PATH>` | String | Storage directory for AI mining state | `storage/` |
| `--config-file <FILE>` | String | JSON configuration file path | None |
| `--generate-config-template` | Flag | Generate configuration template | false |
| `--request-timeout-secs <SECS>` | u64 | Request timeout in seconds | `30` |
| `--connection-timeout-secs <SECS>` | u64 | Connection timeout in seconds | `10` |
| `--max-retries <COUNT>` | u32 | Maximum number of retries | `3` |
| `--retry-delay-ms <MS>` | u64 | Retry delay in milliseconds | `1000` |
| `--strict-validation` | Flag | Enable strict configuration validation | false |
| `--log-level <LEVEL>` | String | Set log level (off, error, warn, info, debug, trace) | `info` |

### Examples

```bash
# Run on development network
tos_ai_miner --network devnet --daemon-address http://127.0.0.1:8080

# Generate configuration template
tos_ai_miner --generate-config-template --config-file config.json

# Run with configuration file
tos_ai_miner --config-file production.json

# Run with specific miner address
tos_ai_miner --miner-address tos1abc123... --network testnet
```

## Configuration System

### Configuration File Format

```json
{
  "network": "devnet",
  "daemon_address": "http://127.0.0.1:8080",
  "miner_address": "tos1abc123...",
  "storage_path": "storage/",
  "request_timeout_secs": 30,
  "connection_timeout_secs": 10,
  "max_retries": 3,
  "retry_delay_ms": 1000,
  "strict_validation": true,
  "log_level": "info",
  "logs_path": "logs/",
  "disable_file_logging": false
}
```

### Configuration Validation

The system automatically validates:
- Network compatibility
- Address format validation
- Path accessibility
- Timeout ranges
- Retry configuration limits

## Transaction Builder API

### Core Structures

#### AIMiningTransactionMetadata

```rust
pub struct AIMiningTransactionMetadata {
    pub payload: AIMiningPayload,
    pub estimated_fee: u64,
    pub estimated_size: usize,
    pub nonce: u64,
    pub network: Network,
}
```

#### AIMiningPayload Types

```rust
pub enum AIMiningPayload {
    RegisterMiner {
        miner_address: CompressedPublicKey,
        registration_fee: u64,
    },
    PublishTask {
        task_id: Hash,
        reward_amount: u64,
        difficulty: DifficultyLevel,
        deadline: u64,
        description: String, // 10-2048 bytes, UTF-8 encoded
    },
    SubmitAnswer {
        task_id: Hash,
        answer_content: String, // 10-2048 bytes, UTF-8 encoded
        answer_hash: Hash,
        stake_amount: u64,
    },
    ValidateAnswer {
        task_id: Hash,
        answer_id: Hash,
        validation_score: u8, // 0-100
    },
}
```

### Transaction Builder Methods

#### Miner Registration

```rust
pub fn build_register_miner_transaction(
    &self,
    miner_address: CompressedPublicKey,
    registration_fee: u64,
    nonce: u64,
    fee: u64,
) -> Result<AIMiningTransactionMetadata>
```

**Example**:
```rust
let builder = AIMiningTransactionBuilder::new(Network::Testnet);
let metadata = builder.build_register_miner_transaction(
    miner_address,
    100000, // 100K nanoTOS
    0,      // nonce
    0       // auto-estimate fee
)?;
```

#### Task Publication

```rust
pub fn build_publish_task_transaction(
    &self,
    task_id: Hash,
    reward_amount: u64,
    difficulty: DifficultyLevel,
    deadline: u64,
    description: String, // Task description (10-2048 bytes)
    nonce: u64,
    fee: u64,
) -> Result<AIMiningTransactionMetadata>
```

**Example**:
```rust
let metadata = builder.build_publish_task_transaction(
    task_id,
    2000000,                    // 2M nanoTOS reward
    DifficultyLevel::Intermediate,
    1234567890,                 // deadline timestamp
    "Analyze this image and identify all objects with their positions and confidence scores. Provide detailed reasoning for your classifications.".to_string(), // task description
    1,                          // nonce
    0                           // auto-estimate fee
)?;
```

#### Answer Submission

```rust
pub fn build_submit_answer_transaction(
    &self,
    task_id: Hash,
    answer_content: String, // Answer content (10-2048 bytes)
    answer_hash: Hash,
    stake_amount: u64,
    nonce: u64,
    fee: u64,
) -> Result<AIMiningTransactionMetadata>
```

#### Answer Validation

```rust
pub fn build_validate_answer_transaction(
    &self,
    task_id: Hash,
    answer_id: Hash,
    validation_score: u8,
    nonce: u64,
    fee: u64,
) -> Result<AIMiningTransactionMetadata>
```

### Fee Calculation

#### Network-Specific Fee Multipliers

| Network | Multiplier | Base Fee (200 bytes) |
|---------|------------|---------------------|
| Devnet | 0.5x | 1,250 nanoTOS |
| Testnet | 1.0x | 2,500 nanoTOS |
| Mainnet | 2.0x | 5,000 nanoTOS |
| Stagenet | 1.5x | 3,750 nanoTOS |

#### Content-based Gas Pricing

| Content Type | Price per Byte | Example (100 bytes) |
|--------------|----------------|---------------------|
| Task Description | 1,000,000 nanoTOS | 100,000,000 nanoTOS (0.1 TOS) |
| Answer Content | 1,000,000 nanoTOS | 100,000,000 nanoTOS (0.1 TOS) |

**Length Constraints**:
- Minimum: 10 bytes (both description and answer)
- Maximum: 2048 bytes (both description and answer)
- Encoding: UTF-8 required

#### Payload Complexity Multipliers

| Payload Type | Multiplier | Example Fee (Testnet) |
|--------------|------------|---------------------|
| RegisterMiner | 1.0x | 2,500 nanoTOS |
| SubmitAnswer | 1.5x | 3,750 nanoTOS |
| ValidateAnswer | 1.75x | 4,375 nanoTOS |
| PublishTask | 2.0x | 5,000 nanoTOS |

## Daemon Client API

### Core Methods

#### Connection Management

```rust
pub fn new(daemon_address: &str) -> Result<Self>
pub fn with_config(daemon_address: &str, config: DaemonClientConfig) -> Result<Self>
pub async fn test_connection(&self) -> Result<Value>
```

#### RPC Methods

```rust
// Basic daemon information
pub async fn get_info(&self) -> Result<Value>
pub async fn get_height(&self) -> Result<u64>

// Transaction operations
pub async fn submit_transaction(&self, tx: &Transaction) -> Result<Hash>
pub async fn get_transaction(&self, tx_hash: &Hash) -> Result<Value>
pub async fn get_tx_status(&self, tx_hash: &Hash) -> Result<Value>

// AI Mining specific
pub async fn get_ai_mining_info(&self) -> Result<Value>
pub async fn get_ai_mining_tasks(&self, limit: Option<u64>) -> Result<Value>
pub async fn get_ai_mining_task(&self, task_id: &Hash) -> Result<Value>
pub async fn get_miner_stats(&self, miner_address: &str) -> Result<Value>

// Network information
pub async fn get_network_stats(&self) -> Result<Value>
pub async fn get_peers_info(&self) -> Result<Value>
pub async fn get_difficulty(&self) -> Result<Value>
```

### Configuration

```rust
pub struct DaemonClientConfig {
    pub request_timeout: Duration,
    pub max_retries: u32,
    pub retry_delay: Duration,
    pub connection_timeout: Duration,
}

impl Default for DaemonClientConfig {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(30),
            max_retries: 3,
            retry_delay: Duration::from_secs(1),
            connection_timeout: Duration::from_secs(10),
        }
    }
}
```

## Storage Manager API

### Core Structures

#### Task Information

```rust
pub struct TaskInfo {
    pub task_id: String,
    pub reward_amount: u64,
    pub difficulty: DifficultyLevel,
    pub deadline: u64,
    pub state: TaskState,
    pub created_at: u64,
    pub updated_at: u64,
}

pub enum TaskState {
    Published,
    AnswersReceived,
    Validated,
    Expired,
}
```

#### Miner Information

```rust
pub struct MinerInfo {
    pub miner_address: String,
    pub registration_fee: u64,
    pub registered_at: u64,
    pub is_active: bool,
    pub total_tasks_published: u64,
    pub total_answers_submitted: u64,
    pub total_validations_performed: u64,
}
```

### Storage Operations

```rust
// Initialization
pub async fn new(storage_dir: PathBuf, network: Network) -> Result<Self>

// Miner management
pub async fn register_miner(&mut self, miner_address: &PublicKey, registration_fee: u64) -> Result<()>
pub fn get_miner_info(&self) -> Option<&MinerInfo>

// Task management
pub async fn add_task(&mut self, task_id: &Hash, reward_amount: u64, difficulty: DifficultyLevel, deadline: u64) -> Result<()>
pub async fn update_task_state(&mut self, task_id: &Hash, new_state: TaskState) -> Result<()>
pub fn get_task(&self, task_id: &Hash) -> Option<&TaskInfo>
pub fn get_all_tasks(&self) -> &HashMap<String, TaskInfo>

// Transaction tracking
pub async fn add_transaction(&mut self, metadata: &AIMiningTransactionMetadata, tx_hash: Option<Hash>) -> Result<()>
pub fn get_transactions(&self) -> &Vec<TransactionRecord>
pub fn get_recent_transactions(&self, limit: usize) -> Vec<&TransactionRecord>

// Statistics
pub fn get_stats(&self) -> StorageStats
```

## Python Integration

### Basic Client

```python
import json
import urllib.request
import random

class TOSAIMiningClient:
    def __init__(self, daemon_url: str = "http://127.0.0.1:8080"):
        self.daemon_url = daemon_url

    def rpc_call(self, method: str, params: dict = None) -> dict:
        payload = {
            "jsonrpc": "2.0",
            "method": method,
            "id": random.randint(1, 10000)
        }
        if params:
            payload["params"] = params

        data = json.dumps(payload).encode('utf-8')
        req = urllib.request.Request(
            f"{self.daemon_url}/json_rpc",
            data=data,
            headers={'Content-Type': 'application/json'}
        )

        with urllib.request.urlopen(req, timeout=30) as response:
            result = json.loads(response.read().decode('utf-8'))

        if "error" in result:
            raise Exception(f"RPC Error: {result['error']}")
        return result.get("result", {})
```

### Usage Examples

```python
# Connect to daemon
client = TOSAIMiningClient("http://127.0.0.1:8080")

# Get daemon information
daemon_info = client.rpc_call("get_info")
print(f"Daemon Version: {daemon_info.get('version')}")
print(f"Network: {daemon_info.get('network')}")

# Get current height
height_result = client.rpc_call("get_height")
print(f"Current Height: {height_result.get('height', 0)}")
```

## RPC Methods

### Standard Daemon Methods

| Method | Parameters | Returns | Description |
|--------|------------|---------|-------------|
| `get_info` | None | `DaemonInfo` | Get daemon version and network info |
| `get_height` | None | `{"height": u64}` | Get current blockchain height |
| `submit_transaction` | `{"tx_data": String}` | `{"tx_hash": String}` | Submit transaction to network |

### AI Mining Methods (Future)

| Method | Parameters | Returns | Description |
|--------|------------|---------|-------------|
| `get_ai_mining_info` | None | `AIMiningInfo` | Get AI mining statistics |
| `get_ai_mining_tasks` | `{"limit": u64}` | `[TaskInfo]` | List active AI mining tasks |
| `get_ai_mining_task` | `{"task_id": String}` | `TaskInfo` | Get specific task details |
| `submit_ai_answer` | `AnswerData` | `{"answer_id": String}` | Submit answer to task |

## Error Handling

### Error Types

```rust
pub enum AIMiningError {
    ConfigurationError(String),
    NetworkError(String),
    TransactionError(String),
    StorageError(String),
    ValidationError(String),
    TimeoutError(String),
}
```

### Common Error Scenarios

#### Network Connectivity
```rust
// Connection timeout
Error: Network error: Connection timed out after 10s

// RPC error
Error: RPC Error: {"code": -32602, "message": "Invalid parameters"}

// Daemon unreachable
Error: Network error: Connection refused (os error 61)
```

#### Configuration Issues
```rust
// Invalid network
Error: Configuration error: Invalid network 'invalid-net'

// Invalid address format
Error: Configuration error: Invalid miner address format

// Storage path issues
Error: Storage error: Unable to create directory '/invalid/path'
```

#### Transaction Errors
```rust
// Insufficient fee
Error: Transaction error: Fee too low for network

// Invalid payload
Error: Transaction error: Invalid payload structure

// Nonce issues
Error: Transaction error: Invalid nonce sequence
```

### Error Recovery Strategies

1. **Automatic Retry**: Network errors with exponential backoff
2. **Graceful Degradation**: Continue operation with limited functionality
3. **Configuration Validation**: Prevent errors through upfront validation
4. **Detailed Logging**: Comprehensive error reporting and debugging

## Testing

### Test Coverage

All major components have comprehensive test coverage:

- ✅ **Transaction Building**: All payload types and fee calculation
- ✅ **Storage Operations**: Task and miner management
- ✅ **Network Communication**: RPC calls and error handling
- ✅ **Configuration**: Validation and template generation
- ✅ **Integration**: End-to-end workflow testing

### Test Execution

```bash
# Run all tests
cargo test

# Run specific test suite
cargo test --test ai_mining_workflow_tests

# Run with output
cargo test -- --nocapture
```

## Performance Considerations

### Resource Usage

- **Memory**: ~10-50MB depending on task history
- **Storage**: ~1MB per 1000 tasks (JSON format)
- **Network**: ~1-5KB per RPC call
- **CPU**: Minimal, mainly JSON processing

### Optimization Guidelines

1. **Batch Operations**: Group multiple tasks when possible
2. **Connection Pooling**: Reuse daemon connections
3. **Storage Cleanup**: Regularly archive completed tasks
4. **Retry Logic**: Configure appropriate timeouts and retry counts

## Security Considerations

### Best Practices

1. **Configuration Security**: Store sensitive data securely
2. **Network Security**: Use HTTPS/TLS for daemon connections
3. **Input Validation**: Validate all external inputs
4. **Error Handling**: Don't expose sensitive information in errors

### Audit Trail

All operations are logged with:
- Timestamps
- Transaction hashes
- Error details
- Performance metrics

---

**Last Updated**: September 27, 2025
**Version**: 1.1.0 - Answer Content Storage Update
**Implementation Status**: ✅ Complete and Tested
**Key Updates**: Direct answer content storage, length-based gas pricing, enhanced validation capabilities