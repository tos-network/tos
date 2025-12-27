# TIP: QR Code Payment System

## Overview

This TIP defines a QR code-based instant payment system for TOS blockchain, similar to PayPay/Alipay/WeChat Pay.

## Design Goals

1. **Fast confirmation** - Leverage 3-second block time for near-instant payments
2. **Simple QR codes** - Compact payment request format suitable for QR encoding
3. **Merchant-friendly** - Easy integration with point-of-sale systems
4. **Secure** - Prevent replay attacks and amount manipulation

## Payment Flow

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   Merchant  │     │   Customer  │     │   Daemon    │
└──────┬──────┘     └──────┬──────┘     └──────┬──────┘
       │                   │                   │
       │ 1. Create Payment │                   │
       │    Request        │                   │
       │◄──────────────────│                   │
       │                   │                   │
       │ 2. Display QR     │                   │
       │──────────────────►│                   │
       │                   │                   │
       │                   │ 3. Scan & Pay     │
       │                   │──────────────────►│
       │                   │                   │
       │ 4. Poll Status    │                   │
       │──────────────────────────────────────►│
       │                   │                   │
       │ 5. Confirm        │                   │
       │◄──────────────────────────────────────│
       │                   │                   │
```

## Payment Request Format

### URI Scheme

```
tos://pay?<parameters>
```

### Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `to` | Address | Yes | Merchant receiving address (bech32 format) |
| `amount` | u64 | No | Payment amount in atomic units (nanoTOS) |
| `asset` | Hash | No | Asset hash in hex (64 chars, lowercase). If omitted, treat as TOS native |
| `memo` | String | No | Payment memo (max 64 bytes UTF-8, URL-encoded) |
| `id` | String | No | Payment request ID (max 32 bytes ASCII, URL-safe) |
| `exp` | u64 | No | Expiration timestamp (Unix seconds) |
| `callback` | URL | No | Webhook URL for payment notification (HTTPS required, see Security) |

**Encoding Notes:**
- `id`: MUST be ASCII-only (a-z, A-Z, 0-9, `-`, `_`). Non-ASCII characters are rejected. IDs longer than 32 bytes MUST be rejected (no truncation).
- `memo`: UTF-8 encoded, URL-encoded in URI. Truncated at byte boundary if > 64 bytes.
- `asset`: 64-character lowercase hex string representing 32-byte hash. Omit for native TOS. In RPC, `null` is equivalent to omitted.

### Example URIs

**Fixed amount payment:**
```
tos://pay?to=tst12zacnuun3lkv5kxzn2jy8l28d0zft7rqhyxlz2v6h6u23xmruy7sqm0d38u&amount=1000000000&memo=Coffee&id=order-12345
```

**Open amount (tip jar):**
```
tos://pay?to=tst12zacnuun3lkv5kxzn2jy8l28d0zft7rqhyxlz2v6h6u23xmruy7sqm0d38u&memo=Tips
```

**With expiration:**
```
tos://pay?to=tst12zacnuun3lkv5kxzn2jy8l28d0zft7rqhyxlz2v6h6u23xmruy7sqm0d38u&amount=5000000000&id=inv-001&exp=1734567890
```

## QR Code Encoding

1. Generate payment URI as above
2. Encode URI as QR code (recommended: Version 10, ECC Level M)
3. Display QR code to customer

### Size Optimization

For smaller QR codes, use integrated addresses with embedded payment data:

```
tst12zacnuun3lkv5kxzn2jy8l28d0zft7rqhyxlz2v6h6u23xmruy7sqm0d38u[encoded_payment_data]
```

**Integrated Address Format (Future Extension):**
- Base address: Standard bech32 address
- Payment data: Base58-encoded, appended after address
- Format: `{base_address}{base58_payment_data}`
- Maximum total length: 128 characters

The wallet extracts payment data using `split_address` RPC.

**Note:** Integrated address support is a future extension. Current implementations SHOULD use the full URI format.

## Extra Data Format for Payments

When including payment metadata in transactions, use this format:

```
Byte 0:     Type (0x01 = payment)
Bytes 1-32: Payment ID (32 bytes, ASCII, zero-padded on right)
Bytes 33+:  Memo (UTF-8, remaining bytes up to 95 bytes)
```

**Encoding Rules:**
- Payment ID: ASCII bytes, right-padded with 0x00 to 32 bytes. IDs longer than 32 bytes MUST be rejected.
- Memo: UTF-8 bytes, truncated to fit remaining space (128 - 33 = 95 bytes max).
- Total extra_data MUST NOT exceed 128 bytes.

**Decoding Algorithm:**
```
1. Check byte[0] == 0x01 (payment type)
2. Extract bytes[1..33], trim trailing zeros → payment_id (ASCII string)
3. Extract bytes[33..] → memo (UTF-8 string, may be empty)
```

This fits within the 128-byte extra_data limit per transfer.

## API Endpoints

### Daemon RPC

#### `create_payment_request`

Create a new payment request and return the QR data.

**Request:**
```json
{
    "jsonrpc": "2.0",
    "method": "create_payment_request",
    "params": {
        "address": "tst12zac...",
        "amount": 1000000000,
        "asset": null,
        "memo": "Coffee",
        "expires_in_seconds": 300
    }
}
```

**Response:**
```json
{
    "jsonrpc": "2.0",
    "result": {
        "payment_id": "pr_abc123def456",
        "uri": "tos://pay?to=tst12zac...&amount=1000000000&memo=Coffee&id=pr_abc123def456&exp=1734567890",
        "expires_at": 1734567890,
        "qr_data": "tos://pay?...",
        "min_topoheight": 123456
    }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `payment_id` | String | Unique payment identifier |
| `uri` | String | Full payment URI for QR code |
| `expires_at` | u64 | Expiration timestamp (Unix seconds) |
| `qr_data` | String | Same as URI, for QR code generation |
| `min_topoheight` | u64 | Current topoheight, use for `get_payment_status` queries |

**Error Format (structured):**
```json
{
    "jsonrpc": "2.0",
    "error": {
        "code": -32602,
        "message": "Invalid params: invalid_payment_id",
        "data": {
            "code": "invalid_payment_id",
            "reason": "length exceeds 32 bytes"
        }
    },
    "id": 1
}
```

#### `get_payment_status`

Check the status of a payment by scanning the blockchain for matching transactions.

**Important:** This method scans the blockchain (mempool + recent blocks), allowing **any node** to verify payment status without requiring local storage synchronization.

**Request:**
```json
{
    "jsonrpc": "2.0",
    "method": "get_payment_status",
    "params": {
        "payment_id": "pr_abc123def456",
        "address": "tst12zac...",
        "expected_amount": 1000000000,
        "exp": 1734567890,
        "min_topoheight": 100000
    }
}
```

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `payment_id` | String | Yes | Payment ID to search for in transaction extra_data |
| `address` | Address | Yes | Receiving address to filter transactions |
| `expected_amount` | u64 | No | Expected payment amount (for underpaid detection) |
| `exp` | u64 | No | Expiration timestamp (Unix seconds) for request; enables `expired` status |
| `min_topoheight` | u64 | No | Start searching from this topoheight (default: current - 1200 blocks ≈ 1 hour) |

**Response:**
```json
{
    "jsonrpc": "2.0",
    "result": {
        "payment_id": "pr_abc123def456",
        "status": "confirmed",
        "tx_hash": "abc123...",
        "amount_received": 1000000000,
        "confirmations": 8,
        "confirmed_at": 1734567920
    }
}
```

**Status values:**
- `pending` - No matching transaction found
- `mempool` - Transaction in mempool (0 confirmations)
- `confirming` - In block but < 8 confirmations
- `confirmed` - >= 8 confirmations (stable, reorg extremely unlikely)
- `expired` - Payment request has expired (only if `exp` provided in request; late payments are treated as expired)
- `underpaid` - Amount received < `expected_amount` (only returned if `expected_amount` provided)

**Note:** If `expected_amount` is not provided, underpaid transactions are reported as `confirmed` and fixed-amount enforcement is not guaranteed. If `exp` is not provided, `expired` is never returned.

#### `watch_address_payments`

Subscribe to payment notifications for an address via WebSocket.

### Wallet RPC

#### `pay_request`

Parse and pay a payment request URI.

**Request:**
```json
{
    "jsonrpc": "2.0",
    "method": "pay_request",
    "params": {
        "uri": "tos://pay?to=tst12zac...&amount=1000000000&memo=Coffee&id=pr_abc123def456"
    }
}
```

**Response:**
```json
{
    "jsonrpc": "2.0",
    "result": {
        "tx_hash": "def456...",
        "amount": 1000000000,
        "fee": 1000,
        "payment_id": "pr_abc123def456"
    }
}
```

**Error Format (structured):**
```json
{
    "jsonrpc": "2.0",
    "error": {
        "code": -32602,
        "message": "Invalid params: expired",
        "data": {
            "code": "expired",
            "reason": "Payment request has expired"
        }
    },
    "id": 1
}
```

#### `parse_payment_request`

Parse a payment URI without executing payment.

**Request:**
```json
{
    "jsonrpc": "2.0",
    "method": "parse_payment_request",
    "params": {
        "uri": "tos://pay?to=tst12zac...&amount=1000000000"
    }
}
```

**Response:**
```json
{
    "jsonrpc": "2.0",
    "result": {
        "address": "tst12zac...",
        "amount": 1000000000,
        "asset": null,
        "memo": "Coffee",
        "payment_id": "pr_abc123def456",
        "expires_at": 1734567890,
        "is_expired": false
    }
}
```

## Confirmation Strategy

### Fast Confirmation (0-conf)

For small payments (< 100 TOS), merchants MAY accept 0-conf (mempool) transactions:

- Transaction is in mempool
- Amount matches or exceeds request (requires `expected_amount`)
- No conflicting transactions detected
- Risk: Double-spend possible but economically unlikely for small amounts

### Standard Confirmation (1-conf)

For medium payments (100-1000 TOS):

- Wait for 1 block confirmation (~3 seconds)
- Transaction is in a block
- Very low double-spend risk

### Full Confirmation (8-conf)

For large payments (> 1000 TOS):

- Wait for 8 block confirmations (~24 seconds)
- Transaction is in stable height
- Reorg probability: < 10^-9 (practically impossible)

## Architecture

### Payment Status Verification

The payment status verification uses **blockchain scanning** rather than local storage synchronization. This design choice enables:

1. **Cross-node queries** - Any node can verify payment status
2. **No additional P2P messages** - Leverages existing transaction propagation
3. **Trustless verification** - Based on actual blockchain state

```
┌─────────────────────────────────────────────────────────────┐
│                    TOS Blockchain Network                    │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌─────────┐    P2P Sync    ┌─────────┐    P2P Sync         │
│  │ Node A  │◄──────────────►│ Node B  │◄──────────────►...  │
│  │         │                │         │                      │
│  │ Mempool │                │ Mempool │                      │
│  │ Blocks  │                │ Blocks  │                      │
│  └────┬────┘                └────┬────┘                      │
│       │                          │                           │
└───────┼──────────────────────────┼───────────────────────────┘
        │                          │
        ▼                          ▼
┌───────────────┐          ┌───────────────┐
│   Merchant    │          │   Customer    │
│               │          │               │
│ 1. create_    │          │ 2. pay_       │
│    payment_   │          │    request    │
│    request    │          │               │
│               │          │               │
│ 3. get_       │          │               │
│    payment_   │◄─────────┤               │
│    status     │  (can query any node)    │
└───────────────┘          └───────────────┘
```

### Scanning Algorithm

`get_payment_status` performs the following scan:

```
Constants:
  DEFAULT_SCAN_BLOCKS = 1200  (≈ 1 hour at 3s/block)
  STABLE_CONFIRMATIONS = 8

1. If exp provided AND current_time > exp:
   └── Return status: "expired"

2. Determine scan range:
   └── start_height = min_topoheight OR (current_topoheight - DEFAULT_SCAN_BLOCKS)
   └── end_height = current_topoheight

3. Scan Mempool
   └── For each transaction in mempool:
       └── If destination == address AND payment_id matches extra_data:
           └── If expected_amount provided AND amount < expected_amount:
               └── Return status: "underpaid"
           └── Return status: "mempool"

4. Scan Blocks [start_height..end_height]
   └── For each block in range:
       └── For each transaction:
           └── If destination == address AND payment_id matches extra_data:
               └── confirmations = current_topoheight - block_topoheight + 1
               └── If expected_amount provided AND amount < expected_amount:
                   └── Return status: "underpaid"
               └── If confirmations >= STABLE_CONFIRMATIONS:
                   └── Return status: "confirmed"
               └── Else:
                   └── Return status: "confirming"

5. If multiple matches exist:
   └── Prefer the highest topoheight transaction; block results override mempool

6. No match found
   └── Return status: "pending"
```

### Performance Considerations

| Scenario | Blocks to Scan | Estimated Time |
|----------|---------------|----------------|
| Fresh payment (< 5 min) | ~100 blocks | < 100ms |
| Recent payment (< 1 hour) | ~1,200 blocks | < 500ms |
| Old payment (> 1 hour) | Use `min_topoheight` | Varies |

**Recommendations:**
- Merchants SHOULD store `min_topoheight` when creating payment requests
- Use `min_topoheight` parameter to limit scan range for efficiency
- For real-time updates, use WebSocket `TransactionExecuted` events

## Implementation Notes

### Merchant Integration

1. **POS Integration**
   ```
   Step 1: Create payment request
   ─────────────────────────────
   POST /json_rpc
   { "method": "create_payment_request", "params": { "address": "...", "amount": 1000000000 } }

   Response: { "payment_id": "pr_xxx", "min_topoheight": 123456, ... }

   Step 2: Display QR code
   ─────────────────────────────
   Show QR code with uri/qr_data to customer

   Step 3: Poll for payment (every 1-2 seconds)
   ─────────────────────────────
   POST /json_rpc
   { "method": "get_payment_status", "params": {
       "payment_id": "pr_xxx",
       "address": "...",
       "min_topoheight": 123456  // Use value from step 1
   }}

   Step 4: Show success when status = "confirmed" (or "mempool" for 0-conf)
   ```

2. **E-commerce Integration**
   - Generate payment request at checkout, store `min_topoheight` in order record
   - Redirect to payment page with QR code
   - Use WebSocket `TransactionExecuted` event for real-time updates
   - Or poll `get_payment_status` with stored `min_topoheight`
   - Redirect to success page when payment confirmed

3. **Cross-Node Query**
   - Merchant can query payment status from **any synced node**
   - No need to use the same node that created the payment request
   - Useful for load balancing and high availability setups

### Wallet Implementation

1. **QR Scanner**
   - Scan QR code
   - Parse payment URI with `parse_payment_request`
   - Display payment details to user
   - Confirm and execute with `pay_request`

2. **Payment Verification**
   - Validate amount is within user's balance
   - Check expiration before sending
   - Include payment_id in extra_data for merchant matching
   - If multiple matching transactions are found for the same payment_id, use the highest topoheight transaction for status evaluation

### Expired Payment Handling

- Merchants SHOULD display a clear "expired" state and stop polling once `expired` is returned.
- Late payments (after `exp`) MUST be treated as expired; merchants SHOULD not mark orders as paid.
- If a late payment is detected out-of-band, merchants MAY offer manual reconciliation or refund flow.

## Security Considerations

### 1. Replay Protection
- Payment IDs MUST be unique per merchant
- Wallets SHOULD warn if same payment_id used twice
- Recommended format: `{merchant_prefix}_{timestamp}_{random}` (e.g., `shop_1734567890_a1b2c3`)

### 2. Amount Verification
- Fixed-amount requests MUST match exactly; merchants SHOULD provide `expected_amount` when validating fixed amounts
- Underpayment results in "underpaid" status
- Overpayment is accepted (no refund mechanism in protocol)

### 3. Expiration
- Expired payment requests MUST be rejected by wallets
- Merchants SHOULD use short expiration (5-10 minutes)
- Wallets MUST reject expired payment requests before sending.
- If `exp` is provided, `get_payment_status` MUST return `expired` after the deadline, even if a late payment is detected.
- Daemon MAY reject transactions to expired payment requests only if it has the original `exp` (e.g., from a locally created request or if provided out-of-band).

### 4. Callback Security

**IMPORTANT:** The `callback` parameter enables server-to-server payment notifications but requires careful security implementation.

#### Callback Request Format

When a payment is detected, the daemon/payment service sends:

```http
POST {callback_url}
Content-Type: application/json
X-TOS-Signature: {hmac_signature}
X-TOS-Timestamp: {unix_timestamp}

{
  "event": "payment_received",
  "payment_id": "pr_abc123",
  "tx_hash": "abc123...",
  "amount": 1000000000,
  "confirmations": 1,
  "timestamp": 1734567890
}
```
**Timestamp rule:** `X-TOS-Timestamp` is authoritative for signature verification; body `timestamp` is informational and MUST match the header value.

#### Signature Verification

Merchants MUST verify the callback signature:

```
1. Extract X-TOS-Timestamp header
2. Check timestamp is within 5 minutes of current time (replay protection)
3. Concatenate: timestamp + "." + request_body
4. Compute HMAC-SHA256 with merchant's webhook_secret
5. Encode signature as lowercase hex
6. Compare with X-TOS-Signature (constant-time comparison)
```

**Signature Example (pseudo):**
```text
timestamp = "1734567890"
body = '{"event":"payment_received","payment_id":"pr_abc123","tx_hash":"abc123...","amount":1000000000,"confirmations":1,"timestamp":1734567890}'
payload = timestamp + "." + body
hmac = HMAC_SHA256(webhook_secret, payload)
signature = hex_lowercase(hmac)
```

#### Security Requirements

| Requirement | Description |
|-------------|-------------|
| HTTPS Only | Callback URLs MUST use HTTPS with valid certificate |
| Signature Verification | Merchants MUST verify HMAC signature |
| Timestamp Check | Reject callbacks > 5 minutes old |
| Idempotency | Handle duplicate callbacks gracefully (same payment_id) |
| Timeout | Callback requests timeout after 10 seconds |
| Retry Policy | 3 retries with exponential backoff (1s, 5s, 25s) |

#### Webhook Secret Registration

Merchants register webhook secrets via out-of-band mechanism (not part of this TIP). The secret is used for HMAC signature generation.

**Note:** If callback security is not required, merchants SHOULD poll `get_payment_status` instead of using callbacks.

## Implementation Consistency Notes

- `payment_id` MUST be ASCII [A-Za-z0-9_-], non-empty, max 32 bytes; reject on violation (no truncation).
- Confirmations MUST be computed as `current_topoheight - block_topoheight + 1`.
- Underpaid detection in mempool MUST follow the same `expected_amount` rule as confirmed blocks.

## Comparison with PayPay

| Feature | TOS QR Payment | PayPay |
|---------|---------------|--------|
| Confirmation Time | 3-24 seconds | Instant |
| Decentralized | Yes | No |
| Offline Support | Limited | Yes |
| Transaction Fee | ~0.001 TOS | 0-3% |
| Reversibility | No | Yes (chargebacks) |

## Future Enhancements

1. **NFC Payments** - Tap-to-pay using NFC
2. **Lightning-style Channels** - Sub-second payments for micro-transactions
3. **Merchant Certification** - Verified merchant identities in QR codes
4. **Multi-currency** - Support for TOS tokens and stablecoins
