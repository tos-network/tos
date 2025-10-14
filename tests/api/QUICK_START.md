# Quick Start Guide - TOS API Testing

## 5-Minute Setup

### 1. Install Python Dependencies

```bash
cd tests/api
pip3 install -r requirements.txt
```

### 2. Start Daemon

```bash
# From project root
./target/release/tos_daemon --network devnet --dir-path ~/tos_devnet/ --log-level info
```

Or if not built yet:

```bash
cargo build --release --bin tos_daemon
```

### 3. Run Tests

**Option A: Direct Python**

```bash
cd tests/api
python3 run_tests.py -v
```

**Option B: Via Cargo**

```bash
# From project root
cargo test --test api_tests test_daemon_get_info -- --ignored
```

**Option C: Using pytest directly**

```bash
cd tests/api
pytest -v
```

## Running Specific Tests

### By Test File

```bash
cd tests/api

# Network info and BPS (TIP-2)
pytest daemon/test_get_info.py -v

# GHOSTDAG APIs (TIP-2)
pytest daemon/test_ghostdag_apis.py -v

# Balance and accounts
pytest daemon/test_balance_apis.py -v

# Block queries
pytest daemon/test_block_apis.py -v

# Network and P2P
pytest daemon/test_network_apis.py -v

# Utility functions
pytest daemon/test_utility_apis.py -v
```

### By Category

```bash
# All daemon tests
pytest daemon/ -v

# Only TIP-2 related tests
pytest -m tip2 -v

# Performance tests
pytest -m performance -v
```

### Single Test Function

```bash
pytest daemon/test_get_info.py::test_get_info_bps_fields -v
pytest daemon/test_balance_apis.py::test_get_balance -v
```

### Coverage Report

```bash
# See detailed coverage
cat TEST_COVERAGE.md

# Or run with coverage tracking
pytest --cov=lib --cov-report=html
```

## Quick Validation

Test if everything works:

```bash
cd tests/api
python3 lib/rpc_client.py
```

Should output:

```
Testing RPC connection...
✓ Daemon is reachable

Network Info:
  Blue Score:    41234
  Topoheight:    41234
  BPS:           1.0
  Actual BPS:    0.95
```

## Common Issues

### Issue: `ImportError: No module named 'pytest'`

**Solution:** Install requirements

```bash
pip3 install -r tests/api/requirements.txt
```

### Issue: `Connection refused`

**Solution:** Start the daemon first

```bash
./target/release/tos_daemon --network devnet --dir-path ~/tos_devnet/
```

### Issue: `Daemon not available`

**Solution:** Check daemon URL

```bash
curl http://127.0.0.1:8080/json_rpc -X POST \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"get_info","params":[],"id":1}'
```

## Environment Variables

Configure test behavior:

```bash
# Set custom daemon URL
export TOS_DAEMON_RPC_URL=http://localhost:8080/json_rpc

# Enable debug logging
export TOS_DEBUG=1

# Run tests
pytest -v
```

## Next Steps

1. Read [README.md](README.md) for full documentation
2. Check [API_REFERENCE.md](../../docs/API_REFERENCE.md) for API specs
3. Add new tests in appropriate subdirectories
4. Run `pytest --help` to see all options

## Example: Adding a New Test

Create `tests/api/daemon/test_my_api.py`:

```python
import pytest
from lib.rpc_client import TosRpcClient

@pytest.fixture
def client():
    client = TosRpcClient()
    if not client.ping():
        pytest.skip("Daemon not available")
    return client

def test_my_api(client):
    """Test description"""
    result = client.call("my_method", [])
    assert "expected_field" in result
```

Run it:

```bash
pytest daemon/test_my_api.py -v
```

## Help

```bash
# Show all pytest options
pytest --help

# Show custom options
python3 run_tests.py --help

# List all tests without running
pytest --collect-only
```
