# await_holding_lock Fix Plan

## Issue Statistics

- **Total**: 192 `await_holding_lock` warnings
- **Inherited from Xelis**: 74
- **TOS additions**: 118 ⚠️

## Risk Level: 🔴 High

Performing `.await` operations while holding locks can lead to:
- Deadlocks
- Severe performance degradation
- Concurrency race conditions
- Blockchain synchronization failures

## Fix Strategy

### Phase 1: Automatic Fix (Try First)

```bash
# Attempt automatic fix
cargo clippy --fix --allow-dirty --allow-staged -- -W clippy::await_holding_lock
```

### Phase 2: Manual Fix Patterns

#### Pattern A: Release Lock Early

```rust
// ❌ Wrong
let data = lock.lock().unwrap();
some_async_fn().await;
drop(data);

// ✅ Correct
let data = {
    let data = lock.lock().unwrap();
    data.clone()
}; // Lock automatically released here
some_async_fn().await;
```

#### Pattern B: Use Async-Aware Locks

```rust
// ❌ Wrong: Using std::sync::Mutex
use std::sync::Mutex;
let lock = Mutex::new(data);

// ✅ Correct: Using tokio::sync::Mutex
use tokio::sync::Mutex;
let lock = Mutex::new(data);
let guard = lock.lock().await;
some_async_fn().await;
```

#### Pattern C: Reduce Lock Scope

```rust
// ❌ Wrong: Lock scope too large
let guard = lock.lock().unwrap();
let value = guard.get_value();
let result = process(value).await;

// ✅ Correct: Only hold lock when necessary
let value = {
    let guard = lock.lock().unwrap();
    guard.get_value().clone()
};
let result = process(value).await;
```

## Estimated Workload

- **Phase 1 Automatic Fix**: May fix 30-50% (60-96 instances)
- **Phase 2 Manual Fix**: Remaining 96-132 instances
- **Total Time**: 3-5 days

## Automatic Fix Result ❌

Attempted `cargo clippy --fix` but failed:
- Clippy attempted fixes but introduced compilation errors
- Error: Generic parameter removal caused type mismatches
- Conclusion: **Must fix manually**

## Manual Fix Strategy

### Priority Order

1. **High Priority**: 118 TOS additions (recent code)
2. **Medium Priority**: 74 inherited from Xelis

### Execution Steps

1. ✅ Create fix branch and push
2. ✅ Attempt automatic fix (failed)
3. ⏳ Fix manually one by one (requires 3-5 days)
4. ⏳ Run tests to verify each batch of fixes
5. ⏳ Submit PR after all fixes complete
6. ⏳ Code review and merge to main

## Next Actions

**Recommendation**: Due to needing to manually fix 192 issues, suggest phased approach:

### Phase 1: Fix Critical Modules (1-2 days)
- `daemon/src/core/blockchain.rs` - Blockchain core
- `daemon/src/core/mempool.rs` - Transaction pool
- `daemon/src/rpc/rpc.rs` - RPC interface

### Phase 2: Fix TAKO Related (1 day)
- `daemon/src/tako_integration/` - TAKO VM integration

### Phase 3: Fix Other Modules (1-2 days)
- Remaining daemon and wallet modules

Submit after each phase completes for incremental review.

## References

- Clippy documentation: https://rust-lang.github.io/rust-clippy/master/index.html#await_holding_lock
- Tokio sync primitives: https://docs.rs/tokio/latest/tokio/sync/
