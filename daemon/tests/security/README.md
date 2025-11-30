# TOS Blockchain Security Test Suite

**Last Updated**: December 1, 2025
**Version**: 2.0

## Overview

This directory contains comprehensive security tests for all 27 vulnerabilities discovered in the TOS blockchain security audit. **All tests are now fully active** with complete RocksDB integration.

**Audit Reference**: `../../../TIPs/SECURITY_AUDIT_REPORT.md`

## Test Organization

### Test Files

| File | Vulnerabilities | Tests | Status |
|------|----------------|-------|--------|
| `ghostdag_security_tests.rs` | V-01 to V-07 | 17 | ✅ All Active |
| `state_security_tests.rs` | V-13 to V-19 | 14 | ✅ All Active |
| `storage_security_tests.rs` | V-20 to V-27 | 13 | ✅ All Active |
| `block_submission_tests.rs` | Issue #2 | 18 | ✅ All Active |
| `websocket_pentest.rs` | Network | 12 | ✅ All Active |
| `integration_security_tests.rs` | All | 7 | ✅ 5 Active, 2 Benchmark |
| `state_transaction_integration_tests.rs` | V-13 to V-19 | 8+ | ✅ All Active |
| `test_utilities.rs` | - | - | Common test helpers and mocks |

**Note**: Cryptographic tests (V-08 to V-12) are in `common/tests/security/crypto_security_tests.rs` (19 tests).

### Total Coverage

| Metric | Value |
|--------|-------|
| **Total Vulnerabilities** | 27 |
| **Total Tests** | 100 |
| **Active Tests** | 98 (98%) |
| **Ignored Tests** | 2 (benchmarks only) |
| **RocksDB Integration** | Complete |

## Running Tests

### Run All Security Tests

```bash
# Run all security tests (active only)
cargo test --test '*' security

# Run with ignored tests (requires full implementation)
cargo test --test '*' security -- --ignored

# Run all tests including ignored
cargo test --test '*' security -- --include-ignored
```

### Run Specific Vulnerability Tests

```bash
# Test specific vulnerability
cargo test --test '*' test_v01   # V-01: Blue score overflow
cargo test --test '*' test_v03   # V-03: K-cluster validation (CRITICAL)
cargo test --test '*' test_v08   # V-08: Key validation
cargo test --test '*' test_v14   # V-14: Balance overflow/underflow

# Run all GHOSTDAG tests
cargo test --test '*' ghostdag_security

# Run all crypto tests
cargo test --package tos_common --test '*' crypto_security

# Run all state tests
cargo test --test '*' state_security

# Run all storage tests
cargo test --test '*' storage_security

# Run integration tests
cargo test --test '*' integration_security

# Run block submission tests (Issue #2 fixes)
cargo test --test '*' block_submission
```

### Run Tests by Category

```bash
# Consensus tests (V-01 to V-07)
cd daemon && cargo test --test '*' ghostdag_security

# Cryptography tests (V-08 to V-12)
cd common && cargo test --test '*' crypto_security

# State management tests (V-13 to V-19)
cd daemon && cargo test --test '*' state_security

# Storage tests (V-20 to V-27)
cd daemon && cargo test --test '*' storage_security
```

## Vulnerability Test Matrix

### GHOSTDAG Consensus (V-01 to V-07)

| ID | Vulnerability | Tests | Status | Priority |
|----|---------------|-------|--------|----------|
| V-01 | Blue score/work overflow | 2 | ✅ Active | CRITICAL |
| V-02 | Reachability interval exhaustion | 1 | ✅ Active | CRITICAL |
| V-03 | K-cluster validation bypass | 4 | ✅ Active | **CRITICAL** |
| V-04 | GHOSTDAG race condition | 1 | ✅ Active | CRITICAL |
| V-05 | Missing parent validation | 2 | ✅ Active | CRITICAL |
| V-06 | Zero difficulty division | 2 | ✅ Active | CRITICAL |
| V-07 | DAA timestamp manipulation | 3 | ✅ Active | CRITICAL |

**V-03 K-cluster validation is now FULLY ACTIVE** with all 4 tests passing.

### Cryptography (V-08 to V-12)

| ID | Vulnerability | Tests | Status | Priority |
|----|---------------|-------|--------|----------|
| V-08 | Non-standard key construction | 5 | ✅ Active | CRITICAL |
| V-09 | Missing point validation | 2 | ⚠️ Partial | CRITICAL |
| V-10 | Custom signature scheme | 2 | ✅ Active | CRITICAL |
| V-11 | Nonce race condition | 1 | ✅ Active | CRITICAL |
| V-12 | Timing side-channel | 3 | ⚠️ Partial | HIGH |

### State Management (V-13 to V-19)

| ID | Vulnerability | Tests | Status | Priority |
|----|---------------|-------|--------|----------|
| V-13 | Mempool nonce race | 1 | ✅ Active | CRITICAL |
| V-14 | Balance overflow/underflow | 3 | ✅ Active | CRITICAL |
| V-15 | Non-atomic state transactions | 2 | ✅ Active | CRITICAL |
| V-16 | Missing snapshot isolation | 1 | ✅ Active | HIGH |
| V-17 | Nonce checker desync | 1 | ✅ Active | HIGH |
| V-18 | Mempool cleanup race | 1 | ✅ Active | HIGH |
| V-19 | Nonce rollback missing | 2 | ✅ Active | HIGH |

### Storage & Concurrency (V-20 to V-27)

| ID | Vulnerability | Tests | Status | Priority |
|----|---------------|-------|--------|----------|
| V-20 | State corruption (concurrent) | 3 | ✅ Active (RocksDB) | HIGH |
| V-21 | Timestamp manipulation | 1 | ✅ Active | HIGH |
| V-22 | Missing fsync on writes | 1 | ✅ Active | HIGH |
| V-23 | Insufficient cache invalidation | 1 | ✅ Active | HIGH |
| V-24 | Tip selection gaps | 1 | ✅ Active | HIGH |
| V-25 | Concurrent balance access | 1 | ✅ Active | HIGH |
| V-26 | Unbounded orphan TX set | 1 | ✅ Active | HIGH (DoS) |
| V-27 | Skip validation on mainnet | 1 | ✅ Active | HIGH |

## Test Status Legend

- ✅ **Active**: Test runs in current implementation
- ⚠️ **Partial**: Mix of active and ignored tests
- ⏸️ **Ignored**: Requires full blockchain implementation (marked with `#[ignore]`)

## Test Implementation Status (December 2025)

### All Tests Now Active!

As of December 2025, **all security tests are now fully active**. The following blockers have been resolved:

- ✅ Full blockchain storage implementation - COMPLETE
- ✅ Complete mempool with nonce tracking - COMPLETE
- ✅ GHOSTDAG with reachability service - COMPLETE
- ✅ RocksDB integration - COMPLETE
- ✅ Concurrent block processing - COMPLETE

### Only 2 Tests Remain Ignored

Only benchmark/stress tests that require extended runtime:
1. `test_ghostdag_stress_large_dag` - Extended benchmark
2. `test_high_load_concurrent_operations` - Stress test

## Critical Test Cases

### Most Important Tests to Monitor

1. **V-03: K-cluster Validation** (`test_v03_k_cluster_validation_detects_violations`)
   - **WHY**: Core consensus security guarantee
   - **IMPACT**: Double-spend prevention
   - **STATUS**: ✅ FULLY ACTIVE (all 4 tests passing)

2. **V-04: GHOSTDAG Race Conditions** (`test_v04_ghostdag_race_condition_prevented`)
   - **WHY**: Consensus integrity under concurrency
   - **IMPACT**: Chain splits, inconsistent state
   - **STATUS**: ✅ ACTIVE

3. **V-11: Nonce Atomicity** (`test_v11_nonce_race_condition_prevented`)
   - **WHY**: Double-spend prevention
   - **IMPACT**: Transaction replay attacks
   - **STATUS**: ✅ ACTIVE

4. **V-14: Balance Safety** (`test_v14_balance_overflow_detected`)
   - **WHY**: Economic integrity
   - **IMPACT**: Supply manipulation
   - **STATUS**: ✅ ACTIVE

5. **V-15: State Atomicity** (`test_v15_state_rollback_on_tx_failure`)
   - **WHY**: State consistency
   - **IMPACT**: State corruption
   - **STATUS**: ✅ ACTIVE

6. **V-20: Concurrent Balance Updates** (`test_v20_concurrent_balance_updates_safe`)
   - **WHY**: Data integrity under concurrent access
   - **IMPACT**: Balance corruption
   - **STATUS**: ✅ ACTIVE (RocksDB integration)

## Test Utilities

The `test_utilities.rs` module provides:

### Mock Implementations
- `MockAccount`: Simulated account with balance and nonce
- `MockTransaction`: Simulated transaction
- `MockStorage`: In-memory storage for testing
- `MockBlock`: Simulated block structure
- `MockMempool`: Simulated transaction pool

### Helper Functions
- `test_hash(value)`: Create test hashes
- `test_hashes(count)`: Create multiple test hashes
- `verify_disjoint(set1, set2)`: Check set disjointness
- `BoundedCollection<T>`: Size-limited collection
- `AtomicNonceChecker`: Thread-safe nonce tracking

### Usage Example

```rust
use crate::security::test_utilities::*;

#[tokio::test]
async fn my_security_test() {
    // Use mock storage
    let storage = MockStorage::new();
    storage.set_balance("alice", 1000).await;

    // Create test transactions
    let tx = MockTransaction::new("alice".to_string(), "bob".to_string(), 100, 1);

    // Use mock mempool
    let mempool = MockMempool::new();
    mempool.add_transaction(tx).await.unwrap();

    // Assertions...
}
```

## Integration Tests

Integration tests validate security across multiple components:

1. **Double-Spend Prevention** (V-11, V-13, V-19)
   - End-to-end nonce validation
   - Mempool to blockchain flow

2. **Concurrent Processing** (V-04, V-15, V-20)
   - Parallel block processing
   - State consistency under load

3. **Complete TX Lifecycle** (V-08-V-12, V-13-V-19, V-20-V-21)
   - Key generation → signature → validation → execution
   - Full security validation pipeline

## Adding New Security Tests

### Test Naming Convention

```rust
/// V-XX: Test <vulnerability name>
///
/// Verifies that <security property>.
#[test]
fn test_vXX_<descriptive_name>() {
    // SECURITY FIX LOCATION: path/to/fix.rs:line
    // Test implementation
}
```

### Test Structure

```rust
#[tokio::test]
async fn test_vXX_vulnerability_name() {
    // 1. Setup
    let test_data = setup_test_data();

    // 2. Execute operation that should be protected
    let result = vulnerable_operation(test_data).await;

    // 3. Verify security fix works
    assert!(matches!(result, Err(SecurityError::...)),
        "Should reject vulnerable input");

    // 4. Verify valid operations still work
    let valid_result = valid_operation(test_data).await;
    assert!(valid_result.is_ok(),
        "Valid operations should succeed");
}
```

### Documentation Requirements

Each test should document:
- Vulnerability ID (V-XX)
- Security fix location (file:line)
- Attack scenario prevented
- Expected behavior

## Fuzzing

Fuzzing harnesses are in `daemon/fuzz/fuzz_targets/`:

- `ghostdag_fuzzer.rs`: Fuzz GHOSTDAG with random inputs
- (More to be added)

Run fuzzing:
```bash
cargo fuzz run ghostdag_fuzzer
```

## Performance Benchmarks

Security fixes should not significantly degrade performance:

- Checked arithmetic: < 5% overhead
- Atomic operations: < 10% overhead
- Reachability queries: < 50ms per query
- K-cluster validation: < 100ms per block

Monitor performance with:
```bash
cargo bench -- security
```

## Continuous Integration

Security tests run in CI on every commit:

```yaml
# .github/workflows/security.yml
- name: Run security tests
  run: cargo test --test '*' security

- name: Run crypto security tests
  run: cd common && cargo test --test '*' crypto_security
```

## Coverage Goals

| Component | Current | Target |
|-----------|---------|--------|
| GHOSTDAG | 70% | 90% |
| Cryptography | 85% | 95% |
| State Management | 50% | 90% |
| Storage | 40% | 85% |
| **Overall** | **60%** | **90%** |

Generate coverage:
```bash
cargo tarpaulin --test '*' --out Html --output-dir coverage/
```

## Known Limitations

### Resolved (December 2025)

All previously "ignored" tests have been activated. The following requirements are now met:

- ✅ **Full Storage Layer**: RocksDB integration complete
- ✅ **Complete Mempool**: Nonce tracking and cleanup active
- ✅ **Reachability Service**: K-cluster validation working
- ✅ **Concurrent Block Processing**: Thread-safe operations verified

### Timing Tests

Constant-time tests (V-12) are challenging:
- CPU frequency scaling affects results
- Cache effects introduce noise
- Requires many iterations for statistical significance

Consider using specialized timing analysis tools.

## Reporting Issues

If a security test fails:

1. **DO NOT** disable the test
2. Investigate the root cause
3. Verify if it's:
   - Regression of fixed vulnerability
   - New variant of vulnerability
   - Test implementation issue
4. Report to security team if needed

## References

- **Security Audit Report**: `../../../TIPs/SECURITY_AUDIT_REPORT.md`
- **GHOSTDAG Paper**: https://eprint.iacr.org/2018/104.pdf
- **Curve25519**: https://cr.yp.to/ecdh.html

## Contributing

When adding security tests:

1. Reference vulnerability ID (V-XX)
2. Document security fix location
3. Include both positive and negative test cases
4. Add to appropriate test file
5. Update this README
6. Ensure tests are deterministic

---

**Last Updated**: December 1, 2025
**Test Suite Version**: 2.0
**Coverage**: 100 tests covering all 27 vulnerabilities (98% active)
