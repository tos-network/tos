# Mock Daemon Design for Integration Tests

## Problem Statement

Current integration tests suffer from instability due to:
1. **Slow startup time**: Daemon process takes 10-30 seconds to initialize
2. **Timeout issues**: Tests frequently timeout waiting for nodes to be ready
3. **Resource intensive**: Running multiple real daemon processes consumes significant CPU/memory
4. **Flaky tests**: Network timing issues cause intermittent failures
5. **Hard to debug**: Real daemon logs are scattered across multiple processes

## Proposed Solution: In-Process Mock Daemon

Replace real daemon processes with in-process mock nodes that simulate the same RPC/P2P behavior.

### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Integration Test                          │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │  MockNode 1  │  │  MockNode 2  │  │  MockNode 3  │      │
│  ├──────────────┤  ├──────────────┤  ├──────────────┤      │
│  │ Mock RPC     │  │ Mock RPC     │  │ Mock RPC     │      │
│  │ Mock P2P     │  │ Mock P2P     │  │ Mock P2P     │      │
│  │ Mock State   │  │ Mock State   │  │ Mock State   │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
│         │                 │                 │                │
│         └─────────────────┴─────────────────┘                │
│                  In-Memory Message Bus                       │
└─────────────────────────────────────────────────────────────┘
```

### Key Components

#### 1. MockNode

```rust
pub struct MockNode {
    /// Node ID
    node_id: usize,

    /// Mock blockchain state
    state: Arc<RwLock<MockChainState>>,

    /// Mock RPC server (in-memory HTTP server)
    rpc_server: MockRpcServer,

    /// Mock P2P connections
    p2p_peers: Arc<RwLock<HashMap<usize, MockP2pConnection>>>,

    /// Network simulator for latency/packet loss
    network_sim: NetworkSimulator,

    /// Configuration
    config: NodeConfig,
}

impl MockNode {
    /// Start the mock node (instant, no process spawn)
    pub async fn start(&mut self) -> Result<()> {
        // Initialize mock state
        self.state.write().await.initialize();

        // Start in-memory RPC server
        self.rpc_server.start().await?;

        // Ready immediately (no 30-second wait!)
        Ok(())
    }

    /// Handle RPC request
    pub async fn handle_rpc(&self, method: &str, params: Value) -> Result<Value> {
        match method {
            "get_info" => self.get_info().await,
            "get_block" => self.get_block(params).await,
            "submit_transaction" => self.submit_transaction(params).await,
            _ => Err(anyhow!("Unknown RPC method: {}", method))
        }
    }

    /// Simulate block propagation
    pub async fn propagate_block(&self, block: Block) {
        // Apply network delay
        self.network_sim.apply_delay().await;

        // Broadcast to peers
        for (peer_id, conn) in self.p2p_peers.read().await.iter() {
            conn.send_block(block.clone()).await;
        }
    }
}
```

#### 2. MockChainState

```rust
pub struct MockChainState {
    /// Current block height
    height: u64,

    /// Blocks by hash
    blocks: HashMap<Hash, Block>,

    /// Account balances
    balances: HashMap<Address, u64>,

    /// Pending transactions
    mempool: Vec<Transaction>,

    /// Network partition state
    is_partitioned: bool,

    /// Partition group (for testing network splits)
    partition_group: Option<usize>,
}

impl MockChainState {
    /// Mine a new block (instant, no actual mining)
    pub fn mine_block(&mut self, txs: Vec<Transaction>) -> Block {
        let block = Block {
            height: self.height + 1,
            timestamp: now(),
            transactions: txs,
            // ... simplified block structure
        };

        self.blocks.insert(block.hash(), block.clone());
        self.height += 1;

        block
    }

    /// Apply transaction to state
    pub fn apply_transaction(&mut self, tx: &Transaction) -> Result<()> {
        // Simplified state transition
        // No actual cryptographic verification (too slow for tests)

        // Just check basic validity
        if tx.amount > self.balances.get(&tx.sender).copied().unwrap_or(0) {
            return Err(anyhow!("Insufficient balance"));
        }

        // Update balances
        *self.balances.entry(tx.sender).or_insert(0) -= tx.amount;
        *self.balances.entry(tx.receiver).or_insert(0) += tx.amount;

        Ok(())
    }
}
```

#### 3. MockRpcServer (In-Memory HTTP)

```rust
pub struct MockRpcServer {
    /// Node reference
    node: Weak<MockNode>,

    /// In-memory request handler
    handler: Arc<dyn Fn(String, Value) -> BoxFuture<'static, Result<Value>>>,
}

impl MockRpcServer {
    pub async fn start(&mut self) -> Result<()> {
        // No actual HTTP server needed!
        // Tests call handle_request() directly
        Ok(())
    }

    pub async fn handle_request(&self, method: String, params: Value) -> Result<Value> {
        (self.handler)(method, params).await
    }
}
```

#### 4. TestScenario with Mocks

```rust
#[async_trait::async_trait]
impl TestScenario for NetworkPartitionRecovery {
    async fn run(&self, harness: &mut MultiNodeHarness) -> Result<MetricsReport> {
        let mut collector = MetricsCollector::new();

        // Create mock nodes (instant!)
        harness.spawn_all_mocked().await?;
        // No wait_for_ready() needed - mocks are ready immediately

        // Connect nodes
        harness.connect_full_mesh().await?;

        // Phase 1: Normal operation
        for i in 0..100 {
            let tx = create_test_transaction(i);
            harness.node(0).submit_transaction(tx).await?;

            // Mock propagates instantly (or with simulated delay)
            sleep(Duration::from_millis(10)).await;
        }

        // Phase 2: Simulate network partition
        harness.partition_network(vec![0], vec![1, 2]).await?;

        // Nodes 0 is isolated from nodes 1,2
        // They continue operating independently
        sleep(Duration::from_secs(self.partition_duration_secs)).await;

        // Phase 3: Heal partition
        harness.heal_partition().await?;

        // Nodes should re-sync (mock state reconciliation)
        sleep(Duration::from_secs(5)).await;

        // Verify all nodes converged to same state
        let state0 = harness.node(0).get_state().await?;
        let state1 = harness.node(1).get_state().await?;
        let state2 = harness.node(2).get_state().await?;

        assert_eq!(state0.height, state1.height);
        assert_eq!(state1.height, state2.height);

        Ok(collector.generate_report())
    }
}
```

### Benefits

| Aspect | Real Daemon | Mock Daemon |
|--------|-------------|-------------|
| **Startup time** | 10-30 seconds | < 100ms |
| **Memory usage** | ~100MB per node | ~10MB per node |
| **CPU usage** | High (consensus, mining) | Low (state simulation) |
| **Determinism** | Non-deterministic timing | Fully deterministic |
| **Debuggability** | Scattered logs | Single process, easy debugging |
| **Test isolation** | Requires cleanup | Perfect isolation |
| **Network simulation** | Limited control | Full control over latency/loss |

### Implementation Plan

#### Phase 1: Core Mock Infrastructure (1-2 days)
- [ ] Create `MockNode` struct
- [ ] Implement `MockChainState` with basic operations
- [ ] Add in-memory RPC handler
- [ ] Write unit tests for mock components

#### Phase 2: Test Harness Integration (1 day)
- [ ] Add `spawn_all_mocked()` to `MultiNodeHarness`
- [ ] Implement mock P2P message passing
- [ ] Add network partition simulation
- [ ] Update `NetworkSimulator` to work with mocks

#### Phase 3: Scenario Migration (1-2 days)
- [ ] Migrate `NetworkPartitionRecovery` to use mocks
- [ ] Migrate `BasicConsensusTPS` to use mocks
- [ ] Add new mock-specific scenarios (edge cases)
- [ ] Keep real daemon tests for end-to-end validation

#### Phase 4: Advanced Features (optional)
- [ ] Mock mining with adjustable difficulty
- [ ] Mock transaction validation (basic checks only)
- [ ] Mock consensus (GHOSTDAG simulation)
- [ ] State snapshot/restore for test replay

### Testing Strategy

Keep both real and mock tests:

```
tests/
├── mock/                    # Fast, stable mock tests
│   ├── network_partition.rs
│   ├── consensus_tps.rs
│   └── byzantine_faults.rs
└── real/                    # Slow, end-to-end real tests
    ├── network_partition_e2e.rs.disabled
    └── multi_node_tps.rs.disabled
```

Mock tests run on every commit (fast CI).
Real tests run nightly or on-demand (integration validation).

### Example: Network Partition Test Comparison

**Before (Real Daemon):**
```rust
// Total time: ~90 seconds
// - 30s startup
// - 30s partition
// - 30s recovery
```

**After (Mock Daemon):**
```rust
// Total time: ~1 second
// - 0.1s startup
// - 0.5s partition (simulated)
// - 0.4s recovery (simulated)
```

**90x faster!** 🚀

### Migration Path

1. **Start with one test**: Migrate `network_partition.rs` to use mocks
2. **Measure improvement**: Compare test time and stability
3. **Iterate**: Add more mock features as needed
4. **Gradually migrate**: Move other tests to mocks one by one
5. **Keep real tests**: Maintain a few end-to-end tests with real daemons

### Trade-offs

**Pros:**
- ✅ 90x faster test execution
- ✅ No timeout issues
- ✅ Perfect determinism
- ✅ Easy debugging
- ✅ Test more edge cases

**Cons:**
- ❌ Not testing real daemon code paths
- ❌ May miss real-world issues (race conditions, etc.)
- ❌ Need to maintain mock implementation

**Mitigation:**
- Keep some real daemon tests for end-to-end validation
- Run real tests nightly or before releases
- Use mocks for rapid development, real tests for confidence

## Next Steps

Would you like me to:
1. Implement the basic `MockNode` infrastructure?
2. Migrate the `network_partition.rs` test to use mocks as a proof-of-concept?
3. Create a separate `mock_daemon` module in the integration test crate?

Let me know which approach you prefer!
