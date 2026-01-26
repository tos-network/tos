# Bounty Program

This document lists open bounties for the TOS protocol implementation. Contributions that address these items may be eligible for token-based rewards.

## Policy

### General Terms

- Bounties are **discretionary** and awarded at the sole judgment of the core maintainers
- This is **not employment** - contributors are independent participants
- Rewards are primarily in **tokens**, not cash
- All rewards are subject to **mainnet launch** and applicable **vesting conditions**
- No reward is guaranteed until a PR is **merged and accepted** by maintainers
- Partial completions may receive partial rewards or no reward
- Bounties may be modified, reassigned, or withdrawn at any time

### Before You Start

- **Small tasks (S)**: You may start immediately; open a PR when ready
- **Medium/Large tasks (M/L)**: Comment on the relevant issue or open a new one to discuss your approach **before** writing significant code
- Duplicate work will not be rewarded - check if someone else is already working on a bounty

### Scope of Contributions

- Bounty rewards are for **code and documentation contributions only**
- No contributor receives ownership, admin access, or moderation privileges on official project channels as part of any bounty
- All code must follow the coding standards defined in CLAUDE.md

---

## Open Bounties

### B-001: BlockDAG Fork Choice Rule Verification

**Difficulty:** L
**Area:** Consensus
**Reward Tier:** Large token grant

**Scope / Acceptance Criteria:**

- [ ] Implement comprehensive test suite for GHOSTDAG fork choice rule
- [ ] Verify blue score calculation correctness under various DAG topologies
- [ ] Test fork resolution with competing chains of equal weight
- [ ] Verify deterministic ordering of blocks with same blue score
- [ ] Document edge cases and invariants in code comments

**Notes:** Consensus-critical. Must use deterministic test inputs. No floating-point arithmetic. Discuss approach with maintainers before starting.

---

### B-002: Transaction Validation Hardening

**Difficulty:** M
**Area:** Core / Security
**Reward Tier:** Medium token grant

**Scope / Acceptance Criteria:**

- [ ] Audit transaction deserialization for input length limits
- [ ] Verify signature validation rejects malformed inputs
- [ ] Add fuzz tests for transaction parsing
- [ ] Ensure all arithmetic uses checked/saturating operations
- [ ] No panics reachable from malformed transaction data

**Notes:** Security-sensitive. Follow defensive coding guidelines in CLAUDE.md. Include proof-of-concept for any issues found.

---

### B-003: UTXO Set Integrity Verification

**Difficulty:** M
**Area:** Core / Storage
**Reward Tier:** Medium token grant

**Scope / Acceptance Criteria:**

- [ ] Implement UTXO commitment calculation for state snapshots
- [ ] Add verification that UTXO set matches block commitments
- [ ] Test rollback scenarios preserve UTXO consistency
- [ ] Verify no double-spend possible across reorgs
- [ ] Document UTXO pruning safety invariants

**Notes:** State integrity is critical. Must handle all reorg edge cases. Deterministic computation required.

---

### B-004: VM Syscall Security Audit

**Difficulty:** L
**Area:** VM / Security
**Reward Tier:** Large token grant

**Scope / Acceptance Criteria:**

- [ ] Audit all syscall implementations for input validation
- [ ] Verify gas metering correctness for each syscall
- [ ] Test resource limits (memory, stack, call depth)
- [ ] Fuzz test syscall handlers with malformed inputs
- [ ] Document security boundaries and trust assumptions

**Notes:** VM security is critical for contract safety. Coordinate with maintainers on scope. See tck/ for existing test infrastructure.

---

### B-005: Gas Metering Accuracy Verification

**Difficulty:** M
**Area:** VM
**Reward Tier:** Medium token grant

**Scope / Acceptance Criteria:**

- [ ] Verify gas costs match documented specification
- [ ] Test gas exhaustion handling mid-execution
- [ ] Ensure deterministic gas consumption across runs
- [ ] Add regression tests for gas calculation
- [ ] Document any deviations from EVM gas semantics

**Notes:** Gas metering must be deterministic. Use u64 arithmetic only. No floating-point.

---

### B-006: P2P Block Propagation Robustness

**Difficulty:** M
**Area:** Daemon / Networking
**Reward Tier:** Medium token grant

**Scope / Acceptance Criteria:**

- [ ] Handle malformed block announcements gracefully
- [ ] Implement duplicate block detection and rejection
- [ ] Add bandwidth limits for block requests
- [ ] Test behavior under high peer churn
- [ ] Log metrics for block propagation latency

**Notes:** Must not panic on malformed network input. Follow logging guidelines in CLAUDE.md.

---

### B-007: Difficulty Adjustment Algorithm Testing

**Difficulty:** M
**Area:** Consensus
**Reward Tier:** Medium token grant

**Scope / Acceptance Criteria:**

- [ ] Implement test vectors for difficulty adjustment
- [ ] Verify algorithm behavior at boundary conditions
- [ ] Test with simulated timestamp manipulation attempts
- [ ] Ensure deterministic calculation across platforms
- [ ] Document algorithm parameters and rationale

**Notes:** Must use integer arithmetic only. Verify against reference implementation if available.

---

### B-008: Block Template Construction Optimization

**Difficulty:** S
**Area:** Miner
**Reward Tier:** Small token grant

**Scope / Acceptance Criteria:**

- [ ] Profile block template construction performance
- [ ] Optimize transaction selection algorithm
- [ ] Reduce memory allocations in hot path
- [ ] Maintain deterministic transaction ordering
- [ ] Include before/after benchmarks

**Notes:** Performance-focused. Must not change consensus behavior. Wrap logs with `log::log_enabled!` checks.

---

### B-009: RPC Input Validation Hardening

**Difficulty:** S
**Area:** Daemon / RPC
**Reward Tier:** Small token grant

**Scope / Acceptance Criteria:**

- [ ] Audit all RPC endpoints for input size limits
- [ ] Add validation for hex string lengths
- [ ] Ensure numeric parameters have bounds checks
- [ ] Return appropriate error codes for invalid input
- [ ] No panics or resource exhaustion from RPC input

**Notes:** Security-sensitive. See CLAUDE.md for input validation patterns. Add tests for malformed requests.

---

### B-010: Cryptographic Primitive Review

**Difficulty:** L
**Area:** Common / Crypto
**Reward Tier:** Large token grant

**Scope / Acceptance Criteria:**

- [ ] Review hash function usage for consistency
- [ ] Verify signature scheme implementation correctness
- [ ] Audit key derivation and address generation
- [ ] Test edge cases (zero keys, identity points, etc.)
- [ ] Document cryptographic assumptions and dependencies

**Notes:** Critical security area. Requires cryptographic expertise. Coordinate with maintainers before starting.

---

### B-011: State Transition Determinism Audit

**Difficulty:** L
**Area:** Core / Consensus
**Reward Tier:** Large token grant

**Scope / Acceptance Criteria:**

- [ ] Identify all state transition code paths
- [ ] Verify no use of HashMap iteration in consensus
- [ ] Audit for floating-point arithmetic in state updates
- [ ] Check for system time dependencies
- [ ] Add determinism regression tests

**Notes:** Consensus-critical. Non-determinism causes network splits. See CLAUDE.md determinism requirements.

---

### B-012: Contract Storage Security Review

**Difficulty:** M
**Area:** VM / Storage
**Reward Tier:** Medium token grant

**Scope / Acceptance Criteria:**

- [ ] Verify storage isolation between contracts
- [ ] Test transient storage (EIP-1153) implementation
- [ ] Audit storage slot calculation for collisions
- [ ] Verify storage metering accuracy
- [ ] Document storage model and limitations

**Notes:** Storage bugs can cause fund loss. Deterministic behavior required. See tck/ for test contracts.

---

## How to Claim a Bounty

1. **Check availability** - Ensure the bounty is still open and not already claimed
2. **Comment on the issue** - State your intent to work on the bounty (for M/L tasks, wait for maintainer acknowledgment)
3. **Submit a PR** - Reference the bounty ID (e.g., "Addresses B-003") in your PR description
4. **Address review feedback** - Respond to maintainer comments and make requested changes
5. **Await merge** - Reward eligibility is confirmed only after PR is merged
6. **Maintainer confirmation** - Core team will follow up regarding reward details after merge

For questions about specific bounties, open a GitHub issue with the bounty ID in the title.

---

*Last updated: 2025-01*
