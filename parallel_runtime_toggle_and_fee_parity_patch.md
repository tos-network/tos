# Patch: Parallel Runtime Toggle (default OFF) + Debug Fee Parity Check

**Scope**
- Repo: `tos-network/tos`
- PR: `parallel-transaction-execution-v3 → master`
- Files:
  - `daemon/src/config.rs`
  - `daemon/src/core/blockchain.rs`

**Goal**
1) Make parallel execution **runtime-configurable** via env vars and **default OFF** (safer for mainnet).
2) Add a **debug-only fee parity check** after parallel execution to catch accounting drift early.

---

## ✅ Changes Overview

### 1) Runtime toggle (English-only comments)
- Remove hardcoded booleans for enabling parallel execution.
- Introduce two runtime helpers:
  ```rust
  pub fn parallel_execution_enabled() -> bool { /* reads TOS_PARALLEL_EXECUTION */ }
  pub fn parallel_test_mode_enabled() -> bool { /* reads TOS_PARALLEL_TEST_MODE */ }
  ```
- Rationale: safer default (OFF), enable via env in dev/test without rebuild.

### 2) Fee parity check (debug-only)
- After parallel path computes `total_fees`, compare with `parallel_state.get_gas_fee()` under `#[cfg(debug_assertions)]`.
- Log `warn!` on mismatch, `debug!` on match. **No consensus-side effect**.

---

## 🔧 Semantic Edit Steps (preferred by code assistants)

> Use these steps if unified diff fails to apply cleanly. All comments must remain in **English**.

### A) `daemon/src/config.rs`

1. **Remove** previous hard-coded booleans (if present), e.g.:
   - `PARALLEL_EXECUTION_ENABLED`
   - `PARALLEL_EXECUTION_TEST_MODE`

2. **Add** the following **runtime toggle** helpers (place near other config helpers):

```rust
// -----------------------------------------------------------------------------
// Parallel Execution Configuration (runtime toggles)
// -----------------------------------------------------------------------------
//
// We purposefully default to DISABLED in production builds and enable via
// environment variables. This avoids accidentally turning on parallel execution
// where it hasn't been validated.
//
// Environment variables:
//   - TOS_PARALLEL_EXECUTION
//       "1" | "true"  => enabled
//       (unset/other) => disabled (default)
//
//   - TOS_PARALLEL_TEST_MODE
//       "1" | "true"  => enabled (runs extra parity checks)
//       (unset/other) => disabled (default)
//
// Rationale:
// - Safer default for mainnet.
// - Easy to turn on for dev/test environments without a rebuild.
use std::env;

/// Returns true if parallel execution is enabled at runtime.
pub fn parallel_execution_enabled() -> bool {
    match env::var("TOS_PARALLEL_EXECUTION") {
        Ok(v) => matches!(v.as_str(), "1" | "true" | "TRUE" | "True"),
        Err(_) => false,
    }
}

/// Returns true if parallel test-mode is enabled at runtime.
pub fn parallel_test_mode_enabled() -> bool {
    match env::var("TOS_PARALLEL_TEST_MODE") {
        Ok(v) => matches!(v.as_str(), "1" | "true" | "TRUE" | "True"),
        Err(_) => false,
    }
}
```

> Keep the existing `get_min_txs_for_parallel(...)` and thresholds **unchanged**.

---

### B) `daemon/src/core/blockchain.rs`

1. **Update imports** to use runtime toggles (import only what you use to avoid warnings):

```rust
use crate::config::{
    MILLIS_PER_SECOND, SIDE_BLOCK_REWARD_MAX_BLOCKS, PRUNE_SAFETY_LIMIT,
    SIDE_BLOCK_REWARD_PERCENT, SIDE_BLOCK_REWARD_MIN_PERCENT, STABLE_LIMIT,
    TIMESTAMP_IN_FUTURE_LIMIT, DEFAULT_CACHE_SIZE, MAX_ORPHANED_TRANSACTIONS,
    get_min_txs_for_parallel,
    parallel_execution_enabled, // runtime toggle
    // parallel_test_mode_enabled, // optionally import if you will use it
};
```

2. **Rewrite** the decision helper to use the runtime toggle:

```rust
fn should_use_parallel_execution(&self, tx_count: usize) -> bool {
    let min_txs = get_min_txs_for_parallel(&self.network);
    // Safer default: only enable when the runtime toggle is ON
    parallel_execution_enabled() && tx_count >= min_txs
}
```

3. **Add** a debug-only fee parity check **after** the parallel/sequential execution block (place it where both `parallel_state` and `total_fees` are available and the parallel branch was taken):

```rust
// ---------------------------------------------------------------------
// Debug-only parity check (parallel path only):
// Verify that the sum of per-TX fees equals the gas fee accumulated by
// the parallel execution path. This helps catch accidental divergence
// between "per-TX accounting" and "parallel-state aggregation".
// NOTE: This is NOT consensus-critical and only runs in debug builds.
// ---------------------------------------------------------------------
if self.should_use_parallel_execution(block.get_transactions().len()) {
    #[cfg(debug_assertions)]
    {
        let aggregated_gas = parallel_state.get_gas_fee();
        if aggregated_gas != total_fees {
            warn!(
                "Fees parity check failed for block {}: aggregated_gas={} total_fees={}",
                hash, aggregated_gas, total_fees
            );
        } else {
            debug!("Fees parity check OK for block {}: {}", hash, total_fees);
        }
    }
}
```

> If you see an “unused import” warning for `parallel_test_mode_enabled`, remove it or import as alias `_`.

---

## 💾 Unified Diff (fallback)

> Use this if your tool supports applying unified diffs. Context lines are approximate; resolve conflicts by following the semantic steps above.

### Patch 1/2 — `daemon/src/config.rs`

```diff
diff --git a/daemon/src/config.rs b/daemon/src/config.rs
--- a/daemon/src/config.rs
+++ b/daemon/src/config.rs
@@
-// Parallel Execution Configuration
-// Enable parallel transaction execution (V3 implementation)
-pub const PARALLEL_EXECUTION_ENABLED: bool = true; // DEVNET TESTING: Enabled for performance validation
-// Enable parallel testing mode (run parallel alongside sequential, compare results)
-pub const PARALLEL_EXECUTION_TEST_MODE: bool = false; // Default: disabled
+// -----------------------------------------------------------------------------
+// Parallel Execution Configuration (runtime toggles)
+// -----------------------------------------------------------------------------
+//
+// We purposefully default to DISABLED in production builds and enable via
+// environment variables. This avoids accidentally turning on parallel execution
+// where it hasn't been validated.
+//
+// Environment variables:
+//   - TOS_PARALLEL_EXECUTION
+//       "1" | "true"  => enabled
+//       (unset/other) => disabled (default)
+//
+//   - TOS_PARALLEL_TEST_MODE
+//       "1" | "true"  => enabled (runs extra parity checks)
+//       (unset/other) => disabled (default)
+//
+// Rationale:
+// - Safer default for mainnet.
+// - Easy to turn on for dev/test environments without a rebuild.
+use std::env;
+
+/// Returns true if parallel execution is enabled at runtime.
+pub fn parallel_execution_enabled() -> bool {
+    match env::var("TOS_PARALLEL_EXECUTION") {
+        Ok(v) => matches!(v.as_str(), "1" | "true" | "TRUE" | "True"),
+        Err(_) => false,
+    }
+}
+
+/// Returns true if parallel test-mode is enabled at runtime.
+pub fn parallel_test_mode_enabled() -> bool {
+    match env::var("TOS_PARALLEL_TEST_MODE") {
+        Ok(v) => matches!(v.as_str(), "1" | "true" | "TRUE" | "True"),
+        Err(_) => false,
+    }
+}
```

### Patch 2/2 — `daemon/src/core/blockchain.rs`

```diff
diff --git a/daemon/src/core/blockchain.rs b/daemon/src/core/blockchain.rs
--- a/daemon/src/core/blockchain.rs
+++ b/daemon/src/core/blockchain.rs
@@
-    use crate::config::{
-        MILLIS_PER_SECOND, SIDE_BLOCK_REWARD_MAX_BLOCKS, PRUNE_SAFETY_LIMIT,
-        SIDE_BLOCK_REWARD_PERCENT, SIDE_BLOCK_REWARD_MIN_PERCENT, STABLE_LIMIT,
-        TIMESTAMP_IN_FUTURE_LIMIT, DEFAULT_CACHE_SIZE, MAX_ORPHANED_TRANSACTIONS,
-        PARALLEL_EXECUTION_ENABLED, get_min_txs_for_parallel,
-    };
+    use crate::config::{
+        MILLIS_PER_SECOND, SIDE_BLOCK_REWARD_MAX_BLOCKS, PRUNE_SAFETY_LIMIT,
+        SIDE_BLOCK_REWARD_PERCENT, SIDE_BLOCK_REWARD_MIN_PERCENT, STABLE_LIMIT,
+        TIMESTAMP_IN_FUTURE_LIMIT, DEFAULT_CACHE_SIZE, MAX_ORPHANED_TRANSACTIONS,
+        get_min_txs_for_parallel,
+        parallel_execution_enabled,
+        // parallel_test_mode_enabled,
+    };
@@
-    fn should_use_parallel_execution(&self, tx_count: usize) -> bool {
-        let min_txs = get_min_txs_for_parallel(&self.network);
-        PARALLEL_EXECUTION_ENABLED && tx_count >= min_txs
-    }
+    fn should_use_parallel_execution(&self, tx_count: usize) -> bool {
+        let min_txs = get_min_txs_for_parallel(&self.network);
+        // Safer default: only enable when the runtime toggle is ON
+        parallel_execution_enabled() && tx_count >= min_txs
+    }
@@
+                // ---------------------------------------------------------------------
+                // Debug-only parity check (parallel path only)
+                // ---------------------------------------------------------------------
+                if self.should_use_parallel_execution(block.get_transactions().len()) {
+                    #[cfg(debug_assertions)]
+                    {
+                        let aggregated_gas = parallel_state.get_gas_fee();
+                        if aggregated_gas != total_fees {
+                            warn!(
+                                "Fees parity check failed for block {}: aggregated_gas={} total_fees={}",
+                                hash, aggregated_gas, total_fees
+                            );
+                        } else {
+                            debug!("Fees parity check OK for block {}: {}", hash, total_fees);
+                        }
+                    }
+                }
```

---

## ▶️ How to Enable at Runtime

```bash
# Enable parallel execution (runtime)
export TOS_PARALLEL_EXECUTION=1

# Optional: enable extra parity diagnostics (if used)
export TOS_PARALLEL_TEST_MODE=1
```

Thresholds remain unchanged: Mainnet=20 / Testnet=10 / Devnet=4.

---

## 🔁 Rollback Plan

- If any issue is detected, unset the env var:
  ```bash
  unset TOS_PARALLEL_EXECUTION
  ```
- The node falls back to the sequential path automatically.

---

## 🧪 Quick Verification

```bash
# 1) Build (debug for parity check)
cargo build -p tos_daemon

# 2) Run sequential (default OFF)
./target/debug/tos_daemon # should log sequential path for small blocks

# 3) Run parallel
TOS_PARALLEL_EXECUTION=1 ./target/debug/tos_daemon # should log parallel path when threshold met
```

> In debug builds, watch for `Fees parity check OK/failed` logs after blocks processed in parallel.
