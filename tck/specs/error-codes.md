# Error Codes Specification

This document specifies the standardized error codes for the TOS protocol. All clients MUST use these exact error codes.

## 1. Error Code Structure

All error codes are standardized as u16 values:

```
+-------------------+---------------------------------------+
|  Category (u8)    |  Specific Error (u8)                  |
|  High byte        |  Low byte                             |
+-------------------+---------------------------------------+
```

## 2. Error Categories

| Category | Range | Description |
|----------|-------|-------------|
| 0x00 | 0x0000-0x00FF | Success / No error |
| 0x01 | 0x0100-0x01FF | Validation errors |
| 0x02 | 0x0200-0x02FF | Authorization errors |
| 0x03 | 0x0300-0x03FF | Resource errors |
| 0x04 | 0x0400-0x04FF | State errors |
| 0x05 | 0x0500-0x05FF | Contract errors |
| 0x06 | 0x0600-0x06FF | Network errors |
| 0xFF | 0xFF00-0xFFFF | Internal/Unknown errors |

## 3. Success Codes (0x00xx)

| Code | Name | Description |
|------|------|-------------|
| 0x0000 | SUCCESS | Transaction executed successfully |

## 4. Validation Errors (0x01xx)

| Code | Name | Description |
|------|------|-------------|
| 0x0100 | INVALID_FORMAT | Invalid wire format |
| 0x0101 | INVALID_VERSION | Unsupported protocol version |
| 0x0102 | INVALID_TYPE | Unknown transaction type |
| 0x0103 | INVALID_SIGNATURE | Signature verification failed |
| 0x0104 | INVALID_TIMESTAMP | Timestamp out of acceptable range |
| 0x0105 | INVALID_AMOUNT | Amount is zero or negative |
| 0x0106 | INVALID_ADDRESS | Malformed address |
| 0x0107 | INVALID_PAYLOAD | Payload validation failed |
| 0x0108 | INVALID_FEE | Fee format invalid |
| 0x0109 | INVALID_NONCE_FORMAT | Nonce format invalid |
| 0x010A | INVALID_HASH | Hash validation failed |
| 0x010B | INVALID_PUBKEY | Public key invalid |
| 0x0110 | NONCE_TOO_LOW | Nonce already used |
| 0x0111 | NONCE_TOO_HIGH | Nonce gap exceeds maximum |
| 0x0112 | NONCE_DUPLICATE | Duplicate nonce in mempool |
| 0x0113 | NONCE_MISMATCH | Nonce does not match expected |

## 5. Authorization Errors (0x02xx)

| Code | Name | Description |
|------|------|-------------|
| 0x0200 | UNAUTHORIZED | Sender not authorized for operation |
| 0x0201 | KYC_REQUIRED | Operation requires KYC verification |
| 0x0202 | KYC_LEVEL_TOO_LOW | Insufficient KYC level |
| 0x0203 | NOT_OWNER | Sender is not the owner |
| 0x0204 | NOT_COMMITTEE | Sender is not a KYC committee member |
| 0x0205 | NOT_ARBITRATOR | Sender is not an arbitrator |
| 0x0206 | MULTISIG_THRESHOLD | Insufficient multisig approvals |
| 0x0207 | PERMISSION_DENIED | General permission denied |
| 0x0208 | ACCOUNT_FROZEN | Account is frozen for compliance |
| 0x0209 | SIGNATURE_REQUIRED | Additional signature required |
| 0x020A | EXPIRED_AUTHORIZATION | Authorization has expired |

## 6. Resource Errors (0x03xx)

| Code | Name | Description |
|------|------|-------------|
| 0x0300 | INSUFFICIENT_BALANCE | Not enough liquid balance |
| 0x0301 | INSUFFICIENT_FEE | Fee too low for transaction |
| 0x0302 | INSUFFICIENT_ENERGY | Not enough energy |
| 0x0303 | INSUFFICIENT_FROZEN | Not enough frozen balance |
| 0x0304 | OVERFLOW | Arithmetic overflow |
| 0x0305 | UNDERFLOW | Arithmetic underflow |
| 0x0306 | LIMIT_EXCEEDED | Resource limit exceeded |
| 0x0307 | QUOTA_EXCEEDED | Rate quota exceeded |
| 0x0308 | SIZE_LIMIT | Data size limit exceeded |

## 7. State Errors (0x04xx)

| Code | Name | Description |
|------|------|-------------|
| 0x0400 | ACCOUNT_NOT_FOUND | Target account does not exist |
| 0x0401 | ACCOUNT_EXISTS | Account already exists |
| 0x0402 | ESCROW_NOT_FOUND | Escrow does not exist |
| 0x0403 | ESCROW_WRONG_STATE | Escrow in wrong state for operation |
| 0x0404 | DOMAIN_NOT_FOUND | TNS domain does not exist |
| 0x0405 | DOMAIN_EXISTS | TNS domain already registered |
| 0x0406 | DOMAIN_EXPIRED | TNS domain has expired |
| 0x0407 | DELEGATION_NOT_FOUND | Delegation does not exist |
| 0x0408 | DELEGATION_EXISTS | Delegation already exists |
| 0x0409 | SELF_OPERATION | Cannot perform operation on self |
| 0x040A | INVALID_STATE | Invalid state transition |
| 0x040B | PROPOSAL_NOT_FOUND | Proposal does not exist |
| 0x040C | PROPOSAL_EXPIRED | Proposal has expired |
| 0x040D | ALREADY_VOTED | Already voted on proposal |
| 0x040E | REFERRAL_NOT_FOUND | Referral code not found |
| 0x040F | REFERRAL_EXISTS | Referral code already exists |

## 8. Contract Errors (0x05xx)

| Code | Name | Description |
|------|------|-------------|
| 0x0500 | CONTRACT_NOT_FOUND | Contract does not exist |
| 0x0501 | CONTRACT_REVERT | Contract execution reverted |
| 0x0502 | OUT_OF_GAS | Contract exceeded gas limit |
| 0x0503 | INVALID_OPCODE | Unknown contract opcode |
| 0x0504 | STACK_OVERFLOW | Contract stack overflow |
| 0x0505 | STACK_UNDERFLOW | Contract stack underflow |
| 0x0506 | MEMORY_LIMIT | Contract exceeded memory limit |
| 0x0507 | CALL_DEPTH | Maximum call depth exceeded |
| 0x0508 | INVALID_JUMP | Invalid jump destination |
| 0x0509 | STATIC_VIOLATION | State modification in static call |
| 0x050A | CREATE_COLLISION | Contract address collision |
| 0x050B | INVALID_CODE | Invalid contract bytecode |
| 0x050C | CODE_SIZE_LIMIT | Contract code too large |
| 0x050D | INIT_CODE_FAILED | Contract initialization failed |

## 9. Network Errors (0x06xx)

| Code | Name | Description |
|------|------|-------------|
| 0x0600 | BLOCK_NOT_FOUND | Referenced block not found |
| 0x0601 | INVALID_PARENT | Invalid parent block reference |
| 0x0602 | INVALID_DIFFICULTY | Difficulty check failed |
| 0x0603 | INVALID_POW | Proof of work invalid |
| 0x0604 | TIMESTAMP_TOO_OLD | Block timestamp too old |
| 0x0605 | TIMESTAMP_TOO_NEW | Block timestamp in future |
| 0x0606 | ORPHAN_BLOCK | Block has unknown parent |
| 0x0607 | DUPLICATE_BLOCK | Block already exists |
| 0x0608 | INVALID_MERKLE_ROOT | Merkle root mismatch |
| 0x0609 | INVALID_STATE_ROOT | State root mismatch |

## 10. Internal Errors (0xFFxx)

| Code | Name | Description |
|------|------|-------------|
| 0xFF00 | INTERNAL_ERROR | Unexpected internal error |
| 0xFF01 | NOT_IMPLEMENTED | Feature not implemented |
| 0xFF02 | DATABASE_ERROR | Database operation failed |
| 0xFF03 | SERIALIZATION_ERROR | Serialization/deserialization failed |
| 0xFF04 | TIMEOUT | Operation timed out |
| 0xFFFF | UNKNOWN | Unknown error |

## 11. Error Code Usage

### In Transactions

Failed transactions record error code in receipt:
```
TransactionReceipt {
    txid: [32]u8,
    status: u8,       // 0 = success, 1 = failure
    error_code: u16,  // From this specification
    gas_used: u64,
}
```

### In RPC Responses

RPC errors include error code:
```json
{
    "jsonrpc": "2.0",
    "error": {
        "code": 769,
        "message": "INSUFFICIENT_BALANCE",
        "data": {
            "required": 1000,
            "available": 500
        }
    },
    "id": 1
}
```

### Error Code to Message Mapping

```python
ERROR_MESSAGES = {
    0x0000: "Success",
    0x0100: "Invalid wire format",
    0x0103: "Signature verification failed",
    0x0110: "Nonce already used",
    0x0300: "Insufficient balance",
    # ... etc
}

def get_error_message(code):
    return ERROR_MESSAGES.get(code, f"Unknown error: {hex(code)}")
```

## 12. Test Vectors

Test vectors for error codes are located in:
- `tck/vectors/errors/validation-errors.yaml`
- `tck/vectors/errors/resource-errors.yaml`
- `tck/vectors/errors/state-errors.yaml`
- `tck/vectors/errors/contract-errors.yaml`

---

*Document Version: 1.0*
*Last Updated: 2026-02-03*
*Reference: MULTI_CLIENT_ALIGNMENT_SCHEME.md Section 2.G*
