# TOS Blockchain QR Code Payment System Design

## Overview

This document describes the design for implementing a PayPay/Alipay-style QR code payment system on TOS blockchain, including offline payment capabilities with insurance-backed credit lines.

## System Architecture

### Layer 1: On-Chain Settlement Layer (TOS Blockchain)

**Performance Characteristics:**
- Block time: 1 second (1 BPS)
- Finality time: 60 seconds (STABLE_LIMIT = 60 blocks)
- Transaction cost: ~1000 nanoTOS per transfer (~$0.0001 USD)
- Energy-based: ~60 nanoTOS per transfer (after freezing collateral)

**Transaction Types Used:**
- `Transfer`: Direct peer-to-peer payments
- `InvokeContract`: Interact with insurance credit contracts
- `DeployContract`: Deploy merchant payment gateway contracts

### Layer 2: Payment Gateway Layer (Optional Middleware)

**Responsibilities:**
- Real-time transaction broadcasting
- Merchant settlement aggregation
- QR code generation and validation
- Offline transaction queuing
- Credit limit enforcement

**Infrastructure:**
- REST API for mobile apps
- WebSocket for real-time confirmations
- Database for transaction history
- Redis for rate limiting

### Layer 3: Client Layer (Mobile Wallets)

**Features:**
- QR code generation (merchant mode)
- QR code scanning (customer mode)
- Offline transaction signing
- Local credit balance tracking
- Transaction history and receipts

## QR Code Payment Flow

### 1. Standard Online Payment (10-60 second confirmation)

```
┌──────────┐                    ┌──────────┐                    ┌──────────┐
│ Customer │                    │ Merchant │                    │   TOS    │
│  Wallet  │                    │  Wallet  │                    │Blockchain│
└─────┬────┘                    └─────┬────┘                    └─────┬────┘
      │                               │                               │
      │  1. Scan QR code              │                               │
      │──────────────────────────────>│                               │
      │                               │                               │
      │  2. Display payment details   │                               │
      │  (amount, merchant, memo)     │                               │
      │<──────────────────────────────│                               │
      │                               │                               │
      │  3. Sign & submit transaction │                               │
      │───────────────────────────────────────────────────────────────>│
      │                               │                               │
      │                               │  4. Transaction in mempool    │
      │                               │  (~1 second)                  │
      │                               │                               │
      │  5. Payment submitted         │                               │
      │<───────────────────────────────────────────────────────────────│
      │                               │                               │
      │                               │  6. Transaction in block      │
      │                               │  (~10 seconds)                │
      │                               │                               │
      │  7. Payment confirmed (10s)   │                               │
      │──────────────────────────────>│                               │
      │                               │  8. Merchant delivers goods   │
      │<──────────────────────────────│  (low-value items)            │
      │                               │                               │
      │                               │  9. Transaction final         │
      │                               │  (~60 seconds)                │
      │                               │                               │
      │  10. Final receipt            │                               │
      │──────────────────────────────>│                               │
      │                               │  11. End of day settlement    │
      │                               │  (optional, for aggregation)  │
      └───────────────────────────────┴───────────────────────────────┘
```

**Timeline:**
- t=0s: Customer scans QR code
- t=1s: Transaction signed and submitted
- t=10s: Transaction in block → Merchant delivers goods (low-value)
- t=60s: Transaction finalized → Irreversible settlement

**Risk Profile:**
- **10-second confirmation**: Suitable for low-value transactions (<$10 USD)
- **60-second confirmation**: Suitable for high-value transactions (>$10 USD)
- **Double-spend risk**: Near zero (GHOSTDAG + mempool monitoring)

### 2. QR Code Format

**Standard Format (BIP21-style):**
```
tos://pay?address=<bech32_address>&amount=<nanotos>&memo=<base64>&expires=<unix_timestamp>
```

**Example:**
```
tos://pay?address=tst12zacnuun3lkv5kxzn2jy8l28d0zft7rqhyxlz2v6h6u23xmruy7sqm0d38u&amount=100000000&memo=T1JERVIxMjM0NQ==&expires=1730000000
```

**Fields:**
- `address`: Merchant's TOS bech32 address
- `amount`: Payment amount in nanoTOS (1 TOS = 1,000,000,000 nanoTOS)
- `memo`: Base64-encoded payment metadata (order ID, invoice reference, etc.)
- `expires`: Unix timestamp for QR code expiration (anti-replay)

**QR Code Content Size:**
- Address: 62 chars (bech32)
- Amount: ~18 chars (u64 max)
- Memo: ~170 chars (128 bytes base64)
- Expires: ~10 chars
- **Total: ~260 chars → QR Version 4 (33x33 modules)**

## Offline Payment System

### Challenge

TOS blockchain requires on-chain settlement for transaction finality. True offline payments need:
1. Deferred settlement (when connectivity is restored)
2. Credit risk mitigation (insufficient funds)
3. Double-spend prevention (replay attacks)

### Solution: Insurance-Backed Credit System

#### Phase 1: Credit Line Establishment (On-Chain)

**User Actions:**
1. User deposits collateral into insurance smart contract
   - Option A: Deposit 100 TOS as collateral
   - Option B: Pay premium (e.g., 5 TOS for 3 months coverage)
2. Insurance contract validates deposit and issues credit certificate
3. User receives signed credit certificate with:
   - Credit limit (e.g., 500 USDT equivalent)
   - Expiration date (e.g., 90 days)
   - Certificate ID (unique nonce)
   - Insurance contract signature

**Contract State:**
```rust
struct CreditLine {
    user_address: Address,
    credit_limit: u64,           // In nanoTOS or USDT equivalent
    used_credit: u64,            // Current outstanding balance
    collateral_amount: u64,      // Deposited collateral
    expires_at: u64,             // Unix timestamp
    certificate_id: [u8; 32],    // Unique certificate ID
    status: CreditStatus,        // Active, Suspended, Closed
}

enum CreditStatus {
    Active,       // Can make offline payments
    Suspended,    // Exceeded limit or missed settlement
    Closed,       // Collateral withdrawn or expired
}
```

#### Phase 2: Offline Transaction Creation (Off-Chain)

**Transaction Flow:**

```
┌──────────┐                    ┌──────────┐
│ Customer │                    │ Merchant │
│  Wallet  │                    │  Wallet  │
└─────┬────┘                    └─────┬────┘
      │                               │
      │  1. Scan merchant QR code     │
      │  (no network connection)      │
      │──────────────────────────────>│
      │                               │
      │  2. Create offline payment    │
      │  proof (signed message)       │
      │  {                             │
      │    from: customer_address,    │
      │    to: merchant_address,      │
      │    amount: 50000000000,       │
      │    memo: "ORDER12345",        │
      │    certificate_id: [u8; 32],  │
      │    nonce: unique_nonce,       │
      │    timestamp: unix_time,      │
      │    signature: customer_sig    │
      │  }                            │
      │                               │
      │  3. Send payment proof        │
      │  (via Bluetooth/NFC/QR)       │
      │──────────────────────────────>│
      │                               │
      │                               │  4. Merchant validates:
      │                               │  • Signature validity
      │                               │  • Certificate not expired
      │                               │  • Amount within limits
      │                               │  • Nonce not reused
      │                               │
      │  5. Merchant accepts payment  │
      │  and delivers goods           │
      │<──────────────────────────────│
      │                               │
      │  6. Both wallets queue        │
      │  transaction for settlement   │
      │                               │
      └───────────────────────────────┘
```

**Offline Payment Proof (Signed Message):**
```json
{
  "version": 1,
  "payment_type": "offline_credit",
  "from_address": "tst1xxx...",
  "to_address": "tst1yyy...",
  "amount": 50000000000,
  "memo": "ORDER12345",
  "certificate_id": "0x1234...",
  "nonce": 42,
  "timestamp": 1730000000,
  "expires_at": 1730003600,
  "customer_signature": "0xaabbcc...",
  "merchant_signature": "0xddeeff..."
}
```

**Security Features:**
- **Replay protection**: Unique nonce per transaction + timestamp
- **Expiration**: Payment proof expires in 1 hour (must settle)
- **Mutual signatures**: Both customer and merchant sign (proof of delivery)
- **Local limit tracking**: Wallet tracks available credit balance

#### Phase 3: Settlement (On-Chain, when online)

**Settlement Flow:**

```
┌──────────┐                    ┌──────────┐                    ┌──────────┐
│ Customer │                    │ Merchant │                    │Insurance │
│  Wallet  │                    │  Wallet  │                    │Contract  │
└─────┬────┘                    └─────┬────┘                    └─────┬────┘
      │                               │                               │
      │  1. Wallet connects online    │                               │
      │  (WiFi/4G restored)           │                               │
      │                               │                               │
      │  2. Batch submit queued       │                               │
      │  offline transactions         │                               │
      │───────────────────────────────────────────────────────────────>│
      │                               │                               │
      │                               │  3. Contract validates:       │
      │                               │  • Certificate valid          │
      │                               │  • Credit limit OK            │
      │                               │  • Nonce not reused           │
      │                               │  • Signatures valid           │
      │                               │                               │
      │                               │  4. Scenario A:               │
      │                               │  User has sufficient balance  │
      │  Transfer funds to merchant   │                               │
      │───────────────────────────────────────────────────────────────>│
      │                               │                               │
      │                               │  5. Contract releases payment │
      │                               │  to merchant                  │
      │                               │<──────────────────────────────│
      │                               │                               │
      │  6. Settlement complete       │                               │
      │<──────────────────────────────────────────────────────────────│
      │                               │                               │
      │                               │  7. Scenario B:               │
      │                               │  Insufficient funds           │
      │                               │                               │
      │                               │  8. Insurance pays merchant   │
      │                               │<──────────────────────────────│
      │                               │                               │
      │  9. User owes insurance       │                               │
      │  contract (debt collection)   │                               │
      │<──────────────────────────────────────────────────────────────│
      │                               │                               │
      │  10. Credit line suspended    │                               │
      │  until debt repaid            │                               │
      │                               │                               │
      └───────────────────────────────┴───────────────────────────────┘
```

**Smart Contract Logic:**

```rust
// Pseudo-code for insurance credit contract

pub async fn settle_offline_payment(
    payment_proof: OfflinePaymentProof,
) -> Result<(), ContractError> {
    // 1. Validate payment proof
    validate_signatures(&payment_proof)?;
    validate_certificate(&payment_proof.certificate_id)?;
    validate_nonce(&payment_proof.nonce)?;
    validate_expiration(&payment_proof.expires_at)?;

    // 2. Get credit line
    let mut credit_line = get_credit_line(&payment_proof.from_address)?;

    // 3. Check credit limit
    if credit_line.used_credit + payment_proof.amount > credit_line.credit_limit {
        return Err(ContractError::CreditLimitExceeded);
    }

    // 4. Try to settle from user's balance
    let user_balance = get_balance(&payment_proof.from_address)?;

    if user_balance >= payment_proof.amount {
        // Scenario A: User has sufficient funds
        transfer(
            &payment_proof.from_address,
            &payment_proof.to_address,
            payment_proof.amount,
        )?;

        log_event(OfflinePaymentSettled {
            payment_id: payment_proof.nonce,
            settled_by: "user",
            amount: payment_proof.amount,
        });

    } else {
        // Scenario B: Insufficient funds → Insurance pays

        // Insurance contract pays merchant
        transfer(
            &INSURANCE_CONTRACT_ADDRESS,
            &payment_proof.to_address,
            payment_proof.amount,
        )?;

        // Record debt
        credit_line.used_credit += payment_proof.amount;
        credit_line.status = CreditStatus::Suspended;
        save_credit_line(&credit_line)?;

        log_event(OfflinePaymentSettled {
            payment_id: payment_proof.nonce,
            settled_by: "insurance",
            amount: payment_proof.amount,
        });

        log_event(DebtCreated {
            debtor: payment_proof.from_address,
            amount: payment_proof.amount,
            due_date: current_timestamp() + 30 * 86400, // 30 days
        });
    }

    Ok(())
}
```

### Credit Line Parameters

**Tier 1: Low Credit (No Collateral)**
- Credit limit: 100 USDT equivalent
- Premium: 2 TOS per month
- Insurance coverage: 100%
- Settlement window: 24 hours

**Tier 2: Medium Credit (50% Collateral)**
- Credit limit: 500 USDT equivalent
- Collateral: 250 USDT in TOS
- Premium: 1 TOS per month
- Insurance coverage: 100%
- Settlement window: 7 days

**Tier 3: High Credit (100% Collateral)**
- Credit limit: 2000 USDT equivalent
- Collateral: 2000 USDT in TOS
- Premium: 0 TOS (covered by collateral interest)
- Insurance coverage: 100%
- Settlement window: 30 days

### Risk Mitigation

**For Merchants:**
- Insurance contract guarantees payment
- Settlement within 24-72 hours
- Maximum 1% fee (insurance premium + gas)

**For Users:**
- No overdraft fees if settled within window
- Transparent credit limit tracking
- Ability to top up collateral instantly

**For Insurance Provider:**
- Collateral covers 50-100% of exposure
- Premium income covers default risk
- On-chain enforcement (no legal costs)

## Implementation Phases

### Phase 1: Basic QR Code Payments (No Offline)

**Deliverables:**
- [ ] QR code generation library (Rust + WASM)
- [ ] Mobile wallet SDK (React Native / Flutter)
- [ ] Payment gateway REST API
- [ ] Merchant dashboard (web app)

**Timeline:** 4-6 weeks

**Features:**
- Generate payment QR codes (amount, memo, expiration)
- Scan QR codes and submit transactions
- Real-time payment confirmation (10-60 seconds)
- Transaction history and receipts

### Phase 2: Insurance Credit Contract

**Deliverables:**
- [ ] Smart contract for credit line management
- [ ] Collateral deposit and withdrawal functions
- [ ] Offline payment proof validation
- [ ] Settlement and debt collection logic

**Timeline:** 6-8 weeks

**Features:**
- Deposit collateral to get credit line
- Issue credit certificates
- Validate offline payment proofs
- Automatic settlement when online
- Debt tracking and collection

### Phase 3: Offline Payment Support

**Deliverables:**
- [ ] Offline transaction signing
- [ ] Bluetooth/NFC payment proof transfer
- [ ] Local credit balance tracking
- [ ] Queued transaction management

**Timeline:** 4-6 weeks

**Features:**
- Create and sign transactions offline
- Transfer payment proofs via Bluetooth/NFC
- Queue transactions for later settlement
- Sync when connectivity restored

### Phase 4: Merchant Tools & Analytics

**Deliverables:**
- [ ] POS terminal integration
- [ ] Payment reconciliation tools
- [ ] Analytics dashboard
- [ ] Batch settlement optimization

**Timeline:** 4-6 weeks

**Features:**
- Generate QR codes at POS terminals
- Real-time sales analytics
- End-of-day settlement reports
- Multi-store management

## Technical Considerations

### Performance Optimization

**Transaction Batching:**
- Batch multiple offline payments into single on-chain transaction
- Reduces gas fees by 80-90%
- Settlement window: 24 hours for batch processing

**Energy-Based Fees:**
- Merchants freeze 1000 TOS → Get 14x free transfers
- Effective cost: ~60 nanoTOS per transaction
- Return on investment: 30 days

**Caching Strategy:**
- Cache merchant addresses and QR codes
- Prefetch account nonces for offline signing
- Local balance tracking (sync every 60 seconds)

### Security Considerations

**Replay Attack Prevention:**
- Unique nonce per transaction (cryptographically random)
- Timestamp validation (reject >1 hour old proofs)
- Certificate expiration (90-day validity)

**Double-Spend Prevention:**
- Contract tracks used nonces (bloom filter + database)
- Local wallet prevents nonce reuse
- Settlement order: first-seen wins

**Collateral Security:**
- Multi-signature withdrawal (requires 2-of-3 insurance board approval)
- Time-locked withdrawal (7-day delay)
- Emergency pause mechanism (circuit breaker)

### Scalability

**Current Capacity:**
- Block time: 1 second
- Transactions per block: ~1000 (limited by block size)
- Theoretical TPS: 1000 transactions per second

**Scaling Solutions:**
- Batch settlement: 100 offline payments → 1 on-chain transaction
- Effective TPS: 100,000 offline payments per second
- Settlement latency: 24 hours (acceptable for retail)

## Economic Model

### Insurance Pool Economics

**Revenue Sources:**
1. Monthly premiums (1-2 TOS per user)
2. Collateral interest (5% APY on deposited TOS)
3. Late payment fees (1% per week)
4. Early withdrawal penalties (1% of collateral)

**Expense Sources:**
1. Default payments (insurance covers merchant)
2. Smart contract gas fees
3. Insurance pool management
4. Customer support and dispute resolution

**Break-Even Analysis:**
- Average default rate: 2% (industry standard)
- Premium covers 3% default risk → 1% profit margin
- Collateral covers 97% of exposure → Low capital requirement

### User Cost Comparison

**TOS QR Payment (with credit):**
- Transaction fee: ~$0.0001 (1000 nanoTOS)
- Insurance premium: ~$0.20/month (2 TOS)
- Total monthly cost: ~$0.20 + $0.003 (30 transactions)

**PayPay/Alipay:**
- Transaction fee: 0% (subsidized by merchants)
- No monthly fee (for users)
- Total monthly cost: $0

**Credit Card:**
- Transaction fee: 2-3% (passed to merchant)
- Annual fee: $50-100 (some cards)
- Total monthly cost: $4-8 (for users with fees)

**TOS Advantage:**
- Lower cost than credit cards
- Decentralized (no censorship)
- Instant settlement (no T+1/T+2 delay)
- Cross-border payments (no forex fees)

## Comparison with PayPay/Alipay

| Feature | TOS QR Payment | PayPay/Alipay |
|---------|---------------|---------------|
| **Confirmation Time** | 10-60 seconds | Instant (<1 second) |
| **Offline Payments** | Yes (with credit) | Yes (with credit) |
| **Transaction Fee** | ~$0.0001 | 0% (user), 1-3% (merchant) |
| **Settlement Time** | 60 seconds (final) | T+1 (next business day) |
| **Decentralization** | Yes (blockchain) | No (centralized) |
| **Cross-Border** | Yes (no forex fees) | Limited (high fees) |
| **Censorship Resistance** | Yes | No (KYC/AML enforced) |
| **Privacy** | Pseudonymous | Full KYC required |
| **Merchant Integration** | API + SDK | API + POS terminals |
| **User Experience** | Good (10-60s wait) | Excellent (instant) |

**Key Differences:**

1. **Confirmation Time**: TOS requires 10-60 seconds for finality, while PayPay/Alipay appears instant (but actual settlement is T+1).

2. **Decentralization**: TOS is fully decentralized (no single point of failure), while PayPay/Alipay are centralized (can freeze accounts).

3. **Cost Structure**: TOS has minimal transaction fees but requires insurance premium for offline payments. PayPay/Alipay are free for users but charge merchants 1-3%.

4. **User Experience**: PayPay/Alipay have superior UX (instant confirmation), but TOS offers better privacy and censorship resistance.

## Conclusion

**Is TOS suitable for PayPay/Alipay-style QR payments?**

✅ **YES** - TOS blockchain is well-suited for QR code payments with the following caveats:

**Strengths:**
- Fast finality (60 seconds)
- Low transaction costs (~$0.0001)
- Smart contracts for programmable payments
- Decentralized (no censorship)
- Cross-border without forex fees

**Limitations:**
- Confirmation time (10-60s) vs instant (PayPay/Alipay)
- Requires additional infrastructure (payment gateway, insurance contracts)
- User experience not as polished (requires education)

**Recommended Approach:**

1. **Phase 1**: Launch basic QR payments (online only) targeting crypto-native merchants and users
2. **Phase 2**: Deploy insurance credit contracts for offline payments
3. **Phase 3**: Optimize UX with batching, caching, and mobile SDK improvements
4. **Phase 4**: Scale to mainstream retail with POS integration and merchant tools

**Target Markets:**
- Cross-border remittances (lower fees than Western Union)
- Crypto payments at retail (coffee shops, restaurants)
- Decentralized finance (DeFi) integration (atomic swaps with payment)
- Regions with unreliable internet (offline payment critical)

**Estimated Development Time:**
- Phase 1-2: 3-4 months (basic + credit system)
- Phase 3-4: 2-3 months (offline + merchant tools)
- **Total: 6 months to production-ready system**

## References

- TOS Blockchain: https://github.com/tos-network/tos
- TOS RPC API: `/home/user/tos/docs/DAEMON_RPC_API_REFERENCE.md`
- GHOSTDAG Consensus: `/home/user/tos/TIPs/CONSENSUS_LAYERED_DESIGN.md`
- BIP21 Payment URI: https://github.com/bitcoin/bips/blob/master/bip-0021.mediawiki
- PayPay API: https://developer.paypay.ne.jp/
- Alipay SDK: https://global.alipay.com/docs/ac/

---

**Document Version**: 1.0
**Last Updated**: 2025-10-29
**Author**: TOS Development Team + Claude Code
