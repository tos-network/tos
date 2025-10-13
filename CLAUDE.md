# Claude Code Rules for TOS Project

This document defines mandatory rules for all code contributions to the TOS blockchain project when using Claude Code.

## Code Quality Standards

### 1. Language Requirements

**RULE: All code comments, documentation, and text content must be in English only.**

- ❌ **PROHIBITED**: Chinese (中文), Japanese (日本語), Korean (한국어), or any non-English languages
- ❌ **PROHIBITED**: Unicode symbols that are not ASCII (arrows →, subscripts ₁₂, superscripts ²³, etc.)
- ✅ **REQUIRED**: ASCII characters only (a-z, A-Z, 0-9, and standard punctuation)
- ✅ **REQUIRED**: Use ASCII equivalents:
  - Use `->` instead of `→`
  - Use `P1, P2` instead of `P₁, P₂`
  - Use `n^2` instead of `n²`
  - Use `O(k*n)` instead of `O(k·n)`

#### Verification Command
```bash
# Check for non-ASCII characters in code files
perl -ne 'print "$ARGV:$.: $_" if /[^\x00-\x7F]/' **/*.rs **/*.md
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
- [ ] No Unicode symbols (arrows, subscripts, etc.) - ASCII only
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

- TIPs Directory: `/Users/tomisetsu/tos-network/TIPs/`
- Consensus Design: `TIPs/CONSENSUS_LAYERED_DESIGN.md`
- Refactoring Guide: `TIPs/CONSENSUS_REFACTORING_GUIDE.md`

---

**Last Updated**: 2025-10-13
**Version**: 1.1
**Maintainer**: TOS Development Team
