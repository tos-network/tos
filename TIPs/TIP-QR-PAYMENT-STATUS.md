# TIP-QR-PAYMENT Implementation Status

**Last Updated:** 2025-12-27
**TIP Version:** 1.0
**Implementation Progress:** ~70%

## Overview

This document tracks the implementation status of [TIP-QR-PAYMENT.md](./TIP-QR-PAYMENT.md).

## Implementation Summary

| Category | Status | Progress |
|----------|--------|----------|
| Core Types & Parsing | Complete | 100% |
| Daemon RPC APIs | Partial | 80% |
| Wallet RPC APIs | Complete | 100% |
| Blockchain Scanning | Partial | 50% |
| WebSocket Subscription | Not Started | 0% |
| Callback Security | Not Started | 0% |
| Unit Tests | Complete | 100% |
| Integration Tests | Not Started | 0% |

---

## Detailed Status

### 1. Core Types & Parsing

**Status:** ✅ Complete

| Component | File | Status | Tests |
|-----------|------|--------|-------|
| `PaymentStatus` enum | `common/src/api/payment.rs` | ✅ | - |
| `PaymentRequest` struct | `common/src/api/payment.rs` | ✅ | - |
| `PaymentStatusResponse` struct | `common/src/api/payment.rs` | ✅ | - |
| `PaymentIdError` enum | `common/src/api/payment.rs` | ✅ | - |
| `PaymentParseError` enum | `common/src/api/payment.rs` | ✅ | - |
| URI generation (`to_uri`) | `common/src/api/payment.rs:134` | ✅ | ✅ |
| URI parsing (`from_uri`) | `common/src/api/payment.rs:166` | ✅ | ✅ |
| Payment ID validation | `common/src/api/payment.rs:549` | ✅ | ✅ |
| Extra data encoding | `common/src/api/payment.rs:458` | ✅ | ✅ |
| Extra data decoding | `common/src/api/payment.rs:492` | ✅ | ✅ |
| UTF-8 byte truncation | `common/src/api/payment.rs:419` | ✅ | ✅ |

### 2. Daemon RPC APIs

**Status:** ⚠️ Partial (80%)

| Method | File | Status | Notes |
|--------|------|--------|-------|
| `create_payment_request` | `daemon/src/rpc/rpc.rs:3104` | ✅ Complete | Generates payment request with URI |
| `get_payment_status` | `daemon/src/rpc/rpc.rs:3229` | ⚠️ Partial | See limitations below |
| `parse_payment_request` | `daemon/src/rpc/rpc.rs` | ✅ Complete | Parses URI without payment |
| `get_address_payments` | `daemon/src/rpc/rpc.rs:3415` | ✅ Complete | Balance check helper |

#### `get_payment_status` Limitations

| Feature | TIP Spec | Implementation | Status |
|---------|----------|----------------|--------|
| Check expiration (`exp`) | Required | ✅ Implemented | ✅ |
| Payment ID validation | Required | ✅ Implemented | ✅ |
| Mempool scanning | Required | ✅ Implemented | ✅ |
| Local storage lookup | Required | ✅ Implemented | ✅ |
| Block history scanning | Required | ❌ Not implemented | ❌ |
| `min_topoheight` parameter | Required | ❌ Ignored | ❌ |
| `expected_amount` underpaid check | Required | ✅ Implemented | ✅ |

**Missing:** Full blockchain scanning from `min_topoheight` to current height. Currently only checks mempool and locally stored payment requests.

### 3. Wallet RPC APIs

**Status:** ✅ Complete

| Method | File | Status | Notes |
|--------|------|--------|-------|
| `parse_payment_request` | `wallet/src/api/rpc.rs:957` | ✅ Complete | Parses URI, returns details |
| `pay_request` | `wallet/src/api/rpc.rs:979` | ✅ Complete | Executes payment with extra_data |

### 4. WebSocket Subscription

**Status:** ❌ Not Started

| Feature | TIP Section | Status |
|---------|-------------|--------|
| `watch_address_payments` | API Endpoints | ❌ Not implemented |
| Real-time payment notifications | Architecture | ❌ Not implemented |

**Note:** The existing `TransactionExecuted` WebSocket event can be used as a workaround.

### 5. Callback Security (HMAC-SHA256)

**Status:** ❌ Not Started

| Feature | TIP Section | Status |
|---------|-------------|--------|
| Callback URL parameter | Parameters | ✅ Parsed |
| HMAC-SHA256 signature generation | Callback Security | ❌ Not implemented |
| `X-TOS-Signature` header | Callback Security | ❌ Not implemented |
| `X-TOS-Timestamp` header | Callback Security | ❌ Not implemented |
| Webhook secret registration | Callback Security | ❌ Not implemented |
| Retry with exponential backoff | Callback Security | ❌ Not implemented |

**Priority:** Low (merchants can poll `get_payment_status` instead)

---

## Test Coverage

### Unit Tests (common/src/api/payment.rs)

**Status:** ✅ Complete (11 tests)

| Test | Description | Status |
|------|-------------|--------|
| `test_payment_request_to_uri` | URI generation | ✅ |
| `test_payment_request_from_uri` | URI parsing | ✅ |
| `test_payment_request_roundtrip` | Encode/decode cycle | ✅ |
| `test_payment_expiration` | Expiration check | ✅ |
| `test_invalid_uri` | Invalid URI rejection | ✅ |
| `test_encode_decode_payment_extra_data` | Extra data with memo | ✅ |
| `test_encode_decode_without_memo` | Extra data without memo | ✅ |
| `test_matches_payment_id` | Payment ID matching | ✅ |
| `test_decode_invalid_extra_data` | Invalid data rejection | ✅ |
| `test_long_payment_id_rejected` | Length validation | ✅ |
| `test_extra_data_size_limit` | Size limit enforcement | ✅ |

### Integration Tests

**Status:** ❌ Not Started

| Test | Description | Priority |
|------|-------------|----------|
| Daemon RPC: `create_payment_request` | Create and verify payment request | High |
| Daemon RPC: `get_payment_status` | Status polling with various states | High |
| Wallet RPC: `pay_request` | Execute payment from URI | High |
| End-to-end: Full payment flow | Merchant → Customer → Confirmation | High |
| Blockchain scanning | Verify historical transaction lookup | Medium |
| Mempool detection | Verify 0-conf transaction detection | Medium |
| Underpaid detection | Verify partial payment handling | Medium |
| Expiration handling | Verify expired request rejection | Low |

---

## File Locations

| Component | Path |
|-----------|------|
| TIP Specification | `TIPs/TIP-QR-PAYMENT.md` |
| Core Types | `common/src/api/payment.rs` |
| Daemon RPC | `daemon/src/rpc/rpc.rs` |
| Wallet RPC | `wallet/src/api/rpc.rs` |
| RPC Error Types | `common/src/rpc/error.rs` |

---

## TODO List

### High Priority

- [ ] Implement blockchain history scanning in `get_payment_status`
  - Scan blocks from `min_topoheight` to current height
  - Match transactions by address + payment_id in extra_data
  - Return highest topoheight match

- [ ] Add integration tests for payment flow
  - `testing-framework/tests/payment_integration_test.rs`

### Medium Priority

- [ ] Implement `watch_address_payments` WebSocket subscription
  - Subscribe to address for real-time payment notifications
  - Filter by payment_id if provided

### Low Priority

- [ ] Implement callback security (HMAC-SHA256)
  - Generate signature for callback requests
  - Add retry logic with exponential backoff
  - Webhook secret management

---

## Changelog

| Date | Version | Changes |
|------|---------|---------|
| 2025-12-27 | 1.0 | Initial implementation status document |

---

## References

- [TIP-QR-PAYMENT.md](./TIP-QR-PAYMENT.md) - Full specification
- [CLAUDE.md](../CLAUDE.md) - Development guidelines
