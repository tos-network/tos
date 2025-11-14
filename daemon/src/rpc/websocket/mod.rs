//! WebSocket Security Module
//!
//! This module provides security controls for WebSocket connections.
//!
//! # Features
//!
//! - Origin validation (CORS protection)
//! - Rate limiting (per-IP and per-connection)
//! - Message size limits
//! - Subscription quotas
//! - API key authentication

pub mod security;

pub use security::{
    ApiKeyInfo, WebSocketSecurity, WebSocketSecurityConfig, WebSocketSecurityError,
};
