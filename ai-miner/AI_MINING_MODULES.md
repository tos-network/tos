# AI-Mining Module Implementation Analysis

## 🏗️ Architecture Design

### Module Distribution
```
tos/
├── common/          # Add AI mining transaction types
├── daemon/          # Add AI reward state management
├── miner/           # Keep unchanged - Traditional PoW
├── ai_miner/        # New - AI device miner
├── wallet/          # Support AI reward query and claiming
└── genesis/         # Keep unchanged
```

---

## 📝 Module Modification Details

### 1. **Common Module** (Core Transaction Types)

#### New File: `common/src/transaction/payload/ai_mining.rs`
```rust
use serde::{Deserialize, Serialize};
use crate::crypto::{Hash, Signature};

/// AI device registration transaction
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RegisterDevicePayload {
    pub device_pubkey: CompressedPublicKey,
    pub device_class: DeviceClass,
    pub stake_amount: Option<u64>,
}

/// AI work proof submission transaction
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AIWorkProofPayload {
    pub device_id: Hash,
    pub task_epoch: u32,
    pub proof_data: ProofData,
    pub device_signature: Signature,
}

/// AI reward claiming transaction
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ClaimAIRewardsPayload {
    pub device_id: Hash,
    pub claim_epochs: Vec<u32>,  // Which epochs to claim rewards for
    pub device_signature: Signature,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum DeviceClass {
    Arduino,
    STM32,
    ESP8266,
    ESP32,
    Teensy,
    RaspberryPi,
    OrangePi,
    PC,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ProofData {
    CoverageProof {
        location_samples: Vec<GPSCoordinate>,
        duration: u32,
        signal_strength: Vec<i32>,
    },
    RelayProof {
        messages_relayed: Vec<Hash>,
        bandwidth_used: u64,
    },
    InferenceProof {
        model_hash: Hash,
        input_hash: Hash,
        output_hash: Hash,
        computation_time: u32,
    },
}
```

#### Modify: `common/src/transaction/mod.rs`
```rust
// Add to TransactionType enum:
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum TransactionType {
    // ... existing types
    RegisterDevice(RegisterDevicePayload),      // New
    AIWorkProof(AIWorkProofPayload),           // New
    ClaimAIRewards(ClaimAIRewardsPayload),     // New
}
```

---

### 2. **Daemon Module** (Blockchain State Management)

#### New File: `daemon/src/core/ai_mining/mod.rs`
```rust
use std::collections::HashMap;
use crate::core::storage::Storage;

/// AI mining state manager
pub struct AIMiningManager {
    device_registry: HashMap<Hash, DeviceInfo>,
    reward_trackers: HashMap<Hash, DeviceRewardTracker>,
}

#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub device_id: Hash,
    pub owner_address: Address,
    pub device_class: DeviceClass,
    pub registration_time: Timestamp,
    pub stake_amount: u64,
    pub is_active: bool,
}

#[derive(Clone, Debug)]
pub struct DeviceRewardTracker {
    pub device_id: Hash,
    pub pending_rewards: Vec<EpochReward>,
    pub total_accumulated: u64,
    pub last_claim_time: Timestamp,
}

#[derive(Clone, Debug)]
pub struct EpochReward {
    pub epoch_timestamp: Timestamp,
    pub reward_amount: u64,
    pub task_count: u32,
    pub quality_score: f64,
    pub claimable_after: Timestamp,
}

impl AIMiningManager {
    /// Register new device
    pub fn register_device(&mut self, payload: RegisterDevicePayload, owner: Address) -> Result<()> {
        // Validate device type, create device record
    }

    /// Process AI work proof and calculate rewards
    pub fn process_work_proof(&mut self, payload: AIWorkProofPayload) -> Result<u64> {
        // Verify proof, calculate reward, accumulate to tracker
    }

    /// Process reward claims
    pub fn claim_rewards(&mut self, payload: ClaimAIRewardsPayload) -> Result<u64> {
        // Verify claimable rewards, return total amount
    }
}
```

#### Modify: `daemon/src/core/blockchain.rs`
```rust
// Add AI mining transaction processing in apply_tx
impl Blockchain {
    async fn apply_tx(&self, tx: &Transaction) -> Result<()> {
        match &tx.data {
            // ... existing transaction type handling

            TransactionType::RegisterDevice(payload) => {
                self.ai_mining_manager.register_device(payload, tx.owner)?;
            },

            TransactionType::AIWorkProof(payload) => {
                let reward = self.ai_mining_manager.process_work_proof(payload)?;
                // Accumulate reward to device account
            },

            TransactionType::ClaimAIRewards(payload) => {
                let amount = self.ai_mining_manager.claim_rewards(payload)?;
                // Transfer reward to device owner
            },
        }
    }
}
```

#### New Storage: `daemon/src/core/storage/providers/ai_mining.rs`
```rust
/// AI mining related storage interface
pub trait AIMiningProvider {
    /// Device registry
    async fn get_device_info(&self, device_id: &Hash) -> Result<Option<DeviceInfo>>;
    async fn set_device_info(&self, device_id: &Hash, info: &DeviceInfo) -> Result<()>;

    /// Reward trackers
    async fn get_reward_tracker(&self, device_id: &Hash) -> Result<Option<DeviceRewardTracker>>;
    async fn set_reward_tracker(&self, device_id: &Hash, tracker: &DeviceRewardTracker) -> Result<()>;

    /// Historical proof records
    async fn store_work_proof(&self, proof: &AIWorkProofPayload) -> Result<()>;
    async fn get_work_proofs(&self, device_id: &Hash, from_epoch: u32, to_epoch: u32) -> Result<Vec<AIWorkProofPayload>>;
}
```

---

### 3. **AI-Miner Module** (New Complete Module)

#### New: `ai_miner/Cargo.toml`
```toml
[package]
name = "tos_ai_miner"
version = "0.1.0"
edition = "2021"

[dependencies]
tos_common = { path = "../common", features = ["prompt", "clap"] }
tokio = { workspace = true, features = ["rt", "time"] }
serde = { workspace = true }
serde_json = { workspace = true }
clap = { workspace = true }
anyhow = { workspace = true }
log = { workspace = true }

# Device-specific dependencies
[target.'cfg(any(target_arch = "arm", target_arch = "aarch64"))'.dependencies]
rppal = "0.18"  # Raspberry Pi GPIO

[features]
default = []
arduino = []     # Arduino support
esp32 = []       # ESP32 support
raspberry = []   # Raspberry Pi support
```

#### New: `ai_miner/src/main.rs`
```rust
use clap::{Parser, Subcommand};
use tos_common::crypto::Address;

#[derive(Parser)]
#[command(name = "tos_ai_miner")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Register AI device
    Register {
        #[arg(long)]
        owner_address: Address,
        #[arg(long)]
        device_class: String,
        #[arg(long)]
        daemon_address: String,
    },

    /// Start AI mining
    Start {
        #[arg(long)]
        device_id: String,
        #[arg(long)]
        owner_key: String,
        #[arg(long)]
        daemon_address: String,
    },

    /// Claim accumulated rewards
    Claim {
        #[arg(long)]
        device_id: String,
        #[arg(long)]
        owner_key: String,
        #[arg(long)]
        daemon_address: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Register { owner_address, device_class, daemon_address } => {
            register_device(owner_address, device_class, daemon_address).await?;
        },
        Commands::Start { device_id, owner_key, daemon_address } => {
            start_ai_mining(device_id, owner_key, daemon_address).await?;
        },
        Commands::Claim { device_id, owner_key, daemon_address } => {
            claim_rewards(device_id, owner_key, daemon_address).await?;
        },
    }

    Ok(())
}
```

#### New: `ai_miner/src/tasks/mod.rs`
```rust
pub mod coverage;
pub mod relay;
pub mod inference;

use tos_common::transaction::ProofData;

pub trait AITask {
    /// Execute 30-minute AI task
    async fn execute(&self) -> anyhow::Result<ProofData>;

    /// Verify task result
    fn verify(&self, proof: &ProofData) -> bool;
}

/// Task scheduler
pub struct TaskScheduler {
    device_class: DeviceClass,
    current_task: Option<Box<dyn AITask>>,
}

impl TaskScheduler {
    pub async fn get_next_task(&mut self) -> anyhow::Result<Box<dyn AITask>> {
        match self.device_class {
            DeviceClass::Arduino | DeviceClass::STM32 => {
                Ok(Box::new(coverage::CoverageTask::new()))
            },
            DeviceClass::ESP8266 | DeviceClass::ESP32 => {
                Ok(Box::new(relay::RelayTask::new()))
            },
            DeviceClass::RaspberryPi | DeviceClass::PC => {
                Ok(Box::new(inference::InferenceTask::new()))
            },
            _ => Ok(Box::new(coverage::CoverageTask::new())),
        }
    }
}
```

---

### 4. **Wallet Module** (Add AI Reward Support)

#### Modify: `wallet/src/api/rpc.rs`
```rust
// Add AI mining related RPC interfaces
impl RpcClient {
    /// Query device registration info
    pub async fn get_device_info(&self, device_id: &Hash) -> Result<Option<DeviceInfo>> {
        // RPC call to daemon to get device info
    }

    /// Query device accumulated rewards
    pub async fn get_pending_rewards(&self, device_id: &Hash) -> Result<Vec<EpochReward>> {
        // Query claimable rewards
    }

    /// Submit device registration transaction
    pub async fn register_ai_device(&self, payload: RegisterDevicePayload) -> Result<Hash> {
        // Build and submit RegisterDevice transaction
    }

    /// Submit AI work proof
    pub async fn submit_work_proof(&self, payload: AIWorkProofPayload) -> Result<Hash> {
        // Build and submit AIWorkProof transaction
    }

    /// Claim AI rewards
    pub async fn claim_ai_rewards(&self, payload: ClaimAIRewardsPayload) -> Result<Hash> {
        // Build and submit ClaimAIRewards transaction
    }
}
```

---

## 🚀 Implementation Steps

### Phase 1: Basic Framework
1. ✅ Add AI transaction types in common
2. ✅ Add state management in daemon
3. ✅ Create ai_miner module skeleton

### Phase 2: Core Functionality
1. ✅ Implement device registration and verification
2. ✅ Implement basic AI tasks (coverage proof)
3. ✅ Implement reward calculation and accumulation

### Phase 3: Complete Ecosystem
1. ✅ Support multiple device types
2. ✅ Implement complex AI tasks
3. ✅ Optimize performance and security

---

## 💡 Key Design Principles

### Module Independence
- **AI-Miner**: Completely independent binary, doesn't affect existing miner
- **Backward Compatibility**: All existing functionality remains unchanged
- **Progressive Deployment**: Can gradually enable AI mining features

### Security
- **Device Authentication**: Each device has independent key pair
- **Anti-Cheating**: 24-hour maturation period + staking mechanism
- **Permission Separation**: AI rewards completely separated from block rewards

### Scalability
- **Modular Tasks**: Easy to add new AI task types
- **Device Classification**: Support various performance levels of devices
- **Adjustable Parameters**: Reward parameters adjustable through governance