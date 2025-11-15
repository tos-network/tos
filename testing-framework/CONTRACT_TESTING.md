# Smart Contract Testing with TOS Testing Framework

## Overview

The TOS Testing Framework now supports comprehensive smart contract testing using real TAKO VM execution and RocksDB storage. This provides a superior alternative to mock-based testing approaches.

## Key Features

✅ **Real TAKO VM Execution** - Uses `TakoExecutor::execute_simple()` for authentic contract execution
✅ **Real RocksDB Storage** - Full storage persistence and versioned reads at different topoheights
✅ **Compute Unit Tracking** - Accurate gas metering for performance testing
✅ **Storage Inspection** - Read and verify contract persistent storage state
✅ **Deterministic** - Reproducible test results with seeded RNG
✅ **Fast** - Tests complete in milliseconds with no external dependencies

## Quick Start

```rust
use tos_testing_framework::utilities::{
    create_contract_test_storage, execute_test_contract,
};
use tos_common::crypto::{Hash, KeyPair};

#[tokio::test]
async fn test_my_contract() -> anyhow::Result<()> {
    // Setup: Create storage with funded account
    let account = KeyPair::new();
    let storage = create_contract_test_storage(&account, 1_000_000).await?;

    // Load contract bytecode
    let bytecode = include_bytes!("path/to/contract.so");

    // Execute contract
    let contract_hash = Hash::zero();
    let result = execute_test_contract(bytecode, &storage, 1, &contract_hash).await?;

    // Verify results
    assert_eq!(result.return_value, 0);
    assert!(result.compute_units_used > 0);

    Ok(())
}
```

## Available Helpers

### `create_contract_test_storage(account, balance)`

Creates a RocksDB storage instance with a funded account, ready for contract testing.

**Parameters:**
- `account: &KeyPair` - The account to fund
- `balance: u64` - Initial balance in nanoTOS

**Returns:** `Arc<RwLock<RocksStorage>>`

**Example:**
```rust
let account = KeyPair::new();
let storage = create_contract_test_storage(&account, 1_000_000).await?;
```

### `execute_test_contract(bytecode, storage, topoheight, contract_hash)`

Executes a smart contract using the TAKO VM with real storage.

**Parameters:**
- `bytecode: &[u8]` - Compiled contract bytecode (ELF format)
- `storage: &Arc<RwLock<RocksStorage>>` - RocksDB storage instance
- `topoheight: TopoHeight` - Current topoheight for versioned reads
- `contract_hash: &Hash` - Contract identifier (can be any Hash for testing)

**Returns:** `ExecutionResult` containing:
- `return_value: u64` - Contract return code (0 = success)
- `compute_units_used: u64` - Gas consumed
- `logs: Vec<String>` - Contract log output

**Example:**
```rust
let bytecode = include_bytes!("../fixtures/counter.so");
let result = execute_test_contract(bytecode, &storage, 1, &Hash::zero()).await?;
assert_eq!(result.return_value, 0);
```

### `get_contract_storage(storage, contract_hash, key, topoheight)`

Reads a value from contract's persistent storage.

**Parameters:**
- `storage: &Arc<RwLock<RocksStorage>>` - RocksDB storage instance
- `contract_hash: Hash` - Contract identifier
- `key: &[u8]` - Storage key
- `topoheight: TopoHeight` - Topoheight for versioned read

**Returns:** `Option<Vec<u8>>` - The stored value, or None if key doesn't exist

**Example:**
```rust
let count = get_contract_storage(&storage, contract_hash, b"counter", 10).await?;
if let Some(value) = count {
    println!("Counter value: {:?}", value);
}
```

### `fund_test_account(storage, account, balance)`

Funds an additional test account in existing storage.

**Parameters:**
- `storage: &Arc<RwLock<RocksStorage>>` - RocksDB storage instance
- `account: &KeyPair` - Account to fund
- `balance: u64` - Balance in nanoTOS

**Example:**
```rust
let user = KeyPair::new();
fund_test_account(&storage, &user, 500_000).await?;
```

### `contract_exists(storage, contract_hash, topoheight)`

Checks if a contract is deployed at a given topoheight.

**Parameters:**
- `storage: &Arc<RwLock<RocksStorage>>` - RocksDB storage instance
- `contract_hash: Hash` - Contract identifier
- `topoheight: TopoHeight` - Topoheight to check

**Returns:** `bool` - True if contract exists

**Example:**
```rust
let exists = contract_exists(&storage, contract_hash, 1).await?;
assert!(exists, "Contract should be deployed");
```

## Common Testing Patterns

### Pattern 1: Test Contract Execution Success

```rust
#[tokio::test]
async fn test_contract_executes_successfully() -> anyhow::Result<()> {
    let account = KeyPair::new();
    let storage = create_contract_test_storage(&account, 1_000_000).await?;

    let bytecode = include_bytes!("../fixtures/hello_world.so");
    let result = execute_test_contract(bytecode, &storage, 1, &Hash::zero()).await?;

    assert_eq!(result.return_value, 0, "Contract should return success");
    Ok(())
}
```

### Pattern 2: Test Storage Persistence

```rust
#[tokio::test]
async fn test_contract_storage_persistence() -> anyhow::Result<()> {
    let account = KeyPair::new();
    let storage = create_contract_test_storage(&account, 1_000_000).await?;
    let contract_hash = Hash::zero();

    // Execute contract that writes to storage
    let bytecode = include_bytes!("../fixtures/counter.so");
    execute_test_contract(bytecode, &storage, 1, &contract_hash).await?;

    // Verify storage was written
    let value = get_contract_storage(&storage, contract_hash, b"count", 1).await?;
    assert!(value.is_some(), "Storage should be written");

    Ok(())
}
```

### Pattern 3: Test Compute Units

```rust
#[tokio::test]
async fn test_contract_gas_consumption() -> anyhow::Result<()> {
    let account = KeyPair::new();
    let storage = create_contract_test_storage(&account, 1_000_000).await?;

    let bytecode = include_bytes!("../fixtures/expensive_operation.so");
    let result = execute_test_contract(bytecode, &storage, 1, &Hash::zero()).await?;

    assert!(result.compute_units_used > 0, "Should consume gas");
    assert!(
        result.compute_units_used < 1_000_000,
        "Should not exceed reasonable limit"
    );

    Ok(())
}
```

### Pattern 4: Test Versioned Behavior

```rust
#[tokio::test]
async fn test_contract_at_different_topoheights() -> anyhow::Result<()> {
    let account = KeyPair::new();
    let storage = create_contract_test_storage(&account, 1_000_000).await?;
    let contract_hash = Hash::zero();

    let bytecode = include_bytes!("../fixtures/versioned_contract.so");

    // Execute at different topoheights
    for topoheight in [1, 10, 100, 1000] {
        let result = execute_test_contract(
            bytecode,
            &storage,
            topoheight,
            &contract_hash
        ).await?;

        // Verify storage at each version
        let value = get_contract_storage(
            &storage,
            contract_hash,
            b"version",
            topoheight
        ).await?;

        println!("Topoheight {}: value = {:?}", topoheight, value);
    }

    Ok(())
}
```

### Pattern 5: Test Multiple Contracts

```rust
#[tokio::test]
async fn test_multiple_contracts() -> anyhow::Result<()> {
    let account = KeyPair::new();
    let storage = create_contract_test_storage(&account, 10_000_000).await?;

    // Deploy and test multiple contracts
    let contracts = vec![
        ("counter.so", Hash::from([1u8; 32])),
        ("storage.so", Hash::from([2u8; 32])),
        ("calculator.so", Hash::from([3u8; 32])),
    ];

    for (filename, hash) in contracts {
        let bytecode = include_bytes!(concat!("../fixtures/", filename));
        let result = execute_test_contract(bytecode, &storage, 1, &hash).await?;
        assert_eq!(result.return_value, 0);
    }

    Ok(())
}
```

## Comparison with Mock-Based Testing

### Before (Mock-Based Approach)

```rust
// daemon/tests/tako_hello_world_test.rs (old approach)
struct MockProvider {
    // 100+ lines of mock implementation
}

impl ContractProvider for MockProvider {
    // Implement 20+ trait methods manually
    fn get_contract_balance_for_asset(...) -> Result<Option<(TopoHeight, u64)>> {
        Ok(Some((0, 1000000))) // Hardcoded mock values
    }
    // ... 20 more methods ...
}

#[test]
fn test_contract() {
    let mut provider = MockProvider::new();
    // Test with fake data
}
```

**Problems:**
- ❌ Fragile - breaks when trait changes
- ❌ Incomplete - doesn't test real storage interactions
- ❌ Maintenance burden - 100+ lines of boilerplate per test
- ❌ Not production-like - uses fake data

### After (Testing Framework Approach)

```rust
// testing-framework/tests/contract_integration_example.rs
#[tokio::test]
async fn test_contract() -> anyhow::Result<()> {
    let account = KeyPair::new();
    let storage = create_contract_test_storage(&account, 1_000_000).await?;

    let bytecode = include_bytes!("../fixtures/hello_world.so");
    let result = execute_test_contract(bytecode, &storage, 1, &Hash::zero()).await?;

    assert_eq!(result.return_value, 0);
    Ok(())
}
```

**Benefits:**
- ✅ **Simple** - 10 lines instead of 100+
- ✅ **Real Storage** - Tests actual RocksDB persistence
- ✅ **Deterministic** - Reproducible results
- ✅ **Maintainable** - No mock boilerplate
- ✅ **Production-like** - Uses real TAKO VM

## Example Tests

See `testing-framework/tests/contract_integration_example.rs` for complete examples:

1. **test_hello_world_contract** - Basic contract execution
2. **test_contract_existence_check** - Contract deployment verification
3. **test_contract_compute_units** - Gas consumption tracking
4. **test_contract_execution_at_different_topoheights** - Versioned execution

Run examples:
```bash
cargo test --package tos-testing-framework --test contract_integration_example
```

## Implementation Details

### Module Structure

```
testing-framework/src/utilities/
├── mod.rs                  # Module exports
├── contract_helpers.rs     # Smart contract testing helpers (NEW)
├── daemon_helpers.rs       # RocksDB storage setup
└── storage.rs              # Temporary storage utilities
```

### Dependencies

The contract testing helpers require:
- `tos_daemon` - For `TakoExecutor` and `RocksStorage`
- `tos_common` - For contract types and crypto
- `tos-kernel` - For `ValueCell` type (from TAKO repository)

### How It Works

1. **Storage Setup**: `create_test_rocksdb_storage()` creates a temporary RocksDB instance
2. **Account Funding**: `setup_account_rocksdb()` funds accounts with test balances
3. **Contract Execution**: `TakoExecutor::execute_simple()` runs contracts with real VM
4. **Storage Access**: `RocksStorage` implements `ContractProvider` trait for state access
5. **Cleanup**: Temporary directories are automatically cleaned up (RAII)

### Test Performance

- **Setup**: ~5ms to create storage and fund accounts
- **Execution**: ~1-10ms per contract execution
- **Total**: Most tests complete in < 20ms

## Best Practices

### 1. Use Descriptive Contract Hashes

```rust
// ✅ Good - meaningful identifiers
let counter_hash = Hash::from([1u8; 32]);
let storage_hash = Hash::from([2u8; 32]);

// ❌ Bad - all contracts use same hash
let hash = Hash::zero();
```

### 2. Verify All Execution Results

```rust
// ✅ Good - check all relevant fields
assert_eq!(result.return_value, 0);
assert!(result.compute_units_used > 0);
assert!(result.logs.is_empty());

// ❌ Bad - only check success
assert_eq!(result.return_value, 0);
```

### 3. Test Storage State

```rust
// ✅ Good - verify storage state
execute_test_contract(bytecode, &storage, 1, &hash).await?;
let value = get_contract_storage(&storage, hash, b"key", 1).await?;
assert_eq!(value, Some(vec![42]));

// ❌ Bad - only test execution, not state
execute_test_contract(bytecode, &storage, 1, &hash).await?;
```

### 4. Use include_bytes! for Fixtures

```rust
// ✅ Good - compile-time validation
let bytecode = include_bytes!("../fixtures/counter.so");

// ❌ Bad - runtime file I/O
let bytecode = std::fs::read("../fixtures/counter.so")?;
```

### 5. Test Gas Limits

```rust
// ✅ Good - verify reasonable gas consumption
assert!(result.compute_units_used < MAX_EXPECTED_GAS);

// ❌ Bad - no gas limit verification
assert!(result.compute_units_used > 0);
```

## Troubleshooting

### Contract Execution Fails

**Problem**: `execute_test_contract` returns error

**Solution**: Check that:
1. Bytecode is valid ELF format
2. Storage is properly initialized
3. Account has sufficient balance
4. Contract hash is consistent

### Storage Read Returns None

**Problem**: `get_contract_storage` returns None

**Solution**: Verify:
1. Contract actually writes to that key
2. Using correct topoheight (must be >= execution topoheight)
3. Contract hash matches execution hash

### Test is Non-Deterministic

**Problem**: Test results vary between runs

**Solution**: Ensure:
1. Using `TestRng` for randomness
2. No external time sources
3. No threading without proper synchronization

## Future Enhancements

- **Contract Deployment** - Full deploy workflow with Module parsing
- **Cross-Contract Calls** - CPI testing with multiple contracts
- **Event Inspection** - Verify contract event emissions
- **State Snapshots** - Compare storage state at different topoheights
- **Gas Profiling** - Detailed compute unit breakdowns

## References

- **Source Code**: `testing-framework/src/utilities/contract_helpers.rs`
- **Examples**: `testing-framework/tests/contract_integration_example.rs`
- **TAKO Integration**: `daemon/src/tako_integration/executor.rs`
- **Storage Provider**: `daemon/src/core/storage/rocksdb/`

---

**Last Updated**: 2025-11-15
**Version**: 1.0.0
**Status**: Production Ready ✅
