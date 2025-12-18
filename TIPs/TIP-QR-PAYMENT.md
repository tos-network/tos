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
| `to` | Address | Yes | Merchant receiving address |
| `amount` | u64 | No | Payment amount in atomic units (nanoTOS) |
| `asset` | Hash | No | Asset hash (default: TOS native) |
| `memo` | String | No | Payment memo (max 64 chars, URL-encoded) |
| `id` | String | No | Payment request ID (max 32 chars) |
| `exp` | u64 | No | Expiration timestamp (Unix seconds) |
| `callback` | URL | No | Webhook URL for payment notification |

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

The wallet extracts payment data using `split_address` RPC.

## Extra Data Format for Payments

When including payment metadata in transactions, use this format:

```
Byte 0:     Type (0x01 = payment)
Bytes 1-32: Payment ID (32 bytes, zero-padded)
Bytes 33+:  Memo (UTF-8, remaining bytes)
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
        "qr_data": "tos://pay?..."
    }
}
```

#### `get_payment_status`

Check the status of a payment request.

**Request:**
```json
{
    "jsonrpc": "2.0",
    "method": "get_payment_status",
    "params": {
        "payment_id": "pr_abc123def456"
    }
}
```

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
- `pending` - Waiting for payment
- `mempool` - Transaction in mempool (0 confirmations)
- `confirming` - In block but < 8 confirmations
- `confirmed` - >= 8 confirmations (stable)
- `expired` - Payment request expired
- `underpaid` - Amount received < requested

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
- Amount matches or exceeds request
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
- No reorg possible

## Implementation Notes

### Merchant Integration

1. **POS Integration**
   - Call `create_payment_request` with order details
   - Display QR code on customer-facing screen
   - Poll `get_payment_status` every 1 second
   - Show success when status = "confirmed" (or "mempool" for 0-conf)

2. **E-commerce Integration**
   - Generate payment request at checkout
   - Redirect to payment page with QR code
   - Use WebSocket `watch_address_payments` for real-time updates
   - Redirect to success page when payment confirmed

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

## Security Considerations

1. **Replay Protection**
   - Payment IDs MUST be unique per merchant
   - Wallets SHOULD warn if same payment_id used twice

2. **Amount Verification**
   - Fixed-amount requests MUST match exactly
   - Underpayment results in "underpaid" status

3. **Expiration**
   - Expired payment requests MUST be rejected by wallets
   - Merchants SHOULD use short expiration (5-10 minutes)

4. **HTTPS for Callbacks**
   - Callback URLs MUST use HTTPS
   - Merchants MUST verify callback authenticity

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
