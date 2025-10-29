# TOS QR Payment Implementation Guide

## Overview

This guide provides practical code examples for implementing QR code payments on TOS blockchain, including both online and offline (credit-backed) payment modes.

## Prerequisites

- TOS wallet library (`tos-wallet` crate)
- TOS RPC client (`tos-rpc` crate)
- QR code generation library (`qrcode` crate)
- Mobile development framework (React Native / Flutter)

## Installation

```bash
# Add dependencies to Cargo.toml
[dependencies]
tos-common = "0.1"
tos-wallet = "0.1"
qrcode = "0.13"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
base64 = "0.21"
```

## Part 1: Online QR Payments (Basic)

### 1.1 Generate Payment QR Code (Merchant)

```rust
use qrcode::QrCode;
use qrcode::render::unicode;
use tos_common::crypto::Address;
use serde::{Deserialize, Serialize};
use base64::{Engine as _, engine::general_purpose};

/// Payment QR code data structure
#[derive(Serialize, Deserialize)]
pub struct PaymentQRData {
    /// Protocol version
    pub version: u8,

    /// Merchant's TOS address
    pub address: String,

    /// Payment amount (in nanoTOS)
    pub amount: u64,

    /// Payment memo (order ID, invoice ref, etc.)
    pub memo: String,

    /// QR code expiration timestamp (unix time)
    pub expires: u64,
}

impl PaymentQRData {
    /// Generate payment QR code URI
    pub fn to_uri(&self) -> Result<String, Box<dyn std::error::Error>> {
        let memo_base64 = general_purpose::STANDARD.encode(&self.memo);

        let uri = format!(
            "tos://pay?address={}&amount={}&memo={}&expires={}",
            self.address,
            self.amount,
            memo_base64,
            self.expires
        );

        Ok(uri)
    }

    /// Generate QR code image (ASCII art for terminal)
    pub fn generate_qr_code(&self) -> Result<String, Box<dyn std::error::Error>> {
        let uri = self.to_uri()?;
        let code = QrCode::new(uri.as_bytes())?;
        let string = code.render::<unicode::Dense1x2>()
            .dark_color(unicode::Dense1x2::Dark)
            .light_color(unicode::Dense1x2::Light)
            .build();

        Ok(string)
    }
}

/// Example: Merchant generates QR code for 50 TOS payment
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let payment = PaymentQRData {
        version: 1,
        address: "tst12zacnuun3lkv5kxzn2jy8l28d0zft7rqhyxlz2v6h6u23xmruy7sqm0d38u".to_string(),
        amount: 50_000_000_000,  // 50 TOS in nanoTOS
        memo: "ORDER12345".to_string(),
        expires: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() + 3600,  // Valid for 1 hour
    };

    // Generate QR code URI
    let uri = payment.to_uri()?;
    println!("Payment URI: {}", uri);

    // Generate QR code image
    let qr_code = payment.generate_qr_code()?;
    println!("\nScan this QR code:\n{}", qr_code);

    Ok(())
}
```

**Output:**
```
Payment URI: tos://pay?address=tst12zacnuun3lkv5kxzn2jy8l28d0zft7rqhyxlz2v6h6u23xmruy7sqm0d38u&amount=50000000000&memo=T1JERVIxMjM0NQ==&expires=1730000000

Scan this QR code:
█████████████████████████████
█████████████████████████████
████ ▄▄▄▄▄ █▀█ █▄▄▀▄▄▄▄▄ ████
████ █   █ █▀▀▀█ ▄ █   █ ████
████ █▄▄▄█ █▀ █▀▀ █▄▄▄█ ████
████▄▄▄▄▄▄▄█ ▀ █ █▄▄▄▄▄▄████
...
```

### 1.2 Parse QR Code and Create Transaction (Customer)

```rust
use tos_wallet::Wallet;
use tos_common::transaction::{Transaction, TransactionType};
use tos_common::transaction::payload::TransferPayload;
use url::Url;
use base64::{Engine as _, engine::general_purpose};

/// Parse payment QR code URI
pub fn parse_payment_uri(uri: &str) -> Result<PaymentQRData, Box<dyn std::error::Error>> {
    let url = Url::parse(uri)?;

    if url.scheme() != "tos" || url.host_str() != Some("pay") {
        return Err("Invalid TOS payment URI".into());
    }

    let mut address = None;
    let mut amount = None;
    let mut memo = None;
    let mut expires = None;

    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "address" => address = Some(value.to_string()),
            "amount" => amount = Some(value.parse()?),
            "memo" => {
                let decoded = general_purpose::STANDARD.decode(value.as_bytes())?;
                memo = Some(String::from_utf8(decoded)?);
            }
            "expires" => expires = Some(value.parse()?),
            _ => {}
        }
    }

    Ok(PaymentQRData {
        version: 1,
        address: address.ok_or("Missing address")?,
        amount: amount.ok_or("Missing amount")?,
        memo: memo.unwrap_or_default(),
        expires: expires.ok_or("Missing expires")?,
    })
}

/// Create and sign payment transaction
pub async fn create_payment_transaction(
    wallet: &Wallet,
    payment: &PaymentQRData,
) -> Result<Transaction, Box<dyn std::error::Error>> {
    // Check QR code not expired
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();

    if now > payment.expires {
        return Err("Payment QR code expired".into());
    }

    // Parse merchant address
    let merchant_address = Address::from_string(&payment.address)?;

    // Create transfer payload
    let transfer = TransferPayload {
        transfers: vec![tos_common::transaction::payload::Transfer {
            destination: merchant_address,
            amount: payment.amount,
            memo: payment.memo.as_bytes().to_vec(),
        }],
    };

    // Build transaction
    let tx = wallet.create_transaction(
        TransactionType::Transfer(transfer),
        1000,  // Fee: 1000 nanoTOS
    ).await?;

    Ok(tx)
}

/// Example: Customer scans QR code and pays
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Scan QR code (in real app, use camera)
    let qr_uri = "tos://pay?address=tst12zacnuun3lkv5kxzn2jy8l28d0zft7rqhyxlz2v6h6u23xmruy7sqm0d38u&amount=50000000000&memo=T1JERVIxMjM0NQ==&expires=1730000000";

    // 2. Parse payment data
    let payment = parse_payment_uri(qr_uri)?;
    println!("Payment request:");
    println!("  Merchant: {}", payment.address);
    println!("  Amount: {} TOS", payment.amount as f64 / 1_000_000_000.0);
    println!("  Memo: {}", payment.memo);

    // 3. Load wallet
    let wallet = Wallet::open("./wallet.dat", "password")?;

    // 4. Create and sign transaction
    let tx = create_payment_transaction(&wallet, &payment).await?;
    println!("Transaction created: {}", tx.hash());

    // 5. Submit to blockchain
    let rpc_client = tos_rpc::Client::new("http://127.0.0.1:8080")?;
    let result = rpc_client.submit_transaction(&tx).await?;
    println!("Transaction submitted: {:?}", result);

    // 6. Wait for confirmation (10-60 seconds)
    println!("Waiting for confirmation...");
    let confirmed = rpc_client.wait_for_transaction(&tx.hash(), 60).await?;
    println!("Payment confirmed! Block: {}", confirmed.block_height);

    Ok(())
}
```

### 1.3 Monitor Payment Status (Merchant)

```rust
use tos_rpc::Client;
use tos_common::crypto::Hash;

/// Monitor payment status in real-time
pub async fn monitor_payment(
    rpc_client: &Client,
    tx_hash: &Hash,
) -> Result<PaymentStatus, Box<dyn std::error::Error>> {
    // Check if transaction in mempool
    if let Some(tx) = rpc_client.get_transaction_from_mempool(tx_hash).await? {
        return Ok(PaymentStatus::Pending);
    }

    // Check if transaction in block
    if let Some(tx) = rpc_client.get_transaction(tx_hash).await? {
        if tx.executed_in_block.is_some() {
            // Check if finalized (60 blocks)
            let info = rpc_client.get_info().await?;
            let stable_blue_score = info.stable_blue_score;

            if let Some(block_hash) = tx.executed_in_block {
                let block = rpc_client.get_block_at_height(tx.block_height?).await?;
                let block_blue_score = block.header.blue_score;

                if stable_blue_score >= block_blue_score {
                    return Ok(PaymentStatus::Finalized {
                        block_height: tx.block_height?,
                        confirmations: stable_blue_score - block_blue_score,
                    });
                } else {
                    return Ok(PaymentStatus::Confirmed {
                        block_height: tx.block_height?,
                        confirmations: info.blue_score - block_blue_score,
                    });
                }
            }
        }
    }

    Ok(PaymentStatus::NotFound)
}

pub enum PaymentStatus {
    NotFound,
    Pending,
    Confirmed { block_height: u64, confirmations: u64 },
    Finalized { block_height: u64, confirmations: u64 },
}

/// Example: Merchant monitors payment until finalized
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rpc_client = Client::new("http://127.0.0.1:8080")?;
    let tx_hash = Hash::from_string("0x1234...")?;

    loop {
        let status = monitor_payment(&rpc_client, &tx_hash).await?;

        match status {
            PaymentStatus::NotFound => {
                println!("Payment not found (may be submitted soon)");
            }
            PaymentStatus::Pending => {
                println!("Payment pending (in mempool)");
            }
            PaymentStatus::Confirmed { block_height, confirmations } => {
                println!("Payment confirmed at height {} ({} confirmations)",
                    block_height, confirmations);

                if confirmations >= 10 {
                    // Low-value items: accept at 10 confirmations (~10 seconds)
                    println!("Payment accepted (low-risk)");
                    break;
                }
            }
            PaymentStatus::Finalized { block_height, confirmations } => {
                // High-value items: wait for finality (60 confirmations)
                println!("Payment finalized at height {} ({} confirmations)",
                    block_height, confirmations);
                println!("Payment accepted (zero risk)");
                break;
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }

    Ok(())
}
```

## Part 2: Offline Payments with Credit

### 2.1 Establish Credit Line (User)

```rust
use tos_common::crypto::{Address, PrivateKey, Signature};
use serde::{Deserialize, Serialize};

/// Credit certificate issued by insurance contract
#[derive(Serialize, Deserialize, Clone)]
pub struct CreditCertificate {
    /// Certificate ID (unique nonce)
    pub certificate_id: [u8; 32],

    /// User's address
    pub user_address: Address,

    /// Credit limit (in nanoTOS)
    pub credit_limit: u64,

    /// Expiration timestamp
    pub expires_at: u64,

    /// Credit tier
    pub tier: u8,

    /// Insurance contract signature
    pub signature: Signature,
}

/// Request credit line from insurance contract
pub async fn request_credit_line(
    wallet: &Wallet,
    tier: u8,
    collateral_amount: u64,
) -> Result<CreditCertificate, Box<dyn std::error::Error>> {
    // 1. Create contract invocation to deposit collateral
    let contract_address = Address::from_string(
        "tst1insurance_contract_address..."
    )?;

    // 2. Submit transaction to insurance contract
    let tx = wallet.invoke_contract(
        contract_address,
        "deposit_collateral",
        vec![tier.to_string(), collateral_amount.to_string()],
        collateral_amount,  // Deposit amount
        100000,  // Gas limit
    ).await?;

    // 3. Wait for transaction confirmation
    let rpc_client = tos_rpc::Client::new("http://127.0.0.1:8080")?;
    rpc_client.wait_for_transaction(&tx.hash(), 60).await?;

    // 4. Query credit certificate from contract events
    let events = rpc_client.get_contract_events(&contract_address, tx.hash()).await?;

    for event in events {
        if event.event_type == "CreditLineCreated" {
            let certificate: CreditCertificate = serde_json::from_value(event.data)?;
            return Ok(certificate);
        }
    }

    Err("Credit certificate not found".into())
}

/// Example: User deposits 250 TOS for Tier 2 credit (500 USDT limit)
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wallet = Wallet::open("./wallet.dat", "password")?;

    println!("Requesting credit line (Tier 2)...");
    println!("Depositing 250 TOS as collateral...");

    let certificate = request_credit_line(
        &wallet,
        2,  // Tier 2
        250_000_000_000,  // 250 TOS in nanoTOS
    ).await?;

    println!("Credit certificate issued!");
    println!("  Certificate ID: {}", hex::encode(certificate.certificate_id));
    println!("  Credit limit: {} TOS", certificate.credit_limit as f64 / 1_000_000_000.0);
    println!("  Expires: {}", certificate.expires_at);

    // Save certificate to wallet storage
    wallet.save_credit_certificate(&certificate)?;

    Ok(())
}
```

### 2.2 Create Offline Payment Proof (Customer)

```rust
use tos_common::crypto::{PrivateKey, Signature, sign_message};
use serde::{Deserialize, Serialize};
use rand::Rng;

/// Offline payment proof (signed message)
#[derive(Serialize, Deserialize, Clone)]
pub struct OfflinePaymentProof {
    /// Protocol version
    pub version: u8,

    /// Payment type
    pub payment_type: String,

    /// Sender address
    pub from_address: String,

    /// Recipient address (merchant)
    pub to_address: String,

    /// Payment amount (in nanoTOS)
    pub amount: u64,

    /// Payment memo
    pub memo: String,

    /// Certificate ID from credit line
    pub certificate_id: String,

    /// Unique nonce (replay protection)
    pub nonce: [u8; 32],

    /// Timestamp when payment was created
    pub timestamp: u64,

    /// Expiration timestamp
    pub expires_at: u64,

    /// Customer's signature
    pub customer_signature: String,

    /// Merchant's signature (added after delivery)
    pub merchant_signature: Option<String>,
}

impl OfflinePaymentProof {
    /// Create message to sign (customer)
    fn customer_message_to_sign(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(self.from_address.as_bytes());
        data.extend_from_slice(self.to_address.as_bytes());
        data.extend_from_slice(&self.amount.to_le_bytes());
        data.extend_from_slice(self.memo.as_bytes());
        data.extend_from_slice(&self.nonce);
        data.extend_from_slice(&self.timestamp.to_le_bytes());
        data
    }

    /// Create message to sign (merchant)
    fn merchant_message_to_sign(&self) -> Vec<u8> {
        let mut data = self.customer_message_to_sign();
        data.extend_from_slice(self.customer_signature.as_bytes());
        data
    }

    /// Sign payment proof (customer)
    pub fn sign_by_customer(
        &mut self,
        private_key: &PrivateKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let message = self.customer_message_to_sign();
        let signature = sign_message(&message, private_key)?;
        self.customer_signature = hex::encode(signature.to_bytes());
        Ok(())
    }

    /// Sign payment proof (merchant - proof of delivery)
    pub fn sign_by_merchant(
        &mut self,
        private_key: &PrivateKey,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let message = self.merchant_message_to_sign();
        let signature = sign_message(&message, private_key)?;
        self.merchant_signature = Some(hex::encode(signature.to_bytes()));
        Ok(())
    }

    /// Serialize to JSON for transfer
    pub fn to_json(&self) -> Result<String, Box<dyn std::error::Error>> {
        Ok(serde_json::to_string(self)?)
    }

    /// Deserialize from JSON
    pub fn from_json(json: &str) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(serde_json::from_str(json)?)
    }
}

/// Create offline payment proof (when network is unavailable)
pub fn create_offline_payment(
    wallet: &Wallet,
    certificate: &CreditCertificate,
    merchant_address: &str,
    amount: u64,
    memo: &str,
) -> Result<OfflinePaymentProof, Box<dyn std::error::Error>> {
    // Generate unique nonce
    let mut rng = rand::thread_rng();
    let nonce: [u8; 32] = rng.gen();

    // Get current timestamp
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();

    // Create payment proof
    let mut proof = OfflinePaymentProof {
        version: 1,
        payment_type: "offline_credit".to_string(),
        from_address: wallet.get_address().to_string(),
        to_address: merchant_address.to_string(),
        amount,
        memo: memo.to_string(),
        certificate_id: hex::encode(certificate.certificate_id),
        nonce,
        timestamp,
        expires_at: timestamp + 3600,  // Valid for 1 hour
        customer_signature: String::new(),
        merchant_signature: None,
    };

    // Sign with wallet private key
    proof.sign_by_customer(wallet.get_private_key())?;

    Ok(proof)
}

/// Example: Customer creates offline payment (no network)
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wallet = Wallet::open("./wallet.dat", "password")?;

    // Load credit certificate from wallet
    let certificate = wallet.load_credit_certificate()?;

    // Create offline payment proof
    let proof = create_offline_payment(
        &wallet,
        &certificate,
        "tst12zacnuun3lkv5kxzn2jy8l28d0zft7rqhyxlz2v6h6u23xmruy7sqm0d38u",
        50_000_000_000,  // 50 TOS
        "ORDER12345",
    )?;

    println!("Offline payment proof created:");
    println!("{}", proof.to_json()?);

    // Transfer to merchant via Bluetooth/NFC/QR code
    let qr_code = QrCode::new(proof.to_json()?.as_bytes())?;
    println!("\nShow this QR code to merchant:");
    println!("{}", qr_code.render::<unicode::Dense1x2>().build());

    // Save to queue for later settlement
    wallet.queue_offline_payment(&proof)?;

    Ok(())
}
```

### 2.3 Validate Offline Payment (Merchant)

```rust
use tos_common::crypto::{verify_signature, Signature, PublicKey};

/// Validate offline payment proof (merchant)
pub fn validate_offline_payment(
    proof: &OfflinePaymentProof,
) -> Result<ValidationResult, Box<dyn std::error::Error>> {
    let mut warnings = Vec::new();

    // 1. Check proof not expired
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();

    if now > proof.expires_at {
        return Err("Payment proof expired".into());
    }

    // 2. Verify customer signature
    let customer_address = Address::from_string(&proof.from_address)?;
    let customer_pubkey = PublicKey::from_address(&customer_address)?;

    let message = proof.customer_message_to_sign();
    let signature = Signature::from_hex(&proof.customer_signature)?;

    if !verify_signature(&message, &signature, &customer_pubkey) {
        return Err("Invalid customer signature".into());
    }

    // 3. Check certificate validity (query from contract or cache)
    // Note: In offline mode, merchant relies on previously cached certificates
    let certificate_valid = check_certificate_validity(&proof.certificate_id)?;
    if !certificate_valid {
        warnings.push("Certificate validity cannot be verified (offline)");
    }

    // 4. Check amount within reasonable limits
    if proof.amount > 1000_000_000_000 {  // > 1000 TOS
        warnings.push("High-value payment (>1000 TOS) - recommend online verification");
    }

    // 5. Check nonce not reused (local database)
    let nonce_used = check_nonce_used(&proof.nonce)?;
    if nonce_used {
        return Err("Nonce already used (replay attack)".into());
    }

    Ok(ValidationResult {
        valid: true,
        warnings,
        amount: proof.amount,
        customer: proof.from_address.clone(),
    })
}

pub struct ValidationResult {
    pub valid: bool,
    pub warnings: Vec<&'static str>,
    pub amount: u64,
    pub customer: String,
}

/// Example: Merchant receives and validates offline payment
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Receive payment proof from customer (via Bluetooth/NFC/QR)
    let proof_json = r#"{
        "version": 1,
        "payment_type": "offline_credit",
        "from_address": "tst1xxx...",
        "to_address": "tst1yyy...",
        "amount": 50000000000,
        "memo": "ORDER12345",
        "certificate_id": "1234...",
        "nonce": [1,2,3,...],
        "timestamp": 1730000000,
        "expires_at": 1730003600,
        "customer_signature": "abcdef...",
        "merchant_signature": null
    }"#;

    let proof = OfflinePaymentProof::from_json(proof_json)?;

    // 2. Validate payment proof
    println!("Validating payment proof...");
    let validation = validate_offline_payment(&proof)?;

    if validation.valid {
        println!("Payment valid!");
        println!("  Amount: {} TOS", validation.amount as f64 / 1_000_000_000.0);
        println!("  Customer: {}", validation.customer);

        if !validation.warnings.is_empty() {
            println!("  Warnings:");
            for warning in validation.warnings {
                println!("    - {}", warning);
            }
        }

        // 3. Merchant signs as proof of delivery
        let merchant_wallet = Wallet::open("./merchant_wallet.dat", "password")?;
        let mut signed_proof = proof.clone();
        signed_proof.sign_by_merchant(merchant_wallet.get_private_key())?;

        // 4. Save to settlement queue
        merchant_wallet.queue_offline_payment(&signed_proof)?;

        println!("Payment accepted! Goods delivered.");
    } else {
        println!("Payment invalid - rejected");
    }

    Ok(())
}
```

### 2.4 Settle Offline Payments (When Online)

```rust
use tos_rpc::Client;

/// Settle queued offline payments when connectivity is restored
pub async fn settle_queued_payments(
    wallet: &Wallet,
) -> Result<SettlementSummary, Box<dyn std::error::Error>> {
    let rpc_client = Client::new("http://127.0.0.1:8080")?;

    // 1. Load queued payments from wallet
    let queued_payments = wallet.load_queued_payments()?;

    if queued_payments.is_empty() {
        println!("No queued payments to settle");
        return Ok(SettlementSummary::default());
    }

    println!("Found {} queued payments", queued_payments.len());

    // 2. Batch settlements (10 at a time for gas optimization)
    let mut total_settled = 0;
    let mut total_failed = 0;

    for chunk in queued_payments.chunks(10) {
        // 3. Submit batch settlement to insurance contract
        let tx = wallet.invoke_contract(
            Address::from_string("tst1insurance_contract_address...")?,
            "batch_settle_payments",
            vec![serde_json::to_string(&chunk)?],
            0,  // No deposit
            1_000_000,  // Gas limit for batch
        ).await?;

        // 4. Wait for confirmation
        rpc_client.wait_for_transaction(&tx.hash(), 60).await?;

        // 5. Check settlement results from events
        let events = rpc_client.get_contract_events(
            &Address::from_string("tst1insurance_contract_address...")?,
            tx.hash()
        ).await?;

        for event in events {
            match event.event_type.as_str() {
                "PaymentSettledByUser" => {
                    total_settled += 1;
                    println!("  ✓ Payment settled by user balance");
                }
                "PaymentSettledByInsurance" => {
                    total_settled += 1;
                    println!("  ⚠ Payment settled by insurance (debt created)");
                }
                "SettlementFailed" => {
                    total_failed += 1;
                    println!("  ✗ Payment settlement failed");
                }
                _ => {}
            }
        }

        // 6. Remove settled payments from queue
        wallet.remove_settled_payments(chunk)?;
    }

    Ok(SettlementSummary {
        total_queued: queued_payments.len(),
        total_settled,
        total_failed,
    })
}

#[derive(Default)]
pub struct SettlementSummary {
    pub total_queued: usize,
    pub total_settled: usize,
    pub total_failed: usize,
}

/// Example: Wallet auto-settles when connectivity restored
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let wallet = Wallet::open("./wallet.dat", "password")?;

    // Check if online
    if !is_online().await {
        println!("No network connectivity - payments remain queued");
        return Ok(());
    }

    println!("Network connectivity restored!");
    println!("Settling queued offline payments...");

    let summary = settle_queued_payments(&wallet).await?;

    println!("\nSettlement Summary:");
    println!("  Total queued: {}", summary.total_queued);
    println!("  Successfully settled: {}", summary.total_settled);
    println!("  Failed: {}", summary.total_failed);

    if summary.total_failed > 0 {
        println!("\nNote: Failed payments remain in queue for retry");
    }

    Ok(())
}

async fn is_online() -> bool {
    // Check network connectivity
    tokio::net::TcpStream::connect("127.0.0.1:8080").await.is_ok()
}
```

## Part 3: Mobile Integration (React Native Example)

### 3.1 QR Code Scanner Component

```typescript
// QRPaymentScanner.tsx
import React, { useState } from 'react';
import { View, Text, Button, Alert } from 'react-native';
import { Camera } from 'expo-camera';
import { TOSWallet } from '@tos/wallet-sdk';

interface PaymentQRData {
  address: string;
  amount: string;
  memo: string;
  expires: number;
}

export const QRPaymentScanner: React.FC = () => {
  const [hasPermission, setHasPermission] = useState<boolean | null>(null);
  const [scanned, setScanned] = useState(false);
  const [paymentData, setPaymentData] = useState<PaymentQRData | null>(null);

  const handleBarCodeScanned = async ({ type, data }: any) => {
    setScanned(true);

    try {
      // Parse TOS payment URI
      const url = new URL(data);

      if (url.protocol !== 'tos:' || url.hostname !== 'pay') {
        Alert.alert('Error', 'Invalid TOS payment QR code');
        return;
      }

      const params = url.searchParams;
      const payment: PaymentQRData = {
        address: params.get('address') || '',
        amount: params.get('amount') || '0',
        memo: atob(params.get('memo') || ''),
        expires: parseInt(params.get('expires') || '0'),
      };

      // Check expiration
      const now = Math.floor(Date.now() / 1000);
      if (now > payment.expires) {
        Alert.alert('Error', 'Payment QR code expired');
        return;
      }

      setPaymentData(payment);

      // Show payment confirmation dialog
      Alert.alert(
        'Confirm Payment',
        `Pay ${(parseInt(payment.amount) / 1e9).toFixed(2)} TOS to merchant?\nMemo: ${payment.memo}`,
        [
          { text: 'Cancel', style: 'cancel', onPress: () => setScanned(false) },
          { text: 'Pay', onPress: () => processPayment(payment) },
        ]
      );
    } catch (err) {
      Alert.alert('Error', 'Failed to parse QR code');
      setScanned(false);
    }
  };

  const processPayment = async (payment: PaymentQRData) => {
    try {
      // Load wallet
      const wallet = await TOSWallet.load('wallet_data');

      // Create and submit transaction
      const tx = await wallet.createTransfer(
        payment.address,
        payment.amount,
        payment.memo
      );

      await wallet.submitTransaction(tx);

      Alert.alert(
        'Payment Submitted',
        `Transaction: ${tx.hash}\nWaiting for confirmation...`
      );

      // Wait for confirmation (10-60 seconds)
      const confirmed = await wallet.waitForConfirmation(tx.hash, 60);

      if (confirmed) {
        Alert.alert('Success', 'Payment confirmed!');
      } else {
        Alert.alert('Warning', 'Payment still pending - check status later');
      }
    } catch (err) {
      Alert.alert('Error', `Payment failed: ${err.message}`);
    }

    setScanned(false);
  };

  return (
    <View style={{ flex: 1 }}>
      {hasPermission ? (
        <Camera
          style={{ flex: 1 }}
          onBarCodeScanned={scanned ? undefined : handleBarCodeScanned}
        >
          <View style={{ flex: 1, justifyContent: 'center', alignItems: 'center' }}>
            <Text style={{ color: 'white', fontSize: 20 }}>
              Scan merchant QR code to pay
            </Text>
          </View>
        </Camera>
      ) : (
        <View style={{ flex: 1, justifyContent: 'center', alignItems: 'center' }}>
          <Button title="Grant Camera Permission" onPress={requestPermission} />
        </View>
      )}
    </View>
  );
};
```

### 3.2 Offline Payment UI

```typescript
// OfflinePaymentScreen.tsx
import React, { useState, useEffect } from 'react';
import { View, Text, Button, FlatList, Alert } from 'react-native';
import { TOSWallet } from '@tos/wallet-sdk';
import NetInfo from '@react-native-community/netinfo';

export const OfflinePaymentScreen: React.FC = () => {
  const [queuedPayments, setQueuedPayments] = useState([]);
  const [isOnline, setIsOnline] = useState(false);
  const [creditInfo, setCreditInfo] = useState(null);

  useEffect(() => {
    // Monitor network connectivity
    const unsubscribe = NetInfo.addEventListener(state => {
      setIsOnline(state.isConnected);

      if (state.isConnected && queuedPayments.length > 0) {
        // Auto-settle when connectivity restored
        settleQueuedPayments();
      }
    });

    // Load queued payments
    loadQueuedPayments();

    // Load credit info
    loadCreditInfo();

    return () => unsubscribe();
  }, []);

  const loadQueuedPayments = async () => {
    const wallet = await TOSWallet.load('wallet_data');
    const queued = await wallet.getQueuedPayments();
    setQueuedPayments(queued);
  };

  const loadCreditInfo = async () => {
    const wallet = await TOSWallet.load('wallet_data');
    const credit = await wallet.getCreditCertificate();
    setCreditInfo(credit);
  };

  const settleQueuedPayments = async () => {
    Alert.alert('Settling Payments', 'Network restored - settling queued payments...');

    try {
      const wallet = await TOSWallet.load('wallet_data');
      const summary = await wallet.settleQueuedPayments();

      Alert.alert(
        'Settlement Complete',
        `Settled: ${summary.settled}\nFailed: ${summary.failed}`
      );

      // Reload queued payments
      loadQueuedPayments();
      loadCreditInfo();
    } catch (err) {
      Alert.alert('Error', `Settlement failed: ${err.message}`);
    }
  };

  return (
    <View style={{ flex: 1, padding: 20 }}>
      {/* Credit Line Status */}
      <View style={{ marginBottom: 20 }}>
        <Text style={{ fontSize: 20, fontWeight: 'bold' }}>Credit Line Status</Text>
        {creditInfo ? (
          <>
            <Text>Credit Limit: {(creditInfo.credit_limit / 1e9).toFixed(2)} TOS</Text>
            <Text>Available: {(creditInfo.available_credit / 1e9).toFixed(2)} TOS</Text>
            <Text>Expires: {new Date(creditInfo.expires_at * 1000).toLocaleDateString()}</Text>
          </>
        ) : (
          <Text>No credit line - tap to create</Text>
        )}
      </View>

      {/* Network Status */}
      <View style={{ marginBottom: 20 }}>
        <Text style={{ fontSize: 20, fontWeight: 'bold' }}>Network Status</Text>
        <Text style={{ color: isOnline ? 'green' : 'red' }}>
          {isOnline ? '🟢 Online' : '🔴 Offline'}
        </Text>
      </View>

      {/* Queued Payments */}
      <View style={{ flex: 1 }}>
        <Text style={{ fontSize: 20, fontWeight: 'bold' }}>Queued Payments</Text>
        {queuedPayments.length > 0 ? (
          <FlatList
            data={queuedPayments}
            keyExtractor={(item) => item.nonce}
            renderItem={({ item }) => (
              <View style={{ padding: 10, borderBottomWidth: 1 }}>
                <Text>Amount: {(item.amount / 1e9).toFixed(2)} TOS</Text>
                <Text>To: {item.to_address.substring(0, 20)}...</Text>
                <Text>Memo: {item.memo}</Text>
                <Text>Created: {new Date(item.timestamp * 1000).toLocaleString()}</Text>
              </View>
            )}
          />
        ) : (
          <Text style={{ textAlign: 'center', marginTop: 20 }}>
            No queued payments
          </Text>
        )}
      </View>

      {/* Manual Settle Button */}
      {isOnline && queuedPayments.length > 0 && (
        <Button title="Settle Now" onPress={settleQueuedPayments} />
      )}
    </View>
  );
};
```

## Conclusion

This implementation guide provides all the code examples needed to build a complete QR code payment system on TOS blockchain, including:

1. **Online payments**: Fast QR code payments with 10-60 second confirmation
2. **Offline payments**: Credit-backed payments with later settlement
3. **Mobile integration**: React Native components for production apps

**Next Steps:**

1. Implement the insurance credit contract (see `INSURANCE_CREDIT_CONTRACT_SPEC.md`)
2. Build mobile wallet SDK with QR code support
3. Deploy payment gateway REST API for merchants
4. Create merchant dashboard for transaction monitoring
5. Test end-to-end payment flow on devnet

---

**Document Version**: 1.0
**Last Updated**: 2025-10-29
**Author**: TOS Development Team + Claude Code
