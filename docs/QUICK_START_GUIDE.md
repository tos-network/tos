# TOS AI Mining - Quick Start Guide

## Overview

Get started with TOS AI Mining in 5 minutes! This guide covers everything from installation to running your first AI mining workflow.

**Status**: ✅ **Fully Tested and Working**

## Prerequisites

- Rust 1.70+ (for building from source)
- Python 3.7+ (for testing and integration)
- TOS Daemon running (any network)

## Installation

### Option 1: Build from Source

```bash
# Clone the repository
git clone https://github.com/tos-network/tos.git
cd tos

# Build the AI miner
cargo build --release --bin tos_ai_miner

# Binary will be available at: target/release/tos_ai_miner
```

### Option 2: Development Build

```bash
# For development and testing
cargo build --bin tos_ai_miner

# Binary will be available at: target/debug/tos_ai_miner
```

## Step 1: Start TOS Daemon

First, ensure you have a TOS daemon running:

```bash
# Start daemon on development network
cd daemon
cargo run --bin tos_daemon -- --network devnet
```

You should see output like:
```
✅ TOS Blockchain running version: 0.1.0
✅ RPC Server will listen on: 0.0.0.0:8080
✅ P2p Server will listen on: 0.0.0.0:2125
```

## Step 2: Generate Configuration

Create a configuration file for your AI miner:

```bash
# Generate default configuration
./target/debug/tos_ai_miner --generate-config-template --config-file ai_miner.json
```

This creates `ai_miner.json`:
```json
{
  "network": "devnet",
  "daemon_address": "http://127.0.0.1:8080",
  "miner_address": null,
  "storage_path": "storage/",
  "request_timeout_secs": 30,
  "connection_timeout_secs": 10,
  "max_retries": 3,
  "retry_delay_ms": 1000,
  "strict_validation": false
}
```

## Step 3: Start AI Miner

Launch the AI miner with your configuration:

```bash
# Method 1: Using configuration file
./target/debug/tos_ai_miner --config-file ai_miner.json

# Method 2: Using command line parameters
./target/debug/tos_ai_miner --network devnet --daemon-address http://127.0.0.1:8080
```

Expected output:
```
✅ TOS AI Miner v0.1.0 starting...
✅ Daemon address: http://127.0.0.1:8080
✅ Storage initialized at: storage/
✅ Successfully connected to daemon version: 0.1.0-03854eb
```

## Step 4: Test the Integration

### Option 1: Run Built-in Tests

Test the complete AI mining workflow:

```bash
cd ai_miner
cargo test --test ai_mining_workflow_tests
```

You should see:
```
running 7 tests
test test_task_publication_workflow ... ok
test test_answer_submission_workflow ... ok
test test_validation_workflow ... ok
test test_reward_distribution_workflow ... ok
test test_miner_registration_workflow ... ok
test test_payload_complexity_calculation ... ok
test test_daemon_client_config ... ok

test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### Option 2: Python Integration Test

Create and run a Python test script:

```bash
# From the root directory
python3 simple_ai_test.py
```

Expected results:
```
✅ Daemon connection: TOS v0.1.0-03854eb (Devnet)
✅ Task generation: 2M nanoTOS reward, intermediate difficulty
✅ AI computation: Answer hash generated
✅ Validation: 83% validation score
✅ Reward calculation:
   - Base reward: 2,000,000 nanoTOS
   - Actual reward: 1,660,000 nanoTOS
   - Miner reward: 1,162,000 nanoTOS (70%)
   - Validator reward: 332,000 nanoTOS (20%)
   - Network fee: 166,000 nanoTOS (10%)
```

## Understanding the AI Mining Workflow

### 1. Miner Registration

```rust
// Register as an AI miner
let metadata = builder.build_register_miner_transaction(
    miner_address,
    100000, // Registration fee: 100K nanoTOS
    0,      // Nonce
    0       // Auto-calculate fee
)?;
```

### 2. Task Publication

```rust
// Publish an AI mining task
let metadata = builder.build_publish_task_transaction(
    task_id,
    2000000,                      // Reward: 2M nanoTOS
    DifficultyLevel::Intermediate, // Difficulty level
    deadline_timestamp,           // Task deadline
    1,                           // Nonce
    0                            // Auto-calculate fee
)?;
```

### 3. Answer Submission

```rust
// Submit AI computation result
let metadata = builder.build_submit_answer_transaction(
    task_id,
    answer_hash,
    100000,  // Stake: 100K nanoTOS
    2,       // Nonce
    0        // Auto-calculate fee
)?;
```

### 4. Answer Validation

```rust
// Validate submitted answers
let metadata = builder.build_validate_answer_transaction(
    task_id,
    answer_id,
    85,      // Validation score (85%)
    3,       // Nonce
    0        // Auto-calculate fee
)?;
```

### 5. Reward Distribution

The system automatically calculates and distributes rewards:

- **70%** to answer providers (miners)
- **20%** to validators
- **10%** to network maintenance

## Network Configuration

### Development Network (Devnet)

```bash
tos_ai_miner --network devnet --daemon-address http://127.0.0.1:8080
```

**Features:**
- Lowest fees (0.5x multiplier)
- Fast testing
- Local development only

### Test Network (Testnet)

```bash
tos_ai_miner --network testnet --daemon-address http://testnet.tos.network:8080
```

**Features:**
- Standard fees (1.0x multiplier)
- Public testing environment
- Real blockchain testing

### Production Network (Mainnet)

```bash
tos_ai_miner --network mainnet --daemon-address http://mainnet.tos.network:8080
```

**Features:**
- Production fees (2.0x multiplier)
- Real TOS tokens
- Production environment

## Fee Structure

### Base Fees (200-byte transaction)

| Network | Multiplier | Base Fee |
|---------|------------|----------|
| Devnet | 0.5x | 1,250 nanoTOS |
| Testnet | 1.0x | 2,500 nanoTOS |
| Mainnet | 2.0x | 5,000 nanoTOS |
| Stagenet | 1.5x | 3,750 nanoTOS |

### Transaction Type Multipliers

| Transaction Type | Multiplier | Example Fee (Testnet) |
|------------------|------------|---------------------|
| RegisterMiner | 1.0x | 2,500 nanoTOS |
| SubmitAnswer | 1.5x | 3,750 nanoTOS |
| ValidateAnswer | 1.75x | 4,375 nanoTOS |
| PublishTask | 2.0x | 5,000 nanoTOS |

## Storage and State Management

### Storage Structure

```
storage/
├── ai_mining_devnet.json    # Devnet state
├── ai_mining_testnet.json   # Testnet state
├── ai_mining_mainnet.json   # Mainnet state
└── backups/                 # Automatic backups
```

### State Information

The system tracks:
- **Miner Information**: Registration, statistics, activity
- **Task Data**: Published tasks, states, deadlines
- **Transaction History**: All operations with metadata
- **Performance Metrics**: Success rates, earnings, reputation

## Monitoring and Logs

### Log Configuration

```bash
# Set log level
tos_ai_miner --log-level debug

# Disable file logging
tos_ai_miner

# Custom log path
tos_ai_miner --logs-path ./custom_logs/
```

### Log Locations

- **Console**: Real-time output
- **File**: `logs/tos-ai-miner.log` (rotated daily)
- **Structured**: JSON format for parsing

## Troubleshooting

### Common Issues

#### Connection Problems

```bash
Error: Network error: Connection refused (os error 61)
```

**Solution**: Ensure TOS daemon is running and accessible:
```bash
# Test daemon connection
curl http://127.0.0.1:8080/json_rpc -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","method":"get_info","id":1}'
```

#### Configuration Issues

```bash
Error: Configuration error: Invalid network 'invalid-net'
```

**Solution**: Use valid network names: `mainnet`, `testnet`, `devnet`, `stagenet`

#### Storage Problems

```bash
Error: Storage error: Permission denied (os error 13)
```

**Solution**: Ensure write permissions for storage directory:
```bash
chmod 755 storage/
```

### Debug Mode

Enable verbose logging for troubleshooting:

```bash
tos_ai_miner --log-level debug --network devnet
```

This provides detailed information about:
- RPC calls and responses
- Transaction building process
- Storage operations
- Network communications

## Next Steps

### For Developers

1. **Explore API**: Check [AI_MINER_API_REFERENCE.md](./AI_MINER_API_REFERENCE.md)
2. **Integration**: Build applications using the Python client
3. **Custom Tasks**: Define your own AI task types
4. **Validation**: Implement custom validation logic

### For Miners

1. **Register**: Use a real wallet address for production
2. **Optimize**: Monitor performance and adjust strategies
3. **Scale**: Run multiple miner instances
4. **Monitor**: Track earnings and reputation

### For Task Publishers

1. **Task Design**: Create meaningful AI computation tasks
2. **Reward Strategy**: Set appropriate rewards for task difficulty
3. **Integration**: Connect your applications to publish tasks
4. **Quality Control**: Implement validation criteria

## Advanced Configuration

### Production Configuration

```json
{
  "network": "mainnet",
  "daemon_address": "https://mainnet.tos.network:8080",
  "miner_address": "tos1your_production_address...",
  "storage_path": "/var/lib/tos/ai_miner/",
  "request_timeout_secs": 60,
  "connection_timeout_secs": 30,
  "max_retries": 5,
  "retry_delay_ms": 2000,
  "strict_validation": true,
  "log_level": "info",
  "logs_path": "/var/log/tos/",
  "disable_file_logging": false
}
```

### High Availability Setup

```bash
# Multiple instances with load balancing
tos_ai_miner --config-file instance1.json &
tos_ai_miner --config-file instance2.json &
tos_ai_miner --config-file instance3.json &
```

## Support and Resources

- **Documentation**: [Complete documentation index](./README.md)
- **API Reference**: [AI_MINER_API_REFERENCE.md](./AI_MINER_API_REFERENCE.md)
- **Implementation Status**: [AI_MINING_IMPLEMENTATION_STATUS.md](./AI_MINING_IMPLEMENTATION_STATUS.md)
- **Issues**: Report bugs and feature requests
- **Community**: Join TOS Discord for support

---

**Congratulations!** 🎉

You now have a fully functional TOS AI Mining setup. The system is ready for:
- ✅ Task publication and management
- ✅ AI computation and answer submission
- ✅ Answer validation and scoring
- ✅ Automatic reward distribution
- ✅ Full workflow testing and monitoring

Start mining intelligent work and earning TOS rewards! 🚀

---

**Last Updated**: September 26, 2025
**Version**: 1.0.0
**Status**: ✅ Production Ready