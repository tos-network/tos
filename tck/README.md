# TOS-TCK (Technology Compatibility Kit)

A comprehensive testing framework for the TOS blockchain, inspired by Java's TCK
and Solana's testing infrastructure. TOS-TCK provides conformance testing, fuzzing,
formal verification, and multi-node cluster testing in a unified, layered architecture.

---

## Testing Pyramid

```
                    +-------------------+
                    |      Tier 4       |  Chaos & Property Tests
                    |   (Weekly CI)     |  proptest, Byzantine fault
                    +-------------------+
                    |      Tier 3       |  LocalCluster Multi-Node
                    |   (Nightly CI)    |  Sync, Partition, Convergence
                +---+-------------------+---+
                |         Tier 2            |  Integration Tests
                |       (Per-PR CI)         |  TestDaemon + RPC + RocksDB
            +---+---------------------------+---+
            |            Tier 1.5               |  ChainClient / ContractTest
            |          (Per-PR CI)              |  Direct chain, no network
        +---+-----------------------------------+---+
        |                Tier 1                     |  Component Tests
        |             (Per-Commit)                  |  TestBlockchain (in-memory)
    +---+-------------------------------------------+---+
    |                    Tier 0                          |  Unit Tests
    |                 (Continuous)                       |  Pure functions
    +---------------------------------------------------+
```

---

## Directory Structure

```
tck/
├── src/
│   ├── lib.rs
│   ├── prelude.rs                # Common re-exports for test authors
│   │
│   ├── orchestrator/             # Deterministic test infrastructure
│   │   ├── clock.rs              # MockClock / SystemClock
│   │   └── rng.rs                # Seeded RNG for reproducibility
│   │
│   ├── tier1_component/          # Tier 1: In-memory blockchain simulation
│   │   ├── blockchain.rs         # TestBlockchain
│   │   └── builder.rs            # TestBlockchainBuilder
│   ├── tier1_component_dag/      # Tier 1 DAG: Multi-tip DAG test chain
│   │   ├── blockchain.rs         # TestBlockchainDag
│   │   └── builder.rs            # TestBlockchainDagBuilder
│   │
│   ├── tier1_5/                  # Tier 1.5: Direct chain access (no network)
│   │   ├── chain_client.rs       # ChainClient (fast TX processing)
│   │   ├── contract_test.rs      # ContractTest (TAKO contract harness)
│   │   ├── tx_result.rs          # TxResult, TransactionError
│   │   ├── block_warp.rs         # BlockWarp (topoheight advancement)
│   │   ├── confirmation.rs       # ConfirmationDepth
│   │   └── features.rs           # FeatureSet, FeatureRegistry
│   │
│   ├── tier2_integration/        # Tier 2: Single daemon + RPC
│   │   ├── test_daemon.rs        # TestDaemon
│   │   ├── rpc_helpers.rs        # RPC abstraction
│   │   └── waiters.rs            # Async coordination
│   ├── tier2_integration_dag/    # Tier 2 DAG: DAG daemon wrapper
│   │   └── test_daemon.rs        # TestDaemonDag
│   │
│   ├── tier3_e2e/                # Tier 3: Multi-node cluster
│   │   ├── cluster.rs            # LocalCluster
│   │   ├── cluster_config.rs     # ClusterConfig, NodeConfig
│   │   ├── partition.rs          # PartitionController
│   │   ├── verification.rs       # Cross-node state verification
│   │   └── waiters.rs            # Poll/wait utilities
│   ├── tier3_e2e_dag/            # Tier 3 DAG: Multi-node DAG network
│   │   └── network.rs            # LocalTosNetworkDag
│   │
│   ├── tier4_chaos/              # Tier 4: Chaos & property-based
│   │   └── property_tests.rs     # proptest strategies
│   │
│   ├── conformance/              # Spec-driven conformance tests
│   ├── fixtures/                 # Declarative TX fixture framework
│   ├── blockdag/                 # BlockDAG consensus tests
│   ├── transaction/              # TX processing / mempool tests
│   ├── p2p/                      # P2P protocol tests
│   ├── sync/                     # Data sync & reorg tests
│   ├── fuzz/                     # Fuzzing infrastructure
│   ├── formal/                   # Formal verification (Kani)
│   ├── invariants/               # Blockchain invariant checkers
│   ├── scenarios/                # YAML scenario DSL
│   └── utilities/                # Shared helpers
│
├── tests/                        # Integration test binaries
├── fixtures/                     # YAML fixture files
│   ├── scenarios/
│   ├── regression/
│   ├── stress/
│   └── templates/
├── fuzz/                         # cargo-fuzz targets
│   └── fuzz_targets/
├── specs/                        # Critical Path Specifications
│   ├── wire-format.md            # Binary serialization rules
│   ├── hash-algorithms.md        # Hash function assignments
│   ├── blockdag-ordering.md      # DAG execution order
│   ├── failed-tx-semantics.md    # Failure handling
│   ├── nonce-rules.md            # Nonce validation
│   ├── state-digest.md           # Canonical state format
│   ├── error-codes.md            # Standardized errors
│   ├── syscalls/                 # Syscall specs (YAML)
│   ├── consensus/                # Consensus specs (YAML)
│   └── api/                      # API specs (YAML)
├── vectors/                      # Test Vectors (YAML)
│   ├── crypto/                   # Cryptographic vectors
│   ├── wire/                     # Wire format vectors
│   ├── state/                    # State transition vectors
│   ├── execution/                # Block execution vectors
│   └── errors/                   # Error scenario vectors
├── conformance/                  # Multi-Client Conformance Testing
│   ├── docker-compose.yml        # Multi-client orchestration
│   ├── harness/                  # Python test driver
│   └── api/                      # Conformance API spec
├── benches/                      # Performance benchmarks
└── crypto/                       # Vector generators + legacy vectors
```

---

## Multi-Client Alignment

The TCK includes infrastructure for ensuring alignment between multiple TOS implementations (TOS Rust, Avatar C, and future clients).

### Three-Layer Framework

```
+-----------------------------------------------------------+
|             Layer 1: Critical Path Specifications          |
|                    (tck/specs/*.md)                        |
|  Wire Format | Hashing | BlockDAG Order | Error Codes     |
+-----------------------------------------------------------+
                              |
                              v
+-----------------------------------------------------------+
|             Layer 2: Test Vector Infrastructure            |
|                    (tck/vectors/)                          |
|  Crypto Vectors | State Vectors | Execution Vectors       |
+-----------------------------------------------------------+
                              |
                              v
+-----------------------------------------------------------+
|             Layer 3: Differential Testing                  |
|                    (tck/conformance/)                      |
|  Docker Harness | Result Comparison | Fuzzing             |
+-----------------------------------------------------------+
```

### Quick Start

```bash
# Run conformance tests (requires Docker)
cd tck/conformance
docker-compose up

# Generate test vectors (Rust generators moved)
cd /Users/tomisetsu/tos-spec/rust_generators/crypto
cargo run --release --bin gen_sha256_vectors

# Run fuzz tests
cd tck/fuzz
cargo fuzz run fuzz_transaction
```

### Key Documentation

| Document | Purpose |
|----------|---------|
| `MULTI_CLIENT_ALIGNMENT.md` | Methodology overview |
| `MULTI_CLIENT_ALIGNMENT_SCHEME.md` | Technical specifications |
| `tck/specs/*.md` | Critical path specifications |
| `tck/vectors/README.md` | Test vector guide |
| `tck/conformance/README.md` | Conformance testing guide |
| `tck/fuzz/README.md` | Fuzzing guide |

---

## Tier Selection Guide

Use this decision tree to determine which tier to add tests in:

```
Is the code under test a pure function or struct with no I/O?
  └─ YES → Tier 0 (unit test in the crate itself)
  └─ NO ↓

Does it require blockchain state (balances, nonces, blocks)?
  └─ NO → Tier 0 or domain-specific module (src/blockdag/, src/transaction/, etc.)
  └─ YES ↓

Does it require network communication or RPC?
  └─ NO ↓
  │   Does it involve smart contract execution?
  │     └─ YES → Tier 1.5 (ContractTest)
  │     └─ NO  → Tier 1.5 (ChainClient) or Tier 1 (TestBlockchain)
  └─ YES ↓

Does it require multiple nodes or partition testing?
  └─ NO  → Tier 2 (TestDaemon)
  └─ YES → Tier 3 (LocalCluster)

Is it a property-based or long-running randomized test?
  └─ YES → Tier 4 (proptest / chaos)
```

### Tier Comparison Table

| Criterion | Tier 1 | Tier 1.5 | Tier 2 | Tier 3 | Tier 4 |
|-----------|--------|----------|--------|--------|--------|
| Speed | < 100ms | < 50ms | 1-5s | 5-60s | minutes |
| Network | No | No | Localhost | Multi-node | Multi-node |
| Storage | In-memory | In-memory | RocksDB | RocksDB | RocksDB |
| RPC | No | No | Yes | Yes | Yes |
| Consensus | Simulated | Simulated | Real | Real | Real |
| Deterministic | Yes | Yes | Mostly | Mostly | Seeded |
| Use for | Logic | TX/Contract | RPC/API | Sync/P2P | Invariants |

---

## When to Use Each Tier

### Tier 1: TestBlockchain (Component Tests)

**Use when testing:**
- Balance transfer logic
- Nonce management
- Fee calculation correctness
- DAG ordering algorithms
- Block validation rules (stateless)

```rust
use tos_tck::tier1_component::TestBlockchain;

#[tokio::test]
async fn balance_decreases_after_transfer() {
    let mut chain = TestBlockchain::new();
    let alice = chain.create_account(10_000);
    let bob = chain.create_account(0);
    chain.transfer(alice, bob, 1_000).unwrap();
    chain.mine_block();
    assert_eq!(chain.get_balance(alice), 8_990); // 10000 - 1000 - 10(fee)
}
```

### Tier 1.5: ChainClient / ContractTest

**Use when testing:**
- Transaction lifecycle (submit → mine → verify state)
- Smart contract deployment and execution
- Contract events and return data
- Nonce ordering within/across blocks
- Feature gate activation boundaries
- Gas accounting
- BlockWarp (time/height advancement)

```rust
use tos_tck::tier1_5::{ChainClient, ChainClientConfig, GenesisAccount};

#[tokio::test]
async fn transfer_updates_balance_and_nonce() {
    let mut client = ChainClient::start(ChainClientConfig {
        genesis_accounts: vec![
            GenesisAccount::new(alice_addr, 10_000),
            GenesisAccount::new(bob_addr, 0),
        ],
        ..Default::default()
    }).await.unwrap();

    let tx = client.build_transfer(&alice_kp, &bob_addr, 1_000).unwrap();
    let result = client.process_transaction(tx).await.unwrap();
    assert!(result.is_success());
    assert_eq!(client.get_balance(&bob_addr).await.unwrap(), 1_000);
}
```

**ContractTest (TAKO smart contract testing):**

```rust
use tos_tck::tier1_5::{ContractTest, ContractTestContext};

#[tokio::test]
async fn contract_emits_event_on_transfer() {
    let ctx = ContractTest::new("token", &bytecode)
        .add_account(alice, 10_000)
        .set_max_gas(1_000_000)
        .start().await;

    ctx.call(TRANSFER_ENTRY, encode_args(&bob, 500)).await.unwrap();
    assert_eq!(ctx.last_events().len(), 1);
}
```

### Tier 2: TestDaemon (Integration Tests)

**Use when testing:**
- RPC API correctness (JSON-RPC, WebSocket)
- Storage persistence and recovery
- Full transaction validation pipeline via RPC
- Rate limiting and security features

```rust
use tos_tck::tier2_integration::TestDaemon;

#[tokio::test]
async fn rpc_get_balance_returns_correct_value() {
    let daemon = TestDaemon::start().await.unwrap();
    let balance = daemon.rpc_client.get_balance(&addr).await.unwrap();
    assert_eq!(balance, expected);
}
```

### Tier 3: LocalCluster (E2E Multi-Node Tests)

**Use when testing:**
- Block/transaction propagation across nodes
- Network partition and recovery (convergence)
- Bootstrap sync (late-joining node)
- Cross-node balance/state consistency
- Node crash and restart resilience
- DAG reorg behavior under partition

```rust
use tos_tck::tier3_e2e::{LocalCluster, ClusterConfig};

#[tokio::test]
async fn late_join_node_syncs_to_cluster() {
    let mut cluster = LocalCluster::start(ClusterConfig::default_3_nodes()).await.unwrap();
    cluster.mine_blocks(50).await.unwrap();
    let new_node = cluster.add_node(Default::default()).await.unwrap();
    cluster.wait_all_topoheight(50, Duration::from_secs(30)).await.unwrap();
}
```

### Tier 4: Chaos & Property Tests

**Use when testing:**
- System invariants under random input sequences
- Parallel vs sequential execution equivalence
- Balance conservation across arbitrary operations
- Byzantine fault tolerance

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn balance_is_always_conserved(
        ops in prop::collection::vec(arb_operation(), 1..100)
    ) {
        // Total balance + fees == initial supply
    }
}
```

---

## Domain-Specific Test Modules

For tests that focus on a specific blockchain subsystem, place them in the
corresponding module under `src/`. These modules use the tier infrastructure
internally but are organized by domain:

| Module | Domain | Typical Tier Used |
|--------|--------|-------------------|
| `src/blockdag/` | DAG consensus, tip selection, ordering | Tier 1 / 1.5 |
| `src/transaction/` | TxSelector, mempool, fee, block assembly | Tier 1 / 1.5 |
| `src/p2p/` | Encryption, handshake, propagation, partition | Tier 1 / 3 |
| `src/sync/` | Snapshot, chain validator, reorg | Tier 1 / 1.5 / 3 |
| `src/conformance/` | Syscall/API spec compliance | Tier 1.5 / 2 |
| `src/fixtures/` | Declarative YAML scenario testing | All tiers |

**Rule of thumb:** If the test exercises real daemon code (e.g., `use tos_daemon::p2p::encryption::Encryption`), place it in the domain module. If it tests via RPC or multi-node, it may go in `tests/` as an integration test binary.

---

## Testing Layers Within a Domain Module

Each domain module follows a layered testing approach:

```
Layer 1:  Unit tests — import real daemon structs, test pure logic
Layer 1.5: ChainClient tests — single-node verify→apply pipeline
Layer 3:  Integration tests — LocalCluster multi-node scenarios
```

**Example: Adding tests for a new P2P feature**

```
src/p2p/
  my_new_feature.rs        ← Layer 1: direct import, test serialization/logic
  chain_client_my_feature.rs  ← Layer 1.5: TX lifecycle related to this feature
  (tests/p2p_my_feature_e2e.rs)  ← Layer 3: multi-node propagation test
```

---

## Deterministic Infrastructure

### MockClock (Time Control)

Controls wall-clock time without real waiting. Use for timeout, rate-limiting,
and time-dependent contract testing.

```rust
use tos_tck::orchestrator::MockClock;

let clock = Arc::new(MockClock::at(2025, 6, 1, 0, 0, 0));
clock.advance(Duration::from_secs(3600)); // Instant, no real delay
```

### BlockWarp (Chain State Advancement)

Advances chain topoheight by creating valid empty blocks. Use for testing
state at specific heights without mining overhead.

```rust
use tos_tck::tier1_5::block_warp::BlockWarp;

client.warp_to_topoheight(100).await.unwrap();
// Chain is now at topoheight 100, all blocks pass validation
```

**Key distinction:** `MockClock` advances time only. `BlockWarp` advances chain state.
Smart contracts may depend on either — use both when needed.

### FeatureSet (Protocol Upgrade Testing)

Test behavior before and after feature activation:

```rust
use tos_tck::tier1_5::features::{FeatureSet, Feature};

let features = FeatureSet::mainnet()
    .deactivate("new_fee_model")    // Test old behavior
    .activate_at("new_fee_model", 100); // Activates at height 100
```

---

## TX Fixture Framework (Declarative Testing)

For regression testing and cross-tier consistency, use YAML fixtures:

```yaml
fixture:
  name: "basic_transfer"
  tier: [1, 2, 3]

setup:
  accounts:
    alice: { balance: "10_000 TOS" }
    bob:   { balance: "0 TOS" }

transactions:
  - step: 1
    type: transfer
    from: alice
    to: bob
    amount: "1_000 TOS"
    fee: "10 TOS"
    expect_status: success
  - step: 2
    type: mine_block

expected:
  accounts:
    alice: { balance: "8_990 TOS", nonce: 1 }
    bob:   { balance: "1_000 TOS" }

invariants:
  - balance_conservation: { total_supply_change: "-10 TOS" }
  - nonce_monotonicity: true
```

Place fixtures in:
- `fixtures/scenarios/` — Standard functional tests
- `fixtures/regression/` — Bug reproduction (auto-captured or hand-written)
- `fixtures/stress/` — High-volume scenarios
- `fixtures/templates/` — Reusable account/scenario templates

---

## Verification Layers

Multi-layer verification ensures correctness at different abstraction levels:

| Layer | What It Checks | Example |
|-------|----------------|---------|
| Invariant | System-wide rules always hold | sum(balances) + fees == supply |
| State Convergence | All nodes agree on chain state | same topoheight, same tips |
| Data Consistency | Values match across nodes | balance(alice) same on all nodes |
| Behavioral | Observable actions produce expected results | TX confirmed on all nodes |

---

## Adding Tests for a New Feature: Step-by-Step

1. **Identify the code under test** — Which daemon/common module does it touch?

2. **Choose the tier** — Use the decision tree above. Prefer the lowest tier
   that can exercise the code path (faster feedback, less flakiness).

3. **Choose the location:**
   - Pure logic / daemon struct → `src/<domain>/my_feature.rs`
   - TX/contract lifecycle → `src/<domain>/chain_client_my_feature.rs`
   - RPC endpoint → `tests/my_feature_rpc.rs`
   - Multi-node behavior → `tests/my_feature_e2e.rs`
   - Regression → `fixtures/regression/reg_NNN_description.yaml`

4. **Write the test** following the standard pattern:
   - Import real daemon code (`use tos_daemon::...`) — never self-contained mocks
   - Set up state via the chosen tier infrastructure
   - Execute the operation
   - Verify results and invariants

5. **Add to CI** — Tests in `src/` and `tests/` are picked up automatically.
   Tier 3+ tests should be gated by feature flag or placed in nightly CI.

---

## Critical Rules

- **No self-contained mocks.** Every test must either `use tos_daemon::*` for
  unit/component tests, or use `LocalCluster`/`TestDaemon` for integration tests.
  Self-contained mocks cannot find real bugs.

- **Verify/Apply isolation.** Never mutate state in verification-phase tests.
  All state changes happen only in `apply` functions.

- **Checked arithmetic.** All amount calculations in test helpers must use
  `checked_*` or `saturating_*` functions.

- **Invariant checks.** After any state-changing test, verify:
  - Balance conservation (sum + fees == supply)
  - Nonce monotonicity (strictly increasing)
  - Energy weight consistency (network weight == sum of frozen balances)
  - No negative balances

- **Edge cases required.** Every new feature test must cover:
  - Zero amount input
  - Overflow-inducing amounts
  - Self-referential operations (self-transfer, self-delegation)
  - Duplicate/non-existent entries
  - Maximum capacity boundaries

---

## Fuzzing

Add fuzz targets for any new parsing or deserialization logic:

```
fuzz/fuzz_targets/
  fuzz_my_new_message.rs    ← New fuzz target
```

```rust
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = MyMessage::from_bytes(data);
});
```

Run with: `cargo +nightly fuzz run fuzz_my_new_message`

---

## Conformance Specs

For new syscalls or RPC methods, add a YAML conformance spec:

```yaml
# specs/syscalls/my_new_syscall.yaml
specs:
  - name: "my_syscall_basic"
    category: syscall
    preconditions:
      - account_exists: true
    action:
      syscall: my_new_syscall
      args: [100]
    expected:
      return_code: 0
    postconditions:
      - state_changed: true
```

---

## Quick Reference: Where to Put What

| I want to test... | Put it in... |
|--------------------|-------------|
| A pure algorithm (sorting, hashing, fee calc) | Unit test in the source crate |
| Transaction logic (balance/nonce changes) | `src/<domain>/` using Tier 1 or 1.5 |
| Smart contract behavior | `src/<domain>/` using `ContractTest` |
| RPC endpoint correctness | `tests/<module>_rpc.rs` using Tier 2 |
| Block/TX propagation across nodes | `tests/<module>_e2e.rs` using Tier 3 |
| Network partition recovery | `src/p2p/partition.rs` or `tests/` using Tier 3 |
| System invariants under random input | `src/tier4_chaos/` using proptest |
| Regression from a production bug | `fixtures/regression/reg_NNN.yaml` |
| New serialization format | `fuzz/fuzz_targets/fuzz_<name>.rs` |
| New syscall/RPC spec compliance | `specs/<category>/<name>.yaml` |
| Time-dependent behavior | Use `MockClock` + appropriate tier |
| Feature gate activation boundary | Use `FeatureSet` + Tier 1.5 |
