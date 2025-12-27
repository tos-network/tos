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

use tos_common::api::callback::{
    RegisterWebhookParams, RegisterWebhookResult, UnregisterWebhookParams, UnregisterWebhookResult,
};
use tos_common::api::payment::{
    decode_payment_extra_data, encode_payment_extra_data, is_valid_payment_id, matches_payment_id,
    validate_payment_id, GetPaymentStatusParams, PaymentIdError, PaymentParseError, PaymentRequest,
    PaymentStatus, PaymentStatusResponse, StoredPaymentRequest,
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

#[test]
fn test_memo_with_emoji() {
    let addr = test_address();
    // Test various emoji characters (4-byte UTF-8)
    let emoji_memo = "Payment 😀🎉💰 complete!";

    let request = PaymentRequest::new("emoji-test", addr.clone()).with_memo(emoji_memo);

    // Emoji memo should be preserved
    let memo = request.memo.as_deref().unwrap();
    assert!(memo.contains("😀"));
    assert!(memo.contains("🎉"));
    assert!(memo.contains("💰"));

    // URI roundtrip should preserve emoji
    let uri = request.to_uri();
    let parsed = PaymentRequest::from_uri(&uri).unwrap();
    assert_eq!(parsed.memo.as_deref(), Some(emoji_memo));

    // Extra data encoding should preserve emoji
    let encoded = encode_payment_extra_data("emoji-test", Some(emoji_memo)).unwrap();
    let (decoded_id, decoded_memo) = decode_payment_extra_data(&encoded).unwrap();
    assert_eq!(decoded_id, "emoji-test");
    assert_eq!(decoded_memo.as_deref(), Some(emoji_memo));
}

#[test]
fn test_memo_emoji_truncation_preserves_valid_utf8() {
    let addr = test_address();
    // Create a long memo with emoji that needs truncation
    // Each emoji is 4 bytes, so this will test truncation at emoji boundaries
    let long_emoji_memo = "🚀".repeat(20); // 80 bytes of emoji

    let request = PaymentRequest::new("emoji-trunc", addr).with_memo(&long_emoji_memo);

    let memo = request.memo.unwrap();
    // Should be truncated to <= 64 bytes
    assert!(memo.len() <= 64);
    // Should still be valid UTF-8 (no partial emoji)
    assert!(std::str::from_utf8(memo.as_bytes()).is_ok());
    // Should contain complete emoji only (each emoji is 4 bytes)
    assert!(memo.len() % 4 == 0 || memo.is_empty());
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
        callback_url: None,
        last_callback_status: None,
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
        callback_url: None,
        last_callback_status: None,
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
        callback_url: None,
        last_callback_status: None,
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
        callback_url: None,
        last_callback_status: None,
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
        callback_url: None,
        last_callback_status: None,
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
// Webhook Registration RPC Types Tests
// ============================================================================

#[test]
fn test_register_webhook_params_serialization() {
    let params = RegisterWebhookParams {
        url: "https://merchant.example.com/webhook".to_string(),
        secret_hex: "0123abcd".to_string(),
    };

    let json = serde_json::to_string(&params).unwrap();
    assert!(json.contains("https://merchant.example.com/webhook"));
    assert!(json.contains("secret_hex"));
}

#[test]
fn test_register_webhook_result_serialization() {
    let result = RegisterWebhookResult { success: true };
    let json = serde_json::to_string(&result).unwrap();
    assert!(json.contains("true"));
}

#[test]
fn test_unregister_webhook_params_serialization() {
    let params = UnregisterWebhookParams {
        url: "https://merchant.example.com/webhook".to_string(),
    };

    let json = serde_json::to_string(&params).unwrap();
    assert!(json.contains("https://merchant.example.com/webhook"));
}

#[test]
fn test_unregister_webhook_result_serialization() {
    let result = UnregisterWebhookResult { success: true };
    let json = serde_json::to_string(&result).unwrap();
    assert!(json.contains("true"));
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
        callback_url: None,
        last_callback_status: None,
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
        callback_url: None,
        last_callback_status: None,
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
        callback_url: None,
        last_callback_status: None,
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
        callback_url: None,
        last_callback_status: None,
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

// ============================================================================
// Mempool State Tests
// ============================================================================

#[test]
fn test_stored_payment_status_mempool() {
    // Test explicit mempool state (tx seen but 0 confirmations)
    // This occurs when confirmed_at_topoheight == current_topoheight
    // (transaction just arrived in current block, confirmations = 0)
    let stored = StoredPaymentRequest {
        payment_id: "mempool-test".to_string(),
        address: test_address(),
        amount: Some(1_000_000_000),
        asset: None,
        memo: None,
        callback_url: None,
        last_callback_status: None,
        created_at: current_timestamp(),
        expires_at: None,
        tx_hash: Some(tos_common::crypto::Hash::zero()),
        amount_received: Some(1_000_000_000),
        // Transaction at topoheight 101, current is 100
        // This simulates tx in mempool (future block height)
        confirmed_at_topoheight: Some(101),
    };

    // current=100, stable=90, confirmed_at=101
    // confirmations = 100 - 101 + 1 = 0 (would be negative, clamped to 0)
    // Since confirmations < 1, should be Mempool
    let status = stored.get_status(100, 90);
    assert_eq!(status, PaymentStatus::Mempool);
}

#[test]
fn test_mempool_to_confirming_transition() {
    // Test state transition from mempool to confirming
    let base_stored = StoredPaymentRequest {
        payment_id: "transition-test".to_string(),
        address: test_address(),
        amount: Some(1_000_000_000),
        asset: None,
        memo: None,
        callback_url: None,
        last_callback_status: None,
        created_at: current_timestamp(),
        expires_at: None,
        tx_hash: Some(tos_common::crypto::Hash::zero()),
        amount_received: Some(1_000_000_000),
        confirmed_at_topoheight: Some(100),
    };

    // At topoheight 100, tx is at 100: confirmations = 1
    let status_at_100 = base_stored.get_status(100, 90);
    assert_eq!(status_at_100, PaymentStatus::Confirming);

    // At topoheight 101, tx is at 100: confirmations = 2
    let status_at_101 = base_stored.get_status(101, 90);
    assert_eq!(status_at_101, PaymentStatus::Confirming);

    // At topoheight 107, tx is at 100: confirmations = 8
    // Still confirming because confirmed_at(100) > stable(90)
    let status_at_107 = base_stored.get_status(107, 90);
    assert_eq!(status_at_107, PaymentStatus::Confirming);

    // When stable catches up to 100, it becomes confirmed
    let status_stable = base_stored.get_status(107, 100);
    assert_eq!(status_stable, PaymentStatus::Confirmed);
}

#[test]
fn test_state_machine_monotonicity() {
    // Verify state machine is monotonic (states don't regress)
    // Valid transitions: Pending -> Mempool -> Confirming -> Confirmed
    //                    Pending -> Expired (if no payment before expiry)

    let stored = StoredPaymentRequest {
        payment_id: "monotonic-test".to_string(),
        address: test_address(),
        amount: Some(1_000_000_000),
        asset: None,
        memo: None,
        callback_url: None,
        last_callback_status: None,
        created_at: current_timestamp(),
        expires_at: None,
        tx_hash: Some(tos_common::crypto::Hash::zero()),
        amount_received: Some(1_000_000_000),
        confirmed_at_topoheight: Some(50),
    };

    // Once confirmed at topo 50, it should stay confirmed
    // regardless of current/stable height changes
    let status1 = stored.get_status(100, 60);
    assert_eq!(status1, PaymentStatus::Confirmed);

    // Even with higher topoheight, still confirmed
    let status2 = stored.get_status(200, 150);
    assert_eq!(status2, PaymentStatus::Confirmed);

    // Confirmed should never go back to Confirming
    // (stable_topoheight always increases, so once confirmed_at <= stable, it stays that way)
}

// ============================================================================
// Overpay Handling Tests
// ============================================================================

#[test]
fn test_overpay_is_accepted_as_confirmed() {
    // Overpay scenario: customer pays more than requested
    // Expected behavior: Payment is accepted and marked as Confirmed
    // (No separate "Overpaid" status - excess is treated as a tip/donation)
    let requested_amount = 1_000_000_000u64; // 1 TOS
    let received_amount = 2_000_000_000u64; // 2 TOS (overpaid by 1 TOS)

    let stored = StoredPaymentRequest {
        payment_id: "overpay-test".to_string(),
        address: test_address(),
        amount: Some(requested_amount),
        asset: None,
        memo: None,
        callback_url: None,
        last_callback_status: None,
        created_at: current_timestamp(),
        expires_at: None,
        tx_hash: Some(tos_common::crypto::Hash::zero()),
        amount_received: Some(received_amount),
        confirmed_at_topoheight: Some(80), // Below stable
    };

    // current=100, stable=90, confirmed_at=80 (below stable)
    // Overpayment should result in Confirmed status (not Underpaid)
    let status = stored.get_status(100, 90);
    assert_eq!(status, PaymentStatus::Confirmed);

    // Verify the amount received is indeed greater than requested
    assert!(received_amount > requested_amount);
}

#[test]
fn test_exact_payment_is_confirmed() {
    // Exact payment: customer pays exactly the requested amount
    let amount = 1_000_000_000u64; // 1 TOS

    let stored = StoredPaymentRequest {
        payment_id: "exact-pay-test".to_string(),
        address: test_address(),
        amount: Some(amount),
        asset: None,
        memo: None,
        callback_url: None,
        last_callback_status: None,
        created_at: current_timestamp(),
        expires_at: None,
        tx_hash: Some(tos_common::crypto::Hash::zero()),
        amount_received: Some(amount), // Exact match
        confirmed_at_topoheight: Some(80),
    };

    let status = stored.get_status(100, 90);
    assert_eq!(status, PaymentStatus::Confirmed);
}

#[test]
fn test_open_amount_payment_any_amount_accepted() {
    // Open amount request (no amount specified)
    // Any amount should be accepted
    let stored = StoredPaymentRequest {
        payment_id: "open-amount-test".to_string(),
        address: test_address(),
        amount: None, // Open amount - no specific amount requested
        asset: None,
        memo: None,
        callback_url: None,
        last_callback_status: None,
        created_at: current_timestamp(),
        expires_at: None,
        tx_hash: Some(tos_common::crypto::Hash::zero()),
        amount_received: Some(123_456_789), // Any amount
        confirmed_at_topoheight: Some(80),
    };

    let status = stored.get_status(100, 90);
    assert_eq!(status, PaymentStatus::Confirmed);
}

// ============================================================================
// Duplicate Payment ID Scenario Tests
// ============================================================================

#[test]
fn test_duplicate_payment_id_different_addresses() {
    // Scenario: Same payment_id used for different addresses
    // This should be allowed since (payment_id, address) is the unique key

    let addr1 = test_address();
    let addr2: Address = "tst1yp0hc5z0csf2jk2ze9tjjxkjg8gawt2upltksyegffmudm29z38qqrkvqzk"
        .parse()
        .unwrap();

    let payment_id = "shared-order-001";

    // Create two payment requests with same ID but different addresses
    let request1 = PaymentRequest::new(payment_id, addr1.clone());
    let request2 = PaymentRequest::new(payment_id, addr2.clone());

    // Both should be valid independently
    assert_eq!(request1.payment_id.as_ref(), payment_id);
    assert_eq!(request2.payment_id.as_ref(), payment_id);
    assert_ne!(request1.address, request2.address);

    // URI roundtrip should work for both
    let uri1 = request1.to_uri();
    let uri2 = request2.to_uri();

    let parsed1 = PaymentRequest::from_uri(&uri1).unwrap();
    let parsed2 = PaymentRequest::from_uri(&uri2).unwrap();

    assert_eq!(parsed1.payment_id.as_ref(), payment_id);
    assert_eq!(parsed2.payment_id.as_ref(), payment_id);
    assert_eq!(parsed1.address, addr1);
    assert_eq!(parsed2.address, addr2);
}

#[test]
fn test_duplicate_payment_id_same_address_overwrite_behavior() {
    // Scenario: Same payment_id AND same address - later payment overwrites earlier
    // This tests the storage behavior when a duplicate key is used

    let addr = test_address();
    let payment_id = "order-duplicate";

    // First payment request
    let stored1 = StoredPaymentRequest {
        payment_id: payment_id.to_string(),
        address: addr.clone(),
        amount: Some(1_000_000_000), // 1 TOS
        asset: None,
        memo: Some("First order".to_string()),
        callback_url: None,
        last_callback_status: None,
        created_at: current_timestamp() - 60, // Created 1 minute ago
        expires_at: None,
        tx_hash: None,
        amount_received: None,
        confirmed_at_topoheight: None,
    };

    // Second payment request with same ID and address (should overwrite)
    let stored2 = StoredPaymentRequest {
        payment_id: payment_id.to_string(),
        address: addr.clone(),
        amount: Some(2_000_000_000), // 2 TOS (different amount)
        asset: None,
        memo: Some("Second order".to_string()),
        callback_url: None,
        last_callback_status: None,
        created_at: current_timestamp(), // Created now
        expires_at: None,
        tx_hash: None,
        amount_received: None,
        confirmed_at_topoheight: None,
    };

    // Both have same payment_id
    assert_eq!(stored1.payment_id, stored2.payment_id);
    assert_eq!(stored1.address, stored2.address);

    // But different amounts - in actual storage, the later one would overwrite
    assert_ne!(stored1.amount, stored2.amount);
    assert_ne!(stored1.memo, stored2.memo);

    // Verify both are valid and can be used
    assert_eq!(stored1.get_status(100, 90), PaymentStatus::Pending);
    assert_eq!(stored2.get_status(100, 90), PaymentStatus::Pending);
}

#[test]
fn test_extra_data_with_same_payment_id_matches_correctly() {
    // Test that extra_data matching works correctly for duplicate IDs
    let payment_id = "duplicate-extra-data";

    let encoded = encode_payment_extra_data(payment_id, Some("Test memo")).unwrap();

    // Should match the payment ID
    assert!(matches_payment_id(&encoded, payment_id));

    // Should not match a different ID
    assert!(!matches_payment_id(&encoded, "different-id"));

    // Case-sensitive matching
    assert!(!matches_payment_id(&encoded, "DUPLICATE-EXTRA-DATA"));
}
