# TOS Transaction Account Keys Design

**Goal**: Add explicit read/write account declarations to Transaction for better parallel execution

**Inspiration**: Solana's Message.account_keys pattern

---

## Problem Statement

**Current V3 Issue**: All accounts are treated as writable, causing over-conservative conflict detection.

```rust
// Current V3
TX1: Alice -> Bob (transfer)
Accounts: [Alice, Bob]  // Both treated as writable

TX2: Charlie -> Bob (transfer)
Accounts: [Charlie, Bob]  // Both treated as writable

Conflict detection: ❌ CONFLICT (Bob appears in both)
Reality: ✅ NO CONFLICT (Bob is just a recipient, can be written in parallel)
```

**Impact**:
- Lower parallelism than necessary
- Transfers to same recipient are unnecessarily serialized
- Contract read-only queries block writes

---

## Design Option A: Account Keys in Transaction (Recommended)

### Approach: Add account_keys field with read/write flags

```rust
// File: common/src/transaction/mod.rs

/// Account access mode for parallel execution
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum AccountAccess {
    /// Account will be modified (requires exclusive lock)
    Writable,
    /// Account will only be read (allows concurrent reads)
    ReadOnly,
}

/// Account key with access mode
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AccountKey {
    pub address: CompressedPublicKey,
    pub access: AccountAccess,
}

// Modified Transaction structure
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Transaction {
    version: TxVersion,
    source: CompressedPublicKey,
    data: TransactionType,
    fee: u64,
    fee_type: FeeType,
    nonce: Nonce,
    reference: Reference,
    multisig: Option<MultiSig>,
    signature: Signature,

    // NEW: Explicit account keys for parallel execution
    /// Accounts touched by this transaction with access modes
    /// Must be populated during transaction building
    /// Used for conflict detection in parallel execution
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    account_keys: Vec<AccountKey>,
}

impl Transaction {
    /// Get all writable accounts (for conflict detection)
    pub fn get_writable_accounts(&self) -> Vec<&CompressedPublicKey> {
        self.account_keys
            .iter()
            .filter(|key| key.access == AccountAccess::Writable)
            .map(|key| &key.address)
            .collect()
    }

    /// Get all readonly accounts
    pub fn get_readonly_accounts(&self) -> Vec<&CompressedPublicKey> {
        self.account_keys
            .iter()
            .filter(|key| key.access == AccountAccess::ReadOnly)
            .map(|key| &key.address)
            .collect()
    }

    /// Check if transaction touches an account (any mode)
    pub fn touches_account(&self, account: &CompressedPublicKey) -> bool {
        self.account_keys.iter().any(|key| &key.address == account)
    }

    /// Get access mode for an account
    pub fn get_account_access(&self, account: &CompressedPublicKey) -> Option<AccountAccess> {
        self.account_keys
            .iter()
            .find(|key| &key.address == account)
            .map(|key| key.access.clone())
    }
}
```

### Account Keys Population Rules

```rust
// File: common/src/transaction/builder/mod.rs

impl TransactionBuilder {
    /// Build account keys based on transaction type
    fn build_account_keys(&self) -> Vec<AccountKey> {
        let mut keys = Vec::new();

        // Source account is always writable (nonce increment + fee deduction)
        keys.push(AccountKey {
            address: self.source.clone(),
            access: AccountAccess::Writable,
        });

        match &self.data {
            TransactionType::Transfers(transfers) => {
                for transfer in transfers {
                    // Destination is writable (balance credit)
                    keys.push(AccountKey {
                        address: transfer.get_destination().clone(),
                        access: AccountAccess::Writable,
                    });
                }
            }

            TransactionType::Burn(_) => {
                // Only source account (already added)
            }

            TransactionType::InvokeContract(payload) => {
                // Contract account is readonly for state queries
                // or writable for state mutations
                let access = if payload.is_state_mutating() {
                    AccountAccess::Writable
                } else {
                    AccountAccess::ReadOnly
                };

                // Note: Contract is Hash, need to handle differently
                // For now, add to a separate contract_keys field
            }

            TransactionType::DeployContract(_) => {
                // Only source account (deployer)
            }

            TransactionType::Energy(payload) => {
                // Energy delegation: both accounts writable
                keys.push(AccountKey {
                    address: payload.get_delegate().clone(),
                    access: AccountAccess::Writable,
                });
            }

            TransactionType::MultiSig(_) => {
                // Only source account (multisig state update)
            }

            TransactionType::AIMining(_) => {
                // Only source account (reputation update)
            }
        }

        // Remove duplicates (keep most permissive access)
        Self::deduplicate_account_keys(keys)
    }

    /// Deduplicate account keys, keeping writable over readonly
    fn deduplicate_account_keys(keys: Vec<AccountKey>) -> Vec<AccountKey> {
        use std::collections::HashMap;

        let mut map: HashMap<CompressedPublicKey, AccountAccess> = HashMap::new();

        for key in keys {
            map.entry(key.address.clone())
                .and_modify(|access| {
                    // If any access is Writable, keep it as Writable
                    if key.access == AccountAccess::Writable {
                        *access = AccountAccess::Writable;
                    }
                })
                .or_insert(key.access);
        }

        map.into_iter()
            .map(|(address, access)| AccountKey { address, access })
            .collect()
    }
}
```

### Updated Conflict Detection

```rust
// File: daemon/src/core/executor/parallel_executor_v3.rs

impl ParallelExecutor {
    /// Extract accounts with read/write separation
    fn extract_account_conflicts(&self, tx: &Transaction) -> (Vec<PublicKey>, Vec<PublicKey>) {
        let writable = tx.get_writable_accounts()
            .into_iter()
            .cloned()
            .collect();

        let readonly = tx.get_readonly_accounts()
            .into_iter()
            .cloned()
            .collect();

        (writable, readonly)
    }

    /// Check if two transactions conflict
    fn has_conflict(&self, tx1: &Transaction, tx2: &Transaction) -> bool {
        let (w1, r1) = self.extract_account_conflicts(tx1);
        let (w2, r2) = self.extract_account_conflicts(tx2);

        // Conflict cases:
        // 1. Both write to same account
        let write_write_conflict = w1.iter().any(|acc| w2.contains(acc));

        // 2. One writes, other reads same account
        let write_read_conflict =
            w1.iter().any(|acc| r2.contains(acc)) ||
            w2.iter().any(|acc| r1.contains(acc));

        // 3. Both read same account - NO CONFLICT!
        // let read_read_ok = true;

        write_write_conflict || write_read_conflict
    }

    /// Group transactions into conflict-free batches (updated)
    fn group_by_conflicts(&self, transactions: &[Transaction]) -> Vec<Vec<(usize, Transaction)>> {
        use std::collections::{HashSet, HashMap};

        let mut batches = Vec::new();
        let mut current_batch = Vec::new();

        // Track locked accounts in current batch
        let mut write_locked: HashSet<PublicKey> = HashSet::new();
        let mut read_locked: HashMap<PublicKey, usize> = HashMap::new(); // Count readers

        for (index, tx) in transactions.iter().enumerate() {
            let (writable, readonly) = self.extract_account_conflicts(tx);

            // Check write conflicts
            let has_write_conflict = writable.iter().any(|acc| {
                write_locked.contains(acc) || read_locked.contains_key(acc)
            });

            // Check read conflicts
            let has_read_conflict = readonly.iter().any(|acc| {
                write_locked.contains(acc)
            });

            if has_write_conflict || has_read_conflict {
                // Start new batch
                if !current_batch.is_empty() {
                    batches.push(current_batch);
                    current_batch = Vec::new();
                    write_locked.clear();
                    read_locked.clear();
                }
            }

            // Add to current batch
            current_batch.push((index, tx.clone()));

            // Update locks
            for acc in writable {
                write_locked.insert(acc);
            }
            for acc in readonly {
                *read_locked.entry(acc).or_insert(0) += 1;
            }
        }

        // Add final batch
        if !current_batch.is_empty() {
            batches.push(current_batch);
        }

        batches
    }
}
```

### Example Usage

```rust
// Alice transfers to Bob
let tx1 = TransactionBuilder::new()
    .source(alice)
    .add_transfer(bob, 100)
    .build();

// Account keys automatically populated:
// tx1.account_keys = [
//     AccountKey { address: alice, access: Writable },  // Source (nonce + fee)
//     AccountKey { address: bob, access: Writable },    // Destination (balance credit)
// ]

// Charlie transfers to Bob (same destination!)
let tx2 = TransactionBuilder::new()
    .source(charlie)
    .add_transfer(bob, 200)
    .build();

// Account keys:
// tx2.account_keys = [
//     AccountKey { address: charlie, access: Writable },
//     AccountKey { address: bob, access: Writable },
// ]

// Conflict detection:
has_conflict(tx1, tx2)
// = check if tx1.writable ∩ tx2.writable != ∅
// = {alice, bob} ∩ {charlie, bob} = {bob}
// = ❌ CONFLICT! (both write to bob)

// BUT WAIT! This is still too conservative!
// Bob's balance update can actually be done in parallel
// because DashMap locks per (account, asset) pair
```

---

## The Deeper Issue: Balance Update Granularity

**Realization**: Even with account keys, we still have false conflicts!

```rust
TX1: Alice -> Bob (100 TOS)
TX2: Charlie -> Bob (200 TOS)

Both write to Bob's account, but they're updating DIFFERENT balance entries in DashMap:
- TX1 writes: balances[Bob][TOS]
- TX2 writes: balances[Bob][TOS]

Actually, they DO conflict on the same DashMap key!
```

**The Real Problem**: TOS's balance model is different from Solana's account model.

### Solana:
```rust
// Each account has a single data blob
Account {
    lamports: u64,      // Balance
    data: Vec<u8>,      // State
    owner: Pubkey,      // Program
}
// Writing to account.lamports = exclusive lock
```

### TOS:
```rust
// Each account has MULTIPLE balances (multi-asset)
balances: DashMap<PublicKey, HashMap<Hash, u64>>
//        account -> asset -> balance

// Writing to balances[Alice][TOS] and balances[Alice][USDT] = different locks!
```

---

## Solution: Finer-Grained Conflict Detection

### Option 1: Track (Account, Asset) Pairs

```rust
#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct AssetAccountKey {
    pub account: CompressedPublicKey,
    pub asset: Hash,
    pub access: AccountAccess,
}

impl Transaction {
    /// Get asset-specific writable keys
    pub fn get_writable_asset_accounts(&self) -> Vec<AssetAccountKey> {
        let mut keys = Vec::new();

        match &self.data {
            TransactionType::Transfers(transfers) => {
                for transfer in transfers {
                    // Source: writable for specific asset
                    keys.push(AssetAccountKey {
                        account: self.source.clone(),
                        asset: transfer.get_asset().clone(),
                        access: AccountAccess::Writable,
                    });

                    // Destination: writable for specific asset
                    keys.push(AssetAccountKey {
                        account: transfer.get_destination().clone(),
                        asset: transfer.get_asset().clone(),
                        access: AccountAccess::Writable,
                    });
                }
            }
            // ...
        }

        keys
    }
}

// Conflict detection:
fn has_asset_conflict(tx1: &Transaction, tx2: &Transaction) -> bool {
    let w1 = tx1.get_writable_asset_accounts();
    let w2 = tx2.get_writable_asset_accounts();

    // Only conflict if same (account, asset) pair
    w1.iter().any(|k1| w2.iter().any(|k2|
        k1.account == k2.account && k1.asset == k2.asset
    ))
}
```

**Example**:
```rust
TX1: Alice(TOS) -> Bob(TOS)
Asset keys: [(Alice, TOS, W), (Bob, TOS, W)]

TX2: Charlie(TOS) -> Bob(TOS)
Asset keys: [(Charlie, TOS, W), (Bob, TOS, W)]

Conflict: (Bob, TOS) appears in both ❌ STILL CONFLICTS!
```

**Realization**: This STILL doesn't help! Because both transactions write to `balances[Bob][TOS]`.

### Option 2: Accept the Conflict (Realistic Approach)

**Key insight**: Writing to the same account's balance for the same asset IS a real conflict in TOS!

```rust
// This is a REAL conflict in TOS:
TX1: Alice sends 100 TOS to Bob
TX2: Charlie sends 200 TOS to Bob

// Both need to:
// 1. Read Bob's current TOS balance
// 2. Add to it
// 3. Write back

// Without locking, race condition:
// Initial: Bob has 1000 TOS
// TX1 reads: 1000, computes: 1100, writes: 1100
// TX2 reads: 1000, computes: 1200, writes: 1200
// Final: Bob has 1200 TOS (lost 100!)

// Correct behavior (with locking):
// TX1: 1000 -> 1100 (write)
// TX2: 1100 -> 1300 (write)
// Final: Bob has 1300 TOS ✓
```

**Conclusion**: For balance credits to same (account, asset), serialization IS necessary!

---

## Revised Recommendation: Hybrid Approach

### 1. Add account_keys for contracts and read-only queries

```rust
// Useful cases:
TransactionType::InvokeContract(payload) => {
    if payload.is_view_function() {
        // Read-only query: no conflict!
        account_keys: [
            AccountKey { alice, Writable },   // Source (for fee)
            AccountKey { contract, ReadOnly }, // Just reading state
        ]
    } else {
        // State mutation
        account_keys: [
            AccountKey { alice, Writable },
            AccountKey { contract, Writable },
        ]
    }
}
```

### 2. Accept that transfer conflicts are real

```rust
// These SHOULD conflict:
TX1: Alice -> Bob (TOS)
TX2: Charlie -> Bob (TOS)

// DashMap already handles this correctly with entry locking!
self.balances.entry(bob).or_insert_with(HashMap::new)
    .entry(TOS)
    .and_modify(|b| *b += amount)  // ← This IS thread-safe!
```

**Wait, DashMap IS thread-safe for this!**

Let me reconsider...

---

## The Truth About DashMap

DashMap's `entry()` API provides **exclusive access** to the value:

```rust
// DashMap guarantees:
let mut entry = map.entry(key);  // ← Locks this key
entry.or_insert_with(...);       // ← Exclusive access
entry.and_modify(...);           // ← Exclusive access
// ← Lock released

// So this is SAFE:
self.balances.entry(bob)  // ← Locks balances[bob]
    .or_insert_with(HashMap::new)
    .entry(TOS)
    .and_modify(|b| *b += 100);  // ← Safe modification

// And this parallel execution is CORRECT:
Thread 1: balances[bob][TOS] += 100  // DashMap locks bob
Thread 2: balances[charlie][TOS] += 200  // Different key, no conflict
```

**But what about this?**
```rust
Thread 1: balances[bob][TOS] += 100
Thread 2: balances[bob][TOS] += 200

// DashMap behavior:
// - Thread 1 locks balances[bob]
// - Thread 2 blocks until Thread 1 releases
// - Sequential execution! ✓
```

**Conclusion**: DashMap AUTOMATICALLY serializes writes to the same key!

---

## Final Design Decision

### Option A: Minimal Change (Recommended for V3)

**DO NOT add account_keys field yet.**

**Reason**:
1. DashMap already handles balance conflicts correctly
2. Adding account_keys doesn't improve parallelism for transfers
3. Only useful for contracts (future work)

**Current V3 is correct as-is!**

### Option B: Add account_keys for Future (Contracts)

Add account_keys field, but only populate it for contracts:

```rust
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Transaction {
    // ... existing fields ...

    /// Account keys for parallel execution (optional)
    /// Only populated for contract transactions
    #[serde(default, skip_serializing_if = "Option::is_none")]
    account_keys: Option<Vec<AccountKey>>,
}
```

**Benefits**:
- Enables read-only contract queries to run in parallel
- Doesn't affect transfer performance (already optimal)
- Backward compatible (optional field)

---

## Performance Impact Analysis

### Current V3 (No account_keys)

```
10 parallel transfers to Bob:
TX1: Alice -> Bob
TX2: Charlie -> Bob
...
TX10: Zoe -> Bob

Conflict detection: All conflict (Bob appears in all)
Batching: 10 batches (sequential)
DashMap: Automatically serializes balances[Bob][TOS] updates
Result: Sequential execution (correct!)
```

### With account_keys (No improvement for transfers)

```
Same 10 transfers:
Conflict detection: All conflict (Bob is Writable in all)
Batching: 10 batches (sequential)
DashMap: Same behavior
Result: Sequential execution (same as before)
```

### With account_keys (Improvement for contract reads)

```
10 contract view calls to same contract:
TX1: Alice queries contract
TX2: Bob queries contract
...
TX10: Zoe queries contract

Without account_keys: Sequential (contract appears in all)
With account_keys (ReadOnly): Parallel! (read-read is OK)
Result: 10x speedup! ✓
```

---

## Recommendation

### Phase 1 (Current V3): Skip account_keys
- Current V3 is correct
- DashMap handles conflicts automatically
- Focus on implementing storage loading first

### Phase 2 (Contracts): Add account_keys
- Add optional account_keys field
- Only populate for contract transactions
- Enable parallel read-only queries

### Phase 3 (Optimization): Per-asset tracking
- If needed, track (account, asset) pairs
- Micro-optimization for multi-asset transfers

---

**Decision**: Start with Phase 1 (current V3 as-is), add account_keys in Phase 2 when implementing contracts.

**Next immediate task**: Implement storage loading (as planned).
