# TOS Blockchain Performance Benchmarks

## Overview

This directory contains comprehensive performance benchmarks for the TOS blockchain Phase 3 implementation. The benchmarks cover three main areas:

1. **GHOSTDAG Performance** - Consensus algorithm benchmarks
2. **Block Processing** - Block validation and storage benchmarks
3. **Transaction Verification** - ZK proof verification benchmarks

## Quick Start

### Running Benchmarks

```bash
# Run all benchmarks
cargo bench

# Run specific benchmark suite
cargo bench --bench ghostdag
cargo bench --bench block_processing
cargo bench --bench transaction

# Run specific test
cargo bench --bench transaction -- bench_speedup_verification
```

### View Results

Benchmark results are saved to `target/criterion/`:

```bash
# Open HTML report in browser
open target/criterion/report/index.html
```

## Benchmark Files

### 1. ghostdag.rs - GHOSTDAG Performance Benchmarks

Tests GHOSTDAG consensus algorithm performance including:

- **Linear Chain Performance** - Measures GHOSTDAG on simple chains (10, 100, 1000 blocks)
- **Complex DAG Performance** - Tests with multiple parents (2-4 avg parents)
- **DAA Window Calculation** - Benchmark Difficulty Adjustment Algorithm window ops
- **K-Cluster Validation** - Tests k-cluster constraint checking (k=5, 10, 18)
- **Blue Work Calculation** - Benchmarks work calculation from difficulty
- **Mergeset Ordering** - Tests block ordering by blue work
- **GHOSTDAG Data Size** - Measures serialization overhead

**Key Metrics**:
- GHOSTDAG computation time: < 1 ms per block (target)
- DAA window traversal: < 50 ms for 2016 blocks
- K-cluster validation: O(k) complexity

### 2. block_processing.rs - Block Processing Benchmarks

Tests complete block processing pipeline:

- **Block Addition** - Time to add block (0, 10, 50, 100 txs)
- **GHOSTDAG Computation** - Isolated GHOSTDAG algorithm performance
- **Storage Operations** - Read/write performance for blocks and GHOSTDAG data
- **Memory Usage** - Tracks memory consumption (10-200 blocks)
- **Full Pipeline** - End-to-end block processing
- **Block Validation** - Structure and consistency checks
- **System Resources** - OS-level resource monitoring
- **Parallel Processing** - Sequential vs parallel speedup

**Key Metrics**:
- Block addition: < 10 ms (0 txs), < 400 ms (100 txs)
- Storage operations: < 100 μs read, < 1 ms write
- Memory per block: ~50-100 KB
- Parallel speedup: 2-7x (2-8 threads)

### 3. transaction.rs - Transaction Verification Benchmarks

Comprehensive transaction verification performance tests:

- **Single Transaction Verification** - Individual proof verification times
- **ElGamal Operations** - Encryption/decryption/commitment benchmarks
- **Batch vs Individual** - Verifies 4x batch speedup claim
- **Parallel Scaling** - Tests 1-8 core scaling (8x speedup claim)
- **Parallel Batch** - Combined batch+parallel (32x speedup claim)
- **Proof Generation** - Transaction creation performance
- **Value Size Impact** - Tests if value size affects performance
- **Speedup Verification** - Definitive speedup comparison

**Key Metrics**:
- Single tx verification: ~13-17 ms
- Batch verification speedup: ~4x
- Parallel verification (8 cores): ~7-8x
- Combined batch+parallel: ~29-32x
- Proof generation: ~20-27 ms

## Performance Targets

### GHOSTDAG Consensus
- Linear chain: < 1 ms per block
- Complex DAG: 2-4x linear time
- DAA window: < 55 ms total overhead
- K-cluster validation: < 1 ms per block

### Block Processing
- 0 tx blocks: < 10 ms
- 100 tx blocks: < 400 ms
- Storage reads: < 100 μs (cached)
- Storage writes: < 1 ms (batched)
- Memory: ~50-100 KB per block

### Transaction Verification
- Individual: ~15 ms per tx
- Batch (4x): ~3.75 ms per tx
- Parallel 8-core (8x): ~1.9 ms per tx
- Combined (32x): ~0.5 ms per tx
- Target: 100-200 TPS sustained, 500+ TPS burst

## Benchmark Design

### Mock Storage

All benchmarks use lightweight mock storage implementations to isolate performance measurements from actual disk I/O:

- **MockStorage** (ghostdag.rs) - In-memory GHOSTDAG data storage
- **BlockProcessingStorage** (block_processing.rs) - Block and metadata storage
- **TransactionProofs** (transaction.rs) - ZK proof data structures

### Test Data Generation

Deterministic test data generation ensures reproducible results:

```rust
fn hash_from_index(index: u64) -> Hash
fn create_linear_chain(length: usize, k: KType) -> MockStorage
fn create_complex_dag(blocks: usize, avg_parents: usize, k: KType) -> MockStorage
fn create_transaction_batch(count: usize) -> Vec<TransactionProofs>
```

### Criterion Framework

All benchmarks use [Criterion.rs](https://bheisler.github.io/criterion.rs/book/) which provides:

- Statistical analysis (mean, median, std dev)
- Outlier detection
- HTML report generation
- Baseline comparison
- Configurable sample sizes

## Optimization Recommendations

### Immediate (Week 1-2)
1. Enable RocksDB optimizations (bloom filters, compression)
2. Implement transaction pool batching
3. Add LRU caching for GHOSTDAG data

### Medium-Term (Week 3-4)
1. Parallelize transaction verification
2. Implement block pruning
3. Optimize GHOSTDAG mergeset computation

### Long-Term (Month 2+)
1. Distributed caching for clusters
2. Custom storage layout for GHOSTDAG
3. Investigate faster ZK proof systems

## Comparison with Kaspa

TOS aims to match Kaspa's GHOSTDAG performance while adding significant transaction verification speedups through batch and parallel processing:

| Metric | Kaspa | TOS Target |
|--------|-------|------------|
| Block validation | < 100 ms | < 100 ms |
| DAG traversal | O(k), k=10 | O(k), k=10 |
| Memory/block | ~50 KB | ~50-100 KB |
| TX verification | Individual | 4x batch, 8x parallel |

## CI Integration

Add to CI pipeline for regression testing:

```bash
# Save baseline
cargo bench -- --save-baseline master

# Compare against baseline
cargo bench -- --baseline master

# Fail CI if > 10% regression
cargo bench -- --baseline master --threshold 10
```

## Profiling

For detailed performance analysis:

```bash
# Install profiling tools
cargo install cargo-flamegraph
cargo install cargo-benchcmp

# Generate flamegraph
cargo flamegraph --bench transaction

# Compare results
cargo benchcmp baseline current
```

## Documentation

See [PERFORMANCE_BENCHMARKS.md](../PERFORMANCE_BENCHMARKS.md) for:
- Detailed benchmark descriptions
- Expected performance results
- Optimization roadmap
- Performance comparison tables
- Maintenance guidelines

## Requirements

- Rust 1.70+
- Criterion 0.6.0
- Rayon 1.8 (for parallel benchmarks)
- Sysinfo 0.32 (for system resource monitoring)

## Notes

- Run benchmarks on dedicated hardware (no background processes)
- Use release builds only (`cargo bench` automatically uses release)
- Results vary based on CPU, memory, and system load
- Compare relative performance (speedup ratios) not absolute times
- Some benchmarks have reduced sample sizes to complete in reasonable time

## Contributing

When adding new benchmarks:

1. Follow existing naming conventions (`bench_*`)
2. Use `black_box()` to prevent compiler optimizations
3. Test multiple input sizes for scalability analysis
4. Add expected results to PERFORMANCE_BENCHMARKS.md
5. Document any new test data generation functions

## License

BSD-3-Clause (see LICENSE in repository root)

## Contact

TOS Network Team <info@tos.network>
