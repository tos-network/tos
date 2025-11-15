# TOS Test Scenarios (V2.2 Format)

This directory contains YAML test scenarios following the TOS Testing Framework V3.0 specification.

## Format Rules (V2.2)

1. **No underscores in numbers**: Use strings instead
   - ❌ `balance: 1_000_000_000_000`
   - ✅ `balance: "1000000000000"`

2. **No tilde approximation**: Use `within` structure
   - ❌ `eq: ~1000`
   - ✅ `within: { target: "1000", tolerance: "10" }`

3. **Three assertion modes**:
   - `eq: "exact_value"` - Exact equality
   - `within: { target, tolerance }` - Approximate with tolerance
   - `compare: { gte|lte|gt|lt }` - Relational comparison

## Available Scenarios

### simple_transfer.yaml
Basic transfer demonstrating all three assertion modes:
- Exact (`eq`)
- Within tolerance (`within`)
- Comparison operators (`compare`)

### receive_then_spend.yaml
Tests that received funds are immediately available:
- Alice → Bob → Charlie in same block
- Validates parallel execution correctness

### miner_reward_spend.yaml
Tests miner can spend block rewards immediately:
- Miner receives reward
- Spends more than initial balance (uses reward)

### parallel_transfers.yaml
Multiple independent transfers:
- Alice → Charlie
- Bob → David
- Both execute in parallel (no conflicts)

### nonce_conflict.yaml
Conflicting nonce detection:
- Two transactions with same nonce
- One succeeds, one fails
- Validates conflict resolution

### multi_block_sequence.yaml
Multi-block progression:
- Multiple blocks with sequential operations
- Nonce progression tracking
- Final state validation

## Usage

### Parse a scenario

```rust
use tos_testing_framework::scenarios::parse_scenario;

let yaml = std::fs::read_to_string("scenarios/simple_transfer.yaml")?;
let scenario = parse_scenario(&yaml)?;
```

### Execute a scenario

```rust
use tos_testing_framework::scenarios::ScenarioExecutor;

let executor = ScenarioExecutor::new(&scenario).await?;
executor.execute(&scenario).await?;
```

## Invariants

All scenarios check these invariants after execution:

- `balance_conservation` - Total supply remains constant (accounting for fees)
- `nonce_monotonicity` - Nonces only increase for confirmed transactions

## Adding New Scenarios

1. Create YAML file following V2.2 format
2. Ensure all numbers are strings (no underscores)
3. Use appropriate assertion mode (eq/within/compare)
4. Add invariants section
5. Test with scenario parser

Example template:

```yaml
name: "Your Scenario Name"
description: "What this tests"

genesis:
  network: "devnet"
  accounts:
    - name: "alice"
      balance: "1000000000000"

steps:
  - action: "transfer"
    from: "alice"
    to: "bob"
    amount: "100000000000"
    fee: "50"

  - action: "mine_block"

  - action: "assert_balance"
    account: "bob"
    eq: "100000000000"

invariants:
  - "balance_conservation"
  - "nonce_monotonicity"
```

## Reference

See: `/Users/tomisetsu/tos-network/memo/02-Testing/TOS_TESTING_FRAMEWORK_V3.md`
- Section 9.1-9.4: DSL format and parser specification
