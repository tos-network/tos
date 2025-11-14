# Miri Memory Safety Validation

## What is Miri?

Miri is Rust's interpreter that detects undefined behavior at runtime. It executes Rust code in an interpreted environment with strict checks for memory safety violations.

**What Miri Detects:**
- Use-after-free
- Out-of-bounds memory access
- Data races in concurrent code
- Invalid pointer arithmetic
- Uninitialized memory reads
- Violations of Rust's aliasing rules

**Audit Reference:** Round 3 Re-review recommendation: "Use Miri and cargo-fuzz for memory boundary validation"

## TOS Miri Coverage

### ✅ Tested Components (Miri Compatible)

These components contain pure computation logic without I/O operations:

1. **VarUint Arithmetic** (`common/src/varuint.rs`)
   - Addition, subtraction, multiplication, division
   - Bit shifts and comparisons
   - Serialization/deserialization
   - **Why testable:** Pure U256 arithmetic, no I/O

2. **Serialization Primitives** (`common/src/serializer/`)
   - Basic type serialization (u8, u16, u32, u64, u128)
   - String encoding/decoding
   - **Why testable:** Memory operations on byte buffers, no file I/O

3. **API Data Structures** (`common/src/api/data.rs`)
   - DataElement recursion depth validation
   - Nested structure handling
   - **Why testable:** Pure memory operations, recursion checking

4. **Account Energy Calculations** (`common/src/account/energy.rs`)
   - Energy accumulation formulas
   - Overflow protection
   - **Why testable:** Pure arithmetic, no external state

5. **Immutable Data Structures** (`common/src/immutable.rs`)
   - Immutable type wrappers
   - Interior mutability patterns
   - **Why testable:** Pure memory semantics

### ❌ Not Testable (Miri Limitations)

Miri cannot execute code that uses:

1. **I/O Operations**
   - File system operations (storage layer)
   - Network operations (P2P layer)
   - Database access (RocksDB, Sled)
   - **Example:** `daemon/src/core/storage/`

2. **System Resources**
   - System time (`std::time::SystemTime`)
   - Thread spawning (some async runtime features)
   - **Example:** Mining code with timestamp generation

3. **Foreign Function Interface (FFI)**
   - External C libraries
   - Hardware acceleration (SIMD, AES-NI)
   - **Example:** BPF VM execution, hardware crypto

4. **Async Runtime**
   - Tokio runtime operations
   - Complex async/await patterns
   - **Example:** RPC server, network handlers

### ⚠️ Partial Coverage

Some components can be partially tested:

1. **Blockchain Validation Logic**
   - ✅ Can test: Pure validation logic (merkle tree construction, blue work calculation)
   - ❌ Cannot test: Storage operations, chain state queries

2. **Transaction Processing**
   - ✅ Can test: Parsing, signature verification math, fee calculation
   - ❌ Cannot test: VM execution, state updates, database writes

3. **Cryptography**
   - ✅ Can test: Pure Rust implementations
   - ❌ Cannot test: Hardware-accelerated crypto (AES-NI, SHA extensions)

## Running Miri Tests

### Quick Start

```bash
# Run all Miri-compatible tests
./miri-tests.sh
```

### Manual Execution

```bash
# Install Miri (one-time setup)
rustup +nightly component add miri

# Test specific module
cargo +nightly miri test --package tos_common --lib varuint::tests -- --test-threads=1

# Test with verbose output
MIRIFLAGS="-Zmiri-disable-isolation" cargo +nightly miri test --package tos_common --lib
```

### Test Output Interpretation

**Success:**
```
test varuint::tests::test_serde_0 ... ok
test varuint::tests::test_serde_max ... ok
```

**Undefined Behavior Detected:**
```
error: Undefined Behavior: dereferencing pointer failed: alloc1234 has been freed
  --> src/example.rs:42:5
```

**Unsupported Operation:**
```
error: unsupported operation: can't call foreign function: open
```

## CI Integration

### GitHub Actions Configuration

Add to `.github/workflows/ci.yml`:

```yaml
miri-checks:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4

    - name: Install Rust nightly
      uses: actions-rs/toolchain@v1
      with:
        toolchain: nightly
        components: miri
        override: true

    - name: Run Miri tests
      run: ./miri-tests.sh
      continue-on-error: true  # Don't block CI initially

    - name: Upload Miri results
      uses: actions/upload-artifact@v3
      if: always()
      with:
        name: miri-results
        path: miri-output.log
```

### Local Pre-Commit Hook

Add to `.git/hooks/pre-commit`:

```bash
#!/bin/bash
# Run Miri on changed files (optional)
if git diff --cached --name-only | grep -q "^common/src/varuint.rs"; then
    echo "Running Miri on VarUint..."
    cargo +nightly miri test --package tos_common --lib varuint::tests -- --test-threads=1
fi
```

## Miri Flags and Options

### Useful Flags

```bash
# Disable isolation (allow file system access for some tests)
MIRIFLAGS="-Zmiri-disable-isolation" cargo +nightly miri test

# Track raw pointer tags (more strict checking)
MIRIFLAGS="-Zmiri-track-raw-pointers" cargo +nightly miri test

# Increase stack size for deep recursion tests
MIRIFLAGS="-Zmiri-stack-size=1048576" cargo +nightly miri test

# Enable symbolic alignment checking
MIRIFLAGS="-Zmiri-symbolic-alignment-check" cargo +nightly miri test
```

### Performance Notes

Miri is **slow** (10-100x slower than native execution):
- **Reason:** Interprets every instruction with safety checks
- **Impact:** Only run on critical pure computation modules
- **Workaround:** Use targeted test selection

## Coverage Analysis

### Current Coverage Estimate

| Layer | Total Modules | Miri-Compatible | Coverage |
|-------|--------------|-----------------|----------|
| Common (tos_common) | 45 | 12 | 27% |
| Daemon (tos_daemon) | 120 | 5 | 4% |
| Wallet (tos_wallet) | 15 | 8 | 53% |
| **Total** | **180** | **25** | **14%** |

**Note:** Low overall coverage is expected - most blockchain code involves I/O.

### High-Value Targets

Priority modules for Miri testing:

1. **Arithmetic Operations** (overflow protection)
   - VarUint calculations
   - Balance operations
   - Fee calculations

2. **Recursion Handling** (stack overflow protection)
   - DataElement parsing
   - Merkle tree construction
   - GHOSTDAG traversal logic

3. **Memory-Sensitive Code** (bounds checking)
   - Serialization buffers
   - Array operations
   - Unsafe code blocks

## Known Limitations

### 1. Crypto Hardware Acceleration

**Problem:** Miri doesn't support CPU feature detection.

```rust
// This will fail in Miri if it uses AES-NI
let hash = blake3::hash(data);
```

**Workaround:** Use pure Rust implementations for tests.

### 2. Timing-Dependent Tests

**Problem:** Miri time advances artificially.

```rust
// This will not work correctly in Miri
let start = SystemTime::now();
expensive_operation();
let elapsed = start.elapsed().unwrap();
```

**Workaround:** Test logic separately from timing.

### 3. Concurrency Testing

**Problem:** Miri's thread scheduler is deterministic but limited.

```rust
// May not detect all race conditions
std::thread::spawn(|| { shared_state.update(); });
```

**Workaround:** Use Loom for concurrency testing (see PROPERTY_TESTS.md).

## Comparison with Other Tools

| Tool | Detects | Speed | Coverage | Automation |
|------|---------|-------|----------|------------|
| **Miri** | UB, memory safety | Slow (10-100x) | Limited (no I/O) | Easy |
| **ASan** | Memory errors | Medium (2-5x) | Full | Easy |
| **UBSan** | Undefined behavior | Fast (1.5x) | Full | Easy |
| **Valgrind** | Memory leaks | Slow (10-50x) | Full | Medium |
| **Fuzzing** | Edge cases | Varies | Full | Complex |

**Recommendation:** Use Miri for critical pure code + ASan/UBSan for full system testing.

## Troubleshooting

### Error: "unsupported operation"

**Cause:** Test uses I/O or FFI.

**Solution:** Refactor test to isolate pure logic, or skip Miri for this test.

```rust
#[test]
#[cfg_attr(miri, ignore)]  // Skip in Miri
fn test_with_io() {
    // File I/O test
}
```

### Error: "out of memory"

**Cause:** Test allocates too much memory.

**Solution:** Reduce test data size or increase stack size:

```bash
MIRIFLAGS="-Zmiri-stack-size=2097152" cargo +nightly miri test
```

### Error: "deadlock detected"

**Cause:** Test has actual deadlock or Miri detects potential deadlock.

**Solution:** Review lock ordering, use timeout mechanisms.

## Future Work

### Expand Coverage

1. Add Miri tests for new arithmetic modules
2. Create pure logic variants of I/O-heavy functions
3. Isolate testable components from blockchain state

### Automated Reporting

1. Generate Miri coverage report
2. Track UB detection over time
3. Alert on new Miri failures in CI

### Integration with Fuzzing

1. Use Miri with cargo-fuzz for deeper checking
2. Run fuzzer-generated inputs through Miri
3. Combine property tests with Miri validation

## References

- [Miri Documentation](https://github.com/rust-lang/miri)
- [TOS Audit Round 3 Re-review](../memo/16-AUDIT/)
- [TOS Property Testing Guide](PROPERTY_TESTS.md)
- [TOS Unsafe Code Audit](../memo/16-AUDIT/UNSAFE_CODE_AUDIT.md)

---

**Last Updated:** 2025-11-14
**Maintainer:** TOS Security Team
