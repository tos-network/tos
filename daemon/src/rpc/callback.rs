// Callback delivery service for QR Code Payment System
// See TIPs/TIP-QR-PAYMENT.md for the full specification

use log::{debug, error, warn};
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tos_common::{
    api::callback::{
        generate_callback_signature, CallbackConfig, CallbackPayload, CallbackResult,
        CALLBACK_RETRY_DELAYS_MS, CALLBACK_TIMEOUT_SECONDS,
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
}

impl CallbackService {
    /// Create a new callback service
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(CALLBACK_TIMEOUT_SECONDS))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            webhook_secrets: RwLock::new(HashMap::new()),
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
    /// Note: Currently unused as callbacks use config-provided secrets,
    /// but kept for potential future webhook management API.
    #[allow(dead_code)]
    async fn get_webhook_secret(&self, url: &str) -> Option<Vec<u8>> {
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
    async fn deliver_with_retry(
        &self,
        config: &CallbackConfig,
        payload: &CallbackPayload,
    ) -> CallbackResult {
        let body = match serde_json::to_string(payload) {
            Ok(b) => b,
            Err(e) => {
                return CallbackResult::Failed {
                    error: format!("Failed to serialize payload: {}", e),
                    attempts: 0,
                };
            }
        };

        let timestamp = get_current_time_in_seconds();
        let signature = generate_callback_signature(&config.secret, timestamp, &body);

        for (attempt, delay_ms) in CALLBACK_RETRY_DELAYS_MS.iter().enumerate() {
            if attempt > 0 {
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "Retrying callback to {} (attempt {}/{})",
                        config.url,
                        attempt + 1,
                        CALLBACK_RETRY_DELAYS_MS.len()
                    );
                }
                tokio::time::sleep(Duration::from_millis(*delay_ms)).await;
            }

            match self
                .send_callback(&config.url, &body, timestamp, &signature)
                .await
            {
                Ok(()) => return CallbackResult::Success,
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
            attempts: CALLBACK_RETRY_DELAYS_MS.len() as u32,
        }
    }

    /// Send a single callback request
    async fn send_callback(
        &self,
        url: &str,
        body: &str,
        timestamp: u64,
        signature: &str,
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
            .body(body.to_string())
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
pub fn send_payment_callback(
    service: Arc<CallbackService>,
    callback_url: String,
    webhook_secret: Vec<u8>,
    payment_id: String,
    tx_hash: Hash,
    amount: u64,
    confirmations: u64,
) {
    let config = create_callback_config(callback_url, webhook_secret);
    let payload = if confirmations >= 8 {
        // Stable/confirmed payment
        CallbackPayload::payment_confirmed(payment_id, tx_hash, amount, confirmations)
    } else {
        // Payment received but not yet stable
        CallbackPayload::payment_received(payment_id, tx_hash, amount, confirmations)
    };

    service.deliver_callback(config, payload);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_callback_service_creation() {
        let service = CallbackService::new();
        assert!(service.webhook_secrets.try_read().is_ok());
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
