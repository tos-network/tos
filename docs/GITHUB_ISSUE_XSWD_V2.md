# GitHub Issue: Implement XSWD v2.0 with Application Signature Verification

**Title**: Implement XSWD v2.0 - Direct Upgrade with Ed25519 Signature Verification

**Labels**: `enhancement`, `security`, `P0`, `XSWD`

**Milestone**: Security Audit Response - H1.2

**Assignees**: TOS Development Team

---

## Summary

Implement XSWD v2.0 protocol with Ed25519 signature-based application authentication to address **H1.2 High-Severity Finding** from security audit report.

**Decision**: Direct upgrade to ApplicationDataV2 (no backward compatibility) based on impact analysis showing zero production deployments and only 3-4 internal test applications affected.

**Timeline**: 3 weeks (vs 10 weeks for backward-compatible approach)

---

## Background

### Security Audit Finding (H1.2)

**Severity**: High
**Status**: Deferred (protocol design required)
**Reference**: `docs/security_audit_report_20251122_en.md`

**Problem**: XSWD protocol has error types for signature verification (`ApplicationPermissionsNotSigned`, `InvalidSignatureForApplicationData`), but actual `ApplicationData` struct lacks signature fields and verification logic.

**Attack Scenario**: Malicious applications can impersonate legitimate apps by reusing the same string ID.

**Current Mitigation**: H1.1 fix (localhost-only binding) significantly reduces attack surface, but does not prevent impersonation if user enables remote access.

### Design Documents

- ✅ **Protocol Design**: `docs/XSWD_V2_PROTOCOL_DESIGN.md`
- ✅ **Impact Analysis**: `docs/XSWD_V2_UPGRADE_IMPACT_ANALYSIS.md`
- ✅ **Usage Analysis**: `docs/XSWD_USAGE_ANALYSIS.md`

---

## Objectives

1. **Security**: Bind application permissions to cryptographic public keys instead of string IDs
2. **Replay Protection**: Prevent replay attacks using timestamp + nonce mechanism
3. **Simplicity**: Clean implementation without legacy v1 support
4. **Fast Deployment**: Address H1.2 finding in 3 weeks

---

## Technical Specification

### ApplicationData Changes

#### Before (v1)
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationData {
    pub id: String,
    pub name: String,
    pub description: String,
    pub permissions: Permissions,
}
```

#### After (v2)
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationData {
    // Existing fields (unchanged)
    pub id: String,
    pub name: String,
    pub description: String,
    pub permissions: Permissions,

    // NEW: Security fields
    #[serde(with = "hex")]
    pub public_key: [u8; 32],  // Ed25519 public key

    pub timestamp: u64,         // Unix timestamp (seconds)
    pub nonce: u64,             // Random nonce for replay protection

    #[serde(with = "hex")]
    pub signature: [u8; 64],    // Ed25519 signature over all fields
}
```

### Signature Verification Algorithm

```rust
fn verify_application(app_data: &ApplicationData) -> Result<(), XSWDError> {
    use ed25519_dalek::{PublicKey, Signature, Verifier};

    // 1. Verify timestamp is recent (within 5 minutes)
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    if now.abs_diff(app_data.timestamp) > 300 {
        return Err(XSWDError::ApplicationPermissionsNotSigned);
    }

    // 2. Verify signature
    let public_key = PublicKey::from_bytes(&app_data.public_key)
        .map_err(|_| XSWDError::InvalidSignatureForApplicationData)?;

    let signature = Signature::from_bytes(&app_data.signature)
        .map_err(|_| XSWDError::InvalidSignatureForApplicationData)?;

    let message = serialize_for_signing(app_data);

    public_key.verify(&message, &signature)
        .map_err(|_| XSWDError::InvalidSignatureForApplicationData)?;

    Ok(())
}

fn serialize_for_signing(data: &ApplicationData) -> Vec<u8> {
    // Deterministic serialization (concatenate all fields except signature)
    let mut buf = Vec::new();
    buf.extend_from_slice(data.id.as_bytes());
    buf.extend_from_slice(data.name.as_bytes());
    buf.extend_from_slice(data.description.as_bytes());
    buf.extend_from_slice(&serialize_permissions(&data.permissions));
    buf.extend_from_slice(&data.public_key);
    buf.extend_from_slice(&data.timestamp.to_le_bytes());
    buf.extend_from_slice(&data.nonce.to_le_bytes());
    buf
}
```

---

## Implementation Plan (3 Weeks)

### Week 1: Wallet XSWD Server

#### Day 1-2: Update ApplicationData struct
- [ ] **File**: `wallet/src/api/xswd/mod.rs`
  - [ ] Add new fields to `ApplicationData` struct
  - [ ] Add `serialize_for_signing()` helper function
  - [ ] Update JSON serialization tests

- [ ] **File**: `wallet/Cargo.toml`
  - [ ] Add dependency: `ed25519-dalek = "2.0"`
  - [ ] Add dependency: `hex = "0.4"` (for hex serialization)

#### Day 3-4: Implement signature verification
- [ ] **File**: `wallet/src/api/xswd/verification.rs` (NEW)
  - [ ] Implement `verify_application()` function
  - [ ] Implement `serialize_for_signing()` function
  - [ ] Add timestamp validation (5-minute window)
  - [ ] Add nonce tracking (prevent replay within session)

- [ ] **File**: `wallet/src/api/xswd/handler.rs`
  - [ ] Update `handle_register_application()` to call verification
  - [ ] Add error handling for verification failures
  - [ ] Update permission storage to bind to public_key instead of app_id

#### Day 5: Testing
- [ ] **Unit tests**: Signature verification logic
  - [ ] Valid signature → Success
  - [ ] Invalid signature → Error
  - [ ] Expired timestamp → Error
  - [ ] Replay attack (same nonce) → Error

- [ ] **Integration tests**: Full registration flow
  - [ ] Test with valid ApplicationData
  - [ ] Test with tampered fields
  - [ ] Test with old timestamp

- [ ] **Build verification**:
  ```bash
  cargo build --package tos_wallet --lib
  cargo test --package tos_wallet --lib api::xswd
  cargo clippy --package tos_wallet -- -D warnings
  ```

**Week 1 Deliverable**: ✅ Wallet XSWD server validates Ed25519 signatures

---

### Week 2: Client SDKs

#### Day 1-2: Update tos-js-sdk
- [ ] **File**: `@tosnetwork/sdk/xswd/websocket.js`
  - [ ] Add `@noble/ed25519` dependency
  - [ ] Implement keypair generation
  - [ ] Implement `serializeForSigning()` function
  - [ ] Implement `signApplicationData()` function
  - [ ] Update `connect()` to generate and sign ApplicationData

- [ ] **File**: `@tosnetwork/sdk/xswd/types.ts`
  - [ ] Update `ApplicationData` TypeScript interface
  - [ ] Add `ApplicationDataV2` type

- [ ] **Example usage**:
  ```javascript
  // SDK automatically handles crypto
  const xswd = new XSWD();
  await xswd.connect('ws://127.0.0.1:44325', {
      appId: 'my-app',
      appName: 'My Application',
      description: 'My dApp',
      permissions: { wallet: { getAddress: true } }
  });
  ```

- [ ] **Testing**:
  ```bash
  npm install
  npm test
  npm run build
  ```

#### Day 3-4: Update tos-dart-sdk
- [ ] **File**: `tos-dart-sdk/lib/xswd/client.dart`
  - [ ] Add `ed25519_edwards: ^0.3.0` to `pubspec.yaml`
  - [ ] Implement keypair generation
  - [ ] Implement signing logic
  - [ ] Update `connect()` method

- [ ] **File**: `tos-dart-sdk/lib/xswd/types.dart`
  - [ ] Update `ApplicationData` class
  - [ ] Add `toSigningMessage()` method

- [ ] **Testing**:
  ```bash
  flutter pub get
  flutter test
  ```

#### Day 5: SDK Integration Testing
- [ ] Test tos-js-sdk → wallet registration
- [ ] Test tos-dart-sdk → wallet registration
- [ ] Verify signature verification in wallet logs
- [ ] Performance benchmarks (signature generation < 5ms)

**Week 2 Deliverable**: ✅ Both SDKs generate valid Ed25519 signatures

---

### Week 3: Test Applications & Documentation

#### Day 1: Update tos-chatgpt-app
- [ ] **File**: `tos-chatgpt-app/package.json`
  - [ ] Update `@tosnetwork/sdk` to v2.0.0

- [ ] **File**: `tos-chatgpt-app/src/wallet/xswd.js`
  - [ ] Update connection code (minimal changes, SDK handles crypto)
  - [ ] Test registration flow

- [ ] **Testing**:
  ```bash
  npm install
  npm run dev
  # Verify wallet connection works
  ```

#### Day 2-3: Documentation
- [ ] **Update**: `docs/XSWD_V2_PROTOCOL_DESIGN.md`
  - [ ] Add "Implementation Status: ✅ Complete" section
  - [ ] Add implementation notes

- [ ] **Update**: `docs/SECURITY_AUDIT_STATUS.md`
  - [ ] Change H1.2 status from ⚠️ DEFERRED to ✅ FIXED
  - [ ] Update overall compliance percentage

- [ ] **Create**: `docs/XSWD_V2_MIGRATION_GUIDE.md`
  - [ ] SDK upgrade instructions
  - [ ] Code examples (before/after)
  - [ ] Troubleshooting guide

- [ ] **Create**: `docs/XSWD_V2_SECURITY_ANALYSIS.md`
  - [ ] Threat model
  - [ ] Security properties
  - [ ] Attack scenarios prevented

#### Day 4-5: End-to-End Testing & Verification
- [ ] **E2E Test Scenario 1**: Registration flow
  ```bash
  # Terminal 1: Start wallet
  ./target/debug/tos_wallet --enable-xswd

  # Terminal 2: Test tos-chatgpt-app
  cd ~/tos-network/tos-chatgpt-app
  npm run test:xswd
  ```

- [ ] **E2E Test Scenario 2**: Permission requests
  - [ ] Test getAddress permission
  - [ ] Test signTransaction permission
  - [ ] Verify public_key binding in wallet storage

- [ ] **E2E Test Scenario 3**: Attack scenarios
  - [ ] Attempt signature tampering → Should fail
  - [ ] Attempt timestamp manipulation → Should fail
  - [ ] Attempt nonce replay → Should fail
  - [ ] Attempt different app using same public_key → Should fail (different signature)

- [ ] **Performance Testing**:
  - [ ] Registration latency < 100ms
  - [ ] Signature verification < 5ms
  - [ ] No memory leaks in long-running sessions

- [ ] **Security Review**:
  - [ ] Code review by security team
  - [ ] Verify no signature bypass paths
  - [ ] Verify nonce tracking prevents replays

**Week 3 Deliverable**: ✅ Complete XSWD v2.0 implementation with full documentation

---

## Acceptance Criteria

### Security Requirements
- [ ] All application registrations require valid Ed25519 signatures
- [ ] Timestamp validation prevents old signatures (5-minute window)
- [ ] Nonce tracking prevents replay attacks within session
- [ ] Permissions are bound to public keys, not string IDs
- [ ] Signature verification uses well-tested library (ed25519-dalek)

### Functional Requirements
- [ ] tos-js-sdk supports XSWD v2.0
- [ ] tos-dart-sdk supports XSWD v2.0
- [ ] tos-chatgpt-app works with new protocol
- [ ] Wallet displays application public key fingerprint in UI
- [ ] Clear error messages for signature verification failures

### Code Quality Requirements
- [ ] All code compiles with `cargo build --workspace` (0 warnings)
- [ ] All tests pass with `cargo test --workspace` (0 warnings)
- [ ] Clippy passes with `cargo clippy --workspace -- -D warnings`
- [ ] Code formatted with `cargo fmt --all`
- [ ] All English comments (no Chinese/Japanese text)

### Documentation Requirements
- [ ] XSWD v2.0 protocol specification complete
- [ ] Migration guide for SDK users
- [ ] Security analysis document
- [ ] Updated API reference
- [ ] Updated SECURITY_AUDIT_STATUS.md

---

## Testing Checklist

### Unit Tests
- [ ] `wallet/src/api/xswd/verification.rs`
  - [ ] `test_valid_signature_verification()`
  - [ ] `test_invalid_signature_fails()`
  - [ ] `test_expired_timestamp_fails()`
  - [ ] `test_future_timestamp_fails()`
  - [ ] `test_nonce_replay_fails()`
  - [ ] `test_tampered_field_fails()`

### Integration Tests
- [ ] `wallet/tests/xswd_v2_integration.rs`
  - [ ] `test_full_registration_flow()`
  - [ ] `test_permission_request_flow()`
  - [ ] `test_signature_tampering_rejected()`

### End-to-End Tests
- [ ] tos-chatgpt-app connection test
- [ ] tos-js-sdk example project test
- [ ] tos-dart-sdk example project test

---

## Dependencies

### Rust Dependencies
```toml
[dependencies]
ed25519-dalek = "2.0"  # Ed25519 signature verification
hex = "0.4"            # Hex serialization for public_key/signature
```

### JavaScript Dependencies (tos-js-sdk)
```json
{
  "dependencies": {
    "@noble/ed25519": "^2.0.0"
  }
}
```

### Dart Dependencies (tos-dart-sdk)
```yaml
dependencies:
  ed25519_edwards: ^0.3.0
```

---

## Security Considerations

### Threat Model

**Threats Mitigated**:
1. ✅ **Application Impersonation**: Attacker cannot reuse another app's ID without private key
2. ✅ **Replay Attacks**: Timestamp + nonce prevent replay of old permission requests
3. ✅ **Man-in-the-Middle**: Signature verification ensures data integrity
4. ✅ **Permission Escalation**: Permissions bound to public key, not mutable app_id

**Remaining Risks**:
1. ⚠️ **Compromised Application**: If app's private key is stolen, attacker can impersonate that specific app
   - **Mitigation**: User still sees permission requests, can revoke
2. ⚠️ **Social Engineering**: User grants permissions to malicious app with convincing name
   - **Mitigation**: Display public key fingerprint, add trusted app registry (future)

### Cryptographic Design Decisions

**Why Ed25519?**
- ✅ Fast: Signature generation < 1ms, verification < 0.5ms
- ✅ Small: 32-byte public keys, 64-byte signatures
- ✅ Secure: Industry standard (Signal, SSH, TLS 1.3)
- ✅ Well-tested: ed25519-dalek has 10M+ downloads, audited

**Why Not ECDSA (secp256k1)?**
- ❌ Slower: 3-5x slower than Ed25519
- ❌ Larger signatures: 65-73 bytes vs 64 bytes
- ❌ More complex: Requires nonce management (k value)

**Why Timestamp + Nonce?**
- Timestamp prevents old signatures (5-minute window)
- Nonce prevents replay within valid time window
- Together provide strong replay protection

---

## Rollout Plan

### Phase 1: Internal Testing (Week 3, Days 1-3)
- Deploy to development environment
- Test with tos-chatgpt-app
- Verify all flows work

### Phase 2: Staging Deployment (Week 3, Day 4)
- Deploy wallet to staging server
- Test with all SDK examples
- Performance benchmarks

### Phase 3: Production Deployment (Week 3, Day 5)
- Update all SDKs to npm/pub.dev
- Deploy wallet with XSWD v2.0
- Monitor error rates
- Update documentation on website

**Note**: Since there are no production users yet, rollout risk is minimal.

---

## Success Metrics

### Security Metrics
- [ ] **Zero signature bypasses** in security review
- [ ] **100% signature verification rate** in production logs
- [ ] **Zero replay attacks** detected in monitoring

### Performance Metrics
- [ ] **Registration latency** < 100ms (95th percentile)
- [ ] **Signature verification** < 5ms (average)
- [ ] **SDK signature generation** < 10ms (average)

### Code Quality Metrics
- [ ] **Zero build warnings** in CI/CD
- [ ] **100% test coverage** for verification logic
- [ ] **Zero clippy warnings** in wallet/xswd code

---

## Related Issues

- **Security Audit**: #??? (link to security audit issue if exists)
- **H1.1 Fix**: Completed (XSWD localhost binding)
- **H1.2 Fix**: This issue

---

## References

- **Protocol Design**: `docs/XSWD_V2_PROTOCOL_DESIGN.md`
- **Impact Analysis**: `docs/XSWD_V2_UPGRADE_IMPACT_ANALYSIS.md`
- **Security Audit**: `docs/security_audit_report_20251122_en.md`
- **Audit Status**: `docs/SECURITY_AUDIT_STATUS.md`

---

## Notes for Implementers

### Development Environment Setup
```bash
# Ensure custom toolchain is available (if needed)
cd ~/tos-network/tos

# Install dependencies
cargo update

# Build wallet with XSWD feature
cargo build --package tos_wallet --features api_server

# Run tests
cargo test --package tos_wallet --lib api::xswd -- --nocapture
```

### Debugging Tips
```bash
# Enable trace logging for XSWD
RUST_LOG=tos_wallet::api::xswd=trace ./tos_wallet --enable-xswd

# Test signature verification manually
cargo test --package tos_wallet test_signature_verification -- --nocapture
```

### Common Pitfalls
1. **Deterministic Serialization**: Ensure `serialize_for_signing()` produces identical bytes on all platforms
2. **Timestamp Validation**: Use server time, not client-provided timestamp for "current time"
3. **Nonce Storage**: Store used nonces per session, clear on wallet restart (in-memory)
4. **Error Messages**: Don't leak signature details in error messages (side-channel attack)

---

## Post-Implementation Checklist

After completing all 3 weeks:

- [ ] Update `SECURITY_AUDIT_STATUS.md`: H1.2 status → ✅ FIXED
- [ ] Update `CHANGELOG.md`: Add XSWD v2.0 release notes
- [ ] Tag release: `v0.x.0-xswd-v2`
- [ ] Announce in developer Discord/Telegram
- [ ] Close this issue with summary

---

**Created**: 2025-11-22
**Estimated Completion**: 3 weeks from start date
**Priority**: P0 (Security Audit Response)
**Complexity**: Medium-High

🤖 Generated with [Claude Code](https://claude.com/claude-code)
