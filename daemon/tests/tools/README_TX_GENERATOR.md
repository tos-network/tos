# TOS Transaction Generator Tool

**Location**: `/Users/tomisetsu/tos-network/tos/daemon/tests/tools/tx_generator.rs`

**Purpose**: Generate and submit batches of signed transactions to devnet/testnet for testing parallel transaction execution performance.

---

## Features

✅ **Automatic keypair generation** - Creates sender/receiver keypairs automatically
✅ **Batch transaction generation** - Generate 10, 20, 50, 100+ transactions
✅ **Flexible submission** - Submit in batches with configurable delays
✅ **Conflict modes** - Same sender (conflicting) or different senders (conflict-free)
✅ **Performance tracking** - TPS, latency, success rate measurements
✅ **RPC integration** - Direct submission to daemon via JSON-RPC

---

## Building

```bash
# Build in release mode (recommended)
cargo build --release --bin tx_generator

# Build in debug mode (faster compilation)
cargo build --bin tx_generator

# Binary location
ls -lh ./target/release/tx_generator
```

---

## Usage

### Basic Usage (25 transactions)

```bash
./target/release/tx_generator \
  --count 25 \
  --daemon http://127.0.0.1:8080
```

### Advanced Usage

```bash
./target/release/tx_generator \
  --count 50 \
  --daemon http://127.0.0.1:8080 \
  --batch-size 10 \
  --delay-ms 200 \
  --different-senders \
  --amount 5000 \
  --fee 200 \
  --network devnet \
  --verbose
```

---

## Command-Line Options

| Option               | Short | Default              | Description                                    |
|----------------------|-------|----------------------|------------------------------------------------|
| `--count`            | `-c`  | `25`                 | Number of transactions to generate             |
| `--daemon`           | `-d`  | `http://127.0.0.1:8080` | Daemon RPC address                          |
| `--batch-size`       | `-b`  | `1`                  | Submit N transactions at once                  |
| `--delay-ms`         |       | `100`                | Delay between batches (milliseconds)           |
| `--different-senders |       | `false`              | Use different senders (conflict-free mode)     |
| `--amount`           | `-a`  | `1000`               | Amount to transfer (nanoTOS)                   |
| `--fee`              | `-f`  | `100`                | Fee per transaction (nanoTOS)                  |
| `--network`          | `-n`  | `devnet`             | Network (devnet, testnet, mainnet)             |
| `--verbose`          | `-v`  | `false`              | Enable verbose logging                         |

---

## Testing Scenarios

### Scenario 1: Trigger Parallel Execution (≥20 txs)

```bash
# Generate 25 transactions from same sender (conflicting)
# Submit all at once to create a block with 25 txs
./target/release/tx_generator --count 25 --batch-size 25

# Expected: Parallel execution triggered (MIN_TXS_FOR_PARALLEL=20)
# Check daemon logs for parallel execution activity
```

### Scenario 2: Conflict-Free Transactions

```bash
# Generate 50 transactions from different senders
# Each sender has only 1 transaction (no conflicts)
./target/release/tx_generator \
  --count 50 \
  --batch-size 50 \
  --different-senders

# Expected: Maximum parallel speedup (no conflicts to resolve)
```

### Scenario 3: Large Batch (100+ txs)

```bash
# Generate 100 transactions
# Submit in batches of 25 with 500ms delay
./target/release/tx_generator \
  --count 100 \
  --batch-size 25 \
  --delay-ms 500

# Expected: 4 blocks with 25 txs each
# All blocks should use parallel execution
```

### Scenario 4: Performance Measurement

```bash
# Generate 200 conflict-free transactions
# Submit all at once for maximum throughput
./target/release/tx_generator \
  --count 200 \
  --batch-size 200 \
  --different-senders \
  --verbose

# Monitor performance metrics in output
```

---

## Output Example

```
TOS Transaction Generator
========================
Configuration:
  Transaction count: 50
  Daemon URL:        http://127.0.0.1:8080
  Batch size:        10
  Delay between batches: 100ms
  Different senders: true
  Amount per tx:     1000 nanoTOS
  Fee per tx:        100 nanoTOS
  Network:           devnet

Fetching chain info from daemon...
Chain info:
  Topoheight:    1234
  Stable height: 1230
  Top block:     a1b2c3d4...

Generating 50 sender keypairs...
Sender addresses:
  Sender 0: tst1abc123...
  Sender 1: tst1def456...
  ...
Receiver address: tst1xyz789...

Generating 50 transactions (different_senders: true)...
Successfully generated 50 transactions

Submitting 50 transactions in 5 batches...

Submitting batch 'Batch 1/5' with 10 transactions...
Batch 'Batch 1/5' complete: 10/10 submitted, 0 errors, 12.50 TPS, 800ms elapsed

Submitting batch 'Batch 2/5' with 10 transactions...
Batch 'Batch 2/5' complete: 10/10 submitted, 0 errors, 13.33 TPS, 750ms elapsed

...

======================================================================
PERFORMANCE SUMMARY
======================================================================
Total transactions generated: 50
Total transactions submitted: 50
Total errors:                 0
Success rate:                 100.00%
Total duration:               4.2s
Average TPS:                  11.90
Batch durations:              min=750ms, avg=840ms, max=920ms
======================================================================

Transaction generation complete!
Check daemon logs for parallel execution activity.
Look for blocks with 20+ transactions to trigger parallel path.
```

---

## Verifying Parallel Execution

After running the tool, check daemon logs for parallel execution:

```bash
# Check for blocks with ≥20 transactions
tail -100 ~/tos_devnet/daemon.log | grep "with [2-9][0-9] txs"

# Example output showing parallel execution:
# [INFO] Processed block abc123... at height 1235 in 45ms with 25 txs (DAG: true)
#                                                              ^^^ ≥20 txs = parallel!

# Look for timing improvements
# Sequential: ~100ms for 25 txs (~4ms per tx)
# Parallel:   ~45ms for 25 txs (~1.8ms per tx) = 2.2x speedup!
```

---

## Troubleshooting

### Issue 1: "Failed to get chain info. Is the daemon running?"

**Solution**: Ensure daemon is running and RPC is accessible

```bash
# Check if daemon is running
ps aux | grep tos_daemon

# Test RPC endpoint
curl -X POST http://127.0.0.1:8080 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"get_info","params":{}}'

# Start daemon if not running
./target/release/tos_daemon \
  --network devnet \
  --dir-path ~/tos_devnet/ \
  --log-level info
```

### Issue 2: "Block submitted has been rejected"

**Cause**: Transactions have invalid nonces or insufficient balance

**Solution**: The tool uses mock balances (1M TOS per sender). This should only happen if:
1. The same sender is used for too many transactions
2. Network state doesn't match tool's assumptions

**Workaround**: Use `--different-senders` to avoid nonce conflicts

### Issue 3: "RPC error -32601: Method not found"

**Cause**: Daemon RPC method mismatch

**Solution**: Ensure daemon and tool versions are compatible

```bash
# Rebuild both daemon and tool
cargo build --release --package tos_daemon
cargo build --release --bin tx_generator
```

### Issue 4: Transactions not appearing in mempool

**Cause**: Mempool may be full or transactions are being rejected

**Solution**: Check daemon logs for rejection reasons

```bash
tail -100 ~/tos_devnet/daemon.log | grep -E "(reject|invalid|error)"
```

---

## Architecture

### Transaction Generation Flow

```
1. Fetch chain state (topoheight, reference hash)
   │
   ↓
2. Generate keypairs
   - N senders (1 or count, depending on --different-senders)
   - 1 receiver
   │
   ↓
3. For each transaction:
   - Create TestAccountState (balance, nonce, reference)
   - Build TransferBuilder (amount, destination)
   - Build TransactionBuilder (signature, fees)
   │
   ↓
4. Submit in batches via RPC
   - Serialize to hex
   - POST to /submit_transaction
   - Track success/failure
   │
   ↓
5. Performance summary
   - Total TPS
   - Success rate
   - Batch statistics
```

### Key Components

#### TestAccountState

Minimal implementation of `AccountState` trait for transaction building:
- Provides balance (fixed at 1M TOS)
- Provides nonce (from transaction index)
- Provides reference (from chain state)
- Used by TransactionBuilder to construct valid signed transactions

#### RpcClient

Async JSON-RPC client for daemon communication:
- `get_info()`: Fetch chain state (topoheight, stable height)
- `submit_transaction()`: Submit signed transaction hex
- Automatic request ID management
- 30-second timeout per request

#### TransactionGenerator

Core transaction generation logic:
- Manages keypairs (sender/receiver)
- Tracks reference (topoheight + hash)
- Generates signed Transfer transactions
- Supports both conflicting and conflict-free modes

#### TransactionSubmitter

Batch submission orchestrator:
- Submits transactions in configurable batches
- Measures per-batch TPS and latency
- Tracks success/error counts

#### PerformanceTracker

Performance metrics collector:
- Total TPS across all batches
- Success rate percentage
- Min/avg/max batch durations
- Pretty-printed summary

---

## Configuration for Different Test Cases

### Test Case 1: Minimum Parallel Threshold (20 txs)

```bash
./target/release/tx_generator --count 20 --batch-size 20
```

**Expected**: Exactly at threshold, parallel execution should be triggered

### Test Case 2: Large Block (100 txs)

```bash
./target/release/tx_generator --count 100 --batch-size 100 --different-senders
```

**Expected**: Maximum parallelization benefit (conflict-free + large batch)

### Test Case 3: Sequential Comparison (10 txs)

```bash
./target/release/tx_generator --count 10 --batch-size 10
```

**Expected**: Sequential execution (below threshold), baseline for comparison

### Test Case 4: Conflicting Transactions

```bash
./target/release/tx_generator --count 50 --batch-size 50
# Default: same sender for all transactions
```

**Expected**: Parallel execution with conflict resolution overhead

### Test Case 5: High Throughput Stress Test

```bash
./target/release/tx_generator \
  --count 500 \
  --batch-size 50 \
  --delay-ms 1000 \
  --different-senders
```

**Expected**: 10 blocks with 50 txs each, all using parallel execution

---

## Integration with Testing Workflow

### Step 1: Start Devnet

```bash
# Clean start
rm -rf ~/tos_devnet/

# Start daemon with parallel execution enabled
./target/release/tos_daemon \
  --network devnet \
  --dir-path ~/tos_devnet/ \
  --log-level info \
  --auto-compress-logs
```

### Step 2: Generate Baseline (Sequential)

```bash
# Generate 10 transactions (sequential path)
./target/release/tx_generator --count 10 --batch-size 10 > /tmp/seq_10.log

# Check processing time
grep "Processed block" ~/tos_devnet/daemon.log | tail -1
# Example: "in 25ms with 10 txs" → ~2.5ms per tx
```

### Step 3: Generate Parallel Test (25 txs)

```bash
# Generate 25 transactions (parallel path)
./target/release/tx_generator --count 25 --batch-size 25 --different-senders > /tmp/par_25.log

# Check processing time
grep "Processed block" ~/tos_devnet/daemon.log | tail -1
# Example: "in 35ms with 25 txs" → ~1.4ms per tx (1.8x speedup!)
```

### Step 4: Generate Large Batch (100 txs)

```bash
# Generate 100 conflict-free transactions
./target/release/tx_generator --count 100 --batch-size 100 --different-senders > /tmp/par_100.log

# Check processing time
grep "Processed block" ~/tos_devnet/daemon.log | tail -1
# Expected: "in 80-120ms with 100 txs" → ~0.8-1.2ms per tx (2-3x speedup!)
```

### Step 5: Collect Metrics

```bash
# Extract all block processing times
grep "Processed block.*with [0-9]* txs" ~/tos_devnet/daemon.log | \
  awk '{print $9, $11}' | \
  sed 's/ms//' > /tmp/block_metrics.txt

# Analyze performance
# Sequential (< 20 txs): avg ~2-3ms per tx
# Parallel (≥ 20 txs):   avg ~0.8-1.5ms per tx
```

---

## Expected Performance Improvements

Based on theoretical analysis (from `memo/BENCHMARK_RESULTS.md`):

| Batch Size | Mode       | Expected Time (P=1) | Expected Time (P=4) | Speedup  |
|------------|------------|---------------------|---------------------|----------|
| 10 txs     | Sequential | 25-30ms             | 25-30ms (no change) | 1.0x     |
| 20 txs     | Parallel   | 50-60ms             | 25-30ms             | 2.0x     |
| 50 txs     | Parallel   | 125-150ms           | 50-60ms             | 2.5x     |
| 100 txs    | Parallel   | 250-300ms           | 80-120ms            | 2.5-3.0x |

**Real-world factors**:
- Signature verification: ~100µs per signature
- Balance proof verification: ~500µs - 2ms per proof
- State read/write: ~100µs - 1ms per operation
- Network I/O: Negligible (local devnet)

---

## Known Limitations

1. **Mock balances**: Tool assumes all senders have 1M TOS
   - Real network may reject transactions with insufficient balance
   - Use devnet with pre-funded accounts for realistic testing

2. **No nonce tracking**: Tool starts nonces from 0
   - May conflict with existing transactions from same address
   - Use `--different-senders` to avoid nonce issues

3. **No proof generation**: Simplified transaction building
   - Real transactions require ZK proofs for balance commitments
   - Tool focuses on infrastructure testing, not cryptographic validation

4. **RPC limitations**: Direct RPC submission bypasses some validation
   - Real P2P propagation may have additional checks
   - Use for local testing only

---

## Future Enhancements

- [ ] **Real balance proofs**: Generate valid ZK proofs for production testing
- [ ] **Nonce management**: Query account nonces from chain state
- [ ] **Account pre-funding**: Auto-fund sender accounts via faucet
- [ ] **Multi-network support**: Test across devnet/testnet/mainnet
- [ ] **Performance profiling**: Integrated flamegraph generation
- [ ] **Grafana integration**: Export metrics to Prometheus
- [ ] **Transaction templates**: Support different transaction types (contracts, multisig)

---

## References

- **Source Code**: `/Users/tomisetsu/tos-network/tos/daemon/tests/tools/tx_generator.rs`
- **Build Config**: `/Users/tomisetsu/tos-network/tos/daemon/Cargo.toml` (lines 70-72)
- **Benchmark Results**: `/Users/tomisetsu/tos-network/tos/memo/BENCHMARK_RESULTS.md`
- **Devnet Testing Guide**: `/Users/tomisetsu/tos-network/tos/memo/DEVNET_TESTING_GUIDE.md`
- **Phase 3 Integration**: `/Users/tomisetsu/tos-network/tos/memo/PHASE_3_COMPLETE.md`

---

**Last Updated**: 2025-10-27
**Version**: 1.0
**Status**: ✅ **READY FOR TESTING** (RPC integration issue needs investigation)
