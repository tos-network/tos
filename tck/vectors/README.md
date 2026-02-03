# TCK Test Vectors

This directory contains test vectors for multi-client alignment testing.

## Directory Structure

```
vectors/
├── crypto/          # Cryptographic primitive vectors
│   ├── sha256.yaml
│   ├── blake3.yaml
│   ├── ed25519.yaml
│   └── ...
├── wire/            # Wire format (serialization) vectors
│   ├── transfer.yaml
│   ├── burn.yaml
│   └── ...
├── state/           # State transition vectors
│   ├── transfer.yaml
│   ├── energy.yaml
│   └── ...
├── execution/       # Block execution vectors
│   ├── single-tx.yaml
│   ├── multi-tx-ordering.yaml
│   └── ...
└── errors/          # Error scenario vectors
    ├── validation-errors.yaml
    ├── resource-errors.yaml
    └── ...
```

## Vector Categories

### crypto/

Pure cryptographic operation vectors:
- Hash functions (SHA256, BLAKE3, SHA3, Keccak)
- Signatures (Ed25519, Schnorr, SECP256k1)
- Encryption (AES-GCM, ChaCha20)
- Key exchange (X25519, Curve25519)
- ZK proofs (Poseidon, BLS12-381, Bulletproofs)

**Note**: Legacy vectors remain in `tck/crypto/*.yaml` for backward compatibility.
New vectors should be added here.

### wire/

Wire format (binary serialization) vectors:
- Transaction envelope encoding
- Payload encoding per transaction type
- Signature encoding

### state/

State transition vectors with pre/post state:
```yaml
test_vectors:
  - name: "transfer_basic"
    pre_state:
      accounts: [...]
    transaction: {...}
    expected:
      status: "success"
    post_state:
      accounts: [...]
    state_digest_hex: "..."
```

### execution/

Block execution vectors:
- Transaction ordering within blocks
- Multi-transaction scenarios
- DAG reorg scenarios

### errors/

Error scenario vectors:
- Validation errors (invalid format, nonce, signature)
- Resource errors (insufficient balance, energy)
- State errors (account not found, wrong state)

## Vector Schema Version

Current schema version: **1.0**

All vectors should include:
```yaml
schema_version: 1
domain: "crypto|wire|state|execution|errors"
generator: "TOS Rust vX.X.X"
generated_at: "YYYY-MM-DDTHH:MM:SSZ"
```

## Generating Vectors

Vectors are generated from the TOS Rust implementation:

```bash
cd tck/crypto
cargo run --release --bin gen_sha256_vectors
```

For state transition vectors:
```bash
cd tck/generators/state
cargo run --release --bin gen_transfer_state
```

## Consuming Vectors

### Rust
```rust
let vectors: VectorFile = serde_yaml::from_str(&yaml_content)?;
for vector in vectors.test_vectors {
    // Run test
}
```

### C (Avatar)
```c
at_yaml_doc_t doc;
at_yaml_parse_file(&doc, "vectors/crypto/sha256.yaml");
at_yaml_array_t *vectors = at_yaml_get_array(&doc, "test_vectors");
```

### Python
```python
import yaml
with open("vectors/crypto/sha256.yaml") as f:
    vectors = yaml.safe_load(f)
for vector in vectors["test_vectors"]:
    # Run test
```

## Adding New Vectors

1. Create generator in `tck/generators/<category>/`
2. Generate YAML to `tck/vectors/<category>/`
3. Update this README if adding new category
4. Ensure Avatar C consumer is updated

## Related Documents

- `MULTI_CLIENT_ALIGNMENT.md` - Methodology overview
- `MULTI_CLIENT_ALIGNMENT_SCHEME.md` - Technical specifications
- `tck/specs/` - Critical path specifications
