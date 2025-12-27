#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::disallowed_methods)]

//! Payment System Integration Tests
//!
//! Comprehensive tests for the TIP-QR-PAYMENT system including:
//! - Payment request creation and parsing
//! - URI generation and roundtrip
//! - Extra data encoding/decoding
//! - Payment ID validation
//! - Expiration handling
//! - Status state machine
//!
//! These tests verify the complete payment flow as specified in TIP-QR-PAYMENT.md

use std::borrow::Cow;
use std::time::{SystemTime, UNIX_EPOCH};

use tos_common::api::payment::{
    decode_payment_extra_data, encode_payment_extra_data, is_valid_payment_id, validate_payment_id,
    GetPaymentStatusParams, PaymentIdError, PaymentParseError, PaymentRequest, PaymentStatus,
    PaymentStatusResponse, StoredPaymentRequest,
};
use tos_common::crypto::Address;

// ============================================================================
// Test Helpers
// ============================================================================

fn test_address() -> Address {
    "tst12zacnuun3lkv5kxzn2jy8l28d0zft7rqhyxlz2v6h6u23xmruy7sqm0d38u"
        .parse()
        .unwrap()
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ============================================================================
// Payment Request Creation Tests
// ============================================================================

#[test]
fn test_create_payment_request_basic() {
    let addr = test_address();
    let request = PaymentRequest::new("order-001", addr.clone());

    assert_eq!(request.payment_id.as_ref(), "order-001");
    assert_eq!(request.address, addr);
    assert!(request.amount.is_none());
    assert!(request.memo.is_none());
    assert!(request.expires_at.is_none());
}

#[test]
fn test_create_payment_request_with_amount() {
    let addr = test_address();
    let request = PaymentRequest::new("order-002", addr).with_amount(1_000_000_000); // 1 TOS

    assert_eq!(request.amount, Some(1_000_000_000));
}

#[test]
fn test_create_payment_request_with_memo() {
    let addr = test_address();
    let request = PaymentRequest::new("order-003", addr).with_memo("Coffee payment");

    assert_eq!(request.memo.as_deref(), Some("Coffee payment"));
}

#[test]
fn test_create_payment_request_with_expiration() {
    let addr = test_address();
    let expires_at = current_timestamp() + 300; // 5 minutes from now
    let request = PaymentRequest::new("order-004", addr).with_expires_at(expires_at);

    assert_eq!(request.expires_at, Some(expires_at));
    assert!(!request.is_expired());
}

#[test]
fn test_create_payment_request_expired() {
    let addr = test_address();
    let request = PaymentRequest::new("order-005", addr).with_expires_at(1); // Long ago

    assert!(request.is_expired());
}

#[test]
fn test_create_payment_request_full() {
    let addr = test_address();
    let expires_at = current_timestamp() + 600;

    let request = PaymentRequest::new("shop_123_abc", addr.clone())
        .with_amount(5_000_000_000) // 5 TOS
        .with_memo("Premium subscription")
        .with_expires_at(expires_at)
        .with_callback("https://shop.example.com/webhook");

    assert_eq!(request.payment_id.as_ref(), "shop_123_abc");
    assert_eq!(request.address, addr);
    assert_eq!(request.amount, Some(5_000_000_000));
    assert_eq!(request.memo.as_deref(), Some("Premium subscription"));
    assert_eq!(request.expires_at, Some(expires_at));
    assert_eq!(
        request.callback.as_deref(),
        Some("https://shop.example.com/webhook")
    );
}

// ============================================================================
// URI Generation and Parsing Tests
// ============================================================================

#[test]
fn test_uri_generation_minimal() {
    let addr = test_address();
    let request = PaymentRequest::new("test-123", addr);
    let uri = request.to_uri();

    assert!(uri.starts_with("tos://pay?to="));
    assert!(uri.contains("id=test-123"));
}

#[test]
fn test_uri_generation_with_amount() {
    let addr = test_address();
    let request = PaymentRequest::new("test-456", addr).with_amount(1_000_000_000);
    let uri = request.to_uri();

    assert!(uri.contains("amount=1000000000"));
}

#[test]
fn test_uri_generation_with_memo_encoding() {
    let addr = test_address();
    let request = PaymentRequest::new("test-789", addr).with_memo("Hello World!");
    let uri = request.to_uri();

    // Memo should be URL-encoded
    assert!(uri.contains("memo=Hello%20World%21"));
}

#[test]
fn test_uri_roundtrip() {
    let addr = test_address();
    let expires_at = 1734567890;

    let original = PaymentRequest::new("roundtrip-001", addr)
        .with_amount(2_500_000_000)
        .with_memo("Test payment")
        .with_expires_at(expires_at);

    let uri = original.to_uri();
    let parsed = PaymentRequest::from_uri(&uri).unwrap();

    assert_eq!(original.payment_id.as_ref(), parsed.payment_id.as_ref());
    assert_eq!(original.amount, parsed.amount);
    assert_eq!(original.memo.as_deref(), parsed.memo.as_deref());
    assert_eq!(original.expires_at, parsed.expires_at);
}

#[test]
fn test_uri_parsing_minimal() {
    let uri = "tos://pay?to=tst12zacnuun3lkv5kxzn2jy8l28d0zft7rqhyxlz2v6h6u23xmruy7sqm0d38u";
    let request = PaymentRequest::from_uri(uri).unwrap();

    assert_eq!(request.address, test_address());
    assert!(request.amount.is_none());
    // Payment ID should be auto-generated
    assert!(!request.payment_id.is_empty());
}

#[test]
fn test_uri_parsing_invalid_scheme() {
    let uri = "http://example.com/pay";
    let result = PaymentRequest::from_uri(uri);

    assert!(matches!(result, Err(PaymentParseError::InvalidScheme)));
}

#[test]
fn test_uri_parsing_missing_address() {
    let uri = "tos://pay?amount=100";
    let result = PaymentRequest::from_uri(uri);

    assert!(matches!(result, Err(PaymentParseError::MissingAddress)));
}

#[test]
fn test_uri_parsing_invalid_payment_id() {
    // Payment ID with invalid characters
    let uri = "tos://pay?to=tst12zacnuun3lkv5kxzn2jy8l28d0zft7rqhyxlz2v6h6u23xmruy7sqm0d38u&id=invalid@id";
    let result = PaymentRequest::from_uri(uri);

    assert!(matches!(
        result,
        Err(PaymentParseError::InvalidPaymentId(_))
    ));
}

#[test]
fn test_uri_parsing_payment_id_too_long() {
    let long_id = "a".repeat(33); // > 32 bytes
    let uri = format!(
        "tos://pay?to=tst12zacnuun3lkv5kxzn2jy8l28d0zft7rqhyxlz2v6h6u23xmruy7sqm0d38u&id={}",
        long_id
    );
    let result = PaymentRequest::from_uri(&uri);

    assert!(matches!(
        result,
        Err(PaymentParseError::InvalidPaymentId(PaymentIdError::TooLong))
    ));
}

// ============================================================================
// Payment ID Validation Tests
// ============================================================================

#[test]
fn test_payment_id_valid() {
    assert!(is_valid_payment_id("order-123"));
    assert!(is_valid_payment_id("pr_abc123_def456"));
    assert!(is_valid_payment_id("SHOP_ORDER_001"));
    assert!(is_valid_payment_id("a")); // Single char
    assert!(is_valid_payment_id(&"a".repeat(32))); // Max length
}

#[test]
fn test_payment_id_invalid_empty() {
    let result = validate_payment_id("");
    assert!(matches!(result, Err(PaymentIdError::Empty)));
}

#[test]
fn test_payment_id_invalid_too_long() {
    let result = validate_payment_id(&"a".repeat(33));
    assert!(matches!(result, Err(PaymentIdError::TooLong)));
}

#[test]
fn test_payment_id_invalid_chars() {
    assert!(!is_valid_payment_id("order@123")); // @ not allowed
    assert!(!is_valid_payment_id("order#456")); // # not allowed
    assert!(!is_valid_payment_id("order 789")); // space not allowed
    assert!(!is_valid_payment_id("order/abc")); // / not allowed
}

#[test]
fn test_payment_id_allowed_chars() {
    // Only A-Z, a-z, 0-9, -, _ are allowed
    assert!(is_valid_payment_id("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef"));
    assert!(is_valid_payment_id("0123456789"));
    assert!(is_valid_payment_id("order-with-dashes"));
    assert!(is_valid_payment_id("order_with_underscores"));
    assert!(is_valid_payment_id("Mixed-Case_123"));
}

// ============================================================================
// Extra Data Encoding/Decoding Tests
// ============================================================================

#[test]
fn test_extra_data_encode_decode() {
    let payment_id = "order-12345";
    let memo = Some("Coffee");

    let encoded = encode_payment_extra_data(payment_id, memo).unwrap();
    let (decoded_id, decoded_memo) = decode_payment_extra_data(&encoded).unwrap();

    assert_eq!(decoded_id, payment_id);
    assert_eq!(decoded_memo.as_deref(), memo);
}

#[test]
fn test_extra_data_encode_decode_no_memo() {
    let payment_id = "order-67890";

    let encoded = encode_payment_extra_data(payment_id, None).unwrap();
    let (decoded_id, decoded_memo) = decode_payment_extra_data(&encoded).unwrap();

    assert_eq!(decoded_id, payment_id);
    assert!(decoded_memo.is_none());
}

#[test]
fn test_extra_data_encode_decode_long_memo() {
    let payment_id = "order-abc";
    let long_memo = "x".repeat(100); // Long memo, will be truncated

    let encoded = encode_payment_extra_data(payment_id, Some(&long_memo)).unwrap();
    assert!(encoded.len() <= 128); // Max extra_data size

    let (decoded_id, decoded_memo) = decode_payment_extra_data(&encoded).unwrap();
    assert_eq!(decoded_id, payment_id);
    // Memo should be truncated to fit
    assert!(decoded_memo.unwrap().len() <= 95); // 128 - 33 = 95 max memo bytes
}

#[test]
fn test_extra_data_encode_invalid_payment_id() {
    // Payment ID too long
    let result = encode_payment_extra_data(&"a".repeat(50), None);
    assert!(result.is_none());
}

#[test]
fn test_extra_data_decode_invalid_type() {
    // Wrong type byte
    let data = vec![0x02, 0x01, 0x02, 0x03];
    let result = decode_payment_extra_data(&data);
    assert!(result.is_none());
}

#[test]
fn test_extra_data_decode_too_short() {
    // Too short
    let data = vec![0x01];
    let result = decode_payment_extra_data(&data);
    assert!(result.is_none());
}

// ============================================================================
// Memo Truncation Tests
// ============================================================================

#[test]
fn test_memo_truncation_at_byte_boundary() {
    let addr = test_address();
    // Create a memo with multi-byte UTF-8 characters
    let long_memo = "Hello ".to_string() + &"世界".repeat(20); // ~80+ bytes

    let request = PaymentRequest::new("test", addr).with_memo(&long_memo);

    // Memo should be truncated to MAX_MEMO_LENGTH (64 bytes)
    let memo = request.memo.unwrap();
    assert!(memo.len() <= 64);
    // Should be valid UTF-8
    assert!(std::str::from_utf8(memo.as_bytes()).is_ok());
}

#[test]
fn test_memo_no_truncation_needed() {
    let addr = test_address();
    let short_memo = "Short memo";

    let request = PaymentRequest::new("test", addr).with_memo(short_memo);

    assert_eq!(request.memo.as_deref(), Some(short_memo));
}

// ============================================================================
// Payment Status State Machine Tests
// ============================================================================

#[test]
fn test_stored_payment_status_pending() {
    let stored = StoredPaymentRequest {
        payment_id: "test-001".to_string(),
        address: test_address(),
        amount: Some(1_000_000_000),
        asset: None,
        memo: None,
        created_at: current_timestamp(),
        expires_at: Some(current_timestamp() + 300),
        tx_hash: None,
        amount_received: None,
        confirmed_at_topoheight: None,
    };

    let status = stored.get_status(100, 90);
    assert_eq!(status, PaymentStatus::Pending);
}

#[test]
fn test_stored_payment_status_expired() {
    let stored = StoredPaymentRequest {
        payment_id: "test-002".to_string(),
        address: test_address(),
        amount: Some(1_000_000_000),
        asset: None,
        memo: None,
        created_at: current_timestamp() - 600,
        expires_at: Some(current_timestamp() - 300), // Expired
        tx_hash: None,
        amount_received: None,
        confirmed_at_topoheight: None,
    };

    let status = stored.get_status(100, 90);
    assert_eq!(status, PaymentStatus::Expired);
}

#[test]
fn test_stored_payment_status_confirming() {
    let stored = StoredPaymentRequest {
        payment_id: "test-003".to_string(),
        address: test_address(),
        amount: Some(1_000_000_000),
        asset: None,
        memo: None,
        created_at: current_timestamp(),
        expires_at: None,
        tx_hash: Some(tos_common::crypto::Hash::zero()),
        amount_received: Some(1_000_000_000),
        confirmed_at_topoheight: Some(98), // 3 confirmations (100 - 98 + 1)
    };

    // current=100, stable=90, confirmed_at=98
    // confirmations = 100 - 98 + 1 = 3 < 8 (STABLE_CONFIRMATIONS)
    let status = stored.get_status(100, 90);
    assert_eq!(status, PaymentStatus::Confirming);
}

#[test]
fn test_stored_payment_status_confirmed() {
    let stored = StoredPaymentRequest {
        payment_id: "test-004".to_string(),
        address: test_address(),
        amount: Some(1_000_000_000),
        asset: None,
        memo: None,
        created_at: current_timestamp(),
        expires_at: None,
        tx_hash: Some(tos_common::crypto::Hash::zero()),
        amount_received: Some(1_000_000_000),
        confirmed_at_topoheight: Some(85), // Below stable height
    };

    // current=100, stable=90, confirmed_at=85 (below stable)
    let status = stored.get_status(100, 90);
    assert_eq!(status, PaymentStatus::Confirmed);
}

#[test]
fn test_stored_payment_status_underpaid() {
    let stored = StoredPaymentRequest {
        payment_id: "test-005".to_string(),
        address: test_address(),
        amount: Some(1_000_000_000), // Expected 1 TOS
        asset: None,
        memo: None,
        created_at: current_timestamp(),
        expires_at: None,
        tx_hash: Some(tos_common::crypto::Hash::zero()),
        amount_received: Some(500_000_000), // Only 0.5 TOS received
        confirmed_at_topoheight: Some(85),
    };

    let status = stored.get_status(100, 90);
    assert_eq!(status, PaymentStatus::Underpaid);
}

// ============================================================================
// GetPaymentStatusParams Tests
// ============================================================================

#[test]
fn test_get_payment_status_params_minimal() {
    let params = GetPaymentStatusParams {
        payment_id: "order-123".to_string(),
        address: test_address(),
        expected_amount: None,
        exp: None,
        min_topoheight: None,
    };

    assert_eq!(params.payment_id, "order-123");
    assert!(params.expected_amount.is_none());
    assert!(params.min_topoheight.is_none());
}

#[test]
fn test_get_payment_status_params_full() {
    let params = GetPaymentStatusParams {
        payment_id: "order-456".to_string(),
        address: test_address(),
        expected_amount: Some(1_000_000_000),
        exp: Some(1734567890),
        min_topoheight: Some(100000),
    };

    assert_eq!(params.expected_amount, Some(1_000_000_000));
    assert_eq!(params.exp, Some(1734567890));
    assert_eq!(params.min_topoheight, Some(100000));
}

// ============================================================================
// PaymentStatusResponse Tests
// ============================================================================

#[test]
fn test_payment_status_response_pending() {
    let response = PaymentStatusResponse {
        payment_id: Cow::Borrowed("test"),
        status: PaymentStatus::Pending,
        tx_hash: None,
        amount_received: None,
        confirmations: None,
        confirmed_at: None,
    };

    assert_eq!(response.status, PaymentStatus::Pending);
    assert!(response.tx_hash.is_none());
}

#[test]
fn test_payment_status_response_confirmed() {
    let response = PaymentStatusResponse {
        payment_id: Cow::Borrowed("test"),
        status: PaymentStatus::Confirmed,
        tx_hash: Some(Cow::Owned(tos_common::crypto::Hash::zero())),
        amount_received: Some(1_000_000_000),
        confirmations: Some(10),
        confirmed_at: Some(1734567890),
    };

    assert_eq!(response.status, PaymentStatus::Confirmed);
    assert!(response.tx_hash.is_some());
    assert_eq!(response.amount_received, Some(1_000_000_000));
    assert_eq!(response.confirmations, Some(10));
}

// ============================================================================
// End-to-End Flow Simulation Tests
// ============================================================================

#[test]
fn test_e2e_payment_flow_simulation() {
    // Step 1: Merchant creates payment request
    let merchant_address = test_address();
    let payment_id = "shop_order_001";
    let amount = 2_500_000_000u64; // 2.5 TOS
    let expires_at = current_timestamp() + 300; // 5 minutes

    let request = PaymentRequest::new(payment_id, merchant_address.clone())
        .with_amount(amount)
        .with_memo("Order #001 - Premium Widget")
        .with_expires_at(expires_at);

    // Generate QR code URI
    let uri = request.to_uri();
    assert!(uri.contains("tos://pay?"));
    assert!(uri.contains(&format!("id={}", payment_id)));

    // Step 2: Customer scans and parses URI
    let parsed = PaymentRequest::from_uri(&uri).unwrap();
    assert_eq!(parsed.payment_id.as_ref(), payment_id);
    assert_eq!(parsed.amount, Some(amount));
    assert!(!parsed.is_expired());

    // Step 3: Customer pays (simulated by creating extra_data)
    let extra_data = encode_payment_extra_data(payment_id, Some("Order #001")).unwrap();
    assert!(extra_data.len() <= 128);

    // Step 4: Merchant checks status (simulated)
    // Decode extra_data from transaction
    let (found_id, found_memo) = decode_payment_extra_data(&extra_data).unwrap();
    assert_eq!(found_id, payment_id);
    assert_eq!(found_memo.as_deref(), Some("Order #001"));

    // Simulate payment found and confirmed
    let stored = StoredPaymentRequest {
        payment_id: payment_id.to_string(),
        address: merchant_address,
        amount: Some(amount),
        asset: None,
        memo: request.memo.map(|m| m.into_owned()),
        created_at: current_timestamp(),
        expires_at: Some(expires_at),
        tx_hash: Some(tos_common::crypto::Hash::zero()),
        amount_received: Some(amount),
        confirmed_at_topoheight: Some(80),
    };

    // With current_topo=100 and stable_topo=90, payment at topo 80 should be confirmed
    let status = stored.get_status(100, 90);
    assert_eq!(status, PaymentStatus::Confirmed);
}

#[test]
fn test_e2e_underpaid_flow() {
    let payment_id = "order_underpaid";
    let expected_amount = 1_000_000_000u64; // 1 TOS expected
    let received_amount = 500_000_000u64; // Only 0.5 TOS received

    let stored = StoredPaymentRequest {
        payment_id: payment_id.to_string(),
        address: test_address(),
        amount: Some(expected_amount),
        asset: None,
        memo: None,
        created_at: current_timestamp(),
        expires_at: None,
        tx_hash: Some(tos_common::crypto::Hash::zero()),
        amount_received: Some(received_amount),
        confirmed_at_topoheight: Some(80),
    };

    let status = stored.get_status(100, 90);
    assert_eq!(status, PaymentStatus::Underpaid);
}

#[test]
fn test_e2e_expired_before_payment() {
    let payment_id = "order_expired";

    let stored = StoredPaymentRequest {
        payment_id: payment_id.to_string(),
        address: test_address(),
        amount: Some(1_000_000_000),
        asset: None,
        memo: None,
        created_at: current_timestamp() - 600,
        expires_at: Some(current_timestamp() - 300), // Expired
        tx_hash: None,                               // No payment received
        amount_received: None,
        confirmed_at_topoheight: None,
    };

    let status = stored.get_status(100, 90);
    assert_eq!(status, PaymentStatus::Expired);
}

// ============================================================================
// Confirmations Calculation Tests
// ============================================================================

#[test]
fn test_confirmations_calculation() {
    // confirmations = current_topoheight - block_topoheight + 1

    // Block at topo 95, current at 100: 100 - 95 + 1 = 6 confirmations
    let stored = StoredPaymentRequest {
        payment_id: "conf-test".to_string(),
        address: test_address(),
        amount: None,
        asset: None,
        memo: None,
        created_at: current_timestamp(),
        expires_at: None,
        tx_hash: Some(tos_common::crypto::Hash::zero()),
        amount_received: Some(1_000_000_000),
        confirmed_at_topoheight: Some(95),
    };

    // 6 confirmations < 8 (STABLE_CONFIRMATIONS), so should be "confirming"
    let status = stored.get_status(100, 90);
    assert_eq!(status, PaymentStatus::Confirming);

    // Block at topo 92, current at 100: 100 - 92 + 1 = 9 confirmations
    let stored2 = StoredPaymentRequest {
        confirmed_at_topoheight: Some(92),
        ..stored.clone()
    };

    // 9 confirmations >= 8, but stable_topo=90, so 92 > 90 means not yet stable
    // Status depends on stable_topoheight comparison
    let status2 = stored2.get_status(100, 90);
    assert_eq!(status2, PaymentStatus::Confirming);

    // Block at topo 88, current at 100, stable at 90
    // 88 <= 90 means it's in stable range
    let stored3 = StoredPaymentRequest {
        confirmed_at_topoheight: Some(88),
        ..stored
    };

    let status3 = stored3.get_status(100, 90);
    assert_eq!(status3, PaymentStatus::Confirmed);
}
