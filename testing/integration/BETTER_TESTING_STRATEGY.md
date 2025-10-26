# Better Testing Strategy for TOS Integration Tests

## Problem with Current Approach

After reviewing the code, I found several issues:

1. **Tests don't actually test anything meaningful**:
   - Line 183: `NOTE: In real implementation, this would use network_simulator to block traffic. For now, this is a logical placeholder`
   - Tests just sleep and record fake metrics
   - No verification of actual node behavior

2. **Pure mock approach is wrong**:
   - If everything is mocked, we're just testing our mock implementation
   - Doesn't catch real bugs in daemon code
   - Gives false confidence

3. **Pure real daemon approach is slow and flaky**:
   - 30-second startup per test
   - Timeout issues
   - Resource intensive

## Better Solution: Hybrid Testing Strategy

Use **real daemon components** but with **controlled test environment**.

### Three-Layer Testing Pyramid

```
┌─────────────────────────────────────────┐
│  E2E Tests (Real Daemons)               │  ← Few, slow, high confidence
│  - Run nightly or on-demand             │
│  - Full integration validation          │
├─────────────────────────────────────────┤
│  Component Tests (Real Logic, Mocked I/O)│ ← Most tests here
│  - Use real blockchain/consensus code   │
│  - Mock only network/RPC layer          │
│  - Fast, deterministic, meaningful       │
├─────────────────────────────────────────┤
│  Unit Tests (Pure Logic)                │  ← Many, very fast
│  - Test individual functions            │
│  - Already exist in daemon/tests/       │
└─────────────────────────────────────────┘
```

### Component Test Architecture (Recommended)

**Use real daemon components in-process, mock only I/O:**

```rust
pub struct TestNode {
    /// Real blockchain instance (in-memory storage)
    blockchain: Arc<RwLock<Blockchain>>,

    /// Real consensus engine
    consensus: Arc<ConsensusEngine>,

    /// Real mempool
    mempool: Arc<Mempool>,

    /// Mock network layer (in-memory message passing)
    network: MockNetwork,

    /// Mock RPC (direct function calls, no HTTP)
    rpc: MockRpc,
}

impl TestNode {
    pub async fn new(node_id: usize) -> Result<Self> {
        // Use real blockchain with in-memory storage (fast!)
        let storage = InMemoryStorage::new();
        let blockchain = Blockchain::new(storage)?;

        // Use real consensus engine
        let consensus = ConsensusEngine::new(blockchain.clone())?;

        // Use real mempool
        let mempool = Mempool::new(blockchain.clone())?;

        // Mock only network I/O
        let network = MockNetwork::new(node_id);

        // Mock only RPC I/O (call functions directly)
        let rpc = MockRpc::new(blockchain.clone(), mempool.clone());

        Ok(Self {
            blockchain,
            consensus,
            mempool,
            network,
            rpc,
        })
    }

    /// Submit transaction - uses REAL validation logic
    pub async fn submit_transaction(&self, tx: Transaction) -> Result<Hash> {
        // This calls the REAL mempool.add_transaction()
        self.mempool.add_transaction(tx).await
    }

    /// Process block - uses REAL consensus logic
    pub async fn process_block(&self, block: Block) -> Result<()> {
        // This calls the REAL blockchain.add_block() and consensus validation
        self.consensus.validate_block(&block).await?;
        self.blockchain.write().await.add_block(block).await
    }
}
```

### Example: Network Partition Test (Component Approach)

```rust
#[tokio::test]
async fn test_network_partition_recovery() -> Result<()> {
    // Create 3 nodes with REAL blockchain/consensus logic
    let mut node0 = TestNode::new(0).await?;
    let mut node1 = TestNode::new(1).await?;
    let mut node2 = TestNode::new(2).await?;

    // Connect them via mock network (instant, in-memory)
    let mut network = MockNetwork::new();
    network.connect(&node0, &node1);
    network.connect(&node1, &node2);
    network.connect(&node0, &node2);

    // Phase 1: Normal operation - submit transactions
    for i in 0..10 {
        let tx = create_test_transaction(i);
        node0.submit_transaction(tx.clone()).await?;

        // REAL propagation logic (via mock network)
        network.propagate_transaction(0, tx).await;
    }

    // Wait for REAL consensus to process
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify all nodes have same state (REAL verification!)
    let state0 = node0.blockchain.read().await.get_tip_hash();
    let state1 = node1.blockchain.read().await.get_tip_hash();
    let state2 = node2.blockchain.read().await.get_tip_hash();
    assert_eq!(state0, state1);
    assert_eq!(state1, state2);

    // Phase 2: Create network partition
    network.partition(vec![0], vec![1, 2]);

    // Nodes 0 is isolated, submit tx only to node 1
    for i in 10..20 {
        let tx = create_test_transaction(i);
        node1.submit_transaction(tx.clone()).await?;
        network.propagate_transaction(1, tx).await; // Only reaches node2, not node0
    }

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify divergence (REAL state check!)
    let state0_partitioned = node0.blockchain.read().await.get_tip_hash();
    let state1_partitioned = node1.blockchain.read().await.get_tip_hash();
    assert_ne!(state0_partitioned, state1_partitioned); // They should diverge!

    // Phase 3: Heal partition
    network.heal_partition();

    // Trigger REAL sync protocol
    network.trigger_sync(0, 1).await;

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Verify convergence (REAL state check!)
    let state0_healed = node0.blockchain.read().await.get_tip_hash();
    let state1_healed = node1.blockchain.read().await.get_tip_hash();
    let state2_healed = node2.blockchain.read().await.get_tip_hash();
    assert_eq!(state0_healed, state1_healed);
    assert_eq!(state1_healed, state2_healed);

    Ok(())
}
```

### What This Tests

✅ **Real consensus logic** - uses actual GHOSTDAG, validation, etc.
✅ **Real state transitions** - uses actual blockchain state machine
✅ **Real transaction processing** - uses actual mempool and validation
✅ **Real sync protocol** - uses actual synchronization logic
❌ **NOT testing** - process spawning, HTTP parsing, file I/O (these are implementation details)

### Benefits Over Pure Mock

| Aspect | Pure Mock | Component Test | Real Daemon |
|--------|-----------|----------------|-------------|
| **Tests real logic** | ❌ No | ✅ Yes | ✅ Yes |
| **Test speed** | ⚡ <1ms | ⚡ <100ms | 🐌 30s+ |
| **Catches bugs** | ❌ Only mock bugs | ✅ Logic bugs | ✅ All bugs |
| **Deterministic** | ✅ Yes | ✅ Yes | ❌ No |
| **Easy debug** | ✅ Yes | ✅ Yes | ❌ No |
| **Resource usage** | Low | Low | High |

### Implementation Strategy

#### Step 1: Extract daemon components for in-process use

Current daemon architecture:
```rust
// daemon/src/main.rs
fn main() {
    let blockchain = Blockchain::new(...);  // ← We want this!
    let consensus = ConsensusEngine::new(...);  // ← And this!
    let mempool = Mempool::new(...);  // ← And this!

    // We DON'T need these for tests:
    let rpc_server = RpcServer::new(...);
    let p2p_server = P2pServer::new(...);
}
```

Refactor to allow in-process usage:
```rust
// daemon/src/lib.rs (NEW)
pub struct DaemonCore {
    pub blockchain: Arc<RwLock<Blockchain>>,
    pub consensus: Arc<ConsensusEngine>,
    pub mempool: Arc<Mempool>,
}

impl DaemonCore {
    pub fn new(storage: Box<dyn Storage>) -> Result<Self> {
        // Initialize core components without I/O
        // ...
    }
}

// daemon/src/main.rs
fn main() {
    let core = DaemonCore::new(Box::new(RocksDbStorage::new(...)))?;

    // Wrap with I/O layers
    let rpc_server = RpcServer::new(core.clone());
    let p2p_server = P2pServer::new(core.clone());
    // ...
}
```

#### Step 2: Create in-memory storage for tests

```rust
// testing/integration/src/test_storage.rs
pub struct InMemoryStorage {
    blocks: Arc<RwLock<HashMap<Hash, Block>>>,
    // ... other data structures
}

impl Storage for InMemoryStorage {
    // Implement storage trait using in-memory structures
    // Much faster than RocksDB for tests!
}
```

#### Step 3: Create mock network layer

```rust
// testing/integration/src/mock_network.rs
pub struct MockNetwork {
    nodes: HashMap<usize, Weak<TestNode>>,
    partitions: Vec<Vec<usize>>,
}

impl MockNetwork {
    pub async fn propagate_transaction(&self, from: usize, tx: Transaction) {
        // Send to connected nodes (respecting partitions)
        for (to, node) in &self.nodes {
            if self.can_communicate(from, *to) {
                node.receive_transaction(tx.clone()).await;
            }
        }
    }

    pub fn partition(&mut self, group1: Vec<usize>, group2: Vec<usize>) {
        self.partitions = vec![group1, group2];
    }

    fn can_communicate(&self, from: usize, to: usize) -> bool {
        // Check if both nodes are in same partition group
        // ...
    }
}
```

### Migration Path

1. **Don't start with full mock** - that's the wrong direction
2. **Don't keep failing real daemon tests** - they're too slow/flaky
3. **Instead: Refactor daemon to expose core components** (1-2 days)
4. **Build component test infrastructure** (1-2 days)
5. **Write meaningful component tests** (1-2 days)
6. **Keep 1-2 real daemon tests** for end-to-end validation (run nightly)

### Realistic Test Example

Here's what a GOOD component test looks like:

```rust
#[tokio::test]
async fn test_double_spend_rejection() -> Result<()> {
    let node = TestNode::new(0).await?;

    // Create transaction spending 100 TOS
    let tx1 = Transaction {
        sender: alice_address(),
        receiver: bob_address(),
        amount: 100,
        // ... (uses REAL transaction structure)
    };

    // Submit to mempool (REAL validation!)
    node.submit_transaction(tx1.clone()).await?;

    // Create conflicting transaction (double spend)
    let tx2 = Transaction {
        sender: alice_address(),  // Same sender
        receiver: charlie_address(),
        amount: 100,  // Spending same funds
        // ...
    };

    // Should be rejected by REAL mempool logic
    let result = node.submit_transaction(tx2).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), Error::DoubleSpend));

    Ok(())
}
```

This test:
- ✅ Uses REAL mempool validation logic
- ✅ Tests actual double-spend detection
- ✅ Runs in <10ms
- ✅ Is deterministic
- ✅ Catches real bugs

## Recommendation

**Don't implement full mocks. Instead:**

1. Refactor `tos_daemon` to expose `DaemonCore` as a library
2. Use real blockchain/consensus logic in tests
3. Mock only network and RPC I/O layers
4. Write meaningful tests that verify actual behavior

This gives you:
- 🚀 **90% speed improvement** (100ms vs 30s)
- ✅ **Real bug detection** (tests actual logic)
- 🎯 **Meaningful coverage** (tests what matters)
- 🔧 **Easy debugging** (single process)

Would you like me to start with refactoring the daemon to expose core components for testing?
