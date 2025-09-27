# TOS AI Miner

TOS AI Mining system implementation and testing tools.

## Test Scripts

### Python Test Scripts
- **`simple_ai_test.py`** - Simplified AI mining workflow test
  - Tests basic daemon connection
  - Simulates complete AI mining workflow
  - Generates test results in `simple_ai_test_result.json`

- **`ai_mining_test.py`** - Comprehensive AI mining test suite
  - Full workflow testing from task publication to reward distribution
  - Includes detailed transaction simulation
  - Generates test results in `ai_mining_test_result.json`

### Running Tests

```bash
# Simple test
python3 simple_ai_test.py

# Comprehensive test
python3 ai_mining_test.py
```

### Requirements
- TOS daemon running on devnet
- Python 3.7+
- No external dependencies (uses built-in urllib)

## Binary

The main AI miner binary is built with:
```bash
cargo build --release --bin tos_ai_miner
```

Binary location: `../target/release/tos_ai_miner` (or `../target/debug/tos_ai_miner` for debug builds)