# TOS Consensus Specification

**Version**: 1.0
**Date**: 2025-11-30
**Status**: Official Specification

---

## 1. Overview

TOS uses a **PoW + GHOSTDAG** consensus mechanism that combines:
- Proof of Work (PoW) for Sybil resistance
- GHOSTDAG protocol for DAG-based block ordering and fork resolution

This specification documents the consensus rules that all TOS implementations MUST follow.

---

## 2. Block Structure

### 2.1 Block Header Fields

| Field | Type | Description |
|-------|------|-------------|
| `version` | `u8` | Block version (current: 0) |
| `parents_by_level` | `Vec<Vec<Hash>>` | Parent block hashes by level (only level 0 used) |
| `hash_merkle_root` | `Hash` | Merkle root of transactions |
| `timestamp` | `u64` | Block timestamp in milliseconds |
| `bits` | `u32` | Compact difficulty target |
| `nonce` | `u64` | PoW nonce |
| `extra_nonce` | `[u8; 32]` | Additional nonce space |
| `miner` | `PublicKey` | Miner's public key |
| `blue_score` | `u64` | GHOSTDAG blue score |
| `blue_work` | `U256` | Cumulative blue work |
| `daa_score` | `u64` | DAA window index |
| `pruning_point` | `Hash` | Current pruning point hash |

### 2.2 Block Hash Calculation

```
block_hash = blake3(serialize(header))
```

The PoW hash uses a different algorithm:
```
pow_hash = progpow_v2(header_without_nonce, nonce, extra_nonce)
```

### 2.3 Parent Structure

- **RULE**: `parents_by_level.len()` MUST equal 1 (only level 0 parents)
- **RULE**: `parents_by_level[0].len()` MUST be >= 1 and <= TIPS_LIMIT (32)
- All parent hashes MUST reference existing blocks in the DAG

---

## 3. GHOSTDAG Protocol

### 3.1 Parameters

| Parameter | Value | Description |
|-----------|-------|-------------|
| `GHOSTDAG_K` | 10 | Maximum anticone size for blue blocks |
| `TIPS_LIMIT` | 32 | Maximum number of parent tips |

### 3.2 Blue Score Calculation

For a block B with parents P:
```
blue_score(B) = max(blue_score(p) for p in P) + 1
```

For genesis block:
```
blue_score(genesis) = 0
```

### 3.3 Blue Work Calculation

Blue work represents cumulative PoW difficulty:

```rust
block_work = (2^256 - 1) / (target + 1) + 1

// For block B with selected parent SP:
blue_work(B) = blue_work(SP) + block_work(B)
```

Where `target` is derived from the `bits` field using Bitcoin's compact target format.

**SECURITY**: If target equals U256::MAX or is zero, special handling applies to prevent overflow/division-by-zero.

### 3.4 Fork Choice Rule

The canonical chain tip is selected by:

1. **Primary**: Maximum `blue_work`
2. **Tie-breaker**: Lexicographically smaller block hash

```rust
fn select_best_tip(tips: &[Hash]) -> Hash {
    tips.iter()
        .max_by(|a, b| {
            let work_a = get_blue_work(a);
            let work_b = get_blue_work(b);
            work_a.cmp(&work_b).then_with(|| b.cmp(a))
        })
        .unwrap()
}
```

---

## 4. Difficulty Adjustment Algorithm (DAA)

### 4.1 Parameters

| Parameter | Value | Description |
|-----------|-------|-------------|
| `DAA_WINDOW_SIZE` | 2016 | Blocks in adjustment window |
| `TARGET_TIME_PER_BLOCK` | 1 second | Target block interval |
| `MIN_DIFFICULTY_RATIO` | 0.25 | Minimum adjustment ratio |
| `MAX_DIFFICULTY_RATIO` | 4.0 | Maximum adjustment ratio |

### 4.2 Algorithm

```rust
fn calculate_next_difficulty(current_block: &Block) -> Difficulty {
    let daa_score = current_block.daa_score;

    // If window not yet filled, use parent difficulty
    if daa_score < DAA_WINDOW_SIZE {
        return parent_difficulty;
    }

    // Get window boundaries
    let window_start = get_block_at_daa_score(daa_score - DAA_WINDOW_SIZE);
    let window_end = get_block_at_daa_score(daa_score - 1);

    // Collect timestamps in window
    let timestamps: Vec<u64> = get_timestamps_in_window();

    // Use IQR (25%-75% percentile) for manipulation resistance
    let q1 = percentile(&timestamps, 25);
    let q3 = percentile(&timestamps, 75);
    let actual_time = (q3 - q1) * 2; // Scale back to full range

    // Calculate expected time
    let expected_time = DAA_WINDOW_SIZE * TARGET_TIME_PER_BLOCK * 1000; // in ms

    // Calculate ratio with bounds
    let ratio = actual_time as f64 / expected_time as f64;
    let clamped_ratio = ratio.clamp(MIN_DIFFICULTY_RATIO, MAX_DIFFICULTY_RATIO);

    // Adjust difficulty
    current_difficulty * clamped_ratio
}
```

### 4.3 Bits Field Validation

```rust
fn validate_bits(block: &Block, expected_difficulty: Difficulty) -> bool {
    let expected_bits = difficulty_to_bits(expected_difficulty);
    block.bits == expected_bits
}
```

---

## 5. Block Validation

### 5.1 Validation Stages

All blocks MUST pass these validation stages in order:

1. **Basic Validation**
   - Version matches expected version for blue_score
   - Block size <= MAX_BLOCK_SIZE
   - Recomputed hash matches provided hash

2. **Parent Validation**
   - All parents exist in DAG
   - Only level 0 parents (parents_by_level.len() == 1)
   - Parent count <= TIPS_LIMIT

3. **PoW Validation**
   - pow_hash < target (derived from bits)
   - bits matches DAA-calculated difficulty

4. **GHOSTDAG Validation**
   - blue_score matches computed value
   - blue_work matches computed value

5. **Timestamp Validation**
   - timestamp > max(parent timestamps)
   - timestamp > median(parent timestamps) for multi-parent blocks

6. **Merkle Root Validation**
   - Empty blocks: merkle_root == zero_hash
   - Non-empty blocks: merkle_root == compute_merkle_root(transactions)

7. **Commitment Validation**
   - pruning_point matches computed pruning point
   - Reserved fields (accepted_id_merkle_root, utxo_commitment) == 0

8. **Transaction Validation**
   - All transactions valid
   - No double spends within block

### 5.2 Rejection Conditions

Blocks are REJECTED if any validation fails. Error codes:

| Error | Condition |
|-------|-----------|
| `InvalidVersion` | Version mismatch |
| `BlockHashMismatch` | Hash verification failed |
| `InvalidParentsLevelCount` | More than 1 parent level |
| `InvalidTipsCount` | Too many parents |
| `InvalidBitsField` | Difficulty mismatch |
| `InvalidBlockHeight` | Blue score mismatch |
| `InvalidMerkleRoot` | Merkle root mismatch |
| `InvalidTimestamp` | Timestamp validation failed |

---

## 6. Finality

### 6.1 Stable Height

A block is considered **stable** (finalized) when:
```
current_blue_score - block.blue_score >= STABLE_LIMIT
```

| Parameter | Value | Description |
|-----------|-------|-------------|
| `STABLE_LIMIT` | 20 | Blocks required for finality |

**Finality time**: ~20 seconds at 1 BPS

### 6.2 Security Properties

- Reorg probability at depth d: P(reorg) = O(e^{-cd})
- At STABLE_LIMIT=20 with honest majority: reorg probability < 0.001%

---

## 7. Network Parameters

### 7.1 Mainnet

| Parameter | Value |
|-----------|-------|
| Network ID | `mainnet` |
| Genesis Hash | (TBD) |
| Default P2P Port | 8080 |

### 7.2 Testnet

| Parameter | Value |
|-----------|-------|
| Network ID | `testnet` |
| Genesis Hash | (TBD) |
| Default P2P Port | 8080 |

### 7.3 Devnet

| Parameter | Value |
|-----------|-------|
| Network ID | `devnet` |
| Genesis Hash | Configurable |
| Default P2P Port | 8080 |

---

## 8. Security Considerations

### 8.1 Attack Resistance

- **51% Attack**: Requires majority hash power
- **Selfish Mining**: GHOSTDAG reduces profitability vs single-chain
- **Timestamp Manipulation**: IQR-based DAA provides resistance
- **Long-range Attack**: Pruning point + checkpoints

### 8.2 Unsafe Configuration Flags

The following flags bypass security checks and are ONLY allowed on devnet:

| Flag | Risk |
|------|------|
| `--skip-pow-verification` | Accepts any PoW |
| `--skip-block-template-txs-verification` | Accepts invalid TXs |
| `--allow-fast-sync` (on mainnet/testnet) | Requires `--i-understand-fast-sync-risks` |

---

## 9. Implementation Requirements

### 9.1 Determinism

All consensus calculations MUST be deterministic:
- Use integer arithmetic (U256) for blue_work
- Use fixed-point arithmetic for DAA ratios
- Sort operations must use consistent ordering

### 9.2 Validation Path

**CRITICAL**: All blocks MUST go through `add_new_block()` validation:
- RPC submissions
- P2P received blocks
- Miner-produced blocks

No shortcuts or alternative paths are permitted.

---

## 10. References

- [GHOSTDAG Paper](https://eprint.iacr.org/2018/104.pdf)
- [Bitcoin Difficulty](https://en.bitcoin.it/wiki/Difficulty)
- [TOS Source Code](https://github.com/tos-network/tos)

---

## Changelog

- **v1.0 (2025-11-30)**: Initial specification based on security audit F-02
