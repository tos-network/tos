# Devnet Testing Guide - Parallel Transaction Execution

## Overview

This guide provides step-by-step instructions for testing the parallel transaction execution feature on the TOS devnet. The parallel execution system allows independent transactions to be processed concurrently, improving throughput for blocks with many transactions.

**Feature Status**: Currently DISABLED by default (`PARALLEL_EXECUTION_ENABLED = false`)

**Testing Goal**: Verify parallel execution works correctly on devnet before mainnet deployment

**Estimated Time**: 6-10 hours for comprehensive testing

## Prerequisites

Before starting, ensure you have:

- [ ] Devnet node built and ready (`cargo build --release --package tos_daemon`)
- [ ] Wallet configured with test TOS tokens (minimum 1000 TOS for testing)
- [ ] Development environment set up (see CLAUDE.md)
- [ ] Monitoring tools ready (log viewer, system monitor)
- [ ] Backup of devnet data directory (`~/tos_devnet/`)

## Architecture Overview

### How Parallel Execution Works

1. **Conflict Detection**: System analyzes transactions to identify independent sets
2. **Parallel Processing**: Independent transactions execute concurrently using Rayon
3. **Result Merging**: Parallel results are deterministically merged into final state
4. **Fallback**: System falls back to sequential execution if conflicts detected

### Key Thresholds

```rust
// Minimum transactions required to trigger parallel execution
MIN_TXS_FOR_PARALLEL = 20

// Parallel execution disabled by default
PARALLEL_EXECUTION_ENABLED = false
```

### Files Involved

- `daemon/src/config.rs` - Feature flag
- `daemon/src/core/blockchain.rs` - Main execution logic
- `daemon/src/core/executor/` - Parallel execution engine
- `daemon/src/core/nonce_checker.rs` - Nonce validation

## Phase 1: Enable Parallel Execution (30 minutes)

### Step 1.1: Backup Current State

```bash
# Stop any running daemon
pkill tos_daemon

# Backup devnet data
cp -r ~/tos_devnet/ ~/tos_devnet_backup_$(date +%Y%m%d_%H%M%S)
```

### Step 1.2: Modify Configuration

**File**: `/Users/tomisetsu/tos-network/tos/daemon/src/config.rs`

**Change**:
```rust
/// Enable parallel transaction execution (default: false for safety)
pub const PARALLEL_EXECUTION_ENABLED: bool = false;
```

**To**:
```rust
/// Enable parallel transaction execution (TESTING ON DEVNET)
pub const PARALLEL_EXECUTION_ENABLED: bool = true;
```

### Step 1.3: Rebuild Daemon

```bash
cd /Users/tomisetsu/tos-network/tos

# Clean build (recommended)
cargo clean --package tos_daemon

# Build with release optimizations
cargo build --release --package tos_daemon

# Verify build
./target/release/tos_daemon --version
```

**Expected Output**: No warnings or errors

### Step 1.4: Start Devnet with Monitoring

```bash
# Start daemon with verbose logging
./target/release/tos_daemon \
    --network devnet \
    --dir-path ~/tos_devnet/ \
    --log-level info \
    --auto-compress-logs \
    2>&1 | tee devnet_parallel_test.log
```

**Initial Checks**:
- Daemon starts without errors
- Blockchain syncs normally
- No immediate crashes

## Phase 2: Monitor Logs (Continuous)

### Success Indicators

**Parallel Execution Triggered**:
```
[INFO] Using parallel execution for 25 transactions in block 0x1234abcd...
[INFO] Merged parallel results: 25 nonces, 50 balances, gas=250, burned=25
```

**Sequential Fallback (Expected for small blocks)**:
```
[DEBUG] Using sequential execution for 15 transactions (below threshold)
```

**No State Errors**:
```
[INFO] Block 0x1234abcd validated successfully
[INFO] Balance verification passed
```

### Warning Signs

**Critical Issues** (require immediate rollback):
```
[ERROR] Parallel execution failed: state inconsistency
[ERROR] Nonce gap detected after parallel execution
[ERROR] Balance mismatch: expected X, got Y
[ERROR] Merkle root mismatch after parallel execution
```

**Performance Issues** (investigate but not critical):
```
[WARN] Parallel execution slower than sequential
[WARN] High conflict rate: 80% transactions conflicting
```

### Log Monitoring Commands

```bash
# Monitor parallel execution events
tail -f devnet_parallel_test.log | grep "parallel execution"

# Monitor errors
tail -f devnet_parallel_test.log | grep -i "error\|warn"

# Count parallel vs sequential blocks
grep "Using parallel execution" devnet_parallel_test.log | wc -l
grep "Using sequential execution" devnet_parallel_test.log | wc -l
```

## Phase 3: Generate Test Load (1-2 hours)

### Preparation

**Verify Wallet Balance**:
```bash
./target/release/tos_wallet balance
```

**Expected**: At least 1000 TOS for testing

### Test 3.1: Small Batch (Verify Threshold)

**Goal**: Confirm parallel doesn't trigger below MIN_TXS_FOR_PARALLEL (20 txs)

```bash
# Send 15 transactions (below threshold)
for i in {1..15}; do
    ./target/release/tos_wallet transfer \
        --amount 1 \
        --to tst1yp0hc5z0csf2jk2ze9tjjxkjg8gawt2upltksyegffmudm29z38qqrkvqzk
    sleep 0.5
done
```

**Expected Log**: "Using sequential execution for 15 transactions"

### Test 3.2: Large Batch (Trigger Parallel)

**Goal**: Trigger parallel execution with 30+ transactions

```bash
# Send 30 transactions (above threshold)
for i in {1..30}; do
    ./target/release/tos_wallet transfer \
        --amount 1 \
        --to tst1yp0hc5z0csf2jk2ze9tjjxkjg8gawt2upltksyegffmudm29z38qqrkvqzk
    sleep 0.5
done
```

**Expected Logs**:
```
[INFO] Using parallel execution for 30 transactions in block 0x...
[INFO] Merged parallel results: 30 nonces, 60 balances, gas=300, burned=30
```

### Test 3.3: Alternating Addresses (Low Conflict)

**Goal**: Test parallel with independent transactions

```bash
# Create script for alternating addresses
cat > test_parallel_independent.sh << 'EOF'
#!/bin/bash
ADDR1="tst12zacnuun3lkv5kxzn2jy8l28d0zft7rqhyxlz2v6h6u23xmruy7sqm0d38u"
ADDR2="tst1yp0hc5z0csf2jk2ze9tjjxkjg8gawt2upltksyegffmudm29z38qqrkvqzk"

for i in {1..25}; do
    if [ $((i % 2)) -eq 0 ]; then
        ./target/release/tos_wallet transfer --amount 1 --to $ADDR1
    else
        ./target/release/tos_wallet transfer --amount 1 --to $ADDR2
    fi
    sleep 0.5
done
EOF

chmod +x test_parallel_independent.sh
./test_parallel_independent.sh
```

**Expected**: High parallel success rate (low conflicts)

### Test 3.4: Same Address (High Conflict)

**Goal**: Test sequential fallback with dependent transactions

```bash
# Send 25 transactions to same address (creates dependencies)
for i in {1..25}; do
    ./target/release/tos_wallet transfer \
        --amount 1 \
        --to tst12zacnuun3lkv5kxzn2jy8l28d0zft7rqhyxlz2v6h6u23xmruy7sqm0d38u
    sleep 0.5
done
```

**Expected**: May trigger sequential fallback or handle conflicts correctly

## Phase 4: Validation (4-6 hours)

### Test 4.1: State Consistency

**Goal**: Verify balances are correct after parallel execution

```bash
# Record initial balance
INITIAL_BALANCE=$(./target/release/tos_wallet balance | grep "Balance:" | awk '{print $2}')
echo "Initial balance: $INITIAL_BALANCE"

# Send 50 transactions of 1 TOS each
for i in {1..50}; do
    ./target/release/tos_wallet transfer \
        --amount 1 \
        --to tst1yp0hc5z0csf2jk2ze9tjjxkjg8gawt2upltksyegffmudm29z38qqrkvqzk
    sleep 0.5
done

# Wait for confirmations
sleep 30

# Check final balance
FINAL_BALANCE=$(./target/release/tos_wallet balance | grep "Balance:" | awk '{print $2}')
echo "Final balance: $FINAL_BALANCE"

# Calculate difference (should be 50 + fees)
# Note: Account for transaction fees
```

**Success Criteria**:
- Balance decreased by exactly 50 TOS + fees
- No missing or duplicate transactions
- Blockchain state consistent

### Test 4.2: Nonce Correctness

**Goal**: Verify nonces increment correctly with parallel execution

```bash
# Get current nonce
CURRENT_NONCE=$(./target/release/tos_wallet info | grep "Nonce:" | awk '{print $2}')
echo "Starting nonce: $CURRENT_NONCE"

# Send 30 sequential transactions
for i in {1..30}; do
    ./target/release/tos_wallet transfer \
        --amount 1 \
        --to tst1yp0hc5z0csf2jk2ze9tjjxkjg8gawt2upltksyegffmudm29z38qqrkvqzk
    sleep 0.5
done

# Wait for confirmations
sleep 30

# Check final nonce
FINAL_NONCE=$(./target/release/tos_wallet info | grep "Nonce:" | awk '{print $2}')
echo "Final nonce: $FINAL_NONCE"

# Verify increment
EXPECTED_NONCE=$((CURRENT_NONCE + 30))
if [ "$FINAL_NONCE" -eq "$EXPECTED_NONCE" ]; then
    echo "✓ Nonce increment correct"
else
    echo "✗ Nonce mismatch! Expected $EXPECTED_NONCE, got $FINAL_NONCE"
fi
```

**Success Criteria**:
- Nonce increments by exactly 30
- No nonce gaps
- No duplicate nonces

### Test 4.3: Parallel vs Sequential Performance

**Goal**: Measure performance improvement (optional)

```bash
# Test 1: With parallel execution (current state)
echo "Testing with parallel execution..."
START_TIME=$(date +%s)

for i in {1..50}; do
    ./target/release/tos_wallet transfer \
        --amount 1 \
        --to tst1yp0hc5z0csf2jk2ze9tjjxkjg8gawt2upltksyegffmudm29z38qqrkvqzk
    sleep 0.5
done

# Wait for all blocks to be mined
sleep 60

END_TIME=$(date +%s)
PARALLEL_TIME=$((END_TIME - START_TIME))
echo "Parallel execution time: ${PARALLEL_TIME}s"

# Test 2: Disable parallel, rebuild, repeat
# (Requires manual configuration change and rebuild)
```

**Note**: Performance comparison is optional and requires disabling/re-enabling the feature.

### Test 4.4: Block Validation

**Goal**: Verify merkle roots and blue scores are correct

```bash
# Monitor block validation logs
tail -f devnet_parallel_test.log | grep "Block.*validated"
```

**Expected**:
```
[INFO] Block 0x1234... validated successfully
[INFO] Merkle root verified: 0xabcd...
[INFO] Blue score increment valid
```

**Success Criteria**:
- All blocks validate successfully
- No merkle root mismatches
- No blue score inconsistencies

## Phase 5: Stress Testing (Optional, 8+ hours)

### Test 5.1: Large Batch Stress Test

**Goal**: Test with 100+ transactions in single block

```bash
# Create large batch script
cat > stress_test_large_batch.sh << 'EOF'
#!/bin/bash
echo "Starting large batch stress test..."

for i in {1..100}; do
    ./target/release/tos_wallet transfer \
        --amount 1 \
        --to tst1yp0hc5z0csf2jk2ze9tjjxkjg8gawt2upltksyegffmudm29z38qqrkvqzk &

    # Throttle to avoid overwhelming daemon
    if [ $((i % 10)) -eq 0 ]; then
        wait
        sleep 2
    fi
done

wait
echo "Large batch stress test complete"
EOF

chmod +x stress_test_large_batch.sh
./stress_test_large_batch.sh
```

**Monitor**:
- Memory usage: `top -p $(pgrep tos_daemon)`
- Log for errors
- Block processing time

### Test 5.2: Sustained Load Test

**Goal**: Run for 1000+ blocks to detect memory leaks

```bash
# Run continuous transaction generation
cat > sustained_load_test.sh << 'EOF'
#!/bin/bash
BLOCKS=0
MAX_BLOCKS=1000

echo "Starting sustained load test for $MAX_BLOCKS blocks..."

while [ $BLOCKS -lt $MAX_BLOCKS ]; do
    # Send batch of 25 transactions
    for i in {1..25}; do
        ./target/release/tos_wallet transfer \
            --amount 1 \
            --to tst1yp0hc5z0csf2jk2ze9tjjxkjg8gawt2upltksyegffmudm29z38qqrkvqzk
        sleep 0.5
    done

    BLOCKS=$((BLOCKS + 1))
    echo "Progress: $BLOCKS/$MAX_BLOCKS blocks"

    # Monitor memory every 100 blocks
    if [ $((BLOCKS % 100)) -eq 0 ]; then
        ps aux | grep tos_daemon | grep -v grep
    fi

    sleep 30
done

echo "Sustained load test complete"
EOF

chmod +x sustained_load_test.sh
./sustained_load_test.sh
```

**Monitor for**:
- Memory leaks (RSS should stabilize)
- Performance degradation
- Error accumulation

### Test 5.3: Mixed Transaction Types

**Goal**: Test with different transaction types (transfers, smart contracts, etc.)

**Note**: This requires smart contract deployment capabilities. Test transfer transactions thoroughly first.

## Rollback Procedure

### When to Rollback

**Critical Issues** (rollback immediately):
- State inconsistencies (balance/nonce mismatches)
- Blockchain halts or crashes
- Merkle root validation failures
- Data corruption

**Non-Critical Issues** (investigate, rollback optional):
- Performance worse than sequential
- High conflict rates
- Excessive memory usage

### Rollback Steps

#### Step 1: Disable Parallel Execution

**File**: `/Users/tomisetsu/tos-network/tos/daemon/src/config.rs`

**Change back**:
```rust
/// Parallel execution DISABLED after devnet issues
pub const PARALLEL_EXECUTION_ENABLED: bool = false;
```

#### Step 2: Rebuild

```bash
cd /Users/tomisetsu/tos-network/tos

# Clean build
cargo clean --package tos_daemon
cargo build --release --package tos_daemon
```

#### Step 3: Restart Daemon

```bash
# Stop current daemon
pkill tos_daemon

# Restart with sequential execution
./target/release/tos_daemon \
    --network devnet \
    --dir-path ~/tos_devnet/ \
    --log-level info \
    --auto-compress-logs \
    2>&1 | tee devnet_sequential_recovery.log
```

#### Step 4: Verify Recovery

```bash
# Check blockchain syncs
tail -f devnet_sequential_recovery.log | grep "Block.*accepted"

# Verify balances
./target/release/tos_wallet balance

# Check nonces
./target/release/tos_wallet info
```

#### Step 5: Restore from Backup (If Corrupted)

```bash
# Stop daemon
pkill tos_daemon

# Restore backup
rm -rf ~/tos_devnet/
cp -r ~/tos_devnet_backup_* ~/tos_devnet/

# Restart
./target/release/tos_daemon --network devnet --dir-path ~/tos_devnet/ --log-level info
```

## Success Criteria

Before declaring devnet testing successful:

- [ ] **100+ blocks processed with parallel execution enabled**
- [ ] **No state inconsistencies detected** (balances/nonces correct)
- [ ] **All validation tests passed** (merkle roots, blue scores)
- [ ] **No blockchain halts or crashes** (stability over 24+ hours)
- [ ] **Parallel execution triggered consistently** (for blocks with 20+ txs)
- [ ] **Sequential fallback works correctly** (for conflicting transactions)
- [ ] **Memory usage stable** (no leaks over sustained load)
- [ ] **Nonce increments correct** (no gaps or duplicates)
- [ ] **Performance acceptable** (not worse than sequential)

## Troubleshooting

### Issue: Parallel Execution Never Triggers

**Symptoms**:
```
[DEBUG] Using sequential execution for 30 transactions
```

**Possible Causes**:
1. Feature flag not enabled (check `config.rs`)
2. Transactions below threshold (< 20 txs per block)
3. High conflict rate (system falls back to sequential)

**Solutions**:
```bash
# Verify feature flag
grep "PARALLEL_EXECUTION_ENABLED" daemon/src/config.rs

# Lower threshold temporarily for testing
# Edit daemon/src/config.rs
pub const MIN_TXS_FOR_PARALLEL: usize = 10;

# Rebuild and test
cargo build --release --package tos_daemon
```

### Issue: State Inconsistency Detected

**Symptoms**:
```
[ERROR] Balance mismatch: expected 1000, got 998
[ERROR] Nonce gap detected: expected 50, found 48
```

**Immediate Action**: ROLLBACK (see Rollback Procedure)

**Investigation**:
```bash
# Check logs for merge errors
grep "merge_parallel_results" devnet_parallel_test.log

# Check for race conditions
grep "parallel execution failed" devnet_parallel_test.log
```

**Report Bug**: Include logs and reproduction steps in GitHub issue

### Issue: Performance Worse Than Sequential

**Symptoms**:
```
[WARN] Parallel execution time: 5.2s vs sequential: 2.1s
```

**Possible Causes**:
1. High conflict rate (too many dependencies)
2. Small transaction count (overhead dominates)
3. System resource constraints

**Solutions**:
```bash
# Increase threshold to require more transactions
pub const MIN_TXS_FOR_PARALLEL: usize = 50;

# Check system resources
top -p $(pgrep tos_daemon)

# Profile with larger batches
# Send 100+ transactions and measure
```

### Issue: High Memory Usage

**Symptoms**:
- RSS grows continuously
- System becomes unresponsive

**Investigation**:
```bash
# Monitor memory over time
while true; do
    ps aux | grep tos_daemon | grep -v grep
    sleep 60
done >> memory_monitor.log

# Check for leaks in parallel executor
grep "parallel execution" devnet_parallel_test.log | wc -l
```

**Solutions**:
- Reduce `MIN_TXS_FOR_PARALLEL` to limit batch size
- Add memory limits in code
- Rollback if memory leak confirmed

### Issue: Blockchain Halts

**Symptoms**:
- No new blocks accepted
- Daemon unresponsive

**Immediate Action**:
```bash
# Check if daemon is alive
ps aux | grep tos_daemon

# Check logs for panic
tail -100 devnet_parallel_test.log

# Restart if necessary
pkill tos_daemon
./target/release/tos_daemon --network devnet --dir-path ~/tos_devnet/ --log-level debug
```

**If persistent**: ROLLBACK immediately

## Data Collection for Mainnet Decision

During testing, collect the following metrics:

### Performance Metrics

```bash
# Count parallel vs sequential executions
PARALLEL_COUNT=$(grep -c "Using parallel execution" devnet_parallel_test.log)
SEQUENTIAL_COUNT=$(grep -c "Using sequential execution" devnet_parallel_test.log)

echo "Parallel executions: $PARALLEL_COUNT"
echo "Sequential executions: $SEQUENTIAL_COUNT"
echo "Parallel percentage: $(( PARALLEL_COUNT * 100 / (PARALLEL_COUNT + SEQUENTIAL_COUNT) ))%"
```

### Stability Metrics

```bash
# Count errors
ERROR_COUNT=$(grep -c "ERROR" devnet_parallel_test.log)
WARN_COUNT=$(grep -c "WARN" devnet_parallel_test.log)

echo "Errors: $ERROR_COUNT"
echo "Warnings: $WARN_COUNT"
```

### Resource Usage

```bash
# Average memory usage (manual monitoring)
# Peak CPU usage (manual monitoring)
# Average block processing time (from logs)
```

### Test Coverage

- [ ] Small blocks (< 20 txs): Sequential
- [ ] Medium blocks (20-50 txs): Parallel
- [ ] Large blocks (50-100 txs): Parallel
- [ ] Very large blocks (100+ txs): Parallel
- [ ] Low conflict transactions: Parallel success
- [ ] High conflict transactions: Sequential fallback
- [ ] Mixed transaction types: All validated

## Next Steps After Devnet Success

1. **Document Results**: Create summary report with metrics
2. **Code Review**: Final review of parallel execution code
3. **Testnet Deployment**: Enable on testnet with wider testing
4. **Mainnet Preparation**: Plan phased rollout strategy
5. **Monitoring Setup**: Configure alerts for mainnet deployment

## References

- **CLAUDE.md**: Project coding standards
- **daemon/src/config.rs**: Feature flag configuration
- **daemon/src/core/blockchain.rs**: Main execution logic (lines 2071-2156)
- **daemon/src/core/executor/**: Parallel execution engine
- **DEVNET_TESTING_CHECKLIST.md**: Quick reference checklist

## Support

For issues or questions:
1. Check troubleshooting section above
2. Review logs in `devnet_parallel_test.log`
3. Create GitHub issue with logs and reproduction steps
4. Contact TOS development team

---

**Document Version**: 1.0
**Last Updated**: 2025-10-27
**Author**: TOS Development Team
**Status**: Ready for Devnet Testing
