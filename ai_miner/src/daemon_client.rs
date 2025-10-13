use anyhow::{Result, anyhow};
use reqwest::Client;
use serde_json::{Value, json};
use url::Url;
use log::{debug, info, warn};
use std::time::Duration;
use tokio::time::sleep;

use tos_common::{
    crypto::Hash,
    transaction::Transaction,
    serializer::Serializer,
};

/// Configuration for daemon client retries and timeouts
#[derive(Debug, Clone)]
pub struct DaemonClientConfig {
    pub request_timeout: Duration,
    pub max_retries: u32,
    pub retry_delay: Duration,
    pub connection_timeout: Duration,
}

impl Default for DaemonClientConfig {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(30),
            max_retries: 3,
            retry_delay: Duration::from_millis(1000),
            connection_timeout: Duration::from_secs(10),
        }
    }
}

/// Health status information for the daemon
#[derive(Debug, Clone)]
pub struct DaemonHealthStatus {
    pub is_healthy: bool,
    pub version: Option<String>,
    pub response_time: Duration,
    pub error_message: Option<String>,
    pub peer_count: Option<usize>,
    pub mempool_size: Option<usize>,
}

/// Daemon client for AI mining operations
pub struct DaemonClient {
    client: Client,
    base_url: Url,
    config: DaemonClientConfig,
}

/// JSON-RPC request structure
#[derive(Debug, serde::Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: u32,
    method: String,
    params: Value,
}

/// JSON-RPC response structure
#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: u32,
    result: Option<Value>,
    error: Option<JsonRpcError>,
}

/// JSON-RPC error structure
#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)]
struct JsonRpcError {
    code: i32,
    message: String,
    data: Option<Value>,
}

#[allow(dead_code)]
impl DaemonClient {
    /// Create a new daemon client with default configuration
    pub fn new(daemon_address: &str) -> Result<Self> {
        Self::with_config(daemon_address, DaemonClientConfig::default())
    }

    /// Create a new daemon client with custom configuration
    pub fn with_config(daemon_address: &str, config: DaemonClientConfig) -> Result<Self> {
        let base_url = if daemon_address.starts_with("http://") || daemon_address.starts_with("https://") {
            Url::parse(daemon_address)?
        } else {
            Url::parse(&format!("http://{}", daemon_address))?
        };

        let client = Client::builder()
            .timeout(config.request_timeout)
            .connect_timeout(config.connection_timeout)
            .build()?;

        Ok(Self {
            client,
            base_url,
            config,
        })
    }

    /// Make a JSON-RPC request to the daemon with retry logic
    async fn make_request(&self, method: &str, params: Value) -> Result<Value> {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: rand::random::<u32>(),
            method: method.to_string(),
            params,
        };

        let url = self.base_url.join("json_rpc")?;
        debug!("Making JSON-RPC request to {}: {}", url, method);

        let mut last_error = None;

        for attempt in 0..=self.config.max_retries {
            if attempt > 0 {
                warn!("Retrying request to {} (attempt {}/{})", url, attempt, self.config.max_retries);
                sleep(self.config.retry_delay).await;
            }

            match self.make_single_request(&url, &request).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    last_error = Some(e);

                    // Don't retry on certain types of errors
                    if let Some(ref err) = last_error {
                        let err_msg = err.to_string().to_lowercase();
                        if err_msg.contains("invalid") ||
                           err_msg.contains("malformed") ||
                           err_msg.contains("unauthorized") ||
                           err_msg.contains("forbidden") {
                            debug!("Not retrying due to non-retryable error: {}", err);
                            break;
                        }
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("Unknown error during request")))
    }

    /// Make a single JSON-RPC request without retry logic
    async fn make_single_request(&self, url: &Url, request: &JsonRpcRequest) -> Result<Value> {
        let response = self.client
            .post(url.clone())
            .json(request)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    anyhow!("Request timeout after {:?}", self.config.request_timeout)
                } else if e.is_connect() {
                    anyhow!("Connection failed: {}", e)
                } else {
                    anyhow!("Network error: {}", e)
                }
            })?;

        if !response.status().is_success() {
            return Err(anyhow!("HTTP error {}: {}",
                response.status().as_u16(),
                response.status().canonical_reason().unwrap_or("Unknown error")
            ));
        }

        let rpc_response: JsonRpcResponse = response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse JSON response: {}", e))?;

        if let Some(error) = rpc_response.error {
            return Err(anyhow!("RPC error {}: {}", error.code, error.message));
        }

        rpc_response.result.ok_or_else(|| anyhow!("No result in response"))
    }

    /// Get daemon version information
    pub async fn get_version(&self) -> Result<String> {
        let result = self.make_request("get_version", Value::Null).await?;
        Ok(result.as_str().unwrap_or("unknown").to_string())
    }

    /// Get current blockchain blue score
    pub async fn get_blue_score(&self) -> Result<u64> {
        let result = self.make_request("get_blue_score", Value::Null).await?;
        Ok(result.as_u64().unwrap_or(0))
    }

    /// Get current blockchain info
    pub async fn get_info(&self) -> Result<Value> {
        self.make_request("get_info", Value::Null).await
    }

    /// Submit a transaction to the mempool
    pub async fn submit_transaction(&self, tx: &Transaction) -> Result<Hash> {
        let tx_hex = hex::encode(&tx.to_bytes());
        let params = json!({
            "tx_hex": tx_hex
        });

        let result = self.make_request("submit_transaction", params).await?;

        // The result should contain the transaction hash
        let tx_hash_hex = result.as_str()
            .ok_or_else(|| anyhow!("Expected transaction hash string"))?;

        let tx_hash_bytes = hex::decode(tx_hash_hex)?;
        if tx_hash_bytes.len() != 32 {
            return Err(anyhow!("Invalid transaction hash length"));
        }

        let mut hash_array = [0u8; 32];
        hash_array.copy_from_slice(&tx_hash_bytes);
        Ok(Hash::new(hash_array))
    }

    /// Get transaction by hash
    pub async fn get_transaction(&self, tx_hash: &Hash) -> Result<Value> {
        let params = json!({
            "tx_hash": hex::encode(tx_hash.as_bytes())
        });

        self.make_request("get_transaction", params).await
    }

    /// Get mempool size
    pub async fn get_mempool_size(&self) -> Result<usize> {
        let result = self.make_request("get_mempool_size", Value::Null).await?;
        Ok(result.as_u64().unwrap_or(0) as usize)
    }

    /// Check if daemon is connected to peers
    pub async fn get_peers(&self) -> Result<Vec<Value>> {
        let result = self.make_request("get_peers", Value::Null).await?;
        Ok(result.as_array().unwrap_or(&Vec::new()).clone())
    }

    /// Perform a comprehensive health check of the daemon
    pub async fn health_check(&self) -> Result<DaemonHealthStatus> {
        let start_time = std::time::Instant::now();

        // Test basic connectivity
        let version = match self.get_version().await {
            Ok(v) => v,
            Err(e) => return Ok(DaemonHealthStatus {
                is_healthy: false,
                version: None,
                response_time: start_time.elapsed(),
                error_message: Some(format!("Version check failed: {}", e)),
                peer_count: None,
                mempool_size: None,
            }),
        };

        // Test additional endpoints
        let peer_count = self.get_peers().await.ok().map(|peers| peers.len());
        let mempool_size = self.get_mempool_size().await.ok();

        let response_time = start_time.elapsed();

        Ok(DaemonHealthStatus {
            is_healthy: true,
            version: Some(version),
            response_time,
            error_message: None,
            peer_count,
            mempool_size,
        })
    }

    /// Test connection to daemon (legacy method)
    pub async fn test_connection(&self) -> Result<()> {
        info!("Testing connection to daemon at {}", self.base_url);
        let health = self.health_check().await?;

        if health.is_healthy {
            info!("Successfully connected to daemon version: {}",
                  health.version.as_deref().unwrap_or("unknown"));
            info!("Daemon response time: {:?}", health.response_time);
        } else {
            return Err(anyhow!("Daemon health check failed: {}",
                             health.error_message.unwrap_or_else(|| "Unknown error".to_string())));
        }

        Ok(())
    }

    /// Get account nonce for a specific address
    pub async fn get_nonce(&self, address: &str) -> Result<u64> {
        let params = json!({
            "address": address
        });

        let result = self.make_request("get_nonce", params).await?;
        Ok(result.as_u64().unwrap_or(0))
    }

    /// Get account balance for a specific address
    pub async fn get_balance(&self, address: &str) -> Result<u64> {
        let params = json!({
            "address": address
        });

        let result = self.make_request("get_balance", params).await?;
        Ok(result.as_u64().unwrap_or(0))
    }

    /// Get network statistics
    pub async fn get_network_stats(&self) -> Result<Value> {
        self.make_request("get_network_stats", Value::Null).await
    }

    /// Get block by height
    pub async fn get_block_by_height(&self, height: u64) -> Result<Value> {
        let params = json!({
            "height": height
        });

        self.make_request("get_block_by_height", params).await
    }

    /// Get block by hash
    pub async fn get_block_by_hash(&self, block_hash: &Hash) -> Result<Value> {
        let params = json!({
            "hash": hex::encode(block_hash.as_bytes())
        });

        self.make_request("get_block_by_hash", params).await
    }

    /// Get transaction pool information
    pub async fn get_tx_pool_info(&self) -> Result<Value> {
        self.make_request("get_tx_pool_info", Value::Null).await
    }

    /// Get transaction status
    pub async fn get_tx_status(&self, tx_hash: &Hash) -> Result<Value> {
        let params = json!({
            "tx_hash": hex::encode(tx_hash.as_bytes())
        });

        self.make_request("get_tx_status", params).await
    }

    /// Get AI mining specific information
    pub async fn get_ai_mining_info(&self) -> Result<Value> {
        self.make_request("get_ai_mining_info", Value::Null).await
    }

    /// Get AI mining tasks (published tasks)
    pub async fn get_ai_mining_tasks(&self, limit: Option<u64>) -> Result<Value> {
        let params = if let Some(limit) = limit {
            json!({ "limit": limit })
        } else {
            Value::Null
        };

        self.make_request("get_ai_mining_tasks", params).await
    }

    /// Get AI mining task by ID
    pub async fn get_ai_mining_task(&self, task_id: &Hash) -> Result<Value> {
        let params = json!({
            "task_id": hex::encode(task_id.as_bytes())
        });

        self.make_request("get_ai_mining_task", params).await
    }

    /// Get AI mining answers for a task
    pub async fn get_ai_mining_answers(&self, task_id: &Hash) -> Result<Value> {
        let params = json!({
            "task_id": hex::encode(task_id.as_bytes())
        });

        self.make_request("get_ai_mining_answers", params).await
    }

    /// Get AI mining validations for an answer
    pub async fn get_ai_mining_validations(&self, answer_id: &Hash) -> Result<Value> {
        let params = json!({
            "answer_id": hex::encode(answer_id.as_bytes())
        });

        self.make_request("get_ai_mining_validations", params).await
    }

    /// Get miner statistics
    pub async fn get_miner_stats(&self, miner_address: &str) -> Result<Value> {
        let params = json!({
            "address": miner_address
        });

        self.make_request("get_miner_stats", params).await
    }

    /// Estimate transaction fee
    pub async fn estimate_fee(&self, tx_data: &str) -> Result<u64> {
        let params = json!({
            "tx_data": tx_data
        });

        let result = self.make_request("estimate_fee", params).await?;
        Ok(result.as_u64().unwrap_or(0))
    }

    /// Get current network difficulty
    pub async fn get_difficulty(&self) -> Result<Value> {
        self.make_request("get_difficulty", Value::Null).await
    }

    /// Get daemon sync status
    pub async fn get_sync_status(&self) -> Result<Value> {
        self.make_request("get_sync_status", Value::Null).await
    }

    /// Get connected peers information
    pub async fn get_peers_info(&self) -> Result<Value> {
        self.make_request("get_peers_info", Value::Null).await
    }

    /// Get the client configuration
    pub fn config(&self) -> &DaemonClientConfig {
        &self.config
    }
}

impl std::fmt::Debug for DaemonClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonClient")
            .field("base_url", &self.base_url)
            .finish()
    }
}