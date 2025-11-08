# TAKO Execution Engine Security Audit

_Date: 2024-05-17_

## Scope & Methodology
- **Scope:** `daemon/src/tako_integration` components (`executor.rs`, `storage.rs`, `accounts.rs`, `loader.rs`, `executor_adapter.rs`).
- **Methodology:** Manual source review with focus on state isolation, privilege boundaries, and correctness of adapters exposing TOS runtime to TAKO smart contracts.

## Executive Summary
The TAKO integration introduces a new execution path for smart contracts but currently exposes critical flaws in the storage and balance adapters. The most severe issues allow contracts to corrupt or misinterpret their own state and to report successful token transfers without funds ever moving. Immediate remediation is required before running untrusted contracts on this engine.

| Severity | Finding | Location |
| --- | --- | --- |
| Critical | Storage adapter collapses all keys/values to a placeholder, enabling total state corruption | `daemon/src/tako_integration/storage.rs` |
| High | Account transfer syscall always reports success without producing a ledger change | `daemon/src/tako_integration/accounts.rs` |
| Medium | Storage reads return serialized `ValueCell` metadata instead of original bytes | `daemon/src/tako_integration/storage.rs` |

## Findings

### 1. Critical – Storage adapter collapses all keys and values
- **Details:** `bytes_to_value_cell` returns `ValueCell::default()` for every key/value after serializing bytes but discarding the result.【F:daemon/src/tako_integration/storage.rs†L94-L104】  Because the cache maps use the `ValueCell` as the hash key, every storage slot collapses to the same placeholder entry. Any write overwrites the previous cached value regardless of the logical key, defeating per-key isolation.
- **Impact:** Contracts cannot reliably maintain state. A malicious contract call can clobber unrelated keys, causing arbitrary state corruption or preventing honest state reads.
- **Recommendation:** Implement a stable one-to-one encoding between arbitrary byte slices and `ValueCell` (e.g., introduce a `ValueCell::Bytes` variant or store the serialized bytes inside the cell) so that each key maps to a unique `ValueCell` representation.

### 2. High – `transfer` syscall reports success without moving funds
- **Details:** `TosAccountAdapter::transfer` validates balances and account existence but never schedules or records a transfer; it simply returns `Ok(())` after the checks.【F:daemon/src/tako_integration/accounts.rs†L78-L126】  The TODO comment confirms that actual transfer logic is deferred to a future phase.
- **Impact:** Contracts that rely on the syscall to move assets will observe a success response and may release goods or unlock logic even though no funds moved, enabling theft or broken invariants.
- **Recommendation:** Either integrate with TOS’s post-execution transfer pipeline immediately or make the syscall fail with an explicit “unimplemented” error until funds can be moved atomically.

### 3. Medium – Storage reads return serialized metadata instead of stored bytes
- **Details:** `value_cell_to_bytes` calls `bincode::serialize(cell)` (despite the comment about deserialization), so reads return the serialized structure of the `ValueCell` object rather than the raw bytes that were originally written.【F:daemon/src/tako_integration/storage.rs†L112-L161】  Combined with the placeholder writes, contracts cannot reconstruct their state from storage.
- **Impact:** Even after fixing the key collision, reads would still produce opaque `ValueCell`-encoded blobs instead of contract-level data, leading to logic errors or the need for unsafe decoding inside contracts.
- **Recommendation:** Replace `bincode::serialize(cell)` with a true decoding path that recovers the stored byte vector (after the representation from Finding #1 is fixed). Ensure round-trip correctness through unit tests that cover heterogeneous keys and values.

## Additional Observations
- The executor correctly enforces compute budgets and loaded-data accounting, mitigating resource exhaustion in the TBPF runtime.【F:daemon/src/tako_integration/executor.rs†L84-L213】
- Cross-program invocation is limited to TAKO-ELF modules, preventing accidental execution of legacy TOS-VM code paths.【F:daemon/src/tako_integration/loader.rs†L49-L94】

## Recommendations
1. Prioritize fixes for Findings #1 and #2 before exposing TAKO to user-deployed contracts.
2. Add regression tests that cover multi-key storage, realistic return-data flows, and contract-level token transfers once implemented.
3. Conduct a follow-up review after remediation, focusing on syscall surface area and interaction with consensus-critical state transitions.

## Remediation Status (May 2024)
- **Storage adapter fixes implemented:** The adapter now stores raw bytes with `ValueCell::Bytes` and rejects non-byte reads, preventing cache collisions and metadata leakage.【F:daemon/src/tako_integration/storage.rs†L94-L161】【F:daemon/src/tako_integration/storage.rs†L408-L461】
- **Transfer syscall now stages outputs:** Account transfers are queued via `TransferOutput` and surfaced through the executor result so the transaction processor can persist them atomically.【F:daemon/src/tako_integration/accounts.rs†L20-L154】【F:daemon/src/tako_integration/executor.rs†L94-L352】【F:daemon/src/tako_integration/executor_adapter.rs†L16-L126】【F:common/src/contract/executor.rs†L1-L42】
- **Additional regression tests added:** Storage tests cover distinct-key behaviour and byte round-trips, while account tests validate transfer queue semantics.【F:daemon/src/tako_integration/storage.rs†L400-L461】【F:daemon/src/tako_integration/accounts.rs†L244-L327】

