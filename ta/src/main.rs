//! TOS Admin (ta) - Daemon Control Tool
//!
//! A lightweight CLI tool for TOS daemon management, similar to `bitcoin-cli`.
//!
//! # Usage
//!
//! ```bash
//! # Stop daemon gracefully
//! ta stop
//!
//! # Check daemon status (compact)
//! ta status
//!
//! # Get full blockchain info
//! ta info
//! ```

use clap::{Parser, Subcommand};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use std::process::ExitCode;
use std::time::Duration;

/// TOS Admin - Daemon Control Tool
#[derive(Parser)]
#[command(name = "ta")]
#[command(about = "TOS Admin - Daemon Control Tool")]
#[command(version)]
struct Cli {
    /// RPC server address
    #[arg(short, long, default_value = "127.0.0.1:8080")]
    rpc_address: String,

    /// Request timeout in seconds
    #[arg(short, long, default_value = "30")]
    timeout: u64,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Stop the daemon gracefully
    Stop {
        /// Shutdown timeout in seconds
        #[arg(short = 's', long, default_value = "30")]
        timeout: u64,
    },
    /// Get daemon status (compact)
    Status,
    /// Get full blockchain info
    Info,
}

/// RPC response structure
#[derive(Deserialize)]
struct RpcResponse {
    result: Option<Value>,
    error: Option<RpcError>,
}

/// RPC error structure
#[derive(Deserialize)]
struct RpcError {
    code: i32,
    message: String,
}

/// Application error type
#[derive(Debug)]
enum AppError {
    /// Network/HTTP error
    Network(String),
    /// RPC returned an error
    Rpc { code: i32, message: String },
    /// JSON parsing error
    Json(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::Network(msg) => write!(f, "Network error: {}", msg),
            AppError::Rpc { code, message } => write!(f, "RPC error ({}): {}", code, message),
            AppError::Json(msg) => write!(f, "JSON error: {}", msg),
        }
    }
}

impl std::error::Error for AppError {}

/// Call RPC method on the daemon
async fn call_rpc(
    client: &Client,
    address: &str,
    method: &str,
    params: Value,
) -> Result<Value, AppError> {
    let url = format!("http://{}/json_rpc", address);
    let body = json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": 1
    });

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Network(e.to_string()))?;

    let rpc_response: RpcResponse = response
        .json()
        .await
        .map_err(|e| AppError::Json(e.to_string()))?;

    if let Some(error) = rpc_response.error {
        return Err(AppError::Rpc {
            code: error.code,
            message: error.message,
        });
    }

    rpc_response
        .result
        .ok_or_else(|| AppError::Json("Missing result field in response".to_string()))
}

/// Execute the stop command
async fn execute_stop(client: &Client, address: &str, timeout: u64) -> Result<Value, AppError> {
    call_rpc(
        client,
        address,
        "shutdown",
        json!({
            "confirm": true,
            "timeout_seconds": timeout
        }),
    )
    .await
}

/// Execute the status command
async fn execute_status(client: &Client, address: &str) -> Result<Value, AppError> {
    let info = call_rpc(client, address, "get_info", json!({})).await?;

    // Return compact status with only essential fields
    Ok(json!({
        "height": info.get("height"),
        "topoheight": info.get("topoheight"),
        "network": info.get("network"),
        "version": info.get("version")
    }))
}

/// Execute the info command
async fn execute_info(client: &Client, address: &str) -> Result<Value, AppError> {
    call_rpc(client, address, "get_info", json!({})).await
}

/// Run the application
async fn run() -> Result<(), AppError> {
    let cli = Cli::parse();

    let client = Client::builder()
        .timeout(Duration::from_secs(cli.timeout))
        .build()
        .map_err(|e| AppError::Network(format!("Failed to create HTTP client: {}", e)))?;

    let result = match cli.command {
        Commands::Stop { timeout } => execute_stop(&client, &cli.rpc_address, timeout).await?,
        Commands::Status => execute_status(&client, &cli.rpc_address).await?,
        Commands::Info => execute_info(&client, &cli.rpc_address).await?,
    };

    // Pretty print the result
    let output = serde_json::to_string_pretty(&result)
        .map_err(|e| AppError::Json(format!("Failed to format output: {}", e)))?;

    println!("{}", output);
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {}", e);
            ExitCode::FAILURE
        }
    }
}
