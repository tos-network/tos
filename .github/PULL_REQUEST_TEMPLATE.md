## Problem

<!-- Describe the issue this PR addresses. What is broken, missing, or suboptimal? -->

## Summary of Changes

<!-- Briefly describe what this PR does and how it solves the problem. -->

## Testing

<!-- How was this change tested? Include commands, test cases, or manual verification steps. -->

- [ ] `cargo fmt --all -- --check` passes
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes
- [ ] `cargo test --workspace` passes
- [ ] New/modified code has appropriate test coverage

## Related Issues

<!-- Link related issues. Use "Fixes #123" to auto-close on merge. -->

Fixes #

## Checklist

- [ ] Code follows project coding standards (see CLAUDE.md)
- [ ] No `.unwrap()` or `.expect()` in production code paths
- [ ] No floating-point arithmetic in consensus-critical code
- [ ] Logs with format arguments wrapped in `log::log_enabled!` checks
- [ ] Documentation updated if public APIs changed

<!-- For bounty submissions, include: "Addresses B-XXX" -->
