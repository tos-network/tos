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

### 2. Blockchain Coding Safety

**RULE: Consensus-critical code MUST use deterministic integer arithmetic. Floating-point types (f32/f64) are PROHIBITED in consensus layer.**

#### Why f32/f64 are Dangerous in Consensus Code

Floating-point arithmetic produces **platform-dependent results**:
- Different CPU architectures (x86 vs ARM) may produce different results
- Different compiler optimizations may change calculation order
- Different rounding modes in FPU can cause inconsistency
- This leads to **network splits** where nodes disagree on valid blocks

**Examples of consensus-critical code**:
- ❌ Block validation
- ❌ Transaction fee calculation
- ❌ Reward distribution
- ❌ Difficulty adjustment
- ❌ Any computation stored in blockchain state

**Examples of safe f64 usage** (non-consensus):
- ✅ UI display formatting (`format_coin()`, `format_hashrate()`)
- ✅ RPC response fields (for human readability)
- ✅ Prometheus metrics (monitoring only)
- ✅ Client-side fee estimation (network validates sufficiency, not calculation method)
- ✅ Offline configuration tools (not used at runtime)

#### The u128 Scaled Integer Pattern

**Use `u128` with a SCALE factor to represent decimal values deterministically.**

```rust
const SCALE: u128 = 10000;  // Represents 1.0

// Representing decimal values
let multiplier_1_2 = 12000u128;   // 1.2 * SCALE
let multiplier_0_85 = 8500u128;   // 0.85 * SCALE
let multiplier_1_5 = 15000u128;   // 1.5 * SCALE
```

#### Step-by-Step Calculation Pattern

**CRITICAL**: Divide after EACH multiplication to prevent overflow.

✅ **CORRECT**: Divide after each step
```rust
const SCALE: u128 = 10000;

pub fn calculate_reward(
    base_reward: u64,
    quality_multiplier: u128,  // e.g., 8500 for 0.85
    scarcity_multiplier: u128, // e.g., 12000 for 1.2
    loyalty_multiplier: u128,  // e.g., 11000 for 1.1
) -> u64 {
    // Step 1: base × quality / SCALE
    let temp1 = (base_reward as u128 * quality_multiplier) / SCALE;

    // Step 2: temp1 × scarcity / SCALE
    let temp2 = (temp1 * scarcity_multiplier) / SCALE;

    // Step 3: temp2 × loyalty / SCALE
    let result = (temp2 * loyalty_multiplier) / SCALE;

    result as u64
}
```

❌ **INCORRECT**: Using f64 (non-deterministic)
```rust
pub fn calculate_reward(
    base_reward: u64,
    quality: f64,    // 0.85
    scarcity: f64,   // 1.2
    loyalty: f64,    // 1.1
) -> u64 {
    // Different platforms may produce different results!
    let result = base_reward as f64 * quality * scarcity * loyalty;
    result as u64  // ❌ CONSENSUS RISK
}
```

❌ **INCORRECT**: Multiplying without dividing (overflow risk)
```rust
// This can overflow!
let result = base * multiplier1 * multiplier2 * multiplier3;  // ❌ OVERFLOW RISK
```

#### Real-World Examples from TOS

**Example 1: AI Mining Reward Calculation** ✅
```rust
// File: common/src/ai_mining/reputation.rs

pub fn calculate_final_reward(
    base_reward: u64,
    validation_score: u8,
    reputation: &AccountReputation,
) -> u64 {
    const SCALE: u128 = 10000;

    // Quality: validation_score (0-100) → scaled (0-10000)
    let quality_scaled = (validation_score as u128 * SCALE) / 100;

    // Scarcity bonus based on quality threshold
    let scarcity_scaled = if validation_score >= 90 {
        15000  // 1.5 * SCALE
    } else if validation_score >= 80 {
        12000  // 1.2 * SCALE
    } else {
        10000  // 1.0 * SCALE
    };

    // Loyalty bonus for high reputation
    let loyalty_scaled = if reputation.reputation_score >= 9000 {
        11000  // 1.1 * SCALE
    } else {
        10000  // 1.0 * SCALE
    };

    // Calculate: base × quality × scarcity × loyalty
    let temp1 = (base_reward as u128 * quality_scaled) / SCALE;
    let temp2 = (temp1 * scarcity_scaled) / SCALE;
    let final_reward = (temp2 * loyalty_scaled) / SCALE;

    final_reward as u64
}
```

**Example 2: Difficulty Adjustment Algorithm** ✅
```rust
// File: daemon/src/core/ghostdag/daa.rs

fn apply_difficulty_adjustment(
    current_difficulty: &Difficulty,
    expected_time: u64,
    actual_time: u64,
) -> Result<Difficulty, BlockchainError> {
    use tos_common::varuint::VarUint;

    // Use U256 integer arithmetic (VarUint wraps U256)
    let current = *current_difficulty;
    let expected = VarUint::from(expected_time);
    let actual = VarUint::from(actual_time);

    // new_difficulty = (current × expected) / actual
    let new_difficulty = (current * expected) / actual;

    // Clamp to 4x range using integer operations
    let max_difficulty = current * 4u64;
    let min_difficulty = current / 4u64;

    let clamped = if new_difficulty > max_difficulty {
        max_difficulty
    } else if new_difficulty < min_difficulty {
        min_difficulty
    } else {
        new_difficulty
    };

    Ok(clamped)
}
```

#### Overflow Safety Analysis

```rust
// TOS maximum supply: ~18M TOS = 1.8 × 10^16 nanoTOS
const MAX_TOS_SUPPLY: u64 = 18_000_000_000_000_000;
const SCALE: u128 = 10000;

// Worst case: 3 multiplications with SCALE=10000
// max_value = 1.8 × 10^16 × 10^4 × 10^4 × 10^4 = 1.8 × 10^28
// u128::MAX = 3.4 × 10^38
// Safety margin = 10^10x ✅ SAFE

// Always divide after each multiplication:
let step1 = (value * factor1) / SCALE;  // ← Division prevents accumulation
let step2 = (step1 * factor2) / SCALE;  // ← Division prevents accumulation
let step3 = (step2 * factor3) / SCALE;  // ← Division prevents accumulation
```

#### Performance Comparison

```
Benchmark: 100,000 reward calculations (Apple M1)

f64 (baseline):     100µs  ❌ Non-deterministic
u128 scaled:        545µs  ✅ Deterministic (5.5x slower, acceptable)
U256:              1500µs  ✅ Deterministic (15x slower, use only when needed)

Conclusion: u128 scaled is the sweet spot for most consensus calculations
```

#### When to Use U256 Instead of u128

Use `U256` (from `primitive-types` crate) when:

1. **Working with existing U256 types** (e.g., `Difficulty`, `VarUint`)
2. **Very large numbers** (> 10^30)
3. **Need extra safety margin** for future extensions

```rust
use primitive_types::U256;
use tos_common::varuint::VarUint;

// Difficulty is already U256-based, use U256 arithmetic
fn adjust_difficulty(
    current: &Difficulty,  // VarUint wraps U256
    ratio: u64,
) -> Difficulty {
    let current_u256 = current.as_ref().clone();
    let new_difficulty = current_u256 * U256::from(ratio);
    VarUint::from(new_difficulty)
}
```

#### Documentation Requirements

All safe f64 usages MUST be documented with safety comments:

```rust
// ✅ CORRECT: Documented safe usage
/// Calculate energy usage percentage
/// SAFE: f64 for display/UI purposes only, not consensus-critical
pub fn usage_percentage(&self) -> f64 {
    (self.used_energy as f64 / self.total_energy as f64) * 100.0
}

// ✅ CORRECT: RPC response field
/// SAFE: f64 for RPC display only, not consensus-critical
pub bps: f64,

// ✅ CORRECT: Client-side estimation
// SAFE: Client-side fee estimation, network only validates sufficiency
FeeBuilder::Multiplier(multiplier) => (expected_fee as f64 * multiplier) as u64,
```

#### Verification Checklist for Consensus Code

Before merging consensus-critical changes:

- [ ] No f32/f64 types in consensus calculations
- [ ] All decimal arithmetic uses `u128` with `SCALE=10000`
- [ ] Division after EACH multiplication (overflow prevention)
- [ ] Overflow safety analysis documented
- [ ] Tests verify deterministic results across platforms
- [ ] All safe f64 usages have `// SAFE:` comments

### 3. Documentation

**RULE: Keep documentation synchronized with code.**

- Update TIPs documents when changing consensus logic
- Update inline comments when refactoring
- Add references to TIPs in code comments where relevant

### 4. Backward Compatibility

**RULE: Maintain backward compatibility unless explicitly breaking.**

- Keep legacy methods marked with `#[allow(dead_code)]` and commented as "Legacy"
- Don't remove public APIs without deprecation cycle
- Maintain P2P protocol compatibility

## Verification Checklist

Before committing, verify:

- [ ] No Chinese, Japanese, or other non-English text in code/docs
- [ ] All log statements with format arguments are wrapped with `if log::log_enabled!`
- [ ] **No f32/f64 in consensus-critical code** (block validation, fees, rewards, difficulty)
- [ ] All consensus calculations use `u128` with `SCALE=10000` pattern
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
# Example output: e8d3d0b Add test contracts to examples/
```

**Step 2: Update all TAKO references in TOS workspace**

Update the `rev` value in these three files:

**File: daemon/Cargo.toml**
```toml
# TOS Kernel - TOS Kernel(TAKO) runtime
tos-kernel = { package = "tos-kernel", git = "https://github.com/tos-network/tako", rev = "e8d3d0b" }
tos-program-runtime = { package = "tos-program-runtime", git = "https://github.com/tos-network/tako", rev = "e8d3d0b" }
tos-syscalls = { package = "tos-syscalls", git = "https://github.com/tos-network/tako", rev = "e8d3d0b" }
tako-sdk = { package = "tako-sdk", git = "https://github.com/tos-network/tako", rev = "e8d3d0b" }
tos-environment = { package = "tos-environment", git = "https://github.com/tos-network/tako", rev = "e8d3d0b" }
```

**File: common/Cargo.toml**
```toml
# TOS Kernel - Simplified re-export crate in TAKO
tos-kernel = { git = "https://github.com/tos-network/tako", rev = "e8d3d0b" }
```

**File: testing-framework/Cargo.toml**
```toml
# TAKO dependencies for contract testing
tos-kernel = { package = "tos-kernel", git = "https://github.com/tos-network/tako", rev = "e8d3d0b" }
tos-syscalls = { package = "tos-syscalls", git = "https://github.com/tos-network/tako", rev = "e8d3d0b" }
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
   | expected struct `tos_kernel::types::Address` (tos-kernel v0.1.0 @ rev e8d3d0b)
   |    found struct `tos_kernel::types::Address` (tos-kernel v0.1.0 @ rev 5bcc0ad)
```

**Root cause**: Different workspace packages are using different TAKO versions, causing type incompatibility.

**✅ SOLUTION**: Always update ALL three Cargo.toml files together with the same `rev` value.

#### Version History Tracking

When updating TAKO dependencies, document the version in commit messages:

```bash
# Good commit message
chore(deps): Update tako dependency to e8d3d0b (test contracts)

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
| **Verify consistency** | `./verify_tako_versions.sh` |

## Local CI Check (Pre-Push Verification)

Before pushing code to GitHub, run CI checks locally to catch issues early. This saves time by avoiding failed GitHub Actions runs.

### Quick Start

```bash
# Quick check (fmt + clippy + build) - ~1 minute
./scripts/ci-check.sh quick

# Standard check (+ unit tests + doc tests) - ~5 minutes
./scripts/ci-check.sh

# Full check (+ integration tests + release build) - ~15 minutes
./scripts/ci-check.sh full
```

### Docker Mode (Matches GitHub Actions Environment)

Run checks in a Docker container that matches the GitHub Actions environment:

```bash
# Build Docker image (first time only)
docker build -t tos-ci-check docker/ci-check/

# Run checks in Docker
./scripts/ci-check.sh docker quick
./scripts/ci-check.sh docker
./scripts/ci-check.sh docker full
```

### CI Checks Overview

| Mode | Checks Performed | Time |
|------|------------------|------|
| `quick` | Format, Clippy (critical + security), Build with `-D warnings` | ~1 min |
| default | quick + Unit tests + Doc tests + Integration tests (debug) | ~8 min |
| `full` | default + Integration tests (release) + Parallel tests + Security tests + Release build | ~20 min |

### What Gets Checked

**Formatting (lint job)**
```bash
cargo fmt --all -- --check
```

**Clippy - Critical Lints (lint job)**
```bash
cargo clippy --workspace --all-targets -- \
    -D clippy::await_holding_lock \
    -D clippy::todo \
    -D clippy::unimplemented \
    -W clippy::all
```

**Security Clippy - Production Code (security-checks job)**
```bash
cargo clippy \
    --package tos_daemon \
    --package tos_common \
    --package tos_wallet \
    --lib -- \
    -D clippy::unwrap_used \
    -D clippy::expect_used \
    -D clippy::panic \
    -D clippy::disallowed_methods \
    -D warnings
```

**Build with Strict Warnings (build job)**
```bash
RUSTFLAGS="-D warnings" cargo build \
    --package tos_daemon \
    --package tos_common \
    --package tos_wallet \
    --lib
```

### File Structure

```
tos/
├── docker/
│   └── ci-check/
│       ├── Dockerfile        # Docker image (rust:1.91-bookworm)
│       └── ci-check.sh       # CI check script
└── scripts/
    └── ci-check.sh           # Entry point (local or Docker)
```

### Recommended Workflow

1. **Before committing**: Run `./scripts/ci-check.sh quick` to catch format/lint issues
2. **Before pushing**: Run `./scripts/ci-check.sh` to verify tests pass
3. **Before PR merge**: Run `./scripts/ci-check.sh full` for complete verification

### Troubleshooting

**If local check passes but GitHub Actions fails:**

1. Check Rust version matches (local vs Docker vs GitHub Actions)
   ```bash
   rustc --version  # Local
   # Docker uses rust:1.91-bookworm
   # GitHub Actions uses dtolnay/rust-toolchain@stable
   ```

2. Run in Docker to match GitHub Actions environment:
   ```bash
   ./scripts/ci-check.sh docker
   ```

3. Check for platform-specific issues (macOS vs Linux)