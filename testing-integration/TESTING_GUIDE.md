# TOS Blockchain Testing Guide

**Version**: 1.0
**Last Updated**: 2025-10-30
**Status**: Production Ready

---

## Table of Contents

1. [Overview](#overview)
2. [The Sled Deadlock Problem](#the-sled-deadlock-problem)
3. [Safe Testing Patterns](#safe-testing-patterns)
4. [Migration Guide](#migration-guide)
5. [Test Examples](#test-examples)
6. [API Reference](#api-reference)
7. [Troubleshooting](#troubleshooting)
8. [Best Practices](#best-practices)

---

## Overview

### Purpose of testing-integration Package

The `tos-testing-integration` package provides safe, reliable utilities for writing integration tests for TOS blockchain components. It solves the critical **sled storage deadlock problem** that has caused 73+ tests to be marked as `#[ignore]`.

### When to Use It

Use `tos-testing-integration` when:

- ✅ Testing parallel transaction execution (`ParallelChainState`)
- ✅ Testing state consistency under concurrent operations
- ✅ Writing stress tests that setup multiple accounts
- ✅ Testing transaction validation logic
- ✅ Any test that needs to setup account state before execution

Do NOT use it for:

- ❌ End-to-end blockchain tests (use daemon directly)
- ❌ Network/P2P tests (use daemon cluster setup)
- ❌ Simple unit tests that don't touch storage

### Key Benefits

1. **No Deadlocks**: Safe helpers prevent sled internal state conflicts
2. **Fast**: Tests complete in milliseconds instead of timing out
3. **Reliable**: Consistent, deterministic test results
4. **Clean**: Simple API with minimal boilerplate
5. **Well-Tested**: Proven patterns from working tests

---

## The Sled Deadlock Problem

### What Causes It

When tests manually write versioned balances to sled storage and then create `ParallelChainState`, a deadlock occurs in sled's internal LRU cache:

```
┌─────────────────────────────────────────────────────────────┐
│ Test Thread                                                 │
├─────────────────────────────────────────────────────────────┤
│ 1. storage.write().set_last_balance_to(...) ← Writes data  │
│ 2. drop(storage_write)                      ← Releases lock │
│ 3. ParallelChainState::new(storage)         ← Reads data    │
│                                                             │
│ ⚠️  Problem: Sled's internal flush not complete!           │
│                                                             │
│ 4. ParallelChainState loads accounts in parallel            │
│ 5. Concurrent reads hit uncommitted sled state              │
│ 6. Sled LRU cache Mutex deadlocks          ❌ TIMEOUT      │
└─────────────────────────────────────────────────────────────┘
```

### Why Tests Timeout

Sled storage uses internal caching and batching mechanisms:

1. **Write batching**: Writes are buffered before being committed to the tree
2. **LRU cache**: Recent reads/writes cached in a Mutex-protected structure
3. **Concurrent access**: Multiple threads can trigger cache eviction simultaneously
4. **Mutex contention**: Under concurrent load, cache Mutex can deadlock

When `ParallelChainState::new()` spawns parallel tasks to load accounts, these tasks race with uncommitted sled internal operations, causing deadlock.

### How to Recognize It

**Symptoms**:
- Test hangs indefinitely (no output)
- Test timeout after 60-120 seconds
- CPU usage near 0% during hang
- No error message, just timeout

**Common locations**:
```rust
// Test hangs HERE ↓
let parallel_state = ParallelChainState::new(storage, 0).await?;
```

**Fix identification**:
- Check if test manually calls `set_last_balance_to()` or `set_last_nonce_to()`
- Check if test creates `ParallelChainState` shortly after storage writes
- Check if test is marked `#[ignore]` with comment about "deadlock" or "timeout"

---

## Safe Testing Patterns

### Pattern 1: Using setup_account_safe()

**Best for**: Parallel execution tests using real sled storage

```rust
use std::sync::Arc;
use tempdir::TempDir;
use tos_testing_integration::utils::storage_helpers::{
    create_test_storage,
    setup_account_safe,
    flush_storage_and_wait,
};

#[tokio::test]
async fn test_parallel_execution_safe() {
    // 1. Create test storage
    let storage = create_test_storage().await;

    // 2. Setup accounts using SAFE helper
    let account_a = create_test_account(1);
    let account_b = create_test_account(2);

    setup_account_safe(&storage, &account_a, 1000, 0).await.unwrap();
    setup_account_safe(&storage, &account_b, 2000, 0).await.unwrap();

    // 3. CRITICAL: Flush storage before parallel state creation
    flush_storage_and_wait(&storage).await;

    // 4. Create ParallelChainState (no deadlock!)
    let parallel_state = ParallelChainState::new(storage.clone(), 0).await.unwrap();

    // 5. Test logic
    assert_eq!(parallel_state.get_balance(&account_a, &TOS_ASSET), 1000);
    assert_eq!(parallel_state.get_balance(&account_b, &TOS_ASSET), 2000);
}
```

**Why it works**:
1. `setup_account_safe()` writes in single-threaded context
2. Adds 10ms delay after write for sled internal flush
3. `flush_storage_and_wait()` adds additional 50ms safety delay
4. ParallelChainState reads from fully-committed storage

### Pattern 2: Using MockStorage (Alternative)

**Best for**: Unit tests that don't need real storage

```rust
use std::sync::Arc;
use parking_lot::RwLock;
use tos_testing_integration::{MockStorage, setup_account_mock};

#[tokio::test]
async fn test_parallel_state_basic() {
    // 1. Create MockStorage (in-memory, no sled)
    let storage = MockStorage::new_with_tos_asset();

    // 2. Setup accounts (no deadlock risk!)
    let account_a = create_test_account(1);
    setup_account_mock(&storage, &account_a, 1000, 0);

    // 3. Create ParallelChainState
    let storage_arc = Arc::new(RwLock::new(storage.clone()));
    let parallel_state = ParallelChainState::new(storage_arc, 0).await.unwrap();

    // 4. Test logic
    assert_eq!(parallel_state.get_balance(&account_a, &TOS_ASSET), 1000);
}
```

**Why it works**:
1. MockStorage uses simple `HashMap + RwLock` (no sled)
2. No internal caching or batching
3. Fully synchronous operations
4. Zero deadlock risk

### Pattern 3: Batch Account Setup

**Best for**: Tests needing many accounts

```rust
use tos_testing_integration::utils::storage_helpers::{
    create_test_storage_with_accounts,
};

#[tokio::test]
async fn test_many_accounts() {
    // Create 100 accounts in one call (safe and fast!)
    let accounts = vec![
        (account_1, 1000, 0),
        (account_2, 2000, 5),
        // ... 98 more accounts
    ];

    let storage = create_test_storage_with_accounts(accounts).await;
    let parallel_state = ParallelChainState::new(storage, 0).await.unwrap();

    // All accounts loaded safely!
}
```

---

## Migration Guide

### Before/After Examples

#### Example 1: Basic Parallel Execution Test

**BEFORE (deadlock-prone)**:
```rust
#[ignore]  // ← Test times out!
#[tokio::test]
async fn test_parallel_vs_sequential() {
    let temp_dir = TempDir::new("test").unwrap();
    let storage = SledStorage::new(
        temp_dir.path().to_string_lossy().to_string(),
        /* ... */
    ).unwrap();
    let storage_arc = Arc::new(RwLock::new(storage));

    // Manual storage write (causes deadlock!)
    {
        let mut storage_write = storage_arc.write().await;
        storage_write.set_last_balance_to(
            &account_a,
            &TOS_ASSET,
            0,
            &VersionedBalance::new(1000, Some(0))
        ).await.unwrap();
        storage_write.set_last_nonce_to(
            &account_a,
            0,
            &VersionedNonce::new(0, Some(0))
        ).await.unwrap();
    }

    // THIS HANGS ↓
    let parallel_state = ParallelChainState::new(storage_arc, 0).await.unwrap();
}
```

**AFTER (safe)**:
```rust
#[tokio::test]  // ← No #[ignore]!
async fn test_parallel_vs_sequential() {
    use tos_testing_integration::utils::storage_helpers::{
        create_test_storage,
        setup_account_safe,
        flush_storage_and_wait,
    };

    // Create test storage
    let storage = create_test_storage().await;

    // Safe account setup
    setup_account_safe(&storage, &account_a, 1000, 0).await.unwrap();

    // Flush storage (ensures sled commits)
    flush_storage_and_wait(&storage).await;

    // Create parallel state (no deadlock!)
    let parallel_state = ParallelChainState::new(storage, 0).await.unwrap();
}
```

**Key changes**:
1. ✅ Replaced manual `set_last_balance_to()` with `setup_account_safe()`
2. ✅ Added `flush_storage_and_wait()` before `ParallelChainState::new()`
3. ✅ Removed `#[ignore]` attribute
4. ✅ Test completes in ~50ms instead of timing out

#### Example 2: Multiple Accounts

**BEFORE (deadlock-prone)**:
```rust
#[ignore]
#[tokio::test]
async fn test_many_accounts() {
    let storage = create_storage().await;

    // Manual writes (each one risks deadlock!)
    for i in 0..100 {
        let account = create_test_account(i);
        let mut storage_write = storage.write().await;
        storage_write.set_last_balance_to(
            &account,
            &TOS_ASSET,
            0,
            &VersionedBalance::new(i * 100, Some(0))
        ).await.unwrap();
        drop(storage_write);
    }

    // Deadlock likely here ↓
    let parallel_state = ParallelChainState::new(storage, 0).await.unwrap();
}
```

**AFTER (safe with MockStorage)**:
```rust
#[tokio::test]
async fn test_many_accounts() {
    use tos_testing_integration::{MockStorage, setup_account_mock};

    let storage = MockStorage::new_with_tos_asset();

    // Safe batch setup (no deadlock risk!)
    let accounts: Vec<_> = (0..100)
        .map(|i| {
            let account = create_test_account(i);
            setup_account_mock(&storage, &account, i * 100, 0);
            account
        })
        .collect();

    // Create parallel state (instant!)
    let storage_arc = Arc::new(RwLock::new(storage));
    let parallel_state = ParallelChainState::new(storage_arc, 0).await.unwrap();

    // Verify all accounts loaded
    for (i, account) in accounts.iter().enumerate() {
        assert_eq!(parallel_state.get_balance(account, &TOS_ASSET), i as u64 * 100);
    }
}
```

**Key changes**:
1. ✅ Switched from `SledStorage` to `MockStorage`
2. ✅ Used `setup_account_mock()` instead of manual writes
3. ✅ No need for flush or delays
4. ✅ Test completes in ~10ms

### Step-by-Step Migration Process

#### Step 1: Identify Tests to Migrate

Look for:
- Tests marked `#[ignore]`
- Comments mentioning "timeout", "deadlock", or "sled"
- Tests in `daemon/tests/parallel_execution_*.rs`
- Tests that call `set_last_balance_to()` or `set_last_nonce_to()`

#### Step 2: Choose Migration Strategy

**Use `setup_account_safe()` if**:
- Test needs real blockchain storage
- Test validates storage persistence
- Test interacts with blockchain components beyond ParallelChainState

**Use `MockStorage` if**:
- Test only validates ParallelChainState logic
- Test doesn't need storage persistence
- Test setup involves 10+ accounts
- Test is purely a unit test

#### Step 3: Update Imports

```rust
// Add to top of test file:
use tos_testing_integration::utils::storage_helpers::{
    create_test_storage,
    setup_account_safe,
    flush_storage_and_wait,
};

// OR for MockStorage:
use tos_testing_integration::{MockStorage, setup_account_mock};
```

#### Step 4: Replace Storage Setup

**Find this pattern**:
```rust
let mut storage_write = storage.write().await;
storage_write.set_last_balance_to(...).await?;
storage_write.set_last_nonce_to(...).await?;
drop(storage_write);
```

**Replace with**:
```rust
setup_account_safe(&storage, &account, balance, nonce).await?;
```

#### Step 5: Add Flush Before ParallelChainState

**Find this pattern**:
```rust
let parallel_state = ParallelChainState::new(storage, 0).await?;
```

**Replace with**:
```rust
flush_storage_and_wait(&storage).await;
let parallel_state = ParallelChainState::new(storage, 0).await?;
```

#### Step 6: Remove #[ignore] and Test

1. Remove `#[ignore]` attribute
2. Run test: `cargo test test_name -- --nocapture`
3. Verify test completes in < 1 second
4. Verify test passes consistently (run 10 times)

### Common Pitfalls

❌ **Pitfall 1: Forgetting flush_storage_and_wait()**

```rust
// BAD: Still can deadlock!
setup_account_safe(&storage, &account, 1000, 0).await?;
let parallel_state = ParallelChainState::new(storage, 0).await?;
```

```rust
// GOOD: Safe
setup_account_safe(&storage, &account, 1000, 0).await?;
flush_storage_and_wait(&storage).await;  // ← Don't forget!
let parallel_state = ParallelChainState::new(storage, 0).await?;
```

❌ **Pitfall 2: Mixing manual writes with safe helpers**

```rust
// BAD: Mixing patterns is dangerous!
setup_account_safe(&storage, &account_a, 1000, 0).await?;
{
    let mut write = storage.write().await;
    write.set_last_balance_to(&account_b, &TOS_ASSET, 0, ...).await?;
}
let parallel_state = ParallelChainState::new(storage, 0).await?;
```

```rust
// GOOD: Use one pattern consistently
setup_account_safe(&storage, &account_a, 1000, 0).await?;
setup_account_safe(&storage, &account_b, 2000, 0).await?;
flush_storage_and_wait(&storage).await;
let parallel_state = ParallelChainState::new(storage, 0).await?;
```

❌ **Pitfall 3: Using MockStorage for full blockchain tests**

```rust
// BAD: MockStorage doesn't support full Storage trait
let storage = MockStorage::new();
let blockchain = Blockchain::new(storage)?;  // ❌ Won't compile!
```

```rust
// GOOD: Use real storage for blockchain
let storage = create_test_storage().await;
let blockchain = Blockchain::new(storage)?;  // ✅ Works
```

---

## Test Examples

### Example 1: Basic Parallel Execution Test

```rust
use std::sync::Arc;
use tos_testing_integration::utils::storage_helpers::{
    create_test_storage,
    setup_account_safe,
    flush_storage_and_wait,
};
use tos_daemon::core::state::parallel_chain_state::ParallelChainState;
use tos_common::config::TOS_ASSET;

#[tokio::test]
async fn test_parallel_transfer_consistency() {
    // 1. Setup storage
    let storage = create_test_storage().await;

    // 2. Create test accounts
    let sender = create_test_account(1);
    let receiver = create_test_account(2);

    // 3. Setup initial state
    setup_account_safe(&storage, &sender, 1000, 0).await.unwrap();
    setup_account_safe(&storage, &receiver, 0, 0).await.unwrap();
    flush_storage_and_wait(&storage).await;

    // 4. Create parallel state
    let parallel_state = ParallelChainState::new(storage, 0).await.unwrap();

    // 5. Execute transfer
    parallel_state.sub_balance(&sender, &TOS_ASSET, 500).unwrap();
    parallel_state.add_balance(&receiver, &TOS_ASSET, 500);
    parallel_state.increment_nonce(&sender).unwrap();

    // 6. Verify consistency
    assert_eq!(parallel_state.get_balance(&sender, &TOS_ASSET), 500);
    assert_eq!(parallel_state.get_balance(&receiver, &TOS_ASSET), 500);
    assert_eq!(parallel_state.get_nonce(&sender), 1);

    println!("✅ Parallel transfer test passed");
}
```

### Example 2: Multiple Account Setup

```rust
use tos_testing_integration::{MockStorage, setup_account_mock};

#[tokio::test]
async fn test_bulk_account_operations() {
    // 1. Create storage
    let storage = MockStorage::new_with_tos_asset();

    // 2. Setup 50 accounts
    let accounts: Vec<_> = (0..50)
        .map(|i| {
            let account = create_test_account(i);
            setup_account_mock(&storage, &account, i as u64 * 1000, i as u64);
            account
        })
        .collect();

    // 3. Create parallel state
    let storage_arc = Arc::new(parking_lot::RwLock::new(storage));
    let parallel_state = ParallelChainState::new(storage_arc, 0).await.unwrap();

    // 4. Verify all accounts
    for (i, account) in accounts.iter().enumerate() {
        let balance = parallel_state.get_balance(account, &TOS_ASSET);
        let nonce = parallel_state.get_nonce(account);

        assert_eq!(balance, i as u64 * 1000, "Account {} balance mismatch", i);
        assert_eq!(nonce, i as u64, "Account {} nonce mismatch", i);
    }

    println!("✅ Bulk account test passed: {} accounts verified", accounts.len());
}
```

### Example 3: Complex State Scenarios

```rust
use tos_testing_integration::{MockStorage, setup_account_mock};
use tos_daemon::core::state::parallel_chain_state::ParallelChainState;

#[tokio::test]
async fn test_parallel_multi_asset_transfers() {
    // 1. Setup storage with multiple assets
    let storage = MockStorage::new_with_tos_asset();

    let custom_asset = create_test_asset();
    storage.register_asset(&custom_asset, 8, "CUSTOM");

    // 2. Setup accounts with multiple assets
    let account_a = create_test_account(1);
    let account_b = create_test_account(2);

    setup_account_mock(&storage, &account_a, 1000, 0);  // TOS balance
    setup_account_mock(&storage, &account_b, 2000, 0);

    // Add custom asset balances
    storage.setup_balance(&account_a, &custom_asset, 5000);
    storage.setup_balance(&account_b, &custom_asset, 3000);

    // 3. Create parallel state
    let storage_arc = Arc::new(parking_lot::RwLock::new(storage));
    let parallel_state = ParallelChainState::new(storage_arc, 0).await.unwrap();

    // 4. Execute multi-asset transfers
    // Transfer 1: A sends 100 TOS to B
    parallel_state.sub_balance(&account_a, &TOS_ASSET, 100).unwrap();
    parallel_state.add_balance(&account_b, &TOS_ASSET, 100);

    // Transfer 2: A sends 500 CUSTOM to B
    parallel_state.sub_balance(&account_a, &custom_asset, 500).unwrap();
    parallel_state.add_balance(&account_b, &custom_asset, 500);

    parallel_state.increment_nonce(&account_a).unwrap();

    // 5. Verify final state
    assert_eq!(parallel_state.get_balance(&account_a, &TOS_ASSET), 900);
    assert_eq!(parallel_state.get_balance(&account_b, &TOS_ASSET), 2100);
    assert_eq!(parallel_state.get_balance(&account_a, &custom_asset), 4500);
    assert_eq!(parallel_state.get_balance(&account_b, &custom_asset), 3500);

    // 6. Verify only one nonce increment
    assert_eq!(parallel_state.get_nonce(&account_a), 1);
    assert_eq!(parallel_state.get_nonce(&account_b), 0);

    println!("✅ Multi-asset transfer test passed");
}
```

### Example 4: Version Tracking Validation

```rust
#[tokio::test]
async fn test_version_tracking_correctness() {
    let storage = MockStorage::new_with_tos_asset();

    let modified_account = create_test_account(1);
    let unchanged_account = create_test_account(2);

    setup_account_mock(&storage, &modified_account, 1000, 5);
    setup_account_mock(&storage, &unchanged_account, 2000, 10);

    let storage_arc = Arc::new(parking_lot::RwLock::new(storage));
    let parallel_state = ParallelChainState::new(storage_arc, 0).await.unwrap();

    // Modify only modified_account
    parallel_state.add_balance(&modified_account, &TOS_ASSET, 500);
    parallel_state.increment_nonce(&modified_account).unwrap();

    // Get modified state
    let modified_balances = parallel_state.get_modified_balances();
    let modified_nonces = parallel_state.get_modified_nonces();

    // Verify only modified_account appears
    assert_eq!(modified_balances.len(), 1, "Only one balance should be modified");
    assert_eq!(modified_nonces.len(), 1, "Only one nonce should be modified");

    let (modified_account_key, _) = &modified_balances[0];
    assert_eq!(modified_account_key, &modified_account);

    let (modified_nonce_key, _) = &modified_nonces[0];
    assert_eq!(modified_nonce_key, &modified_account);

    println!("✅ Version tracking test passed");
}
```

---

## API Reference

### Storage Helpers

#### create_test_storage()

Creates a temporary sled storage instance with TOS asset registered.

```rust
pub async fn create_test_storage() -> Arc<tokio::sync::RwLock<SledStorage>>
```

**Returns**: Arc-wrapped RwLock-protected SledStorage

**Example**:
```rust
let storage = create_test_storage().await;
```

**Cleanup**: Automatic (uses `TempDir` which deletes on drop)

#### setup_account_safe()

Setup account state in sled storage WITHOUT deadlock risk.

```rust
pub async fn setup_account_safe(
    storage: &Arc<tokio::sync::RwLock<SledStorage>>,
    account: &CompressedPublicKey,
    balance: u64,
    nonce: u64,
) -> Result<(), BlockchainError>
```

**Parameters**:
- `storage`: Arc-wrapped storage instance
- `account`: Account public key
- `balance`: Initial TOS balance (in nanoTOS)
- `nonce`: Initial nonce value

**Returns**: `Result<(), BlockchainError>`

**Safety**: Includes 10ms delay after write to let sled complete internal flush

**Example**:
```rust
let account = create_test_account(1);
setup_account_safe(&storage, &account, 1000, 0).await?;
```

#### flush_storage_and_wait()

Force flush sled storage and wait for completion.

```rust
pub async fn flush_storage_and_wait(storage: &Arc<tokio::sync::RwLock<SledStorage>>)
```

**Parameters**:
- `storage`: Arc-wrapped storage instance

**Behavior**:
- Spawns blocking task to flush sled
- Waits 50ms for flush completion
- Adds 50ms safety delay for LRU cache settling

**When to use**: Always call this AFTER all account setup and BEFORE creating `ParallelChainState`

**Example**:
```rust
setup_account_safe(&storage, &account_a, 1000, 0).await?;
setup_account_safe(&storage, &account_b, 2000, 0).await?;
flush_storage_and_wait(&storage).await;  // ← CRITICAL!
let parallel_state = ParallelChainState::new(storage, 0).await?;
```

#### create_test_storage_with_accounts()

Create test storage with multiple accounts pre-setup.

```rust
pub async fn create_test_storage_with_accounts(
    accounts: Vec<(PublicKey, u64, u64)>
) -> Arc<tokio::sync::RwLock<SledStorage>>
```

**Parameters**:
- `accounts`: Vector of `(account, balance, nonce)` tuples

**Returns**: Arc-wrapped storage with all accounts setup and flushed

**Example**:
```rust
let accounts = vec![
    (account_1, 1000, 0),
    (account_2, 2000, 5),
    (account_3, 3000, 10),
];
let storage = create_test_storage_with_accounts(accounts).await;
```

### MockStorage

#### MockStorage::new()

Create new empty MockStorage instance.

```rust
pub fn new() -> Self
```

**Returns**: MockStorage with empty state

**Note**: TOS asset NOT registered. Use `new_with_tos_asset()` instead for most tests.

#### MockStorage::new_with_tos_asset()

Create MockStorage with TOS asset pre-registered.

```rust
pub fn new_with_tos_asset() -> Self
```

**Returns**: MockStorage with TOS asset registered at topoheight 0

**Example**:
```rust
let storage = MockStorage::new_with_tos_asset();
```

#### setup_account_mock()

Setup account in MockStorage.

```rust
pub fn setup_account_mock(
    storage: &MockStorage,
    account: &PublicKey,
    balance: u64,
    nonce: u64,
)
```

**Parameters**:
- `storage`: MockStorage reference
- `account`: Account public key
- `balance`: Initial TOS balance
- `nonce`: Initial nonce

**Example**:
```rust
let storage = MockStorage::new_with_tos_asset();
setup_account_mock(&storage, &account, 1000, 0);
```

**Safety**: No deadlock risk (in-memory HashMap)

#### setup_account_mock_at_topoheight()

Setup account at specific topoheight.

```rust
pub fn setup_account_mock_at_topoheight(
    storage: &MockStorage,
    account: &PublicKey,
    balance: u64,
    nonce: u64,
    topoheight: u64,
)
```

**Use case**: Testing versioned state at different blockchain heights

### Test Utilities

#### create_test_account()

Create deterministic test account from ID.

```rust
pub fn create_test_account(id: u8) -> CompressedPublicKey
```

**Parameters**:
- `id`: Account ID (0-255)

**Returns**: Deterministic public key for testing

**Example**:
```rust
let account_1 = create_test_account(1);
let account_2 = create_test_account(2);
// account_1 and account_2 are always the same across test runs
```

#### create_test_asset()

Create test asset hash.

```rust
pub fn create_test_asset() -> Hash
```

**Returns**: Deterministic asset hash for testing

#### setup_multiple_accounts()

Setup multiple accounts in MockStorage.

```rust
pub fn setup_multiple_accounts(
    storage: &MockStorage,
    balances: Vec<u64>,
) -> Vec<PublicKey>
```

**Parameters**:
- `storage`: MockStorage reference
- `balances`: Vector of initial balances

**Returns**: Vector of created account public keys

**Example**:
```rust
let storage = MockStorage::new_with_tos_asset();
let accounts = setup_multiple_accounts(&storage, vec![1000, 2000, 3000]);
// accounts[0] has balance 1000
// accounts[1] has balance 2000
// accounts[2] has balance 3000
```

---

## Troubleshooting

### Problem: Test Still Times Out

**Symptoms**:
- Test hangs after calling `ParallelChainState::new()`
- No error message, just timeout
- CPU usage near 0%

**Diagnosis**:
```bash
# Check if test is using safe helpers
rg "setup_account_safe|flush_storage_and_wait" daemon/tests/your_test.rs

# If not found, test is using unsafe pattern
```

**Solution 1**: Add `flush_storage_and_wait()`
```rust
// BEFORE
setup_account_safe(&storage, &account, 1000, 0).await?;
let parallel_state = ParallelChainState::new(storage, 0).await?;  // ← Times out

// AFTER
setup_account_safe(&storage, &account, 1000, 0).await?;
flush_storage_and_wait(&storage).await;  // ← Add this!
let parallel_state = ParallelChainState::new(storage, 0).await?;  // ← Works
```

**Solution 2**: Switch to MockStorage
```rust
// If test doesn't need real storage, use MockStorage
let storage = MockStorage::new_with_tos_asset();
setup_account_mock(&storage, &account, 1000, 0);
let storage_arc = Arc::new(RwLock::new(storage));
let parallel_state = ParallelChainState::new(storage_arc, 0).await?;
```

### Problem: Test Passes Locally but Fails in CI

**Symptoms**:
- Test passes on developer machine
- Test times out in CI/CD
- Inconsistent results

**Cause**: Timing-dependent race condition (10ms delay insufficient under CI load)

**Solution**: Increase flush delay
```rust
// In storage_helpers.rs, increase delay:
tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;  // Was 10ms

// Or call flush_storage_and_wait() explicitly (adds 100ms total)
flush_storage_and_wait(&storage).await;
```

### Problem: Balance/Nonce Not Found After Setup

**Symptoms**:
- `setup_account_safe()` completes without error
- `ParallelChainState::new()` loads successfully
- Account balance is 0 instead of expected value

**Cause**: Account not registered in storage

**Solution**: Check account registration
```rust
// setup_account_safe() should call this internally:
storage_write.set_account_registration_topoheight(&account, 0).await?;

// If using MockStorage, ensure TOS asset is registered:
let storage = MockStorage::new_with_tos_asset();  // ← Use this, not new()
```

### Problem: MockStorage Doesn't Compile with Blockchain

**Symptoms**:
```
error: the trait `Storage` is not implemented for `MockStorage`
```

**Cause**: MockStorage is NOT a full `Storage` implementation (by design)

**Solution**: Use real storage for blockchain tests
```rust
// BAD: MockStorage can't be used as full Storage
let storage = MockStorage::new();
let blockchain = Blockchain::new(storage)?;  // ❌ Won't compile

// GOOD: Use SledStorage for blockchain
let storage = create_test_storage().await;
let blockchain = Blockchain::new(storage)?;  // ✅ Works
```

### Problem: Test Fails with "TOS asset not found"

**Symptoms**:
```
Error: Asset TOS not found in storage
```

**Cause**: TOS asset not registered in storage

**Solution**:
```rust
// For SledStorage:
let storage = create_test_storage().await;  // ← Automatically registers TOS

// For MockStorage:
let storage = MockStorage::new_with_tos_asset();  // ← Use this!

// Not this:
let storage = MockStorage::new();  // ❌ TOS not registered
```

### Problem: Nonce Increment Fails

**Symptoms**:
```
Error: Cannot increment nonce for unregistered account
```

**Cause**: Account not registered before increment

**Solution**: Ensure account setup includes registration
```rust
// setup_account_safe() handles this automatically
setup_account_safe(&storage, &account, 1000, 0).await?;

// For MockStorage:
setup_account_mock(&storage, &account, 1000, 0);
// Internally calls: storage.set_account_registration_topoheight(account, 0)
```

---

## Best Practices

### 1. Always Use Safe Helpers

❌ **DON'T** manually write to storage:
```rust
let mut write = storage.write().await;
write.set_last_balance_to(&account, &TOS_ASSET, 0, ...).await?;
```

✅ **DO** use safe helpers:
```rust
setup_account_safe(&storage, &account, balance, nonce).await?;
```

### 2. Always Flush Before ParallelChainState

❌ **DON'T** skip flushing:
```rust
setup_account_safe(&storage, &account, 1000, 0).await?;
let parallel_state = ParallelChainState::new(storage, 0).await?;  // ← Risk!
```

✅ **DO** flush explicitly:
```rust
setup_account_safe(&storage, &account, 1000, 0).await?;
flush_storage_and_wait(&storage).await;  // ← Safe!
let parallel_state = ParallelChainState::new(storage, 0).await?;
```

### 3. Use MockStorage for Unit Tests

❌ **DON'T** use sled for simple unit tests:
```rust
// Overkill for unit test
let temp_dir = TempDir::new("test")?;
let storage = SledStorage::new(temp_dir.path(), ...)?;
```

✅ **DO** use MockStorage:
```rust
// Fast and simple
let storage = MockStorage::new_with_tos_asset();
```

### 4. Use Real Storage for Integration Tests

❌ **DON'T** use MockStorage for blockchain tests:
```rust
let storage = MockStorage::new();
let blockchain = Blockchain::new(storage)?;  // ❌ Won't compile
```

✅ **DO** use real storage:
```rust
let storage = create_test_storage().await;
let blockchain = Blockchain::new(storage)?;  // ✅ Works
```

### 5. Create Deterministic Test Accounts

❌ **DON'T** use random accounts:
```rust
let account = Keypair::generate().public_key();  // ← Different each run
```

✅ **DO** use deterministic helpers:
```rust
let account = create_test_account(1);  // ← Same every run
```

### 6. Document Deadlock Fixes in Comments

✅ **DO** add comments explaining fixes:
```rust
// DEADLOCK FIX: Use setup_account_safe() instead of manual storage writes
// to avoid sled LRU cache deadlock during parallel state creation
setup_account_safe(&storage, &account, 1000, 0).await?;
flush_storage_and_wait(&storage).await;
let parallel_state = ParallelChainState::new(storage, 0).await?;
```

### 7. Verify Test Reliability

✅ **DO** run tests multiple times:
```bash
# Run test 10 times to check for flakiness
for i in {1..10}; do cargo test test_name -- --nocapture || break; done
```

### 8. Use Descriptive Test Names

❌ **DON'T** use vague names:
```rust
#[tokio::test]
async fn test_1() { ... }
```

✅ **DO** use descriptive names:
```rust
#[tokio::test]
async fn test_parallel_transfer_maintains_total_supply() { ... }
```

### 9. Add Success Messages

✅ **DO** add clear success indicators:
```rust
#[tokio::test]
async fn test_something() {
    // ... test logic ...

    println!("✅ Test passed: {}", test_description);
}
```

### 10. Keep Tests Focused

❌ **DON'T** test multiple things in one test:
```rust
#[tokio::test]
async fn test_everything() {
    // Tests transfers, nonces, balances, assets, all at once
}
```

✅ **DO** write focused tests:
```rust
#[tokio::test]
async fn test_parallel_transfer_balance_update() { ... }

#[tokio::test]
async fn test_parallel_nonce_increment() { ... }

#[tokio::test]
async fn test_parallel_multi_asset_support() { ... }
```

---

## Summary

### Quick Reference Card

**For Parallel Execution Tests (Real Storage)**:
```rust
let storage = create_test_storage().await;
setup_account_safe(&storage, &account, 1000, 0).await?;
flush_storage_and_wait(&storage).await;
let parallel_state = ParallelChainState::new(storage, 0).await?;
```

**For Parallel Execution Tests (MockStorage)**:
```rust
let storage = MockStorage::new_with_tos_asset();
setup_account_mock(&storage, &account, 1000, 0);
let storage_arc = Arc::new(RwLock::new(storage));
let parallel_state = ParallelChainState::new(storage_arc, 0).await?;
```

**For Multiple Accounts**:
```rust
let accounts = setup_multiple_accounts(&storage, vec![1000, 2000, 3000]);
```

### When to Use What

| Use Case | Use This | Don't Use |
|----------|----------|-----------|
| Parallel execution unit test | `MockStorage` | `SledStorage` |
| Parallel execution with 10+ accounts | `MockStorage` | `SledStorage` |
| Integration test with blockchain | `SledStorage + setup_account_safe()` | `MockStorage` |
| Stress test | `SledStorage + create_test_storage_with_accounts()` | Manual setup |
| Simple unit test | `MockStorage` | `SledStorage` |

### Migration Checklist

- [ ] Identify test marked `#[ignore]` or timing out
- [ ] Choose migration strategy (MockStorage or setup_account_safe)
- [ ] Update imports
- [ ] Replace manual storage writes with safe helpers
- [ ] Add `flush_storage_and_wait()` if using SledStorage
- [ ] Remove `#[ignore]` attribute
- [ ] Run test 10 times to verify reliability
- [ ] Add success message to test output
- [ ] Document fix in commit message

---

**Document Version**: 1.0
**Last Updated**: 2025-10-30
**Maintainer**: TOS Development Team
**Questions?**: See `/daemon/tests/common/mod.rs` for implementation details

---

## Additional Resources

- **Implementation Status**: `TOS_INTEGRATION_TEST_FRAMEWORK_IMPLEMENTATION_STATUS.md`
- **Framework Analysis**: `INTEGRATION_TEST_FRAMEWORK_ANALYSIS.md`
- **Example Tests**: `testing-integration/tests/parallel_execution_example.rs`
- **Helper Source**: `daemon/tests/common/mod.rs`
- **Claude Code Rules**: `CLAUDE.md` (project coding standards)

---

*This guide is part of the TOS blockchain testing infrastructure. For questions or improvements, please open an issue or PR on GitHub.*
