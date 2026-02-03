# Wire Format Specification

This document specifies the canonical binary serialization format for all TOS protocol data structures. All clients MUST implement these rules identically to maintain consensus.

## 1. Primitive Type Encoding

| Type | Size | Encoding | Notes |
|------|------|----------|-------|
| `u8` | 1 byte | Little-endian | Unsigned 8-bit integer |
| `u16` | 2 bytes | Little-endian | Unsigned 16-bit integer |
| `u32` | 4 bytes | Little-endian | Unsigned 32-bit integer |
| `u64` | 8 bytes | Little-endian | Unsigned 64-bit integer |
| `u128` | 16 bytes | Little-endian | Unsigned 128-bit integer |
| `i64` | 8 bytes | Little-endian (two's complement) | Signed 64-bit integer |
| `bool` | 1 byte | `0x00` = false, `0x01` = true | No other values allowed |

### Example: u64 encoding

```
Value: 1000000 (0xF4240)
Wire:  40 42 0F 00 00 00 00 00  (8 bytes, little-endian)
```

## 2. Variable-Length Encoding

### Length-Prefixed Bytes

For arbitrary data:

```
+------------------+---------------------------------+
|  Length (u32)    |  Data (length bytes)            |
|  4 bytes LE      |  variable                       |
+------------------+---------------------------------+
```

**Example: "Hello" (5 bytes)**
```
Wire: 05 00 00 00 48 65 6C 6C 6F
      [length  ] [data        ]
```

### Fixed-Size Arrays

For known sizes:
- No length prefix
- Elements concatenated directly
- Size known from context (e.g., hash = 32 bytes, pubkey = 33 bytes)

## 3. Optional Fields

Optional fields are encoded with a presence byte:

```
+-----------------+---------------------------------+
|  Present (u8)   |  Value (if present = 0x01)      |
|  0x00 or 0x01   |  type-specific encoding         |
+-----------------+---------------------------------+
```

**Example: Optional u64**
```
None:    00
Some(5): 01 05 00 00 00 00 00 00 00
```

## 4. Collections

### Vectors (variable-length lists)

```
+------------------+---------------------------------+
|  Count (u32)     |  Elements (count x element_size)|
|  4 bytes LE      |  concatenated                   |
+------------------+---------------------------------+
```

### Maps (key-value pairs)

```
+------------------+---------------------------------------------+
|  Count (u32)     |  Entries (count x (key_size + value_size)) |
|  4 bytes LE      |  sorted by key, concatenated               |
+------------------+---------------------------------------------+
```

**CRITICAL**: Map entries MUST be sorted by key in lexicographic order to ensure canonical encoding.

## 5. Account Address Encoding

Account addresses are 21 bytes:

```
+-----------------+---------------------------------+
|  Type (u8)      |  Hash (20 bytes)                |
|  Account type   |  RIPEMD160(SHA256(pubkey))      |
+-----------------+---------------------------------+
```

### Account Types

| Type Byte | Account Type |
|-----------|--------------|
| `0x01` | Standard (single-sig) |
| `0x02` | MultiSig |
| `0x03` | Contract |
| `0x04` | Agent |
| `0x05` | System |

## 6. Transaction Envelope

All transactions share a common envelope structure:

```
+-----------------------------------------------------------+
|                    Transaction Envelope                    |
+-----------------+-----------------------------------------+
|  version (u8)   |  Protocol version (currently 0x01)      |
+-----------------+-----------------------------------------+
|  type (u8)      |  Transaction type (0-255)               |
+-----------------+-----------------------------------------+
|  sender (21)    |  Account address of sender              |
+-----------------+-----------------------------------------+
|  nonce (u64)    |  Sender's transaction sequence number   |
+-----------------+-----------------------------------------+
|  fee (u64)      |  Fee in base units                      |
+-----------------+-----------------------------------------+
|  timestamp (u64)|  Unix timestamp in milliseconds         |
+-----------------+-----------------------------------------+
|  payload        |  Type-specific payload (variable)       |
+-----------------+-----------------------------------------+
|  signature      |  Signature over envelope (64-65 bytes)  |
+-----------------+-----------------------------------------+
```

**Envelope header size**: 1 + 1 + 21 + 8 + 8 + 8 = **47 bytes** (fixed)

## 7. Transaction Type Catalog

| Type | Name | Payload Size | Description |
|------|------|--------------|-------------|
| 0 | Burn | 8 | Destroy tokens |
| 1 | Transfer | 29 | Send tokens to address |
| 2 | TransferMulti | Variable | Send to multiple addresses |
| 3 | Genesis | Variable | Network initialization (block 0 only) |
| 4 | Coinbase | 0 | Block reward (block producer only) |
| 5 | EnergyBuy | 8 | Convert tokens to energy |
| 6 | EnergyRefund | 8 | Convert energy back to tokens |
| 7 | Delegate | 29 | Delegate energy to address |
| 8 | Undelegate | 29 | Remove energy delegation |
| 9 | SetKyc | Variable | Set KYC verification level |
| 10 | RevokeKyc | 21 | Revoke KYC verification |
| 11 | RenewKyc | Variable | Extend KYC expiration |
| 12 | RegisterCommittee | Variable | Register as KYC committee |
| 13 | DeregisterCommittee | 0 | Unregister KYC committee |
| 14 | UpdateCommittee | Variable | Update committee parameters |
| 15 | BootstrapKyc | Variable | Initial KYC setup |
| 16 | KycApprove | Variable | Committee approves KYC |
| 17 | KycReject | Variable | Committee rejects KYC |
| 24 | EscrowCreate | Variable | Create escrow |
| 25 | EscrowDeposit | Variable | Deposit to escrow |
| 26 | EscrowRelease | Variable | Release escrow funds |
| 27 | EscrowRefund | Variable | Refund escrow |
| 28 | EscrowChallenge | Variable | Challenge escrow release |
| 29 | EscrowVerdict | Variable | Arbitrator verdict |
| 30 | EscrowAppeal | Variable | Appeal verdict |
| 31 | EscrowResolve | Variable | Final resolution |
| 32 | EscrowCancel | Variable | Cancel escrow |
| 40 | TNSRegister | Variable | Register domain name |
| 41 | TNSTransfer | Variable | Transfer domain |
| 42 | TNSUpdate | Variable | Update domain record |
| 43 | TNSExtend | Variable | Extend domain registration |
| 44 | TNSRelease | Variable | Release domain |
| 50 | ContractDeploy | Variable | Deploy smart contract |
| 51 | ContractCall | Variable | Call contract method |
| 52 | ContractUpgrade | Variable | Upgrade contract |
| 60 | ReferralRegister | Variable | Register referral code |
| 61 | ReferralActivate | Variable | Activate with referral |
| 62 | ReferralClaim | Variable | Claim referral rewards |
| 70 | AgentCreate | Variable | Create agent account |
| 71 | AgentUpdate | Variable | Update agent parameters |
| 72 | AgentRevoke | Variable | Revoke agent permissions |
| 80 | MultiSigCreate | Variable | Create multisig account |
| 81 | MultiSigPropose | Variable | Propose transaction |
| 82 | MultiSigApprove | Variable | Approve proposal |
| 83 | MultiSigExecute | Variable | Execute approved proposal |
| 84 | MultiSigReject | Variable | Reject proposal |
| 90 | UnoTransfer | Variable | Private transfer (ZK) |
| 91 | UnoMint | Variable | Mint private tokens |
| 92 | UnoBurn | Variable | Burn private tokens |

## 8. Wire Format Examples

### Transfer Transaction (Type 1)

Payload structure:
```
+-----------------+-----------------------------------------+
|  recipient (21) |  Destination account address            |
+-----------------+-----------------------------------------+
|  amount (u64)   |  Transfer amount in base units          |
+-----------------+-----------------------------------------+
```

Complete wire format (without signature):
```
Offset  Size  Field         Example Value
------  ----  -----         -------------
0       1     version       01
1       1     type          01 (Transfer)
2       21    sender        01 + <20 bytes hash>
23      8     nonce         05 00 00 00 00 00 00 00 (5)
31      8     fee           01 00 00 00 00 00 00 00 (1)
39      8     timestamp     00 E8 76 48 17 01 00 00 (ms since epoch)
47      21    recipient     01 + <20 bytes hash>
68      8     amount        64 00 00 00 00 00 00 00 (100)
------
Total: 76 bytes (+ signature)
```

### Burn Transaction (Type 0)

Payload structure:
```
+-----------------+-----------------------------------------+
|  amount (u64)   |  Amount to burn in base units           |
+-----------------+-----------------------------------------+
```

Complete wire format:
```
Offset  Size  Field         Example Value
------  ----  -----         -------------
0       1     version       01
1       1     type          00 (Burn)
2       21    sender        01 + <20 bytes hash>
23      8     nonce         05 00 00 00 00 00 00 00
31      8     fee           01 00 00 00 00 00 00 00
39      8     timestamp     00 E8 76 48 17 01 00 00
47      8     amount        E8 03 00 00 00 00 00 00 (1000)
------
Total: 55 bytes (+ signature)
```

---

*Document Version: 1.0*
*Last Updated: 2026-02-03*
*Reference: MULTI_CLIENT_ALIGNMENT_SCHEME.md Section 2.A*
