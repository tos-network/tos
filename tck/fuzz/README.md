# TOS Fuzz Testing Infrastructure

This directory contains fuzzing infrastructure for discovering edge cases and potential vulnerabilities in the TOS blockchain implementation.

## Overview

Fuzzing generates random inputs to test code paths that may not be covered by unit tests. This is critical for:
- Finding parsing bugs in wire format handling
- Discovering edge cases in transaction execution
- Testing robustness of RPC endpoints
- Ensuring contract execution safety

## Directory Structure

```
fuzz/
├── README.md             # This file
├── Cargo.toml            # Fuzz target dependencies
├── Cargo.lock
├── corpus/               # Seed inputs for fuzzers
│   ├── block/            # Block parsing seeds
│   ├── rpc/              # RPC message seeds
│   ├── syscall/          # Syscall input seeds
│   └── transaction/      # Transaction seeds
├── fuzz_targets/         # Fuzz target implementations
│   ├── fuzz_address.rs
│   ├── fuzz_block_header.rs
│   ├── fuzz_contract.rs
│   ├── fuzz_merkle.rs
│   ├── fuzz_p2p.rs
│   ├── fuzz_rpc_json.rs
│   ├── fuzz_signature.rs
│   ├── fuzz_state.rs
│   ├── fuzz_syscall.rs
│   └── fuzz_transaction.rs
├── scripts/              # Helper scripts
│   ├── run-libfuzzer.sh
│   ├── run-all.sh
│   └── minimize-corpus.sh
└── target/               # Build artifacts
```

## Quick Start

### Prerequisites

- Rust nightly toolchain
- cargo-fuzz installed: `cargo install cargo-fuzz`

### Running Fuzzers

```bash
# List available fuzz targets
cargo fuzz list

# Run a specific target
cargo fuzz run fuzz_transaction

# Run with timeout (e.g., 1 hour)
cargo fuzz run fuzz_transaction -- -max_total_time=3600

# Run with specific corpus
cargo fuzz run fuzz_transaction corpus/transaction/
```

### Running All Fuzzers

```bash
# Run all targets for 30 minutes each
./scripts/run-all.sh --timeout 1800

# Run in CI mode (stop on first crash)
./scripts/run-all.sh --ci
```

## Fuzz Targets

### Transaction Parsing (`fuzz_transaction`)

Tests transaction deserialization from arbitrary bytes:
- Wire format parsing
- Field validation
- Signature structure

### Block Header (`fuzz_block_header`)

Tests block header parsing:
- Header field validation
- Parent hash handling
- Timestamp validation

### RPC JSON (`fuzz_rpc_json`)

Tests JSON-RPC request handling:
- Method parsing
- Parameter validation
- Error handling

### Contract Execution (`fuzz_contract`)

Tests contract bytecode execution:
- Opcode handling
- Stack operations
- Memory access

### Syscall (`fuzz_syscall`)

Tests TAKO syscall handling:
- Input validation
- State transitions
- Error paths

### State (`fuzz_state`)

Tests state manipulation:
- Account operations
- Balance transfers
- Nonce handling

### P2P Messages (`fuzz_p2p`)

Tests P2P protocol message parsing:
- Message framing
- Payload validation
- Version negotiation

### Signature (`fuzz_signature`)

Tests signature verification:
- Malformed signatures
- Wrong key types
- Edge cases

### Address (`fuzz_address`)

Tests address parsing and validation:
- Bech32 decoding
- Type byte handling
- Checksum validation

### Merkle Tree (`fuzz_merkle`)

Tests Merkle tree operations:
- Tree construction
- Proof verification
- Edge cases (empty, single element)

## Corpus Management

### Adding Seeds

Add known-good inputs to improve fuzzing efficiency:

```bash
# Add a valid transaction
cp tx_valid.bin corpus/transaction/

# Add from test vectors
for f in ../crypto/*.yaml; do
    ./scripts/extract-inputs.py "$f" >> corpus/
done
```

### Minimizing Corpus

Reduce corpus to essential inputs:

```bash
cargo fuzz cmin fuzz_transaction corpus/transaction/
```

### Merging Corpus

Combine corpus from multiple fuzzing sessions:

```bash
cargo fuzz merge fuzz_transaction corpus/transaction/ /tmp/new_corpus/
```

## Crash Analysis

When a crash is found:

1. **Reproduce**:
   ```bash
   cargo fuzz run fuzz_transaction artifacts/fuzz_transaction/crash-xxx
   ```

2. **Minimize**:
   ```bash
   cargo fuzz tmin fuzz_transaction artifacts/fuzz_transaction/crash-xxx
   ```

3. **Analyze**:
   ```bash
   RUST_BACKTRACE=1 cargo fuzz run fuzz_transaction artifacts/fuzz_transaction/crash-xxx
   ```

4. **Report**: Create issue with minimized input and stack trace

## Integration with Conformance Testing

Fuzzing complements conformance testing by:
1. Finding edge cases that test vectors may miss
2. Generating inputs for cross-client differential testing
3. Discovering parsing divergences between implementations

```bash
# Export interesting corpus for conformance testing
./scripts/export-for-conformance.sh corpus/transaction/ ../vectors/fuzz/
```

## CI Integration

Fuzzing runs in CI on a schedule:

```yaml
# .github/workflows/fuzz.yml
schedule:
  - cron: '0 2 * * *'  # Daily at 2 AM

steps:
  - name: Run fuzzers (4 hours)
    run: cargo fuzz run fuzz_transaction -- -max_total_time=14400
```

## Coverage

Check code coverage from fuzzing:

```bash
# Generate coverage report
cargo fuzz coverage fuzz_transaction

# View coverage
cargo cov -- show target/coverage/fuzz_transaction
```

## Related Documentation

- `MULTI_CLIENT_ALIGNMENT.md` - Overall alignment methodology
- `MULTI_CLIENT_ALIGNMENT_SCHEME.md` - Differential testing details
- `tck/conformance/` - Conformance testing infrastructure
