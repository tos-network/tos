# Claude Code Rules for TOS Project

This document defines mandatory rules for all code contributions to the TOS blockchain project when using Claude Code.

## Code Quality Standards

### 1. Language Requirements

**RULE: All code comments, documentation, and text content must be in English only.**

- ❌ **PROHIBITED**: Chinese (中文), Japanese (日本語), Korean (한국어), or any non-English languages
- ✅ **REQUIRED**: Use English for all comments, documentation, and user-facing text
- ✅ **ALLOWED**: Unicode symbols for mathematical and technical notation in code comments

#### Allowed Unicode Symbols

The following Unicode symbols are **permitted** in code comments for clarity and precision:

**Mathematical symbols:**
- Arrows: →, ←, ↔, ⇒, ⇐, ⇔
- Comparison: ≈, ≠, ≤, ≥, <, >
- Set operations: ∩ (intersection), ∪ (union), ∈ (element of), ∉ (not element of), ⊂ (subset), ⊆ (subset or equal)
- Summation/Product: Σ (summation), ∏ (product)

**Special symbols:**
- Bullets: •, ◦, ▪, ▫
- Numbered circles: ①, ②, ③, ④, ⑤, ⑥, ⑦, ⑧, ⑨
- Dots: ·, ⋅ (middle dot)

**Technical symbols:**
- Math operators: ±, ×, ÷, √, ∞
- Logic: ∧ (and), ∨ (or), ¬ (not)

#### Examples

✅ **CORRECT**: Using Unicode for clarity
```rust
// BlockDAG: Select chain with maximum cumulative difficulty
// Tip selection: max(cumulative_difficulty) → best tip
// DAG ordering: topoheight determines block order
```

✅ **CORRECT**: Mathematical notation
```rust
// Time complexity: O(n²) where n is the number of blocks
// Condition: height(v) ≥ height(u) ⇒ v is descendant of u
```

✅ **CORRECT**: Set theory notation
```rust
// past(B) = {v ∈ G | v is an ancestor of B}
// tips(G) = {v ∈ G | v has no children}
```

#### Verification Command
```bash
# Check for non-English text in code files (manual review recommended)
# Focus on ensuring English-only comments and documentation
```

### 2. Compilation Requirements

**RULE: Code must compile without any warnings or errors.**

#### Build Verification
```bash
cargo build --workspace
# Expected output: 0 warnings, 0 errors
```

#### Standards
- ✅ **REQUIRED**: Zero compilation warnings
- ✅ **REQUIRED**: Zero compilation errors
- ✅ **REQUIRED**: Use `#[allow(dead_code)]` for intentionally unused legacy code
- ✅ **REQUIRED**: Use `#[allow(unused)]` in test modules if needed
- ❌ **PROHIBITED**: Pushing code with compilation warnings

### 3. Testing Requirements

**RULE: All tests must pass without warnings or errors before committing.**

#### Test Verification
```bash
cargo test --workspace
# Expected output: All tests passing, 0 warnings, 0 errors
```

#### Standards
- ✅ **REQUIRED**: All tests must pass (0 failures)
- ✅ **REQUIRED**: Zero test warnings
- ✅ **REQUIRED**: Fix or suppress all unused variable warnings in tests
- ❌ **PROHIBITED**: Ignoring or skipping existing tests without justification

### 4. Code Quality and Linting Requirements

**RULE: All code changes must pass clippy lints and formatting checks before committing.**

#### Clippy Verification
```bash
cargo clippy --workspace --all-targets --all-features -- -D warnings
# Expected output: 0 warnings, 0 errors
```

**IMPORTANT**: Clippy must run with `-D warnings` flag to treat all warnings as errors. This ensures the highest code quality standards.

#### Code Formatting
```bash
# Auto-format all code (run this before committing)
cargo fmt --all

# Verify formatting is correct (CI check)
cargo fmt --all -- --check
# Expected output: No formatting differences
```

#### Standards
- ✅ **REQUIRED**: Zero clippy warnings (all warnings treated as errors with `-D warnings`)
- ✅ **REQUIRED**: All code must be formatted with `cargo fmt --all`
- ✅ **REQUIRED**: Formatting verification must pass (`cargo fmt --all -- --check`)
- ✅ **REQUIRED**: Run these checks after every code change, before committing
- ❌ **PROHIBITED**: Committing code with clippy warnings or formatting issues

#### Common Clippy Issues to Fix
- `clippy::empty_line_after_doc_comments` - Remove empty lines after doc comments
- `clippy::redundant_field_names` - Use shorthand for identical field names (e.g., `validator` instead of `validator: validator`)
- `clippy::inconsistent_digit_grouping` - Use consistent underscores in numbers (e.g., `1_000_000` not `1000_000`)
- `clippy::manual_range_contains` - Use `.contains()` instead of manual range checks
- `clippy::needless_return` - Remove unnecessary `return` keywords

**Note**: Pre-existing clippy warnings in the codebase should be fixed gradually. New code must have zero warnings.

#### Security Clippy (Production Code)

**RULE: Production code (daemon, common, wallet) must pass strict security lints.**

```bash
cargo clippy --package tos_daemon --package tos_common --package tos_wallet --lib -- \
    -D clippy::unwrap_used -D clippy::expect_used -D clippy::panic -D warnings
```

**Prohibited in production code:**
- `clippy::unwrap_used` - Use `ok_or()`, `unwrap_or()`, `?` operator, or proper error handling instead of `.unwrap()`
- `clippy::expect_used` - Use pre-defined constants (e.g., `NONZERO` constants) or proper error handling instead of `.expect()`
- `clippy::panic` - Use `Result` types and `?` operator instead of `panic!()`

**Allowed exceptions:**
- Test code (`#[cfg(test)]` modules)
- Build scripts and tools
- Cases where panic is truly unrecoverable (document with `// SAFETY:` comment)

### 5. Git Workflow

**RULE: Follow the standard commit and push workflow.**

#### Commit Message Format
```
<type>: <subject>

<body>

<footer>

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>
```

#### Types
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `refactor`: Code refactoring
- `test`: Test additions or changes
- `chore`: Maintenance tasks

#### Workflow
```bash
# 1. Check for non-English content
perl -ne 'print "$ARGV:$.: $_" if /[^\x00-\x7F]/' **/*.rs **/*.md

# 2. Code formatting (auto-fix)
cargo fmt --all

# 3. Formatting verification
cargo fmt --all -- --check

# 4. Clippy linting (treat warnings as errors)
cargo clippy --workspace --all-targets --all-features -- -D warnings

# 5. Security Clippy (production code - REQUIRED)
cargo clippy --package tos_daemon --package tos_common --package tos_wallet --lib -- \
    -D clippy::unwrap_used -D clippy::expect_used -D clippy::panic -D warnings

# 6. Build verification
cargo build --workspace

# 7. Test verification
cargo test --workspace

# 8. Stage changes
git add <files>

# 9. Commit with detailed message
git commit -m "<message>"

# 10. Push to GitHub
git push origin <branch>
```

### 5. Logging Performance Requirements

**RULE: All log statements with format arguments must be wrapped with log level checks for zero-overhead logging.**

#### Optimization Pattern

Log macros (`error!`, `warn!`, `info!`, `debug!`, `trace!`) that contain format arguments (`{}` or `{:?}`) must be wrapped with `if log::log_enabled!` checks to prevent expensive string formatting when the log level is disabled.

✅ **CORRECT**: Zero-overhead logging
```rust
// Wrap logs with format arguments
if log::log_enabled!(log::Level::Debug) {
    debug!("Processing block {} at height {}", hash, height);
}

if log::log_enabled!(log::Level::Trace) {
    trace!("Peer {} sent {} bytes", peer_id, data.len());
}

if log::log_enabled!(log::Level::Error) {
    error!("Failed to verify transaction {}: {}", tx_hash, err);
}
```

❌ **INCORRECT**: Unoptimized logging (format arguments evaluated even when disabled)
```rust
// DO NOT write logs like this - wastes CPU on formatting
debug!("Processing block {} at height {}", hash, height);
trace!("Peer {} sent {} bytes", peer_id, data.len());
error!("Failed to verify transaction {}: {}", tx_hash, err);
```

#### When to Apply

- ✅ **REQUIRED**: All logs with format arguments (containing `{}` or `{:?}`)
- ✅ **REQUIRED**: Logs in hot paths (consensus, network I/O, storage operations)
- ✅ **REQUIRED**: Debug and trace logs (frequently disabled in production)
- ⚠️ **OPTIONAL**: Simple string logs without arguments (minimal overhead)

#### CRITICAL: Avoid High-Frequency Logs in Hot Paths

**RULE: NEVER use `info!` or higher level logs inside loops, iterators, or frequently-called functions.**

High-frequency logging can cause severe performance degradation, especially in production environments where INFO level is often enabled.

❌ **PROHIBITED**: Logging inside loops or hot paths
```rust
// DO NOT do this - will be called thousands of times per second
for item in database_iterator {
    info!("Processing item {}", item.id);  // ❌ SEVERE PERFORMANCE ISSUE
    process(item);
}

// DO NOT do this - called on every balance query
fn get_balance(&self) -> Balance {
    info!("Fetching balance");  // ❌ HIGH FREQUENCY
    self.balance
}
```

✅ **CORRECT**: Log only at boundaries or use debug/trace
```rust
// Log once before/after the loop
info!("Starting batch processing of {} items", items.len());
for item in database_iterator {
    // Use debug/trace for per-item logging (disabled in production)
    if log::log_enabled!(log::Level::Debug) {
        debug!("Processing item {}", item.id);  // ✅ OK, disabled by default
    }
    process(item);
}
info!("Completed batch processing");

// Or log only exceptional cases
fn get_balance(&self) -> Balance {
    if self.balance.is_corrupted() {
        warn!("Corrupted balance detected, recovering");  // ✅ OK, rare event
    }
    self.balance
}
```

**Hot Path Examples**:
- Database iteration loops (`for item in iterator`)
- Per-transaction processing
- Per-block validation
- Network packet handling
- Balance queries
- Any function called more than 10 times per second

**Safe Logging Levels in Hot Paths**:
- `error!`: Only for actual errors (rare)
- `warn!`: Only for exceptional conditions (rare)
- ~~`info!`~~: ❌ **NEVER** in hot paths
- `debug!`: ✅ OK (disabled in production)
- `trace!`: ✅ OK (disabled in production)

#### Examples by Log Level

```rust
// Error logs (critical errors)
if log::log_enabled!(log::Level::Error) {
    error!("Consensus failure at block {}: {}", block_hash, error);
}

// Warning logs (important warnings)
if log::log_enabled!(log::Level::Warn) {
    warn!("Peer {} exceeded rate limit: {} requests/sec", peer, rate);
}

// Info logs (notable events)
if log::log_enabled!(log::Level::Info) {
    info!("New block {} accepted at height {}", hash, height);
}

// Debug logs (development debugging)
if log::log_enabled!(log::Level::Debug) {
    debug!("Cache hit for key {} with value {:?}", key, value);
}

// Trace logs (verbose tracing)
if log::log_enabled!(log::Level::Trace) {
    trace!("Acquired lock {} at {}", lock_name, location);
}
```

#### Logs That Don't Need Optimization

Simple string logs without format arguments have minimal overhead and don't require wrapping:

```rust
// These are fine without wrapping (no format arguments)
info!("Daemon started");
debug!("Cache initialized");
error!("Connection failed");
```

#### Performance Impact

This optimization provides:
- **Zero overhead** when log level is disabled
- **No format argument evaluation** when not needed
- **No string allocation** when logging is filtered
- **Significant performance improvement** in hot paths (consensus, network, storage)

#### Verification

Check for unoptimized logs with format arguments:
```bash
# Find logs with format arguments that may need optimization
rg '^\s*(error|warn|info|debug|trace)!\(.*\{' --type rust

# Count optimized logs
rg 'if log::log_enabled!\(log::Level::' --type rust | wc -l

# CRITICAL: Check for info! logs inside loops (potential performance issues)
# Look for info! within 5 lines of 'for', 'while', or 'loop'
rg -A 5 '(for|while|loop)' --type rust | rg 'info!'

# Check for warn! logs inside loops (usually OK if rare, but review)
rg -A 5 '(for|while|loop)' --type rust | rg 'warn!'
```

## Project-Specific Rules

### 1. Deterministic Arithmetic

**RULE: Consensus-critical code MUST use deterministic integer arithmetic. Floating-point types (f32/f64) are PROHIBITED in consensus layer.**

#### Why f32/f64 are Dangerous in Consensus Code

- Different CPU architectures (x86 vs ARM) may produce different results
- Different compiler optimizations may change calculation order
- Different rounding modes in FPU can cause inconsistency
- This leads to **network splits** where nodes disagree on valid blocks

#### Prohibited in Consensus Code

- ❌ Block validation
- ❌ Transaction fee calculation
- ❌ Reward distribution
- ❌ Difficulty adjustment
- ❌ Any computation stored in blockchain state

#### Safe f64 Usage (Non-Consensus)

- ✅ UI display formatting (`format_coin()`, `format_hashrate()`)
- ✅ RPC response fields (for human readability)
- ✅ Prometheus metrics (monitoring only)
- ✅ Client-side fee estimation
- ✅ Test code for display/logging

#### The u128 Scaled Integer Pattern

Use `u128` with `SCALE = 10000` to represent decimal values deterministically:

- Multiply: `value * factor`
- **Divide after EACH multiplication** to prevent overflow: `(value * factor) / SCALE`
- For very large numbers, use `U256` from `primitive-types` crate

#### Documentation Requirements

All safe f64 usages MUST be documented with `// SAFE:` comments explaining why it's non-consensus-critical.

### 2. Documentation

**RULE: Keep documentation synchronized with code.**

- Update inline comments when refactoring
- Keep API documentation up-to-date

### 3. Backward Compatibility

**RULE: Maintain backward compatibility unless explicitly breaking.**

- Keep legacy methods marked with `#[allow(dead_code)]` and commented as "Legacy"
- Don't remove public APIs without deprecation cycle
- Maintain P2P protocol compatibility

## Verification Checklist

Before committing, verify:

- [ ] No Chinese, Japanese, or other non-English text in code/docs
- [ ] All log statements with format arguments are wrapped with `if log::log_enabled!`
- [ ] **No f32/f64 in consensus-critical code** (use u128 scaled integers instead)
- [ ] All safe f64 usages have `// SAFE:` documentation comments
- [ ] `cargo clippy --workspace -- -D warnings` produces 0 warnings
- [ ] **Security Clippy passes** for production code (no `unwrap()`, `expect()`, `panic!()` in daemon/common/wallet)
- [ ] `cargo build --workspace` produces 0 warnings
- [ ] `cargo test --workspace` produces 0 warnings and 0 failures
- [ ] All modified files staged with `git add`
- [ ] Commit message follows format with Co-Authored-By
- [ ] Changes pushed to correct branch

## Security Audit Checklist

Before releasing or deploying, verify:

### Input Validation

- [ ] All user input strings have length limits before processing
- [ ] All deserialization functions validate input size
- [ ] Hex string inputs are limited (e.g., max 128 chars for 32-byte values)
- [ ] No unbounded memory allocation from user input

**Example**: `extra_nonce` deserialization (common/src/block/header.rs:38-45)
```rust
// SECURITY FIX: Hard limit on input string length to prevent memory exhaustion DoS
const MAX_HEX_LENGTH: usize = 128;
if hex.len() > MAX_HEX_LENGTH {
    return Err(serde::de::Error::custom(
        format!("Invalid extraNonce hex string: length {} exceeds maximum {}", hex.len(), MAX_HEX_LENGTH)
    ));
}
```

### RPC Security

- [ ] RPC endpoints have authentication/authorization
- [ ] RPC is bound to localhost only in default config
- [ ] Documentation warns about RPC security requirements
- [ ] Rate limiting is implemented or documented
- [ ] TLS/SSL is required for remote RPC access

**Documentation**: See docs/API_REFERENCE.md "SECURITY WARNING: RPC Access Control" section

### Memory Safety

- [ ] No `.unwrap()` on user input
- [ ] Array bounds checked before `copy_from_slice()`
- [ ] Collection sizes limited to prevent DoS
- [ ] No unbounded loops on user-controlled data

### Audit Documentation

When security issues are found and fixed:

1. Document the vulnerability in code comments
2. Add test cases for the attack scenario
3. Update security checklist if new pattern found
4. Reference the fix in commit message

**Example Commit Message**:
```
fix: Add hard limit for extra_nonce input length to prevent DoS

SECURITY FIX: Prevent memory exhaustion DoS attack via extremely long
hex strings in extra_nonce deserialization. Added 128-character limit
before hex decoding in three locations:
- common/src/block/header.rs
- common/src/block/header_legacy.rs
- common/src/api/daemon/mod.rs
- daemon/src/core/mining/stratum.rs

Risk: Low (requires RPC access, mitigated by other limits)
Impact: Memory exhaustion, potential node crash

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>
```

## Automated Checks (Future)

These checks should be automated in CI/CD:

```bash
#!/bin/bash
# pre-commit hook

echo "Checking for non-English content..."
if perl -ne 'exit 1 if /[^\x00-\x7F]/' **/*.rs **/*.md; then
  echo "✓ No non-English content found"
else
  echo "✗ Non-English content detected!"
  exit 1
fi

echo "Running cargo build..."
if cargo build --workspace 2>&1 | grep -q "warning:"; then
  echo "✗ Build has warnings!"
  exit 1
else
  echo "✓ Build successful with no warnings"
fi

echo "Running cargo test..."
if cargo test --workspace 2>&1 | grep -q "warning:"; then
  echo "✗ Tests have warnings!"
  exit 1
else
  echo "✓ All tests pass with no warnings"
fi

echo "All checks passed! ✓"
```

## Exceptions

Exceptions to these rules require:
1. Explicit discussion and approval in GitHub issue/PR
2. Documentation of the exception reason
3. Temporary exception period defined

---

**Last Updated**: 2025-10-14
**Version**: 1.2
**Maintainer**: TOS Development Team

## Development Environment

### Development Wallet Addresses

**Development Wallet Address 1**:
```
tst12zacnuun3lkv5kxzn2jy8l28d0zft7rqhyxlz2v6h6u23xmruy7sqm0d38u
```

**Development Wallet Address 2**:
```
tst1yp0hc5z0csf2jk2ze9tjjxkjg8gawt2upltksyegffmudm29z38qqrkvqzk
```

### Running Development Chain

Stop the daemon, then run:

```bash
./target/debug/tos_daemon --network devnet --dir-path ~/tos_devnet/ --log-level info --auto-compress-logs
```

### Running Development Miner

```bash
./target/debug/tos_miner --miner-address tst12zacnuun3lkv5kxzn2jy8l28d0zft7rqhyxlz2v6h6u23xmruy7sqm0d38u --daemon-address 127.0.0.1:8080 --num-threads 1
```

## Dependency Management

### Updating TAKO Project Dependencies

**IMPORTANT**: When updating TAKO (TOS Kernel) dependencies, you MUST update the `rev` value consistently across ALL workspace packages to prevent type mismatches and compilation errors.

#### TAKO Dependencies Location

The TOS project references TAKO from three Cargo.toml files:

1. **daemon/Cargo.toml** - Core daemon dependencies (tos-kernel, tos-program-runtime, tos-syscalls, tako-sdk, tos-environment)
2. **common/Cargo.toml** - Common library dependencies (tos-kernel, tos-crypto)
3. **testing-framework/Cargo.toml** - Test framework dependencies (tos-kernel, tos-syscalls)

#### Update Workflow

When TAKO project is updated and pushed to GitHub, follow these steps to update TOS dependencies:

**Step 1: Get the latest TAKO commit hash**

```bash
cd ~/tos-network/tako
git log --oneline -1
# Example output: 336c254 Add test contracts to examples/
```

**Step 2: Update all TAKO references in TOS workspace**

Update the `rev` value in these three files:

**File: daemon/Cargo.toml**
```toml
# TOS Kernel - TOS Kernel(TAKO) runtime
tos-kernel = { package = "tos-kernel", git = "https://github.com/tos-network/tako", rev = "336c254" }
tos-program-runtime = { package = "tos-program-runtime", git = "https://github.com/tos-network/tako", rev = "336c254" }
tos-syscalls = { package = "tos-syscalls", git = "https://github.com/tos-network/tako", rev = "336c254" }
tako-sdk = { package = "tako-sdk", git = "https://github.com/tos-network/tako", rev = "336c254" }
tos-environment = { package = "tos-environment", git = "https://github.com/tos-network/tako", rev = "336c254" }
```

**File: common/Cargo.toml**
```toml
# TOS Kernel - Simplified re-export crate in TAKO
tos-kernel = { git = "https://github.com/tos-network/tako", rev = "336c254" }
```

**File: testing-framework/Cargo.toml**
```toml
# TAKO dependencies for contract testing
tos-kernel = { package = "tos-kernel", git = "https://github.com/tos-network/tako", rev = "336c254" }
tos-syscalls = { package = "tos-syscalls", git = "https://github.com/tos-network/tako", rev = "336c254" }
```

**Step 3: Verify compilation**

```bash
cd ~/tos-network/tos
cargo check -p tos_daemon
# Expected: Successful compilation with 0 warnings, 0 errors
```

**Step 4: Run tests**

```bash
cargo test --test tako_syscalls_comprehensive_test
# Expected: All tests passing
```

**Step 5: Commit and push**

```bash
git add Cargo.lock daemon/Cargo.toml common/Cargo.toml testing-framework/Cargo.toml
git commit -m "chore(deps): Update tako dependency to <commit-hash>

Updated tako references consistently across all workspace packages:
- daemon/Cargo.toml: <commit-hash>
- common/Cargo.toml: <commit-hash>
- testing-framework/Cargo.toml: <commit-hash>

<Brief description of changes in TAKO>

All integration tests passing.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"

git push
```

#### Common Pitfalls

**❌ MISTAKE: Inconsistent TAKO versions**

If you update only some files and leave others with old versions, you'll get hundreds of compilation errors like:

```
error[E0308]: mismatched types
  --> daemon/src/tako_integration/executor.rs:123:45
   |
   | expected struct `tos_kernel::types::Address` (tos-kernel v0.1.0 @ rev 336c254)
   |    found struct `tos_kernel::types::Address` (tos-kernel v0.1.0 @ rev 5bcc0ad)
```

**Root cause**: Different workspace packages are using different TAKO versions, causing type incompatibility.

**✅ SOLUTION**: Always update ALL three Cargo.toml files together with the same `rev` value.

#### Version History Tracking

When updating TAKO dependencies, document the version in commit messages:

```bash
# Good commit message
chore(deps): Update tako dependency to 336c254 (test contracts)

Updated tako references consistently across all workspace packages.

This update includes 6 test contracts added to tako examples/:
- simple-v3-test: Basic contract structure validation
- test-balance-transfer: Balance & transfer syscalls (6 tests)
- test-code-ops: Code inspection syscalls (7 tests)
- test-environment: Environment data syscalls (5 tests)
- test-events: Event emission LOG0-LOG4 (5 tests)
- test-transient-storage: EIP-1153 storage (5 tests)

No API changes from previous rev 95b9892, only test contracts added.
All 11 tako syscall integration tests passing (100%).
```

#### Verification Script

Use this script to verify all TAKO references are consistent:

```bash
#!/bin/bash
# verify_tako_versions.sh

echo "Checking TAKO dependency versions..."

# Extract all tako rev values
daemon_revs=$(grep 'git = "https://github.com/tos-network/tako"' daemon/Cargo.toml | grep -o 'rev = "[^"]*"' | sort -u)
common_revs=$(grep 'git = "https://github.com/tos-network/tako"' common/Cargo.toml | grep -o 'rev = "[^"]*"' | sort -u)
testing_revs=$(grep 'git = "https://github.com/tos-network/tako"' testing-framework/Cargo.toml | grep -o 'rev = "[^"]*"' | sort -u)

echo "daemon/Cargo.toml TAKO versions:"
echo "$daemon_revs"
echo ""
echo "common/Cargo.toml TAKO versions:"
echo "$common_revs"
echo ""
echo "testing-framework/Cargo.toml TAKO versions:"
echo "$testing_revs"
echo ""

# Check if all are the same
all_revs=$(echo -e "$daemon_revs\n$common_revs\n$testing_revs" | sort -u)
count=$(echo "$all_revs" | wc -l | tr -d ' ')

if [ "$count" -eq 1 ]; then
    echo "✅ All TAKO dependencies are consistent: $all_revs"
    exit 0
else
    echo "❌ TAKO dependencies are INCONSISTENT!"
    echo "Different versions found:"
    echo "$all_revs"
    exit 1
fi
```

#### Quick Reference

| Task | Command |
|------|---------|
| **Check current TAKO version** | `grep 'tako.*rev' daemon/Cargo.toml` |
| **Get latest TAKO commit** | `cd ~/tos-network/tako && git log -1 --format="%h %s"` |
| **Update all TAKO refs** | Edit 3 Cargo.toml files with same `rev` value |
| **Verify compilation** | `cargo check -p tos_daemon` |
| **Run integration tests** | `cargo test --test tako_syscalls_comprehensive_test` |
| **Verify consistency** | See verification script below |

## Local CI Check (Pre-Push Verification)

Before pushing code to GitHub, run CI checks locally to catch issues early.

### Manual Verification Commands

```bash
# 1. Code formatting (auto-fix)
cargo fmt --all

# 2. Formatting verification
cargo fmt --all -- --check

# 3. Clippy linting (treat warnings as errors)
cargo clippy --workspace --all-targets --all-features -- -D warnings

# 4. Security Clippy (production code)
cargo clippy --package tos_daemon --package tos_common --package tos_wallet --lib -- \
    -D clippy::unwrap_used -D clippy::expect_used -D clippy::panic -D warnings

# 5. Build verification
cargo build --workspace

# 6. Test verification
cargo test --workspace
```

### TAKO Version Consistency Check

```bash
# Check all TAKO references are consistent
grep 'git = "https://github.com/tos-network/tako"' daemon/Cargo.toml common/Cargo.toml testing-framework/Cargo.toml | grep -o 'rev = "[^"]*"' | sort -u

# Expected output: single unique rev value
```