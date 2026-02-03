# State Digest Specification

This document specifies the canonical state digest format for the TOS protocol. All clients MUST produce identical state digests for the same state.

## 1. Purpose

The state digest provides a canonical, deterministic representation of blockchain state for:
- Cross-client state comparison (testing)
- State synchronization verification
- Checkpoint validation

**Note**: State digest is primarily for testing and verification. Production clients may use different internal representations as long as they produce matching digests.

## 2. Canonical State Representation

The state is serialized in a deterministic order:

```
+-----------------------------------------------------------+
|                      State Digest                          |
+-----------------+-----------------------------------------+
|  version (u8)   |  State format version (currently 0x01)  |
+-----------------+-----------------------------------------+
|  block_hash(32) |  Hash of the block this state follows   |
+-----------------+-----------------------------------------+
|  account_count  |  Number of accounts (u32)               |
+-----------------+-----------------------------------------+
|  accounts       |  Sorted account states (see below)      |
+-----------------+-----------------------------------------+
|  global_state   |  Global protocol state (see below)      |
+-----------------+-----------------------------------------+
```

## 3. Account State Encoding

Each account is encoded as:

```
+-----------------+-----------------------------------------+
|  address (21)   |  Account address                        |
+-----------------+-----------------------------------------+
|  balance (u64)  |  Liquid balance                         |
+-----------------+-----------------------------------------+
|  nonce (u64)    |  Transaction counter                    |
+-----------------+-----------------------------------------+
|  frozen (u64)   |  Frozen (staked) balance                |
+-----------------+-----------------------------------------+
|  energy (u64)   |  Available energy                       |
+-----------------+-----------------------------------------+
|  flags (u32)    |  Account flags (KYC level, etc.)        |
+-----------------+-----------------------------------------+
|  data_len (u32) |  Length of additional data              |
+-----------------+-----------------------------------------+
|  data           |  Type-specific data (contracts, etc.)   |
+-----------------+-----------------------------------------+
```

### Account Sorting

**CRITICAL**: Accounts MUST be sorted by address in lexicographic (byte) order.

```python
def sort_accounts(accounts):
    return sorted(accounts, key=lambda a: a.address)
```

### Account Flags

| Bit | Flag | Description |
|-----|------|-------------|
| 0-2 | KYC_LEVEL | KYC verification level (0-7) |
| 3 | IS_CONTRACT | Account is a contract |
| 4 | IS_AGENT | Account is an agent |
| 5 | IS_MULTISIG | Account is a multisig |
| 6 | IS_FROZEN | Account is frozen (compliance) |
| 7-31 | RESERVED | Reserved for future use |

## 4. Global State Encoding

```
+-----------------+-----------------------------------------+
|  total_supply   |  Total token supply (u128)              |
+-----------------+-----------------------------------------+
|  total_burned   |  Total tokens burned (u128)             |
+-----------------+-----------------------------------------+
|  total_energy   |  Total network energy (u128)            |
+-----------------+-----------------------------------------+
|  block_height   |  Current block height (u64)             |
+-----------------+-----------------------------------------+
|  timestamp      |  Block timestamp (u64)                  |
+-----------------+-----------------------------------------+
```

## 5. Digest Computation

```python
def compute_state_digest(state, block_hash):
    """
    Compute canonical state digest.
    Returns: SHA3-256 hash (32 bytes)
    """
    buffer = bytearray()

    # Header
    buffer.append(0x01)  # version
    buffer.extend(block_hash)  # 32 bytes

    # Accounts (sorted)
    accounts = sorted(state.accounts.items(), key=lambda x: x[0])
    buffer.extend(len(accounts).to_bytes(4, 'little'))

    for address, account in accounts:
        buffer.extend(address)  # 21 bytes
        buffer.extend(account.balance.to_bytes(8, 'little'))
        buffer.extend(account.nonce.to_bytes(8, 'little'))
        buffer.extend(account.frozen.to_bytes(8, 'little'))
        buffer.extend(account.energy.to_bytes(8, 'little'))
        buffer.extend(account.flags.to_bytes(4, 'little'))
        buffer.extend(len(account.data).to_bytes(4, 'little'))
        buffer.extend(account.data)

    # Global state
    buffer.extend(state.total_supply.to_bytes(16, 'little'))
    buffer.extend(state.total_burned.to_bytes(16, 'little'))
    buffer.extend(state.total_energy.to_bytes(16, 'little'))
    buffer.extend(state.block_height.to_bytes(8, 'little'))
    buffer.extend(state.timestamp.to_bytes(8, 'little'))

    # Compute digest
    return sha3_256(bytes(buffer))
```

## 6. Example

### State Before Encoding

```yaml
block_hash: "abc123..."
accounts:
  - address: "01aaaaaa..."
    balance: 1000
    nonce: 5
    frozen: 0
    energy: 100
    flags: 0
    data: ""
  - address: "01bbbbbb..."
    balance: 500
    nonce: 3
    frozen: 200
    energy: 50
    flags: 1
    data: ""
global:
  total_supply: 1000000000
  total_burned: 1000
  total_energy: 500000
  block_height: 12345
  timestamp: 1700000000000
```

### Encoded Bytes (hex)

```
01                                          # version
abc123...                                   # block_hash (32 bytes)
02 00 00 00                                 # account_count (2)

# Account 1 (01aaaaaa...)
01 aa aa aa ...                             # address (21 bytes)
e8 03 00 00 00 00 00 00                     # balance (1000)
05 00 00 00 00 00 00 00                     # nonce (5)
00 00 00 00 00 00 00 00                     # frozen (0)
64 00 00 00 00 00 00 00                     # energy (100)
00 00 00 00                                 # flags (0)
00 00 00 00                                 # data_len (0)

# Account 2 (01bbbbbb...)
01 bb bb bb ...                             # address (21 bytes)
f4 01 00 00 00 00 00 00                     # balance (500)
03 00 00 00 00 00 00 00                     # nonce (3)
c8 00 00 00 00 00 00 00                     # frozen (200)
32 00 00 00 00 00 00 00                     # energy (50)
01 00 00 00                                 # flags (1)
00 00 00 00                                 # data_len (0)

# Global state
00 ca 9a 3b 00 00 00 00 00 00 00 00 00 00 00 00  # total_supply
e8 03 00 00 00 00 00 00 00 00 00 00 00 00 00 00  # total_burned
20 a1 07 00 00 00 00 00 00 00 00 00 00 00 00 00  # total_energy
39 30 00 00 00 00 00 00                          # block_height
00 90 85 a4 8c 01 00 00                          # timestamp
```

### Final Digest

```
state_digest = SHA3-256(encoded_bytes)
             = "def456..."  (32 bytes)
```

## 7. Incremental State Updates

For efficiency, clients may use incremental updates:

```python
def update_state_digest(old_digest, changes):
    """
    Incremental update (optional optimization).
    Must produce same result as full computation.
    """
    # Implementation-specific optimization
    # Result must match compute_state_digest()
    pass
```

## 8. Empty State

Empty state (genesis - 1) has a defined digest:

```python
def empty_state_digest():
    """
    Digest of empty state (before genesis).
    """
    buffer = bytearray()
    buffer.append(0x01)  # version
    buffer.extend(bytes(32))  # zero block_hash
    buffer.extend((0).to_bytes(4, 'little'))  # 0 accounts
    buffer.extend((0).to_bytes(16, 'little'))  # total_supply
    buffer.extend((0).to_bytes(16, 'little'))  # total_burned
    buffer.extend((0).to_bytes(16, 'little'))  # total_energy
    buffer.extend((0).to_bytes(8, 'little'))   # block_height
    buffer.extend((0).to_bytes(8, 'little'))   # timestamp
    return sha3_256(bytes(buffer))
```

## 9. Test Vectors

Test vectors for state digest are located in:
- `tck/vectors/state/digest-basic.yaml`
- `tck/vectors/state/digest-accounts.yaml`
- `tck/vectors/state/digest-global.yaml`

---

*Document Version: 1.0*
*Last Updated: 2026-02-03*
*Reference: MULTI_CLIENT_ALIGNMENT_SCHEME.md Section 2.F*
