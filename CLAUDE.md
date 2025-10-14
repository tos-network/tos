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
// GHOSTDAG: Select chain with maximum blue work
// Blue blocks: B ∩ past(v) where v is the tip
// Chain selection: max(blue_work) → best tip
```

✅ **CORRECT**: Mathematical notation
```rust
// Time complexity: O(n²) where n is the number of blocks
// Condition: height(v) ≥ height(u) ⇒ v is descendant of u
```

✅ **CORRECT**: Set theory notation
```rust
// anticone(B) = {v ∈ G | v ∉ past(B) ∧ B ∉ past(v)}
// mergeset_blues = past(tip) ∩ blue_set
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

### 4. Git Workflow

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

# 2. Build verification
cargo build --workspace

# 3. Test verification
cargo test --workspace

# 4. Stage changes
git add <files>

# 5. Commit with detailed message
git commit -m "<message>"

# 6. Push to GitHub
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
```

## Project-Specific Rules

### 1. Consensus Logic

**RULE: Use the correct metrics for each layer.**

- **Storage Layer**: Use `topoheight` for continuous indexing (0,1,2,3...)
- **Consensus Layer**: Use `blue_work` from GHOSTDAG for chain selection
- **Validation Layer**: Use `blue_score` for DAG depth measurement

#### Code Examples

✅ **CORRECT**: Chain selection using blue_work
```rust
pub async fn find_best_tip_by_blue_work<'a, G, I>(
    provider: &G,
    tips: I
) -> Result<&'a Hash, BlockchainError>
where
    G: GhostdagDataProvider,
    I: Iterator<Item = &'a Hash>
{
    tips.iter()
        .max_by_key(|hash| provider.get_ghostdag_blue_work(hash))
        .ok_or(BlockchainError::ExpectedTips)
}
```

❌ **INCORRECT**: Using cumulative_difficulty for consensus
```rust
// DO NOT USE THIS FOR CONSENSUS DECISIONS
tips.iter()
    .max_by_key(|hash| get_cumulative_difficulty(hash))
    .unwrap()
```

✅ **CORRECT**: Storage indexing using topoheight
```rust
async fn get_block_at_topoheight(topoheight: u64) -> Result<Block>
async fn get_balance_at_topoheight(addr: &Address, topoheight: u64) -> Result<u64>
```

### 2. Documentation

**RULE: Keep documentation synchronized with code.**

- Update TIPs documents when changing consensus logic
- Update inline comments when refactoring
- Add references to TIPs in code comments where relevant

### 3. Backward Compatibility

**RULE: Maintain backward compatibility unless explicitly breaking.**

- Keep legacy methods marked with `#[allow(dead_code)]` and commented as "Legacy"
- Don't remove public APIs without deprecation cycle
- Maintain P2P protocol compatibility

## Verification Checklist

Before committing, verify:

- [ ] No Chinese, Japanese, or other non-English text in code/docs
- [ ] All log statements with format arguments are wrapped with `if log::log_enabled!`
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

### Consensus Security

- [ ] Merkle root validation enforced for all blocks
- [ ] Blue score validation against parent tips
- [ ] Blue work calculation verified against GHOSTDAG algorithm
- [ ] Empty blocks must have zero merkle root
- [ ] Non-empty blocks must have matching merkle root

**Reference**: daemon/src/core/blockchain.rs:2155-2181 (merkle root validation)

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

## References

- TIPs Directory: `../TIPs/`
- Consensus Design: `../TIPs/CONSENSUS_LAYERED_DESIGN.md`
- Refactoring Guide: `../TIPs/CONSENSUS_REFACTORING_GUIDE.md`

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