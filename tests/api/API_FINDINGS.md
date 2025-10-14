# TOS Daemon API - Actual Implementation Details

**Date**: 2025-10-14
**Purpose**: Document the actual API parameter and response structures discovered from code analysis

## Key Findings

### 1. Network Field Serialization
**File**: `common/src/network.rs:70-79`

```rust
impl Display for Network {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let str = match &self {
            Self::Mainnet => "Mainnet",
            Self::Testnet => "Testnet",
            Self::Stagenet => "Stagenet",
            Self::Devnet => "Dev"  // ← Returns "Dev", not "devnet"
        };
        write!(f, "{}", str)
    }
}
```

**Impact**: `get_info` API returns `network: "Dev"` for devnet, not `"devnet"`

**Test Fix**: Update test to expect `"Dev"` instead of `"devnet"`

---

### 2. Difficulty Type Serialization
**File**: `common/src/varuint.rs:242-246`

```rust
impl Serialize for VarUint {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())  // ← Serializes as STRING
    }
}
```

**File**: `common/src/difficulty.rs:9`
```rust
pub type Difficulty = VarUint;
```

**Impact**: Difficulty field returns string "1011", not integer 1011

**Test Fix**: Update test to expect string type for difficulty

---

### 3. Balance API Parameters
**File**: `common/src/api/daemon/mod.rs:245-249`

#### GetBalanceParams
```rust
#[derive(Serialize, Deserialize)]
pub struct GetBalanceParams<'a> {
    pub address: Cow<'a, Address>,
    pub asset: Cow<'a, Hash>  // ← REQUIRED, not optional
}
```

**Impact**: `get_balance` requires BOTH address AND asset hash

**Test Fix**:
```python
# OLD (wrong):
client.call("get_balance", [address])

# NEW (correct):
client.call("get_balance", {"address": address, "asset": TOS_ASSET_HASH})
```

#### GetBalanceAtTopoHeightParams
```rust
#[derive(Serialize, Deserialize)]
pub struct GetBalanceAtTopoHeightParams<'a> {
    pub address: Cow<'a, Address>,
    pub asset: Cow<'a, Hash>,
    pub topoheight: TopoHeight  // ← 3 parameters required
}
```

**Impact**: Requires 3 parameters: address, asset, topoheight

**Test Fix**:
```python
# OLD (wrong):
client.call("get_balance_at_topoheight", [address, topoheight])

# NEW (correct):
client.call("get_balance_at_topoheight", {
    "address": address,
    "asset": TOS_ASSET_HASH,
    "topoheight": topoheight
})
```

#### HasBalanceParams
```rust
#[derive(Serialize, Deserialize)]
pub struct HasBalanceParams<'a> {
    pub address: Cow<'a, Address>,
    pub asset: Cow<'a, Hash>,
    #[serde(default)]
    pub topoheight: Option<TopoHeight>
}
```

**Test Fix**:
```python
client.call("has_balance", {
    "address": address,
    "asset": TOS_ASSET_HASH
})
```

---

### 4. Nonce API Parameters
**File**: `common/src/api/daemon/mod.rs:272-287`

#### GetNonceParams
```rust
#[derive(Serialize, Deserialize)]
pub struct GetNonceParams<'a> {
    pub address: Cow<'a, Address>
}
```

#### GetNonceResult
```rust
#[derive(Serialize, Deserialize)]
pub struct GetNonceResult {
    pub topoheight: TopoHeight,
    #[serde(flatten)]
    pub version: VersionedNonce  // ← Flattened, contains nonce field
}
```

**Test Fix**:
```python
# OLD (wrong):
client.call("get_nonce", [address])

# NEW (correct):
client.call("get_nonce", {"address": address})
```

**Response structure**: Nonce may be returned as string or in versioned format

---

### 5. Block Response Structure
**File**: `common/src/api/daemon/mod.rs:73-103`

```rust
#[derive(Serialize, Deserialize)]
pub struct RPCBlockResponse<'a> {
    pub hash: Cow<'a, Hash>,
    pub topoheight: Option<TopoHeight>,
    pub block_type: BlockType,
    pub difficulty: Cow<'a, Difficulty>,
    // ... all fields at top level (flat structure)
    pub timestamp: TimestampMillis,
    pub height: u64,
    pub nonce: Nonce,
    pub extra_nonce: Cow<'a, [u8; EXTRA_NONCE_SIZE]>,
    pub miner: Cow<'a, Address>,
    pub txs_hashes: Cow<'a, IndexSet<Hash>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transactions: Vec<RPCTransaction<'a>>,  // ← Optional, flat
}
```

**Impact**: Block structure is FLAT, not nested with separate `header` and `transactions`

**Test Fix**:
```python
# OLD (wrong):
assert "header" in block
assert block["header"]["hash"]

# NEW (correct):
assert "hash" in block
assert block["hash"]
assert "topoheight" in block
assert "transactions" in block  # Optional, may be empty
```

---

### 6. Peer List Response Structure
**File**: `common/src/api/daemon/mod.rs:380-406`

```rust
#[derive(Serialize, Deserialize)]
pub struct GetPeersResponse<'a> {
    pub peers: Vec<PeerEntry<'a>>,  // ← Nested inside object
    pub total_peers: usize,
    pub hidden_peers: usize
}
```

**Impact**: get_peers returns object with metadata, not direct array

**Test Fix**:
```python
# OLD (wrong):
result = client.call("get_peers", [])
assert isinstance(result, list)

# NEW (correct):
result = client.call("get_peers", [])
assert "peers" in result
assert "total_peers" in result
assert "hidden_peers" in result
assert isinstance(result["peers"], list)
```

---

### 7. Account History Parameters
**File**: `common/src/api/daemon/mod.rs:456-468`

```rust
#[derive(Serialize, Deserialize)]
pub struct GetAccountHistoryParams {
    pub address: Address,
    #[serde(default = "default_tos_asset")]
    pub asset: Hash,  // ← Has default value
    pub minimum_topoheight: Option<TopoHeight>,
    pub maximum_topoheight: Option<TopoHeight>,
    #[serde(default = "default_true_value")]
    pub incoming_flow: bool,  // ← Defaults to true
    #[serde(default = "default_true_value")]
    pub outgoing_flow: bool,  // ← Defaults to true
}
```

**Test Fix**:
```python
# OLD (wrong):
client.call("get_account_history", [address])

# NEW (correct):
client.call("get_account_history", {
    "address": address
    # asset, incoming_flow, outgoing_flow have defaults
})
```

---

### 8. Mempool API Parameters

**Finding**: `get_mempool` expects no parameters (empty array or object)

**Test Fix**:
```python
# OLD (wrong):
client.call("get_mempool", [False])

# NEW (correct):
client.call("get_mempool", [])
# or
client.call("get_mempool", {})
```

---

### 9. Utility API Response Structures

#### get_difficulty Response
**Expected**: Returns object with multiple fields

```python
{
    "difficulty": "1049",  # String, not int
    "hashrate": "1049",
    "hashrate_formatted": "1.05 KH/s"
}
```

**Test Fix**:
```python
# OLD (wrong):
assert isinstance(result, int)

# NEW (correct):
assert isinstance(result, dict)
assert "difficulty" in result
assert isinstance(result["difficulty"], str)
```

#### get_size_on_disk Response
**Expected**: Returns object with formatted size

```python
{
    "size_bytes": 45323768,
    "size_formatted": "43.2 MiB"
}
```

**Test Fix**:
```python
# OLD (wrong):
assert isinstance(result, str)

# NEW (correct):
assert isinstance(result, dict)
assert "size_bytes" in result
assert "size_formatted" in result
```

---

## TOS Asset Hash Constant

Based on code analysis, the native TOS asset needs to be referenced. From API defaults:

```rust
fn default_tos_asset() -> Hash {
    crate::config::TOS_ASSET
}
```

**Action**: Need to find the actual TOS_ASSET hash value or use the string identifier "tos"

**From balance API test failure**:
```
AssertionError: assert 'tos' == '0000000000000000000000000000000000000000000000000000000000000000'
```

**Conclusion**: Native TOS asset can be referenced with string `"tos"` instead of the zero hash

---

## Parameter Format Summary

### JSON-RPC Parameter Formats

Most TOS daemon APIs expect **named object parameters**, not positional arrays:

#### ✅ CORRECT Format (Named Object):
```json
{
    "jsonrpc": "2.0",
    "method": "get_balance",
    "params": {
        "address": "tst1...",
        "asset": "tos"
    },
    "id": 1
}
```

#### ❌ WRONG Format (Positional Array):
```json
{
    "jsonrpc": "2.0",
    "method": "get_balance",
    "params": ["tst1..."],
    "id": 1
}
```

### Exceptions (Arrays OK):
- `get_info` - no parameters: `[]`
- `p2p_status` - no parameters: `[]`
- `get_version` - no parameters: `[]`
- `get_blocks_range_by_topoheight` - array: `[start, end]`
- `get_blocks_range_by_blue_score` - array: `[start, end]`

---

## Test Updates Required

### High Priority (Many Failures)
1. ✅ **test_balance_apis.py** - Fix all parameter formats to use named objects
2. ✅ **test_block_apis.py** - Update to flat block structure (no nested header)
3. ✅ **test_ghostdag_apis.py** - Update to flat block structure
4. ✅ **test_network_apis.py** - Fix peer list and mempool parameters

### Medium Priority (Type Fixes)
5. ✅ **test_get_info.py** - Fix network field (expect "Dev") and difficulty type (expect string)
6. ✅ **test_utility_apis.py** - Fix response structure expectations

### Low Priority (Documentation)
7. ✅ Update `DAEMON_RPC_API_REFERENCE.md` with correct formats
8. ✅ Update `lib/rpc_client.py` helper methods

---

## Next Steps

1. **Fix test files** - Update all tests to use correct parameter formats
2. **Add TOS_ASSET constant** - Define native asset identifier in config
3. **Update RPC client** - Add helper methods that format parameters correctly
4. **Rerun tests** - Verify fixes work
5. **Update documentation** - Reflect actual API behavior

**Estimated time**: 3-4 hours
