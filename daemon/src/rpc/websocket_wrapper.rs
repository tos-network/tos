//! WebSocket Wrapper with Security Integration
//!
//! This module provides a secure WebSocket connection handler that integrates
//! the WebSocketSecurity module with the actual WebSocket connection.

use crate::rpc::websocket::WebSocketSecurity;
use actix_web::{web::Payload, HttpRequest, HttpResponse};
use log::{debug, warn};
use std::{net::IpAddr, str::FromStr, sync::Arc};
use tos_common::rpc::server::websocket::{WebSocketHandler, WebSocketServerShared};

/// Extract client IP address from the HTTP request
/// Checks X-Forwarded-For header first, then falls back to connection info
fn extract_client_ip(req: &HttpRequest) -> Result<IpAddr, &'static str> {
    // Check X-Forwarded-For header (for reverse proxies)
    if let Some(forwarded) = req.headers().get("X-Forwarded-For") {
        if let Ok(forwarded_str) = forwarded.to_str() {
            if let Some(first_ip) = forwarded_str.split(',').next() {
                if let Ok(ip) = IpAddr::from_str(first_ip.trim()) {
                    return Ok(ip);
                }
            }
        }
    }

    // Fall back to connection peer address
    if let Some(peer_addr) = req.peer_addr() {
        return Ok(peer_addr.ip());
    }

    Err("Unable to extract client IP")
}

/// Secure WebSocket connection handler
///
/// This function wraps the WebSocket connection with security checks:
/// 1. Origin validation
/// 2. Connection rate limiting
/// 3. Client IP extraction
pub async fn secure_websocket_handler<H>(
    websocket_server: &WebSocketServerShared<H>,
    security: &Arc<WebSocketSecurity>,
    request: HttpRequest,
    body: Payload,
) -> Result<HttpResponse, actix_web::Error>
where
    H: WebSocketHandler + 'static,
{
    // Extract client IP
    let client_ip = match extract_client_ip(&request) {
        Ok(ip) => ip,
        Err(e) => {
            if log::log_enabled!(log::Level::Warn) {
                warn!("Failed to extract client IP: {}", e);
            }
            return Ok(HttpResponse::Forbidden().body("Unable to determine client IP"));
        }
    };

    if log::log_enabled!(log::Level::Debug) {
        debug!("WebSocket connection attempt from IP: {}", client_ip);
    }

    // Check connection rate limit
    if let Err(e) = security.check_connection_rate(client_ip).await {
        if log::log_enabled!(log::Level::Warn) {
            warn!("Connection rate limit exceeded for IP {}: {}", client_ip, e);
        }
        return Ok(HttpResponse::TooManyRequests().body(format!("Rate limit exceeded: {}", e)));
    }

    // Validate Origin header
    let origin = request
        .headers()
        .get("Origin")
        .and_then(|h| h.to_str().ok());

    if let Err(e) = security.validate_origin(origin) {
        if log::log_enabled!(log::Level::Warn) {
            warn!("Origin validation failed for IP {}: {}", client_ip, e);
        }
        return Ok(HttpResponse::Forbidden().body(format!("Origin validation failed: {}", e)));
    }

    // All security checks passed, proceed with WebSocket connection
    if log::log_enabled!(log::Level::Debug) {
        debug!("WebSocket security checks passed for IP: {}", client_ip);
    }

    websocket_server.handle_connection(request, body).await
}
