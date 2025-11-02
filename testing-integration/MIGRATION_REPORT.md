# Parallel Execution Test Migration Report

**Date**: 2025-10-30
**Task**: Migrate 3-5 parallel execution tests from ignored state to working state
**Status**: ❌ Blocked by architectural limitation

## Summary

Attempted to migrate 5 ignored parallel execution tests to use the safe testing framework (MockStorage). The migration revealed a **fundamental architectural limitation**: `ParallelChainState` requires a full `Storage` trait implementation, but `MockStorage` only implements individual storage providers (`NonceProvider`, `BalanceProvider`, etc.) to avoid sled deadlocks.

## Background

The parallel execution tests in `daemon/tests/` were marked `#[ignore]` due to sled deadlock issues. The FIXME comments stated:

> FIXME: This test times out due to test infrastructure issues with versioned balance storage setup.
> The test creates two separate storage instances and manually writes versioned balances, which
> triggers sled deadlocks. Production code works correctly - other parallel execution tests pass.

The `testing-integration` package was created to provide:
- **MockStorage**: In-memory storage backend avoiding sled deadlocks
- Safe helper functions like `setup_account_mock()`
- Test utilities

## Tests Identified for Migration

1. **test_parallel_matches_sequential_receive_then_spend** (parity_tests.rs:454-552)
   - Tests Alice → Bob → Charlie transfer chain
   - Verifies parallel execution produces same state as sequential

2. **test_parallel_matches_sequential_multiple_spends** (parity_tests.rs:558-648)
   - Tests Alice sending to both Bob and Charlie in same block
   - Verifies nonce increment and output_sum handling

3. **test_parallel_preserves_receiver_balance** (security_tests.rs:367-498)
   - Security Test #2: Verifies balances are incremented not overwritten
   - Bob has 500 TOS, receives 1 TOS → should have 501 TOS (not 1 TOS)

4. **test_parallel_deducts_fees** (security_tests.rs:510-650)
   - Security Test #3: Verifies transaction fees are deducted from sender
   - Alice sends 1 TOS with 10 nanoTOS fee → deduction should be 1.00000010 TOS

5. **test_double_spend_prevention** (new test)
   - Demonstrates safe testing pattern for error conditions
   - Alice attempts to spend 100 TOS twice with only 100 TOS balance

## Migration Attempts

### Created Files

```
testing-integration/tests/
├── mod.rs                               # Test module organization
├── helpers.rs                           # Shared helper functions
├── migrated_receive_then_spend.rs       # Migrated test #1
├── migrated_multiple_spends.rs          # Migrated test #2
├── migrated_balance_preservation.rs     # Migrated test #3
├── migrated_fee_deduction.rs            # Migrated test #4
└── migrated_double_spend_prevention.rs  # Migrated test #5
```

### Migration Pattern

The migration followed this pattern:

**BEFORE (with sled deadlocks):**
```rust
// Create two separate sled storage instances
let storage_seq = Arc::new(RwLock::new(create_storage()));
let storage_par = Arc::new(RwLock::new(create_storage()));

// Manual versioned balance writes → deadlock!
storage_seq.write().await
    .set_last_balance_to(&account, &asset, 0, &VersionedBalance::new(1000, Some(0)))
    .await?;
```

**AFTER (attempted with MockStorage):**
```rust
// Create MockStorage (in-memory, no deadlocks)
let storage = MockStorage::new_with_tos_asset();

// Simple helper function → no deadlocks!
setup_account_mock(&storage, &account, 1000, 0);

// Create ParallelChainState
let parallel_state = create_parallel_state(storage).await?;
```

## The Blocking Issue

### Compilation Error

```
error[E0277]: the trait bound `MockStorage: Storage` is not satisfied
  --> testing-integration/tests/helpers.rs:48:26
   |
48 |     let parallel_state = ParallelChainState::new(
   |                          ^^^^^^^^^^^^^^^^^^^^^^^ the trait `Storage` is not implemented for `MockStorage`
   |
   = help: the following other types implement trait `Storage`:
             RocksStorage
             SledStorage
```

### Root Cause

`ParallelChainState` requires a full `Storage` trait:

```rust
// daemon/src/core/state/parallel_chain_state.rs:78
pub struct ParallelChainState<S: Storage> {
    storage: Arc<RwLock<S>>,
    // ...
}

impl<S: Storage> ParallelChainState<S> {
    pub async fn new(
        storage: Arc<RwLock<S>>,
        environment: Arc<Environment>,
        stable_topoheight: TopoHeight,
        topoheight: TopoHeight,
        block_version: BlockVersion,
        block, hash: Hash,
    ) -> Arc<Self> {
        // ...
    }
}
```

But `MockStorage` only implements individual providers:

```rust
// testing-integration/src/storage/mock_storage.rs
impl NonceProvider for MockStorage { /* ... */ }
impl BalanceProvider for MockStorage { /* ... */ }
impl AssetProvider for MockStorage { /* ... */ }
impl AccountProvider for MockStorage { /* ... */ }
impl MultiSigProvider for MockStorage { /* ... */ }

// NOTE: Does NOT implement Storage trait
// Storage trait requires ~15+ additional providers:
// - BlockDagProvider
// - GhostdagDataProvider
// - BlockHeaderProvider
// - TransactionsProvider
// - ... and many more
```

The comment in `mock_storage.rs:660-669` explicitly states:

> NOTE: MockStorage does NOT implement the full Storage trait because it requires
> many additional providers (BlockDagProvider, GhostdagDataProvider, etc.) that are
> not needed for basic parallel execution tests.

## Why This Design?

### Option A: Implement Full Storage Trait for MockStorage
- ❌ **Rejected**: Would require implementing 15+ providers
- ❌ Massive implementation burden (~3000+ lines of code)
- ❌ Defeats the purpose of a "mock" storage (lightweight, focused)
- ❌ Would duplicate functionality from real storage backends

### Option B: Relax ParallelChainState Requirements
- ❌ **Not feasible**: ParallelChainState needs multiple providers
- ❌ Changes would ripple through production code
- ❌ Would complicate the production API

### Option C: Tests Use Real Storage (Current Approach)
- ✅ **Current state**: Tests use `SledStorage` or `RocksStorage`
- ❌ **Problem**: Real storage triggers deadlocks in test scenarios
- ❌ Why deadlocks occur:
  - Tests create multiple storage instances
  - Manual `set_last_balance_to()` calls
  - Sled's internal locking conflicts with concurrent access patterns

## Alternative Approaches Explored

###  1. Use example tests (parallel_execution_example.rs)

The example tests use `parking_lot::RwLock` and call `ParallelChainState::new(storage, 0)`:

```rust
// testing-integration/tests/parallel_execution_example.rs:85
let storage_arc = Arc::new(RwLock::new(storage.clone()));
let parallel_state = ParallelChainState::new(storage_arc.clone(), 0).await?;
```

**Problem**: This API signature doesn't match the actual implementation:
```rust
// Actual signature requires 7 parameters:
pub async fn new(
    storage: Arc<RwLock<S>>,
    environment: Arc<Environment>,
    stable_topoheight: TopoHeight,
    topoheight: TopoHeight,
    block_version: BlockVersion,
    block: Block,
    block_hash: Hash,
) -> Arc<Self>
```

**Conclusion**: The example tests are outdated and don't compile with the current API.

### 2. Implement Storage Trait for MockStorage

Would require implementing all of these providers:
- `BlockDagProvider`
- `GhostdagDataProvider`
- `BlockHeaderProvider`
- `TransactionsProvider`
- `RegistrationProvider`
- `DifficultyProvider`
- `PruningProvider`
- `MerkleProvider`
- `... and 7+ more`

**Estimate**: ~3000+ lines of boilerplate code.

**Decision**: Not worth the effort for test infrastructure.

### 3. Simplify Tests to Not Use ParallelChainState

The migrated tests I created directly test the state operations without going through `ParallelChainState`:

```rust
// Instead of:
let parallel_state = ParallelChainState::new(...).await?;
parallel_state.sub_balance(&alice, &TOS_ASSET, 100)?;

// Do:
let storage = MockStorage::new_with_tos_asset();
storage.set_last_balance_to(&alice, &TOS_ASSET, 0, &VersionedBalance::new(900, Some(1))).await?;
```

**Problem**: This doesn't test `ParallelChainState` itself, which is the whole point!

## Recommendations

### Short Term (Immediate Action)

1. **Document the limitation** ✅ (this report)
2. **Keep the ignored tests as-is** for now
3. **Update example tests** to match current API or remove them
4. **Add explanation** to test FIXME comments pointing to this report

### Medium Term (Next Sprint)

1. **Investigate sled deadlock root cause**
   - Profile sled lock acquisition during test execution
   - Identify specific code paths causing contention
   - Consider test-specific sled configuration

2. **Explore test-only workarounds**
   - Single shared storage instance instead of separate seq/par
   - Sequential test execution (disable parallelism in tests)
   - Introduce delays/yields to reduce contention

3. **Consider alternative storage backend for tests**
   - In-memory SQLite?
   - Simple HashMap-based storage with manual locking?
   - RocksDB in-memory mode?

### Long Term (Future Design)

1. **Refactor ParallelChainState to use trait objects**
   ```rust
   // Instead of generic S: Storage
   pub struct ParallelChainState {
       storage: Arc<RwLock<dyn Storage>>,
       // ...
   }
   ```
   - Would allow MockStorage to implement subset of Storage
   - Methods not implemented could panic!() or return NotImplemented

2. **Create StorageProvider trait hierarchy**
   ```rust
   // Minimal trait for parallel execution
   trait ParallelExecutionStorage:
       NonceProvider + BalanceProvider + AssetProvider + AccountProvider
   {}

   impl<S: Storage> ParallelExecutionStorage for S {}
   impl ParallelExecutionStorage for MockStorage {}
   ```

3. **Implement full MockStorage**
   - Bite the bullet and implement all 15+ providers
   - Use code generation or macros to reduce boilerplate
   - Make it the de facto test storage backend

## Files Created (For Future Reference)

The migrated test files demonstrate the desired testing pattern, even though they don't currently compile:

1. **helpers.rs** - Shared test utilities
   - `create_dummy_block()` - Generate test blocks
   - `create_parallel_state()` - Initialize ParallelChainState (blocked)

2. **migrated_receive_then_spend.rs** - Parity test
   - Alice → Bob → Charlie transfer chain
   - Verifies receive-then-spend within same block

3. **migrated_multiple_spends.rs** - Parity test
   - Alice sends to both Bob and Charlie
   - Verifies nonce sequencing

4. **migrated_balance_preservation.rs** - Security test
   - Tests Vulnerability #2 fix
   - Verifies balances increment (not overwrite)

5. **migrated_fee_deduction.rs** - Security test
   - Tests Vulnerability #3 fix
   - Verifies fees are deducted

6. **migrated_double_spend_prevention.rs** - Additional security test
   - Tests insufficient balance detection
   - Demonstrates error handling pattern

## Key Insights

1. **MockStorage was designed for unit tests**, not integration tests
   - Unit tests: Test individual provider implementations
   - Integration tests: Test full blockchain/execution flows

2. **ParallelChainState is an integration component**
   - Requires full storage capabilities
   - Can't be tested in isolation easily

3. **The sled deadlocks are a real problem**
   - Not just "test infrastructure issues"
   - Production code may encounter similar patterns
   - Should be investigated and fixed properly

4. **Test architecture needs rethinking**
   - Current approach: Real storage in tests
   - Desired approach: Mock storage for speed/reliability
   - Gap: No mock storage that works with integration components

## Conclusion

The test migration task revealed a **fundamental architectural mismatch** between the lightweight `MockStorage` design and the requirements of `ParallelChainState`. The ignored tests cannot be migrated to use `MockStorage` without either:

A) Implementing the full `Storage` trait for `MockStorage` (~3000 lines), or
B) Refactoring `ParallelChainState` to use trait objects or smaller trait bounds

The root issue (sled deadlocks) remains unsolved and should be addressed directly rather than worked around with mocks.

**Status**: Migration blocked pending architectural decisions.

**Recommended Next Step**: Investigate and fix sled deadlock root cause instead of mocking around it.
