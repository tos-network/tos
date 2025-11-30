# TOS Block Validation Path

**Version**: 1.0
**Date**: 2025-11-30
**Status**: Developer Documentation
**Security Audit Reference**: F-06

---

## Overview

This document describes the unified block validation path in TOS. All blocks MUST go through this path regardless of their source (RPC, P2P, or miner).

**CRITICAL**: No alternative validation paths are permitted. Any bypass would be a consensus security vulnerability.

---

## Entry Points

All blocks enter the consensus through a single function:

```rust
// daemon/src/core/blockchain.rs
pub async fn add_new_block(&self, block: Block, broadcast: bool) -> Result<(), BlockchainError>
```

### Source → Entry Point Mapping

| Source | Code Path | Entry Point |
|--------|-----------|-------------|
| **RPC (submit_block)** | `rpc/rpc.rs` → `submit_block()` | `blockchain.add_new_block()` |
| **P2P (received block)** | `p2p/mod.rs` → `handle_block()` | `blockchain.add_new_block()` |
| **Miner (new block)** | `core/mining/` → `submit_block()` | `blockchain.add_new_block()` |
| **Chain Sync** | `p2p/chain_sync/` → `process_blocks()` | `blockchain.add_new_block()` |

---

## Validation Stages

The `add_new_block()` function performs validation in the following order:

### Stage 1: Hash Verification
```rust
// SECURITY FIX: Always recompute hash
let computed_hash = block.compute_hash();
if let Some(provided_hash) = caller_hash {
    if computed_hash != provided_hash {
        return Err(BlockchainError::BlockHashMismatch);
    }
}
```

### Stage 2: Version Check
```rust
let expected_version = get_version_at_height(network, block.blue_score);
if block.version != expected_version {
    return Err(BlockchainError::InvalidVersion);
}
```

### Stage 3: Size Check
```rust
if block.size() > MAX_BLOCK_SIZE {
    return Err(BlockchainError::BlockTooBig);
}
```

### Stage 4: Parent Validation
```rust
// SECURITY FIX: Only level 0 parents allowed
if block.parents_by_level.len() != 1 {
    return Err(BlockchainError::InvalidParentsLevelCount);
}

if block.parents_by_level[0].len() > TIPS_LIMIT {
    return Err(BlockchainError::InvalidTipsCount);
}

// Verify all parents exist
for parent in &block.parents_by_level[0] {
    if !self.has_block(parent).await? {
        return Err(BlockchainError::InvalidParent);
    }
}
```

### Stage 5: PoW Validation
```rust
// Calculate expected difficulty from DAA
let expected_difficulty = self.daa.calculate_difficulty(&block)?;
let expected_bits = difficulty_to_bits(expected_difficulty);

// SECURITY FIX: Verify bits field matches DAA
if block.bits != expected_bits {
    return Err(BlockchainError::InvalidBitsField);
}

// Verify PoW hash meets target
if !self.pow.verify(&block)? {
    return Err(BlockchainError::InvalidPoW);
}
```

### Stage 6: GHOSTDAG Validation
```rust
// Compute GHOSTDAG data
let ghostdag_data = self.ghostdag.compute(&block)?;

// SECURITY FIX: Verify blue_score matches
if block.blue_score != ghostdag_data.blue_score {
    return Err(BlockchainError::InvalidBlockHeight);
}

// SECURITY FIX: Verify blue_work matches
if block.blue_work != ghostdag_data.blue_work {
    return Err(BlockchainError::InvalidBlueWork);
}
```

### Stage 7: Timestamp Validation
```rust
// SECURITY FIX: Use unified timestamp validation
validate_block_timestamp(&block, &parent_timestamps)?;
```

### Stage 8: Merkle Root Validation
```rust
// SECURITY FIX: Validate merkle root
let computed_root = compute_merkle_root(&block.transactions);
if block.transactions.is_empty() {
    if block.hash_merkle_root != ZERO_HASH {
        return Err(BlockchainError::InvalidMerkleRoot);
    }
} else {
    if block.hash_merkle_root != computed_root {
        return Err(BlockchainError::InvalidMerkleRoot);
    }
}
```

### Stage 9: Commitment Validation
```rust
// SECURITY FIX: Validate commitment fields
let expected_pruning_point = self.calc_pruning_point(&block)?;
if block.pruning_point != expected_pruning_point {
    return Err(BlockchainError::InvalidPruningPoint);
}

// Reserved fields must be zero
if block.accepted_id_merkle_root != ZERO_HASH {
    return Err(BlockchainError::InvalidCommitment);
}
```

### Stage 10: Transaction Validation
```rust
// Validate all transactions
for tx in &block.transactions {
    self.validate_transaction(tx).await?;
}

// Check for double spends within block
self.check_intra_block_double_spend(&block)?;
```

### Stage 11: State Execution
```rust
// Execute block and update state
self.execute_block(&block).await?;
```

### Stage 12: Storage Commit
```rust
// Commit block to storage
self.storage.store_block(&block).await?;
self.storage.store_ghostdag_data(&ghostdag_data).await?;
```

---

## Security Invariants

### Invariant 1: Single Entry Point
All blocks MUST enter through `add_new_block()`. No shortcuts.

### Invariant 2: Complete Validation
All validation stages MUST pass. No partial validation.

### Invariant 3: Atomic Commits
State changes are committed atomically after all validation passes.

### Invariant 4: Deterministic Order
Validation order is fixed and deterministic across all nodes.

---

## Code Review Checklist

When reviewing PRs that touch consensus code:

- [ ] Does the PR add a new block entry point? (REJECT if not through `add_new_block`)
- [ ] Does the PR skip any validation stage? (REJECT)
- [ ] Does the PR change validation order? (CAREFUL REVIEW)
- [ ] Does the PR modify GHOSTDAG/DAA calculations? (CAREFUL REVIEW)
- [ ] Does the PR add new commit paths? (REJECT if not atomic)

---

## Testing Requirements

1. **Unit Tests**: Each validation stage should have dedicated tests
2. **Integration Tests**: Full block submission path should be tested
3. **Fuzzing**: Invalid blocks should be rejected at appropriate stages
4. **Property Tests**: Validation should be deterministic (same input → same result)

---

## Related Files

| File | Description |
|------|-------------|
| `daemon/src/core/blockchain.rs` | Main validation logic |
| `daemon/src/core/ghostdag/mod.rs` | GHOSTDAG computation |
| `daemon/src/core/ghostdag/daa.rs` | Difficulty adjustment |
| `daemon/src/rpc/rpc.rs` | RPC entry point |
| `daemon/src/p2p/mod.rs` | P2P entry point |

---

## Changelog

- **v1.0 (2025-11-30)**: Initial documentation based on security audit F-06
