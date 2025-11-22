# TOS Network Security Audit Report

**Audit Date**: November 22, 2025
**Source**: `tos-source-20251122.zip` code snapshot
**Auditor**: Third-party security review
**Audit Type**: Static code analysis + pattern scanning

---

## Executive Summary

This audit was performed on the TOS blockchain source code snapshot (600+ `.rs` files). The review focused on critical modules through static analysis and global pattern scanning, without running tests or operating live nodes.

> ⚠️ **Scope Limitations**:
> - Code volume is extensive (600+ Rust files) - this is a targeted review, not a line-by-line formal verification
> - No unit/integration tests were executed, no live node operations were performed
> - Severity ratings (High/Medium/Low) are relative assessments within the reviewed scope, not absolute security guarantees

---

## I. Overall Assessment (Executive Summary)

From this source code snapshot:

**Architecture**: TOS is a **Rust-based PoW + GhostDAG L1 blockchain** with core modules including:
- `daemon/`: Full node (consensus, P2P, RPC, TAKO VM integration)
- `common/`: Shared libraries (accounts, transactions, blocks, serialization, cryptography, AI mining)
- `wallet/`: Local wallet + XSWD WebSocket protocol
- `miner/`: Mining program

**Security Posture**: Generally rigorous:
- Uses **OsRng** for cryptographic random number generation
- P2P encryption with **x25519 + ChaCha20-Poly1305**
- Wallet encryption with **Argon2id + XChaCha20-Poly1305** for password derivation and data encryption
- Extensive `// SECURITY FIX:` comments and dedicated security regression tests

**Findings**:
- **No obvious "instant coin theft" critical bugs** discovered (e.g., RPC allowing unauthorized fund transfers, completely unvalidated consensus state)
- **Several security concerns identified**, particularly:
  - Wallet's **XSWD WebSocket binds to `0.0.0.0` by default**, relying on "user interaction + application ID" authentication without true cryptographic identity verification (**High severity** - configuration/design issue)
  - Extensive use of **`.unwrap()` / `.expect()`** in production code paths throughout node and common libraries, creating potential **DoS vulnerability** if triggered by external input
  - P2P encryption uses 64-bit nonce counter with "1GB re-key" to prevent nonce reuse - acceptable risk, but could be more robust
  - XSWD protocol has error types for "application permission signatures," but **actual implementation lacks signature fields in ApplicationData and public key verification**, diverging from documented security model

### Risk Ratings (for this snapshot)

- **Consensus & Storage Logic**: No obvious logical vulnerabilities found, but high complexity suggests formal verification/model checking → ★★★✩☆ (Medium-High)
- **P2P & Encryption Layer**: Modern approach, implementation considers DoS and replay attacks, minor improvements possible → ★★★★☆
- **Wallet & XSWD**: Encryption is solid, but listening address and application identity authentication design requires **urgent re-audit** → ★★✩✩✩

---

## II. Audit Scope & Methodology

### Modules Covered

**Core Modules**:
- `daemon/src/core/`
  - `blockchain.rs` (blockchain state machine, 6000+ lines)
  - `blockdag.rs`, `ghostdag/*` (DAG consensus)
  - `difficulty/*` (difficulty adjustment)
  - `storage/*` (RocksDB storage, caching)

- `daemon/src/p2p/*`
  - `diffie_hellman.rs`, `encryption.rs`, `connection.rs`
  - `packet/handshake.rs` and other message types

- `daemon/src/rpc/*`
  - JSON-RPC, getwork, WebSocket + `websocket/security.rs`

- `common/src/*`
  - `crypto/*` (random numbers, hashing, signatures)
  - `serializer/*`, `block/*`, `transaction/*`
  - `account/*` (balance, energy mechanism)

- `wallet/src/*`
  - `config.rs` (Argon2 parameters, default bind address)
  - `cipher.rs` (XChaCha20-Poly1305)
  - `storage/*`, `api/xswd/*`, `api/server/xswd_server.rs`

**Auxiliary Analysis**:
- Repository-wide scan for `unsafe` / `panic!` / `.unwrap()` / `.expect()` usage
- Review of `daemon/tests/security/storage_security_tests.rs` and other security regression tests

### Not Deeply Reviewed (Future Work)

- TAKO VM (eBPF contract execution) bytecode verification logic
- AI mining / ZK implementation details
- Complete game-theoretic analysis of economic models

---

## III. Findings by Severity

### High Severity H1: XSWD Default Binds to 0.0.0.0 and Lacks Strong Identity Authentication

**Location**:

`wallet/src/config.rs`:
```rust
pub const XSWD_BIND_ADDRESS: &str = "0.0.0.0:44325";
```

`wallet/src/api/server/xswd_server.rs`:
- Directly binds to `XSWD_BIND_ADDRESS`, exposing WebSocket interface to all IPs

`wallet/src/api/xswd/*`:
- Primary security model relies on "first-time application permission request" + user confirmation
- `ApplicationData` structure **contains no signature field**
- `XSWDHandler::get_public_key()` is not used

**Risk Analysis**:

1. **Listening on 0.0.0.0**:
   - Any client with network access to port 44325 can attempt XSWD connections
   - On desktop/LAN environments, users may be unaware of this exposure, creating vulnerability to malicious software or LAN attacks

2. **Lack of Cryptographic Application Identity Authentication**:
   - XSWD comments state "applications must authenticate and declare permissions," but actual `ApplicationData` only contains:
     - `id: String`
     - `name, description, url, permissions`
   - No signature or public key binding
   - Error types `ApplicationPermissionsNotSigned` / `InvalidSignatureForApplicationData` **are currently unused**
   - `verify_application()` only checks:
     - `id` is 64 characters and hexadecimal
     - Name/description/URL length constraints
     - RPC method existence for requested permissions
     - `provider.has_app_with_id()` to prevent ID duplication

3. **Risk Combined with "AlwaysAccept" Permissions**:
   - If a user grants "always allow this method" to an app ID, that creates long-term privileges for that ID
   - Since ID is just a string without signature binding, **any client knowing the ID can impersonate that application** and reuse permissions
   - Attack surface significantly expands when XSWD Server is exposed on 0.0.0.0

**Impact**:

Once XSWD is externally accessible (accidental port mapping, server deployment, etc.), attackers can:
- Submit application registration requests to phish/deceive users into granting permissions
- Reuse existing app IDs with historical permissions (if user configured AlwaysAllow)
- Issue transfers/signatures through wallet, indirectly stealing assets controlled by private keys

**Remediation Recommendations**:

1. **Default to localhost-only binding**:
   - Change `XSWD_BIND_ADDRESS` default to `"127.0.0.1:44325"`
   - OR require explicit `--xswd-bind 0.0.0.0:44325` flag for external exposure

2. **Add security guardrails**:
   - When binding to non-127.0.0.1:
     - Log repeated `WARN` level messages
     - Require additional confirmation (e.g., CLI prompt "I KNOW WHAT I AM DOING")

3. **Implement application permission signature verification**:
   - Add to `ApplicationData`:
     - Application public key
     - Signature field over app data + permissions list
   - In `verify_application()`:
     - Use `XSWDHandler::get_public_key()` or app's public key to verify signature
     - Return `ApplicationPermissionsNotSigned` / `InvalidSignatureForApplicationData` on failure

4. **Document security boundaries**:
   - Clearly state XSWD is designed for "local browser / trusted frontend"
   - Should not expose to public internet without separate TLS termination and reverse proxy authentication

---

### Medium Severity M1: Extensive `.unwrap()` / `.expect()` in Production Paths Creates DoS Surface

**Statistics** (approximate, via grep):

- Repository-wide:
  - `.unwrap()` ≈ 1900+ occurrences
  - `.expect(` ≈ 400+ occurrences
  - `panic!` ≈ 70+ occurrences

- **Production binary paths**:
  - `daemon/src`: `panic!` ≈ 8, `.unwrap()` ≈ 160, `.expect()` ≈ 27
  - `common/src`: `panic!` ≈ 19, `.unwrap()` ≈ 245
  - `wallet/src`: `.unwrap()` ≈ 70+ (some acceptable, but recommend control)

Many `panic!` occurrences are in test code (e.g., `daemon/src/core/tests/*`), which is fine. However, **`.unwrap()` / `.expect()` in daemon core logic processing network or disk input creates risk**:

- Malicious nodes/clients can craft specific data to trigger unwrap/expect failures, causing node panic exit
- This becomes a DoS attack vector
- Particularly sensitive locations:
  - P2P packet decoding
  - Block/transaction deserialization
  - RPC parameter parsing
  - Storage layer (RocksDB read errors)

**Recommendations**:

- Systematically review `daemon/src` and `common/src`:
  - Tag all `.unwrap()` / `.expect()` as:
    - ✅ "Internal invariant + impossible to trigger from external input" (e.g., constant new_unchecked with compile-time assert)
    - ⚠️ "Reachable by external input" (network, disk, RPC)
  - Convert the latter to return `Result<_, Error>` with unified error handling:
    - Log the error
    - Disconnect connection / drop transaction or block, rather than crashing entire program

- For truly "unreachable code paths," prefer:
  - `debug_assert!` + return error
  - Or `unreachable!()`, ensuring compile-time proof of safety

---

### Medium Severity M2: RPC / HTTP Interface Lacks Unified Authentication Mechanism

**Current State**:

`daemon/src/config.rs` default:
```rust
pub const DEFAULT_RPC_BIND_ADDRESS: &str = "127.0.0.1:8080";
```
This is excellent - defaults to localhost-only.

RPC module (`daemon/src/rpc/*`) includes:
- JSON-RPC
- getwork (miner interface)
- WebSocket (has dedicated `websocket/security.rs` with origin whitelist, rate limiting, message size limits, and optional API key)

**Issues**:

- HTTP JSON-RPC currently **primarily relies on "bind to 127.0.0.1" as security boundary**
- If user accidentally exposes RPC to internet (e.g., Docker/reverse proxy misconfiguration):
  - Anyone can issue RPC calls
  - Even without direct "spend money" RPC, exposure includes:
    - Mempool observation
    - Node management, peer banning, debug interfaces
- WebSocket security module is well-implemented, but:
  - API key is "optional"
  - Recommend **mandatory API key / authentication** for any non-localhost binding

**Recommendations**:

1. **Enhanced configuration protection**:
   - If RPC binds to non-local address (e.g., `0.0.0.0` or public IP):
     - Print red/WARN level logs at startup
     - Require explicit CLI flag like `--allow-remote-rpc`

2. **Add optional auth module for HTTP JSON-RPC**:
   - Similar to Bitcoin's `rpcuser:rpcpassword` / Bearer token mechanism
   - Or provide documentation for reverse proxy (nginx/caddy) best practices

3. **Document security boundaries**:
   - Clearly state "RPC interface assumes localhost / trusted network only"
   - Provide hardening deployment examples

---

### Low-Medium Severity M3: P2P Encryption Nonce Overflow Theoretical Risk (Currently Acceptable in Practice)

**Location**:

`daemon/src/p2p/encryption.rs`:
- Uses `ChaCha20Poly1305`, each connection maintains:
  - `nonce: u64` + `nonce_buffer: [u8; 12]`
- Each encrypt/decrypt:
  - Writes 8 bytes of `nonce` into `nonce_buffer[0..8]`
  - Uses as AEAD nonce
  - Increments `nonce += 1` after use

`daemon/src/p2p/connection.rs`:
- Maintains `bytes_encrypted: AtomicUsize`
- **Automatically sends rotate-key packet and calls `Encryption::rotate_key` to regenerate symmetric key after cumulative encrypted data exceeds 1GB**
- Resets `bytes_encrypted` to 0

**Security Assessment**:

- Theoretically, if connection lives long enough, `nonce: u64` could overflow:
  - Rust debug mode panics
  - Release mode wraps around (starts from 0), causing nonce reuse
- In current implementation:
  - Each connection re-keys after sending 1GB data
  - To exhaust 2^64 nonce values requires sending **2^64 × (average packet size)** data - virtually impossible
  - In practice, connections rebuild due to network instability, reconnections, etc.

**Recommendations** (paranoia-level hardening, not mandatory):

- Add explicit check before `nonce += 1`:
  - If `nonce == u64::MAX`:
    - Return `EncryptionError::InvalidNonce`
    - Or force connection closure, requiring upper layer rebuild
- From security audit perspective, this "mathematically eliminates nonce reuse risk" provides better assurance

---

### Low Severity L1: Wallet Argon2 Parameters Could Be More Conservative (Current Parameters Are Reasonable)

**Location**:

`wallet/src/config.rs`:
```rust
let params = Params::new(15 * 1000, 16, 1, Some(PASSWORD_HASH_SIZE)).unwrap();
Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
```

Parameters:
- `PASSWORD_HASH_SIZE = 32`
- `SALT_SIZE = 32`
- Argon2 parameters:
  - memory = 15 MB
  - iterations = 16
  - parallelism = 1

**Assessment**:

- Algorithm is **Argon2id v0x13** - currently recommended mode
- Parameters represent **acceptable** security level for typical desktop environments, far superior to plain SHA-256

**Potential Optimizations**:

- For high-value wallets, consider:
  - Dynamically increase memory based on device capability (e.g., 64-128MB)
  - Expose CLI / configuration for user parameter tuning
- This is a "hardening suggestion," not an obvious vulnerability

---

## IV. Positive Security Findings (Worth Highlighting in Whitepaper)

### 1. Cryptography & Random Number Generation

- Wallet and P2P use:
  - `XChaCha20Poly1305` (wallet database encryption)
  - `ChaCha20Poly1305` / `AES-256-GCM` (XSWD Relayer)
  - Diffie-Hellman based on `x25519_dalek::StaticSecret` + `OsRng` key generation

- `common/src/crypto/random.rs` specifically encapsulates CSPRNG:
  - Explicit comments prohibiting production use of `thread_rng()` for nonce/key generation
  - Repository-wide search shows `thread_rng()` only in test helper code

### 2. Strict Serialization / Deserialization Boundary Checks

- `common/src/serializer/reader.rs` checks remaining length in all `read_bytes(n)` calls, preventing out-of-bounds reads

- Block headers contain multiple `// SECURITY FIX:` comments:
  - **Upper limit checks** on `parents_by_level` length to prevent DAG layer overflow causing serialization overflow and consensus forks
  - Limits max hexadecimal string length in `extra_nonce` deserialization to prevent memory DoS via excessively long strings

### 3. P2P Handshake & Network Isolation

`daemon/src/p2p/packet/handshake.rs`:
- Handshake contains `Network` + 16-byte `network_id`
- Length/value validation for version, node label, topological height fields

`daemon/src/p2p/mod.rs`:
- After receiving handshake, **must simultaneously satisfy**:
  - `handshake.network == local config network enum`
  - `handshake.network_id == config::NETWORK_ID`
- Otherwise connection is rejected, preventing cross-chain incorrect connections

### 4. WebSocket Security Module

`daemon/src/rpc/websocket/security.rs` provides:
- Origin whitelist
- Per-connection / per-IP message rate limiting
- Message size limits
- Subscription quota
- Optional API key authentication

This is a strong positive, indicating team consideration of WebSocket DoS and CSRF scenarios.

### 5. Storage & Reorganization Security Regression Tests

`daemon/tests/security/storage_security_tests.rs` specifically labels:
- "Balance updates must be atomic"
- "All caches must be invalidated on reorg"
- "Orphaned TX set must have maximum size"
- "skip_validation flags must be rejected on mainnet"

Combined with `daemon/src/core/storage/*` implementation, this indicates prior audit/production issue fixes now locked down through tests - an excellent practice.

### 6. Secure Default Configuration

- Daemon: `DEFAULT_RPC_BIND_ADDRESS = "127.0.0.1:8080"`
- Miner: `DEFAULT_DAEMON_ADDRESS = "127.0.0.1:8080"`
- P2P: Reject peers with mismatched network/network_id in handshake

These defaults prevent many "accidental misconfiguration exposure" issues (except XSWD noted above).

---

## V. Recommended Next Steps

If deploying to mainnet / large-scale public release, prioritize as follows:

### P0 (Urgent - Handle ASAP)

1. **Change XSWD default binding to 127.0.0.1**, or at minimum add documentation + startup warnings + explicit `--xswd-allow-remote` flag requirement

2. **Implement application signature verification** in XSWD protocol, activating `InvalidSignatureForApplicationData` and related error types

### P1 (Within 1-2 Iterations)

3. **Systematic `.unwrap()` / `.expect()` cleanup** in `daemon/src` and `common/src`:
   - Retain only for internal invariants + user-uncontrollable cases
   - Convert network input, RPC input, disk data reads to robust error handling, avoiding panic DoS

4. **Add more P2P layer fuzz / property tests**:
   - Manually construct out-of-order, duplicate, extreme size, malicious handshakes
   - Verify no infinite loops, memory explosions, or `panic!`

5. **Design unified "production deployment recommended template"** for RPC / WebSocket:
   - Add API key / JWT
   - Recommend placing behind reverse proxy
   - Provide nginx/caddy examples in documentation

### P2 (Medium-Long Term)

6. **Consensus core (GhostDAG + difficulty + storage) formal verification**:
   - Model checking / simulation tools (e.g., property testing / model checking frameworks)
   - Differential comparison testing with other implementations of same protocol (if available)

7. **Separate security audit for TAKO VM / eBPF execution layer**:
   - Bytecode verifier completeness
   - Syscall permissions
   - Gas metering coverage of all expensive operations

---

## VI. Request for Function-Level Detailed Reports

This response provides **overall risk landscape + specific findings + recommendations** in one reply. However, given:

- Files like `daemon/src/core/blockchain.rs` are 6000+ lines alone
- TAKO VM / contract execution logic not yet deeply examined

If you need **more granular code-level reports** (with function names/line numbers/call chains), specify which areas you prioritize, for example:

- "Dig deeper into consensus + block validation"
- "Focus on wallet signing / transaction construction / multisig logic"
- "Review AI mining / ZK module boundary checks"

I can continue with more detailed audits in those specific areas.

---

**End of Report**
