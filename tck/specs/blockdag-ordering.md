# TOS BlockDAG Ordering Specification

> Based on actual implementation in `daemon/src/core/blockchain.rs` and `daemon/src/core/blockdag.rs`

## 1. Overview

TOS uses a BlockDAG (Directed Acyclic Graph) structure where blocks can have multiple parents. This document specifies the deterministic ordering algorithm for transaction execution.

**IMPORTANT**: TOS does NOT use GHOSTDAG. It uses a custom cumulative-difficulty-based topological ordering.

## 2. Block Structure

**File**: `common/src/block/header.rs:60-87`

```rust
pub struct BlockHeader {
    pub version: BlockVersion,
    pub tips: Immutable<IndexSet<Hash>>,  // Parent block hashes (1-3)
    pub timestamp: TimestampMillis,
    pub height: u64,
    pub nonce: u64,
    pub extra_nonce: [u8; EXTRA_NONCE_SIZE],
    pub miner: CompressedPublicKey,
    pub txs_hashes: IndexSet<Hash>,
    pub vrf: Option<BlockVrfData>,
}
```

### Key Fields

- **tips**: Parent block references (up to `TIPS_LIMIT = 3`)
- **height**: `max(parent_heights) + 1`
- **timestamp**: Must be >= all parent timestamps

## 3. Topological Height (TopoHeight)

**File**: `common/src/block/mod.rs`

```rust
pub type TopoHeight = u64;  // Sequential counter, 0-based
```

TopoHeight provides a total ordering of all blocks in the DAG. Each block is assigned a unique topoheight during execution.

## 4. DAG Ordering Algorithm

**File**: `daemon/src/core/blockchain.rs:2232-2326`

The `generate_full_order()` function produces a deterministic total ordering:

### Algorithm Steps

1. **Initialize**: Start from target block, create processing stack
2. **Traverse**: Walk DAG backward through tips
3. **Sort Tips**: Order tips by cumulative difficulty (ascending)
4. **Process**: Pop from stack, assign topoheight in order

### Code Reference (lines 2307-2317)

```rust
// Sort tips by cumulative difficulty (ascending for correct processing order)
blockdag::sort_ascending_by_cumulative_difficulty(&mut scores);

processed.insert(current_hash.clone());
stack.push_back(current_hash);

for (tip_hash, _) in scores {
    stack.push_back(tip_hash);
}
```

### Execution Order

```
topoheight = base_topoheight + skipped + i
```

Where:
- `base_topoheight`: Starting topoheight for this batch
- `skipped`: Number of already-processed blocks
- `i`: Index in current ordering

## 5. Cumulative Difficulty

**File**: `daemon/src/core/blockchain.rs:2113-2169`

```rust
pub async fn find_tip_work_score<P>() -> Result<(HashSet<Hash>, CumulativeDifficulty)>
```

Cumulative difficulty is the sum of all difficulties from a block back through its entire ancestry. Used for:
- Tip selection
- Chain weighting
- Fork resolution

## 6. Best Tip Selection

**File**: `daemon/src/core/blockdag.rs:118-150`

```rust
pub async fn find_best_tip_by_cumulative_difficulty<'a>() -> Result<&'a Hash>
```

When multiple tips exist, select the one with highest cumulative difficulty.

**Fork Choice Rule** (conceptual):
```
best_tip = max(candidates, key=cumulative_difficulty)
```

Tiebreaker: Lower block hash (lexicographic).

## 7. Reachability Verification

**File**: `daemon/src/core/blockdag.rs:192-268`

```rust
pub async fn verify_non_reachability<P>() -> Result<bool>
pub async fn build_reachability<P>() -> Result<HashSet<Hash>>
```

### Non-Reachability Rule

No tip can be an ancestor of another tip. This ensures tips are independent branches.

**Validation** (line 3457 in blockchain.rs):
```rust
blockdag::verify_non_reachability(&*storage, block.get_tips(), version).await?;
```

Traverses ancestry graph up to `2 × STABLE_LIMIT` depth.

## 8. Block Height Calculation

**File**: `common/src/block/header.rs`

```rust
height = max(tip.height for tip in tips) + 1
```

- Genesis block: height = 0
- All other blocks: max parent height + 1

## 9. Tip Validation

**File**: `daemon/src/core/blockchain.rs:2329-2342`

```rust
async fn validate_tips() -> Result<bool>
```

### Rules

1. **Count**: Maximum `TIPS_LIMIT` (3) tips allowed
2. **Existence**: All tips must exist in chain
3. **Difficulty**: All tips within 91% of best tip difficulty
4. **Non-reachability**: No tip is ancestor of another tip

## 10. Stability and Finality

**File**: `daemon/src/config.rs`

```rust
pub const STABLE_LIMIT: u64 = 24;           // Blocks for finality
pub const PRUNE_SAFETY_LIMIT: u64 = STABLE_LIMIT * 10;  // = 240, max rewind depth
```

### Stability Check

**File**: `daemon/src/core/blockchain.rs:5272-5290`

```rust
pub async fn has_block_stable_order<P>() -> Result<bool> {
    // Block is stable if:
    // current_topoheight - block_topoheight >= STABLE_LIMIT
}
```

Once stable, blocks cannot be orphaned by new blocks.

## 11. Side Block Detection

**File**: `daemon/src/core/blockchain.rs:5230-5269`

A block is a "side block" if:
- It is topologically ordered (has assigned topoheight)
- Its block height ≤ any block in the past STABLE_LIMIT (24) topographical blocks
- Not genesis block

Side blocks receive reduced mining rewards to incentivize building on the main chain.

## 12. Orphaning and Reorgs

**File**: `daemon/src/core/blockchain.rs:4034-4127`

When new blocks arrive, the DAG may reorder:

1. Compute new `full_order` from new tip
2. Compare with previous order
3. Blocks not in new order become **orphaned**
4. Orphaned blocks: transactions unexecuted, balances reverted
5. Event: `BlockOrphanedEvent`

## 13. Transaction Ordering Within Blocks

Transactions within a single block are ordered by their position in `txs_hashes` (insertion order maintained by `IndexSet`).

## 14. Storage Providers

**File**: `daemon/src/core/storage/rocksdb/providers/dag_order.rs`

| Column | Key | Value |
|--------|-----|-------|
| `TopoByHash` | Block Hash | TopoHeight |
| `HashAtTopo` | TopoHeight | Block Hash |

## 15. Summary

TOS BlockDAG characteristics:

| Feature | Value |
|---------|-------|
| Max Parents (Tips) | 3 (TIPS_LIMIT) |
| Ordering Method | Cumulative difficulty sort |
| Finality Depth | 24 blocks (STABLE_LIMIT) |
| Prune Safety | 240 blocks (PRUNE_SAFETY_LIMIT) |
| Tip Difficulty Threshold | 91% of best |
| Protocol | Custom (NOT GHOSTDAG) |

---

*Document Version: 1.0*
*Based on: TOS Rust implementation*
*Last Updated: 2026-02-04*
