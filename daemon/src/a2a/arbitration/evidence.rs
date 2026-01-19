use std::net::IpAddr;
use std::time::Duration;

use reqwest::redirect::Policy;
use reqwest::Client;
use tokio::time::sleep;
use url::Url;

use tos_common::crypto::Hash;

use super::{
    arbitration_root, ensure_dir, write_atomic, ArbitrationError, MAX_EVIDENCE_FETCH_SECS,
    MAX_EVIDENCE_TOTAL_BYTES,
};

const MAX_REDIRECTS: usize = 3;
const ENV_CA_BUNDLE: &str = "TOS_ARBITRATION_CA_BUNDLE";
const ENV_ALLOW_LOCAL: &str = "TOS_ARBITRATION_TEST_ALLOW_LOCAL";
const ENV_TIMEOUT_SECS: &str = "TOS_ARBITRATION_EVIDENCE_TIMEOUT_SECS";
const ENV_MAX_BYTES: &str = "TOS_ARBITRATION_EVIDENCE_MAX_BYTES";
const ENV_MAX_REDIRECTS: &str = "TOS_ARBITRATION_EVIDENCE_MAX_REDIRECTS";

pub struct EvidenceArtifact {
    pub bytes: Vec<u8>,
    pub final_url: String,
}

const MAX_EVIDENCE_RETRY_ATTEMPTS: usize = 3;
const EVIDENCE_RETRY_BASE_MS: u64 = 200;

pub async fn fetch_evidence(
    uri: &str,
    expected_hash: &Hash,
) -> Result<EvidenceArtifact, ArbitrationError> {
    let attempts = evidence_retry_attempts();
    for attempt in 0..attempts {
        match fetch_evidence_once(uri, expected_hash).await {
            Ok(artifact) => return Ok(artifact),
            Err(err) => {
                if !err.is_retryable() || attempt + 1 == attempts {
                    return Err(ArbitrationError::Evidence(err.message()));
                }
                let delay = evidence_retry_delay_ms(attempt);
                sleep(Duration::from_millis(delay)).await;
            }
        }
    }
    Err(ArbitrationError::Evidence(
        "evidence fetch failed".to_string(),
    ))
}

fn load_a2a_artifact(
    uri: &str,
    expected_hash: &Hash,
) -> Result<EvidenceArtifact, ArbitrationError> {
    let hash_part = uri.trim_start_matches("a2a://artifact/");
    let Some(root) = arbitration_root() else {
        return Err(ArbitrationError::Evidence("no base dir".to_string()));
    };
    let artifacts_dir = root.join("artifacts");
    let path = artifacts_dir.join(hash_part);
    let bytes = std::fs::read(&path).map_err(|e| ArbitrationError::Evidence(e.to_string()))?;
    let hash = compute_hash(&bytes);
    if &hash != expected_hash {
        return Err(ArbitrationError::Evidence("hash mismatch".to_string()));
    }
    Ok(EvidenceArtifact {
        bytes,
        final_url: uri.to_string(),
    })
}

pub fn store_a2a_artifact(hash: &Hash, bytes: &[u8]) -> Result<(), ArbitrationError> {
    let Some(root) = arbitration_root() else {
        return Err(ArbitrationError::Evidence("no base dir".to_string()));
    };
    let artifacts_dir = root.join("artifacts");
    ensure_dir(&artifacts_dir)?;
    let path = artifacts_dir.join(hash.to_hex());
    write_atomic(&path, bytes)
}

fn compute_hash(bytes: &[u8]) -> Hash {
    use sha3::{Digest, Sha3_256};
    let mut hasher = Sha3_256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    Hash::new(digest.into())
}

fn is_allowed_content_type(value: &str) -> bool {
    let value = value.to_lowercase();
    value.starts_with("application/json") || value.starts_with("application/octet-stream")
}

async fn fetch_evidence_once(
    uri: &str,
    expected_hash: &Hash,
) -> Result<EvidenceArtifact, FetchFailure> {
    if uri.starts_with("a2a://") {
        return match load_a2a_artifact(uri, expected_hash) {
            Ok(artifact) => Ok(artifact),
            Err(ArbitrationError::Evidence(message)) => Err(FetchFailure::Fatal(message)),
            Err(err) => Err(FetchFailure::Fatal(err.to_string())),
        };
    }

    let mut url = Url::parse(uri).map_err(|_| FetchFailure::Fatal("invalid url".to_string()))?;
    if url.scheme() != "https" {
        return Err(FetchFailure::Fatal("unsupported scheme".to_string()));
    }

    let mut client = Client::builder()
        .redirect(Policy::none())
        .timeout(Duration::from_secs(evidence_timeout_secs()))
        .build()
        .map_err(|e| FetchFailure::Fatal(e.to_string()))?;

    if let Ok(bundle_path) = std::env::var(ENV_CA_BUNDLE) {
        let pem = std::fs::read(&bundle_path).map_err(|e| FetchFailure::Fatal(e.to_string()))?;
        let cert =
            reqwest::Certificate::from_pem(&pem).map_err(|e| FetchFailure::Fatal(e.to_string()))?;
        client = Client::builder()
            .redirect(Policy::none())
            .timeout(Duration::from_secs(evidence_timeout_secs()))
            .add_root_certificate(cert)
            .build()
            .map_err(|e| FetchFailure::Fatal(e.to_string()))?;
    }

    let mut redirects = 0usize;
    loop {
        if !allow_local_hosts() {
            validate_url_host(&url)
                .await
                .map_err(|e| FetchFailure::Fatal(e.to_string()))?;
        }

        let response = match client.get(url.clone()).send().await {
            Ok(response) => response,
            Err(err) => {
                let message = err.to_string();
                if err.is_timeout() || err.is_connect() {
                    return Err(FetchFailure::Retryable(message));
                }
                return Err(FetchFailure::Fatal(message));
            }
        };

        if response.status().is_redirection() {
            if redirects >= max_redirects() {
                return Err(FetchFailure::Fatal("too many redirects".to_string()));
            }
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .ok_or_else(|| FetchFailure::Fatal("redirect missing location".to_string()))?;
            let next = location
                .to_str()
                .map_err(|_| FetchFailure::Fatal("invalid redirect".to_string()))?;
            url = url
                .join(next)
                .map_err(|_| FetchFailure::Fatal("invalid redirect".to_string()))?;
            redirects += 1;
            continue;
        }

        if !response.status().is_success() {
            if response.status().is_server_error() || response.status() == 429 {
                return Err(FetchFailure::Retryable(format!(
                    "fetch failed: {}",
                    response.status()
                )));
            }
            return Err(FetchFailure::Fatal(format!(
                "fetch failed: {}",
                response.status()
            )));
        }

        if let Some(content_type) = response.headers().get(reqwest::header::CONTENT_TYPE) {
            let value = content_type.to_str().unwrap_or("");
            if !is_allowed_content_type(value) {
                return Err(FetchFailure::Fatal("content-type not allowed".to_string()));
            }
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| FetchFailure::Fatal(e.to_string()))?
            .to_vec();
        if bytes.len() > evidence_max_bytes() {
            return Err(FetchFailure::Fatal("evidence too large".to_string()));
        }

        let hash = compute_hash(&bytes);
        if &hash != expected_hash {
            return Err(FetchFailure::Fatal("hash mismatch".to_string()));
        }

        return Ok(EvidenceArtifact {
            bytes,
            final_url: url.to_string(),
        });
    }
}

async fn validate_url_host(url: &Url) -> Result<(), ArbitrationError> {
    let host = url
        .host_str()
        .ok_or_else(|| ArbitrationError::Evidence("missing host".to_string()))?;
    let port = url.port_or_known_default().unwrap_or(443);

    let addrs = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| ArbitrationError::Evidence("dns failure".to_string()))?;

    for addr in addrs {
        if is_blocked_ip(addr.ip()) {
            return Err(ArbitrationError::Evidence("blocked ip".to_string()));
        }
    }

    Ok(())
}

fn allow_local_hosts() -> bool {
    matches!(std::env::var(ENV_ALLOW_LOCAL).as_deref(), Ok("1"))
}

fn evidence_timeout_secs() -> u64 {
    std::env::var(ENV_TIMEOUT_SECS)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(MAX_EVIDENCE_FETCH_SECS)
}

fn evidence_max_bytes() -> usize {
    std::env::var(ENV_MAX_BYTES)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(MAX_EVIDENCE_TOTAL_BYTES)
}

fn evidence_retry_attempts() -> usize {
    MAX_EVIDENCE_RETRY_ATTEMPTS.max(1)
}

fn evidence_retry_delay_ms(attempt: usize) -> u64 {
    let exp = 2u64.saturating_pow(attempt as u32);
    EVIDENCE_RETRY_BASE_MS.saturating_mul(exp).min(5_000)
}

fn max_redirects() -> usize {
    std::env::var(ENV_MAX_REDIRECTS)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(MAX_REDIRECTS)
}

fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.octets()[0] == 0
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unique_local()
                || v6.is_unspecified()
                || v6.is_multicast()
                || v6.is_unicast_link_local()
        }
    }
}

enum FetchFailure {
    Retryable(String),
    Fatal(String),
}

impl FetchFailure {
    fn is_retryable(&self) -> bool {
        matches!(self, FetchFailure::Retryable(_))
    }

    fn message(self) -> String {
        match self {
            FetchFailure::Retryable(message) | FetchFailure::Fatal(message) => message,
        }
    }
}
