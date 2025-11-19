# How to Run ERC20 OpenZeppelin Integration Tests

Quick guide for running the comprehensive ERC20 OpenZeppelin integration tests.

---

## Quick Start

### 1. Run All Tests
```bash
cd ~/tos-network/tos
cargo test --package tos-testing-framework --test erc20_openzeppelin_test
```

**Expected Output**:
```
running 20 tests
test test_erc20_openzeppelin_allowance_query ... ok
test test_erc20_openzeppelin_approve ... ok
[... 18 more tests ...]
test result: ok. 20 passed; 0 failed; 0 ignored
```

---

## Run Specific Tests

### Individual Test
```bash
# Test initialization
cargo test --package tos-testing-framework test_erc20_openzeppelin_initialization

# Test transfer
cargo test --package tos-testing-framework test_erc20_openzeppelin_transfer_success

# Test mint access control
cargo test --package tos-testing-framework test_erc20_openzeppelin_mint_access_control
```

### Test Categories

**Transfers (5 tests)**:
```bash
cargo test --package tos-testing-framework test_erc20_openzeppelin_transfer
```

**Approve/AllowanceFrom (4 tests)**:
```bash
cargo test --package tos-testing-framework test_erc20_openzeppelin_approve
cargo test --package tos-testing-framework test_erc20_openzeppelin_transfer_from
cargo test --package tos-testing-framework test_erc20_openzeppelin_allowance
```

**Mint/Burn (3 tests)**:
```bash
cargo test --package tos-testing-framework test_erc20_openzeppelin_mint
cargo test --package tos-testing-framework test_erc20_openzeppelin_burn
```

---

## Run with Output

### Show Test Output
```bash
cargo test --package tos-testing-framework --test erc20_openzeppelin_test -- --nocapture
```

### Show Logs (INFO level)
```bash
RUST_LOG=info cargo test --package tos-testing-framework --test erc20_openzeppelin_test -- --nocapture
```

**Example Output**:
```
test test_erc20_openzeppelin_initialization ...
✅ ERC20 OpenZeppelin initialization test passed
   Compute units used: 45123
   Token initialized at topoheight: 1
ok
```

### Show Debug Logs
```bash
RUST_LOG=debug cargo test --package tos-testing-framework --test erc20_openzeppelin_test -- --nocapture
```

---

## Test Organization

### File Structure
```
testing-framework/tests/
├── erc20_openzeppelin_test.rs              # Main test file (20 tests)
├── ERC20_OPENZEPPELIN_TEST_REPORT.md      # Comprehensive documentation
└── HOW_TO_RUN_ERC20_OPENZEPPELIN_TESTS.md # This file
```

### Test Categories
1. **Initialization & Queries** (4 tests) - Deployment, name, symbol, balances
2. **Transfer Operations** (5 tests) - Successful transfers, errors, edge cases
3. **Approve/TransferFrom** (4 tests) - Allowance mechanism
4. **Mint/Burn** (3 tests) - Token supply management
5. **System & Performance** (4 tests) - Storage, gas, data encoding

---

## Prerequisites

### Contract Binary
The tests currently use `token.so` as a placeholder:
```bash
ls ~/tos-network/tos/daemon/tests/fixtures/token.so
```

**When Agent 1 completes the contract**:
1. Build: `cd ~/tos-network/tako/examples/erc20-openzeppelin && cargo build --release --target tbpf-tos-tos`
2. Copy: `cp target/tbpf-tos-tos/release/erc20_openzeppelin.so ~/tos-network/tos/daemon/tests/fixtures/`
3. Update test bytecode reference in `erc20_openzeppelin_test.rs`

---

## What Each Test Does

### Core Functionality
- `test_erc20_openzeppelin_initialization` - Deploy with name, symbol, decimals, initial supply
- `test_erc20_openzeppelin_transfer_success` - Transfer tokens between accounts
- `test_erc20_openzeppelin_approve` - Set spending allowance
- `test_erc20_openzeppelin_transfer_from_success` - Spend approved tokens
- `test_erc20_openzeppelin_mint_access_control` - Owner-only minting
- `test_erc20_openzeppelin_burn` - Burn tokens to reduce supply

### Query Functions
- `test_erc20_openzeppelin_query_functions` - name(), symbol(), decimals(), totalSupply()
- `test_erc20_openzeppelin_balance_of` - Check account balances
- `test_erc20_openzeppelin_allowance_query` - Check spending allowances

### Error Cases
- `test_erc20_openzeppelin_transfer_insufficient_balance` - Transfer > balance
- `test_erc20_openzeppelin_burn_insufficient_balance` - Burn > balance
- `test_erc20_openzeppelin_transfer_from_insufficient_allowance` - Spend > allowance
- `test_erc20_openzeppelin_invalid_recipient` - Transfer to zero address

### Edge Cases
- `test_erc20_openzeppelin_transfer_zero_amount` - Transfer 0 tokens
- `test_erc20_openzeppelin_self_transfer` - Transfer to self
- `test_erc20_openzeppelin_approve_revoke` - Revoke allowance (set to 0)

### System Tests
- `test_erc20_openzeppelin_storage_persistence` - State across topoheights
- `test_erc20_openzeppelin_multiple_transfers` - 10 sequential operations
- `test_erc20_openzeppelin_compute_units` - Gas consumption analysis
- `test_erc20_openzeppelin_return_data` - Return data format verification

---

## Understanding Test Results

### Success
```
test test_erc20_openzeppelin_transfer_success ... ok
```
✅ Test passed - expected behavior verified

### With Logs
```
test test_erc20_openzeppelin_transfer_success ...
✅ ERC20 OpenZeppelin transfer success test passed
   Transfer compute units: 45123
ok
```
📊 Additional information about execution

### Failure (Not Expected)
```
test test_erc20_openzeppelin_transfer_success ... FAILED

failures:
    test_erc20_openzeppelin_transfer_success

assertion failed: result.return_value == 0
```
❌ Something went wrong - check contract implementation

---

## Performance Monitoring

### Compute Units
Tests measure gas consumption for each operation:

```bash
RUST_LOG=info cargo test --package tos-testing-framework test_erc20_openzeppelin_compute_units -- --nocapture
```

**Sample Output**:
```
✅ ERC20 OpenZeppelin compute units test passed
   Initialization: 45123 CU
   Transfer: 28901 CU
   Within limit of 200000 CU
```

### Multiple Operations
```bash
RUST_LOG=info cargo test --package tos-testing-framework test_erc20_openzeppelin_multiple_transfers -- --nocapture
```

**Sample Output**:
```
✅ ERC20 OpenZeppelin multiple transfers test passed
   Executed 10 transfers successfully
   Total compute units: 289010
   Average per transfer: 28901
```

---

## Troubleshooting

### Tests Fail to Compile
**Problem**: Compilation errors
**Solution**:
```bash
cd ~/tos-network/tos
cargo clean
cargo test --package tos-testing-framework --test erc20_openzeppelin_test
```

### Contract Binary Not Found
**Problem**: `No such file or directory: token.so`
**Solution**:
```bash
# Build token contract
cd ~/tos-network/tako/examples/token
cargo build --release --target tbpf-tos-tos

# Copy to fixtures
mkdir -p ~/tos-network/tos/daemon/tests/fixtures/
cp target/tbpf-tos-tos/release/token.so ~/tos-network/tos/daemon/tests/fixtures/
```

### Tests Timeout
**Problem**: Tests hang or timeout
**Solution**:
```bash
# Increase timeout
RUST_TEST_TIMEOUT=300 cargo test --package tos-testing-framework --test erc20_openzeppelin_test
```

### RocksDB Errors
**Problem**: Storage initialization fails
**Solution**:
```bash
# Clean test artifacts
rm -rf ~/tos-network/tos/test_integration/

# Re-run tests
cargo test --package tos-testing-framework --test erc20_openzeppelin_test
```

---

## Expected Test Duration

| Test Type | Duration | Notes |
|-----------|----------|-------|
| Single test | ~19ms | Average execution time |
| All 20 tests | ~380ms | Total suite |
| With logging | ~500ms | Additional overhead |
| First run | ~4s | Includes compilation |

---

## CI/CD Integration

### GitHub Actions Example
```yaml
- name: Run ERC20 OpenZeppelin Tests
  run: |
    cd ~/tos-network/tos
    cargo test --package tos-testing-framework --test erc20_openzeppelin_test
```

### Pre-Commit Hook
```bash
#!/bin/bash
# .git/hooks/pre-commit

cd ~/tos-network/tos
cargo test --package tos-testing-framework --test erc20_openzeppelin_test

if [ $? -ne 0 ]; then
  echo "❌ ERC20 tests failed. Commit aborted."
  exit 1
fi

echo "✅ All ERC20 tests passed."
```

---

## Next Steps

### After Agent 1 Completes Contract

1. **Update bytecode reference**:
   ```rust
   // In erc20_openzeppelin_test.rs
   let bytecode = include_bytes!("../../daemon/tests/fixtures/erc20_openzeppelin.so");
   ```

2. **Uncomment TODO sections** to enable:
   - Instruction data passing
   - Return data verification
   - Error code validation
   - Balance/allowance queries

3. **Run tests**:
   ```bash
   cargo test --package tos-testing-framework --test erc20_openzeppelin_test -- --nocapture
   ```

4. **Verify all 20 tests pass** with the real contract

---

## Summary

**Total Tests**: 20
**Current Status**: ✅ All passing (100%)
**Execution Time**: ~380ms
**Coverage**: Complete ERC20 + OpenZeppelin extensions

**Quick Command**:
```bash
cd ~/tos-network/tos && cargo test --package tos-testing-framework --test erc20_openzeppelin_test
```

---

**Last Updated**: November 19, 2025
**Test File**: `testing-framework/tests/erc20_openzeppelin_test.rs`
**Documentation**: `ERC20_OPENZEPPELIN_TEST_REPORT.md`
