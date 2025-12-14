# CI/CD Setup Guide

This document explains how to configure and use the GitHub Actions CI/CD workflows for the TOS Testing Framework.

## Table of Contents

- [Overview](#overview)
- [Workflows](#workflows)
- [Setting Up CI/CD](#setting-up-cicd)
- [Interpreting Results](#interpreting-results)
- [Downloading and Replaying Artifacts](#downloading-and-replaying-artifacts)
- [Advanced Configuration](#advanced-configuration)
- [Troubleshooting](#troubleshooting)

## Overview

The TOS Testing Framework includes two GitHub Actions workflows:

1. **PR Tests** (`.github/workflows/pr-tests.yml`) - Runs on every pull request
2. **Nightly Chaos** (`.github/workflows/nightly-chaos.yml`) - Runs extended tests nightly

Both workflows are designed to catch regressions early and provide detailed failure artifacts for debugging.

## Workflows

### 1. Pull Request Tests (`pr-tests.yml`)

**Triggers:**
- Pull requests to `main` or `develop` branches
- Pushes to `main` or `develop` branches
- Only runs when relevant files change

**Jobs:**
- **Format Check**: Ensures code is formatted with `cargo fmt`
- **Clippy Lints**: Checks for warnings with `cargo clippy`
- **Build**: Compiles the workspace with and without features
- **Test Suite**: Runs base tests and chaos tests
- **Test Examples**: Verifies examples compile and run
- **Summary**: Aggregates results and reports status

**Duration:** ~5-10 minutes

**Example Output:**
```
✅ All PR checks passed!
- Code formatting: ✅
- Clippy lints: ✅
- Build: ✅
- Tests: ✅
- Examples: ✅
```

### 2. Nightly Chaos Testing (`nightly-chaos.yml`)

**Triggers:**
- Scheduled: Every night at 2 AM UTC
- Manual: Via "Run workflow" button in GitHub Actions

**Jobs:**
- **Chaos Tests**: Runs chaos tests with 10,000 proptest cases
- **Property Tests**: Extended property-based testing
- **Stress Tests**: Runs high-throughput tests 10 times
- **Report**: Generates summary and uploads artifacts

**Duration:** ~1-2 hours

**Artifact Collection:**
- Test output logs
- Failed test names
- RNG seeds for reproduction
- Proptest regression files

## Setting Up CI/CD

### Step 1: Enable GitHub Actions

1. Navigate to your repository on GitHub
2. Click **Settings** → **Actions** → **General**
3. Under "Actions permissions", select **Allow all actions and reusable workflows**
4. Click **Save**

### Step 2: Configure Branch Protection (Optional)

To require PR tests to pass before merging:

1. Go to **Settings** → **Branches**
2. Click **Add rule** for `main` or `develop`
3. Check **Require status checks to pass before merging**
4. Select the following checks:
   - `Code Formatting Check`
   - `Clippy Lints`
   - `Build`
   - `Test Suite (Base Tests)`
   - `Test Suite (Chaos Tests)`
   - `Test Examples`
5. Click **Create** or **Save changes**

### Step 3: Verify Workflows

1. Create a test PR or push to a protected branch
2. Go to **Actions** tab to see workflows running
3. Click on a workflow run to see detailed logs

### Step 4: Configure Notifications (Optional)

To receive notifications when nightly tests fail:

1. Go to **Settings** → **Notifications**
2. Enable **Actions** notifications
3. Or add custom notification logic to `nightly-chaos.yml` (see Advanced Configuration)

## Interpreting Results

### Pull Request Test Results

**Successful Run:**
```
✅ All checks passed successfully!
```

**Failed Run - Format Check:**
```
❌ Code Formatting Check failed
Error: some files are not formatted correctly
Run: cargo fmt --all
```

**Failed Run - Clippy:**
```
❌ Clippy Lints failed
warning: unused variable: `x`
  --> src/test.rs:42:9
```

**Failed Run - Tests:**
```
❌ Test Suite (Chaos Tests) failed
test test_partition_with_competing_chains ... FAILED

Failures:
    test_partition_with_competing_chains

test result: FAILED. 224 passed; 1 failed
```

### Nightly Chaos Test Results

**Successful Run:**
```
# Nightly Chaos Tests - Success ✅

All chaos tests passed with extended proptest cases!

Configuration:
- Proptest cases: 10000
- Test threads: 1
```

**Failed Run:**
```
# Chaos Test Failures

## Failed Tests
test tier4_chaos::property_tests::prop_supply_accounting_invariant ... FAILED

## RNG Seeds
TestRng seed: 0xa3f5c8e1b2d94706
   Replay: TOS_TEST_SEED=0xa3f5c8e1b2d94706 cargo test ...

📦 Artifacts uploaded for debugging. Download from Actions artifacts section.
```

## Downloading and Replaying Artifacts

When chaos tests fail, the workflow uploads artifacts containing:
- Full test output logs
- RNG seeds used in failing tests
- Proptest regression files
- Failure artifact JSON files (if captured)

### Step 1: Download Artifacts

1. Go to the **Actions** tab
2. Click on the failed workflow run
3. Scroll to **Artifacts** section at the bottom
4. Download `chaos-test-failures-<run-id>` or `property-test-failures-<run-id>`

### Step 2: Extract Artifacts

```bash
# Extract downloaded artifact
unzip chaos-test-failures-*.zip -d chaos-artifacts/

# View test output
cat chaos-artifacts/logs/chaos_test_output.log

# View failed tests
cat chaos-artifacts/logs/failed_tests.txt

# View RNG seeds
cat chaos-artifacts/logs/test_seeds.txt
```

### Step 3: Replay Failure Locally

```bash
# Find the RNG seed from test_seeds.txt
# Example: TestRng seed: 0xa3f5c8e1b2d94706

# Replay the exact test
TOS_TEST_SEED=0xa3f5c8e1b2d94706 cargo test test_name --features chaos

# Or replay with verbose output
TOS_TEST_SEED=0xa3f5c8e1b2d94706 cargo test test_name --features chaos -- --nocapture
```

### Step 4: Load Artifact JSON (if available)

If the test used `ArtifactCollector`, you can load and inspect the artifact:

```rust
use tos_testing_framework::utilities::replay::{load_artifact, print_artifact_summary};

#[tokio::test]
async fn inspect_artifact() -> Result<()> {
    let artifact = load_artifact("./chaos-artifacts/failures/test_example.json").await?;
    print_artifact_summary(&artifact);
    Ok(())
}
```

## Advanced Configuration

### Customizing Proptest Cases

**For PR Tests:**

Edit `.github/workflows/pr-tests.yml`:

```yaml
- name: Run tests - Chaos Tests
  run: cargo test --workspace --features chaos --verbose
  env:
    PROPTEST_CASES: 100  # Change from default
```

**For Nightly Tests:**

Trigger manually with custom input:

1. Go to **Actions** → **Nightly Chaos Testing**
2. Click **Run workflow**
3. Enter custom `proptest_cases` value (e.g., 50000)
4. Click **Run workflow**

### Adding Slack Notifications

Edit `.github/workflows/nightly-chaos.yml`:

```yaml
- name: Send notification (on failure)
  if: failure()
  run: |
    curl -X POST -H 'Content-type: application/json' \
      --data '{"text":"Nightly chaos tests failed! Check ${{ github.server_url }}/${{ github.repository }}/actions/runs/${{ github.run_id }}"}' \
      ${{ secrets.SLACK_WEBHOOK_URL }}
```

Add `SLACK_WEBHOOK_URL` to repository secrets.

### Caching Strategy

The workflows use cargo caching to speed up builds:

```yaml
- name: Cache cargo registry
  uses: actions/cache@v4
  with:
    path: |
      ~/.cargo/registry
      ~/.cargo/git
    key: ${{ runner.os }}-cargo-registry-${{ hashFiles('**/Cargo.lock') }}
```

**Cache is invalidated when:**
- `Cargo.lock` changes
- Cache key prefix changes

**To clear cache:**
1. Go to **Settings** → **Actions** → **Caches**
2. Delete specific cache entries

## Troubleshooting

### Problem: PR tests timeout

**Solution:**
- Check if tests hang (missing `#[tokio::test(start_paused = true)]`)
- Increase timeout in workflow (default: 6 hours)

```yaml
jobs:
  test:
    timeout-minutes: 120  # Increase from default
```

### Problem: Nightly tests always fail

**Solution:**
- Check if proptest cases are too high
- Review failed test patterns
- Add test to exclude list if flaky

```yaml
- name: Run chaos tests
  run: cargo test --workspace --features chaos --verbose -- --skip flaky_test_name
```

### Problem: Artifacts not uploaded

**Solution:**
- Ensure `continue-on-error: true` is set
- Check artifact path exists:

```yaml
- name: Debug artifact path
  run: ls -la artifacts/logs/
```

### Problem: Cache not working

**Solution:**
- Verify `Cargo.lock` is committed
- Check cache key matches
- Clear stale caches manually

### Problem: Format check fails with "No such file or directory"

**Solution:**
- Ensure `rustfmt` component is installed:

```yaml
- name: Install Rust toolchain
  uses: dtolnay/rust-toolchain@stable
  with:
    components: rustfmt
```

## Best Practices

### For Contributors

1. **Run tests locally before pushing:**
   ```bash
   cargo fmt --all
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   cargo test --workspace --features chaos
   ```

2. **Fix warnings immediately:**
   - Clippy warnings are treated as errors in CI
   - Format code with `cargo fmt --all`

3. **Test with chaos feature:**
   ```bash
   cargo test --features chaos
   ```

### For Maintainers

1. **Review nightly test failures weekly**
   - Check for patterns in failures
   - Update tests if infrastructure changed

2. **Keep workflows updated**
   - Update action versions when available
   - Adjust timeouts as test suite grows

3. **Archive old artifacts**
   - Artifacts retained for 30 days
   - Download important failures for investigation

4. **Monitor cache usage**
   - GitHub provides 10GB cache storage per repo
   - Clear old caches periodically

## References

- [GitHub Actions Documentation](https://docs.github.com/en/actions)
- [cargo test Documentation](https://doc.rust-lang.org/cargo/commands/cargo-test.html)
- [Proptest Configuration](https://altsysrq.github.io/proptest-book/proptest/tutorial.html)
- [TOS Testing Framework README](./README.md)
- [Artifact Collection System](./src/utilities/artifacts.rs)

## Support

For issues with CI/CD:
- Check workflow logs in GitHub Actions tab
- Review this documentation
- Open an issue with `[CI]` prefix

For test failures:
- Download artifacts
- Replay with RNG seed
- Check CHANGELOG.md for known issues

---

**Last Updated:** 2025-11-15
**Version:** 1.0
**Maintainer:** TOS Testing Framework Team
