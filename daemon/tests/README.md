# TOS Daemon Test Suite

Comprehensive test suite for the TOS blockchain daemon, covering unit tests, integration tests, and stress tests.

## Table of Contents

1. [Test Organization](#test-organization)
2. [Running Tests](#running-tests)
3. [Test Categories](#test-categories)
4. [Performance Benchmarks](#performance-benchmarks)
5. [Coverage Reports](#coverage-reports)
6. [Known Issues](#known-issues)
7. [Contributing Tests](#contributing-tests)

## Test Organization

```
daemon/tests/
├── integration/           # Integration tests
│   ├── dag_tests.rs      # Full DAG integration tests
│   ├── ghostdag_tests.rs # GHOSTDAG integration tests
│   └── daa_tests.rs      # DAA integration tests
├── stress/               # Stress and load tests
│   ├── high_load.rs      # High throughput tests
│   └── memory_tests.rs   # Memory pressure tests
├── integration_tests.rs  # Main integration test file
└── README.md            # This file

daemon/src/core/
├── ghostdag/
│   ├── mod.rs           # GHOSTDAG unit tests (15 new tests)
│   └── daa.rs           # DAA unit tests (13 new tests)
└── difficulty/
    ├── mod.rs           # Difficulty calculation tests
    ├── v1.rs           # Kalman filter v1 tests
    └── v2.rs           # Kalman filter v2 tests
```

## Running Tests

### Run All Tests

```bash
cargo test --package tos_daemon
```

### Run Unit Tests Only

```bash
# Run all unit tests
cargo test --package tos_daemon --lib

# Run specific module tests
cargo test --package tos_daemon --lib ghostdag
cargo test --package tos_daemon --lib daa
cargo test --package tos_daemon --lib difficulty
```

### Run Integration Tests

```bash
# Run all integration tests
cargo test --package tos_daemon --test integration_tests

# Run specific integration test file
cargo test --package tos_daemon --test integration_tests --test dag_tests
cargo test --package tos_daemon --test integration_tests --test ghostdag_tests
cargo test --package tos_daemon --test integration_tests --test daa_tests
```

### Run Stress Tests

```bash
# Run all stress tests (ignored by default)
cargo test --package tos_daemon --test stress -- --ignored

# Run specific stress test
cargo test --package tos_daemon stress_high_block_rate -- --ignored --nocapture
cargo test --package tos_daemon stress_memory_large_dag -- --ignored --nocapture
```

### Run Tests with Output

```bash
# Show println! output
cargo test --package tos_daemon -- --nocapture

# Show only failing tests
cargo test --package tos_daemon -- --quiet
```

## Test Categories

### Unit Tests (95%+ coverage target)

#### DAA (Difficulty Adjustment Algorithm) Tests

Located in: `daemon/src/core/ghostdag/daa.rs`

**New comprehensive tests (13 tests):**
1. `test_daa_window_empty` - DAA window calculation when not yet full
2. `test_daa_window_exactly_full` - Boundary case when window is exactly full
3. `test_daa_window_past_full` - Window boundaries for mature blockchain
4. `test_difficulty_adjustment_minimum_ratio` - Minimum difficulty decrease (0.25x)
5. `test_difficulty_adjustment_maximum_ratio` - Maximum difficulty increase (4.0x)
6. `test_timestamp_backwards_resistance` - Protection against backward timestamps
7. `test_timestamp_future_resistance` - Protection against far-future timestamps
8. `test_zero_difficulty_edge_case` - Handling of zero difficulty
9. `test_very_large_difficulty` - Handling of very large difficulty values
10. `test_difficulty_adjustment_precision` - Small adjustment precision
11. `test_window_boundary_overflow_protection` - Overflow protection in calculations
12. `test_expected_time_consistency` - Expected time calculation consistency
13. `test_ratio_calculation_exact_match` - Perfect difficulty stability case

**Test Coverage:**
- Window calculation edge cases
- Difficulty adjustment boundaries
- Timestamp manipulation resistance
- Overflow protection
- Precision and consistency

#### GHOSTDAG Tests

Located in: `daemon/src/core/ghostdag/mod.rs`

**New comprehensive tests (15 tests):**
1. `test_ghostdag_max_parent_count` - Maximum parent count (32 parents)
2. `test_ghostdag_k_cluster_exactly_k` - K-cluster with exactly K parents
3. `test_ghostdag_k_cluster_single_parent` - Single parent edge case
4. `test_ghostdag_deep_dag_blue_score` - Blue score accumulation in deep DAG
5. `test_ghostdag_deep_dag_blue_work` - Blue work accumulation
6. `test_ghostdag_anticone_size_tracking` - Anticone size validation
7. `test_ghostdag_blue_red_classification` - Blue/red boundary cases
8. `test_ghostdag_genesis_special_case` - Genesis block handling
9. `test_ghostdag_selected_parent_highest_work` - Selected parent selection
10. `test_ghostdag_sortable_block_ordering` - Block ordering by blue work
11. `test_ghostdag_work_calculation` - Work calculation from difficulty
12. `test_ghostdag_zero_difficulty` - Zero difficulty edge case
13. `test_ghostdag_large_dag_scaling` - Large DAG performance simulation
14. `test_ghostdag_mergeset_size_limits` - Mergeset size constraints
15. `test_ghostdag_data_invariants` - TosGhostdagData structure invariants

**Test Coverage:**
- Maximum parent count (32)
- K-cluster edge cases
- Deep DAG structures
- Blue/red classification
- Work calculations
- Data structure invariants

### Integration Tests

Located in: `daemon/tests/integration/`

**DAG Integration Tests (`dag_tests.rs`):**
- Full DAG with DAA (2500+ blocks)
- Chain reorganization scenarios
- Concurrent block addition
- Large DAG performance (10,000+ blocks)
- Complex DAG topologies
- Block validation in DAG context

**GHOSTDAG Integration Tests (`ghostdag_tests.rs`):**
- Multiple branch merging
- K-cluster constraint enforcement
- Deep ancestry chains
- Selected parent selection
- Blue work accumulation
- Mergeset ordering
- Maximum parents (32)
- Reachability integration

**DAA Integration Tests (`daa_tests.rs`):**
- Stable hashrate scenarios
- Increasing hashrate (difficulty rise)
- Decreasing hashrate (difficulty drop)
- Window boundary calculations
- Mergeset_non_daa filtering
- Timestamp manipulation resistance
- Multiple adjustment periods (10,000+ blocks)
- GHOSTDAG integration

### Stress Tests

Located in: `daemon/tests/stress/`

**High Load Tests (`high_load.rs`):**
- High block rate (100+ blocks/sec for 10 seconds)
- Large DAG depth (10,000+ blocks)
- High parent count (32 parents)
- Concurrent block processing (50 blocks × 100 batches)
- Long-running stability (24 hours)

**Memory Tests (`memory_tests.rs`):**
- Memory pressure with large DAG (100,000 blocks)
- Memory leak detection (10,000 iterations)
- Cache pressure test
- Large block processing (10,000 transactions/block)
- Recovery from memory pressure

## Performance Benchmarks

### Expected Performance Targets

#### Unit Test Performance
- All unit tests should complete in < 5 seconds total
- Individual tests: < 1 second each
- Fast feedback for development

#### Integration Test Performance
- DAG operations: < 100ms per block
- GHOSTDAG calculations: < 1 second for 32-parent blocks
- DAA calculations: < 10ms per block
- Query operations: < 10ms

#### Stress Test Performance
- Block processing: 100+ blocks/sec sustained
- Memory usage: < 2GB for 100,000 blocks
- CPU usage: < 80% during normal operation
- No memory leaks (stable memory after initial ramp)

### Running Benchmarks

```bash
# Run with timing information
cargo test --package tos_daemon -- --nocapture --test-threads=1

# Measure specific test performance
cargo test --package tos_daemon test_name -- --nocapture --exact
```

## Coverage Reports

### Generating Coverage Reports

Using `cargo-tarpaulin`:

```bash
# Install tarpaulin
cargo install cargo-tarpaulin

# Generate coverage report
cargo tarpaulin --package tos_daemon --out Html --output-dir coverage

# Generate coverage for specific modules
cargo tarpaulin --package tos_daemon --out Html --output-dir coverage \
  --exclude-files 'main.rs' 'rpc/*' 'p2p/*'
```

### Coverage Targets

- **Overall Target**: 95%+ code coverage
- **Core modules**: 95%+ coverage
  - `daemon/src/core/ghostdag/`: 95%+
  - `daemon/src/core/difficulty/`: 95%+
  - `daemon/src/core/blockdag.rs`: 90%+
- **Integration critical paths**: 100% coverage
- **Edge cases**: Explicitly tested

### Current Coverage Status

Run `cargo tarpaulin` to see current coverage. Key areas:

1. ✅ DAA algorithm: Comprehensive unit tests
2. ✅ GHOSTDAG: Comprehensive unit tests
3. ⚠️ Integration tests: Require full storage implementation
4. ⚠️ Stress tests: Require full storage implementation

## Known Issues and Limitations

### Test Infrastructure

1. **Storage Dependency**: Most integration and stress tests require full storage implementation
   - Status: Marked with `#[ignore]`
   - Resolution: Implement once storage layer is complete

2. **Mining Module**: Mining tests pending due to compilation issues
   - Status: Mining module has unresolved compilation errors
   - Resolution: Fix mining module compilation first

3. **Reachability Tests**: Some GHOSTDAG tests need reachability service
   - Status: Fallback implemented for blocks without reachability data
   - Resolution: Full implementation in Phase 2

### Test Execution

1. **Long-Running Tests**: Some stress tests take hours
   - Marked with `#[ignore]`
   - Run explicitly with `--ignored` flag
   - Consider running overnight or in CI

2. **Resource-Intensive Tests**: Some tests require significant resources
   - Memory tests: Up to 2GB RAM
   - High-load tests: High CPU usage
   - Run on appropriate hardware

3. **Flaky Tests**: None currently identified
   - All tests should be deterministic
   - Report any flaky tests as issues

## Test Organization Best Practices

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_specific_functionality() {
        // Arrange
        let input = setup_test_data();

        // Act
        let result = function_under_test(input);

        // Assert
        assert_eq!(result, expected_value);
    }
}
```

### Integration Tests

```rust
#[tokio::test]
#[ignore] // Mark as ignored if requires full implementation
async fn test_integration_scenario() {
    // Setup
    let blockchain = setup_test_blockchain().await;

    // Execute
    let result = blockchain.add_block(test_block).await;

    // Verify
    assert!(result.is_ok());
    verify_blockchain_state(&blockchain).await;
}
```

### Stress Tests

```rust
#[tokio::test]
#[ignore] // Always mark stress tests as ignored
async fn stress_test_scenario() {
    const LOAD_PARAMS: usize = 10_000;

    // Measure performance
    let start = Instant::now();

    // Execute under load
    for i in 0..LOAD_PARAMS {
        process_item(i).await;
    }

    // Verify performance targets
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs() < EXPECTED_MAX_SECONDS);
}
```

## Contributing Tests

### Adding New Tests

1. **Unit Tests**: Add to relevant module's `mod tests` section
2. **Integration Tests**: Add to appropriate file in `tests/integration/`
3. **Stress Tests**: Add to appropriate file in `tests/stress/`

### Test Guidelines

1. **Naming**: Use descriptive names that explain what is tested
   - Good: `test_ghostdag_k_cluster_exactly_k`
   - Bad: `test_ghostdag_1`

2. **Documentation**: Include comments explaining:
   - What is being tested
   - Why this test is important
   - Expected behavior

3. **Assertions**: Use clear assertion messages
   ```rust
   assert!(value <= limit, "Value {} exceeds limit {}", value, limit);
   ```

4. **Determinism**: Tests must be deterministic
   - No random values without fixed seeds
   - No time-dependent behavior (use mocked time)
   - No network dependencies

5. **Speed**: Keep unit tests fast (< 1 second)
   - Use `#[ignore]` for slow tests
   - Mock expensive operations

### Test Review Checklist

- [ ] Test name is clear and descriptive
- [ ] Test has documentation comments
- [ ] Test follows Arrange-Act-Assert pattern
- [ ] Assertions have clear error messages
- [ ] Test is deterministic
- [ ] Test is fast (or marked with `#[ignore]`)
- [ ] Test covers edge cases
- [ ] Test verifies both success and failure paths

## Additional Resources

- [TOS Whitepaper](https://tos.network/whitepaper) - Protocol specification
- [GHOSTDAG Paper](https://eprint.iacr.org/2018/104.pdf) - GHOSTDAG algorithm details
- [Rust Testing Guide](https://doc.rust-lang.org/book/ch11-00-testing.html) - Rust testing best practices

## Support

For questions or issues with tests:
1. Check this README first
2. Review existing test examples
3. Open an issue on GitHub
4. Contact the development team

---

**Note**: This test suite is part of Phase 3 development (TIP-2 Implementation). Some tests are marked as `#[ignore]` pending completion of storage and blockchain infrastructure. These will be enabled as the implementation progresses.

**Last Updated**: October 2025
**Test Coverage Target**: 95%+
**Current Status**: Unit tests complete, integration tests pending storage implementation
