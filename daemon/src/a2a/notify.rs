use std::net::IpAddr;
use std::time::Duration;

use reqwest::Client;
use tokio::net::lookup_host;
use tokio::time::sleep;
use url::Url;

use tos_common::a2a::{
    ListTaskPushNotificationConfigResponse, StreamResponse, TaskPushNotificationConfig,
};

use super::storage::A2AStore;

const RETRY_DELAYS_MS: [u64; 3] = [200, 1000, 3000];
const REQUEST_TIMEOUT_SECS: u64 = 5;

/// Validate push notification URL for SSRF protection (basic hostname check)
fn is_safe_url_hostname(url: &Url) -> bool {
    // Only allow HTTPS
    if url.scheme() != "https" {
        return false;
    }

    // Check for private/internal IPs
    if let Some(host) = url.host_str() {
        // Block localhost
        if host == "localhost" || host == "127.0.0.1" || host == "::1" {
            return false;
        }

        // Try to parse as IP and check for private ranges
        if let Ok(ip) = host.parse::<IpAddr>() {
            if is_private_ip(&ip) {
                return false;
            }
        }

        // Block internal hostnames
        if host.ends_with(".local") || host.ends_with(".internal") || host.ends_with(".localhost") {
            return false;
        }
    }

    true
}

/// Validate push notification URL with DNS resolution for SSRF/DNS rebinding protection
async fn validate_url_with_dns(url_str: &str) -> Result<Url, &'static str> {
    let url = Url::parse(url_str).map_err(|_| "invalid URL")?;

    // Basic hostname validation
    if !is_safe_url_hostname(&url) {
        return Err("URL blocked by SSRF protection");
    }

    // Get host and port for DNS resolution
    let host = url.host_str().ok_or("no host in URL")?;
    let port = url.port_or_known_default().unwrap_or(443);

    // Skip DNS check if already an IP address
    if host.parse::<IpAddr>().is_ok() {
        return Ok(url);
    }

    // Resolve DNS and check all resolved IPs
    let addr_str = format!("{}:{}", host, port);
    let addrs = lookup_host(&addr_str)
        .await
        .map_err(|_| "DNS resolution failed")?;

    let resolved: Vec<_> = addrs.collect();
    if resolved.is_empty() {
        return Err("DNS resolution returned no addresses");
    }

    // Check all resolved IPs are safe (prevent DNS rebinding)
    for addr in &resolved {
        if is_private_ip(&addr.ip()) {
            return Err("DNS resolved to private IP");
        }
    }

    Ok(url)
}

/// Check if IP address is in private range
fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                // 169.254.0.0/16 link-local
                || (v4.octets()[0] == 169 && v4.octets()[1] == 254)
        }
        IpAddr::V6(v6) => v6.is_loopback() || v6.is_unspecified(),
    }
}

pub async fn notify_task_event(store: &A2AStore, task_id: &str, event: StreamResponse) {
    let configs = list_all_configs(store, task_id).await;
    if configs.is_empty() {
        return;
    }

    let client = match Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
    {
        Ok(c) => c,
        Err(_) => return,
    };
    for config in configs {
        let client = client.clone();
        let event = event.clone();
        tokio::spawn(async move {
            send_with_retry(&client, &config, &event).await;
        });
    }
}

async fn list_all_configs(store: &A2AStore, task_id: &str) -> Vec<TaskPushNotificationConfig> {
    let mut page_token: Option<String> = None;
    let mut configs = Vec::new();
    loop {
        let response = store
            .list_push_configs(task_id, None, page_token.clone())
            .unwrap_or_else(|_| ListTaskPushNotificationConfigResponse {
                configs: Vec::new(),
                next_page_token: String::new(),
            });
        configs.extend(response.configs);
        if response.next_page_token.is_empty() {
            break;
        }
        page_token = Some(response.next_page_token);
    }
    configs
}

async fn send_with_retry(
    client: &Client,
    config: &TaskPushNotificationConfig,
    event: &StreamResponse,
) {
    // SSRF protection with DNS rebinding prevention: validate URL and resolve DNS
    let url = match validate_url_with_dns(&config.push_notification_config.url).await {
        Ok(u) => u,
        Err(reason) => {
            if log::log_enabled!(log::Level::Warn) {
                log::warn!(
                    "Push notification URL blocked ({}): {}",
                    reason,
                    config.push_notification_config.url
                );
            }
            return;
        }
    };

    for (idx, delay) in RETRY_DELAYS_MS.iter().enumerate() {
        let mut request = client.post(url.as_str());
        if let Some(token) = config.push_notification_config.token.as_ref() {
            request = request.bearer_auth(token);
        }
        let response = request.json(event).send().await;
        if response
            .as_ref()
            .map(|r| r.status().is_success())
            .unwrap_or(false)
        {
            return;
        }
        if idx + 1 < RETRY_DELAYS_MS.len() {
            sleep(Duration::from_millis(*delay)).await;
        }
    }
}
