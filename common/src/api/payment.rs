// QR Code Payment Request API types
// See TIPs/TIP-QR-PAYMENT.md for the full specification

use serde::{Deserialize, Serialize};
use std::borrow::Cow;

use crate::crypto::{Address, Hash};

/// Payment request status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaymentStatus {
    /// Waiting for payment
    Pending,
    /// Transaction is in mempool (0 confirmations)
    Mempool,
    /// Transaction in block but < STABLE_LIMIT confirmations
    Confirming,
    /// Transaction has >= STABLE_LIMIT confirmations (stable)
    Confirmed,
    /// Payment request has expired
    Expired,
    /// Amount received is less than requested
    Underpaid,
}

/// Payment request for QR code generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentRequest<'a> {
    /// Unique payment request ID
    pub payment_id: Cow<'a, str>,
    /// Receiving address
    pub address: Address,
    /// Requested amount in atomic units (optional for open amount)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<u64>,
    /// Asset hash (None = native TOS)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset: Option<Cow<'a, Hash>>,
    /// Payment memo/description (max 64 bytes)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memo: Option<Cow<'a, str>>,
    /// Expiration timestamp (Unix seconds)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    /// Callback URL for payment notification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callback: Option<Cow<'a, str>>,
}

impl<'a> PaymentRequest<'a> {
    /// Maximum memo length in bytes
    pub const MAX_MEMO_LENGTH: usize = 64;

    /// Maximum payment ID length
    pub const MAX_PAYMENT_ID_LENGTH: usize = 32;

    /// Create a new payment request
    pub fn new(payment_id: impl Into<Cow<'a, str>>, address: Address) -> Self {
        Self {
            payment_id: payment_id.into(),
            address,
            amount: None,
            asset: None,
            memo: None,
            expires_at: None,
            callback: None,
        }
    }

    /// Set the requested amount
    pub fn with_amount(mut self, amount: u64) -> Self {
        self.amount = Some(amount);
        self
    }

    /// Set the asset (None = native TOS)
    pub fn with_asset(mut self, asset: Hash) -> Self {
        self.asset = Some(Cow::Owned(asset));
        self
    }

    /// Set the payment memo
    /// Truncates to MAX_MEMO_LENGTH bytes at a valid UTF-8 boundary
    pub fn with_memo(mut self, memo: impl Into<Cow<'a, str>>) -> Self {
        let memo: Cow<'a, str> = memo.into();
        // Truncate to max length (bytes), respecting UTF-8 boundaries
        if memo.len() > Self::MAX_MEMO_LENGTH {
            // Find the last valid UTF-8 char boundary within limit
            let truncated = truncate_to_byte_boundary(&memo, Self::MAX_MEMO_LENGTH);
            self.memo = Some(Cow::Owned(truncated.to_string()));
        } else {
            self.memo = Some(memo);
        }
        self
    }

    /// Set the expiration timestamp
    pub fn with_expires_at(mut self, expires_at: u64) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    /// Set expiration from duration (seconds from now)
    pub fn with_expires_in(mut self, seconds: u64) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.expires_at = Some(now + seconds);
        self
    }

    /// Set the callback URL
    pub fn with_callback(mut self, callback: impl Into<Cow<'a, str>>) -> Self {
        self.callback = Some(callback.into());
        self
    }

    /// Check if the payment request has expired
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            now > expires_at
        } else {
            false
        }
    }

    /// Convert to payment URI for QR code
    pub fn to_uri(&self) -> String {
        let mut uri = format!("tos://pay?to={}", self.address);

        if let Some(amount) = self.amount {
            uri.push_str(&format!("&amount={}", amount));
        }

        if let Some(asset) = &self.asset {
            uri.push_str(&format!("&asset={}", asset));
        }

        if let Some(memo) = &self.memo {
            // URL encode the memo
            let encoded = urlencoding::encode(memo);
            uri.push_str(&format!("&memo={}", encoded));
        }

        uri.push_str(&format!("&id={}", self.payment_id));

        if let Some(expires_at) = self.expires_at {
            uri.push_str(&format!("&exp={}", expires_at));
        }

        if let Some(callback) = &self.callback {
            let encoded = urlencoding::encode(callback);
            uri.push_str(&format!("&callback={}", encoded));
        }

        uri
    }

    /// Parse a payment URI
    pub fn from_uri(uri: &str) -> Result<PaymentRequest<'static>, PaymentParseError> {
        // Remove scheme prefix
        let query = uri
            .strip_prefix("tos://pay?")
            .or_else(|| uri.strip_prefix("tos:pay?"))
            .ok_or(PaymentParseError::InvalidScheme)?;

        let mut address: Option<Address> = None;
        let mut amount: Option<u64> = None;
        let mut asset: Option<Hash> = None;
        let mut memo: Option<String> = None;
        let mut payment_id: Option<String> = None;
        let mut expires_at: Option<u64> = None;
        let mut callback: Option<String> = None;

        for pair in query.split('&') {
            let mut parts = pair.splitn(2, '=');
            let key = parts.next().ok_or(PaymentParseError::InvalidFormat)?;
            let value = parts.next().ok_or(PaymentParseError::InvalidFormat)?;

            match key {
                "to" => {
                    address = Some(
                        value
                            .parse()
                            .map_err(|_| PaymentParseError::InvalidAddress)?,
                    );
                }
                "amount" => {
                    amount = Some(
                        value
                            .parse()
                            .map_err(|_| PaymentParseError::InvalidAmount)?,
                    );
                }
                "asset" => {
                    asset = Some(value.parse().map_err(|_| PaymentParseError::InvalidAsset)?);
                }
                "memo" => {
                    memo = Some(
                        urlencoding::decode(value)
                            .map_err(|_| PaymentParseError::InvalidMemo)?
                            .into_owned(),
                    );
                }
                "id" => {
                    validate_payment_id(value).map_err(PaymentParseError::InvalidPaymentId)?;
                    payment_id = Some(value.to_string());
                }
                "exp" => {
                    expires_at = Some(
                        value
                            .parse()
                            .map_err(|_| PaymentParseError::InvalidExpiration)?,
                    );
                }
                "callback" => {
                    callback = Some(
                        urlencoding::decode(value)
                            .map_err(|_| PaymentParseError::InvalidCallback)?
                            .into_owned(),
                    );
                }
                _ => {
                    // Ignore unknown parameters for forward compatibility
                }
            }
        }

        let address = address.ok_or(PaymentParseError::MissingAddress)?;
        let payment_id = payment_id.unwrap_or_else(generate_payment_id);

        Ok(PaymentRequest {
            payment_id: Cow::Owned(payment_id),
            address,
            amount,
            asset: asset.map(Cow::Owned),
            memo: memo.map(Cow::Owned),
            expires_at,
            callback: callback.map(Cow::Owned),
        })
    }
}

/// Error when parsing payment URI
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PaymentParseError {
    #[error("Invalid URI scheme, expected 'tos://pay?'")]
    InvalidScheme,
    #[error("Invalid URI format")]
    InvalidFormat,
    #[error("Missing required 'to' address")]
    MissingAddress,
    #[error("Invalid address format")]
    InvalidAddress,
    #[error("Invalid amount value")]
    InvalidAmount,
    #[error("Invalid asset hash")]
    InvalidAsset,
    #[error("Invalid payment ID: {0}")]
    InvalidPaymentId(PaymentIdError),
    #[error("Invalid memo encoding")]
    InvalidMemo,
    #[error("Invalid expiration value")]
    InvalidExpiration,
    #[error("Invalid callback URL")]
    InvalidCallback,
}

/// Payment status response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentStatusResponse<'a> {
    /// Payment request ID
    pub payment_id: Cow<'a, str>,
    /// Current status
    pub status: PaymentStatus,
    /// Transaction hash (if payment received)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<Cow<'a, Hash>>,
    /// Amount received (in atomic units)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount_received: Option<u64>,
    /// Number of confirmations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmations: Option<u64>,
    /// Timestamp when confirmed (Unix seconds)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmed_at: Option<u64>,
}

// RPC Request/Response types

/// Request to create a payment request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePaymentRequestParams {
    /// Receiving address
    pub address: Address,
    /// Requested amount (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<u64>,
    /// Asset hash (optional, default = TOS)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset: Option<Hash>,
    /// Payment memo
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memo: Option<String>,
    /// Expiration in seconds from now
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_in_seconds: Option<u64>,
}

/// Response from create_payment_request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePaymentRequestResult {
    /// Payment ID
    pub payment_id: String,
    /// Full payment URI
    pub uri: String,
    /// QR code data (same as URI for now)
    pub qr_data: String,
    /// Expiration timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
}

/// Request to get payment status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetPaymentStatusParams {
    /// Payment request ID to search for
    pub payment_id: String,
    /// Receiving address to filter transactions
    pub address: Address,
    /// Expected payment amount (for underpaid detection)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_amount: Option<u64>,
    /// Expiration timestamp (Unix seconds) for request; enables `expired` status
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<u64>,
    /// Minimum topoheight to start searching from (optional, default: current - 1200)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_topoheight: Option<u64>,
}

/// Request to parse a payment URI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsePaymentRequestParams {
    /// Payment URI to parse
    pub uri: String,
}

/// Response from parse_payment_request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsePaymentRequestResult {
    /// Receiving address
    pub address: Address,
    /// Requested amount (if specified)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<u64>,
    /// Asset (if specified)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset: Option<Hash>,
    /// Memo
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memo: Option<String>,
    /// Payment ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payment_id: Option<String>,
    /// Expiration timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    /// Whether the request is expired
    pub is_expired: bool,
}

/// Request to pay a payment request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayRequestParams {
    /// Payment URI
    pub uri: String,
    /// Override amount (if different from URI)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<u64>,
}

/// Response from pay_request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayRequestResult {
    /// Transaction hash
    pub tx_hash: Hash,
    /// Amount paid
    pub amount: u64,
    /// Fee paid
    pub fee: u64,
    /// Payment ID from the request
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payment_id: Option<String>,
}

/// Generate a random payment ID
fn generate_payment_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Simple ID: pr_<timestamp>_<random>
    let random: u32 = rand::random();
    format!("pr_{:x}_{:08x}", timestamp, random)
}

/// Truncate a string to a maximum number of bytes, respecting UTF-8 boundaries
fn truncate_to_byte_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }

    // Find the last valid char boundary at or before max_bytes
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

// ============================================================================
// Payment Extra Data Format
// ============================================================================
// Extra data format for embedding payment metadata in transactions:
// - Byte 0:     Type (0x01 = payment)
// - Bytes 1-32: Payment ID (32 bytes, zero-padded)
// - Bytes 33+:  Memo (UTF-8, remaining bytes up to MAX_EXTRA_DATA)
// ============================================================================

/// Payment extra data type identifier
pub const PAYMENT_EXTRA_DATA_TYPE: u8 = 0x01;

/// Maximum size of extra data in a transfer (matches protocol limit)
pub const MAX_EXTRA_DATA_SIZE: usize = 128;

/// Maximum payment ID size in extra data (allows room for memo)
pub const MAX_PAYMENT_ID_BYTES: usize = 32;

/// Encode payment metadata into extra_data bytes for a transaction
///
/// Format:
/// - Byte 0: Type (0x01 = payment)
/// - Bytes 1-32: Payment ID (32 bytes, zero-padded)
/// - Bytes 33+: Memo (UTF-8, optional)
///
/// Returns None if the data would exceed MAX_EXTRA_DATA_SIZE
pub fn encode_payment_extra_data(payment_id: &str, memo: Option<&str>) -> Option<Vec<u8>> {
    if !is_valid_payment_id(payment_id) {
        return None;
    }
    let mut data = Vec::with_capacity(MAX_EXTRA_DATA_SIZE);

    // Type byte
    data.push(PAYMENT_EXTRA_DATA_TYPE);

    // Payment ID (32 bytes, zero-padded)
    let id_bytes = payment_id.as_bytes();
    let id_len = id_bytes.len().min(MAX_PAYMENT_ID_BYTES);
    data.extend_from_slice(&id_bytes[..id_len]);
    // Zero-pad to 32 bytes
    data.resize(1 + MAX_PAYMENT_ID_BYTES, 0);

    // Memo (optional, remaining bytes)
    if let Some(m) = memo {
        let memo_bytes = m.as_bytes();
        let available_space = MAX_EXTRA_DATA_SIZE - data.len();
        let memo_len = memo_bytes.len().min(available_space);
        data.extend_from_slice(&memo_bytes[..memo_len]);
    }

    if data.len() > MAX_EXTRA_DATA_SIZE {
        None
    } else {
        Some(data)
    }
}

/// Decode payment metadata from extra_data bytes
///
/// Returns (payment_id, memo) if the data is valid payment extra data
pub fn decode_payment_extra_data(data: &[u8]) -> Option<(String, Option<String>)> {
    // Minimum size: type byte + at least 1 byte of payment ID
    if data.len() < 2 {
        return None;
    }

    // Check type byte
    if data[0] != PAYMENT_EXTRA_DATA_TYPE {
        return None;
    }

    // Extract payment ID (bytes 1-32, trimmed of trailing zeros)
    let id_end = (1 + MAX_PAYMENT_ID_BYTES).min(data.len());
    let id_bytes = &data[1..id_end];

    // Find actual end (before trailing zeros)
    let actual_len = id_bytes
        .iter()
        .rposition(|&b| b != 0)
        .map(|pos| pos + 1)
        .unwrap_or(0);

    let payment_id = String::from_utf8(id_bytes[..actual_len].to_vec()).ok()?;
    if !is_valid_payment_id(&payment_id) {
        return None;
    }

    // Extract memo if present
    let memo = if data.len() > 1 + MAX_PAYMENT_ID_BYTES {
        let memo_bytes = &data[1 + MAX_PAYMENT_ID_BYTES..];
        String::from_utf8(memo_bytes.to_vec()).ok()
    } else {
        None
    };

    Some((payment_id, memo))
}

/// Check if extra_data contains a payment with the given ID
pub fn matches_payment_id(extra_data: &[u8], target_id: &str) -> bool {
    if let Some((id, _)) = decode_payment_extra_data(extra_data) {
        id == target_id
    } else {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PaymentIdError {
    #[error("empty")]
    Empty,
    #[error("length exceeds 32 bytes")]
    TooLong,
    #[error("contains invalid characters (allowed: A-Z a-z 0-9 - _)")]
    InvalidChars,
}

pub fn validate_payment_id(payment_id: &str) -> Result<(), PaymentIdError> {
    if payment_id.is_empty() {
        return Err(PaymentIdError::Empty);
    }
    if payment_id.len() > MAX_PAYMENT_ID_BYTES {
        return Err(PaymentIdError::TooLong);
    }
    if !payment_id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(PaymentIdError::InvalidChars);
    }
    Ok(())
}

pub fn is_valid_payment_id(payment_id: &str) -> bool {
    validate_payment_id(payment_id).is_ok()
}

// ============================================================================
// Stored Payment Request (for daemon storage)
// ============================================================================

/// A stored payment request for tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredPaymentRequest {
    /// Payment ID
    pub payment_id: String,
    /// Receiving address
    pub address: Address,
    /// Requested amount (optional)
    pub amount: Option<u64>,
    /// Asset hash (None = TOS)
    pub asset: Option<Hash>,
    /// Memo
    pub memo: Option<String>,
    /// Creation timestamp
    pub created_at: u64,
    /// Expiration timestamp
    pub expires_at: Option<u64>,
    /// Associated transaction hash (if paid)
    pub tx_hash: Option<Hash>,
    /// Amount received
    pub amount_received: Option<u64>,
    /// Block topoheight where payment was confirmed
    pub confirmed_at_topoheight: Option<u64>,
}

impl StoredPaymentRequest {
    /// Create from a PaymentRequest
    pub fn from_request(request: &PaymentRequest<'_>) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        Self {
            payment_id: request.payment_id.to_string(),
            address: request.address.clone(),
            amount: request.amount,
            asset: request.asset.as_ref().map(|a| a.as_ref().clone()),
            memo: request.memo.as_ref().map(|m| m.to_string()),
            created_at: now,
            expires_at: request.expires_at,
            tx_hash: None,
            amount_received: None,
            confirmed_at_topoheight: None,
        }
    }

    /// Check if this payment request has expired
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            now > expires_at
        } else {
            false
        }
    }

    /// Check if this payment has been fulfilled
    pub fn is_paid(&self) -> bool {
        self.tx_hash.is_some()
    }

    /// Get the current status
    pub fn get_status(&self, current_topoheight: u64, stable_topoheight: u64) -> PaymentStatus {
        if self.is_expired() && !self.is_paid() {
            return PaymentStatus::Expired;
        }

        if let Some(confirmed_topo) = self.confirmed_at_topoheight {
            let confirmations = if current_topoheight >= confirmed_topo {
                current_topoheight - confirmed_topo + 1
            } else {
                0
            };

            // Check for underpayment
            if let (Some(requested), Some(received)) = (self.amount, self.amount_received) {
                if received < requested {
                    return PaymentStatus::Underpaid;
                }
            }

            if confirmed_topo <= stable_topoheight {
                return PaymentStatus::Confirmed;
            } else if confirmations >= 1 {
                return PaymentStatus::Confirming;
            } else {
                return PaymentStatus::Mempool;
            }
        }

        PaymentStatus::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_payment_request_to_uri() {
        let addr: Address = "tst12zacnuun3lkv5kxzn2jy8l28d0zft7rqhyxlz2v6h6u23xmruy7sqm0d38u"
            .parse()
            .unwrap();

        let request = PaymentRequest::new("order-123", addr.clone())
            .with_amount(1_000_000_000)
            .with_memo("Coffee")
            .with_expires_at(1734567890);

        let uri = request.to_uri();
        assert!(uri.starts_with("tos://pay?to="));
        assert!(uri.contains("amount=1000000000"));
        assert!(uri.contains("memo=Coffee"));
        assert!(uri.contains("id=order-123"));
        assert!(uri.contains("exp=1734567890"));
    }

    #[test]
    fn test_payment_request_from_uri() {
        let uri = "tos://pay?to=tst12zacnuun3lkv5kxzn2jy8l28d0zft7rqhyxlz2v6h6u23xmruy7sqm0d38u&amount=1000000000&memo=Coffee&id=order-123";

        let request = PaymentRequest::from_uri(uri).unwrap();
        assert_eq!(request.amount, Some(1_000_000_000));
        assert_eq!(request.memo.as_deref(), Some("Coffee"));
        assert_eq!(request.payment_id.as_ref(), "order-123");
    }

    #[test]
    fn test_payment_request_roundtrip() {
        let addr: Address = "tst12zacnuun3lkv5kxzn2jy8l28d0zft7rqhyxlz2v6h6u23xmruy7sqm0d38u"
            .parse()
            .unwrap();

        let original = PaymentRequest::new("test-123", addr)
            .with_amount(5_000_000_000)
            .with_memo("Test Payment with spaces")
            .with_expires_at(1700000000);

        let uri = original.to_uri();
        let parsed = PaymentRequest::from_uri(&uri).unwrap();

        assert_eq!(original.amount, parsed.amount);
        assert_eq!(original.memo.as_deref(), parsed.memo.as_deref());
        assert_eq!(original.payment_id.as_ref(), parsed.payment_id.as_ref());
        assert_eq!(original.expires_at, parsed.expires_at);
    }

    #[test]
    fn test_payment_expiration() {
        let addr: Address = "tst12zacnuun3lkv5kxzn2jy8l28d0zft7rqhyxlz2v6h6u23xmruy7sqm0d38u"
            .parse()
            .unwrap();

        // Expired payment
        let expired = PaymentRequest::new("expired", addr.clone()).with_expires_at(1);
        assert!(expired.is_expired());

        // Future payment
        let future = PaymentRequest::new("future", addr).with_expires_at(u64::MAX);
        assert!(!future.is_expired());
    }

    #[test]
    fn test_invalid_uri() {
        assert!(PaymentRequest::from_uri("invalid").is_err());
        assert!(PaymentRequest::from_uri("tos://pay?amount=100").is_err()); // missing address
        assert!(PaymentRequest::from_uri("http://example.com").is_err());

        let long_id = "a".repeat(MAX_PAYMENT_ID_BYTES + 1);
        let uri = format!(
            "tos://pay?to=tst12zacnuun3lkv5kxzn2jy8l28d0zft7rqhyxlz2v6h6u23xmruy7sqm0d38u&id={}",
            long_id
        );
        assert!(matches!(
            PaymentRequest::from_uri(&uri),
            Err(PaymentParseError::InvalidPaymentId(_))
        ));
    }

    #[test]
    fn test_encode_decode_payment_extra_data() {
        let payment_id = "pr_123456_abcdef";
        let memo = Some("Coffee order");

        let encoded = encode_payment_extra_data(payment_id, memo).unwrap();

        // Check type byte
        assert_eq!(encoded[0], PAYMENT_EXTRA_DATA_TYPE);

        // Decode and verify
        let (decoded_id, decoded_memo) = decode_payment_extra_data(&encoded).unwrap();
        assert_eq!(decoded_id, payment_id);
        assert_eq!(decoded_memo.as_deref(), memo);
    }

    #[test]
    fn test_encode_decode_without_memo() {
        let payment_id = "order-789";

        let encoded = encode_payment_extra_data(payment_id, None).unwrap();
        let (decoded_id, decoded_memo) = decode_payment_extra_data(&encoded).unwrap();

        assert_eq!(decoded_id, payment_id);
        assert!(decoded_memo.is_none());
    }

    #[test]
    fn test_matches_payment_id() {
        let payment_id = "test-payment-001";
        let encoded = encode_payment_extra_data(payment_id, Some("memo")).unwrap();

        assert!(matches_payment_id(&encoded, "test-payment-001"));
        assert!(!matches_payment_id(&encoded, "wrong-id"));
    }

    #[test]
    fn test_decode_invalid_extra_data() {
        // Empty data
        assert!(decode_payment_extra_data(&[]).is_none());

        // Wrong type byte
        assert!(decode_payment_extra_data(&[0x02, 0x01, 0x02]).is_none());

        // Too short
        assert!(decode_payment_extra_data(&[PAYMENT_EXTRA_DATA_TYPE]).is_none());
    }

    #[test]
    fn test_long_payment_id_rejected() {
        // Payment ID longer than MAX_PAYMENT_ID_BYTES should be rejected
        let long_id = "a".repeat(50);
        let encoded = encode_payment_extra_data(&long_id, None);
        assert!(encoded.is_none());
    }

    #[test]
    fn test_extra_data_size_limit() {
        let payment_id = "test";
        // Very long memo that would exceed limit
        let long_memo = "x".repeat(200);

        let encoded = encode_payment_extra_data(payment_id, Some(&long_memo)).unwrap();
        // Should be truncated to MAX_EXTRA_DATA_SIZE
        assert!(encoded.len() <= MAX_EXTRA_DATA_SIZE);
    }
}
