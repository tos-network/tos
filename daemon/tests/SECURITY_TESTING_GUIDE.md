# TOS Security Testing Guide

## Overview

This guide describes the comprehensive security testing infrastructure for TOS blockchain. The testing suite includes multiple layers of security verification to ensure the robustness of the system.

## Testing Layers

### 1. Fuzz Testing

Fuzz testing uses randomized inputs to discover edge cases and vulnerabilities that traditional tests might miss.

#### Available Fuzz Targets

1. **GHOSTDAG Fuzzer** (`ghostdag_fuzzer.rs`)
   - Tests GHOSTDAG consensus computation
   - Verifies work calculations don't panic
   - Checks k-cluster validation
   - Ensures blue score arithmetic is safe

2. **Block Deserialization** (`fuzz_block_deserialize.rs`)
   - Tests block header parsing
   - Verifies full block deserialization
   - Checks stream parsing robustness
   - Ensures no memory exhaustion

3. **Transaction Decode** (`fuzz_transaction_decode.rs`)
   - Tests transaction deserialization
   - Verifies signature verification safety
   - Checks amount overflow protection
   - Ensures deterministic parsing

4. **Contract Bytecode** (`fuzz_contract_bytecode.rs`)
   - Tests bytecode validation
   - Verifies instruction parsing
   - Checks size limits
   - Ensures no unbounded loops

#### Running Fuzz Tests

```bash
# Install cargo-fuzz if not already installed
cargo install cargo-fuzz

# Install nightly toolchain
rustup toolchain install nightly

# Run individual fuzz targets
cd daemon/fuzz
cargo +nightly fuzz run ghostdag_fuzzer -- -max_total_time=60
cargo +nightly fuzz run fuzz_block_deserialize -- -max_total_time=60
cargo +nightly fuzz run fuzz_transaction_decode -- -max_total_time=60
cargo +nightly fuzz run fuzz_contract_bytecode -- -max_total_time=60

# Run with longer timeout for deeper testing
cargo +nightly fuzz run ghostdag_fuzzer -- -max_total_time=3600
```

#### Interpreting Fuzz Results

- **No crashes**: All fuzz targets should complete without panics
- **Coverage**: Check `fuzz/coverage/` for code coverage reports
- **Artifacts**: Failed inputs are saved in `fuzz/artifacts/`
- **Corpus**: Successful inputs are saved in `fuzz/corpus/`

### 2. Property-Based Testing

Property-based testing verifies that invariants hold across random inputs using proptest.

#### Properties Tested

1. **Balance Invariants**
   - Balances never overflow
   - Balances never go negative
   - Total supply conservation

2. **Nonce Invariants**
   - Nonces are monotonic
   - No duplicate nonces
   - Sequential ordering

3. **GHOSTDAG Invariants**
   - Blue score monotonicity
   - Blue work consistency
   - K-cluster compliance

4. **Arithmetic Safety**
   - Fee calculations never overflow
   - Reward calculations are deterministic
   - Scaled integer arithmetic is safe

5. **Transaction Validation**
   - Validation is deterministic
   - Amount + fee <= balance
   - Gas limits are enforced

#### Running Property Tests

```bash
# Run all property tests
cargo test --package tos_daemon --test property_tests

# Run with more iterations for deeper testing
cargo test --package tos_daemon --test property_tests -- --test-threads=1

# Run specific property test
cargo test --package tos_daemon --test property_tests test_balance_never_negative
```

#### Understanding Property Test Failures

When a property test fails:

1. Check the shrunk input (minimized failing case)
2. Verify the invariant being tested
3. Examine the failing assertion
4. Reproduce with the specific seed

Example:
```
thread 'test_balance_never_negative' panicked at 'property doesn't hold'
  minimal failing input: initial_balance = 100, operations = [150]
```

### 3. Security Integration Tests

Integration tests verify security properties across module boundaries.

#### Test Categories

1. **Consensus Security**
   - Consensus determinism
   - Double-spend prevention
   - Balance overflow protection

2. **Network Security**
   - Merkle root validation
   - Unauthorized state modification
   - Concurrent block processing

3. **Contract Security**
   - Gas limit enforcement
   - Bytecode validation
   - Execution safety

4. **Cryptographic Security**
   - Signature verification
   - Hash consistency
   - Nonce gap prevention

#### Running Integration Tests

```bash
# Run comprehensive security tests
cargo test --package tos_daemon --test security_comprehensive_tests --release

# Run with output
cargo test --package tos_daemon --test security_comprehensive_tests --release -- --nocapture

# Run specific test
cargo test --package tos_daemon --test security_comprehensive_tests test_double_spend_prevention
```

### 4. Performance Benchmarks

Benchmarks measure the performance of security-critical operations.

#### Benchmarked Operations

1. **Signature Verification**
   - Ed25519 single verification
   - Batch verification (10, 100 signatures)

2. **Hash Computation**
   - Blake3 (32 bytes, 1KB, 10KB)
   - SHA3-256 comparison

3. **Transaction Validation**
   - Simple transfers
   - Complex transactions
   - Batch validation (10, 100, 1000)

4. **Block Validation**
   - Varying transaction counts
   - Merkle tree computation

5. **Balance Operations**
   - Checked addition/subtraction
   - Fee calculation with scaling

#### Running Benchmarks

```bash
# Run all security benchmarks
cargo bench --package tos_daemon --bench security_benchmarks

# Run specific benchmark group
cargo bench --package tos_daemon --bench security_benchmarks signature_verification

# Generate HTML report
cargo bench --package tos_daemon --bench security_benchmarks -- --save-baseline security_baseline

# Compare with baseline
cargo bench --package tos_daemon --bench security_benchmarks -- --baseline security_baseline
```

#### Interpreting Benchmark Results

Results are saved in `target/criterion/`:

- `report/index.html`: Interactive HTML report
- `*/base/estimates.json`: Raw performance data
- `*/change/relative.json`: Performance changes

Performance targets:
- Signature verification: < 100 μs
- Hash (32 bytes): < 1 μs
- Transaction validation: < 10 μs
- Block validation (100 tx): < 1 ms

## Complete Test Suite

### Quick Run (5-10 minutes)

```bash
./scripts/run_security_tests.sh --quick
```

Runs:
- Unit tests
- Property tests
- Integration tests
- Clippy lints

### Full Run (30-60 minutes)

```bash
./scripts/run_security_tests.sh --full
```

Runs:
- All quick tests
- Fuzz tests (60s per target)
- Security benchmarks

### Fuzz-Only Run

```bash
./scripts/run_security_tests.sh --fuzz
```

### Benchmark-Only Run

```bash
./scripts/run_security_tests.sh --bench
```

## Continuous Integration

### Pre-Commit Checks

Before committing:

```bash
# Format code
cargo fmt --all

# Check compilation
cargo check --workspace --all-targets

# Run clippy
cargo clippy --workspace --all-targets -- -D warnings

# Run quick security tests
./scripts/run_security_tests.sh --quick
```

### CI Pipeline

The CI pipeline should run:

1. **PR Checks**
   - Compilation
   - Unit tests
   - Property tests
   - Integration tests
   - Clippy lints

2. **Nightly Tests**
   - Full fuzz testing (1 hour per target)
   - Security benchmarks
   - Coverage reports

3. **Release Checks**
   - Extended fuzz testing (24 hours)
   - Performance regression tests
   - Security audit

## Security Test Coverage

### Critical Paths Covered

1. **Consensus Layer**
   - [x] GHOSTDAG computation
   - [x] Blue score calculation
   - [x] Work calculation
   - [x] Block validation

2. **Transaction Layer**
   - [x] Deserialization
   - [x] Signature verification
   - [x] Nonce validation
   - [x] Balance checks

3. **Network Layer**
   - [x] Block deserialization
   - [x] Merkle root validation
   - [x] Peer message handling

4. **Contract Layer**
   - [x] Bytecode validation
   - [x] Gas limit enforcement
   - [x] Execution safety

5. **Storage Layer**
   - [ ] Database consistency
   - [ ] State rollback
   - [ ] Corruption detection

### Coverage Reports

Generate coverage reports:

```bash
# Install cargo-tarpaulin
cargo install cargo-tarpaulin

# Generate coverage
cargo tarpaulin --workspace --out Html --output-dir coverage

# View coverage
open coverage/index.html
```

## Known Issues and Limitations

### Current Limitations

1. **Storage Layer**: Limited testing for database operations
2. **Network Layer**: No network-level fuzz testing yet
3. **Contract VM**: TAKO VM has separate test suite

### Future Improvements

1. Add network protocol fuzz testing
2. Implement differential fuzzing (compare with reference implementation)
3. Add chaos engineering tests
4. Extend property tests to distributed scenarios

## Debugging Failed Tests

### Fuzz Test Failure

1. Check `fuzz/artifacts/<target>/` for failing input
2. Reproduce with: `cargo fuzz run <target> <artifact-file>`
3. Run with debugger: `cargo fuzz run <target> --debug`

### Property Test Failure

1. Note the seed value from test output
2. Reproduce with: `PROPTEST_SEED=<seed> cargo test <test-name>`
3. Add debug output to understand failure

### Integration Test Failure

1. Run with `--nocapture` to see all output
2. Check logs in `/tmp/tos_*.log`
3. Reproduce in isolation: `cargo test <test-name> -- --exact`

## Best Practices

### Writing Security Tests

1. **Always use checked arithmetic** in consensus code
2. **Test edge cases** (u64::MAX, 0, overflow)
3. **Verify determinism** (same input = same output)
4. **Check for panics** (all code paths should return Result)
5. **Measure performance** (ensure no DoS vectors)

### Code Review Checklist

- [ ] All f64 usage in consensus code replaced with u128 scaled integers
- [ ] All arithmetic uses checked_add/sub/mul
- [ ] All deserialization has size limits
- [ ] All loops have iteration limits
- [ ] All signature verification is constant-time
- [ ] All error messages don't leak secrets

## Resources

- [Fuzz Testing Guide](https://rust-fuzz.github.io/book/)
- [Proptest Documentation](https://proptest-rs.github.io/proptest/)
- [Criterion Benchmarking](https://bheisler.github.io/criterion.rs/book/)
- [TOS CLAUDE.md](../../CLAUDE.md) - Project coding standards

## Contact

For security issues, contact: security@tos.network

For testing questions, see: `daemon/tests/README.md`
