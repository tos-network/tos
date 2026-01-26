# Contributing to TOS

Thank you for your interest in contributing to TOS. This document outlines the guidelines and expectations for contributors.

## Overview

We welcome contributions that improve the protocol's correctness, security, performance, and documentation. Areas where contributions are particularly valuable:

- Bug fixes and correctness improvements
- Test coverage expansion
- Documentation and code comments
- CI/CD improvements
- Performance optimizations (with benchmarks)
- Security hardening

We prefer small, well-scoped pull requests. Broad refactors or architectural changes require prior discussion with the core team.

## Before You Start

1. **Read the documentation** - Review existing docs, README files, and inline code comments
2. **Search existing issues** - Your idea or bug may already be tracked
3. **Open an issue first** - For non-trivial changes, discuss your approach before writing code
4. **Understand the codebase** - Familiarize yourself with the relevant modules before proposing changes

## Communication Channels

- **GitHub Issues** - Primary channel for bug reports, feature discussions, and technical questions
- **GitHub Discussions** - For broader questions and community discussion

### Security Notice

- Only trust links from this repository and official project announcements
- Core team members will never ask for private keys, funds, or sensitive information
- Report impersonation attempts to the core team via GitHub issues
- **Do not create "official" channels** (Telegram groups, Discord servers, etc.) on behalf of the project without explicit authorization from the core team

## Contribution Workflow

1. **Fork the repository** and create a feature branch from `main`
2. **Keep commits atomic** - Each commit should represent a single logical change
3. **Write clear commit messages** - Use imperative mood, explain the "why"
4. **Open a pull request** - Reference any related issues
5. **Respond to review feedback** - Be prepared for multiple review rounds
6. **Maintain your PR** - Rebase on `main` if conflicts arise

### Review Expectations

- All PRs require at least one approval from a core maintainer
- CI must pass before merge
- Complex changes may require additional review time
- We may request changes, ask clarifying questions, or decline PRs that don't align with project direction

### Small PRs First

If you're new to the project, start with small, low-risk contributions:

- Fix typos or improve documentation
- Add or improve test cases
- Fix CI warnings or linting issues
- Improve log messages or error handling
- Add code comments to complex sections

This helps you learn the codebase and establishes trust before tackling larger changes.

## Coding Standards

### Rust Style

- Run `cargo fmt --all` before committing
- Run `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- All warnings must be resolved before merge

### Error Handling

- Use proper error types; avoid `.unwrap()` and `.expect()` in production code
- Propagate errors with `?` where appropriate
- Provide meaningful error messages

### Determinism Requirements

Consensus-critical code must be fully deterministic. The following are **prohibited** in consensus paths:

- `std::time::SystemTime` or wall-clock time
- Random number generators (except deterministic seeded PRNGs)
- Floating-point arithmetic (`f32`/`f64`) - use scaled integers
- Thread-dependent ordering or race conditions
- Hash maps with non-deterministic iteration order in consensus logic

Use `u64` timestamps from block headers, not system time. Use `u128` scaled integers instead of floats for precision-sensitive calculations.

### Code Organization

- Keep modules focused and cohesive
- Prefer composition over deep inheritance
- Document public APIs with rustdoc comments
- Add inline comments for non-obvious logic

## Testing & CI

### Running Tests Locally

```bash
# Format check
cargo fmt --all -- --check

# Linting
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Build
cargo build --workspace

# Tests
cargo test --workspace
```

### PR Requirements

- All existing tests must pass
- New functionality should include tests
- Security-sensitive changes require additional review and testing
- Performance changes should include benchmark results

## Logging & Performance

### Logging Guidelines

- Use appropriate log levels: `error` for failures, `warn` for anomalies, `info` for significant events, `debug`/`trace` for development
- Wrap logs with format arguments in `log::log_enabled!` checks:

```rust
if log::log_enabled!(log::Level::Debug) {
    debug!("Processing block {} at height {}", hash, height);
}
```

- Avoid `info!` or higher-level logs in hot paths (loops, per-transaction processing)
- Use structured fields where possible

### Performance Considerations

- Avoid allocations in hot paths
- Prefer iterators over collecting into intermediate vectors
- Use `&str` over `String` where ownership isn't needed
- Profile before optimizing; include benchmarks with performance PRs

## Security Policy

### Reporting Vulnerabilities

If you discover a security vulnerability, **do not** open a public issue. Instead:

1. Email the security contact with details (contact information available in repository settings or SECURITY.md if present)
2. Include steps to reproduce, potential impact, and any suggested fixes
3. Allow reasonable time for the team to respond and address the issue before public disclosure

We take security seriously and will acknowledge reports promptly.

### Responsible Disclosure

- Do not exploit vulnerabilities on live networks
- Do not disclose vulnerabilities publicly before a fix is available
- Work with the core team on coordinated disclosure timelines

## Licensing & Sign-off

- This project is licensed under the terms specified in the repository's LICENSE file
- No CLA is required currently
- By submitting a pull request, you agree to license your contribution under the same license as the project
- Ensure you have the right to submit any code you contribute

## Rewards & Bounties

Contributions may be eligible for token-based rewards at the sole discretion of the core team. Please note:

- Rewards are **not guaranteed** for any contribution
- This is **not employment** - contributors are independent participants
- Any rewards are subject to mainnet launch and applicable vesting conditions
- The core team reserves the right to determine reward eligibility and amounts
- Do not contribute with the expectation of compensation

If you have questions about potential rewards for a specific contribution, open an issue to discuss before starting work.

---

Thank you for contributing to TOS. Your efforts help build a more robust and secure protocol.
