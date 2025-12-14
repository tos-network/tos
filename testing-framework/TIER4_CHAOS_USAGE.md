# Tier 4 Chaos Testing - Usage Guide

## Overview

The Tier 4 chaos testing module provides property-based testing and chaos scenarios to verify blockchain invariants under random, extreme, and edge-case conditions.

## Running Chaos Tests

### Enable the Chaos Feature

Chaos tests are feature-gated and require the `chaos` feature flag:

```bash
# Run all tests including chaos tests
cargo test --package tos-testing-framework --features chaos

# Run only tier4 chaos tests
cargo test --package tos-testing-framework --lib tier4_chaos --features chaos

# Run specific chaos test
cargo test --package tos-testing-framework --features chaos -- prop_nonce_never_decreases
```

### Without Chaos Feature

Without the `chaos` feature, the framework includes 214 tests:

```bash
# Run base tests (no chaos)
cargo test --package tos-testing-framework --lib
```

## Property-Based Tests

Property-based tests use `proptest` to verify invariants across randomly generated scenarios.

### Available Properties

1. **prop_transaction_order_independence**
   - Verifies that transaction ordering doesn't affect final balance
   - Tests: 1-10 transactions with random amounts
   - Invariant: Final balance is deterministic

2. **prop_empty_blocks_preserve_balances**
   - Verifies that empty blocks don't change balances
   - Tests: 1-50 empty blocks
   - Invariant: Balances remain unchanged

3. **prop_supply_accounting_invariant**
   - Verifies economic invariant: supply = balances + fees burned
   - Tests: 1-20 transactions
   - Invariant: Total balances match counter

4. **prop_nonce_never_decreases**
   - Verifies nonce monotonicity
   - Tests: 2-30 transactions
   - Invariant: Nonces always increase

5. **prop_height_monotonicity**
   - Verifies block height always increases
   - Tests: 1-50 blocks
   - Invariant: Height increases with each block

6. **prop_invalid_transactions_rejected**
   - Verifies invalid transactions are rejected
   - Tests: Insufficient balance, wrong nonce
   - Invariant: Validation catches errors

## Standard Chaos Tests

Standard async tests for high-volume and concurrent scenarios:

1. **test_high_transaction_volume**
   - Submits 100 transactions in a single block
   - Verifies: All transactions processed, nonce advances correctly

2. **test_zero_balance_transfers**
   - Tests zero-amount transfers with fees
   - Verifies: Transfers allowed, only fee deducted

3. **test_concurrent_block_mining**
   - Mines 10 blocks concurrently
   - Verifies: Internal lock serialization, correct final height

## Reproducing Test Failures

When a proptest fails, it provides a seed for reproduction:

```
Test failed with seed: 0xa3f5c8e1b2d94706
```

Reproduce the exact failure:

```bash
PROPTEST_RNG_SEED=0xa3f5c8e1b2d94706 cargo test --features chaos prop_nonce_never_decreases
```

## Configuration

### Proptest Configuration

Default configuration in `property_tests.rs`:
- Number of test cases: 100 (proptest default)
- Shrinking: Enabled (finds minimal failing case)
- Timeout: Default proptest timeout

To customize, use environment variables:

```bash
# Run more test cases
PROPTEST_CASES=1000 cargo test --features chaos

# Disable shrinking
PROPTEST_MAX_SHRINK_ITERS=0 cargo test --features chaos
```

## Test Design Principles

### 1. Determinism
- Uses `SystemClock` instead of `PausedClock` for proptest compatibility
- Multi-threaded runtime compatible

### 2. Invariant-Based
- Tests verify properties, not specific outcomes
- Focus on "what should always be true"

### 3. Reproducibility
- All tests use seeded RNG
- Failures can be exactly reproduced

## Writing New Chaos Tests

### Property-Based Test Template

```rust
proptest! {
    /// Property: Your invariant description
    #[test]
    fn prop_your_test_name(
        param1 in strategy1(),
        param2 in strategy2(),
    ) {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            // Setup
            let clock = Arc::new(SystemClock);
            let blockchain = TestBlockchainBuilder::new()
                .with_clock(clock)
                .with_funded_account(address, balance)
                .build()
                .await
                .map_err(|e| TestCaseError::fail(e.to_string()))?;

            // Execute test logic
            
            // Assert invariant
            prop_assert!(condition, "Error message");

            Ok::<(), TestCaseError>(())
        })?;
    }
}
```

### Standard Chaos Test Template

```rust
#[tokio::test]
async fn test_your_chaos_scenario() {
    let clock = Arc::new(SystemClock);
    let blockchain = TestBlockchainBuilder::new()
        .with_clock(clock)
        .with_funded_account(address, balance)
        .build()
        .await
        .unwrap();

    // Your chaos scenario
    
    // Assertions
    assert_eq!(actual, expected);
}
```

## Common Strategies

Available in `tier2_integration/strategies.rs`:

```rust
use crate::tier2_integration::strategies::*;

arb_balance()  // Random balance (1..=10_000_000)
arb_amount()   // Random transfer amount (1..=1_000_000)
arb_fee()      // Random fee (100..=10_000)
```

## Limitations

### Disabled Tests

- `prop_consensus_convergence`: Disabled due to runtime incompatibility
  - Proptest uses multi-threaded runtime
  - LocalTosNetworkBuilder uses PausedClock requiring single-threaded runtime
  - TODO: Re-enable when runtime compatibility is resolved

## Performance

- Property tests run 100 cases each by default
- Total chaos test suite: ~0.57s for all tests
- Individual property tests: 10-50ms each

## Best Practices

1. **Use Appropriate Strategies**
   - Use existing strategies from `tier2_integration/strategies.rs`
   - Keep generated values realistic

2. **Write Clear Assertions**
   - Use `prop_assert!` with descriptive messages
   - Include actual and expected values in error messages

3. **Handle Errors Properly**
   - Use `map_err` to convert to `TestCaseError`
   - Provide context in error messages

4. **Keep Tests Fast**
   - Use `SystemClock` (no time advancement needed)
   - Limit transaction counts to reasonable numbers

## Troubleshooting

### Test Timeout

If tests timeout, reduce the number of operations:

```rust
// Instead of
tx_count in 1usize..100usize,

// Use
tx_count in 1usize..20usize,
```

### Runtime Panics

If you see "time already frozen" panics:
- Ensure using `SystemClock` not `PausedClock`
- Check tokio runtime configuration

### Proptest Failures

When proptest finds a failure:
1. Note the seed from output
2. Reproduce with `PROPTEST_RNG_SEED=<seed>`
3. Debug the minimal failing case

## References

- [Proptest Documentation](https://docs.rs/proptest)
- [TOS Testing Framework README](README.md)
- [Property Tests Source](src/tier4_chaos/property_tests.rs)

---

**Last Updated**: 2025-11-15
**Version**: v3.0.3
