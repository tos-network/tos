# BlockDAG Execution Order Algorithm

This document specifies the deterministic algorithm for ordering blocks and transactions in TOS's BlockDAG structure. All clients MUST implement this algorithm identically.

## 1. Overview

TOS uses a BlockDAG (Directed Acyclic Graph) structure where blocks can have multiple parents. This requires a deterministic algorithm to order transactions for execution.

## 2. Block Structure

```
+-----------------------------------------------------------+
|                         Block Header                       |
+-----------------+-----------------------------------------+
|  version (u8)   |  Block format version                   |
+-----------------+-----------------------------------------+
|  height (u64)   |  Block height (max parent height + 1)   |
+-----------------+-----------------------------------------+
|  timestamp (u64)|  Block creation time (ms)               |
+-----------------+-----------------------------------------+
|  parents        |  List of parent block hashes (1-N)      |
+-----------------+-----------------------------------------+
|  tx_root (32)   |  Merkle root of transactions            |
+-----------------+-----------------------------------------+
|  state_root (32)|  State root after block execution       |
+-----------------+-----------------------------------------+
|  difficulty     |  Proof-of-work difficulty               |
+-----------------+-----------------------------------------+
|  nonce (u64)    |  PoW nonce                              |
+-----------------+-----------------------------------------+
```

## 3. DAG Ordering Algorithm (GHOSTDAG)

The execution order of blocks follows the GHOSTDAG protocol.

### Step 1: Compute Blue Set

```python
def compute_blue_set(block, k):
    """
    Compute the blue (honest) set of blocks.
    k = anticone size parameter (e.g., 18)
    """
    blue_set = set()

    for parent in sorted_parents(block):
        if len(anticone(parent, blue_set)) <= k:
            blue_set.add(parent)
            blue_set |= parent.blue_set

    return blue_set

def anticone(block, blue_set):
    """
    Blocks neither in past nor future of block,
    relative to blue_set.
    """
    all_blocks = get_all_blocks()
    past = get_past(block)
    future = get_future(block)
    return all_blocks - past - future - {block}

def sorted_parents(block):
    """
    Sort parents by blue score (descending), then by hash.
    """
    return sorted(block.parents, key=lambda p: (-p.blue_score, p.hash))
```

### Step 2: Order Blocks

```python
def order_blocks(tip):
    """
    Produce deterministic total ordering of all blocks.
    """
    ordered = []
    blue_set = compute_blue_set(tip, K_PARAMETER)

    # Process blocks in topological order
    for block in topological_sort(tip.ancestors):
        if block in blue_set:
            ordered.append(block)
        else:
            # Red blocks inserted after their blue merge point
            merge_point = find_merge_point(block, blue_set)
            insert_after(ordered, merge_point, block)

    return ordered

def find_merge_point(red_block, blue_set):
    """
    Find the first blue block that is in red_block's future.
    """
    for blue_block in blue_set:
        if red_block in get_past(blue_block):
            return blue_block
    return None  # Should not happen for valid DAG

def topological_sort(blocks):
    """
    Sort blocks so parents come before children.
    """
    visited = set()
    result = []

    def visit(block):
        if block in visited:
            return
        visited.add(block)
        for parent in block.parents:
            visit(parent)
        result.append(block)

    for block in blocks:
        visit(block)

    return result
```

### Step 3: Order Transactions Within Blocks

Within each block, transactions are ordered by:
1. Transaction type priority (Coinbase first)
2. Fee (higher fee first)
3. Nonce (lower nonce first for same sender)
4. Transaction ID (lexicographic, as tiebreaker)

```python
def order_transactions(block):
    """
    Order transactions within a single block.
    """
    return sorted(block.transactions, key=lambda tx: (
        0 if tx.type == COINBASE else 1,  # Coinbase first
        -tx.fee,                           # Higher fee first
        tx.sender,                         # Group by sender
        tx.nonce,                          # Lower nonce first
        tx.txid                            # Tiebreaker
    ))
```

## 4. Height Calculation

Block height is deterministic:

```python
def calculate_height(block):
    """
    Block height = max(parent heights) + 1
    Genesis block has height 0.
    """
    if not block.parents:
        return 0  # Genesis
    return max(parent.height for parent in block.parents) + 1
```

## 5. Fork Choice Rule

When multiple tips exist, select the one with highest cumulative difficulty:

```python
def select_tip(candidates):
    """
    Select the best tip (chain head).
    """
    return max(candidates, key=lambda b: (
        b.cumulative_difficulty,  # Primary: most work
        b.blue_score,             # Secondary: most blue blocks
        b.hash                    # Tertiary: lowest hash (tiebreaker)
    ))

def cumulative_difficulty(block):
    """
    Sum of all difficulties in the blue set.
    """
    return sum(b.difficulty for b in block.blue_set)
```

## 6. Constants

| Constant | Value | Description |
|----------|-------|-------------|
| K_PARAMETER | 18 | GHOSTDAG anticone size parameter |
| MAX_PARENTS | 10 | Maximum parent references per block |
| MIN_PARENTS | 1 | Minimum parent references (except genesis) |

## 7. Execution Order Summary

Final execution order:
1. Sort all blocks using GHOSTDAG ordering
2. For each block in order:
   a. Sort transactions within block
   b. Execute transactions in sorted order
   c. Update state
3. Final state is the result

## 8. Test Vectors

Test vectors for BlockDAG ordering are located in:
- `tck/vectors/execution/dag-ordering.yaml`
- `tck/vectors/execution/dag-reorg.yaml`

---

*Document Version: 1.0*
*Last Updated: 2026-02-03*
*Reference: MULTI_CLIENT_ALIGNMENT_SCHEME.md Section 2.C*
