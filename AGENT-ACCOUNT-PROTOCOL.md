# TOS Agent Account (Protocol-Level Extension)

Purpose: define a protocol-level agent account type for TOS that extends account authentication and policy enforcement in consensus, while preserving existing EOA transaction semantics and tooling.

Vision: enable safe, autonomous agents to transact under strict, verifiable constraints (policy + session keys + owner override), so the network can support high-frequency machine activity without compromising accountability or security.

This document defines a protocol-level agent account type (AA variant) for TOS. It keeps EOA compatibility while adding policy-based validation and session keys in consensus.

## 1) Account Type and Storage ✅

Use a logical `AccountType::AgentAccount` derived from `AgentAccountMeta` (no stored enum). ✅

### 1.1 AgentAccountMeta ✅
```
AgentAccountMeta {
  owner: PublicKey,        // Human owner (override authority)
  controller: PublicKey,   // Agent execution key
  policy_hash: Hash,       // Policy config hash or on-chain policy reference
  status: u8,              // 0=active, 1=frozen
  energy_pool: PublicKey,  // Optional budget/energy payer (owner or controller)
  session_key_root: Hash   // Optional Merkle root for session keys
}
```

### 1.2 New storage columns ✅
- `AccountMeta` (account_id -> AgentAccountMeta)
- `SessionKeys` (account_id + key_id -> SessionKey)

### 1.3 AccountType Detection and Use ✅
Account type is derived from storage, not a separate on-chain enum field:
- **Normal account**: no `AgentAccountMeta` entry in `AccountMeta`.
- **Agent account**: `AgentAccountMeta` exists for the account.

Usage in validation:
- If `AgentAccountMeta` exists, treat sender as `AccountType::AgentAccount` and enforce agent auth rules.
- If not, treat as standard account and use legacy auth rules.

Implementation note:
- `AccountType::AgentAccount` is a logical label used in code paths, not stored directly.

## 2) Transaction Model (Consensus-Level) ✅

Agent accounts reuse the existing transaction `signature` field (no new auth fields or TxVersion changes).

Validation flow when `AccountType::AgentAccount`:
1) Verify the single `signature` against `owner`, `controller`, or any valid `session key`.
2) If `signature` matches `owner`, treat as owner auth (admin override).
3) If `signature` matches `controller` or a session key, enforce session-key constraints.

### 2.1 Upgrade Transaction (register_agent_account) ✅
Protocol-level payload (no contract):
```
RegisterAgentAccountPayload {
  controller: PublicKey,
  policy_hash: Hash,
  energy_pool: Option<PublicKey>,
  session_key_root: Option<Hash>
}
```

Sender rules:
- The transaction sender becomes `owner`.
- Sender must be a normal account (no existing `AgentAccountMeta`).

State writes:
- Insert `AgentAccountMeta` under `AccountMeta` for the sender account.
- Set `status=active`, `controller`, `policy_hash`, `energy_pool`, `session_key_root`.

### 2.2 Upgrade Validation Logic (Consensus) ✅
Pseudo flow:
```
if sender.has_agent_meta():
  return ErrAlreadyAgentAccount

if controller == sender:
  return ErrInvalidController

if policy_hash == 0:
  return ErrInvalidPolicy

if energy_pool.is_some() and !account_exists(energy_pool):
  return ErrEnergyPoolNotFound

write AgentAccountMeta { owner=sender, controller, policy_hash,
                         status=active, energy_pool, session_key_root }
```

Notes:
- The upgrade is reversible by owner-only `set_agent_account_meta` (status=frozen or clear meta).
- Controller rotation uses a separate protocol payload (`rotate_agent_controller`).

### 2.3 AgentAccount Payloads (Protocol-Level) ✅
Add a new payload module:
- `common/src/transaction/payload/agent_account/*`
- wire into `common/src/transaction/payload/mod.rs`

```
enum AgentAccountPayload {
  Register {
    controller: PublicKey,
    policy_hash: Hash,
    energy_pool: Option<PublicKey>,
    session_key_root: Option<Hash>
  },
  UpdatePolicy {
    policy_hash: Hash
  },
  RotateController {
    new_controller: PublicKey
  },
  SetStatus {
    status: u8
  },
  SetEnergyPool {
    energy_pool: Option<PublicKey>
  },
  SetSessionKeyRoot {
    session_key_root: Option<Hash>
  },
  AddSessionKey {
    key: SessionKey
  },
  RevokeSessionKey {
    key_id: u64
  }
}
```

Admin-only payloads (must have valid owner signature): ✅
- `UpdatePolicy`
- `RotateController`
- `SetStatus`
- `SetEnergyPool`
- `SetSessionKeyRoot`
- `AddSessionKey`
- `RevokeSessionKey`

### 2.4 Auth Rules (Formal) ✅
For `AccountType::AgentAccount`:
- Single `signature` must verify against `owner`, `controller`, or a valid session key.
- Owner signature can submit admin payloads and bypass session-key constraints.
- Controller/session signatures cannot submit admin-only payloads.
- Session key signatures must match a registered, non-expired session key.

### 2.5 Parameter Checks (Formal) ✅
All checks must run before any state writes.

Common checks:
- Reject zero keys: `owner`, `controller`, `energy_pool` must be non-zero.
- `policy_hash` and `session_key_root` if present must be non-zero.
- `energy_pool` if present must be either `owner` or `controller`.
- `status` must be in {0,1} (active/frozen).
- `kyc_tier` is read from native KYC module, not stored in AgentAccountMeta.

Register checks:
- Account must not already have `AgentAccountMeta`.
- `controller != owner`.
- `energy_pool` if present must exist and be registered.
- `session_key_root` if present must not be zero.

UpdatePolicy checks:
- `policy_hash != 0`.

RotateController checks:
- `new_controller != owner`.
- `new_controller != controller`.

SetEnergyPool checks:
- If set, `energy_pool` must exist and be registered.

SetSessionKeyRoot checks:
- If set, `session_key_root != 0`.
- If set, there must be no active session keys (avoid mixed modes).

AddSessionKey checks:
- `key_id` must be unique for this account.
- `public_key != 0`.
- `public_key` must be unique for this account.
- `expiry_topoheight > current_topoheight`.
- `max_value_per_window > 0`.
- `allowed_targets.len <= MAX_ALLOWED_TARGETS`.
- `allowed_assets.len <= MAX_ALLOWED_ASSETS`.
- total active session keys per account must be `<= MAX_SESSION_KEYS_PER_ACCOUNT`.

RevokeSessionKey checks:
- `key_id` must exist.

### 2.6 Business Logic Checks (Formal) ✅
Apply after signature validation:
- If `status=frozen`, only admin payloads with owner signature are allowed.
- If signed by a session key, enforce its limits (targets/assets/max_value_per_window per tx).
- Non-energy fees count toward session-key spend and asset checks when charged to the source
  (fees covered by `energy_pool` are excluded from the session-key spend).

## 3) Policy Validation Rules ✅

`policy_hash` is an opaque reference for off-chain policy and audit trails.  
Consensus-level enforcement is limited to session-key constraints.

## 4) Session Key Mechanism ✅

### 4.1 SessionKey structure ✅
```
SessionKey {
  key_id: u64,
  public_key: PublicKey,
  expiry_topoheight: u64,
  max_value_per_window: u64,
  allowed_targets: Vec<PublicKey>,
  allowed_assets: Vec<Hash>
}
```

### 4.2 Semantics ✅
- Short-lived, scoped keys for automation.
- Transactions signed by session key are accepted only if scope limits are met.
- Mitigates risk of leaking controller or owner keys.
- `max_value_per_window` is enforced as a per-transaction cap.

## 5) Energy and Fees ✅

- Default fee payer: `energy_pool` if set and sufficient.
- If `energy_pool` is insufficient, fallback to the agent account balance.
- Energy usage rules remain unchanged (only transfers consume energy).

## 6) Freeze and Rotation ✅

- `status=frozen` blocks any agent-executed tx.
- Owner can rotate controller or revoke session keys.

## 7) RPC Extensions ✅

- `get_agent_account`
- `has_agent_account`
- `get_agent_session_key`
- `get_agent_session_keys`

## 8) Example: Session Key Payment

Owner registers a session key scoped to a compute provider:
```
SessionKey {
  public_key: K_session,
  expiry_topoheight: 1_000_000,
  max_value_per_window: 50 TOS,
  allowed_targets: [ComputeProviderAddr],
  allowed_assets: [TOS]
}
```

Agent uses `K_session` to sign a 5 TOS payment:
- Node verifies session key validity, scope limits, and policy.
- If valid, transaction executes.

## 9) Error Codes ✅
- `AgentAccountUnauthorized`
- `AgentAccountSessionKeyExpired`
- `AgentAccountPolicyViolation`
- `AgentAccountFrozen`
- `AgentAccountInvalidParameter`
- `AgentAccountAlreadyRegistered`
- `AgentAccountInvalidController`
- `AgentAccountSessionKeyExists`
- `AgentAccountSessionKeyNotFound`

## 10) Implementation Mapping (TOS Code Layout) ✅
- Payload definitions: `common/src/transaction/payload/agent_account/*`
- Payload routing: `common/src/transaction/payload/mod.rs`
- Verification: `common/src/transaction/verify/agent_account.rs`
- State apply: `daemon/src/core/state/chain_state/apply.rs`
- Storage provider: `daemon/src/core/storage/providers/agent_account.rs`
- RocksDB types: `daemon/src/core/storage/rocksdb/types/agent_account.rs`
- Columns: `daemon/src/core/storage/rocksdb/column.rs`

## 11) Test Matrix (Must-Haves) ✅
- Register success + already-agent failure.
- Session key add/revoke success + invalid expiry/limits rejection.
- Admin-only payloads require owner signature.
- Frozen account rejects controller/session tx.
- Session key scope enforcement (targets/assets/max_value_per_window per tx).
- Energy pool fee payer logic.
- KYC gating uses native KYC module (no AgentAccountMeta cache).
