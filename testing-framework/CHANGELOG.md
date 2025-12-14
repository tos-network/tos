# TOS Testing Framework - Changelog

## v3.0.5 - Infrastructure and Tooling Complete (2025-11-15)

### Major Features

#### 🔧 Failure Artifact Collection System

Implemented comprehensive artifact collection for test debugging and reproduction:

**ArtifactCollector Module** (`utilities/artifacts.rs`):
- `ArtifactCollector` struct with methods for capturing test failure state
- `capture_logs()` - Capture test output with timestamps and log levels
- `save_topology()` - Save network graph state (node count, connections, partitions)
- `add_blockchain_state()` - Save blockchain state snapshots (balances, nonces, heights, supply)
- `add_transaction()` - Record transaction history
- `save()` - Serialize all artifacts to JSON with human-readable format
- `load()` - Deserialize artifacts from JSON files

**Artifact Replay Module** (`utilities/replay.rs`):
- `load_artifact()` - Load artifact from disk
- `print_artifact_summary()` - Display human-readable artifact summary with box drawing
- `get_replay_command()` - Extract shell command for test replay with seed
- `validate_artifact()` - Verify artifact integrity and consistency

**Artifact Data Structures**:
- `TestArtifact` - Complete test failure snapshot
- `TestMetadata` - Test name, RNG seed, timestamp, duration, failure reason
- `TopologySnapshot` - Network topology and partitions
- `BlockchainStateSnapshot` - Node state (height, hash, balances, nonces, supply)
- `TransactionRecord` - Transaction history with confirmation status
- `LogEntry` - Timestamped log messages

**Features**:
- Human-readable JSON format (< 10MB per artifact)
- Automatic timestamp generation
- RNG seed capture for exact reproduction
- Supply accounting validation
- Full test context preservation

#### 🤖 CI/CD Workflow Configuration

Implemented GitHub Actions workflows for automated testing:

**Pull Request Tests** (`.github/workflows/pr-tests.yml`):
- **Format Check**: `cargo fmt --all -- --check`
- **Clippy Lints**: `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- **Build**: Compile workspace with and without features
- **Test Suite**: Run base tests and chaos tests in parallel
- **Test Examples**: Verify examples compile and run
- **Summary**: Aggregate results and report status
- **Caching**: Cargo registry and build artifacts
- **Duration**: ~5-10 minutes

**Nightly Chaos Testing** (`.github/workflows/nightly-chaos.yml`):
- **Chaos Tests**: Extended tests with 10,000 proptest cases
- **Property Tests**: High-iteration property-based testing
- **Stress Tests**: Run high-throughput tests 10 times
- **Artifact Upload**: Automatic failure artifact collection and upload
- **Scheduled**: Runs every night at 2 AM UTC
- **Manual Trigger**: Support for custom proptest case counts
- **Duration**: ~1-2 hours
- **Retention**: 30-day artifact retention

**CI Features**:
- Parallel job execution for faster feedback
- Automatic failure artifact collection
- RNG seed extraction from logs
- GitHub Actions summary reports
- Support for custom proptest configurations

#### 📚 CI/CD Documentation

Created comprehensive CI_SETUP.md (350+ lines):
- Overview of workflows and jobs
- Step-by-step setup instructions
- Branch protection configuration
- Result interpretation guide
- Artifact download and replay tutorial
- Advanced configuration examples
- Troubleshooting guide
- Best practices for contributors and maintainers

### Code Quality

#### 📦 New Dependencies

Added to `Cargo.toml`:
- `chrono = { version = "0.4", features = ["serde"] }` - Timestamp generation
- `textwrap = "0.16"` - Text wrapping for artifact summaries

#### 🧪 Test Coverage

**New Test Modules**:
- `utilities/artifacts.rs`: 11 tests covering all artifact collection features
- `utilities/replay.rs`: 7 tests for artifact loading, validation, and display

**Test Statistics**:
- **Artifact Tests**: 18 new tests
- **Coverage**: All artifact collection and replay functionality
- **Performance**: < 10ms per artifact test

### Documentation

#### 📖 README.md Updates

Added section "Collecting Failure Artifacts":
- Example of using `ArtifactCollector` in tests
- Example of loading and replaying from artifacts
- Reference to CI_SETUP.md for CI/CD details

### Usage Examples

#### Collecting Artifacts in Tests

```rust
use tos_testing_framework::utilities::ArtifactCollector;

#[tokio::test]
async fn test_with_artifacts() -> Result<()> {
    let mut collector = ArtifactCollector::new("test_consensus_failure");
    let rng = TestRng::new_from_env_or_random();
    collector.set_rng_seed(rng.seed());

    if let Err(e) = run_test().await {
        collector.add_blockchain_state(get_state().await?);
        collector.set_failure_reason(format!("{:?}", e));
        let path = collector.save("./artifacts/").await?;
        eprintln!("Artifact saved to: {}", path.display());
        return Err(e);
    }
    Ok(())
}
```

#### Replaying from Artifacts

```rust
use tos_testing_framework::utilities::{load_artifact, print_artifact_summary};

#[tokio::test]
async fn inspect_failure() -> Result<()> {
    let artifact = load_artifact("./artifacts/test_failure.json").await?;
    print_artifact_summary(&artifact);

    // Replay with same seed
    let rng = TestRng::with_seed(artifact.metadata.rng_seed.unwrap());
    // ... run test with same seed ...
    Ok(())
}
```

#### Artifact Summary Output

```
╔════════════════════════════════════════════════════════════════╗
║              TEST FAILURE ARTIFACT SUMMARY                     ║
╠════════════════════════════════════════════════════════════════╣
║ Test Name:     test_consensus_failure                          ║
║ Timestamp:     2025-11-15T12:34:56Z                            ║
║ Duration:      5432 ms                                         ║
║ RNG Seed:      0xa3f5c8e1b2d94706                              ║
╠════════════════════════════════════════════════════════════════╣
║ FAILURE REASON:                                                ║
║ Height mismatch: expected 5, got 3                             ║
╠════════════════════════════════════════════════════════════════╣
║ Network:       5 nodes                                         ║
║ Node States:   5 captured                                      ║
║ Transactions:  42 recorded                                     ║
║ Log Entries:   128 captured                                    ║
╠════════════════════════════════════════════════════════════════╣
║ REPLAY COMMAND:                                                ║
║ TOS_TEST_SEED=0xa3f5c8e1b2d94706 cargo test test_consensus...  ║
╚════════════════════════════════════════════════════════════════╝
```

### Files Added

**Artifact System**:
- `src/utilities/artifacts.rs` (500+ lines) - Artifact collection implementation
- `src/utilities/replay.rs` (400+ lines) - Artifact replay utilities

**CI/CD Workflows**:
- `.github/workflows/pr-tests.yml` (200+ lines) - PR testing workflow
- `.github/workflows/nightly-chaos.yml` (250+ lines) - Nightly chaos testing

**Documentation**:
- `CI_SETUP.md` (350+ lines) - CI/CD setup and usage guide

### Files Modified

- `src/utilities/mod.rs` - Added artifact and replay module exports
- `Cargo.toml` - Added chrono and textwrap dependencies
- `README.md` - Added artifact collection section
- `CHANGELOG.md` - This entry

### Impact

**For Developers**:
- Easy debugging with artifact collection
- Exact test reproduction with RNG seeds
- Human-readable failure summaries

**For CI/CD**:
- Automated testing on every PR
- Nightly chaos testing with extended cases
- Automatic artifact upload on failures

**For Maintainers**:
- Clear visibility into test failures
- 30-day artifact retention for investigation
- Comprehensive failure context

### Breaking Changes

None - all changes are additive.

### Migration Guide

No migration needed. Artifact collection is opt-in:

```rust
// Optional: Add to failing tests for better debugging
use tos_testing_framework::utilities::ArtifactCollector;
let mut collector = ArtifactCollector::new("my_test");
// ... capture state on failure ...
```

### Future Enhancements

Planned for future releases:
- Automatic artifact collection on test panic
- Integration with Toxiproxy for network fault injection (low priority)
- Kurtosis integration for distributed testing (low priority)
- Benchmark tracking in CI

---

## v3.0.4 - Documentation Quality Improvements (2025-11-15)

### Code Quality

#### 📝 Zero-Warning Build Achievement

Fixed all missing documentation warnings in scenarios/parser.rs:

**Documentation Additions**:
- Added field-level documentation for all `Step` enum variants (Transfer, MineBlock, AssertBalance, AssertNonce, AdvanceTime)
- Added field-level documentation for `BalanceExpect` enum variants (Eq, Within, Compare)
- Added field-level documentation for `CompareOp` enum variants (Gte, Lte, Gt, Lt)

**Impact**:
- **Library Build**: Zero warnings (previously had 18 missing documentation warnings)
- **Test Build**: Zero warnings for 214 base tests
- **Chaos Tests**: Zero warnings for 225 tests (with `--features chaos`)

**Compliance**: Now fully complies with TOS project `#![warn(missing_docs)]` lint level.

---

## v3.0.3 - Tier 4 Chaos Testing Framework (2025-11-15)

### Major Features

#### 🔥 Tier 4: Chaos Engineering & Property-Based Testing

Implemented comprehensive chaos testing framework with proptest integration:

**Property-Based Tests** (6 tests using proptest):
- `prop_transaction_order_independence` - Transaction ordering doesn't affect final balance
- `prop_empty_blocks_preserve_balances` - Empty blocks don't change balances
- `prop_supply_accounting_invariant` - Total supply equals balances + fees burned
- `prop_nonce_never_decreases` - Nonces are monotonically increasing
- `prop_height_monotonicity` - Block height always increases
- `prop_invalid_transactions_rejected` - Invalid transactions are properly rejected

**Standard Chaos Tests** (3 tests):
- `test_high_transaction_volume` - 100 transactions in single block
- `test_zero_balance_transfers` - Zero-amount transfers with fee-only deduction
- `test_concurrent_block_mining` - Concurrent mining with serialization

**Module Structure**:
- Created `tier4_chaos/mod.rs` with chaos testing documentation
- Created `tier4_chaos/property_tests.rs` with proptest framework integration
- Added helper functions for test transaction and hash generation

**Note**: One property test (`prop_consensus_convergence`) temporarily disabled due to runtime compatibility (proptest uses multi-threaded runtime incompatible with PausedClock).

**Feature Flag**: Chaos tests require the `chaos` feature to be enabled:
```bash
cargo test --features chaos
```

**Test Statistics Update**:
- **Before**: 214 tests (0.56s)
- **After**: 225 tests (0.58s) with `--features chaos`
- **Added**: 11 new Tier 4 tests (6 proptests + 3 chaos + 2 module tests)
- **Performance**: ~2.6ms per test

---

## v3.0.2 - Orchestrator Module Test Coverage (2025-11-15)

### Test Coverage Improvements

#### 📊 Tier 0 Orchestrator Tests Expansion

Added 36 comprehensive unit tests for orchestrator modules (Clock and RNG):

**Clock Module Tests** (15 new tests):
- Zero and large duration advancement
- Multiple sequential advancements (10x increments)
- Concurrent reads from multiple tasks
- Sleep expiration correctness
- Multiple concurrent sleeps with different durations
- Default trait implementation
- System clock monotonicity verification
- System clock sleep accuracy testing
- Clock trait object usage
- Precise timing (microsecond/nanosecond level)
- Instant comparison and ordering
- Duration arithmetic validation
- Clock sharing across tasks

**RNG Module Tests** (21 new tests):
- Generation of various types (u8, u16, u32, u64, usize, bool, i32, i64)
- Boundary conditions (single value ranges, equal bounds)
- Large range generation (u64::MAX)
- Negative value ranges
- Empty and large buffer filling
- Deterministic fill_bytes validation
- Shuffle edge cases (empty, single element)
- Shuffle determinism and correctness
- Choose edge cases and distribution
- Seed boundary values (0, u64::MAX)
- Concurrent generation consistency
- Boolean distribution validation
- Invalid environment variable handling
- Cross-session reproducibility
- Float range generation

**Test Statistics Update**:
- **Before**: 178 tests (0.60s)
- **After**: 214 tests (0.56s)
- **Added**: 36 new Tier 0 tests
- **Performance**: Improved to 2.6ms per test

**Orchestrator Module Statistics**:
- **Clock Tests**: 18 total (3 original + 15 new)
- **RNG Tests**: 30 total (9 original + 21 new)
- **DeterministicTestEnv Tests**: 6 total (unchanged)
- **Total Orchestrator**: 54 tests (previously 18)

---

## v3.0.1 - Comprehensive Test Coverage (2025-11-15)

### Test Coverage Improvements

#### 📊 Tier 1 Component Tests Expansion

Added 44 comprehensive unit tests for `TestBlockchain` component:

**Balance Tests** (6 tests):
- Existing account balance queries
- Non-existing account behavior
- Balance updates after sending/receiving
- Multiple sequential balance changes

**Nonce Tests** (4 tests):
- Initial nonce values (zero)
- Nonce increments after transactions
- Sequential nonce progression
- Non-existing account nonce

**Transaction Submission Tests** (2 tests):
- Multiple transaction submission
- Transaction hash verification

**Block Mining Tests** (6 tests):
- Empty block mining
- Single transaction blocks
- Multiple transactions per block
- Sequential block mining
- Block hash generation
- Mempool clearing after mining

**Height and Tips Tests** (4 tests):
- Genesis height verification
- Height progression after mining
- Genesis tips state
- Tips updates after new blocks

**Block Reception Tests** (3 tests):
- Sequential height validation
- Invalid height rejection
- Duplicate block detection

**Block Retrieval Tests** (3 tests):
- Existing block retrieval
- Non-existing block queries
- Beyond-tip height queries

**State Root Tests** (2 tests):
- Deterministic state root computation
- State root changes after transactions

**Accounts KV Tests** (3 tests):
- Single account state
- Multiple accounts state
- State updates after transactions

**Counter Tests** (2 tests):
- Initial counter values
- Counter updates after transactions
- Fee splitting validation (50% burned, 50% miner)

**Confirmed TX Count Tests** (2 tests):
- Initial count (zero)
- Count increments after confirmations

**Clock and Topoheight Tests** (4 tests):
- Clock access verification
- Genesis topoheight
- Topoheight progression

**Edge Case Tests** (7 tests):
- Zero balance accounts
- Large balance handling (1M TOS)
- 50 transactions in single block
- Transactions to self
- Multiple senders in single block
- Empty mempool mining (10 consecutive empty blocks)

**Test Statistics Update**:
- **Before**: 134 tests (0.54s)
- **After**: 178 tests (0.60s)
- **Added**: 44 new Tier 1 tests
- **Performance**: < 11ms per test maintained

---

## v3.0.0 - Phase 4 Complete (2025-01-15)

### Major Features

#### 🎉 Advanced Multi-Node Scenarios

Added 6 comprehensive end-to-end test scenarios demonstrating the framework's full capabilities:

1. **Network Partition and Isolated Mining** (`test_partition_with_competing_chains`)
   - Tests network partition with competing chains
   - Demonstrates partition isolation and state independence
   - 4-node network with [0,1] vs [2,3] partition

2. **Multi-Miner Competition** (`test_multi_miner_competition`)
   - 5-node network with rotating block production
   - Tests concurrent mining and convergence
   - FullMesh topology with 10 transactions across 3 rounds

3. **Cascading Propagation Through Ring** (`test_cascading_propagation_ring`)
   - Demonstrates multi-hop block propagation
   - Ring topology (0→1→2→3→4→0)
   - Tests sequential propagation constraints

4. **Byzantine Behavior Detection** (`test_byzantine_block_rejection`)
   - Invalid block rejection (duplicate blocks, height skipping)
   - Sequential height validation
   - Multiple test cases for different failure modes

5. **High-Throughput Stress Test** (`test_high_throughput_stress`)
   - 50 transactions processed across 10 blocks
   - Tests rapid block production and mempool management
   - Verifies state consistency under high load

6. **Network Healing After Partition** (`test_gradual_partition_healing`)
   - Tests partition healing and re-synchronization
   - Demonstrates block propagation after network recovery
   - 4-node network with two genesis accounts

**File**: `src/tier3_e2e/advanced_scenarios.rs` (395 lines)

#### 🔧 Block Propagation System

Implemented complete block propagation with validation:

- **Block Reception API** (`TestBlockchain::receive_block()`)
  - Sequential height validation
  - Duplicate block detection
  - Transaction processing and state updates
  - Mempool cleanup after block application

- **Block Retrieval API** (`TestBlockchain::get_block_at_height()`)
  - Fetch blocks by height for sharing
  - Enables peer-to-peer synchronization

- **Network-Level Propagation** (`LocalTosNetwork::propagate_block_from()`)
  - Topology-aware propagation
  - Partition-aware filtering
  - Returns count of successfully propagated peers

- **Convenience Method** (`LocalTosNetwork::mine_and_propagate()`)
  - Mine block on specific node
  - Automatically propagate to all connected peers
  - Respects topology and partition constraints

**Files Modified**:
- `src/tier1_component/blockchain.rs` - Block reception/retrieval APIs
- `src/tier2_integration/test_daemon.rs` - RPC wrappers
- `src/tier3_e2e/network.rs` - Network propagation logic

### Documentation

#### 📚 Comprehensive README

Created detailed 540-line README.md with:
- Architecture overview and 4-tier pyramid diagram
- Quick start guide with working examples
- Detailed explanation of each testing tier
- Advanced features (partitions, topologies, time control)
- 3 complete example scenarios with full code
- Best practices and troubleshooting guide
- Performance metrics and contribution guidelines

**File**: `README.md`

#### 📖 Module Documentation

Added missing documentation to all public modules:
- `orchestrator/clock` - Clock abstractions
- `orchestrator/rng` - Deterministic RNG
- `tier2_integration/waiters` - Blockchain state waiters
- `tier3_e2e/waiters` - Consensus convergence primitives
- `utilities/storage` - Temporary storage utilities

### Code Quality

#### ✨ Zero Warnings

- Fixed all unused code warnings (4 total):
  - Added `#[allow(dead_code)]` to reserved helper functions
  - Added public `topology()` getter
  - Marked test-only methods appropriately

- Fixed proptest dependency configuration:
  - Moved from optional to regular dependency
  - Enables property-based testing strategies

#### 🧪 Test Statistics

- **134 tests** in testing-framework (up from 128)
- **6 new advanced scenario tests** added
- **All tests passing** ✅
- **Zero compilation warnings** ✅
- **Test execution time**: ~0.5 seconds

### API Additions

#### New Public Methods

**TestBlockchain**:
```rust
pub async fn receive_block(&self, block: TestBlock) -> Result<()>
pub async fn get_block_at_height(&self, height: u64) -> Result<Option<TestBlock>>
```

**TestDaemon**:
```rust
pub async fn receive_block(&self, block: TestBlock) -> Result<()>
pub async fn get_block_at_height(&self, height: u64) -> Result<Option<TestBlock>>
```

**LocalTosNetwork**:
```rust
pub async fn mine_and_propagate(&self, node_id: usize) -> Result<Hash>
pub fn topology(&self) -> &NetworkTopology
```

### Bug Fixes

- Fixed block propagation placeholder implementation
- Fixed E2E test to use `mine_and_propagate()` for full convergence
- Fixed partition tests to properly verify isolation
- Fixed Byzantine test to check correct error messages

### Testing Enhancements

#### Block Propagation Tests (3 new tests in network.rs)

1. **test_block_propagation** - Basic FullMesh propagation
2. **test_block_propagation_respects_partitions** - Partition awareness
3. **test_full_consensus_convergence** - 5-node multi-block convergence

#### Updated E2E Test

- `test_multi_node_consensus_convergence` now demonstrates full consensus with automatic block propagation

### Framework Capabilities

The framework now supports:

✅ **Component Testing** (Tier 1) - In-process blockchain
✅ **Integration Testing** (Tier 2) - Single daemon with RPC
✅ **Multi-Node E2E** (Tier 3) - Consensus convergence
✅ **Network Partitions** - Simulate network splits
✅ **Block Propagation** - Realistic P2P simulation
✅ **Transaction Propagation** - Mempool synchronization
✅ **Network Topologies** - FullMesh, Ring, Custom
✅ **Byzantine Detection** - Invalid block rejection
✅ **High-Throughput** - Stress testing capabilities
✅ **Deterministic Time** - Full time control
✅ **Reproducible Tests** - Seed-based replay

### Performance

- **Tier 1 (Component)**: < 100ms per test
- **Tier 2 (Integration)**: < 500ms per test
- **Tier 3 (E2E)**: < 1s per test
- **Full Suite**: 0.54s for 134 tests

### Breaking Changes

None - all changes are additive.

### Migration Guide

No migration needed. All existing tests continue to work unchanged.

New capabilities can be adopted incrementally:
- Use `mine_and_propagate()` instead of manual mining + propagation
- Use advanced scenarios as templates for complex tests

### Future Plans

🚧 **Planned for Next Release**:
- Chain reorganization support
- DAG consensus validation
- Full GHOSTDAG testing
- Difficulty adjustment scenarios
- Advanced chaos engineering
- Performance benchmarks

---

## v2.0.0 - Transaction Propagation (Previous Release)

Phase 3 implementation with transaction propagation across multi-node networks.

## v1.0.0 - Foundation (Initial Release)

- Tier 1: Component testing (TestBlockchain)
- Tier 2: Integration testing (TestDaemon)
- Tier 3: Basic multi-node networks
- Deterministic orchestration (Clock + RNG)
