# Nonce Handling Rules

This document specifies nonce validation and processing rules for the TOS protocol. All clients MUST implement these rules identically.

## 1. Overview

The nonce is a strictly monotonically increasing counter per account that:
- Prevents transaction replay
- Orders transactions from the same sender
- Enables transaction replacement

## 2. Nonce Validation Rules

| Rule | Description | Error Code |
|------|-------------|------------|
| **Minimum** | nonce >= account.nonce | `NONCE_TOO_LOW` (0x0110) |
| **Maximum** | nonce <= account.nonce + MAX_GAP | `NONCE_TOO_HIGH` (0x0111) |
| **Uniqueness** | No duplicate nonce in mempool | `NONCE_DUPLICATE` (0x0112) |

**MAX_GAP**: Maximum allowed gap between account nonce and transaction nonce = **64**

## 3. Nonce Processing Phases

### 3.1 Verification Phase (Mempool/Block Validation)

```python
def verify_nonce(tx, account_state):
    """
    Verify nonce during transaction verification.
    Does NOT modify state.
    """
    expected = account_state.nonce

    if tx.nonce < expected:
        return Error.NONCE_TOO_LOW

    if tx.nonce > expected + MAX_NONCE_GAP:
        return Error.NONCE_TOO_HIGH

    return OK
```

### 3.2 Execution Phase (Block Execution)

```python
def apply_nonce(tx, account_state):
    """
    Apply nonce during transaction execution.
    Modifies state.
    """
    # Strict check: nonce must match exactly during execution
    if tx.nonce != account_state.nonce:
        return Error.NONCE_MISMATCH

    account_state.nonce += 1
    return OK
```

## 4. Gap Handling

When transactions arrive with gaps in nonces:

```
Account nonce: 5
Received: tx(nonce=5), tx(nonce=7), tx(nonce=6), tx(nonce=9)

Mempool state:
  nonce 5: tx(nonce=5)  <- ready for execution
  nonce 6: tx(nonce=6)  <- pending (waiting for 5)
  nonce 7: tx(nonce=7)  <- pending (waiting for 5, 6)
  nonce 9: tx(nonce=9)  <- pending (waiting for 5, 6, 7, 8 missing!)

Execution order (when tx with nonce 8 arrives):
  tx(nonce=5) -> tx(nonce=6) -> tx(nonce=7) -> tx(nonce=8) -> tx(nonce=9)
```

### Gap Behavior

| Scenario | Behavior |
|----------|----------|
| nonce = expected | Execute immediately |
| nonce > expected (within MAX_GAP) | Queue, wait for missing nonces |
| nonce > expected + MAX_GAP | Reject with NONCE_TOO_HIGH |
| nonce < expected | Reject with NONCE_TOO_LOW |

## 5. Transaction Replacement

A transaction can be replaced in the mempool if:
1. Same sender
2. Same nonce
3. Higher fee (by at least MIN_FEE_BUMP = 10%)

```python
MIN_FEE_BUMP = 0.10  # 10%

def can_replace(new_tx, existing_tx):
    """
    Check if new_tx can replace existing_tx in mempool.
    """
    if new_tx.sender != existing_tx.sender:
        return False
    if new_tx.nonce != existing_tx.nonce:
        return False
    if new_tx.fee < existing_tx.fee * (1 + MIN_FEE_BUMP):
        return False
    return True

def replace_transaction(mempool, new_tx):
    """
    Attempt to replace existing transaction.
    """
    existing = mempool.get(new_tx.sender, new_tx.nonce)
    if existing is None:
        mempool.add(new_tx)
        return True

    if can_replace(new_tx, existing):
        mempool.remove(existing)
        mempool.add(new_tx)
        return True

    return False  # Replacement failed
```

## 6. Nonce After Failure

**CRITICAL**: Nonce is ALWAYS advanced when a transaction enters a block, even if execution fails.

```python
def execute_with_nonce(tx, state):
    """
    Nonce advancement happens before payload execution.
    """
    # Advance nonce (happens regardless of success/failure)
    state.increment_nonce(tx.sender)

    # Try to execute
    result = execute_payload(tx, state)

    # Even if result is failure, nonce was already advanced
    return result
```

### Example

```
Before:
  alice.nonce = 5
  alice.balance = 50

Transaction: transfer 100 tokens (fails - insufficient balance)

After:
  alice.nonce = 6  (advanced despite failure!)
  alice.balance = 49  (only fee deducted)
```

## 7. Constants

| Constant | Value | Description |
|----------|-------|-------------|
| MAX_NONCE_GAP | 64 | Maximum allowed gap from account nonce |
| MIN_FEE_BUMP | 10% | Minimum fee increase for replacement |
| INITIAL_NONCE | 0 | Nonce for new accounts |

## 8. Edge Cases

### New Account

New accounts (created by receiving tokens) start with nonce 0:
```
Account created -> nonce = 0
First TX from account must have nonce = 0
```

### Genesis Block

Genesis transactions have special nonce handling:
- System account nonces may be pre-set
- Genesis transactions bypass normal nonce validation

### Contract Creation

Contract accounts start with nonce 1 (nonce 0 is used by creation):
```
Contract deployed -> contract.nonce = 1
```

## 9. Test Vectors

Test vectors for nonce handling are located in:
- `tck/vectors/state/nonce-basic.yaml`
- `tck/vectors/state/nonce-gaps.yaml`
- `tck/vectors/state/nonce-replacement.yaml`
- `tck/vectors/errors/validation-errors.yaml`

---

*Document Version: 1.0*
*Last Updated: 2026-02-03*
*Reference: MULTI_CLIENT_ALIGNMENT_SCHEME.md Section 2.E*
