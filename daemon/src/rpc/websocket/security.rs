//! WebSocket Security Controls
//!
//! This module provides security controls for WebSocket connections to prevent:
//! - DoS attacks via connection flooding
//! - DoS attacks via message flooding
//! - DoS attacks via subscription flooding
//! - Cross-origin attacks (XSS, CSRF)
//! - Unauthorized access to write operations
//!
//! # Security Features
//!
//! 1. **Origin Validation**: Whitelist of allowed origins for CORS protection
//! 2. **Rate Limiting**: Per-IP connection limits and per-connection message limits
//! 3. **Message Size Limits**: Prevent memory exhaustion from large messages
//! 4. **Subscription Quotas**: Limit number of active subscriptions per connection
//! 5. **Authentication**: Optional API key authentication for sensitive operations
//!
//! # Usage
//!
//! ```rust
//! # use tos_daemon::rpc::websocket::security::{WebSocketSecurity, WebSocketSecurityConfig};
//! # use std::net::{IpAddr, Ipv4Addr};
//! # tokio::runtime::Runtime::new().unwrap().block_on(async {
//! let config = WebSocketSecurityConfig {
//!     allowed_origins: vec!["http://localhost:3000".to_string()],
//!     require_auth: true,
//!     max_message_size: 1048576, // 1MB
//!     max_subscriptions_per_connection: 100,
//!     ..Default::default()
//! };
//!
//! let security = WebSocketSecurity::new(config);
//!
//! // Validate origin
//! let origin = Some("http://localhost:3000");
//! security.validate_origin(origin).expect("origin should be valid");
//!
//! // Check rate limits
//! let peer_ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
//! security.check_connection_rate(peer_ip).await.expect("connection rate OK");
//!
//! let connection_id = 12345u64;
//! security.check_message_rate(connection_id).await.expect("message rate OK");
//! # });
//! ```

use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use crate::core::error::BlockchainError;
use log::warn;
use tokio::sync::RwLock;

/// WebSocket security configuration
#[derive(Debug, Clone)]
pub struct WebSocketSecurityConfig {
    /// List of allowed origins for CORS protection
    /// Use "*" to allow all origins (NOT RECOMMENDED for production)
    pub allowed_origins: Vec<String>,

    /// Require authentication for write operations
    pub require_auth: bool,

    /// Maximum message size in bytes (default: 1MB)
    pub max_message_size: usize,

    /// Maximum number of subscriptions per connection (default: 100)
    pub max_subscriptions_per_connection: usize,

    /// Maximum connections per IP per minute (default: 100)
    pub max_connections_per_ip_per_minute: u32,

    /// Maximum messages per connection per second (default: 10)
    pub max_messages_per_connection_per_second: u32,
}

impl Default for WebSocketSecurityConfig {
    fn default() -> Self {
        Self {
            // Default: Only localhost allowed
            allowed_origins: vec!["http://localhost:3000".to_string()],
            require_auth: false,
            max_message_size: 1024 * 1024, // 1MB
            max_subscriptions_per_connection: 100,
            max_connections_per_ip_per_minute: 100,
            max_messages_per_connection_per_second: 10,
        }
    }
}

/// Error types for WebSocket security violations
#[derive(Debug, thiserror::Error)]
pub enum WebSocketSecurityError {
    #[error("Missing Origin header")]
    MissingOrigin,

    #[error("Invalid origin: {origin}")]
    InvalidOrigin { origin: String },

    #[error("Rate limit exceeded for IP: {ip}")]
    ConnectionRateLimitExceeded { ip: IpAddr },

    #[error("Message rate limit exceeded for connection: {connection_id}")]
    MessageRateLimitExceeded { connection_id: u64 },

    #[error("Message too large: {size} bytes (max: {max} bytes)")]
    MessageTooLarge { size: usize, max: usize },

    #[error("Subscription quota exceeded: {count} (max: {max})")]
    SubscriptionQuotaExceeded { count: usize, max: usize },

    #[error("Authentication required")]
    MissingAuth,

    #[error("Invalid authentication token")]
    InvalidAuth,

    #[error("Invalid API key")]
    InvalidApiKey,
}

impl From<WebSocketSecurityError> for BlockchainError {
    fn from(err: WebSocketSecurityError) -> Self {
        BlockchainError::Any(err.into())
    }
}

/// Rate limiter for tracking connection attempts per IP
struct ConnectionRateLimiter {
    /// Map of IP addresses to their connection attempt history
    /// Each entry is (last_reset_time, connection_count_this_minute)
    attempts: RwLock<HashMap<IpAddr, (Instant, u32)>>,
    /// Maximum connections per IP per minute
    max_per_minute: u32,
}

impl ConnectionRateLimiter {
    fn new(max_per_minute: u32) -> Self {
        Self {
            attempts: RwLock::new(HashMap::new()),
            max_per_minute,
        }
    }

    /// Check if the IP is allowed to connect
    /// Returns Ok(()) if allowed, Err if rate limit exceeded
    async fn check(&self, ip: IpAddr) -> Result<(), WebSocketSecurityError> {
        let now = Instant::now();
        let mut attempts = self.attempts.write().await;

        let (last_reset, count) = attempts.entry(ip).or_insert_with(|| (now, 0));

        // Reset counter if more than 1 minute has passed
        if now.duration_since(*last_reset) >= Duration::from_secs(60) {
            *last_reset = now;
            *count = 0;
        }

        // Check if limit exceeded
        if *count >= self.max_per_minute {
            return Err(WebSocketSecurityError::ConnectionRateLimitExceeded { ip });
        }

        // Increment counter
        *count += 1;
        Ok(())
    }

    /// Cleanup old entries (should be called periodically)
    async fn cleanup(&self) {
        let now = Instant::now();
        let mut attempts = self.attempts.write().await;
        attempts.retain(|_, (last_reset, _)| {
            now.duration_since(*last_reset) < Duration::from_secs(120)
        });
    }
}

/// Rate limiter for tracking messages per connection
struct MessageRateLimiter {
    /// Map of connection IDs to their message history
    /// Each entry is (last_reset_time, message_count_this_second)
    messages: RwLock<HashMap<u64, (Instant, u32)>>,
    /// Maximum messages per connection per second
    max_per_second: u32,
}

impl MessageRateLimiter {
    fn new(max_per_second: u32) -> Self {
        Self {
            messages: RwLock::new(HashMap::new()),
            max_per_second,
        }
    }

    /// Check if the connection is allowed to send a message
    async fn check(&self, connection_id: u64) -> Result<(), WebSocketSecurityError> {
        let now = Instant::now();
        let mut messages = self.messages.write().await;

        let (last_reset, count) = messages.entry(connection_id).or_insert_with(|| (now, 0));

        // Reset counter if more than 1 second has passed
        if now.duration_since(*last_reset) >= Duration::from_secs(1) {
            *last_reset = now;
            *count = 0;
        }

        // Check if limit exceeded
        if *count >= self.max_per_second {
            return Err(WebSocketSecurityError::MessageRateLimitExceeded { connection_id });
        }

        // Increment counter
        *count += 1;
        Ok(())
    }

    /// Remove a connection from tracking when it closes
    async fn remove_connection(&self, connection_id: u64) {
        let mut messages = self.messages.write().await;
        messages.remove(&connection_id);
    }

    /// Cleanup old entries (should be called periodically)
    async fn cleanup(&self) {
        let now = Instant::now();
        let mut messages = self.messages.write().await;
        messages
            .retain(|_, (last_reset, _)| now.duration_since(*last_reset) < Duration::from_secs(10));
    }
}

/// WebSocket security controls
pub struct WebSocketSecurity {
    config: WebSocketSecurityConfig,
    connection_limiter: ConnectionRateLimiter,
    message_limiter: MessageRateLimiter,
    active_subscriptions: Arc<AtomicUsize>,
    api_keys: RwLock<HashMap<String, ApiKeyInfo>>,
}

/// API key information
#[derive(Debug, Clone)]
pub struct ApiKeyInfo {
    pub key: String,
    pub description: String,
    pub created_at: Instant,
    pub permissions: Vec<String>,
}

impl WebSocketSecurity {
    /// Create a new WebSocket security instance
    pub fn new(config: WebSocketSecurityConfig) -> Self {
        Self {
            connection_limiter: ConnectionRateLimiter::new(
                config.max_connections_per_ip_per_minute,
            ),
            message_limiter: MessageRateLimiter::new(config.max_messages_per_connection_per_second),
            active_subscriptions: Arc::new(AtomicUsize::new(0)),
            api_keys: RwLock::new(HashMap::new()),
            config,
        }
    }

    /// Validate the Origin header for CORS protection
    ///
    /// # Security
    /// This prevents cross-origin attacks (XSS, CSRF) by ensuring that
    /// only whitelisted origins can connect to the WebSocket server.
    ///
    /// # Arguments
    /// * `origin` - The Origin header value from the HTTP request
    ///
    /// # Returns
    /// * `Ok(())` if origin is allowed
    /// * `Err(WebSocketSecurityError)` if origin is missing or not allowed
    pub fn validate_origin(&self, origin: Option<&str>) -> Result<(), WebSocketSecurityError> {
        // If authentication is not required, origin check is optional
        if !self.config.require_auth && origin.is_none() {
            return Ok(());
        }

        match origin {
            None if self.config.require_auth => Err(WebSocketSecurityError::MissingOrigin),
            Some(origin) => {
                // Check if wildcard is allowed
                if self.config.allowed_origins.contains(&"*".to_string()) {
                    if log::log_enabled!(log::Level::Warn) {
                        warn!("WebSocket accepting connections from all origins (wildcard *)");
                    }
                    return Ok(());
                }

                // Check if origin matches any allowed origin
                let is_allowed = self
                    .config
                    .allowed_origins
                    .iter()
                    .any(|allowed| origin.starts_with(allowed));

                if is_allowed {
                    Ok(())
                } else {
                    Err(WebSocketSecurityError::InvalidOrigin {
                        origin: origin.to_string(),
                    })
                }
            }
            None => Ok(()),
        }
    }

    /// Check if the IP address is allowed to connect (rate limiting)
    ///
    /// # Security
    /// This prevents DoS attacks via connection flooding by limiting
    /// the number of connections from a single IP address.
    pub async fn check_connection_rate(&self, ip: IpAddr) -> Result<(), WebSocketSecurityError> {
        self.connection_limiter.check(ip).await
    }

    /// Check if the connection is allowed to send a message (rate limiting)
    ///
    /// # Security
    /// This prevents DoS attacks via message flooding by limiting
    /// the number of messages per connection per second.
    pub async fn check_message_rate(
        &self,
        connection_id: u64,
    ) -> Result<(), WebSocketSecurityError> {
        self.message_limiter.check(connection_id).await
    }

    /// Validate message size
    ///
    /// # Security
    /// This prevents memory exhaustion attacks by rejecting messages
    /// that exceed the configured size limit.
    pub fn validate_message_size(&self, size: usize) -> Result<(), WebSocketSecurityError> {
        if size > self.config.max_message_size {
            Err(WebSocketSecurityError::MessageTooLarge {
                size,
                max: self.config.max_message_size,
            })
        } else {
            Ok(())
        }
    }

    /// Check subscription quota
    ///
    /// # Security
    /// This prevents DoS attacks via subscription flooding by limiting
    /// the total number of active subscriptions.
    pub fn check_subscription_quota(&self) -> Result<(), WebSocketSecurityError> {
        let count = self.active_subscriptions.load(Ordering::Relaxed);
        if count >= self.config.max_subscriptions_per_connection {
            Err(WebSocketSecurityError::SubscriptionQuotaExceeded {
                count,
                max: self.config.max_subscriptions_per_connection,
            })
        } else {
            Ok(())
        }
    }

    /// Increment active subscription count
    pub fn add_subscription(&self) {
        self.active_subscriptions.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement active subscription count
    pub fn remove_subscription(&self) {
        self.active_subscriptions.fetch_sub(1, Ordering::Relaxed);
    }

    /// Remove a connection from rate limiting tracking
    pub async fn remove_connection(&self, connection_id: u64) {
        self.message_limiter.remove_connection(connection_id).await;
    }

    /// Verify API key for authentication
    ///
    /// # Security
    /// This ensures that only authorized clients can perform
    /// write operations (submitting blocks, transactions, etc.).
    pub async fn verify_api_key(&self, key: &str) -> Result<ApiKeyInfo, WebSocketSecurityError> {
        let api_keys = self.api_keys.read().await;
        api_keys
            .get(key)
            .cloned()
            .ok_or(WebSocketSecurityError::InvalidApiKey)
    }

    /// Add an API key
    pub async fn add_api_key(&self, key: String, description: String, permissions: Vec<String>) {
        let mut api_keys = self.api_keys.write().await;
        api_keys.insert(
            key.clone(),
            ApiKeyInfo {
                key,
                description,
                created_at: Instant::now(),
                permissions,
            },
        );
    }

    /// Remove an API key
    pub async fn remove_api_key(&self, key: &str) {
        let mut api_keys = self.api_keys.write().await;
        api_keys.remove(key);
    }

    /// Cleanup old rate limiter entries (call periodically)
    pub async fn cleanup(&self) {
        self.connection_limiter.cleanup().await;
        self.message_limiter.cleanup().await;
    }

    /// Get configuration
    pub fn config(&self) -> &WebSocketSecurityConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[tokio::test]
    async fn test_origin_validation_allowed() {
        let config = WebSocketSecurityConfig {
            allowed_origins: vec!["http://localhost:3000".to_string()],
            ..Default::default()
        };
        let security = WebSocketSecurity::new(config);

        assert!(security
            .validate_origin(Some("http://localhost:3000"))
            .is_ok());
    }

    #[tokio::test]
    async fn test_origin_validation_denied() {
        let config = WebSocketSecurityConfig {
            allowed_origins: vec!["http://localhost:3000".to_string()],
            require_auth: true,
            ..Default::default()
        };
        let security = WebSocketSecurity::new(config);

        assert!(security.validate_origin(Some("http://evil.com")).is_err());
    }

    #[tokio::test]
    async fn test_origin_validation_wildcard() {
        let config = WebSocketSecurityConfig {
            allowed_origins: vec!["*".to_string()],
            ..Default::default()
        };
        let security = WebSocketSecurity::new(config);

        assert!(security
            .validate_origin(Some("http://anywhere.com"))
            .is_ok());
    }

    #[tokio::test]
    async fn test_connection_rate_limit() {
        let config = WebSocketSecurityConfig {
            max_connections_per_ip_per_minute: 2,
            ..Default::default()
        };
        let security = WebSocketSecurity::new(config);
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));

        // First two connections should succeed
        assert!(security.check_connection_rate(ip).await.is_ok());
        assert!(security.check_connection_rate(ip).await.is_ok());

        // Third connection should fail
        assert!(security.check_connection_rate(ip).await.is_err());
    }

    #[tokio::test]
    async fn test_message_rate_limit() {
        let config = WebSocketSecurityConfig {
            max_messages_per_connection_per_second: 2,
            ..Default::default()
        };
        let security = WebSocketSecurity::new(config);
        let conn_id = 12345;

        // First two messages should succeed
        assert!(security.check_message_rate(conn_id).await.is_ok());
        assert!(security.check_message_rate(conn_id).await.is_ok());

        // Third message should fail
        assert!(security.check_message_rate(conn_id).await.is_err());
    }

    #[tokio::test]
    async fn test_message_size_validation() {
        let config = WebSocketSecurityConfig {
            max_message_size: 1024,
            ..Default::default()
        };
        let security = WebSocketSecurity::new(config);

        assert!(security.validate_message_size(512).is_ok());
        assert!(security.validate_message_size(1024).is_ok());
        assert!(security.validate_message_size(2048).is_err());
    }

    #[tokio::test]
    async fn test_subscription_quota() {
        let config = WebSocketSecurityConfig {
            max_subscriptions_per_connection: 2,
            ..Default::default()
        };
        let security = WebSocketSecurity::new(config);

        // First two subscriptions should succeed
        assert!(security.check_subscription_quota().is_ok());
        security.add_subscription();
        assert!(security.check_subscription_quota().is_ok());
        security.add_subscription();

        // Third subscription should fail
        assert!(security.check_subscription_quota().is_err());

        // After removing one, should succeed again
        security.remove_subscription();
        assert!(security.check_subscription_quota().is_ok());
    }

    #[tokio::test]
    async fn test_api_key_management() {
        let config = WebSocketSecurityConfig::default();
        let security = WebSocketSecurity::new(config);

        // Add API key
        security
            .add_api_key(
                "test-key-123".to_string(),
                "Test key".to_string(),
                vec!["read".to_string(), "write".to_string()],
            )
            .await;

        // Verify valid key
        assert!(security.verify_api_key("test-key-123").await.is_ok());

        // Verify invalid key
        assert!(security.verify_api_key("invalid-key").await.is_err());

        // Remove key
        security.remove_api_key("test-key-123").await;

        // Verify removed key
        assert!(security.verify_api_key("test-key-123").await.is_err());
    }
}
