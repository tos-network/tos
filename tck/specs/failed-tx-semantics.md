# Failed Transaction Semantics

This document specifies how the TOS protocol handles failed transactions. All clients MUST implement these semantics identically.

## 1. Overview

When a transaction fails during execution, the system must handle:
- Fee deduction
- State rollback
- Error recording
- Nonce advancement

## 2. Failure Categories

| Category | Fee Charged | Nonce Advanced | State Changes |
|----------|-------------|----------------|---------------|
| **Pre-validation failure** | No | No | None |
| **Execution failure** | Yes | Yes | Rolled back |
| **Partial execution** | Yes | Yes | Partial (see below) |

### Pre-validation Failures

Rejected before execution - transaction never enters a block:
- Invalid signature
- Insufficient fee (cannot pay minimum)
- Nonce too low (already used)
- Nonce too high (gap exceeds maximum)
- Invalid wire format
- Timestamp out of range

### Execution Failures

Fail during execution - transaction is included in block:
- Insufficient balance (for transfer amount)
- Contract revert
- Invalid recipient
- Business logic errors (e.g., escrow wrong state)

## 3. Fee Handling

```python
def execute_transaction(tx, state):
    """
    Transaction execution with fee handling.
    """
    # Step 1: Deduct fee (always, if TX enters block)
    sender_balance = state.get_balance(tx.sender)
    if sender_balance < tx.fee:
        return Error.INSUFFICIENT_FEE  # Pre-validation failure

    state.deduct(tx.sender, tx.fee)
    state.increment_nonce(tx.sender)

    # Step 2: Create checkpoint
    checkpoint = state.checkpoint()

    # Step 3: Execute transaction
    try:
        result = execute_payload(tx, state)
        if result.is_error():
            state.rollback(checkpoint)  # Rollback payload effects
            return result               # Fee still charged
        return result
    except Exception as e:
        state.rollback(checkpoint)
        return Error.EXECUTION_FAILED
```

## 4. Rollback Semantics

| State Component | Rollback Behavior |
|-----------------|-------------------|
| Sender balance | Fee deducted, transfer amount rolled back |
| Sender nonce | Advanced (NOT rolled back) |
| Recipient balance | Rolled back |
| Contract state | Rolled back |
| Events/logs | Cleared |

**CRITICAL**: Nonce is NEVER rolled back on failure. This prevents replay attacks where a failed transaction could be resubmitted.

## 5. State Changes on Failure

### Successful Transaction
```
Before:
  alice.balance = 1000
  alice.nonce = 5
  bob.balance = 500

Transaction: alice -> bob, 100 tokens, fee = 1

After (success):
  alice.balance = 899  (1000 - 100 - 1)
  alice.nonce = 6
  bob.balance = 600    (500 + 100)
```

### Failed Transaction (execution failure)
```
Before:
  alice.balance = 50   (insufficient for transfer)
  alice.nonce = 5
  bob.balance = 500

Transaction: alice -> bob, 100 tokens, fee = 1

After (failure - INSUFFICIENT_BALANCE):
  alice.balance = 49   (50 - 1, fee only)
  alice.nonce = 6      (advanced despite failure)
  bob.balance = 500    (unchanged)
```

### Rejected Transaction (pre-validation failure)
```
Before:
  alice.balance = 1000
  alice.nonce = 5

Transaction: alice -> bob, nonce = 4 (too low)

After (rejected - NONCE_TOO_LOW):
  alice.balance = 1000  (unchanged)
  alice.nonce = 5       (unchanged)
  (transaction not in block)
```

## 6. Failure Recording

Failed transactions are recorded in blocks with:

```
+-----------------+-----------------------------------------+
|  txid (32)      |  Transaction ID                         |
+-----------------+-----------------------------------------+
|  error_code (u8)|  Standardized error code                |
+-----------------+-----------------------------------------+
|  gas_used (u64) |  Computational resources consumed       |
+-----------------+-----------------------------------------+
```

## 7. Failure Matrix by Transaction Type

| TX Type | Common Failures | Fee Charged? |
|---------|-----------------|--------------|
| Transfer | INSUFFICIENT_BALANCE, ACCOUNT_NOT_FOUND | Yes |
| Burn | INSUFFICIENT_BALANCE, INVALID_AMOUNT | Yes |
| EnergyBuy | INSUFFICIENT_BALANCE | Yes |
| Delegate | INSUFFICIENT_FROZEN, DELEGATION_EXISTS | Yes |
| EscrowCreate | INSUFFICIENT_BALANCE, INVALID_PARAMS | Yes |
| ContractCall | CONTRACT_REVERT, OUT_OF_GAS | Yes |

## 8. Gas Accounting

Even failed transactions consume gas:

```python
def account_gas(tx, result):
    """
    Gas is always accounted, success or failure.
    """
    base_gas = get_base_gas(tx.type)

    if result.is_success():
        return base_gas + result.execution_gas
    else:
        # Failed transactions still use base gas + verification gas
        return base_gas + result.gas_used_before_failure
```

## 9. Test Vectors

Test vectors for failure scenarios are located in:
- `tck/vectors/errors/validation-errors.yaml`
- `tck/vectors/errors/resource-errors.yaml`
- `tck/vectors/errors/state-errors.yaml`

---

*Document Version: 1.0*
*Last Updated: 2026-02-03*
*Reference: MULTI_CLIENT_ALIGNMENT_SCHEME.md Section 2.D*
