// Callback delivery service for QR Code Payment System
// See TIPs/TIP-QR-PAYMENT.md for the full specification

use log::{debug, error, warn};
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tos_common::{
    api::callback::{
        callback_idempotency_key, generate_callback_signature, CallbackConfig, CallbackPayload,
        CallbackResult, CALLBACK_RETRY_DELAYS_MS, CALLBACK_TIMEOUT_SECONDS,
    },
    crypto::Hash,
    time::get_current_time_in_seconds,
    tokio::{spawn_task, sync::RwLock},
};

/// Callback service for delivering payment notifications to merchants
pub struct CallbackService {
    /// HTTP client for making callback requests
    client: Client,
    /// Registered webhook secrets by URL
    /// In production, this would be backed by persistent storage
    webhook_secrets: RwLock<HashMap<String, Vec<u8>>>,
    /// Idempotency keys that were successfully delivered
    delivered_keys: RwLock<HashMap<String, u64>>,
}

/// Idempotency key retention window (1 hour)
const CALLBACK_IDEMPOTENCY_TTL_SECONDS: u64 = 3600;
/// Maximum number of idempotency keys to keep before pruning
const CALLBACK_IDEMPOTENCY_MAX_KEYS: usize = 10000;

impl CallbackService {
    /// Create a new callback service
    pub fn new() -> Self {
        // Build HTTP client with timeout, fallback to default if builder fails
        let client = Client::builder()
            .timeout(Duration::from_secs(CALLBACK_TIMEOUT_SECONDS))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            client,
            webhook_secrets: RwLock::new(HashMap::new()),
            delivered_keys: RwLock::new(HashMap::new()),
        }
    }

    /// Register a webhook secret for a callback URL
    ///
    /// # Arguments
    /// * `url` - The callback URL
    /// * `secret` - The webhook secret for HMAC signature
    pub async fn register_webhook(&self, url: String, secret: Vec<u8>) {
        let mut secrets = self.webhook_secrets.write().await;
        secrets.insert(url, secret);
    }

    /// Unregister a webhook secret
    pub async fn unregister_webhook(&self, url: &str) {
        let mut secrets = self.webhook_secrets.write().await;
        secrets.remove(url);
    }

    /// Get the webhook secret for a URL
    ///
    /// Note: This enables out-of-band webhook registration to be used by callback delivery.
    pub async fn get_webhook_secret(&self, url: &str) -> Option<Vec<u8>> {
        let secrets = self.webhook_secrets.read().await;
        secrets.get(url).cloned()
    }

    /// Deliver a callback with retry logic
    ///
    /// This function spawns a background task to deliver the callback
    /// with exponential backoff retry (1s, 5s, 25s).
    pub fn deliver_callback(self: Arc<Self>, config: CallbackConfig, payload: CallbackPayload) {
        spawn_task("callback-delivery", async move {
            let result = self.deliver_with_retry(&config, &payload).await;
            match result {
                CallbackResult::Success => {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!(
                            "Callback delivered successfully to {} for payment {}",
                            config.url, payload.payment_id
                        );
                    }
                }
                CallbackResult::Failed { error, attempts } => {
                    error!(
                        "Callback delivery failed to {} for payment {} after {} attempts: {}",
                        config.url, payload.payment_id, attempts, error
                    );
                }
            }
        });
    }

    /// Deliver callback with retry logic
    ///
    /// BUG-089 FIX: Now uses config.max_retries and config.timeout_seconds
    /// instead of hardcoded CALLBACK_RETRY_DELAYS_MS and CALLBACK_TIMEOUT_SECONDS
    async fn deliver_with_retry(
        &self,
        config: &CallbackConfig,
        payload: &CallbackPayload,
    ) -> CallbackResult {
        let now = get_current_time_in_seconds();
        let payload_for_send = if payload.timestamp == 0 {
            let mut updated = payload.clone();
            updated.timestamp = now;
            updated
        } else {
            payload.clone()
        };
        let idempotency_key = callback_idempotency_key(&payload_for_send);
        {
            let delivered = self.delivered_keys.read().await;
            if delivered
                .get(&idempotency_key)
                .map(|ts| now.saturating_sub(*ts) <= CALLBACK_IDEMPOTENCY_TTL_SECONDS)
                .unwrap_or(false)
            {
                return CallbackResult::Success;
            }
        }

        let body = match serde_json::to_string(&payload_for_send) {
            Ok(b) => b,
            Err(e) => {
                return CallbackResult::Failed {
                    error: format!("Failed to serialize payload: {}", e),
                    attempts: 0,
                };
            }
        };

        let timestamp = payload_for_send.timestamp;
        let signature = generate_callback_signature(&config.secret, timestamp, &body);

        // BUG-089 FIX: Use config.max_retries instead of hardcoded array length
        let max_attempts = config.max_retries as usize;
        let mut attempts_made = 0u32;

        for attempt in 0..max_attempts {
            if attempt > 0 {
                // Use exponential backoff: 1s, 5s, 25s, 125s, ...
                // Fall back to hardcoded delays if within range, otherwise calculate
                let delay_ms = if attempt < CALLBACK_RETRY_DELAYS_MS.len() {
                    CALLBACK_RETRY_DELAYS_MS[attempt]
                } else {
                    // Exponential backoff: 1000 * 5^attempt (capped at 5 minutes)
                    let base_delay = 1000u64;
                    let multiplier = 5u64.saturating_pow(attempt as u32);
                    base_delay.saturating_mul(multiplier).min(300_000)
                };

                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "Retrying callback to {} (attempt {}/{})",
                        config.url,
                        attempt + 1,
                        max_attempts
                    );
                }
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }

            attempts_made = (attempt + 1) as u32;

            // BUG-089 FIX: Use config.timeout_seconds for request timeout
            match self
                .send_callback_with_timeout(
                    &config.url,
                    &body,
                    timestamp,
                    &signature,
                    &idempotency_key,
                    config.timeout_seconds,
                )
                .await
            {
                Ok(()) => {
                    let mut delivered = self.delivered_keys.write().await;
                    delivered.insert(idempotency_key.clone(), now);
                    if delivered.len() > CALLBACK_IDEMPOTENCY_MAX_KEYS {
                        delivered.retain(|_, ts| {
                            now.saturating_sub(*ts) <= CALLBACK_IDEMPOTENCY_TTL_SECONDS
                        });
                    }
                    return CallbackResult::Success;
                }
                Err(e) => {
                    warn!(
                        "Callback attempt {} to {} failed: {}",
                        attempt + 1,
                        config.url,
                        e
                    );
                    // Continue to next retry
                }
            }
        }

        CallbackResult::Failed {
            error: "All retry attempts exhausted".to_string(),
            attempts: attempts_made,
        }
    }

    /// Send a single callback request (using default client timeout)
    #[allow(dead_code)]
    async fn send_callback(
        &self,
        url: &str,
        body: &str,
        timestamp: u64,
        signature: &str,
        idempotency_key: &str,
    ) -> Result<(), String> {
        self.send_callback_with_timeout(
            url,
            body,
            timestamp,
            signature,
            idempotency_key,
            CALLBACK_TIMEOUT_SECONDS,
        )
        .await
    }

    /// BUG-089 FIX: Send a single callback request with configurable timeout
    async fn send_callback_with_timeout(
        &self,
        url: &str,
        body: &str,
        timestamp: u64,
        signature: &str,
        idempotency_key: &str,
        timeout_seconds: u64,
    ) -> Result<(), String> {
        // Validate URL is HTTPS (security requirement)
        if !url.starts_with("https://") {
            return Err("Callback URL must use HTTPS".to_string());
        }

        let response = self
            .client
            .post(url)
            .header("Content-Type", "application/json")
            .header("X-TOS-Signature", signature)
            .header("X-TOS-Timestamp", timestamp.to_string())
            .header("X-TOS-Idempotency", idempotency_key)
            .body(body.to_string())
            .timeout(Duration::from_secs(timeout_seconds)) // BUG-089 FIX: Use config timeout
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;

        let status = response.status();
        if status.is_success() {
            Ok(())
        } else {
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            Err(format!("HTTP {}: {}", status, error_body))
        }
    }
}

impl Default for CallbackService {
    fn default() -> Self {
        Self::new()
    }
}

/// Create a callback configuration from payment request data
pub fn create_callback_config(callback_url: String, webhook_secret: Vec<u8>) -> CallbackConfig {
    CallbackConfig::new(callback_url, webhook_secret)
}

/// Send a payment received callback
///
/// This is a helper function to send a callback when a payment is detected.
/// It should be called from the blockchain event handler when a payment
/// matches a registered payment request with a callback URL.
///
/// BUG-088 FIX: Added expected_amount parameter to detect underpaid transactions.
/// If amount < expected_amount, uses PaymentUnderpaid instead of PaymentConfirmed.
pub fn send_payment_callback(
    service: Arc<CallbackService>,
    callback_url: String,
    webhook_secret: Vec<u8>,
    payment_id: String,
    tx_hash: Hash,
    amount: u64,
    expected_amount: u64,
    confirmations: u64,
) {
    let config = create_callback_config(callback_url, webhook_secret);
    let payload = if confirmations >= 8 {
        // BUG-088 FIX: Check if payment is underpaid before marking as confirmed
        if amount < expected_amount {
            // Underpaid payment - use distinct callback type
            CallbackPayload::payment_underpaid(payment_id, tx_hash, amount, confirmations)
        } else {
            // Fully paid and confirmed
            CallbackPayload::payment_confirmed(payment_id, tx_hash, amount, confirmations)
        }
    } else {
        // Payment received but not yet stable
        CallbackPayload::payment_received(payment_id, tx_hash, amount, confirmations)
    };

    service.deliver_callback(config, payload);
}

/// Send a payment expired callback
pub fn send_payment_expired_callback(
    service: Arc<CallbackService>,
    callback_url: String,
    webhook_secret: Vec<u8>,
    payment_id: String,
) {
    let config = create_callback_config(callback_url, webhook_secret);
    let payload = CallbackPayload::payment_expired(payment_id);
    service.deliver_callback(config, payload);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_callback_service_creation() {
        let service = CallbackService::new();
        // Verify we can read the webhook secrets (should be empty initially)
        let secrets = service.webhook_secrets.read().await;
        assert!(secrets.is_empty());
    }

    #[tokio::test]
    async fn test_register_webhook() {
        let service = CallbackService::new();
        let url = "https://example.com/webhook".to_string();
        let secret = b"test_secret".to_vec();

        service.register_webhook(url.clone(), secret.clone()).await;

        let retrieved = service.get_webhook_secret(&url).await;
        assert_eq!(retrieved, Some(secret));
    }

    #[tokio::test]
    async fn test_unregister_webhook() {
        let service = CallbackService::new();
        let url = "https://example.com/webhook".to_string();
        let secret = b"test_secret".to_vec();

        service.register_webhook(url.clone(), secret).await;
        service.unregister_webhook(&url).await;

        let retrieved = service.get_webhook_secret(&url).await;
        assert_eq!(retrieved, None);
    }

    #[tokio::test]
    async fn test_register_webhook_overwrite() {
        let service = CallbackService::new();
        let url = "https://example.com/webhook".to_string();
        let secret_a = b"secret_a".to_vec();
        let secret_b = b"secret_b".to_vec();

        service.register_webhook(url.clone(), secret_a).await;
        service
            .register_webhook(url.clone(), secret_b.clone())
            .await;

        let retrieved = service.get_webhook_secret(&url).await;
        assert_eq!(retrieved, Some(secret_b));
    }

    #[tokio::test]
    async fn test_unregister_webhook_missing() {
        let service = CallbackService::new();
        let url = "https://example.com/webhook".to_string();

        service.unregister_webhook(&url).await;

        let retrieved = service.get_webhook_secret(&url).await;
        assert_eq!(retrieved, None);
    }

    #[test]
    fn test_create_callback_config() {
        let url = "https://example.com/webhook".to_string();
        let secret = b"test_secret".to_vec();

        let config = create_callback_config(url.clone(), secret.clone());

        assert_eq!(config.url, url);
        assert_eq!(config.secret, secret);
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.timeout_seconds, CALLBACK_TIMEOUT_SECONDS);
    }
}
