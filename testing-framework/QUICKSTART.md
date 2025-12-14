# TOS Testing Framework - Quick Start Guide

**Version**: v3.0.6
**For**: Developers who want to quickly start testing TOS blockchain components

---

## 🚀 5-Minute Quick Start

### 1. Add Testing Framework Dependency

Add to your `Cargo.toml`:

```toml
[dev-dependencies]
tos-testing-framework = { path = "../testing-framework" }
```

### 2. Your First Test (Component Level)

```rust
use tos_testing_framework::tier1_component::TestBlockchainBuilder;
use anyhow::Result;

#[tokio::test]
async fn my_first_test() -> Result<()> {
    // Create a test blockchain
    let blockchain = TestBlockchainBuilder::new()
        .with_funded_account_count(2)
        .build()
        .await?;

    // Mine a block
    let block = blockchain.mine_block().await?;

    // Verify
    assert_eq!(blockchain.get_tip_height().await?, 1);

    Ok(())
}
```

**Run it**:
```bash
cargo test my_first_test
```

---

## 📋 Testing Patterns by Use Case

### Pattern 1: Test Smart Contract

**Use when**: Testing TAKO smart contracts

```rust
use tos_testing_framework::utilities::{
    create_contract_test_storage, execute_test_contract,
};
use tos_common::crypto::{Hash, KeyPair};

#[tokio::test]
async fn test_my_contract() -> anyhow::Result<()> {
    // Setup storage
    let account = KeyPair::new();
    let storage = create_contract_test_storage(&account, 1_000_000).await?;

    // Load and execute contract
    let bytecode = include_bytes!("path/to/contract.so");
    let result = execute_test_contract(
        bytecode,
        &storage,
        1, // topoheight
        &Hash::zero()
    ).await?;

    // Verify
    assert_eq!(result.return_value, 0);
    assert!(result.compute_units_used > 0);

    Ok(())
}
```

**Documentation**: See `CONTRACT_TESTING.md` for complete guide

---

### Pattern 2: Test Multi-Node Network

**Use when**: Testing consensus, partitions, or propagation

```rust
use tos_testing_framework::tier3_e2e::network::LocalTosNetworkBuilder;
use tos_testing_framework::tier3_e2e::network::NetworkTopology;

#[tokio::test]
async fn test_network_consensus() -> anyhow::Result<()> {
    // Create 5-node network
    let network = LocalTosNetworkBuilder::new()
        .with_nodes(5)
        .with_topology(NetworkTopology::FullMesh)
        .with_genesis_account("alice", 10_000_000)
        .build()
        .await?;

    // Mine on node 0 and propagate
    network.mine_and_propagate(0).await?;

    // Verify all nodes converged
    for i in 0..5 {
        let height = network.node(i).daemon().get_tip_height().await?;
        assert_eq!(height, 1);
    }

    Ok(())
}
```

---

### Pattern 3: Test with Failure Debugging

**Use when**: You need to debug complex failures

```rust
use tos_testing_framework::utilities::artifacts::ArtifactCollector;
use tos_testing_framework::orchestrator::rng::TestRng;

#[tokio::test]
async fn test_with_debugging() -> anyhow::Result<()> {
    let mut collector = ArtifactCollector::new("test_name");
    let rng = TestRng::new_from_env_or_random();
    collector.set_rng_seed(rng.seed());

    // Run your test...
    let network = setup_network().await?;

    match run_test(&network).await {
        Ok(_) => Ok(()),
        Err(e) => {
            // Capture failure state
            collector.save_topology(network.get_topology_snapshot());
            for i in 0..network.node_count() {
                let state = capture_node_state(&network, i).await?;
                collector.add_blockchain_state(state);
            }
            collector.set_failure_reason(format!("{:?}", e));

            // Save artifact
            let path = collector.save("./artifacts/").await?;
            eprintln!("\n=== Test Failed ===");
            eprintln!("Artifact: {}", path.display());
            eprintln!("Replay: TOS_TEST_SEED=0x{:016x} cargo test test_name\n", rng.seed());

            Err(e)
        }
    }
}
```

---

### Pattern 4: Test Transaction Processing

**Use when**: Testing transaction validation, mempool, etc.

```rust
use tos_testing_framework::tier1_component::TestBlockchainBuilder;
use tos_testing_framework::tier2_integration::rpc_helpers::create_test_tx;

#[tokio::test]
async fn test_transaction_processing() -> anyhow::Result<()> {
    let blockchain = TestBlockchainBuilder::new()
        .with_funded_account_count(2)
        .build()
        .await?;

    let accounts = blockchain.get_funded_accounts();
    let sender = accounts[0].clone();
    let recipient = accounts[1].clone();

    // Create transaction
    let tx = create_test_tx(
        sender,
        recipient.get_public_key().to_address(),
        100_000, // amount
        100,     // fee
        1        // nonce
    );

    // Submit transaction
    blockchain.submit_transaction(tx.clone()).await?;

    // Mine block
    blockchain.mine_block().await?;

    // Verify transaction was processed
    // (check balances, nonces, etc.)

    Ok(())
}
```

---

### Pattern 5: Test Network Partition

**Use when**: Testing network splits and healing

```rust
use tos_testing_framework::tier3_e2e::network::LocalTosNetworkBuilder;

#[tokio::test]
async fn test_partition_healing() -> anyhow::Result<()> {
    let network = LocalTosNetworkBuilder::new()
        .with_nodes(4)
        .with_genesis_account("alice", 10_000_000)
        .build()
        .await?;

    // Create partition: [0,1] vs [2,3]
    network.partition_groups(&[0, 1], &[2, 3]).await?;

    // Each side mines independently
    network.mine_and_propagate(0).await?; // Side A
    network.mine_and_propagate(2).await?; // Side B

    // Verify isolation (different heights)
    let height_a = network.node(0).daemon().get_tip_height().await?;
    let height_b = network.node(2).daemon().get_tip_height().await?;
    assert_eq!(height_a, 1);
    assert_eq!(height_b, 1);

    // Heal partition
    network.heal_all_partitions().await;

    // Propagate to achieve convergence
    network.propagate_all_blocks().await?;

    // Verify convergence (same height)
    for i in 0..4 {
        let height = network.node(i).daemon().get_tip_height().await?;
        assert_eq!(height, 1); // Should converge
    }

    Ok(())
}
```

---

## 🎯 Test Organization

### Recommended Structure

```
my-package/
├── src/
│   └── lib.rs
├── tests/
│   ├── unit_tests.rs          # Tier 1: Component tests
│   ├── integration_tests.rs   # Tier 2: Single daemon tests
│   ├── contract_tests.rs      # Smart contract tests
│   └── e2e_tests.rs           # Tier 3: Multi-node tests
└── Cargo.toml
```

### Test Naming Convention

```rust
// Unit test
#[tokio::test]
async fn test_block_validation() -> Result<()> { ... }

// Integration test
#[tokio::test]
async fn test_daemon_rpc_get_balance() -> Result<()> { ... }

// E2E test
#[tokio::test]
async fn test_network_partition_and_healing() -> Result<()> { ... }

// Contract test
#[tokio::test]
async fn test_erc20_transfer_contract() -> Result<()> { ... }
```

---

## 🔧 Common Helper Functions

### Create Test Accounts

```rust
use tos_common::crypto::KeyPair;

// Single account
let alice = KeyPair::new();

// Multiple accounts
let accounts: Vec<KeyPair> = (0..5)
    .map(|_| KeyPair::new())
    .collect();
```

### Create Test Transaction

```rust
use tos_testing_framework::tier2_integration::rpc_helpers::create_test_tx;

let tx = create_test_tx(
    sender,
    recipient_address,
    amount,
    fee,
    nonce
);
```

### Assert Helpers

```rust
use tos_testing_framework::tier2_integration::rpc_helpers::*;

// Assert tip height
assert_tip_height(&node, expected_height).await?;

// Assert balance
assert_balance(&node, &account, expected_balance).await?;

// Assert nonce
assert_nonce(&node, &account, expected_nonce).await?;
```

---

## ⚡ Performance Tips

### 1. Use `start_paused = true` for Time-Based Tests

```rust
#[tokio::test(start_paused = true)]  // ← Add this
async fn test_with_time_control() { ... }
```

### 2. Minimize Network Size

```rust
// ❌ Slow: 10 nodes when 3 is enough
.with_nodes(10)

// ✅ Fast: Use minimum nodes needed
.with_nodes(3)
```

### 3. Use Parallel Test Execution

```rust
// ✅ Tests run in parallel by default
#[tokio::test]
async fn test_a() { ... }

#[tokio::test]
async fn test_b() { ... }
```

### 4. Reuse Storage Where Possible

```rust
// ✅ Create storage once, use multiple times
let storage = create_test_rocksdb_storage().await;

test_scenario_1(&storage).await?;
test_scenario_2(&storage).await?;
```

---

## 🐛 Debugging Tips

### 1. Reproduce Failures with RNG Seed

When a test fails, it prints the seed:

```
TestRng seed: 0xa3f5c8e1b2d94706
```

Replay it:

```bash
TOS_TEST_SEED=0xa3f5c8e1b2d94706 cargo test test_name
```

### 2. Enable Logging

```bash
RUST_LOG=debug cargo test test_name -- --nocapture
```

### 3. Use Artifact Collection

For complex failures, use `ArtifactCollector` to capture full state (see Pattern 3 above).

### 4. Inspect Test Output

```bash
# Show test output
cargo test test_name -- --nocapture

# Show only failed tests
cargo test test_name -- --nocapture | grep -A 10 FAILED
```

---

## 📚 Complete Examples

### Example 1: Simple Balance Test

```rust
use tos_testing_framework::tier1_component::TestBlockchainBuilder;
use tos_common::config::COIN_VALUE;

#[tokio::test]
async fn test_balance_after_transfer() -> anyhow::Result<()> {
    let blockchain = TestBlockchainBuilder::new()
        .with_funded_account_count(2)
        .build()
        .await?;

    let accounts = blockchain.get_funded_accounts();
    let alice = accounts[0].clone();
    let bob = accounts[1].clone();

    // Get initial balances
    let alice_initial = blockchain.get_balance(&alice).await?;
    let bob_initial = blockchain.get_balance(&bob).await?;

    // Transfer 1000 TOS from Alice to Bob
    let tx = create_test_tx(
        alice.clone(),
        bob.get_public_key().to_address(),
        1000 * COIN_VALUE,
        100, // fee
        1    // nonce
    );

    blockchain.submit_transaction(tx).await?;
    blockchain.mine_block().await?;

    // Verify balances changed correctly
    let alice_final = blockchain.get_balance(&alice).await?;
    let bob_final = blockchain.get_balance(&bob).await?;

    assert_eq!(alice_final, alice_initial - 1000 * COIN_VALUE - 100);
    assert_eq!(bob_final, bob_initial + 1000 * COIN_VALUE);

    Ok(())
}
```

### Example 2: Contract with Storage

```rust
use tos_testing_framework::utilities::*;
use tos_common::crypto::{Hash, KeyPair};

#[tokio::test]
async fn test_counter_contract() -> anyhow::Result<()> {
    let account = KeyPair::new();
    let storage = create_contract_test_storage(&account, 1_000_000).await?;

    let bytecode = include_bytes!("../fixtures/counter.so");
    let contract_hash = Hash::from([1u8; 32]);

    // Execute contract 5 times
    for i in 1..=5 {
        let result = execute_test_contract(
            bytecode,
            &storage,
            i, // topoheight
            &contract_hash
        ).await?;

        assert_eq!(result.return_value, 0);

        // Verify counter incremented
        let count = get_contract_storage(
            &storage,
            contract_hash,
            b"count",
            i
        ).await?;

        // Counter should equal topoheight
        if let Some(value) = count {
            let count_u64 = u64::from_le_bytes(
                value[..8].try_into().unwrap()
            );
            assert_eq!(count_u64, i);
        }
    }

    Ok(())
}
```

### Example 3: Multi-Miner Competition

```rust
use tos_testing_framework::tier3_e2e::network::*;

#[tokio::test]
async fn test_multi_miner_rotation() -> anyhow::Result<()> {
    let network = LocalTosNetworkBuilder::new()
        .with_nodes(5)
        .with_topology(NetworkTopology::FullMesh)
        .with_genesis_account("whale", 100_000_000)
        .build()
        .await?;

    // Rotate through miners
    for round in 0..5 {
        let miner_id = round % 5;

        // Mine on rotating miner
        network.mine_and_propagate(miner_id).await?;

        // Verify all nodes converged
        for i in 0..5 {
            let height = network.node(i).daemon().get_tip_height().await?;
            assert_eq!(height, (round + 1) as u64);
        }
    }

    Ok(())
}
```

---

## 📖 Further Reading

- **README.md** - Complete framework overview
- **CONTRACT_TESTING.md** - Smart contract testing guide (400+ lines)
- **IMPLEMENTATION_STATUS.md** - Detailed status and features
- **RECENT_IMPROVEMENTS.md** - Latest changes (v3.0.6)

### Module Documentation

```bash
cargo doc --package tos-testing-framework --open
```

### Run Example Tests

```bash
# All tests
cargo test --package tos-testing-framework

# Contract tests only
cargo test --package tos-testing-framework --test contract_integration_example

# Artifact examples
cargo test --package tos-testing-framework --test artifact_collection_example
```

---

## 🎓 Next Steps

1. **Start Simple**: Begin with Tier 1 component tests
2. **Add Complexity**: Move to Tier 2 integration tests
3. **Test Contracts**: Use contract testing helpers
4. **Test Networks**: Use Tier 3 multi-node tests
5. **Add Debugging**: Use artifact collection for failures

### Learning Path

**Week 1**: Component tests (Pattern 1, 4)
**Week 2**: Integration tests + contracts (Pattern 2)
**Week 3**: Multi-node tests (Pattern 2, 5)
**Week 4**: Advanced debugging (Pattern 3)

---

## 🆘 Getting Help

### Common Issues

**Issue**: "Time not paused" error
**Solution**: Add `#[tokio::test(start_paused = true)]`

**Issue**: Non-deterministic test
**Solution**: Use `TestRng` instead of `rand::random()`

**Issue**: Test fails intermittently
**Solution**: Capture RNG seed and replay: `TOS_TEST_SEED=0x... cargo test`

**Issue**: Contract execution fails
**Solution**: Check bytecode is valid ELF and storage is initialized

### Documentation

- Quick questions: Check README.md
- Contract testing: Read CONTRACT_TESTING.md
- Detailed status: See IMPLEMENTATION_STATUS.md
- Examples: Browse tests/ directory

### Community

- GitHub Issues: Report bugs or ask questions
- Documentation: Contribute improvements via PR

---

**Version**: v3.0.6
**Status**: Production Ready ✅
**License**: Same as TOS blockchain project

Happy Testing! 🎉
