//! Factory Deployment Service
//!
//! This is a minimal off-chain service that listens for deployment requests
//! and automatically deploys contracts to the TOS blockchain.
//!
//! # Architecture
//!
//! ```text
//! Factory Contract (on-chain)
//!   ↓ emits "DeploymentRequested" event
//!   ↓
//! This Service (off-chain)
//!   ↓ listens for events
//!   ↓ loads bytecode from local storage
//!   ↓ sends DeployContract transaction
//!   ↓ marks deployment as complete
//! ```
//!
//! # Usage
//!
//! ```bash
//! export FACTORY_ADDRESS=tos1factory_address
//! export TOS_RPC_URL=http://localhost:8080
//! export WALLET_PATH=./factory-owner.key
//! export BYTECODE_DIR=./bytecodes
//!
//! cargo run --release
//! ```

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;

// ============================================================================
// Configuration
// ============================================================================

#[derive(Debug)]
struct Config {
    factory_address: String,
    rpc_url: String,
    wallet_path: String,
    bytecode_dir: String,
}

impl Config {
    fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            factory_address: std::env::var("FACTORY_ADDRESS")
                .unwrap_or_else(|_| "tos1factory_default".to_string()),
            rpc_url: std::env::var("TOS_RPC_URL")
                .unwrap_or_else(|_| "http://localhost:8080".to_string()),
            wallet_path: std::env::var("WALLET_PATH")
                .unwrap_or_else(|_| "./factory-owner.key".to_string()),
            bytecode_dir: std::env::var("BYTECODE_DIR")
                .unwrap_or_else(|_| "./bytecodes".to_string()),
        })
    }
}

// ============================================================================
// Data Structures
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
struct DeploymentEvent {
    deployer: String,
    salt: String,
    bytecode_hash: String,
    predicted_address: String,
}

#[derive(Debug)]
struct BytecodeStorage {
    bytecodes: HashMap<[u8; 32], Vec<u8>>,
}

impl BytecodeStorage {
    fn new() -> Self {
        Self {
            bytecodes: HashMap::new(),
        }
    }

    fn load_from_directory<P: AsRef<Path>>(
        &mut self,
        dir: P,
    ) -> Result<(), Box<dyn std::error::Error>> {
        println!("Loading bytecodes from: {}", dir.as_ref().display());

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            // Only load .so files (TAKO VM bytecode)
            if path.extension().and_then(|s| s.to_str()) == Some("so") {
                let bytecode = std::fs::read(&path)?;
                let hash = Self::compute_hash(&bytecode);

                self.bytecodes.insert(hash, bytecode.clone());

                println!(
                    "  ✓ Loaded: {} ({} bytes, hash: {})",
                    path.file_name().unwrap().to_str().unwrap(),
                    bytecode.len(),
                    hex::encode(&hash[..8]) // Show first 8 bytes
                );
            }
        }

        println!("Total bytecodes loaded: {}", self.bytecodes.len());
        Ok(())
    }

    fn get_bytecode(&self, hash: &[u8; 32]) -> Option<&Vec<u8>> {
        self.bytecodes.get(hash)
    }

    fn compute_hash(bytecode: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(bytecode);
        let result = hasher.finalize();

        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);
        hash
    }
}

// ============================================================================
// Mock TOS SDK (for demonstration)
// ============================================================================

/// In a real implementation, this would be the actual TOS SDK
/// For this example, we provide a minimal mock interface
struct TosClient {
    rpc_url: String,
}

impl TosClient {
    fn new(rpc_url: String) -> Self {
        Self { rpc_url }
    }

    /// Listen for events from a contract
    ///
    /// In real implementation, this would connect to TOS RPC and stream events
    async fn listen_events(
        &self,
        _contract_address: &str,
        _event_name: &str,
    ) -> Result<EventStream, Box<dyn std::error::Error>> {
        println!("Connecting to TOS RPC: {}", self.rpc_url);
        println!("Listening for DeploymentRequested events...");
        println!();
        Ok(EventStream::new())
    }

    /// Deploy a contract to the blockchain
    ///
    /// In real implementation, this would send a DeployContract transaction
    async fn deploy_contract(
        &self,
        bytecode: &[u8],
        salt: &[u8; 32],
    ) -> Result<String, Box<dyn std::error::Error>> {
        println!("  → Sending DeployContract transaction...");
        println!("    Bytecode size: {} bytes", bytecode.len());
        println!("    Salt: {}", hex::encode(salt));

        // Simulate transaction
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

        let tx_hash = format!("0x{}", hex::encode(&salt[..16]));
        println!("  ✓ Transaction sent: {}", tx_hash);

        Ok(tx_hash)
    }

    /// Mark deployment as completed in factory contract
    async fn mark_deployed(
        &self,
        _factory_address: &str,
        salt: &[u8; 32],
    ) -> Result<(), Box<dyn std::error::Error>> {
        println!("  → Marking deployment as complete...");
        println!("    Salt: {}", hex::encode(salt));

        // Simulate transaction
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        println!("  ✓ Deployment marked as complete");
        Ok(())
    }
}

/// Mock event stream
struct EventStream;

impl EventStream {
    fn new() -> Self {
        Self
    }

    /// Get next event (blocks until event arrives)
    ///
    /// In real implementation, this would stream events from blockchain
    async fn next(&mut self) -> Option<DeploymentEvent> {
        // For this example, we simulate events
        // In production, this would come from blockchain RPC
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

        // Return None to indicate no more events (for demo)
        None
    }
}

// ============================================================================
// Deployment Handler
// ============================================================================

struct DeploymentHandler {
    client: TosClient,
    storage: BytecodeStorage,
    factory_address: String,
}

impl DeploymentHandler {
    fn new(
        client: TosClient,
        storage: BytecodeStorage,
        factory_address: String,
    ) -> Self {
        Self {
            client,
            storage,
            factory_address,
        }
    }

    async fn handle_event(
        &self,
        event: DeploymentEvent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("📦 New Deployment Request");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("Deployer: {}", event.deployer);
        println!("Salt: {}", event.salt);
        println!("Bytecode Hash: {}", event.bytecode_hash);
        println!("Predicted Address: {}", event.predicted_address);
        println!();

        // Parse bytecode hash
        let bytecode_hash = hex::decode(&event.bytecode_hash)?;
        if bytecode_hash.len() != 32 {
            return Err("Invalid bytecode hash length".into());
        }
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&bytecode_hash);

        // Get bytecode from storage
        let bytecode = self
            .storage
            .get_bytecode(&hash)
            .ok_or("Bytecode not found in storage")?;

        println!("✓ Found bytecode in storage ({} bytes)", bytecode.len());
        println!();

        // Parse salt
        let salt_bytes = hex::decode(&event.salt)?;
        if salt_bytes.len() != 32 {
            return Err("Invalid salt length".into());
        }
        let mut salt = [0u8; 32];
        salt.copy_from_slice(&salt_bytes);

        // Deploy contract
        let tx_hash = self.client.deploy_contract(bytecode, &salt).await?;
        println!();

        // Mark as deployed
        self.client
            .mark_deployed(&self.factory_address, &salt)
            .await?;
        println!();

        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("✅ Deployment Completed Successfully");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("Transaction: {}", tx_hash);
        println!("Contract Address: {}", event.predicted_address);
        println!();
        println!();

        Ok(())
    }

    async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        println!("Starting event listener...");
        println!();

        let mut event_stream = self
            .client
            .listen_events(&self.factory_address, "DeploymentRequested")
            .await?;

        // Process events
        while let Some(event) = event_stream.next().await {
            match self.handle_event(event).await {
                Ok(_) => {}
                Err(e) => {
                    eprintln!("❌ Error handling deployment: {}", e);
                    eprintln!();
                }
            }
        }

        Ok(())
    }
}

// ============================================================================
// Main Entry Point
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Print banner
    println!("╔═══════════════════════════════════════════════╗");
    println!("║   TOS Contract Factory Deployment Service    ║");
    println!("╚═══════════════════════════════════════════════╝");
    println!();

    // Load configuration
    let config = Config::from_env()?;
    println!("Configuration:");
    println!("  Factory: {}", config.factory_address);
    println!("  RPC URL: {}", config.rpc_url);
    println!("  Wallet: {}", config.wallet_path);
    println!("  Bytecode Dir: {}", config.bytecode_dir);
    println!();

    // Load bytecodes
    let mut storage = BytecodeStorage::new();
    storage.load_from_directory(&config.bytecode_dir)?;
    println!();

    // Create client
    let client = TosClient::new(config.rpc_url);

    // Create handler
    let handler = DeploymentHandler::new(client, storage, config.factory_address);

    // Run service
    println!("🚀 Service is running...");
    println!("   Press Ctrl+C to stop");
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();

    handler.run().await?;

    Ok(())
}
