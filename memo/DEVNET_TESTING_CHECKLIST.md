# Devnet Testing Checklist - Parallel Transaction Execution

**Feature**: Parallel Transaction Execution
**Status**: Currently DISABLED (testing preparation)
**Target**: Enable on devnet for validation

---

## Pre-Testing Setup

- [ ] Backup devnet data directory (`cp -r ~/tos_devnet/ ~/tos_devnet_backup_$(date +%Y%m%d_%H%M%S)`)
- [ ] Verify wallet has 1000+ TOS for testing
- [ ] Build release daemon (`cargo build --release --package tos_daemon`)
- [ ] Review DEVNET_TESTING_GUIDE.md for detailed procedures

---

## Phase 1: Enable Feature (30 minutes)

- [ ] Stop running daemon (`pkill tos_daemon`)
- [ ] Set `PARALLEL_EXECUTION_ENABLED = true` in `daemon/src/config.rs`
- [ ] Rebuild daemon with no warnings (`cargo build --release --package tos_daemon`)
- [ ] Start daemon with logging (`./target/release/tos_daemon --network devnet --dir-path ~/tos_devnet/ --log-level info 2>&1 | tee devnet_parallel_test.log`)
- [ ] Verify daemon starts without errors
- [ ] Confirm blockchain syncs normally

---

## Phase 2: Basic Testing (1-2 hours)

### Test 2.1: Threshold Verification
- [ ] Send 15 transactions (below threshold)
- [ ] Verify logs show "Using sequential execution"
- [ ] Send 30 transactions (above threshold)
- [ ] Verify logs show "Using parallel execution for 30 transactions"

### Test 2.2: Independent Transactions
- [ ] Send 25 transactions alternating between two addresses
- [ ] Verify parallel execution triggered
- [ ] Verify logs show "Merged parallel results"
- [ ] Check no errors in logs

### Test 2.3: Dependent Transactions
- [ ] Send 25 transactions to same address
- [ ] Verify system handles correctly (parallel or sequential)
- [ ] Check no state errors

---

## Phase 3: Validation Testing (4-6 hours)

### Test 3.1: State Consistency
- [ ] Record initial wallet balance
- [ ] Send exactly 50 transactions of 1 TOS each
- [ ] Wait for confirmations (30+ seconds)
- [ ] Verify balance decreased by 50 TOS + fees
- [ ] Check no missing or duplicate transactions

### Test 3.2: Nonce Correctness
- [ ] Record starting nonce
- [ ] Send exactly 30 sequential transactions
- [ ] Wait for confirmations (30+ seconds)
- [ ] Verify nonce incremented by exactly 30
- [ ] Check no nonce gaps or duplicates

### Test 3.3: Block Validation
- [ ] Monitor logs for 30+ minutes
- [ ] Verify all blocks validate successfully
- [ ] Check no merkle root mismatches
- [ ] Check no blue score inconsistencies
- [ ] Verify 100+ blocks processed without errors

---

## Phase 4: Stress Testing (Optional, 8+ hours)

### Test 4.1: Large Batch
- [ ] Send 100+ transactions in rapid succession
- [ ] Monitor memory usage (no excessive growth)
- [ ] Verify all transactions processed
- [ ] Check logs for errors

### Test 4.2: Sustained Load
- [ ] Run continuous transaction generation for 1000+ blocks
- [ ] Monitor memory every 100 blocks (check for leaks)
- [ ] Verify no performance degradation
- [ ] Check daemon remains stable

---

## Success Criteria Verification

- [ ] **100+ blocks processed** with parallel execution enabled
- [ ] **Zero state inconsistencies** (balances and nonces correct)
- [ ] **All validation tests passed** (merkle roots, blue scores)
- [ ] **No crashes or halts** (stability over 24+ hours)
- [ ] **Parallel execution triggered consistently** (for 20+ tx blocks)
- [ ] **Sequential fallback works** (when needed)
- [ ] **Memory usage stable** (no leaks)
- [ ] **Performance acceptable** (not worse than sequential)

---

## Rollback Procedure (If Issues Found)

- [ ] Set `PARALLEL_EXECUTION_ENABLED = false` in `daemon/src/config.rs`
- [ ] Rebuild daemon (`cargo build --release --package tos_daemon`)
- [ ] Stop daemon (`pkill tos_daemon`)
- [ ] Restart daemon with sequential execution
- [ ] Verify blockchain continues normally
- [ ] If corrupted: restore backup (`rm -rf ~/tos_devnet/ && cp -r ~/tos_devnet_backup_* ~/tos_devnet/`)
- [ ] Document issue details for bug report

---

## Log Monitoring Checklist

### Success Indicators (Expected)
- [ ] See "Using parallel execution for N transactions" in logs
- [ ] See "Merged parallel results: N nonces, M balances" in logs
- [ ] See "Block validated successfully" after parallel execution
- [ ] No error messages related to parallel execution

### Warning Signs (Investigate)
- [ ] Check for "Parallel execution failed" errors
- [ ] Check for "state inconsistency" errors
- [ ] Check for "nonce gap" errors
- [ ] Check for "balance mismatch" errors
- [ ] Check for memory warnings
- [ ] Check for performance degradation warnings

---

## Data Collection

### Performance Metrics
- [ ] Count parallel executions: `grep -c "Using parallel execution" devnet_parallel_test.log`
- [ ] Count sequential executions: `grep -c "Using sequential execution" devnet_parallel_test.log`
- [ ] Calculate parallel percentage
- [ ] Record average block processing time (from logs)

### Stability Metrics
- [ ] Count errors: `grep -c "ERROR" devnet_parallel_test.log`
- [ ] Count warnings: `grep -c "WARN" devnet_parallel_test.log`
- [ ] Record uptime duration
- [ ] Record total blocks processed

### Resource Metrics
- [ ] Record peak memory usage (RSS from `top`)
- [ ] Record average CPU usage
- [ ] Record peak CPU usage
- [ ] Check for memory leaks over time

---

## Test Coverage Confirmation

- [ ] **Small blocks (< 20 txs)**: Sequential execution confirmed
- [ ] **Medium blocks (20-50 txs)**: Parallel execution confirmed
- [ ] **Large blocks (50-100 txs)**: Parallel execution confirmed
- [ ] **Very large blocks (100+ txs)**: Tested (stress test)
- [ ] **Low conflict transactions**: Parallel success confirmed
- [ ] **High conflict transactions**: Sequential fallback confirmed
- [ ] **Extended stability**: 24+ hours runtime without issues

---

## Post-Testing Actions

- [ ] Document all test results
- [ ] Create summary report with metrics
- [ ] Save logs for analysis (`cp devnet_parallel_test.log devnet_parallel_test_$(date +%Y%m%d).log`)
- [ ] Review code for any issues found
- [ ] Update documentation if needed
- [ ] Decide: Continue to testnet or rollback for fixes

---

## Troubleshooting Quick Reference

| Issue | Immediate Action |
|-------|------------------|
| State inconsistency | ROLLBACK immediately |
| Blockchain halts | Restart daemon, check logs, rollback if persistent |
| Parallel never triggers | Check feature flag, verify tx count > 20 |
| High memory usage | Monitor for leaks, rollback if excessive |
| Performance worse | Increase MIN_TXS_FOR_PARALLEL threshold |
| Nonce gaps | ROLLBACK immediately, report bug |
| Balance mismatches | ROLLBACK immediately, report bug |

---

## Notes

- **Estimated Total Time**: 6-10 hours for comprehensive testing
- **Critical Issues**: Require immediate rollback (state errors, crashes)
- **Non-Critical Issues**: Can be investigated (performance, high conflicts)
- **Documentation**: See DEVNET_TESTING_GUIDE.md for detailed procedures
- **Support**: Create GitHub issue with logs if problems persist

---

**Checklist Version**: 1.0
**Last Updated**: 2025-10-27
**Status**: Ready for Use
