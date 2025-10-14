use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::fs;
use log::{debug, info, warn};

use tos_common::{
    crypto::{Hash, PublicKey},
    ai_mining::{AIMiningPayload, DifficultyLevel},
    network::Network,
};

use crate::transaction_builder::AIMiningTransactionMetadata;

/// AI Mining task state
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskState {
    /// Task published and waiting for answers
    Published,
    /// Task has received answers, waiting for validation
    AnswersReceived,
    /// Task validation completed
    Validated,
    /// Task expired without completion
    Expired,
}

/// AI Mining task information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInfo {
    pub task_id: String,
    pub reward_amount: u64,
    pub difficulty: DifficultyLevel,
    pub deadline: u64,
    pub state: TaskState,
    pub created_at: u64,
    pub updated_at: u64,
}

/// Miner registration information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinerInfo {
    pub miner_address: String,
    pub registration_fee: u64,
    pub registered_at: u64,
    pub is_active: bool,
    pub total_tasks_published: u64,
    pub total_answers_submitted: u64,
    pub total_validations_performed: u64,
}

/// Transaction history record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionRecord {
    pub tx_hash: Option<String>,
    pub payload_type: String,
    pub estimated_fee: u64,
    pub actual_fee: Option<u64>,
    pub status: TransactionStatus,
    pub created_at: u64,
    pub confirmed_at: Option<u64>,
    pub block_height: Option<u64>,
}

/// Transaction status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransactionStatus {
    /// Transaction created but not yet broadcast
    Created,
    /// Transaction broadcast to mempool
    Broadcast,
    /// Transaction confirmed in block
    Confirmed,
    /// Transaction failed or rejected
    Failed,
}

/// Persistent storage for AI mining state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIMiningState {
    pub network: Network,
    pub miner_info: Option<MinerInfo>,
    pub tasks: HashMap<String, TaskInfo>,
    pub transactions: Vec<TransactionRecord>,
    pub last_updated: u64,
}

impl Default for AIMiningState {
    fn default() -> Self {
        Self {
            network: Network::Mainnet,
            miner_info: None,
            tasks: HashMap::new(),
            transactions: Vec::new(),
            last_updated: chrono::Utc::now().timestamp() as u64,
        }
    }
}

/// Storage manager for AI mining operations
pub struct StorageManager {
    storage_path: PathBuf,
    state: AIMiningState,
}

#[allow(dead_code)]
impl StorageManager {
    /// Create a new storage manager
    pub async fn new(storage_dir: PathBuf, network: Network) -> Result<Self> {
        // Ensure storage directory exists
        if !storage_dir.exists() {
            fs::create_dir_all(&storage_dir).await?;
            if log::log_enabled!(log::Level::Info) {
                info!("Created storage directory: {:?}", storage_dir);
            }
        }

        let storage_path = storage_dir.join(format!("ai_mining_{}.json", network.to_string().to_lowercase()));

        // Load existing state or create new one
        let state = if storage_path.exists() {
            if log::log_enabled!(log::Level::Debug) {
                debug!("Loading existing state from: {:?}", storage_path);
            }
            let content = fs::read_to_string(&storage_path).await?;
            match serde_json::from_str::<AIMiningState>(&content) {
                Ok(mut loaded_state) => {
                    // Update network in case it changed
                    loaded_state.network = network;
                    loaded_state
                }
                Err(e) => {
                    if log::log_enabled!(log::Level::Warn) {
                        warn!("Failed to parse existing state file: {}. Creating new state.", e);
                    }
                    let mut new_state = AIMiningState::default();
                    new_state.network = network;
                    new_state
                }
            }
        } else {
            if log::log_enabled!(log::Level::Info) {
                info!("Creating new AI mining state for network: {:?}", network);
            }
            let mut new_state = AIMiningState::default();
            new_state.network = network;
            new_state
        };

        Ok(Self {
            storage_path,
            state,
        })
    }

    /// Save current state to disk
    pub async fn save(&mut self) -> Result<()> {
        self.state.last_updated = chrono::Utc::now().timestamp() as u64;
        let content = serde_json::to_string_pretty(&self.state)?;
        fs::write(&self.storage_path, content).await?;
        if log::log_enabled!(log::Level::Debug) {
            debug!("Saved AI mining state to: {:?}", self.storage_path);
        }
        Ok(())
    }

    /// Register a new miner
    pub async fn register_miner(&mut self, miner_address: &PublicKey, registration_fee: u64) -> Result<()> {
        let miner_info = MinerInfo {
            miner_address: hex::encode(miner_address.as_bytes()),
            registration_fee,
            registered_at: chrono::Utc::now().timestamp() as u64,
            is_active: true,
            total_tasks_published: 0,
            total_answers_submitted: 0,
            total_validations_performed: 0,
        };

        self.state.miner_info = Some(miner_info);
        self.save().await?;
        if log::log_enabled!(log::Level::Info) {
            info!("Registered miner: {}", hex::encode(miner_address.as_bytes()));
        }
        Ok(())
    }

    /// Add a new task
    pub async fn add_task(&mut self, task_id: &Hash, reward_amount: u64, difficulty: DifficultyLevel, deadline: u64) -> Result<()> {
        let task_info = TaskInfo {
            task_id: hex::encode(task_id.as_bytes()),
            reward_amount,
            difficulty,
            deadline,
            state: TaskState::Published,
            created_at: chrono::Utc::now().timestamp() as u64,
            updated_at: chrono::Utc::now().timestamp() as u64,
        };

        self.state.tasks.insert(hex::encode(task_id.as_bytes()), task_info);

        // Increment task counter
        if let Some(ref mut miner) = self.state.miner_info {
            miner.total_tasks_published += 1;
        }

        self.save().await?;
        if log::log_enabled!(log::Level::Info) {
            info!("Added task: {}", hex::encode(task_id.as_bytes()));
        }
        Ok(())
    }

    /// Update task state
    pub async fn update_task_state(&mut self, task_id: &Hash, new_state: TaskState) -> Result<()> {
        let task_key = hex::encode(task_id.as_bytes());

        if let Some(task) = self.state.tasks.get_mut(&task_key) {
            task.state = new_state;
            task.updated_at = chrono::Utc::now().timestamp() as u64;
            self.save().await?;
            if log::log_enabled!(log::Level::Info) {
                info!("Updated task {} state", task_key);
            }
        } else {
            if log::log_enabled!(log::Level::Warn) {
                warn!("Task not found: {}", task_key);
            }
        }

        Ok(())
    }

    /// Add transaction record
    pub async fn add_transaction(&mut self, metadata: &AIMiningTransactionMetadata, tx_hash: Option<Hash>) -> Result<()> {
        let payload_type = match &metadata.payload {
            AIMiningPayload::RegisterMiner { .. } => "RegisterMiner",
            AIMiningPayload::PublishTask { .. } => "PublishTask",
            AIMiningPayload::SubmitAnswer { .. } => "SubmitAnswer",
            AIMiningPayload::ValidateAnswer { .. } => "ValidateAnswer",
        }.to_string();

        let record = TransactionRecord {
            tx_hash: tx_hash.as_ref().map(|h| hex::encode(h.as_bytes())),
            payload_type: payload_type.clone(),
            estimated_fee: metadata.estimated_fee,
            actual_fee: None,
            status: if tx_hash.is_some() { TransactionStatus::Broadcast } else { TransactionStatus::Created },
            created_at: chrono::Utc::now().timestamp() as u64,
            confirmed_at: None,
            block_height: None,
        };

        self.state.transactions.push(record);

        // Update miner stats based on transaction type
        if let Some(ref mut miner) = self.state.miner_info {
            match &metadata.payload {
                AIMiningPayload::SubmitAnswer { .. } => {
                    miner.total_answers_submitted += 1;
                }
                AIMiningPayload::ValidateAnswer { .. } => {
                    miner.total_validations_performed += 1;
                }
                _ => {}
            }
        }

        self.save().await?;
        if log::log_enabled!(log::Level::Info) {
            info!("Added transaction record: {:?}", payload_type);
        }
        Ok(())
    }

    /// Get miner information
    pub fn get_miner_info(&self) -> Option<&MinerInfo> {
        self.state.miner_info.as_ref()
    }

    /// Get task information
    pub fn get_task(&self, task_id: &Hash) -> Option<&TaskInfo> {
        self.state.tasks.get(&hex::encode(task_id.as_bytes()))
    }

    /// Get all tasks
    pub fn get_all_tasks(&self) -> &HashMap<String, TaskInfo> {
        &self.state.tasks
    }

    /// Get transaction history
    pub fn get_transactions(&self) -> &Vec<TransactionRecord> {
        &self.state.transactions
    }

    /// Get recent transactions (last N)
    pub fn get_recent_transactions(&self, limit: usize) -> Vec<&TransactionRecord> {
        self.state.transactions.iter().rev().take(limit).collect()
    }

    /// Get statistics
    pub fn get_stats(&self) -> StorageStats {
        StorageStats {
            total_tasks: self.state.tasks.len(),
            total_transactions: self.state.transactions.len(),
            miner_registered: self.state.miner_info.is_some(),
            network: self.state.network,
            last_updated: self.state.last_updated,
        }
    }

    /// Clear all data (for testing or reset)
    pub async fn clear_all(&mut self) -> Result<()> {
        self.state = AIMiningState::default();
        self.state.network = self.state.network; // Preserve network
        self.save().await?;
        info!("Cleared all AI mining storage data");
        Ok(())
    }
}

/// Storage statistics
#[derive(Debug)]
pub struct StorageStats {
    pub total_tasks: usize,
    pub total_transactions: usize,
    pub miner_registered: bool,
    pub network: Network,
    pub last_updated: u64,
}