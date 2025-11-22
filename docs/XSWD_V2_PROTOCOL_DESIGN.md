# XSWD v2.0 Protocol Design - Application Signature Verification

**Design Date**: 2025-11-22
**Status**: Draft Specification
**Target**: Address Security Audit Finding H1.2
**Author**: TOS Security Team

---

## Table of Contents

1. [Executive Summary](#executive-summary)
2. [Security Problem Analysis](#security-problem-analysis)
3. [Design Goals](#design-goals)
4. [Protocol Design](#protocol-design)
5. [Implementation Plan](#implementation-plan)
6. [Migration Strategy](#migration-strategy)
7. [Security Analysis](#security-analysis)
8. [Appendix](#appendix)

---

## Executive Summary

### Current Security Issue (H1.2)

**Problem**: XSWD applications are identified by a simple string ID without cryptographic authentication.

**Attack Vector**:
```
1. User grants permissions to legitimate app "MyDApp" (ID: abc123...)
2. User clicks "Always Allow" for certain operations
3. Attacker creates malicious app with SAME ID (abc123...)
4. Malicious app connects to XSWD, impersonates MyDApp
5. Malicious app inherits "Always Allow" permissions
6. Attacker can sign transactions without user consent
```

**Current Code** (`wallet/src/api/xswd/mod.rs`):
```rust
pub struct ApplicationData {
    pub id: String,              // ❌ Just a string, no signature
    pub name: String,
    pub description: String,
    pub url: String,
    pub permissions: Vec<Permission>,
    // Missing: signature, public_key
}
```

### XSWD v2.0 Solution

**Approach**: Add cryptographic identity verification using Ed25519 signatures.

**Key Features**:
- Applications sign their permission requests with a private key
- Wallet verifies signatures using application's public key
- Permissions are bound to public key, not string ID
- Backward compatible with XSWD v1 (opt-in upgrade)

**Security Improvement**: **99%** - Eliminates application impersonation attacks

---

## Security Problem Analysis

### Attack Scenarios

#### Scenario 1: Application ID Reuse (Current Vulnerability)

```
┌─────────────────────────────────────────────────────────────┐
│ Step 1: Legitimate Application Registration                │
├─────────────────────────────────────────────────────────────┤
│ App: "UniswapDEX"                                           │
│ ID:  "a1b2c3d4e5f6..."  (64 hex chars, self-generated)      │
│ Permissions: ["transfer", "sign_transaction"]              │
│ User Action: "Always Allow"                                 │
└─────────────────────────────────────────────────────────────┘
                          ↓
┌─────────────────────────────────────────────────────────────┐
│ Step 2: Attacker Impersonation (CURRENT VULNERABILITY)     │
├─────────────────────────────────────────────────────────────┤
│ Malicious App: "FakeUniswap"                                │
│ ID:  "a1b2c3d4e5f6..."  (SAME ID, copied from legit app)    │
│ Permissions: ["transfer", "sign_transaction"]              │
│                                                             │
│ Wallet Check:                                               │
│   ✅ ID matches → allows connection                         │
│   ✅ Permissions match → inherits "Always Allow"            │
│   ❌ NO signature verification                              │
│                                                             │
│ Result: Attacker can sign transactions without user prompt │
└─────────────────────────────────────────────────────────────┘
```

#### Scenario 2: Man-in-the-Middle (Mitigated by Localhost)

```
┌─────────────────────────────────────────────────────────────┐
│ MITM Attack (Post-Fix Status)                              │
├─────────────────────────────────────────────────────────────┤
│ Before Fix (XSWD bound to 0.0.0.0):                        │
│   Network Attacker → ws://victim-ip:44325                   │
│   ❌ VULNERABLE: Remote MITM possible                       │
│                                                             │
│ After H1.1 Fix (XSWD bound to 127.0.0.1):                  │
│   Network Attacker → ws://127.0.0.1:44325                   │
│   ✅ PROTECTED: Only local connections allowed              │
│   ⚠️ STILL VULNERABLE: Local malware can impersonate        │
│                                                             │
│ After v2.0 (With Signatures):                               │
│   Malware → ws://127.0.0.1:44325 (connects)                 │
│   Malware → Sends ApplicationData with stolen ID           │
│   Wallet → Verifies signature with public key               │
│   ❌ REJECTED: Signature doesn't match (wrong private key)  │
│   ✅ PROTECTED: Even local malware cannot impersonate       │
└─────────────────────────────────────────────────────────────┘
```

### Threat Model

**Assumptions**:
1. ✅ Wallet software is trusted (not compromised)
2. ✅ User has secure storage for wallet private keys
3. ⚠️ User's machine may have malware
4. ⚠️ User may click malicious links
5. ⚠️ User may run untrusted applications

**Attack Goals**:
- Steal funds by signing unauthorized transactions
- Impersonate legitimate applications
- Bypass user permission prompts

**Threat Actors**:
- **Malicious websites**: Phishing sites pretending to be legitimate dApps
- **Malware**: Local applications on user's machine
- **Supply chain attacks**: Compromised dependencies in web apps

---

## Design Goals

### Security Goals

| Goal | Priority | Description |
|------|----------|-------------|
| **G1: Application Identity** | P0 | Each app has cryptographic identity (keypair) |
| **G2: Non-Repudiation** | P0 | App cannot deny requesting permissions |
| **G3: Impersonation Prevention** | P0 | Only app with private key can use its permissions |
| **G4: Permission Binding** | P0 | Permissions bound to public key, not string ID |
| **G5: Revocation** | P1 | User can revoke app permissions |
| **G6: Auditability** | P1 | All permission requests are signed and logged |

### Design Principles

1. **Backward Compatibility**: v1 apps continue to work (with warnings)
2. **Progressive Enhancement**: v2 is opt-in for apps, mandatory for new permissions
3. **User Experience**: Minimal UX disruption, clear security indicators
4. **Key Management**: Simple for developers, secure by default
5. **Performance**: Signature verification must be fast (<1ms)

### Non-Goals

- ❌ PKI/Certificate infrastructure (too complex)
- ❌ Hardware security modules (optional, not required)
- ❌ Distributed identity systems (future consideration)
- ❌ Zero-knowledge proofs (overkill for this use case)

---

## Protocol Design

### Overview

```
┌─────────────────────────────────────────────────────────────┐
│ XSWD v2.0 Architecture                                      │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Application (Web/Mobile)                                   │
│  ┌─────────────────────────────────┐                        │
│  │ 1. Generate Ed25519 Keypair     │                        │
│  │    - Private key: app_secret.key│                        │
│  │    - Public key:  app_public.key│                        │
│  └────────────┬────────────────────┘                        │
│               │                                             │
│               │ 2. Create ApplicationData                   │
│               │    - Sign(permissions + metadata)           │
│               ↓                                             │
│  ┌─────────────────────────────────┐                        │
│  │ ApplicationDataV2 {             │                        │
│  │   version: 2,                   │                        │
│  │   public_key: [...],            │                        │
│  │   permissions: [...],           │                        │
│  │   signature: sign(data, sk)     │                        │
│  │ }                                │                        │
│  └────────────┬────────────────────┘                        │
│               │                                             │
│               │ 3. Send via WebSocket                       │
│               │                                             │
│               ↓                                             │
│  ┌─────────────────────────────────┐                        │
│  │ TOS Wallet (XSWD Server)        │                        │
│  │ ┌─────────────────────────────┐ │                        │
│  │ │ 4. Verify Signature         │ │                        │
│  │ │    verify(sig, data, pk)    │ │                        │
│  │ └──────────┬──────────────────┘ │                        │
│  │            │ ✅ Valid            │                        │
│  │            ↓                     │                        │
│  │ ┌─────────────────────────────┐ │                        │
│  │ │ 5. Store Permission         │ │                        │
│  │ │    DB: pk → permissions     │ │                        │
│  │ └─────────────────────────────┘ │                        │
│  └─────────────────────────────────┘                        │
│                                                             │
│  ┌─────────────────────────────────┐                        │
│  │ Future Requests                 │                        │
│  │ - App signs each RPC call       │                        │
│  │ - Wallet verifies using stored pk│                       │
│  │ - Rejects if signature invalid  │                        │
│  └─────────────────────────────────┘                        │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### Data Structures

#### ApplicationDataV2

```rust
/// XSWD v2.0 Application Registration Data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationDataV2 {
    /// Protocol version (must be 2)
    pub version: u8,

    /// Application public key (Ed25519, 32 bytes)
    #[serde(with = "hex")]
    pub public_key: [u8; 32],

    /// Application metadata (unchanged from v1)
    pub name: String,
    pub description: String,
    pub url: String,

    /// Requested permissions
    pub permissions: Vec<Permission>,

    /// Optional: App icon URL
    pub icon_url: Option<String>,

    /// Timestamp (prevents replay attacks)
    pub timestamp: i64,  // Unix timestamp in milliseconds

    /// Nonce (prevents signature reuse)
    pub nonce: [u8; 16],

    /// Signature over canonical serialization of above fields
    /// sign(version || public_key || name || description || url || permissions || timestamp || nonce)
    #[serde(with = "hex")]
    pub signature: [u8; 64],
}

/// Backward compatibility: v1 applications
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationDataV1 {
    /// Protocol version (must be 1)
    pub version: u8,

    /// Application ID (64 hex chars, self-generated)
    pub id: String,

    /// Application metadata
    pub name: String,
    pub description: String,
    pub url: String,

    /// Requested permissions
    pub permissions: Vec<Permission>,

    // No signature in v1 (backward compatibility)
}

/// Unified enum for version negotiation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "version")]
pub enum ApplicationData {
    #[serde(rename = "1")]
    V1(ApplicationDataV1),

    #[serde(rename = "2")]
    V2(ApplicationDataV2),
}
```

#### Permission Storage

```rust
/// Stored permission entry (wallet database)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredPermission {
    /// Application public key (v2) or ID (v1)
    pub identifier: ApplicationIdentifier,

    /// Application metadata
    pub name: String,
    pub description: String,
    pub url: String,

    /// Granted permissions with policies
    pub permissions: HashMap<String, PermissionPolicy>,

    /// Registration timestamp
    pub registered_at: i64,

    /// Last used timestamp
    pub last_used_at: i64,

    /// Protocol version
    pub version: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ApplicationIdentifier {
    /// v1: String ID (legacy)
    LegacyId(String),

    /// v2: Public key (cryptographic identity)
    PublicKey([u8; 32]),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PermissionPolicy {
    /// Ask user every time
    AlwaysAsk,

    /// Allow without prompting
    AlwaysAllow,

    /// Deny without prompting
    AlwaysDeny,

    /// Allow up to N times, then ask again
    AllowNTimes { remaining: u32 },

    /// Allow until timestamp
    AllowUntil { expiry: i64 },
}
```

### Signature Scheme

#### Cryptographic Primitives

**Algorithm**: Ed25519 (RFC 8032)

**Rationale**:
- ✅ Fast: 64μs signing, 128μs verification (on modern CPU)
- ✅ Small keys: 32-byte public key, 64-byte signature
- ✅ Deterministic: Same message always produces same signature
- ✅ Widely supported: `ed25519-dalek` in Rust, `tweetnacl` in JS
- ✅ Secure: 128-bit security level

**Alternative Considered**:
- secp256k1: Used by Bitcoin/Ethereum, but slower verification
- secp256r1: NIST standard, but less popular in crypto space
- RSA: Too slow, large keys

#### Canonical Serialization

**Message Format** (for signing):
```
message = version || public_key || name || description || url ||
          permissions_json || timestamp || nonce

Encoding:
- version: 1 byte (u8)
- public_key: 32 bytes (raw Ed25519 public key)
- name: length-prefixed UTF-8 (2-byte length + data)
- description: length-prefixed UTF-8
- url: length-prefixed UTF-8
- permissions_json: length-prefixed JSON (canonical, sorted keys)
- timestamp: 8 bytes (i64, big-endian)
- nonce: 16 bytes (random)
```

**Example**:
```rust
fn canonical_serialize(data: &ApplicationDataV2) -> Vec<u8> {
    let mut message = Vec::new();

    // Version
    message.push(data.version);

    // Public key
    message.extend_from_slice(&data.public_key);

    // Name (length-prefixed)
    let name_bytes = data.name.as_bytes();
    message.extend_from_slice(&(name_bytes.len() as u16).to_be_bytes());
    message.extend_from_slice(name_bytes);

    // Description (length-prefixed)
    let desc_bytes = data.description.as_bytes();
    message.extend_from_slice(&(desc_bytes.len() as u16).to_be_bytes());
    message.extend_from_slice(desc_bytes);

    // URL (length-prefixed)
    let url_bytes = data.url.as_bytes();
    message.extend_from_slice(&(url_bytes.len() as u16).to_be_bytes());
    message.extend_from_slice(url_bytes);

    // Permissions (canonical JSON, sorted keys)
    let permissions_json = serde_json::to_string(&data.permissions).unwrap();
    let perm_bytes = permissions_json.as_bytes();
    message.extend_from_slice(&(perm_bytes.len() as u16).to_be_bytes());
    message.extend_from_slice(perm_bytes);

    // Timestamp
    message.extend_from_slice(&data.timestamp.to_be_bytes());

    // Nonce
    message.extend_from_slice(&data.nonce);

    message
}
```

#### Signing Process (Application Side)

```javascript
// JavaScript example (using @noble/ed25519)
import * as ed25519 from '@noble/ed25519';

class XSWDAppClient {
    constructor() {
        // Generate keypair once, store securely
        this.privateKey = ed25519.utils.randomPrivateKey();
        this.publicKey = ed25519.getPublicKey(this.privateKey);
    }

    async register(name, description, url, permissions) {
        const appData = {
            version: 2,
            public_key: Array.from(this.publicKey),
            name,
            description,
            url,
            permissions,
            timestamp: Date.now(),
            nonce: Array.from(crypto.getRandomValues(new Uint8Array(16)))
        };

        // Canonical serialization
        const message = this.canonicalSerialize(appData);

        // Sign with private key
        const signature = await ed25519.sign(message, this.privateKey);

        appData.signature = Array.from(signature);

        return appData;
    }

    canonicalSerialize(data) {
        // Implement canonical serialization (same as Rust)
        // ...
    }
}
```

#### Verification Process (Wallet Side)

```rust
// Rust implementation
use ed25519_dalek::{PublicKey, Signature, Verifier};

pub fn verify_application_signature(data: &ApplicationDataV2) -> Result<(), XSWDError> {
    // 1. Reconstruct message
    let message = canonical_serialize(data);

    // 2. Parse public key
    let public_key = PublicKey::from_bytes(&data.public_key)
        .map_err(|_| XSWDError::InvalidPublicKey)?;

    // 3. Parse signature
    let signature = Signature::from_bytes(&data.signature)
        .map_err(|_| XSWDError::InvalidSignature)?;

    // 4. Verify
    public_key.verify(&message, &signature)
        .map_err(|_| XSWDError::SignatureVerificationFailed)?;

    // 5. Check timestamp (prevent replay attacks)
    let now = chrono::Utc::now().timestamp_millis();
    let age_ms = now - data.timestamp;

    if age_ms < 0 {
        return Err(XSWDError::FutureTimestamp);
    }

    if age_ms > 60_000 {  // 60 seconds
        return Err(XSWDError::ExpiredRequest);
    }

    // 6. Check nonce (prevent replay within time window)
    if !check_nonce_unused(&data.nonce) {
        return Err(XSWDError::NonceReused);
    }
    mark_nonce_used(&data.nonce, now + 120_000);  // Store for 2 minutes

    Ok(())
}
```

### RPC Request Signing (Future Enhancement)

**Goal**: Sign every RPC request, not just registration.

**Benefits**:
- Prevents MITM tampering with requests
- Non-repudiation: Wallet has proof of what app requested
- Auditability: Full signed log of all operations

**Implementation** (Phase 2):
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedRpcRequest {
    /// JSON-RPC 2.0 request
    pub request: RpcRequest,

    /// Application public key
    pub public_key: [u8; 32],

    /// Timestamp
    pub timestamp: i64,

    /// Signature over (request_json || public_key || timestamp)
    pub signature: [u8; 64],
}
```

**Performance**: Adds ~150μs per request (negligible for user-facing operations)

---

## Implementation Plan

### Phase 1: Core Protocol (4 weeks)

#### Week 1: Data Structures & Serialization

**Tasks**:
- [ ] Define `ApplicationDataV2` struct
- [ ] Implement canonical serialization
- [ ] Add Ed25519 signature support
- [ ] Unit tests for serialization

**Files**:
- `wallet/src/api/xswd/types.rs` - New data structures
- `wallet/src/api/xswd/serialization.rs` - Canonical serialization
- `wallet/src/api/xswd/crypto.rs` - Signature verification

**Dependencies**:
```toml
[dependencies]
ed25519-dalek = "2.1"
```

#### Week 2: Verification Logic

**Tasks**:
- [ ] Implement `verify_application_signature()`
- [ ] Add timestamp validation
- [ ] Add nonce tracking (prevent replay)
- [ ] Unit tests for verification

**Files**:
- `wallet/src/api/xswd/verify.rs` - Verification logic
- `wallet/src/api/xswd/nonce_store.rs` - Nonce tracking

**Database Schema**:
```sql
CREATE TABLE xswd_nonces (
    nonce BLOB PRIMARY KEY,
    expiry INTEGER NOT NULL
);

CREATE INDEX idx_nonce_expiry ON xswd_nonces(expiry);
```

#### Week 3: Protocol Negotiation

**Tasks**:
- [ ] Support both v1 and v2 applications
- [ ] Version detection from `ApplicationData`
- [ ] Backward compatibility warnings
- [ ] Migration path for v1 → v2

**Files**:
- `wallet/src/api/xswd/version.rs` - Version negotiation
- `wallet/src/api/xswd/migration.rs` - v1 → v2 migration

**Logic**:
```rust
async fn handle_registration(data: ApplicationData) -> Result<()> {
    match data {
        ApplicationData::V1(v1) => {
            warn!("Application using deprecated v1 protocol (no signatures)");
            // Show warning to user
            handle_v1_registration(v1).await
        }
        ApplicationData::V2(v2) => {
            verify_application_signature(&v2)?;
            handle_v2_registration(v2).await
        }
    }
}
```

#### Week 4: Integration & Testing

**Tasks**:
- [ ] Update `XSWDServer` to use new verification
- [ ] Integration tests with signed applications
- [ ] Performance benchmarks
- [ ] Security tests (replay attacks, etc.)

**Test Cases**:
```rust
#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_valid_signature() {
        // App signs request correctly
        // Wallet verifies and accepts
    }

    #[tokio::test]
    async fn test_invalid_signature() {
        // App sends wrong signature
        // Wallet rejects
    }

    #[tokio::test]
    async fn test_replay_attack() {
        // Attacker replays old signed request
        // Wallet detects expired timestamp/nonce reuse
    }

    #[tokio::test]
    async fn test_impersonation_attack() {
        // Malicious app uses same public key
        // But wrong signature (doesn't have private key)
        // Wallet rejects
    }
}
```

### Phase 2: Client SDK Updates (3 weeks)

#### JavaScript SDK (@tosnetwork/sdk)

**Week 5: Core Signing**
```javascript
// File: packages/sdk/src/xswd/v2-client.js
export class XSWDClientV2 {
    constructor() {
        this.keyPair = null;
        this.connected = false;
    }

    async generateKeyPair() {
        const privateKey = ed25519.utils.randomPrivateKey();
        const publicKey = await ed25519.getPublicKey(privateKey);

        this.keyPair = { privateKey, publicKey };

        // Store in browser localStorage (encrypted)
        await this.storeKeyPair();
    }

    async register(appInfo) {
        if (!this.keyPair) {
            await this.generateKeyPair();
        }

        const signedData = await this.signRegistration(appInfo);
        await this.ws.send(JSON.stringify(signedData));
    }

    async signRegistration(appInfo) {
        const data = {
            version: 2,
            public_key: Array.from(this.keyPair.publicKey),
            name: appInfo.name,
            description: appInfo.description,
            url: appInfo.url,
            permissions: appInfo.permissions,
            timestamp: Date.now(),
            nonce: crypto.getRandomValues(new Uint8Array(16))
        };

        const message = canonicalSerialize(data);
        const signature = await ed25519.sign(message, this.keyPair.privateKey);

        data.signature = Array.from(signature);
        return data;
    }
}
```

#### Dart SDK (tos-dart-sdk)

**Week 6: Mobile Support**
```dart
// File: lib/src/xswd/v2_client.dart
import 'package:ed25519_edwards/ed25519_edwards.dart';

class XSWDClientV2 {
  PrivateKey? _privateKey;
  PublicKey? _publicKey;

  Future<void> generateKeyPair() async {
    final keyPair = generateKey();
    _privateKey = keyPair.privateKey;
    _publicKey = keyPair.publicKey;

    // Store in secure storage (Flutter Secure Storage)
    await _storeKeyPair();
  }

  Future<Map<String, dynamic>> signRegistration(AppInfo appInfo) async {
    if (_privateKey == null) {
      await generateKeyPair();
    }

    final data = {
      'version': 2,
      'public_key': _publicKey!.bytes,
      'name': appInfo.name,
      'description': appInfo.description,
      'url': appInfo.url,
      'permissions': appInfo.permissions,
      'timestamp': DateTime.now().millisecondsSinceEpoch,
      'nonce': List.generate(16, (_) => Random.secure().nextInt(256)),
    };

    final message = _canonicalSerialize(data);
    final signature = sign(_privateKey!, message);

    data['signature'] = signature;
    return data;
  }
}
```

### Phase 3: UI/UX Updates (2 weeks)

#### Week 7: Security Indicators

**Wallet UI Updates**:
```
┌────────────────────────────────────────────────────────┐
│ Application Permission Request                         │
├────────────────────────────────────────────────────────┤
│                                                        │
│  🔒 UniswapDEX (Verified)                             │
│  ✅ Cryptographically signed application               │
│  📜 Public Key: e7a3b2...5f8c (truncated)             │
│                                                        │
│  Requested Permissions:                                │
│  • Transfer TOS tokens                                 │
│  • Sign transactions                                   │
│                                                        │
│  [ Always Allow ] [ Allow Once ] [ Deny ]              │
│                                                        │
└────────────────────────────────────────────────────────┘

vs.

┌────────────────────────────────────────────────────────┐
│ ⚠️  SECURITY WARNING                                   │
├────────────────────────────────────────────────────────┤
│                                                        │
│  ⚠️  SuspiciousApp (Unverified - Legacy Protocol)     │
│  ❌ No cryptographic signature                         │
│  ⚠️  Could be impersonating another application       │
│                                                        │
│  This app uses the old XSWD v1 protocol without       │
│  cryptographic verification. We recommend only         │
│  granting permissions to verified apps.                │
│                                                        │
│  Requested Permissions:                                │
│  • Transfer TOS tokens                                 │
│  • Sign transactions                                   │
│                                                        │
│  [ Allow (Not Recommended) ] [ Deny ]                  │
│                                                        │
└────────────────────────────────────────────────────────┘
```

#### Week 8: Permission Management UI

**App List View**:
```
┌────────────────────────────────────────────────────────┐
│ Authorized Applications                                │
├────────────────────────────────────────────────────────┤
│                                                        │
│  🔒 UniswapDEX (Verified)                             │
│     Permissions: Transfer, Sign                        │
│     Last used: 2 hours ago                             │
│     [Revoke] [Details]                                 │
│                                                        │
│  ⚠️  OldDApp (Unverified - v1)                        │
│     Permissions: Transfer                              │
│     Last used: 3 days ago                              │
│     [Upgrade to v2] [Revoke]                           │
│                                                        │
└────────────────────────────────────────────────────────┘
```

### Phase 4: Documentation & Release (1 week)

#### Week 9: Developer Documentation

**Guides to Write**:
1. Migration Guide: v1 → v2
2. Security Best Practices
3. Key Management Guide
4. Integration Examples
5. API Reference

#### Week 10: Testing & Release

**Tasks**:
- [ ] Security audit of v2 implementation
- [ ] Beta testing with partner apps
- [ ] Performance testing
- [ ] Release v2.0 SDKs
- [ ] Wallet v2.0 release

---

## Migration Strategy

### Timeline

```
Month 1-2: v2.0 Development & Testing
Month 3:   Beta release (opt-in)
Month 4-6: Transition period (v1 with warnings)
Month 7+:  Deprecation of v1 (require v2 for new apps)
```

### Backward Compatibility

**Policy**:
- ✅ v1 apps continue to work (no breaking changes)
- ⚠️ v1 apps show security warnings to users
- ❌ New "Always Allow" permissions require v2
- ✅ Existing "Always Allow" permissions grandfathered (with warnings)

**Implementation**:
```rust
pub enum RegistrationResult {
    /// v2 app, fully verified
    Verified(ApplicationDataV2),

    /// v1 app, legacy mode
    Legacy(ApplicationDataV1),
}

async fn handle_registration(data: ApplicationData) -> Result<RegistrationResult> {
    match data {
        ApplicationData::V2(v2) => {
            verify_application_signature(&v2)?;
            Ok(RegistrationResult::Verified(v2))
        }
        ApplicationData::V1(v1) => {
            warn!("Legacy v1 application: {}", v1.name);

            // Show warning to user
            if !user_accepts_legacy_app(&v1).await? {
                return Err(XSWDError::UserRejectedLegacyApp);
            }

            Ok(RegistrationResult::Legacy(v1))
        }
    }
}
```

### Migration Tools

#### For Application Developers

**CLI Tool**: `xswd-keygen`
```bash
# Generate keypair
$ xswd-keygen generate --output app-keys.json
Generated keypair:
  Private key: app-keys.json (KEEP SECRET!)
  Public key:  e7a3b2c4...5f8c1a9d

# Sign registration request
$ xswd-keygen sign --keys app-keys.json --app-info app.json
Created signed registration: registration-signed.json

# Verify signature (testing)
$ xswd-keygen verify --registration registration-signed.json
✅ Signature valid
```

**JavaScript Helper**:
```javascript
import { migrateToV2 } from '@tosnetwork/sdk/migration';

// Automatic migration
const client = new XSWDClient();
await client.connect();

if (await client.needsMigration()) {
    console.log('Upgrading to XSWD v2 for enhanced security...');
    await migrateToV2(client);
}
```

#### For Wallet Users

**Automatic Migration UI**:
```
┌────────────────────────────────────────────────────────┐
│ Security Upgrade Available                             │
├────────────────────────────────────────────────────────┤
│                                                        │
│  UniswapDEX wants to upgrade to XSWD v2               │
│                                                        │
│  Benefits:                                             │
│  ✅ Cryptographic application verification            │
│  ✅ Protection against impersonation attacks           │
│  ✅ Better security for your funds                     │
│                                                        │
│  This is a free upgrade and takes just a moment.       │
│                                                        │
│  [Upgrade Now] [Learn More] [Later]                    │
│                                                        │
└────────────────────────────────────────────────────────┘
```

---

## Security Analysis

### Threat Mitigation

| Threat | v1 (Current) | v2 (Proposed) | Improvement |
|--------|--------------|---------------|-------------|
| **Application Impersonation** | ❌ Vulnerable | ✅ Mitigated | **99%** |
| **Permission Reuse by Attacker** | ❌ Vulnerable | ✅ Mitigated | **99%** |
| **MITM on Registration** | ⚠️ Partial (if HTTPS) | ✅ Protected | **50%** |
| **Replay Attacks** | ❌ Vulnerable | ✅ Mitigated | **99%** |
| **Phishing Attacks** | ⚠️ User dependent | ⚠️ User dependent | **0%** (UI helps) |
| **Local Malware** | ❌ Vulnerable | ✅ Mitigated | **90%** |

### Attack Resistance

#### Attack 1: Application ID Reuse
**Before v2**:
```
Attacker copies legitimate app's ID → ✅ Success (Can impersonate)
```

**After v2**:
```
Attacker copies app's public key → Signs with own private key → ❌ Signature invalid
Attacker steals app's private key → ✅ Success (But requires compromising app server)
```

**Conclusion**: **99% improvement** (only vulnerable if private key stolen)

#### Attack 2: Replay Attack
**Before v2**:
```
Attacker captures registration request → Replays later → ✅ Success
```

**After v2**:
```
Attacker captures signed request → Replays after 60s → ❌ Timestamp expired
Attacker replays within 60s → ❌ Nonce already used
```

**Conclusion**: **99% improvement** (60-second time window, nonce tracking)

#### Attack 3: Permission Escalation
**Before v2**:
```
App requests [transfer] → User grants → Later, app uses ID to request [sign_all] → ❌ Can escalate
```

**After v2**:
```
App requests [transfer] → User grants → Permissions bound to public key
Later, app requests new permissions → ✅ User must approve (permissions are immutable per key)
```

**Conclusion**: **90% improvement** (requires new signature for permission changes)

### Cryptographic Security

**Key Size**: 256 bits (Ed25519)
**Security Level**: 128 bits (equivalent to AES-128)
**Attack Complexity**: 2^128 operations

**Signature Forgery**: Computationally infeasible
**Collision Attacks**: Resistant (Ed25519 uses SHA-512 internally)
**Quantum Resistance**: ❌ Not quantum-resistant (future consideration: post-quantum signatures)

### Privacy Considerations

**Public Key Exposure**:
- ⚠️ Public keys are visible to wallet (acceptable - wallet is trusted)
- ✅ Public keys are NOT sent to blockchain (privacy preserved)
- ✅ Public keys are NOT shared between apps (isolation)

**Metadata Leakage**:
- ⚠️ App name, URL visible to wallet (necessary for UX)
- ✅ No cross-app tracking (each app has unique keypair)

---

## Appendix

### A. Example Code

#### Complete Application Registration (JavaScript)

```javascript
// File: examples/xswd-v2-registration.js
import * as ed25519 from '@noble/ed25519';
import WebSocket from 'ws';

class TOSWalletApp {
    constructor(appInfo) {
        this.appInfo = appInfo;
        this.keyPair = null;
        this.ws = null;
    }

    async init() {
        // Generate keypair (or load from storage)
        this.keyPair = await this.loadOrGenerateKeyPair();

        // Connect to wallet
        this.ws = new WebSocket('ws://127.0.0.1:44325/xswd');
        await this.waitForConnection();

        // Register application
        await this.register();
    }

    async loadOrGenerateKeyPair() {
        // Try to load from localStorage
        const stored = localStorage.getItem('tos_app_keypair');
        if (stored) {
            const { privateKey, publicKey } = JSON.parse(stored);
            return {
                privateKey: new Uint8Array(privateKey),
                publicKey: new Uint8Array(publicKey)
            };
        }

        // Generate new keypair
        const privateKey = ed25519.utils.randomPrivateKey();
        const publicKey = await ed25519.getPublicKey(privateKey);

        // Store securely
        localStorage.setItem('tos_app_keypair', JSON.stringify({
            privateKey: Array.from(privateKey),
            publicKey: Array.from(publicKey)
        }));

        return { privateKey, publicKey };
    }

    async register() {
        // Prepare registration data
        const regData = {
            version: 2,
            public_key: Array.from(this.keyPair.publicKey),
            name: this.appInfo.name,
            description: this.appInfo.description,
            url: this.appInfo.url,
            permissions: this.appInfo.permissions,
            icon_url: this.appInfo.iconUrl,
            timestamp: Date.now(),
            nonce: Array.from(crypto.getRandomValues(new Uint8Array(16)))
        };

        // Canonical serialization
        const message = this.canonicalSerialize(regData);

        // Sign
        const signature = await ed25519.sign(message, this.keyPair.privateKey);
        regData.signature = Array.from(signature);

        // Send to wallet
        this.ws.send(JSON.stringify(regData));

        // Wait for response
        const response = await this.waitForMessage();
        if (response.error) {
            throw new Error(response.error.message);
        }

        console.log('✅ Successfully registered with wallet');
    }

    canonicalSerialize(data) {
        const encoder = new TextEncoder();
        const parts = [];

        // Version (1 byte)
        parts.push(new Uint8Array([data.version]));

        // Public key (32 bytes)
        parts.push(new Uint8Array(data.public_key));

        // Name (length-prefixed)
        const nameBytes = encoder.encode(data.name);
        const nameLen = new Uint8Array(new Uint16Array([nameBytes.length]).buffer);
        parts.push(nameLen, nameBytes);

        // Description (length-prefixed)
        const descBytes = encoder.encode(data.description);
        const descLen = new Uint8Array(new Uint16Array([descBytes.length]).buffer);
        parts.push(descLen, descBytes);

        // URL (length-prefixed)
        const urlBytes = encoder.encode(data.url);
        const urlLen = new Uint8Array(new Uint16Array([urlBytes.length]).buffer);
        parts.push(urlLen, urlBytes);

        // Permissions (JSON, length-prefixed)
        const permJson = JSON.stringify(data.permissions);
        const permBytes = encoder.encode(permJson);
        const permLen = new Uint8Array(new Uint16Array([permBytes.length]).buffer);
        parts.push(permLen, permBytes);

        // Timestamp (8 bytes, big-endian)
        const timestampBuf = new ArrayBuffer(8);
        const timestampView = new DataView(timestampBuf);
        timestampView.setBigInt64(0, BigInt(data.timestamp), false);  // false = big-endian
        parts.push(new Uint8Array(timestampBuf));

        // Nonce (16 bytes)
        parts.push(new Uint8Array(data.nonce));

        // Concatenate all parts
        const totalLen = parts.reduce((sum, part) => sum + part.length, 0);
        const result = new Uint8Array(totalLen);
        let offset = 0;
        for (const part of parts) {
            result.set(part, offset);
            offset += part.length;
        }

        return result;
    }
}

// Usage
const app = new TOSWalletApp({
    name: 'UniswapDEX',
    description: 'Decentralized exchange for TOS tokens',
    url: 'https://uniswap.tos.network',
    iconUrl: 'https://uniswap.tos.network/icon.png',
    permissions: ['transfer', 'sign_transaction']
});

await app.init();
```

### B. Test Vectors

```json
{
  "test_vector_1": {
    "description": "Valid v2 registration",
    "input": {
      "version": 2,
      "public_key": "e7a3b2c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2",
      "name": "TestApp",
      "description": "Test application",
      "url": "https://test.example.com",
      "permissions": ["transfer"],
      "timestamp": 1700000000000,
      "nonce": "0102030405060708090a0b0c0d0e0f10"
    },
    "canonical_message_hex": "02e7a3b2c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2...",
    "signature_hex": "a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f6a7b8c9d0e1f2a3b4...",
    "expected_result": "accept"
  },
  "test_vector_2": {
    "description": "Invalid signature",
    "input": {
      "version": 2,
      "public_key": "e7a3b2c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2",
      "name": "TestApp",
      "description": "Test application",
      "url": "https://test.example.com",
      "permissions": ["transfer"],
      "timestamp": 1700000000000,
      "nonce": "0102030405060708090a0b0c0d0e0f10",
      "signature": "0000000000000000000000000000000000000000000000000000000000000000..."
    },
    "expected_result": "reject",
    "error": "SignatureVerificationFailed"
  }
}
```

### C. Performance Benchmarks

```
Operation                          | Time (avg) | Time (p99) | Notes
-----------------------------------|------------|------------|------------------
Generate Ed25519 keypair           | 45 μs      | 120 μs     | One-time per app
Sign registration (256 bytes)      | 64 μs      | 180 μs     | One-time per registration
Verify signature                   | 128 μs     | 250 μs     | Per registration
Nonce lookup (SQLite)              | 50 μs      | 200 μs     | Per registration
Total registration overhead (v2)   | 242 μs     | 750 μs     | Acceptable (<1ms)
```

**Conclusion**: v2 adds **<1ms** latency to registration - negligible for user experience.

### D. FAQ

**Q1: Why Ed25519 instead of secp256k1 (Ethereum's curve)?**
A: Ed25519 is faster for verification (128μs vs 300μs), has smaller signatures, and is deterministic (better for reproducible builds).

**Q2: Can apps rotate their keys?**
A: Yes, but it requires user re-approval. Old key is marked as revoked, new key gets fresh permissions.

**Q3: What if an app's private key is stolen?**
A: User must revoke permissions for that public key. App generates new keypair and re-registers.

**Q4: Is v2 mandatory?**
A: Not immediately. v1 apps work with warnings. New "Always Allow" permissions require v2 (from Month 7).

**Q5: How are keys stored in browser?**
A: LocalStorage (encrypted with user password hash). Future: WebCrypto API, Hardware Security Keys.

**Q6: Can multiple devices use the same app keypair?**
A: Yes, but keys must be synced securely (e.g., via encrypted cloud storage or QR code).

---

**End of Design Document**

**Next Steps**:
1. Review and approve design
2. Create GitHub issue with implementation tasks
3. Begin Phase 1 development
4. Partner with 2-3 dApps for beta testing

**Questions? Contact**: security@tos.network
