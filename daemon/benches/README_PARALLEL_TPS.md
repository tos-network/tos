# Parallel vs Sequential TPS Comparison Benchmark

## Overview

This benchmark suite (`parallel_tps_comparison.rs`) provides comprehensive performance comparisons between parallel and sequential transaction execution in the TOS blockchain.

## Benchmark Scenarios

### 1. Sequential Baseline Benchmarks
- `sequential_execution/10_txs` - Sequential execution with 10 transactions
- `sequential_execution/100_txs` - Sequential execution with 100 transactions

### 2. Parallel Execution Benchmarks
- `parallel_execution/10_txs` - Parallel execution with 10 transactions
- `parallel_execution/100_txs` - Parallel execution with 100 transactions

### 3. Conflict Ratio Test (50% conflicts)
- `conflict_ratio_50pct/sequential_50_txs` - Sequential execution with mixed conflicts
- `conflict_ratio_50pct/parallel_50_txs` - Parallel execution with mixed conflicts

### 4. Direct TPS Comparison (Side-by-side)
- `tps_comparison/sequential/*` - Sequential execution at 10, 50, 100 tx
- `tps_comparison/parallel/*` - Parallel execution at 10, 50, 100 tx

## Metrics Measured

- **Total Execution Time**: Measured in microseconds using `std::time::Instant`
- **Throughput (TPS)**: Calculated using integer arithmetic (u64) to avoid floating-point inconsistencies
  - Formula: `TPS = (tx_count * 1_000_000) / elapsed_micros`
- **Speedup Ratio**: Calculated using u128 scaled integers (SCALE=10000)
  - Formula: `speedup = (sequential_time * SCALE) / parallel_time`
  - Example: 15000 represents 1.5x speedup

## Running the Benchmarks

### Quick Test (Verify Functionality)
```bash
cargo bench --bench parallel_tps_comparison -- --test
```

### Run All Benchmarks
```bash
cargo bench --bench parallel_tps_comparison
```

### Run Specific Benchmark Group
```bash
# Only sequential benchmarks
cargo bench --bench parallel_tps_comparison sequential_execution

# Only parallel benchmarks
cargo bench --bench parallel_tps_comparison parallel_execution

# Only conflict ratio tests
cargo bench --bench parallel_tps_comparison conflict_ratio

# Only direct TPS comparison
cargo bench --bench parallel_tps_comparison tps_comparison
```

### Generate HTML Report
```bash
cargo bench --bench parallel_tps_comparison
# Results saved to: target/criterion/parallel_tps_comparison/report/index.html
```

## Code Quality Standards

This benchmark follows **CLAUDE.md** requirements:

1. **English Only**: All comments and documentation in English
2. **Logging Performance**: All logs with format arguments use `if log::log_enabled!` checks
3. **No f64 in Critical Paths**: Uses u64 for TPS and u128 for ratios (no floating point)
4. **Integer Arithmetic**: Uses scaled integers (SCALE=10000) for decimal calculations
5. **Zero Warnings**: Compiles with `cargo build --workspace` producing 0 warnings

## Performance Expectations

Based on hardware configuration:
- **Conflict-free transactions**: ~2-4x speedup with parallel execution
- **50% conflict ratio**: ~1.2-1.5x speedup (limited by conflict batching)
- **High conflict ratio** (>80%): Minimal speedup (most transactions sequential)

## Technical Details

### Transaction Generation
- **Conflict-free**: Each transaction uses a unique sender keypair
- **Mixed conflicts**: 50% share the same sender, forcing sequential batching
- **Transfer amount**: 1000 base units per transaction
- **Fee**: 100 base units per transaction

### Storage Backend
- Uses temporary SledStorage instances for each benchmark run
- `StorageMode::HighThroughput` for optimal performance
- Automatic cleanup via TempDir

### Block Creation
- Minimal blocks with empty transaction lists
- Zero parents (genesis-like blocks)
- No merkle root validation (benchmark focus on execution)

## Interpreting Results

Criterion will output:
- **Throughput**: Elements/sec (transactions per second)
- **Latency**: Time per iteration
- **Statistical Analysis**: Mean, median, standard deviation
- **Comparison**: Change from previous run (if available)

Example output:
```
parallel_execution/10_txs
                        time:   [2.5 ms 2.6 ms 2.7 ms]
                        thrpt:  [3703.7 elem/s 3846.2 elem/s 4000.0 elem/s]
```

## Troubleshooting

### Benchmark Takes Too Long
Reduce sample size by setting environment variable:
```bash
CRITERION_SAMPLE_SIZE=10 cargo bench --bench parallel_tps_comparison
```

### Out of Memory Errors
Reduce transaction counts in benchmark code or increase system memory.

### Inconsistent Results
- Close other applications to reduce CPU noise
- Run multiple times and look at median values
- Check CPU temperature/throttling
- Ensure power management is set to "Performance"

## References

- **Main Implementation**: `daemon/src/core/executor/parallel_executor.rs`
- **State Management**: `daemon/src/core/state/parallel_chain_state.rs`
- **Existing TPS Benchmark**: `daemon/benches/tps.rs`
- **Coding Standards**: `CLAUDE.md` in project root
