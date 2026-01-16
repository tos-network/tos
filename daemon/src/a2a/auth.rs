use std::{
    collections::{HashMap, HashSet},
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use jsonwebtoken::{decode, decode_header, jwk::JwkSet, DecodingKey, Validation};
use once_cell::sync::OnceCell;
use reqwest::Client;
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;
use tos_common::crypto::elgamal::CompressedPublicKey;
use tos_common::{crypto::Signature, rpc::server::RequestMetadata};
use tos_crypto::curve25519_dalek::ristretto::CompressedRistretto;

use crate::core::config::RPCConfig;

const DEFAULT_JWKS_TTL_SECS: u64 = 600;

#[derive(Debug, Clone)]
pub struct A2AAuthConfig {
    pub api_keys: HashSet<String>,
    pub oauth_issuer: Option<String>,
    pub oauth_jwks_url: Option<String>,
    pub oauth_audience: Option<String>,
    pub tos_skew_secs: i64,
    pub tos_nonce_ttl_secs: i64,
}

impl A2AAuthConfig {
    pub fn from_rpc_config(config: &RPCConfig) -> Self {
        Self {
            api_keys: config.a2a_api_keys.iter().cloned().collect(),
            oauth_issuer: config.a2a_oauth_issuer.clone(),
            oauth_jwks_url: config.a2a_oauth_jwks_url.clone(),
            oauth_audience: config.a2a_oauth_audience.clone(),
            tos_skew_secs: config.a2a_tos_skew_secs as i64,
            tos_nonce_ttl_secs: config.a2a_tos_nonce_ttl_secs as i64,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    ApiKey,
    OAuth,
    TosSignature,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("missing authentication")]
    MissingAuth,
    #[error("invalid authentication")]
    InvalidAuth,
    #[error("oauth is not configured")]
    OAuthNotConfigured,
    #[error("oauth token is invalid")]
    OAuthInvalid,
    #[error("api key is invalid")]
    ApiKeyInvalid,
    #[error("TOS signature is invalid")]
    TosSignatureInvalid,
    #[error("TOS signature headers are missing")]
    TosSignatureMissing,
    #[error("TOS timestamp is invalid")]
    TosTimestampInvalid,
    #[error("TOS nonce is invalid")]
    TosNonceInvalid,
    #[error("TOS public key is invalid")]
    TosPublicKeyInvalid,
    #[error("TOS public key is not registered on-chain")]
    TosPublicKeyNotRegistered,
    #[error("TOS signature is expired")]
    TosSignatureExpired,
    #[error("failed to fetch JWKS")]
    JwksFetchFailed,
}

struct JwksCache {
    fetched_at: tokio::time::Instant,
    jwks: JwkSet,
}

pub struct A2AAuth {
    config: A2AAuthConfig,
    http: Client,
    jwks_cache: RwLock<Option<JwksCache>>,
    nonces: Mutex<HashMap<String, i64>>,
}

static AUTH: OnceCell<A2AAuth> = OnceCell::new();

pub fn set_auth_config(config: A2AAuthConfig) {
    let _ = AUTH.set(A2AAuth::new(config));
}

pub fn get_auth_config() -> Option<A2AAuthConfig> {
    AUTH.get().map(|auth| auth.config.clone())
}

pub async fn authorize_metadata(meta: &RequestMetadata) -> Result<AuthMethod, AuthError> {
    let auth = AUTH.get().ok_or(AuthError::MissingAuth)?;
    auth.authorize(meta).await
}

/// Authorize with on-chain verification for TOS signatures.
/// The `is_registered` callback should check if the public key is registered
/// as a controller or session key in the on-chain AgentAccountMeta.
pub async fn authorize_metadata_with_chain_check<F>(
    meta: &RequestMetadata,
    is_registered: F,
) -> Result<AuthMethod, AuthError>
where
    F: Fn(&tos_common::crypto::PublicKey) -> bool,
{
    let auth = AUTH.get().ok_or(AuthError::MissingAuth)?;
    auth.authorize_with_chain_check(meta, is_registered).await
}

/// Extract the TOS signer's public key from request headers.
/// Returns the verified public key if TOS signature headers are present and valid.
pub fn extract_tos_signer_pubkey(
    meta: &RequestMetadata,
) -> Result<tos_common::crypto::PublicKey, AuthError> {
    let auth = AUTH.get().ok_or(AuthError::MissingAuth)?;
    auth.extract_and_verify_tos_pubkey(meta)
}

impl A2AAuth {
    fn new(config: A2AAuthConfig) -> Self {
        Self {
            config,
            http: Client::new(),
            jwks_cache: RwLock::new(None),
            nonces: Mutex::new(HashMap::new()),
        }
    }

    async fn authorize(&self, meta: &RequestMetadata) -> Result<AuthMethod, AuthError> {
        let mut errors = Vec::new();

        if let Some(token) = extract_bearer_token(&meta.headers) {
            if !self.config.api_keys.is_empty() {
                if self.config.api_keys.contains(token) {
                    return Ok(AuthMethod::ApiKey);
                }
                errors.push(AuthError::ApiKeyInvalid);
            }

            if self.oauth_configured() {
                if self.verify_oauth(token).await.is_ok() {
                    return Ok(AuthMethod::OAuth);
                }
                errors.push(AuthError::OAuthInvalid);
            }
        }

        if let Some(key) = meta.headers.get("x-api-key") {
            if !self.config.api_keys.is_empty() && self.config.api_keys.contains(key) {
                return Ok(AuthMethod::ApiKey);
            }
            errors.push(AuthError::ApiKeyInvalid);
        }

        if has_tos_signature_headers(&meta.headers) {
            if self.verify_tos_signature(meta).is_ok() {
                return Ok(AuthMethod::TosSignature);
            }
            errors.push(AuthError::TosSignatureInvalid);
        }

        if errors.is_empty() {
            Err(AuthError::MissingAuth)
        } else {
            Err(AuthError::InvalidAuth)
        }
    }

    async fn authorize_with_chain_check<F>(
        &self,
        meta: &RequestMetadata,
        is_registered: F,
    ) -> Result<AuthMethod, AuthError>
    where
        F: Fn(&tos_common::crypto::PublicKey) -> bool,
    {
        let mut errors = Vec::new();

        if let Some(token) = extract_bearer_token(&meta.headers) {
            if !self.config.api_keys.is_empty() {
                if self.config.api_keys.contains(token) {
                    return Ok(AuthMethod::ApiKey);
                }
                errors.push(AuthError::ApiKeyInvalid);
            }

            if self.oauth_configured() {
                if self.verify_oauth(token).await.is_ok() {
                    return Ok(AuthMethod::OAuth);
                }
                errors.push(AuthError::OAuthInvalid);
            }
        }

        if let Some(key) = meta.headers.get("x-api-key") {
            if !self.config.api_keys.is_empty() && self.config.api_keys.contains(key) {
                return Ok(AuthMethod::ApiKey);
            }
            errors.push(AuthError::ApiKeyInvalid);
        }

        if has_tos_signature_headers(&meta.headers) {
            match self.verify_tos_signature_and_get_pubkey(meta) {
                Ok(pubkey) => {
                    if is_registered(&pubkey) {
                        return Ok(AuthMethod::TosSignature);
                    }
                    errors.push(AuthError::TosPublicKeyNotRegistered);
                }
                Err(e) => errors.push(e),
            }
        }

        if errors.is_empty() {
            Err(AuthError::MissingAuth)
        } else {
            Err(AuthError::InvalidAuth)
        }
    }

    fn extract_and_verify_tos_pubkey(
        &self,
        meta: &RequestMetadata,
    ) -> Result<tos_common::crypto::PublicKey, AuthError> {
        self.verify_tos_signature_and_get_pubkey(meta)
    }

    fn oauth_configured(&self) -> bool {
        self.config.oauth_issuer.is_some() && self.config.oauth_jwks_url.is_some()
    }

    async fn verify_oauth(&self, token: &str) -> Result<(), AuthError> {
        if !self.oauth_configured() {
            return Err(AuthError::OAuthNotConfigured);
        }

        let header = decode_header(token).map_err(|_| AuthError::OAuthInvalid)?;
        let kid = header.kid.ok_or(AuthError::OAuthInvalid)?;
        let jwks = self.load_jwks().await?;
        let jwk = jwks
            .keys
            .iter()
            .find(|jwk| jwk.common.key_id.as_deref() == Some(&kid))
            .ok_or(AuthError::OAuthInvalid)?;

        let decoding_key = DecodingKey::from_jwk(jwk).map_err(|_| AuthError::OAuthInvalid)?;
        let mut validation = Validation::new(header.alg);
        if let Some(issuer) = &self.config.oauth_issuer {
            validation.set_issuer(&[issuer]);
        }
        if let Some(audience) = &self.config.oauth_audience {
            validation.set_audience(&[audience]);
        }
        decode::<serde_json::Value>(token, &decoding_key, &validation)
            .map_err(|_| AuthError::OAuthInvalid)?;
        Ok(())
    }

    async fn load_jwks(&self) -> Result<JwkSet, AuthError> {
        let mut cache_guard = self.jwks_cache.write().await;
        if let Some(cache) = cache_guard.as_ref() {
            if cache.fetched_at.elapsed() < Duration::from_secs(DEFAULT_JWKS_TTL_SECS) {
                return Ok(cache.jwks.clone());
            }
        }

        let url = self
            .config
            .oauth_jwks_url
            .as_ref()
            .ok_or(AuthError::OAuthNotConfigured)?;
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|_| AuthError::JwksFetchFailed)?;
        let jwks = response
            .json::<JwkSet>()
            .await
            .map_err(|_| AuthError::JwksFetchFailed)?;
        *cache_guard = Some(JwksCache {
            fetched_at: tokio::time::Instant::now(),
            jwks: jwks.clone(),
        });
        Ok(jwks)
    }

    fn verify_tos_signature(&self, meta: &RequestMetadata) -> Result<(), AuthError> {
        let timestamp = parse_header_i64(&meta.headers, "tos-timestamp")?;
        let nonce = meta
            .headers
            .get("tos-nonce")
            .ok_or(AuthError::TosSignatureMissing)?
            .to_string();
        let pubkey_hex = meta
            .headers
            .get("tos-public-key")
            .ok_or(AuthError::TosSignatureMissing)?;
        let signature_hex = meta
            .headers
            .get("tos-signature")
            .ok_or(AuthError::TosSignatureMissing)?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let skew = (now - timestamp).abs();
        if skew > self.config.tos_skew_secs {
            return Err(AuthError::TosSignatureExpired);
        }

        // Check nonce uniqueness without storing (stores after signature verification)
        self.check_nonce_unique(&nonce, now)?;

        let body_hash = hex::encode(Sha256::digest(&meta.body));
        let canonical = format!(
            "{}\n{}\n{}\n{}\n{}\n{}",
            meta.method.to_uppercase(),
            meta.path,
            meta.query,
            timestamp,
            nonce,
            body_hash
        );

        let signature =
            Signature::from_hex(signature_hex).map_err(|_| AuthError::TosSignatureInvalid)?;

        let pubkey_bytes = hex::decode(pubkey_hex).map_err(|_| AuthError::TosPublicKeyInvalid)?;
        if pubkey_bytes.len() != 32 {
            return Err(AuthError::TosPublicKeyInvalid);
        }
        let compressed = CompressedRistretto::from_slice(&pubkey_bytes)
            .map_err(|_| AuthError::TosPublicKeyInvalid)?;
        let compressed_key = CompressedPublicKey::new(compressed);
        let public_key = compressed_key
            .decompress()
            .map_err(|_| AuthError::TosPublicKeyInvalid)?;

        if signature.verify(canonical.as_bytes(), &public_key) {
            // Only store nonce after successful signature verification
            self.store_nonce(&nonce, now);
            Ok(())
        } else {
            Err(AuthError::TosSignatureInvalid)
        }
    }

    fn verify_tos_signature_and_get_pubkey(
        &self,
        meta: &RequestMetadata,
    ) -> Result<tos_common::crypto::PublicKey, AuthError> {
        let timestamp = parse_header_i64(&meta.headers, "tos-timestamp")?;
        let nonce = meta
            .headers
            .get("tos-nonce")
            .ok_or(AuthError::TosSignatureMissing)?
            .to_string();
        let pubkey_hex = meta
            .headers
            .get("tos-public-key")
            .ok_or(AuthError::TosSignatureMissing)?;
        let signature_hex = meta
            .headers
            .get("tos-signature")
            .ok_or(AuthError::TosSignatureMissing)?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let skew = (now - timestamp).abs();
        if skew > self.config.tos_skew_secs {
            return Err(AuthError::TosSignatureExpired);
        }

        // Check nonce uniqueness without storing (stores after signature verification)
        self.check_nonce_unique(&nonce, now)?;

        let body_hash = hex::encode(Sha256::digest(&meta.body));
        let canonical = format!(
            "{}\n{}\n{}\n{}\n{}\n{}",
            meta.method.to_uppercase(),
            meta.path,
            meta.query,
            timestamp,
            nonce,
            body_hash
        );

        let signature =
            Signature::from_hex(signature_hex).map_err(|_| AuthError::TosSignatureInvalid)?;

        let pubkey_bytes = hex::decode(pubkey_hex).map_err(|_| AuthError::TosPublicKeyInvalid)?;
        if pubkey_bytes.len() != 32 {
            return Err(AuthError::TosPublicKeyInvalid);
        }
        let compressed = CompressedRistretto::from_slice(&pubkey_bytes)
            .map_err(|_| AuthError::TosPublicKeyInvalid)?;
        let compressed_key = CompressedPublicKey::new(compressed);
        let decompressed = compressed_key
            .decompress()
            .map_err(|_| AuthError::TosPublicKeyInvalid)?;

        if signature.verify(canonical.as_bytes(), &decompressed) {
            // Only store nonce after successful signature verification
            self.store_nonce(&nonce, now);
            Ok(compressed_key)
        } else {
            Err(AuthError::TosSignatureInvalid)
        }
    }

    /// Check if nonce is unique without storing (to prevent memory exhaustion DoS)
    fn check_nonce_unique(&self, nonce: &str, now: i64) -> Result<(), AuthError> {
        let mut guard = self.nonces.lock().map_err(|_| AuthError::TosNonceInvalid)?;
        let ttl = self.config.tos_nonce_ttl_secs;
        guard.retain(|_, ts| now.saturating_sub(*ts) <= ttl);
        if guard.contains_key(nonce) {
            return Err(AuthError::TosNonceInvalid);
        }
        Ok(())
    }

    /// Store nonce after successful signature verification
    fn store_nonce(&self, nonce: &str, now: i64) {
        const MAX_NONCES: usize = 10000;
        if let Ok(mut guard) = self.nonces.lock() {
            // Enforce max cache size to prevent memory growth
            if guard.len() >= MAX_NONCES {
                // Remove oldest entries
                let mut entries: Vec<_> = guard.iter().map(|(k, v)| (k.clone(), *v)).collect();
                entries.sort_by_key(|(_, ts)| *ts);
                let to_remove: Vec<_> = entries
                    .iter()
                    .take(entries.len().saturating_sub(MAX_NONCES / 2))
                    .map(|(k, _)| k.clone())
                    .collect();
                for k in to_remove {
                    guard.remove(&k);
                }
            }
            guard.insert(nonce.to_string(), now);
        }
    }
}

fn extract_bearer_token(headers: &HashMap<String, String>) -> Option<&str> {
    let auth = headers.get("authorization")?;
    let auth = auth.trim();
    if let Some(token) = auth.strip_prefix("Bearer ") {
        return Some(token.trim());
    }
    if let Some(token) = auth.strip_prefix("bearer ") {
        return Some(token.trim());
    }
    None
}

fn has_tos_signature_headers(headers: &HashMap<String, String>) -> bool {
    headers.contains_key("tos-timestamp")
        && headers.contains_key("tos-nonce")
        && headers.contains_key("tos-public-key")
        && headers.contains_key("tos-signature")
}

fn parse_header_i64(headers: &HashMap<String, String>, key: &str) -> Result<i64, AuthError> {
    let value = headers.get(key).ok_or(AuthError::TosSignatureMissing)?;
    value
        .parse::<i64>()
        .map_err(|_| AuthError::TosTimestampInvalid)
}
