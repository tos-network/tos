# TOS Multi-Client Alignment: Hybrid Mode Methodology

## Executive Summary

This document defines the **Hybrid Mode** methodology for ensuring alignment between multiple TOS protocol implementations (currently TOS Rust and Avatar C). The approach leverages Rust as the reference implementation while providing Critical Path Specifications, Extended Test Vectors, and Differential Testing infrastructure to enable verifiable cross-client compatibility.

---

## 1. Overview & Core Principles

### 1.1 The Multi-Client Challenge

Blockchain protocols require multiple independent implementations to ensure:
- **Decentralization**: No single implementation controls the network
- **Security**: Bugs in one client don't compromise the entire network
- **Resilience**: Network continues operating if one implementation fails

However, maintaining consensus across implementations presents challenges:
- Ambiguous specifications lead to divergent behavior
- Edge cases may be handled differently
- Performance optimizations may introduce subtle incompatibilities

### 1.2 Why Hybrid Mode?

We evaluated three approaches for multi-client alignment:

| Approach | Description | Effort | Reliability | Maintenance |
|----------|-------------|--------|-------------|-------------|
| Formal Specification | Mathematical proofs | Very High | High | Low (once complete) |
| Executable Specification | Python as single source | High | Uncertain | Triple burden |
| **Hybrid Mode** | Rust reference + specs + vectors | Low | High | Single source |

**Hybrid Mode** was selected because:

1. **Rust is already the de facto standard** - Production-tested, comprehensive implementation
2. **Existing TCK infrastructure** - 45+ generators, 95+ YAML vector files already exist
3. **Minimal additional effort** - Extends existing patterns rather than rewriting
4. **Clear authority** - No ambiguity about which implementation is correct
5. **Maintainable** - Single implementation to maintain, documents as lightweight spec

### 1.3 Core Principles

1. **Rust as Reference Implementation**
   - TOS Rust is the authoritative source for all protocol behavior
   - When implementations diverge, Rust behavior is correct by definition
   - Other clients implement "Rust-compatible" behavior

2. **Specification by Documentation**
   - Critical paths are documented in human-readable specifications
   - Documents describe "what" and "why", not executable code
   - Easier to read, review, and maintain than executable specifications

3. **Verification by Test Vectors**
   - YAML test vectors capture expected behavior
   - Vectors are generated from Rust implementation
   - Other clients verify against these vectors

4. **Validation by Differential Testing**
   - Docker-based test harness runs multiple clients
   - Same inputs produce same outputs across all clients
   - Bidirectional fuzzing catches edge case divergences

---

## 2. Three-Layer Framework

The Hybrid Mode methodology consists of three complementary layers:

```
┌─────────────────────────────────────────────────────────────┐
│             Layer 1: Critical Path Specifications           │
│                    (Human-Readable Documents)               │
│  Wire Format | Hashing | BlockDAG Order | Error Codes | ... │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│             Layer 2: Test Vector Infrastructure             │
│                    (YAML Files + Generators)                │
│  Crypto Vectors | TX Vectors | State Vectors | Block Vectors│
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│             Layer 3: Differential Testing                   │
│                    (Docker + Fuzzing)                       │
│  Multi-Client Harness | Bidirectional Fuzz | State Comparison│
└─────────────────────────────────────────────────────────────┘
```

### 2.1 Layer 1: Critical Path Specifications

**Purpose**: Document the essential behaviors that all clients MUST implement identically.

**Scope**: Only behaviors where divergence would break consensus:
- Wire format serialization (byte-level encoding rules)
- Cryptographic algorithms (which hash for which purpose)
- BlockDAG execution order (deterministic ordering algorithm)
- Failed transaction semantics (fee handling, rollback rules)
- Nonce handling (validation, gap policies)
- Error codes (standardized numeric codes)

**Format**: Markdown documents with:
- Precise algorithmic descriptions
- Byte-level format tables
- Decision matrices for edge cases
- Examples with hex-encoded data

**Location**: `~/tos/docs/specs/` or inline in `MULTI_CLIENT_ALIGNMENT_SCHEME.md`

### 2.2 Layer 2: Test Vector Infrastructure

**Purpose**: Provide concrete test cases that verify implementation correctness.

**Current State** (already exists):
- **TOS Rust TCK**: 45 generators producing 95+ YAML files
- **Avatar C TCK**: 89 tests consuming 35+ YAML files
- **Coverage**: Crypto ~100%, Transactions ~75%, BlockDAG ~80%

**Extensions Required**:
- **State Transition Vectors**: Pre-state → Transaction → Post-state
- **Block Execution Vectors**: Block of transactions with final state
- **Failure Scenario Vectors**: Expected errors for invalid inputs

**Vector Workflow**:
```
TOS Rust             YAML Vectors            Avatar C
┌──────────┐        ┌────────────┐        ┌──────────┐
│ gen_*.rs │───────►│  *.yaml    │───────►│ test_*.c │
│ (source) │        │ (vectors)  │        │ (verify) │
└──────────┘        └────────────┘        └──────────┘
```

### 2.3 Layer 3: Differential Testing

**Purpose**: Detect behavioral divergences that test vectors might miss.

**Architecture**:
```
┌─────────────────────────────────────────────────────────────┐
│                   Docker Orchestrator                        │
├─────────────────────────────────────────────────────────────┤
│    TOS Rust      │    Avatar C       │    Future Client     │
│    Container     │    Container      │    Container         │
└────────┬─────────┴────────┬──────────┴──────────┬───────────┘
         │                  │                     │
         └──────────────────┼─────────────────────┘
                            ▼
         ┌─────────────────────────────────────────┐
         │    Shared Vectors + Result Comparison   │
         │    - Same TX → Same result              │
         │    - Same block → Same state            │
         │    - Fuzz inputs → No divergence        │
         └─────────────────────────────────────────┘
```

**Components**:
1. **Docker Compose Setup**: Standardized containers for each client
2. **Test Harness**: Feeds identical inputs to all clients
3. **Result Comparator**: Verifies outputs match across clients
4. **Fuzzer Integration**: LibFuzzer/AFL++ for edge case discovery

---

## 3. Coverage Targets

### 3.1 Current Coverage (as of project start)

| Domain | TOS Rust TCK | Avatar C TCK | Status |
|--------|--------------|--------------|--------|
| Hash Functions | 8 generators | 8 consumers | Complete |
| Signatures | 6 generators | 6 consumers | Complete |
| Encryption | 5 generators | 5 consumers | Complete |
| Key Derivation | 3 generators | 3 consumers | Complete |
| Encoding | 3 generators | 3 consumers | Complete |
| ZK Proofs | 4 generators | 4 consumers | Complete |
| Basic TX | 3 generators | 3 consumers | Complete |
| KYC TX | 5 generators | 2 consumers | Partial |
| Escrow TX | 5 generators | 2 consumers | Partial |
| BlockDAG | 5 generators | 0 consumers | Generator only |
| State Transitions | 0 generators | 0 consumers | Not started |

### 3.2 Target Coverage (end of alignment phase)

| Domain | Target | Priority |
|--------|--------|----------|
| Cryptography | 100% (all algorithms) | P0 |
| Wire Format | 100% (all 48 TX types) | P0 |
| Transaction Execution | 90% (core TX types) | P1 |
| State Transitions | 80% (common scenarios) | P1 |
| BlockDAG Ordering | 100% (all rules) | P0 |
| Block Execution | 70% (standard blocks) | P2 |
| Error Handling | 80% (common errors) | P2 |
| Edge Cases | 60% (known edge cases) | P3 |

### 3.3 Coverage Metrics

**Definition of "covered"**:
- YAML vector exists with expected output
- At least one client (Rust) generates the vector
- At least one other client (C) consumes and verifies the vector

**Measurement**:
- Count of vector files per domain
- Count of test cases per vector file
- Pass rate across all clients

---

## 4. Implementation Priority

### Phase 1: Foundation (Weeks 1-2)

**Goal**: Establish specifications and extend existing infrastructure

**Deliverables**:
- [ ] Complete Critical Path Specifications document
- [ ] State transition vector schema definition
- [ ] First 10 state transition vectors (Transfer, Burn)

**Success Criteria**:
- Specifications reviewed and approved
- Avatar C can consume state transition vectors

### Phase 2: Core Coverage (Weeks 3-6)

**Goal**: Achieve 90% coverage on core transaction types

**Deliverables**:
- [ ] State transition vectors for all 48 TX types
- [ ] Block execution vectors (10 scenarios)
- [ ] Error scenario vectors (common failures)

**Success Criteria**:
- All TX types have at least 5 vectors each
- Avatar C passes all vector tests

### Phase 3: Differential Testing (Weeks 7-10)

**Goal**: Automated divergence detection

**Deliverables**:
- [ ] Docker-based test harness
- [ ] CI/CD integration
- [ ] Initial fuzzing campaign (1000 hours)

**Success Criteria**:
- No consensus-breaking divergences found
- Harness runs in CI on every PR

### Phase 4: Continuous Alignment (Ongoing)

**Goal**: Maintain alignment as protocol evolves

**Deliverables**:
- [ ] New vectors for each protocol change
- [ ] Regression tests for fixed divergences
- [ ] Quarterly fuzzing campaigns

**Success Criteria**:
- No regressions introduced
- New features have vectors before merge

---

## 5. Success Criteria

### 5.1 Specification Quality

| Criterion | Measurement |
|-----------|-------------|
| Completeness | All critical paths documented |
| Clarity | External reviewer can implement from spec |
| Accuracy | Spec matches Rust implementation |
| Maintainability | Spec updated within 1 week of Rust changes |

### 5.2 Vector Coverage

| Criterion | Target |
|-----------|--------|
| TX Type Coverage | 100% (all 48 types have vectors) |
| State Transition Coverage | 80% (common scenarios) |
| Error Scenario Coverage | 80% (documented error codes) |
| Edge Case Coverage | 60% (known edge cases) |

### 5.3 Client Compatibility

| Criterion | Target |
|-----------|--------|
| Vector Pass Rate | 100% (all vectors pass all clients) |
| Fuzz Divergence Rate | 0% (no consensus-breaking divergences) |
| State Hash Match Rate | 100% (identical state after same blocks) |

### 5.4 Operational Metrics

| Criterion | Target |
|-----------|--------|
| CI Integration | All vectors run on every PR |
| Vector Generation Time | < 5 minutes for full suite |
| Test Execution Time | < 30 minutes for full suite |
| Maintenance Overhead | < 10% of development time |

---

## 6. Roles and Responsibilities

### TOS Rust Team
- Maintain reference implementation
- Generate test vectors for new features
- Review Critical Path Specifications
- Run differential testing

### Avatar C Team
- Implement vector consumers
- Report divergences found
- Contribute edge case vectors
- Verify specification accuracy

### Cross-Team
- Review and approve specifications
- Triage divergence reports
- Maintain shared infrastructure

---

## 7. Document References

| Document | Purpose |
|----------|---------|
| `MULTI_CLIENT_ALIGNMENT_SCHEME.md` | Technical specifications (this methodology's Layer 1) |
| `~/tos/tck/README.md` | TCK architecture and usage |
| `~/avatar/src/tck/README.md` | Avatar TCK integration guide |

---

## Appendix A: Comparison with Alternative Approaches

### A.1 Why Not Formal Specification?

Formal specifications (TLA+, Coq) provide mathematical guarantees but:
- Require specialized expertise to write and verify
- Take years to develop for complex protocols
- May still have gaps between spec and implementation
- Cannot catch implementation bugs directly

**Verdict**: Too high effort for current project stage. May revisit for critical components.

### A.2 Why Not Executable Python Specification?

An executable specification in Python would:
- Require reimplementing entire protocol in Python
- Create maintenance burden of three codebases (Rust, C, Python)
- Introduce question of "who validates the Python?"
- Provide no additional guarantees over Rust reference

**Verdict**: High effort with uncertain benefit. Hybrid mode achieves same goals with less work.

### A.3 Why Hybrid Mode Works

Hybrid mode succeeds because:
- Rust implementation is already production-quality
- Test vector infrastructure already exists
- Documents are easier to maintain than executable specs
- Differential testing catches what specs and vectors miss

---

## Appendix B: Glossary

| Term | Definition |
|------|------------|
| **Reference Implementation** | The authoritative implementation (TOS Rust) |
| **Critical Path** | Protocol behavior where divergence breaks consensus |
| **Test Vector** | A test case with input and expected output |
| **State Transition Vector** | Vector including pre-state, transaction, and post-state |
| **Differential Testing** | Running same inputs through multiple implementations |
| **TCK** | Technology Compatibility Kit (test vector infrastructure) |
| **Wire Format** | Binary serialization format for network transmission |
| **State Digest** | Cryptographic hash of canonical state representation |

---

*Document Version: 1.0*
*Last Updated: 2026-02-03*
*Status: Draft*
