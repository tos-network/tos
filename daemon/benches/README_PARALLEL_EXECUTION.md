# TOS Parallel Transaction Execution Benchmarks

## Overview

This benchmark suite measures the performance characteristics of the parallel transaction execution infrastructure implemented in Phase 1-3. It focuses on infrastructure overhead and scalability rather than absolute throughput with real transactions.

## Benchmark Categories

### 1. Parallel State Creation (`bench_parallel_state_creation`)
Measures the overhead of creating a `ParallelChainState` instance.

**What it measures:**
- Time to initialize storage Arc and RwLock
- Environment setup overhead
- DashMap initialization for concurrent state tracking

**Use case:** Understanding the fixed overhead of parallel execution setup.

### 2. Parallel Executor Batch Sizes (`bench_parallel_executor_batch_sizes`)
Benchmarks executor performance with different batch sizes: 10, 20, 50, 100 transactions.

**What it measures:**
- End-to-end batch execution time
- Overhead of conflict detection and batching
- Parallelization effectiveness at different scales

**Use case:** Finding optimal batch sizes for production deployment.

### 3. Conflict Detection (`bench_conflict_detection`)
Measures the performance of account extraction and conflict detection logic.

**What it measures:**
- Conflicting transactions (same sender) - worst case
- Conflict-free transactions (different senders) - best case
- Account extraction overhead

**Use case:** Understanding the cost of dependency analysis.

### 4. Account Extraction (`bench_account_extraction`)
Benchmarks the performance of extracting account keys from transactions.

**What it measures:**
- Transaction parsing overhead
- Account key extraction from transfers
- Scalability with transaction count

**Use case:** Profiling the conflict detection preprocessing step.

### 5. Executor Parallelism (`bench_executor_parallelism`)
Tests scalability with different parallelism levels: 1, 2, 4, optimal (CPU count).

**What it measures:**
- Parallel speedup factor
- Tokio JoinSet overhead
- Thread contention at different parallelism levels

**Use case:** Validating parallel execution scalability.

### 6. State Commit Overhead (`bench_state_commit`)
Measures the cost of committing parallel state changes to storage.

**What it measures:**
- State merging time
- Storage write batch performance
- Overhead of committing nonces and balances

**Use case:** Understanding the finalization bottleneck.

### 7. Memory Overhead (`bench_memory_overhead`)
Measures the memory footprint of parallel state objects.

**What it measures:**
- Arc allocation overhead
- DashMap memory usage
- Multiple state instance overhead

**Use case:** Estimating memory requirements for production.

## Running Benchmarks

### Run All Benchmarks
```bash
cargo bench --package tos_daemon --bench parallel_execution
```

### Run Specific Benchmark Group
```bash
# State creation only
cargo bench --package tos_daemon --bench parallel_execution -- parallel_state_creation

# Batch sizes only
cargo bench --package tos_daemon --bench parallel_execution -- parallel_executor_batch_sizes

# Conflict detection only
cargo bench --package tos_daemon --bench parallel_execution -- conflict_detection

# Account extraction only
cargo bench --package tos_daemon --bench parallel_execution -- account_extraction

# Parallelism scalability only
cargo bench --package tos_daemon --bench parallel_execution -- executor_parallelism

# State commit only
cargo bench --package tos_daemon --bench parallel_execution -- state_commit

# Memory overhead only
cargo bench --package tos_daemon --bench parallel_execution -- memory_overhead
```

### Compile Without Running
```bash
cargo bench --package tos_daemon --bench parallel_execution --no-run
```

## Interpreting Results

### Example Output
```
parallel_state_creation/create_parallel_chain_state
                        time:   [2.3456 ms 2.4123 ms 2.4890 ms]

parallel_executor_batch_sizes/10_txs
                        time:   [15.234 ms 15.678 ms 16.123 ms]
```

### Key Metrics
- **time**: Execution time for the benchmark (lower is better)
- **[lower bound, estimate, upper bound]**: 95% confidence interval
- **slope**: Throughput estimate (for parametric benchmarks)
- **R²**: Goodness of fit for linear regression

### Performance Expectations

**Acceptable Ranges (on modern hardware):**
- State creation: 1-5 ms
- Batch execution (10 txs): 10-30 ms
- Batch execution (100 txs): 50-200 ms
- Conflict detection (100 txs): < 1 ms
- Account extraction (100 txs): < 0.5 ms
- State commit (100 txs): 5-20 ms

**Scalability Goals:**
- Parallelism=4 should be 2-3x faster than Parallelism=1
- Batch size 100 should be < 5x slower than batch size 10

## Limitations

### What These Benchmarks DON'T Measure
1. **Real transaction verification**: Uses mock transactions without signatures
2. **Smart contract execution**: Contract invocation is stubbed
3. **Network I/O**: No real block propagation
4. **Cryptographic operations**: Simplified for benchmarking
5. **Storage persistence**: Uses temporary in-memory storage

### Why Infrastructure Benchmarks?
Creating real signed transactions requires:
- Keypair generation (expensive)
- Signature creation (expensive)
- ZK proof generation (very expensive for balance proofs)
- Complex state setup (accounts, balances, nonces)

These benchmarks focus on **measuring the parallel execution infrastructure overhead** independently of transaction complexity.

## Benchmark Environment

### Hardware Recommendations
- **CPU**: Multi-core processor (4+ cores recommended)
- **RAM**: 4GB+ available
- **Storage**: SSD for temporary benchmark files

### Environment Variables
None currently supported. Future additions may include:
- `TOS_BENCH_THREADS`: Override parallelism level
- `TOS_BENCH_SAMPLE_SIZE`: Override Criterion sample size
- `TOS_BENCH_MEASUREMENT_TIME`: Override measurement duration

## Troubleshooting

### Compilation Errors
```bash
# Clean and rebuild
cargo clean
cargo bench --package tos_daemon --bench parallel_execution --no-run
```

### Slow Benchmarks
```bash
# Reduce sample size (faster but less accurate)
# This requires modifying the benchmark code's group.sample_size(10)
# to a lower value like group.sample_size(5)
```

### Insufficient Memory
- Benchmarks create temporary Sled databases
- Each benchmark iteration creates a new TempDir
- If running out of disk space, clean /tmp directory

## Comparison with Integration Tests

| Feature | Integration Tests | Benchmarks |
|---------|------------------|------------|
| Real transactions | ✅ Yes | ❌ No (mocked) |
| Performance measurement | ❌ No | ✅ Yes |
| Storage persistence | ✅ Full | ⚠️ Temporary |
| Signature verification | ✅ Yes | ❌ Skipped |
| CI/CD integration | ✅ On every PR | ⚠️ Manual/nightly |

## Future Enhancements

1. **End-to-end benchmarks**: Once parallel execution is enabled
2. **Comparison benchmarks**: Serial vs parallel execution
3. **Regression tracking**: Store historical benchmark results
4. **Flamegraph integration**: Profile hot paths
5. **Memory profiling**: Track allocations and leaks

## References

- **Phase 1-3 Implementation**: `~/tos-network/tos/daemon/src/core/executor/`
- **Integration Tests**: `~/tos-network/tos/daemon/tests/integration/parallel_execution_tests.rs`
- **Criterion Documentation**: https://bheisler.github.io/criterion.rs/book/

---

**Last Updated**: 2025-10-27
**Version**: 1.0
**Maintainer**: TOS Development Team
