# TOS Testing Framework V3.0

A comprehensive, deterministic testing framework for the TOS blockchain that provides everything from component-level unit tests to multi-node end-to-end scenarios.

## Table of Contents

- [Overview](#overview)
- [Architecture](#architecture)
- [Quick Start](#quick-start)
- [Testing Tiers](#testing-tiers)
- [Advanced Features](#advanced-features)
- [Examples](#examples)
- [Best Practices](#best-practices)

## Overview

The TOS Testing Framework V3.0 provides a unified approach to testing all aspects of the TOS blockchain:

- **Deterministic**: All tests are fully reproducible via seeded RNG and controlled time
- **Fast**: Most tests complete in < 1 second with paused time
- **Comprehensive**: Covers all layers from component to network level
- **Production-Like**: Uses real RocksDB storage, minimal mocking
- **Developer-Friendly**: Clear APIs, excellent error messages, seed-based replay

### Key Features

✅ **Full Determinism**: Control time and randomness for perfect reproducibility
✅ **Multi-Node Networks**: Test consensus, partitions, and network healing
✅ **Block Propagation**: Simulate realistic P2P block distribution
✅ **Transaction Propagation**: Test mempool synchronization across nodes
✅ **Network Topologies**: FullMesh, Ring, Custom configurations
✅ **Network Partitions**: Simulate network splits and healing
✅ **Property-Based Testing**: QuickCheck-style invariant testing
✅ **Scenario Testing**: YAML-based declarative test scenarios

### Test Statistics

- **225 tests** in testing-framework (with `--features chaos`)
- **214 tests** without chaos feature
- **900+ tests** across full workspace
- **All tests passing** ✅
- **Zero compilation warnings** ✅

## Architecture

### 4-Tier Testing Pyramid

```
           ┌─────────────────────┐
           │  Tier 3: E2E        │  Multi-node networks, consensus
           │  (advanced_scenarios)│  convergence, partitions
           ├─────────────────────┤
           │  Tier 2: Integration│  Single daemon, RPC, waiters
           │  (test_daemon)      │  state transitions
           ├─────────────────────┤
           │  Tier 1: Component  │  TestBlockchain, in-process
           │  (blockchain)       │  mining, transactions
           ├─────────────────────┤
           │  Tier 0: Orchestrator│ Clock, RNG, deterministic
           │  (deterministic_env) │ infrastructure
           └─────────────────────┘
```

### Module Overview

- **`orchestrator/`** - Deterministic clock and RNG for reproducible tests
- **`tier1_component/`** - Component-level testing (TestBlockchain)
- **`tier2_integration/`** - Integration testing (TestDaemon, RPC, waiters)
- **`tier3_e2e/`** - End-to-end multi-node networks
- **`utilities/`** - Shared utilities (temp storage, helpers)
- **`scenarios/`** - YAML-based declarative test scenarios
- **`invariants/`** - Property-based invariant checkers

## Quick Start

### Running Tests

```bash
# Run all tests (includes chaos tests)
cargo test --workspace --features chaos

# Run all tests without chaos feature
cargo test --workspace

# Run tier 3 E2E tests only
cargo test --lib tier3_e2e

# Run tier 4 chaos tests only
cargo test --lib tier4_chaos --features chaos

# Run advanced scenarios
cargo test --lib tier3_e2e::advanced_scenarios

# Run a specific test
cargo test test_partition_with_competing_chains
```

### Your First Test

```rust
use tos_testing_framework::tier3_e2e::network::LocalTosNetworkBuilder;
use tos_testing_framework::tier2_integration::rpc_helpers::*;
use anyhow::Result;

#[tokio::test]
async fn test_my_first_network() -> Result<()> {
    // Create a 3-node network
    let network = LocalTosNetworkBuilder::new()
        .with_nodes(3)
        .with_genesis_account("alice", 1_000_000_000)
        .build()
        .await?;

    let alice = network.get_genesis_account("alice").unwrap().0.clone();

    // Submit and mine a transaction
    let tx = create_test_tx(alice.clone(), create_test_address(10), 100_000, 100, 1);
    network.submit_and_propagate(0, tx).await?;
    network.mine_and_propagate(0).await?;

    // Verify all nodes converged
    for i in 0..3 {
        assert_tip_height(&network.node(i), 1).await?;
    }

    Ok(())
}
```

## Testing Tiers

### Tier 1: Component Testing

Fast, in-process testing of individual components.

```rust
use tos_testing_framework::tier1_component::TestBlockchainBuilder;

#[tokio::test]
async fn test_blockchain_component() -> Result<()> {
    let blockchain = TestBlockchainBuilder::new()
        .with_funded_account_count(5)
        .build()
        .await?;

    // Test transactions, mining, state transitions
    blockchain.submit_transaction(tx).await?;
    let block = blockchain.mine_block().await?;

    assert_eq!(blockchain.get_tip_height().await?, 1);
    Ok(())
}
```

**Use Cases:**
- Transaction validation
- Block mining logic
- State mutations
- Balance calculations

### Tier 2: Integration Testing

Single daemon with RPC interface.

```rust
use tos_testing_framework::tier2_integration::TestDaemonBuilder;

#[tokio::test]
async fn test_daemon_integration() -> Result<()> {
    let daemon = TestDaemonBuilder::new()
        .with_funded_accounts(vec![
            ("alice", 1_000_000),
            ("bob", 500_000),
        ])
        .build()
        .await?;

    // Use RPC-like interface
    let balance = daemon.get_balance(&alice).await?;
    daemon.submit_transaction(tx).await?;
    daemon.mine_block().await?;

    Ok(())
}
```

**Use Cases:**
- RPC endpoint testing
- Mempool operations
- State queries
- Daemon lifecycle

### Tier 3: End-to-End Testing

Multi-node networks with consensus and propagation.

```rust
use tos_testing_framework::tier3_e2e::network::LocalTosNetworkBuilder;

#[tokio::test]
async fn test_consensus_convergence() -> Result<()> {
    let network = LocalTosNetworkBuilder::new()
        .with_nodes(5)
        .with_topology(NetworkTopology::FullMesh)
        .with_genesis_account("alice", 10_000_000)
        .build()
        .await?;

    // Test multi-node scenarios
    network.submit_and_propagate(0, tx).await?;
    network.mine_and_propagate(0).await?;

    // All nodes should converge
    for i in 0..5 {
        assert_tip_height(&network.node(i), 1).await?;
    }

    Ok(())
}
```

**Use Cases:**
- Consensus convergence
- Network partitions
- Block propagation
- Fork resolution
- Byzantine behavior

## Advanced Features

### Network Partitions

Simulate network splits and healing:

```rust
// Create partition: [0,1] vs [2,3]
network.partition_groups(&[0, 1], &[2, 3]).await?;

// Each side mines independently
network.mine_and_propagate(0).await?;  // Side A
network.mine_and_propagate(2).await?;  // Side B

// Heal partition
network.heal_all_partitions().await;

// Propagate blocks to achieve convergence
network.propagate_block_from(0, 1).await?;
```

### Network Topologies

Three built-in topologies:

```rust
// Full mesh - all nodes connected
.with_topology(NetworkTopology::FullMesh)

// Ring - circular connections (0→1→2→3→0)
.with_topology(NetworkTopology::Ring)

// Custom - specify exact connections
.with_topology(NetworkTopology::Custom(vec![
    vec![1, 2],    // Node 0 → nodes 1, 2
    vec![0, 2],    // Node 1 → nodes 0, 2
    vec![0, 1],    // Node 2 → nodes 0, 1
]))
```

### Deterministic Time Control

```rust
use tos_testing_framework::orchestrator::DeterministicTestEnv;

#[tokio::test(start_paused = true)]
async fn test_with_time_control() {
    let env = DeterministicTestEnv::new_time_paused();

    // Advance time by 1 hour
    env.advance_time(Duration::from_secs(3600)).await;

    // Time-based assertions
    assert_eq!(env.clock.now() - start, Duration::from_secs(3600));
}
```

### Reproducible Randomness

```rust
// Test fails and prints seed
// Output: "TestRng seed: 0xa3f5c8e1b2d94706"

// Replay exact failure:
TOS_TEST_SEED=0xa3f5c8e1b2d94706 cargo test test_name
```

## Examples

### Example 1: Network Partition Test

```rust
#[tokio::test]
async fn test_partition_and_healing() -> Result<()> {
    let network = LocalTosNetworkBuilder::new()
        .with_nodes(4)
        .with_genesis_account("alice", 10_000_000)
        .with_genesis_account("bob", 10_000_000)
        .build()
        .await?;

    let alice = network.get_genesis_account("alice").unwrap().0.clone();
    let bob = network.get_genesis_account("bob").unwrap().0.clone();

    // Create partition: [0,1] vs [2,3]
    network.partition_groups(&[0, 1], &[2, 3]).await?;

    // Side A: Alice's transactions
    for nonce in 1..=3 {
        let tx = create_test_tx(alice.clone(), create_test_address(50), 1_000_000, 100, nonce);
        network.submit_and_propagate(0, tx).await?;
    }

    // Side B: Bob's transactions
    for nonce in 1..=2 {
        let tx = create_test_tx(bob.clone(), create_test_address(60), 2_000_000, 100, nonce);
        network.submit_and_propagate(2, tx).await?;
    }

    // Both sides mine independently
    network.mine_and_propagate(0).await?;  // Side A
    network.mine_and_propagate(2).await?;  // Side B

    // Verify isolation
    assert_nonce(&network.node(0), &alice, 3).await?;
    assert_nonce(&network.node(2), &bob, 2).await?;

    // Verify no cross-contamination
    assert_nonce(&network.node(0), &bob, 0).await?;
    assert_nonce(&network.node(2), &alice, 0).await?;

    Ok(())
}
```

### Example 2: Multi-Miner Competition

```rust
#[tokio::test]
async fn test_multi_miner_competition() -> Result<()> {
    let network = LocalTosNetworkBuilder::new()
        .with_nodes(5)
        .with_topology(NetworkTopology::FullMesh)
        .with_genesis_account("whale", 100_000_000)
        .build()
        .await?;

    let whale = network.get_genesis_account("whale").unwrap().0.clone();

    // Submit 10 transactions
    for nonce in 1..=10 {
        let tx = create_test_tx(whale.clone(), create_test_address(70 + nonce), 500_000, 100, nonce);
        network.submit_and_propagate(0, tx).await?;
    }

    // Rotate miners across 3 rounds
    for round in 1..=3 {
        let miner = round % 5;
        network.mine_and_propagate(miner).await?;

        // Verify convergence after each round
        for i in 0..5 {
            assert_tip_height(&network.node(i), round as u64).await?;
        }
    }

    // Verify final state consistency
    for i in 0..5 {
        assert_nonce(&network.node(i), &whale, 10).await?;
    }

    Ok(())
}
```

### Example 3: Byzantine Behavior Detection

```rust
#[tokio::test]
async fn test_invalid_block_rejection() -> Result<()> {
    let network = LocalTosNetworkBuilder::new()
        .with_nodes(3)
        .with_genesis_account("alice", 5_000_000)
        .build()
        .await?;

    // Mine valid block
    network.mine_and_propagate(0).await?;

    // Get block at height 1
    let block = network.node(0).daemon().get_block_at_height(1).await?.unwrap();

    // Try to send same block again (duplicate)
    let result = network.node(1).daemon().receive_block(block).await;

    // Should fail - height validation
    assert!(result.is_err());

    Ok(())
}
```

## Best Practices

### 1. Use Deterministic Time

```rust
#[tokio::test(start_paused = true)]  // ← Important!
async fn test_with_time() {
    let env = DeterministicTestEnv::new_time_paused();
    // ...
}
```

### 2. Always Use Helper Assertions

```rust
// ✅ Good - uses helper with clear error messages
assert_tip_height(&node, 1).await?;
assert_nonce(&node, &alice, 5).await?;
assert_balance(&node, &bob, 1_000_000).await?;

// ❌ Bad - manual assertions with unclear errors
assert_eq!(node.get_tip_height().await?, 1);
```

### 3. Create Meaningful Genesis Accounts

```rust
// ✅ Good - descriptive names
.with_genesis_account("alice", 1_000_000)
.with_genesis_account("miner", 10_000_000)
.with_genesis_account("validator", 5_000_000)

// ❌ Bad - generic names
.with_funded_account_count(3)
```

### 4. Test Isolation with Partitions

```rust
// Verify partition isolation
assert_nonce(&network.node(0), &alice, 3).await?;
assert_nonce(&network.node(2), &alice, 0).await?;  // ← Other side has no state
```

### 5. Use mine_and_propagate() for Consensus

```rust
// ✅ Good - automatic propagation
network.mine_and_propagate(0).await?;

// ❌ Bad - manual propagation (error-prone)
network.node(0).daemon().mine_block().await?;
for i in 1..network.node_count() {
    network.propagate_block_from(0, i).await?;
}
```

## Test Categories

### ✅ Currently Supported

- Component-level unit tests (Tier 1)
- Integration tests with single daemon (Tier 2)
- Multi-node consensus convergence (Tier 3)
- Network partitions and healing (Tier 3)
- Block propagation (Tier 3)
- Transaction propagation (Tier 3)
- Byzantine behavior detection (Tier 3)
- High-throughput stress testing (Tier 3)
- Multi-hop propagation (Tier 3)

### 🚧 Planned Features

- Chain reorganization (requires fork resolution logic)
- DAG consensus testing
- Full GHOSTDAG validation
- Difficulty adjustment testing
- Advanced chaos scenarios
- Performance benchmarking

## Troubleshooting

### Test Fails with "Time not paused"

Add `#[tokio::test(start_paused = true)]`:

```rust
#[tokio::test(start_paused = true)]  // ← Add this
async fn test_name() { ... }
```

### Test is Non-Deterministic

1. Check if using `SystemClock` instead of `PausedClock`
2. Verify all randomness uses `TestRng`
3. Check for external time sources (e.g., `std::time::Instant::now()`)

### Replay a Failed Test

```bash
# Test prints: "TestRng seed: 0xa3f5c8e1b2d94706"
TOS_TEST_SEED=0xa3f5c8e1b2d94706 cargo test test_name
```

### Collecting Failure Artifacts

For complex test failures, use the `ArtifactCollector` to capture detailed state:

```rust
use tos_testing_framework::utilities::ArtifactCollector;

#[tokio::test]
async fn test_with_artifacts() -> Result<()> {
    let mut collector = ArtifactCollector::new("test_consensus_failure");
    let rng = TestRng::new_from_env_or_random();
    collector.set_rng_seed(rng.seed());

    // Run test...
    let network = setup_network().await?;

    // On failure, capture state
    if let Err(e) = run_test(&network).await {
        // Capture blockchain state from each node
        for i in 0..network.node_count() {
            let state = network.node(i).get_state_snapshot().await?;
            collector.add_blockchain_state(state);
        }

        // Capture topology
        let topology = network.get_topology_snapshot();
        collector.save_topology(topology);

        // Set failure reason
        collector.set_failure_reason(format!("{:?}", e));

        // Save to disk
        let artifact_path = collector.save("./artifacts/").await?;
        eprintln!("Artifact saved to: {}", artifact_path.display());

        return Err(e);
    }

    Ok(())
}
```

**Replay from artifact:**

```rust
use tos_testing_framework::utilities::{load_artifact, print_artifact_summary};

#[tokio::test]
async fn inspect_failure() -> Result<()> {
    let artifact = load_artifact("./artifacts/test_consensus_failure_20251115.json").await?;

    // Print human-readable summary
    print_artifact_summary(&artifact);

    // Access artifact data
    println!("RNG Seed: 0x{:016x}", artifact.metadata.rng_seed.unwrap());
    println!("Failed at: {}", artifact.metadata.timestamp);

    // Replay with same seed
    let rng = TestRng::with_seed(artifact.metadata.rng_seed.unwrap());
    // ... run test with same seed ...

    Ok(())
}
```

See [CI_SETUP.md](./CI_SETUP.md) for more details on artifact collection in CI/CD.

## Contributing

When adding new tests:

1. Choose appropriate tier (1, 2, or 3)
2. Use deterministic time (`PausedClock`)
3. Use seeded RNG (`TestRng`)
4. Add comprehensive assertions
5. Document expected behavior
6. Use descriptive test names

## Performance

Typical test performance:

- **Tier 0 (Orchestrator)**: < 5ms per test
- **Tier 1 (Component)**: < 10ms per test
- **Tier 2 (Integration)**: < 20ms per test
- **Tier 3 (E2E)**: < 50ms per test
- **Tier 4 (Chaos/Proptest)**: Variable (proptest runs multiple iterations)
- **Full Suite**: ~0.58 seconds for 225 tests with chaos feature (~2.6ms average per test)
- **Base Suite**: ~0.56 seconds for 214 tests without chaos feature

## License

Same as TOS blockchain project.

## Support

For questions or issues:
- Check existing tests for examples
- Review documentation in module headers
- Open an issue on GitHub

---

**TOS Testing Framework V3.0** - Deterministic, Fast, Comprehensive ✨
