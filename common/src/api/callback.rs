// Callback security for QR Code Payment System
// See TIPs/TIP-QR-PAYMENT.md for the full specification

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::crypto::Hash;
use crate::time::get_current_time_in_seconds;

// HMAC-SHA256 type alias
type HmacSha256 = Hmac<Sha256>;

/// Maximum age of a callback request in seconds (5 minutes)
pub const CALLBACK_MAX_AGE_SECONDS: u64 = 300;

/// Callback request timeout in seconds
pub const CALLBACK_TIMEOUT_SECONDS: u64 = 10;

/// Retry delays in milliseconds (exponential backoff: 1s, 5s, 25s)
pub const CALLBACK_RETRY_DELAYS_MS: [u64; 3] = [1000, 5000, 25000];

/// Payment callback event types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallbackEventType {
    /// Payment received (in mempool or block)
    PaymentReceived,
    /// Payment confirmed (>= STABLE_CONFIRMATIONS)
    PaymentConfirmed,
    /// Payment expired
    PaymentExpired,
}

/// Callback request body sent to merchant webhook
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackPayload {
    /// Event type
    pub event: CallbackEventType,
    /// Payment request ID
    pub payment_id: String,
    /// Transaction hash (if payment received/confirmed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<Hash>,
    /// Amount received in atomic units
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<u64>,
    /// Number of confirmations
    pub confirmations: u64,
    /// Timestamp of the callback (Unix seconds)
    pub timestamp: u64,
}

impl CallbackPayload {
    /// Create a new payment received callback payload
    pub fn payment_received(
        payment_id: String,
        tx_hash: Hash,
        amount: u64,
        confirmations: u64,
    ) -> Self {
        Self {
            event: CallbackEventType::PaymentReceived,
            payment_id,
            tx_hash: Some(tx_hash),
            amount: Some(amount),
            confirmations,
            timestamp: get_current_time_in_seconds(),
        }
    }

    /// Create a new payment confirmed callback payload
    pub fn payment_confirmed(
        payment_id: String,
        tx_hash: Hash,
        amount: u64,
        confirmations: u64,
    ) -> Self {
        Self {
            event: CallbackEventType::PaymentConfirmed,
            payment_id,
            tx_hash: Some(tx_hash),
            amount: Some(amount),
            confirmations,
            timestamp: get_current_time_in_seconds(),
        }
    }

    /// Create a new payment expired callback payload
    pub fn payment_expired(payment_id: String) -> Self {
        Self {
            event: CallbackEventType::PaymentExpired,
            payment_id,
            tx_hash: None,
            amount: None,
            confirmations: 0,
            timestamp: get_current_time_in_seconds(),
        }
    }
}

/// Generate HMAC-SHA256 signature for callback request
///
/// Signature format (per TIP-QR-PAYMENT):
/// 1. Concatenate: timestamp + "." + request_body
/// 2. Compute HMAC-SHA256 with webhook_secret
/// 3. Encode as lowercase hex
///
/// # Arguments
/// * `webhook_secret` - The shared secret for HMAC
/// * `timestamp` - Unix timestamp in seconds
/// * `body` - JSON body of the callback request
///
/// # Returns
/// Lowercase hex-encoded HMAC-SHA256 signature
pub fn generate_callback_signature(webhook_secret: &[u8], timestamp: u64, body: &str) -> String {
    // Concatenate: timestamp + "." + body
    let payload = format!("{}.{}", timestamp, body);

    // Compute HMAC-SHA256
    let mut mac =
        HmacSha256::new_from_slice(webhook_secret).expect("HMAC can take key of any size");
    mac.update(payload.as_bytes());
    let result = mac.finalize();

    // Encode as lowercase hex
    hex::encode(result.into_bytes())
}

/// Verify HMAC-SHA256 signature for callback request
///
/// # Arguments
/// * `webhook_secret` - The shared secret for HMAC
/// * `timestamp` - Unix timestamp from X-TOS-Timestamp header
/// * `body` - JSON body of the callback request
/// * `signature` - Signature from X-TOS-Signature header
///
/// # Returns
/// true if signature is valid and timestamp is within allowed range
pub fn verify_callback_signature(
    webhook_secret: &[u8],
    timestamp: u64,
    body: &str,
    signature: &str,
) -> bool {
    // Check timestamp is within allowed range (5 minutes)
    let now = get_current_time_in_seconds();
    if now > timestamp && now - timestamp > CALLBACK_MAX_AGE_SECONDS {
        return false;
    }
    if timestamp > now && timestamp - now > CALLBACK_MAX_AGE_SECONDS {
        return false;
    }

    // Generate expected signature
    let expected = generate_callback_signature(webhook_secret, timestamp, body);

    // Constant-time comparison
    constant_time_compare(expected.as_bytes(), signature.as_bytes())
}

/// Constant-time comparison to prevent timing attacks
fn constant_time_compare(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    let mut result = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        result |= x ^ y;
    }
    result == 0
}

/// Callback delivery result
#[derive(Debug, Clone)]
pub enum CallbackResult {
    /// Callback delivered successfully
    Success,
    /// Callback failed after all retries
    Failed {
        /// Last error message
        error: String,
        /// Number of attempts made
        attempts: u32,
    },
}

/// Callback delivery configuration
#[derive(Debug, Clone)]
pub struct CallbackConfig {
    /// Webhook URL
    pub url: String,
    /// Webhook secret for HMAC signature
    pub secret: Vec<u8>,
    /// Maximum number of retry attempts (default: 3)
    pub max_retries: u32,
    /// Request timeout in seconds (default: 10)
    pub timeout_seconds: u64,
}

impl CallbackConfig {
    /// Create a new callback configuration
    pub fn new(url: String, secret: Vec<u8>) -> Self {
        Self {
            url,
            secret,
            max_retries: CALLBACK_RETRY_DELAYS_MS.len() as u32,
            timeout_seconds: CALLBACK_TIMEOUT_SECONDS,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_callback_signature() {
        let secret = b"test_webhook_secret";
        let timestamp = 1734567890u64;
        let body = r#"{"event":"payment_received","payment_id":"pr_abc123","tx_hash":"abc123","amount":1000000000,"confirmations":1,"timestamp":1734567890}"#;

        let signature = generate_callback_signature(secret, timestamp, body);

        // Signature should be 64 hex characters (32 bytes)
        assert_eq!(signature.len(), 64);
        // Should be lowercase hex
        assert!(signature.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(signature, signature.to_lowercase());
    }

    #[test]
    fn test_verify_callback_signature_valid() {
        let secret = b"test_webhook_secret";
        let timestamp = get_current_time_in_seconds();
        let body = r#"{"event":"payment_received","payment_id":"pr_abc123"}"#;

        let signature = generate_callback_signature(secret, timestamp, body);

        assert!(verify_callback_signature(
            secret, timestamp, body, &signature
        ));
    }

    #[test]
    fn test_verify_callback_signature_invalid() {
        let secret = b"test_webhook_secret";
        let timestamp = get_current_time_in_seconds();
        let body = r#"{"event":"payment_received","payment_id":"pr_abc123"}"#;

        // Wrong signature
        assert!(!verify_callback_signature(
            secret,
            timestamp,
            body,
            "invalid_signature"
        ));

        // Wrong secret
        let signature = generate_callback_signature(secret, timestamp, body);
        assert!(!verify_callback_signature(
            b"wrong_secret",
            timestamp,
            body,
            &signature
        ));

        // Wrong body
        assert!(!verify_callback_signature(
            secret,
            timestamp,
            r#"{"event":"modified"}"#,
            &signature
        ));
    }

    #[test]
    fn test_verify_callback_signature_expired() {
        let secret = b"test_webhook_secret";
        let body = r#"{"event":"payment_received"}"#;

        // Timestamp too old (6 minutes ago)
        let old_timestamp = get_current_time_in_seconds() - 360;
        let signature = generate_callback_signature(secret, old_timestamp, body);
        assert!(!verify_callback_signature(
            secret,
            old_timestamp,
            body,
            &signature
        ));

        // Timestamp in future (6 minutes ahead)
        let future_timestamp = get_current_time_in_seconds() + 360;
        let signature = generate_callback_signature(secret, future_timestamp, body);
        assert!(!verify_callback_signature(
            secret,
            future_timestamp,
            body,
            &signature
        ));
    }

    #[test]
    fn test_callback_payload_serialization() {
        let payload =
            CallbackPayload::payment_received("pr_abc123".to_string(), Hash::zero(), 1000000000, 1);

        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("payment_received"));
        assert!(json.contains("pr_abc123"));
        assert!(json.contains("1000000000"));
    }

    #[test]
    fn test_constant_time_compare() {
        assert!(constant_time_compare(b"hello", b"hello"));
        assert!(!constant_time_compare(b"hello", b"world"));
        assert!(!constant_time_compare(b"hello", b"hell"));
        assert!(!constant_time_compare(b"hello", b"helloo"));
    }
}
