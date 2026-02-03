# TCK Conformance Testing

This directory contains the multi-client conformance testing infrastructure for validating that different TOS implementations produce identical results.

## Overview

Conformance testing ensures that:
- All client implementations produce identical state digests
- Transaction execution results match across implementations
- Error codes are consistent
- Block execution order is deterministic

## Architecture

```
+-----------------------------------------------------------+
|                   Docker Orchestrator                      |
+-----------------------------------------------------------+
|    TOS Rust      |    Avatar C       |    Future Go       |
|    Container     |    Container      |    Container       |
+--------+---------+---------+---------+----------+---------+
         |                   |                    |
         +-------------------+--------------------+
                             |
                             v
         +-------------------------------------------+
         |    Shared Vectors + Result Comparison     |
         |    - Same TX -> Same result               |
         |    - Same block -> Same state             |
         |    - Fuzz inputs -> No divergence         |
         +-------------------------------------------+
```

## Directory Structure

```
conformance/
├── README.md                 # This file
├── docker-compose.yml        # Multi-client orchestration
├── Dockerfile.tos-rust       # TOS Rust conformance image
├── harness/                  # Python test driver
│   ├── requirements.txt
│   ├── __init__.py
│   ├── runner.py             # Main test runner
│   ├── comparator.py         # Result comparison logic
│   ├── reporter.py           # Report generation
│   └── config.py             # Configuration
├── api/                      # API specifications
│   ├── openapi.yaml          # Conformance API schema
│   └── README.md
└── results/                  # Test results (gitignored)
    └── .gitkeep
```

## Quick Start

### Prerequisites

- Docker and Docker Compose
- Python 3.9+
- TOS Rust built with conformance features
- Avatar C built with conformance features

### Running Tests

```bash
# Build containers
docker-compose build

# Run all conformance tests
docker-compose up

# Run specific vector suite
docker-compose run orchestrator python runner.py --vectors=/vectors/state/transfer.yaml

# Run with verbose output
docker-compose run orchestrator python runner.py --verbose
```

### Local Development

```bash
# Install Python dependencies
cd harness
pip install -r requirements.txt

# Run harness against local endpoints
python runner.py \
    --rust-endpoint http://localhost:8081 \
    --c-endpoint http://localhost:8082 \
    --vectors ../vectors/
```

## Conformance API

Each client implementation must expose the Conformance API (see `api/openapi.yaml`):

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/state/reset` | POST | Reset to genesis state |
| `/state/load` | POST | Load state from JSON |
| `/state/digest` | GET | Get current state digest |
| `/state/account/{address}` | GET | Get account state |
| `/tx/execute` | POST | Execute single transaction |
| `/block/execute` | POST | Execute block of transactions |

## Test Vectors

Vectors are loaded from `tck/vectors/`:

- `vectors/state/*.yaml` - State transition tests
- `vectors/execution/*.yaml` - Block execution tests
- `vectors/errors/*.yaml` - Error scenario tests

## Adding a New Client

1. Implement the Conformance API (see `api/openapi.yaml`)
2. Create Dockerfile in this directory
3. Add service to `docker-compose.yml`
4. Update `harness/config.py` with new endpoint
5. Run full conformance suite

## Divergence Handling

When a divergence is detected:

1. Test harness logs the divergence details
2. State dumps are saved to `results/divergences/`
3. Investigation should follow the process in `MULTI_CLIENT_ALIGNMENT.md`

## CI/CD Integration

The conformance tests run in CI on every PR:

```yaml
# .github/workflows/conformance.yml
- name: Run conformance tests
  working-directory: tck/conformance
  run: docker-compose up --exit-code-from orchestrator
```

## Related Documentation

- `MULTI_CLIENT_ALIGNMENT.md` - Methodology overview
- `MULTI_CLIENT_ALIGNMENT_SCHEME.md` - Technical specifications
- `tck/specs/` - Critical path specifications
- `tck/vectors/` - Test vectors
