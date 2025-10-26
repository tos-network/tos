# TOS Multi-Node Integration Testing Framework

A comprehensive framework for testing TOS blockchain behavior across multiple nodes with realistic network conditions.

## Overview

This framework provides:

- **Network Simulation**: Realistic network conditions (latency, jitter, packet loss, bandwidth limits)
- **Multi-Node Testing**: Spawn and manage multiple daemon processes
- **Metrics Collection**: Track consensus TPS, propagation times, and network overhead
- **Test Scenarios**: Pre-built scenarios for common testing patterns

## Architecture

```
testing/integration/
├── src/
│   ├── lib.rs                  # Main library exports
│   ├── network_simulator.rs    # Network condition simulation
│   ├── node_harness.rs         # Node process management
│   ├── metrics.rs              # Metrics collection and reporting
│   ├── scenarios.rs            # Pre-built test scenarios
│   └── utils.rs                # Helper utilities
├── tests/
│   ├── multi_node_tps.rs       # Basic consensus TPS test
│   ├── network_partition.rs    # Partition recovery test
│   └── variable_network.rs     # Variable network conditions test
├── Cargo.toml
└── README.md
```

## Quick Start

### Prerequisites

1. Build the TOS daemon:
   ```bash
   cargo build --package tos_daemon
   ```

2. Set the daemon path (optional):
   ```bash
   export TOS_DAEMON_PATH=./target/debug/tos_daemon
   ```

### Running Tests

Run all integration tests:
```bash
cargo test --package tos_integration_tests
```

Run specific test:
```bash
cargo test --package tos_integration_tests --test multi_node_tps
cargo test --package tos_integration_tests --test network_partition
cargo test --package tos_integration_tests --test variable_network
```

## Network Configurations

The framework provides several pre-configured network profiles:

### Perfect Network (Testing Only)
```rust
NetworkConfig::perfect()
```
- Latency: 0ms
- Jitter: 0ms
- Packet loss: 0%
- Bandwidth: Unlimited

### LAN (Fast Local Network)
```rust
NetworkConfig::lan()
```
- Latency: 10ms
- Jitter: 5ms
- Packet loss: 0.1%
- Bandwidth: 1 Gbps

### Internet (Medium Quality)
```rust
NetworkConfig::internet()
```
- Latency: 100ms
- Jitter: 50ms
- Packet loss: 1%
- Bandwidth: 100 Mbps

### Poor Network (Congested/Mobile)
```rust
NetworkConfig::poor()
```
- Latency: 500ms
- Jitter: 200ms
- Packet loss: 5%
- Bandwidth: 10 Mbps

### Production-like (Realistic)
```rust
NETWORK_CONFIG_PRODUCTION
```
- Latency: 200ms
- Jitter: 100ms
- Packet loss: 2%
- Bandwidth: 50 Mbps

## Test Scenarios

### 1. Basic Consensus TPS

Measures transactions per second across multiple nodes under specified network conditions.

```rust
use tos_integration::*;

#[tokio::test]
async fn test_basic_tps() {
    let scenario = scenarios::BasicConsensusTPS::new(1000, 60);
    let runner = ScenarioRunner::new("./target/debug/tos_daemon".to_string());

    let report = runner
        .run_scenario(&scenario, 3, NETWORK_CONFIG_PRODUCTION)
        .await
        .unwrap();

    report.print_summary();
}
```

**Metrics tracked:**
- Consensus TPS (transactions confirmed by all nodes)
- Average confirmation time
- Transaction propagation latency
- Network overhead

### 2. Network Partition Recovery

Tests how nodes recover after network partition and measure sync time.

```rust
let scenario = scenarios::NetworkPartitionRecovery::new(30, 60);
```

**Metrics tracked:**
- Partition duration
- Recovery time
- Chain reorganizations
- Synchronization speed

### 3. Variable Network Conditions

Tests TPS under varying network quality (good → medium → poor).

```rust
let scenario = scenarios::VariableNetworkConditions::new(30, 500);
```

**Metrics tracked:**
- TPS under each network condition
- Propagation time variance
- Consensus stability

### 4. High Load Stress Test

Stress tests the network with high transaction volume.

```rust
let scenario = scenarios::HighLoadStressTest::new(1000, 60);
```

**Metrics tracked:**
- Maximum sustained TPS
- Transaction drop rate
- Memory usage
- Network bandwidth usage

## Custom Test Scenarios

Create custom test scenarios by implementing the `TestScenario` trait:

```rust
use tos_integration::*;

pub struct CustomScenario {
    // Configuration fields
}

#[async_trait::async_trait]
impl TestScenario for CustomScenario {
    fn name(&self) -> &str {
        "Custom Test Scenario"
    }

    async fn run(&self, harness: &mut MultiNodeHarness) -> Result<MetricsReport> {
        let mut collector = MetricsCollector::new();

        // Spawn and configure nodes
        harness.spawn_all().await?;
        harness.wait_for_all_ready(30).await?;

        // Run your test logic
        // ...

        // Build and return report
        Ok(collector.build_report(self.name().to_string(), harness.num_nodes()))
    }
}
```

## Metrics Reference

### Network Metrics

- `avg_tx_propagation_ms`: Average time for transaction to reach all nodes
- `tx_propagation_p50/p90/p99_ms`: Percentiles for transaction propagation
- `avg_block_propagation_ms`: Average time for block to reach all nodes
- `block_propagation_p50/p90/p99_ms`: Percentiles for block propagation
- `total_bytes_sent/received`: Network bandwidth usage
- `network_overhead_percent`: Overhead ratio (actual vs minimum required bandwidth)
- `packets_lost`: Number of packets dropped by network simulation
- `avg_rtt_ms`: Average round-trip time

### Consensus Metrics

- `consensus_tps`: Transactions confirmed by all nodes per second
- `transactions_submitted`: Total transactions submitted
- `transactions_confirmed`: Transactions confirmed by all nodes
- `avg_confirmation_time_ms`: Time from submission to consensus
- `blocks_produced`: Total blocks produced during test
- `avg_block_time_sec`: Average time between blocks
- `orphaned_blocks`: Blocks that were orphaned
- `orphan_rate_percent`: Percentage of orphaned blocks
- `chain_reorgs`: Number of chain reorganizations
- `avg_blue_score`: Average blue score across nodes
- `blue_score_stddev`: Standard deviation of blue scores (consensus health)

## Advanced Usage

### Manual Node Management

```rust
use tos_integration::*;

#[tokio::test]
async fn test_manual_nodes() {
    let network_config = NetworkConfig::internet();
    let mut harness = MultiNodeHarness::new(3, network_config).await.unwrap();

    // Spawn nodes
    harness.spawn_all().await.unwrap();
    harness.wait_for_all_ready(30).await.unwrap();

    // Access individual nodes
    let node0 = harness.node(0).unwrap();
    let node1 = harness.node(1).unwrap();

    // Make RPC calls
    // let info: DaemonInfo = node0.rpc_call("/api/daemon/get_info").await.unwrap();

    // Stop specific node
    harness.node_mut(0).unwrap().stop().await.unwrap();

    // Cleanup
    harness.stop_all().await.unwrap();
    harness.cleanup_all().unwrap();
}
```

### Network Partition Simulation

```rust
use tos_integration::*;

// Isolate node 0 from the rest
let partition = NetworkPartition::isolate_node(3, 0);

// 50-50 split
let partition = NetworkPartition::split_half(4);

// Check if nodes can communicate
assert!(!partition.can_communicate(0, 1));
```

### Custom Network Simulator

```rust
let config = NetworkConfig {
    base_latency_ms: 150,
    jitter_ms: 75,
    packet_loss_rate: 0.03,
    bandwidth_mbps: Some(25),
};

let sim = NetworkSimulator::new(config).unwrap();

// Simulate packet transmission
let success = sim.transmit(1024).await; // 1KB payload

// Calculate transmission delay
let delay_ms = sim.transmission_delay_ms(1024);
```

## Metrics Export

Export test results to JSON or CSV:

```rust
report.export_json("test_results.json")?;
report.export_csv("test_results.csv")?;
```

JSON format includes full metrics with nested structure:
```json
{
  "test_name": "Basic Consensus TPS",
  "duration_secs": 60.0,
  "num_nodes": 3,
  "network": { ... },
  "consensus": { ... },
  "start_time": "2025-10-26T...",
  "end_time": "2025-10-26T..."
}
```

CSV format includes flattened key metrics suitable for spreadsheet analysis.

## Environment Variables

- `TOS_DAEMON_PATH`: Path to daemon binary (default: `./target/debug/tos_daemon`)
- `RUST_LOG`: Log level (e.g., `RUST_LOG=info,tos_integration=debug`)

## Troubleshooting

### Nodes fail to start

1. Check daemon binary exists: `ls -la ./target/debug/tos_daemon`
2. Check permissions: `chmod +x ./target/debug/tos_daemon`
3. Check logs in node data directories: `/tmp/tos_test_node_*/stdout.log`

### Tests timeout

1. Increase timeout: `harness.wait_for_all_ready(60).await`
2. Check network simulation is not too aggressive (high packet loss)
3. Verify system has enough resources (CPU, memory, disk I/O)

### Port conflicts

Tests use ports starting from:
- RPC: 8080, 8081, 8082, ...
- P2P: 2125, 2126, 2127, ...

Ensure these ports are available or configure custom ports:

```rust
let config = NodeConfig {
    node_id: 0,
    rpc_bind_address: "127.0.0.1:9080".to_string(),
    p2p_bind_address: "0.0.0.0:3125".to_string(),
    // ...
};
```

## Implementation Status

### ✅ Implemented

- Network simulation (latency, jitter, packet loss, bandwidth)
- Node process spawning and lifecycle management
- Metrics collection framework
- Test scenario infrastructure
- Network partition modeling
- Metrics export (JSON, CSV)

### ⚠️  Simplified (Placeholder)

The current implementation provides a **foundation** for multi-node testing but includes simplified/mocked components:

1. **Transaction Submission**: Uses simulated transactions instead of actual RPC calls
2. **Consensus Verification**: Assumes confirmation based on timing rather than querying actual chain state
3. **Network Partition**: Logical partition model (not integrated with actual network traffic blocking)
4. **Metrics Collection**: Based on simulated events rather than actual daemon metrics

### 🚧 Requires TOS Infrastructure

For full multi-node testing, the following TOS daemon features are needed:

1. **RPC APIs**:
   - Submit transaction endpoint
   - Query transaction status by ID
   - Get block by hash/height
   - Get node peer list
   - Get mempool statistics

2. **P2P Observability**:
   - Peer connection events
   - Message propagation tracking
   - Bandwidth statistics

3. **Consensus Metrics**:
   - Real-time blue score reporting
   - Chain tip tracking per node
   - Orphan block detection

## Roadmap

See `/Users/tomisetsu/tos-network/tos/memo/P2_MULTI_NODE_FRAMEWORK.md` for detailed implementation roadmap and requirements.

### Phase 1 (Current)
- ✅ Network simulator
- ✅ Node harness infrastructure
- ✅ Metrics framework
- ✅ Test scenarios (simplified)

### Phase 2 (Next)
- 🚧 Integrate with actual TOS RPC APIs
- 🚧 Real transaction submission and tracking
- 🚧 Consensus state verification across nodes

### Phase 3 (Future)
- 🚧 Network traffic interception for true partition simulation
- 🚧 Real-time metrics streaming from daemons
- 🚧 Distributed tracing for transaction flow
- 🚧 Automated performance regression testing

## Contributing

When adding new test scenarios:

1. Implement the `TestScenario` trait
2. Add comprehensive metrics collection
3. Document expected behavior and pass criteria
4. Add integration test in `tests/` directory
5. Update this README with usage examples

## License

BSD-3-Clause

## References

- TOS Consensus Design: `../../TIPs/CONSENSUS_LAYERED_DESIGN.md`
- TOS Network Protocol: `../../docs/NETWORK_PROTOCOL.md`
- Performance Benchmarks: `../../daemon/benches/README.md`
