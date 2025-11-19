# ERC20 Integration Tests - Quick Start Guide

This directory contains comprehensive ERC20 token integration tests for TOS blockchain.

## 📁 Test Files

```
tests/
├── erc20_integration_test.rs    # Basic operations (8 tests)
├── erc20_advanced_test.rs       # Advanced features (10 tests)
├── erc20_scenario_test.rs       # Real-world scenarios (7 tests)
└── README_ERC20_TESTS.md        # This file
```

## 🚀 Quick Start

### 1. Prerequisites

Ensure token contract bytecode exists:

```bash
ls ../daemon/tests/fixtures/token.so
```

If not found, build it:

```bash
cd ~/tos-network/tako/examples/token
cargo build --release --target tbpf-tos-tos
mkdir -p ~/tos-network/tos/daemon/tests/fixtures/
cp target/tbpf-tos-tos/release/token.so ~/tos-network/tos/daemon/tests/fixtures/
```

### 2. Run All Tests

```bash
cd ~/tos-network/tos

# Run all 25 ERC20 tests
cargo test --package tos_testing_framework erc20
```

### 3. Run Specific Test Suites

```bash
# Basic operations (8 tests)
cargo test --package tos_testing_framework --test erc20_integration_test

# Advanced features (10 tests)
cargo test --package tos_testing_framework --test erc20_advanced_test

# Scenarios (7 tests)
cargo test --package tos_testing_framework --test erc20_scenario_test
```

### 4. Run Individual Tests

```bash
# Deployment test
cargo test --package tos_testing_framework test_erc20_deployment_and_initial_supply -- --nocapture

# Transfer test with logs
RUST_LOG=info cargo test --package tos_testing_framework test_erc20_transfer -- --nocapture

# Multiple transfers
cargo test --package tos_testing_framework test_erc20_multiple_transfers -- --nocapture
```

## 📊 Test Categories

### Basic Operations (erc20_integration_test.rs)

- ✅ Deployment and initial supply
- ✅ Transfer operations
- ✅ Insufficient balance error handling
- ✅ Multiple sequential transfers
- ✅ Compute unit consumption
- ✅ Zero amount transfers
- ✅ Storage persistence
- ✅ Sequential operations

### Advanced Features (erc20_advanced_test.rs)

- ✅ Approve/TransferFrom mechanism
- ✅ Burn operations
- ✅ Mint with access control
- ✅ Overflow protection
- ✅ Self-transfer
- ✅ Batch operations
- ✅ State rollback on error
- ✅ Large balance operations
- ✅ Gas estimation accuracy
- ✅ Event logging

### Real-World Scenarios (erc20_scenario_test.rs)

- ✅ Token sale (ICO)
- ✅ Staking and rewards
- ✅ Token vesting
- ✅ Multi-signature wallet
- ✅ DEX swaps
- ✅ Airdrop distribution
- ✅ Governance voting

## 🎯 Expected Output

```
running 25 tests
test test_erc20_deployment_and_initial_supply ... ok
test test_erc20_transfer ... ok
test test_erc20_insufficient_balance ... ok
...
test test_erc20_governance_voting_scenario ... ok

test result: ok. 25 passed; 0 failed; 0 ignored
```

## 📖 Detailed Documentation

See [ERC20_INTEGRATION_TESTS.md](../ERC20_INTEGRATION_TESTS.md) for complete documentation.

## 🐛 Troubleshooting

**Contract not found:**
```
ls daemon/tests/fixtures/token.so
# Should exist, if not, see step 1 above
```

**Permission denied:**
```bash
chmod -R 755 test_integration/
```

**Test timeout:**
Add to failing test:
```rust
#[tokio::test(flavor = "multi_thread")]
```

## 💡 Tips

- Use `--nocapture` to see log output
- Use `RUST_LOG=info` for detailed logs
- Use `RUST_LOG=debug` for very verbose logs
- Run tests in parallel: `cargo test -- --test-threads=4`
- Run specific pattern: `cargo test erc20_transfer`

## 📝 Adding New Tests

See the template in [ERC20_INTEGRATION_TESTS.md](../ERC20_INTEGRATION_TESTS.md#writing-new-tests)

