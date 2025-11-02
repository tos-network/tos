# Tos Wallet Batch Mode Tests

This directory contains Python test scripts for testing the Tos wallet's batch mode functionality. The tests are split into multiple files based on command categories to make debugging easier.

## Test Files

### Individual Command Tests

- **`test_basic_commands.py`** - Tests basic commands
  - Tests: `help`, `version`, `exit`, `set_log_level`

- **`test_display_address.py`** - Tests the `display_address` command
  - Tests: `display_address` (no parameters)

- **`test_list_commands.py`** - Tests list-related commands
  - Tests: `list_balances`, `list_assets`, `list_tracked_assets` (with optional page parameters)

- **`test_balance_commands.py`** - Tests balance and asset-related commands
  - Tests: `balance`, `track_asset`, `untrack_asset`, `set_asset_name` (with hash parameters)

- **`test_energy_commands.py`** - Tests energy-related commands
  - Tests: `energy_info` (no parameters), `freeze_tos`, `unfreeze_tos` (with amount, duration, confirm parameters)

- **`test_transaction_commands.py`** - Tests transaction-related commands
  - Tests: `transfer`, `transfer_all`, `burn` (with various parameters)

- **`test_utility_commands.py`** - Tests utility commands
  - Tests: `status`, `nonce`, `tx_version`, `history`, `seed`, `online_mode`, `offline_mode`, `rescan`, `export_transactions`

- **`test_wallet_management.py`** - Tests wallet management commands
  - Tests: `change_password`, `logout`, `transaction`, `set_nonce`, `set_tx_version`, `clear_tx_cache`

- **`test_server_commands.py`** - Tests server-related commands
  - Tests: `start_rpc_server`, `start_xswd`, `stop_api_server`, `add_xswd_relayer`

- **`test_multisig_commands.py`** - Tests multisig commands
  - Tests: `multisig_setup`, `multisig_sign`, `multisig_show`

### Test Runner

- **`run_all_tests.py`** - Main test runner that can execute all tests or specific categories
- **`wallet_batch_test.py`** - Original comprehensive test file (kept for reference)

## Usage

### Running Individual Tests

```bash
# Test basic commands
python3 tests/test_basic_commands.py

# Test display_address command
python3 tests/test_display_address.py

# Test list commands
python3 tests/test_list_commands.py

# Test balance commands
python3 tests/test_balance_commands.py

# Test energy commands
python3 tests/test_energy_commands.py

# Test transaction commands
python3 tests/test_transaction_commands.py

# Test utility commands
python3 tests/test_utility_commands.py

# Test wallet management commands
python3 tests/test_wallet_management.py

# Test server commands
python3 tests/test_server_commands.py

# Test multisig commands
python3 tests/test_multisig_commands.py
```

### Running All Tests

```bash
# Run all tests
python3 tests/run_all_tests.py

# Run specific test categories
python3 tests/run_all_tests.py --basic
python3 tests/run_all_tests.py --display
python3 tests/run_all_tests.py --list
python3 tests/run_all_tests.py --balance
python3 tests/run_all_tests.py --energy
python3 tests/run_all_tests.py --transaction
python3 tests/run_all_tests.py --utility
python3 tests/run_all_tests.py --wallet-management
python3 tests/run_all_tests.py --server
python3 tests/run_all_tests.py --multisig
```

### Running via Cargo

```bash
# Run all Python tests via Rust
cargo test --test python_integration_tests

# Run specific test categories
cargo test test_display_address_command
cargo test test_list_commands
cargo test test_balance_commands
cargo test test_energy_commands
cargo test test_transaction_commands
cargo test test_utility_commands
```

## Test Structure

Each test file follows this structure:

1. **Setup**: Initialize wallet binary path and test parameters
2. **Command Testing**: Test various command parameter combinations
3. **Result Validation**: Check success/failure and provide detailed output
4. **Summary**: Report overall test results

## Failure Handling

The test runner includes **fail-fast behavior**: when any test fails, the runner immediately stops and does not continue with remaining tests. This helps with debugging by focusing on the first issue encountered.

**Example:**
```bash
# If display_address fails, the runner stops immediately
python3 tests/run_all_tests.py --display --list --balance
# Only display_address runs, list and balance are skipped if display_address fails
```

## Batch Mode Understanding

The wallet's batch mode works as follows:

- **Global Options**: `--wallet-path` and `--password` are global options for the wallet executable
- **Batch Mode**: `--batch-mode --cmd "command_name param1 param2"` executes a single command
- **Parameter Format**: Parameters are space-separated and passed as a single string to `--cmd`
- **No Interactive Input**: All parameters must be provided upfront

## Example Commands

```bash
# Display address (no parameters)
../target/debug/tos_wallet --batch-mode --cmd "display_address" --wallet-path test_wallet --password test123

# List balances with page parameter
../target/debug/tos_wallet --batch-mode --cmd "list_balances 1" --wallet-path test_wallet --password test123

# Freeze TOS with parameters
../target/debug/tos_wallet --batch-mode --cmd "freeze_tos 100000000 7 yes" --wallet-path test_wallet --password test123

# Transfer with parameters
../target/debug/tos_wallet --batch-mode --cmd "transfer tos1address 100000000 tos yes" --wallet-path test_wallet --password test123
```

## Requirements

- Python 3.6+
- Cargo and Rust toolchain
- tos-wallet binary must be built (`cargo build --bin tos_wallet`)

## Troubleshooting

### Common Issues

1. **Wallet Binary Not Found**: Ensure `cargo build --bin tos_wallet` has been run
2. **Test Wallet Not Found**: Tests create a test wallet automatically
3. **Command Parameters**: Ensure parameters are space-separated, not `key=value` format
4. **Hash Parameters**: Use 64-character hex strings for hash parameters

### Debugging

- Check the test output for detailed error messages
- Run individual test files to isolate issues
- Verify wallet binary exists and is executable
- Check that test wallet directory is writable

## Test Categories

### No-Parameter Commands
Commands that don't require any parameters:
- `display_address`
- `status`
- `energy_info`
- `nonce`
- `tx_version`
- `logout`
- `clear_tx_cache`
- `offline_mode`
- `multisig_show`

### Optional Parameter Commands
Commands with optional parameters:
- `list_balances [page]`
- `list_assets [page]`
- `list_tracked_assets [page]`
- `history [page]`
- `seed [language]`
- `online_mode [daemon_address]`
- `rescan [topoheight]`

### Required Parameter Commands
Commands that require specific parameters:
- `export_transactions <filename>`
- `freeze_tos <amount> <duration> <confirm>`
- `unfreeze_tos <amount> <confirm>`
- `set_asset_name <hash>`
- `balance <hash>`
- `track_asset <hash>`
- `untrack_asset <hash>`
- `transfer <asset> <address> <amount> <fee_type> <confirm>`
- `transfer_all <asset> <address> <fee_type> <confirm>`
- `burn <amount> <confirm>` 
