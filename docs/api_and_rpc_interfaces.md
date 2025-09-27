# AI Mining API Interfaces and RPC Call Design

## API Architecture Overview

### 1. RPC Interface Design (daemon/src/rpc/ai_rpc.rs)

```rust
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use crate::{
    ai::{types::*, state::*, task_manager::*, miner_registry::*},
    crypto::{Hash, CompressedPublicKey},
    rpc::*,
};

/// AI Mining RPC Interface
pub struct AIRpcHandler {
    task_manager: Arc<RwLock<TaskManager>>,
    miner_registry: Arc<RwLock<MinerRegistry>>,
    enabled: bool,
}

impl AIRpcHandler {
    pub fn new(
        task_manager: Arc<RwLock<TaskManager>>,
        miner_registry: Arc<RwLock<MinerRegistry>>,
    ) -> Self {
        Self {
            task_manager,
            miner_registry,
            enabled: true,
        }
    }

    /// Get AI mining network status
    pub async fn get_ai_network_info(&self) -> Result<AINetworkInfo, RpcError> {
        if !self.enabled {
            return Err(RpcError::MethodNotEnabled("ai_get_network_info".to_string()));
        }

        let task_manager = self.task_manager.read().await;
        let miner_registry = self.miner_registry.read().await;

        let active_tasks = task_manager.get_active_tasks_count().await
            .map_err(|e| RpcError::InternalError(e.to_string()))?;

        let total_miners = miner_registry.get_total_miners_count().await
            .map_err(|e| RpcError::InternalError(e.to_string()))?;

        let network_metrics = task_manager.get_network_metrics().await
            .map_err(|e| RpcError::InternalError(e.to_string()))?;

        Ok(AINetworkInfo {
            active_tasks,
            total_miners,
            network_metrics,
            total_rewards_distributed: network_metrics.total_rewards_distributed,
            average_task_completion_time: network_metrics.average_completion_time,
            success_rate: network_metrics.overall_success_rate,
        })
    }

    /// Publish new task
    pub async fn publish_task(&self, request: PublishTaskRequest) -> Result<PublishTaskResponse, RpcError> {
        if !self.enabled {
            return Err(RpcError::MethodNotEnabled("ai_publish_task".to_string()));
        }

        // Validate request parameters
        self.validate_publish_task_request(&request)?;

        let mut task_manager = self.task_manager.write().await;

        let task_id = task_manager.publish_task(
            request.publisher,
            request.task_data,
            request.current_block_height,
        ).await.map_err(|e| RpcError::TaskError(e.to_string()))?;

        Ok(PublishTaskResponse {
            task_id,
            status: "published".to_string(),
            estimated_participants: self.estimate_participants(&request.task_data).await?,
            recommended_deadline: self.calculate_recommended_deadline(&request.task_data),
        })
    }

    /// Submit answer
    pub async fn submit_answer(&self, request: SubmitAnswerRequest) -> Result<SubmitAnswerResponse, RpcError> {
        if !self.enabled {
            return Err(RpcError::MethodNotEnabled("ai_submit_answer".to_string()));
        }

        self.validate_submit_answer_request(&request)?;

        let mut task_manager = self.task_manager.write().await;

        let submission_id = task_manager.submit_answer(
            request.submitter,
            request.submission_data,
            request.current_block_height,
        ).await.map_err(|e| RpcError::TaskError(e.to_string()))?;

        Ok(SubmitAnswerResponse {
            submission_id,
            status: "submitted".to_string(),
            validation_eta: self.estimate_validation_time(&request.submission_data).await?,
            fraud_risk_score: task_manager.get_fraud_risk_score(&submission_id).await
                .unwrap_or(0.0),
        })
    }

    /// Validate answer
    pub async fn validate_answer(&self, request: ValidateAnswerRequest) -> Result<ValidateAnswerResponse, RpcError> {
        if !self.enabled {
            return Err(RpcError::MethodNotEnabled("ai_validate_answer".to_string()));
        }

        self.validate_validate_answer_request(&request)?;

        let mut task_manager = self.task_manager.write().await;

        let validation_id = task_manager.validate_submission(
            request.validator,
            request.validation_data,
            request.current_block_height,
        ).await.map_err(|e| RpcError::TaskError(e.to_string()))?;

        Ok(ValidateAnswerResponse {
            validation_id,
            status: "validated".to_string(),
            consensus_progress: task_manager.get_consensus_progress(&request.validation_data.task_id).await
                .unwrap_or(0.0),
        })
    }

    /// Register miner
    pub async fn register_miner(&self, request: RegisterMinerRequest) -> Result<RegisterMinerResponse, RpcError> {
        if !self.enabled {
            return Err(RpcError::MethodNotEnabled("ai_register_miner".to_string()));
        }

        self.validate_register_miner_request(&request)?;

        let mut miner_registry = self.miner_registry.write().await;

        let registration_result = miner_registry.register_miner(
            request.registration_data,
            request.signature,
            request.current_block_height,
        ).await.map_err(|e| RpcError::MinerError(e.to_string()))?;

        Ok(RegisterMinerResponse {
            miner_id: registration_result.miner_id,
            registration_status: registration_result.registration_status,
            initial_reputation: registration_result.initial_reputation,
            onboarding_tasks: registration_result.recommended_tasks,
        })
    }

    /// Get task details
    pub async fn get_task(&self, task_id: Hash) -> Result<TaskDetails, RpcError> {
        if !self.enabled {
            return Err(RpcError::MethodNotEnabled("ai_get_task".to_string()));
        }

        let task_manager = self.task_manager.read().await;

        let task_state = task_manager.get_task_state(&task_id).await
            .map_err(|e| RpcError::TaskError(e.to_string()))?
            .ok_or_else(|| RpcError::TaskNotFound(task_id))?;

        let submissions = task_manager.get_task_submissions(&task_id).await
            .map_err(|e| RpcError::TaskError(e.to_string()))?;

        let validation_status = task_manager.get_task_validation_status(&task_id).await
            .map_err(|e| RpcError::TaskError(e.to_string()))?;

        Ok(TaskDetails {
            task_id,
            task_state,
            submissions,
            validation_status,
            estimated_completion: task_manager.estimate_task_completion(&task_id).await.ok(),
        })
    }

    /// Get miner profile
    pub async fn get_miner_profile(&self, miner_address: CompressedPublicKey) -> Result<MinerProfile, RpcError> {
        if !self.enabled {
            return Err(RpcError::MethodNotEnabled("ai_get_miner_profile".to_string()));
        }

        let miner_registry = self.miner_registry.read().await;

        miner_registry.get_miner_profile(&miner_address).await
            .map_err(|e| RpcError::MinerError(e.to_string()))
    }

    /// Get recommended tasks
    pub async fn get_recommended_tasks(&self, miner_address: CompressedPublicKey, limit: u32) -> Result<Vec<TaskRecommendation>, RpcError> {
        if !self.enabled {
            return Err(RpcError::MethodNotEnabled("ai_get_recommended_tasks".to_string()));
        }

        let miner_registry = self.miner_registry.read().await;
        let task_manager = self.task_manager.read().await;

        let miner_profile = miner_registry.get_miner_profile(&miner_address).await
            .map_err(|e| RpcError::MinerError(e.to_string()))?;

        task_manager.get_recommended_tasks_for_miner(&miner_profile, limit).await
            .map_err(|e| RpcError::TaskError(e.to_string()))
    }

    /// Get task list
    pub async fn list_tasks(&self, filter: TaskFilter) -> Result<TaskList, RpcError> {
        if !self.enabled {
            return Err(RpcError::MethodNotEnabled("ai_list_tasks".to_string()));
        }

        let task_manager = self.task_manager.read().await;

        let tasks = task_manager.list_tasks_with_filter(&filter).await
            .map_err(|e| RpcError::TaskError(e.to_string()))?;

        Ok(TaskList {
            tasks,
            total_count: task_manager.get_total_tasks_count(&filter).await.unwrap_or(0),
            has_more: tasks.len() as u32 >= filter.limit,
        })
    }

    /// Get leaderboard
    pub async fn get_leaderboard(&self, category: LeaderboardCategory, period: TimePeriod, limit: u32) -> Result<Vec<MinerLeaderboardEntry>, RpcError> {
        if !self.enabled {
            return Err(RpcError::MethodNotEnabled("ai_get_leaderboard".to_string()));
        }

        let miner_registry = self.miner_registry.read().await;

        miner_registry.get_miner_leaderboard(category, period, limit).await
            .map_err(|e| RpcError::MinerError(e.to_string()))
    }

    /// Get reward distribution information
    pub async fn get_reward_distribution(&self, task_id: Hash) -> Result<RewardDistribution, RpcError> {
        if !self.enabled {
            return Err(RpcError::MethodNotEnabled("ai_get_reward_distribution".to_string()));
        }

        let task_manager = self.task_manager.read().await;

        task_manager.get_reward_distribution(&task_id).await
            .map_err(|e| RpcError::TaskError(e.to_string()))?
            .ok_or_else(|| RpcError::RewardNotFound(task_id))
    }

    /// Claim reward
    pub async fn claim_reward(&self, request: ClaimRewardRequest) -> Result<ClaimRewardResponse, RpcError> {
        if !self.enabled {
            return Err(RpcError::MethodNotEnabled("ai_claim_reward".to_string()));
        }

        self.validate_claim_reward_request(&request)?;

        let mut task_manager = self.task_manager.write().await;

        let claim_result = task_manager.process_reward_claim(
            request.claimer,
            request.task_id,
            request.claim_type,
        ).await.map_err(|e| RpcError::TaskError(e.to_string()))?;

        Ok(ClaimRewardResponse {
            transaction_hash: claim_result.transaction_hash,
            amount_claimed: claim_result.amount,
            status: "claimed".to_string(),
        })
    }

    /// Submit dispute
    pub async fn submit_dispute(&self, request: SubmitDisputeRequest) -> Result<SubmitDisputeResponse, RpcError> {
        if !self.enabled {
            return Err(RpcError::MethodNotEnabled("ai_submit_dispute".to_string()));
        }

        self.validate_submit_dispute_request(&request)?;

        let mut task_manager = self.task_manager.write().await;

        let dispute_id = task_manager.submit_dispute(request.dispute_case).await
            .map_err(|e| RpcError::TaskError(e.to_string()))?;

        Ok(SubmitDisputeResponse {
            dispute_id,
            status: "submitted".to_string(),
            estimated_resolution_time: 7 * 24 * 3600, // 7 days
        })
    }

    /// Get network statistics
    pub async fn get_network_statistics(&self, period: TimePeriod) -> Result<NetworkStatistics, RpcError> {
        if !self.enabled {
            return Err(RpcError::MethodNotEnabled("ai_get_network_statistics".to_string()));
        }

        let task_manager = self.task_manager.read().await;
        let miner_registry = self.miner_registry.read().await;

        let task_stats = task_manager.get_statistics_for_period(&period).await
            .map_err(|e| RpcError::TaskError(e.to_string()))?;

        let miner_stats = miner_registry.get_statistics_for_period(&period).await
            .map_err(|e| RpcError::MinerError(e.to_string()))?;

        Ok(NetworkStatistics {
            period,
            task_statistics: task_stats,
            miner_statistics: miner_stats,
            economic_metrics: self.calculate_economic_metrics(&task_stats, &miner_stats),
        })
    }
}

// Validation methods
impl AIRpcHandler {
    fn validate_publish_task_request(&self, request: &PublishTaskRequest) -> Result<(), RpcError> {
        if request.task_data.reward_amount == 0 {
            return Err(RpcError::InvalidParameter("reward_amount cannot be zero".to_string()));
        }

        if request.task_data.deadline <= chrono::Utc::now().timestamp() as u64 {
            return Err(RpcError::InvalidParameter("deadline must be in the future".to_string()));
        }

        if request.task_data.encrypted_data.is_empty() {
            return Err(RpcError::InvalidParameter("task data cannot be empty".to_string()));
        }

        Ok(())
    }

    fn validate_submit_answer_request(&self, request: &SubmitAnswerRequest) -> Result<(), RpcError> {
        if request.submission_data.encrypted_answer.is_empty() {
            return Err(RpcError::InvalidParameter("answer cannot be empty".to_string()));
        }

        if request.submission_data.stake_amount == 0 {
            return Err(RpcError::InvalidParameter("stake amount cannot be zero".to_string()));
        }

        Ok(())
    }

    fn validate_validate_answer_request(&self, request: &ValidateAnswerRequest) -> Result<(), RpcError> {
        if request.validation_data.validator_stake == 0 {
            return Err(RpcError::InvalidParameter("validator stake cannot be zero".to_string()));
        }

        Ok(())
    }

    fn validate_register_miner_request(&self, request: &RegisterMinerRequest) -> Result<(), RpcError> {
        if request.registration_data.specializations.is_empty() {
            return Err(RpcError::InvalidParameter("must specify at least one specialization".to_string()));
        }

        if request.registration_data.initial_stake == 0 {
            return Err(RpcError::InvalidParameter("initial stake cannot be zero".to_string()));
        }

        Ok(())
    }

    fn validate_claim_reward_request(&self, request: &ClaimRewardRequest) -> Result<(), RpcError> {
        // Basic validation logic
        Ok(())
    }

    fn validate_submit_dispute_request(&self, request: &SubmitDisputeRequest) -> Result<(), RpcError> {
        if request.dispute_case.evidence.is_empty() {
            return Err(RpcError::InvalidParameter("dispute must include evidence".to_string()));
        }

        Ok(())
    }

    async fn estimate_participants(&self, task_data: &PublishTaskPayload) -> Result<u32, RpcError> {
        // Estimate number of participants based on task type and reward
        let base_participants = match &task_data.task_type {
            TaskType::CodeAnalysis { .. } => 5,
            TaskType::SecurityAudit { .. } => 3,
            TaskType::DataAnalysis { .. } => 4,
            TaskType::AlgorithmOptimization { .. } => 2,
            TaskType::LogicReasoning { .. } => 6,
            TaskType::GeneralTask { .. } => 8,
        };

        // Adjust based on reward
        let reward_multiplier = if task_data.reward_amount > 500 {
            1.5
        } else if task_data.reward_amount > 100 {
            1.2
        } else {
            1.0
        };

        Ok((base_participants as f64 * reward_multiplier) as u32)
    }

    fn calculate_recommended_deadline(&self, task_data: &PublishTaskPayload) -> u64 {
        let base_duration = match &task_data.difficulty_level {
            DifficultyLevel::Beginner => 24 * 3600,      // 1 day
            DifficultyLevel::Intermediate => 3 * 24 * 3600, // 3 days
            DifficultyLevel::Advanced => 7 * 24 * 3600,   // 7 days
            DifficultyLevel::Expert => 14 * 24 * 3600,    // 14 days
        };

        chrono::Utc::now().timestamp() as u64 + base_duration
    }
}

// Request and response type definitions
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PublishTaskRequest {
    pub publisher: CompressedPublicKey,
    pub task_data: PublishTaskPayload,
    pub current_block_height: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PublishTaskResponse {
    pub task_id: Hash,
    pub status: String,
    pub estimated_participants: u32,
    pub recommended_deadline: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SubmitAnswerRequest {
    pub submitter: CompressedPublicKey,
    pub submission_data: SubmitAnswerPayload,
    pub current_block_height: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SubmitAnswerResponse {
    pub submission_id: Hash,
    pub status: String,
    pub validation_eta: u64,
    pub fraud_risk_score: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ValidateAnswerRequest {
    pub validator: CompressedPublicKey,
    pub validation_data: ValidateAnswerPayload,
    pub current_block_height: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ValidateAnswerResponse {
    pub validation_id: Hash,
    pub status: String,
    pub consensus_progress: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RegisterMinerRequest {
    pub registration_data: RegisterMinerPayload,
    pub signature: crate::crypto::Signature,
    pub current_block_height: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RegisterMinerResponse {
    pub miner_id: CompressedPublicKey,
    pub registration_status: RegistrationStatus,
    pub initial_reputation: u32,
    pub onboarding_tasks: Vec<TaskRecommendation>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TaskDetails {
    pub task_id: Hash,
    pub task_state: TaskState,
    pub submissions: Vec<SubmissionSummary>,
    pub validation_status: ValidationStatus,
    pub estimated_completion: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TaskFilter {
    pub task_types: Option<Vec<TaskType>>,
    pub difficulty_levels: Option<Vec<DifficultyLevel>>,
    pub status: Option<Vec<TaskStatus>>,
    pub min_reward: Option<u64>,
    pub max_reward: Option<u64>,
    pub publisher: Option<CompressedPublicKey>,
    pub offset: u32,
    pub limit: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TaskList {
    pub tasks: Vec<TaskSummary>,
    pub total_count: u32,
    pub has_more: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ClaimRewardRequest {
    pub claimer: CompressedPublicKey,
    pub task_id: Hash,
    pub claim_type: RewardClaimType,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ClaimRewardResponse {
    pub transaction_hash: Hash,
    pub amount_claimed: u64,
    pub status: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SubmitDisputeRequest {
    pub dispute_case: DisputeCase,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SubmitDisputeResponse {
    pub dispute_id: Hash,
    pub status: String,
    pub estimated_resolution_time: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AINetworkInfo {
    pub active_tasks: u32,
    pub total_miners: u32,
    pub network_metrics: NetworkMetrics,
    pub total_rewards_distributed: u64,
    pub average_task_completion_time: f64,
    pub success_rate: f64,
}

// RPC error types
#[derive(Debug, Clone)]
pub enum RpcError {
    MethodNotEnabled(String),
    InvalidParameter(String),
    TaskError(String),
    MinerError(String),
    TaskNotFound(Hash),
    RewardNotFound(Hash),
    InternalError(String),
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RpcError::MethodNotEnabled(method) => write!(f, "Method not enabled: {}", method),
            RpcError::InvalidParameter(param) => write!(f, "Invalid parameter: {}", param),
            RpcError::TaskError(err) => write!(f, "Task error: {}", err),
            RpcError::MinerError(err) => write!(f, "Miner error: {}", err),
            RpcError::TaskNotFound(id) => write!(f, "Task not found: {}", id),
            RpcError::RewardNotFound(id) => write!(f, "Reward not found: {}", id),
            RpcError::InternalError(err) => write!(f, "Internal error: {}", err),
        }
    }
}

impl std::error::Error for RpcError {}
```

### 2. WebSocket Real-time API (daemon/src/rpc/ai_websocket.rs)

```rust
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use serde::{Deserialize, Serialize};
use crate::{
    ai::{types::*, events::*},
    rpc::websocket::*,
};

/// AI Mining WebSocket Handler
pub struct AIWebSocketHandler {
    event_broadcaster: broadcast::Sender<AIEvent>,
    subscription_manager: SubscriptionManager,
    connection_pool: ConnectionPool,
}

impl AIWebSocketHandler {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1000);
        Self {
            event_broadcaster: tx,
            subscription_manager: SubscriptionManager::new(),
            connection_pool: ConnectionPool::new(),
        }
    }

    /// Handle WebSocket connection
    pub async fn handle_connection(&self, connection: WebSocketConnection) -> Result<(), WebSocketError> {
        let connection_id = self.connection_pool.add_connection(connection).await?;

        // Start message handling loop
        self.message_loop(connection_id).await
    }

    /// Subscribe to task updates
    pub async fn subscribe_task_updates(
        &self,
        connection_id: ConnectionId,
        filter: TaskSubscriptionFilter,
    ) -> Result<(), WebSocketError> {
        self.subscription_manager.add_subscription(
            connection_id,
            SubscriptionType::TaskUpdates(filter),
        ).await
    }

    /// Subscribe to miner status updates
    pub async fn subscribe_miner_updates(
        &self,
        connection_id: ConnectionId,
        miner_address: CompressedPublicKey,
    ) -> Result<(), WebSocketError> {
        self.subscription_manager.add_subscription(
            connection_id,
            SubscriptionType::MinerUpdates(miner_address),
        ).await
    }

    /// Broadcast AI event
    pub async fn broadcast_event(&self, event: AIEvent) -> Result<(), WebSocketError> {
        // Send to broadcast channel
        let _ = self.event_broadcaster.send(event.clone());

        // Filter based on subscriptions and send to relevant connections
        let subscribers = self.subscription_manager.get_subscribers_for_event(&event).await;

        for connection_id in subscribers {
            if let Some(connection) = self.connection_pool.get_connection(&connection_id).await {
                let message = WebSocketMessage::Event(event.clone());
                let _ = connection.send(message).await;
            }
        }

        Ok(())
    }

    async fn message_loop(&self, connection_id: ConnectionId) -> Result<(), WebSocketError> {
        let mut receiver = self.event_broadcaster.subscribe();

        loop {
            tokio::select! {
                // Handle messages from client
                msg = self.connection_pool.receive_message(&connection_id) => {
                    match msg? {
                        Some(message) => self.handle_client_message(connection_id, message).await?,
                        None => break, // Connection closed
                    }
                },
                // Handle broadcast events
                event = receiver.recv() => {
                    match event {
                        Ok(ai_event) => {
                            if self.subscription_manager.should_send_to_connection(&connection_id, &ai_event).await {
                                let message = WebSocketMessage::Event(ai_event);
                                self.connection_pool.send_message(&connection_id, message).await?;
                            }
                        },
                        Err(_) => break, // Broadcast channel closed
                    }
                }
            }
        }

        // Cleanup connection
        self.connection_pool.remove_connection(&connection_id).await;
        self.subscription_manager.remove_all_subscriptions(&connection_id).await;

        Ok(())
    }

    async fn handle_client_message(
        &self,
        connection_id: ConnectionId,
        message: ClientMessage,
    ) -> Result<(), WebSocketError> {
        match message {
            ClientMessage::Subscribe { subscription_type } => {
                self.subscription_manager.add_subscription(connection_id, subscription_type).await?;

                let response = ServerMessage::SubscriptionConfirmed {
                    subscription_id: self.subscription_manager.get_last_subscription_id(&connection_id).await,
                };
                self.connection_pool.send_message(&connection_id, WebSocketMessage::Server(response)).await?;
            },
            ClientMessage::Unsubscribe { subscription_id } => {
                self.subscription_manager.remove_subscription(&connection_id, &subscription_id).await?;

                let response = ServerMessage::SubscriptionCancelled { subscription_id };
                self.connection_pool.send_message(&connection_id, WebSocketMessage::Server(response)).await?;
            },
            ClientMessage::GetTaskStatus { task_id } => {
                // Handle real-time task status query
                let status = self.get_real_time_task_status(&task_id).await?;
                let response = ServerMessage::TaskStatus { task_id, status };
                self.connection_pool.send_message(&connection_id, WebSocketMessage::Server(response)).await?;
            },
            ClientMessage::Ping => {
                let response = ServerMessage::Pong;
                self.connection_pool.send_message(&connection_id, WebSocketMessage::Server(response)).await?;
            },
        }

        Ok(())
    }
}

// WebSocket message types
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum WebSocketMessage {
    Event(AIEvent),
    Server(ServerMessage),
    Client(ClientMessage),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ClientMessage {
    Subscribe { subscription_type: SubscriptionType },
    Unsubscribe { subscription_id: SubscriptionId },
    GetTaskStatus { task_id: Hash },
    Ping,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ServerMessage {
    SubscriptionConfirmed { subscription_id: SubscriptionId },
    SubscriptionCancelled { subscription_id: SubscriptionId },
    TaskStatus { task_id: Hash, status: TaskStatus },
    Pong,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum SubscriptionType {
    TaskUpdates(TaskSubscriptionFilter),
    MinerUpdates(CompressedPublicKey),
    NetworkStats,
    Leaderboard(LeaderboardCategory),
    RewardDistributions,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TaskSubscriptionFilter {
    pub task_types: Option<Vec<TaskType>>,
    pub publisher: Option<CompressedPublicKey>,
    pub min_reward: Option<u64>,
}

// AI event types
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum AIEvent {
    TaskPublished {
        task_id: Hash,
        task_type: TaskType,
        reward_amount: u64,
        publisher: CompressedPublicKey,
    },
    SubmissionReceived {
        task_id: Hash,
        submission_id: Hash,
        submitter: CompressedPublicKey,
    },
    ValidationCompleted {
        task_id: Hash,
        submission_id: Hash,
        validator: CompressedPublicKey,
        result: ValidationResult,
    },
    TaskCompleted {
        task_id: Hash,
        winner: Option<CompressedPublicKey>,
        total_rewards: u64,
    },
    RewardDistributed {
        task_id: Hash,
        recipient: CompressedPublicKey,
        amount: u64,
        reward_type: RewardType,
    },
    FraudDetected {
        submission_id: Hash,
        miner: CompressedPublicKey,
        risk_score: f64,
    },
    DisputeRaised {
        dispute_id: Hash,
        task_id: Hash,
        disputer: CompressedPublicKey,
    },
    MinerRegistered {
        miner_address: CompressedPublicKey,
        specializations: Vec<TaskType>,
    },
    ReputationChanged {
        miner_address: CompressedPublicKey,
        old_score: u32,
        new_score: u32,
        change_reason: String,
    },
    NetworkStatsUpdated {
        active_tasks: u32,
        total_miners: u32,
        total_rewards: u64,
    },
}
```

### 3. HTTP REST API (daemon/src/rpc/ai_http.rs)

```rust
use std::sync::Arc;
use warp::{Filter, Reply};
use serde_json::json;
use crate::{
    ai::{task_manager::*, miner_registry::*},
    rpc::ai_rpc::*,
};

/// Create AI Mining HTTP API routes
pub fn create_ai_routes(
    rpc_handler: Arc<AIRpcHandler>,
) -> impl Filter<Extract = impl Reply, Error = warp::Rejection> + Clone {
    let base = warp::path("ai");

    // GET /ai/network-info
    let network_info = base
        .and(warp::path("network-info"))
        .and(warp::path::end())
        .and(warp::get())
        .and(with_rpc_handler(rpc_handler.clone()))
        .and_then(handle_get_network_info);

    // POST /ai/tasks
    let publish_task = base
        .and(warp::path("tasks"))
        .and(warp::path::end())
        .and(warp::post())
        .and(warp::body::json())
        .and(with_rpc_handler(rpc_handler.clone()))
        .and_then(handle_publish_task);

    // GET /ai/tasks
    let list_tasks = base
        .and(warp::path("tasks"))
        .and(warp::path::end())
        .and(warp::get())
        .and(warp::query())
        .and(with_rpc_handler(rpc_handler.clone()))
        .and_then(handle_list_tasks);

    // GET /ai/tasks/{task_id}
    let get_task = base
        .and(warp::path("tasks"))
        .and(warp::path::param::<String>())
        .and(warp::path::end())
        .and(warp::get())
        .and(with_rpc_handler(rpc_handler.clone()))
        .and_then(handle_get_task);

    // POST /ai/tasks/{task_id}/submissions
    let submit_answer = base
        .and(warp::path("tasks"))
        .and(warp::path::param::<String>())
        .and(warp::path("submissions"))
        .and(warp::path::end())
        .and(warp::post())
        .and(warp::body::json())
        .and(with_rpc_handler(rpc_handler.clone()))
        .and_then(handle_submit_answer);

    // POST /ai/submissions/{submission_id}/validations
    let validate_answer = base
        .and(warp::path("submissions"))
        .and(warp::path::param::<String>())
        .and(warp::path("validations"))
        .and(warp::path::end())
        .and(warp::post())
        .and(warp::body::json())
        .and(with_rpc_handler(rpc_handler.clone()))
        .and_then(handle_validate_answer);

    // POST /ai/miners/register
    let register_miner = base
        .and(warp::path("miners"))
        .and(warp::path("register"))
        .and(warp::path::end())
        .and(warp::post())
        .and(warp::body::json())
        .and(with_rpc_handler(rpc_handler.clone()))
        .and_then(handle_register_miner);

    // GET /ai/miners/{address}
    let get_miner_profile = base
        .and(warp::path("miners"))
        .and(warp::path::param::<String>())
        .and(warp::path::end())
        .and(warp::get())
        .and(with_rpc_handler(rpc_handler.clone()))
        .and_then(handle_get_miner_profile);

    // GET /ai/miners/{address}/recommendations
    let get_recommendations = base
        .and(warp::path("miners"))
        .and(warp::path::param::<String>())
        .and(warp::path("recommendations"))
        .and(warp::path::end())
        .and(warp::get())
        .and(warp::query())
        .and(with_rpc_handler(rpc_handler.clone()))
        .and_then(handle_get_recommendations);

    // GET /ai/leaderboard
    let get_leaderboard = base
        .and(warp::path("leaderboard"))
        .and(warp::path::end())
        .and(warp::get())
        .and(warp::query())
        .and(with_rpc_handler(rpc_handler.clone()))
        .and_then(handle_get_leaderboard);

    // GET /ai/statistics
    let get_statistics = base
        .and(warp::path("statistics"))
        .and(warp::path::end())
        .and(warp::get())
        .and(warp::query())
        .and(with_rpc_handler(rpc_handler.clone()))
        .and_then(handle_get_statistics);

    // POST /ai/rewards/claim
    let claim_reward = base
        .and(warp::path("rewards"))
        .and(warp::path("claim"))
        .and(warp::path::end())
        .and(warp::post())
        .and(warp::body::json())
        .and(with_rpc_handler(rpc_handler.clone()))
        .and_then(handle_claim_reward);

    // POST /ai/disputes
    let submit_dispute = base
        .and(warp::path("disputes"))
        .and(warp::path::end())
        .and(warp::post())
        .and(warp::body::json())
        .and(with_rpc_handler(rpc_handler.clone()))
        .and_then(handle_submit_dispute);

    network_info
        .or(publish_task)
        .or(list_tasks)
        .or(get_task)
        .or(submit_answer)
        .or(validate_answer)
        .or(register_miner)
        .or(get_miner_profile)
        .or(get_recommendations)
        .or(get_leaderboard)
        .or(get_statistics)
        .or(claim_reward)
        .or(submit_dispute)
}

// HTTP handler functions
async fn handle_get_network_info(
    rpc_handler: Arc<AIRpcHandler>,
) -> Result<impl Reply, warp::Rejection> {
    match rpc_handler.get_ai_network_info().await {
        Ok(info) => Ok(warp::reply::json(&info)),
        Err(e) => Ok(warp::reply::json(&json!({
            "error": e.to_string()
        }))),
    }
}

async fn handle_publish_task(
    request: PublishTaskRequest,
    rpc_handler: Arc<AIRpcHandler>,
) -> Result<impl Reply, warp::Rejection> {
    match rpc_handler.publish_task(request).await {
        Ok(response) => Ok(warp::reply::json(&response)),
        Err(e) => Ok(warp::reply::json(&json!({
            "error": e.to_string()
        }))),
    }
}

async fn handle_list_tasks(
    filter: TaskFilter,
    rpc_handler: Arc<AIRpcHandler>,
) -> Result<impl Reply, warp::Rejection> {
    match rpc_handler.list_tasks(filter).await {
        Ok(tasks) => Ok(warp::reply::json(&tasks)),
        Err(e) => Ok(warp::reply::json(&json!({
            "error": e.to_string()
        }))),
    }
}

// Helper functions
fn with_rpc_handler(
    rpc_handler: Arc<AIRpcHandler>,
) -> impl Filter<Extract = (Arc<AIRpcHandler>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || rpc_handler.clone())
}
```

### 4. CLI Command Line Interface (cli/src/ai_commands.rs)

```rust
use clap::{App, Arg, ArgMatches, SubCommand};
use serde_json::json;
use crate::{
    client::Client,
    config::Config,
    ai::types::*,
};

pub fn create_ai_subcommands() -> App<'static, 'static> {
    App::new("ai")
        .about("AI Mining commands")
        .subcommand(
            SubCommand::with_name("publish-task")
                .about("Publish a new AI task")
                .arg(Arg::with_name("task-type")
                    .long("task-type")
                    .value_name("TYPE")
                    .help("Type of task (code-analysis, security-audit, data-analysis, etc.)")
                    .required(true))
                .arg(Arg::with_name("reward")
                    .long("reward")
                    .value_name("AMOUNT")
                    .help("Reward amount in TOS")
                    .required(true))
                .arg(Arg::with_name("deadline")
                    .long("deadline")
                    .value_name("TIMESTAMP")
                    .help("Task deadline (Unix timestamp)")
                    .required(true))
                .arg(Arg::with_name("data-file")
                    .long("data-file")
                    .value_name("FILE")
                    .help("File containing task data")
                    .required(true))
                .arg(Arg::with_name("description")
                    .long("description")
                    .value_name("TEXT")
                    .help("Task description")
                    .required(true))
        )
        .subcommand(
            SubCommand::with_name("list-tasks")
                .about("List available tasks")
                .arg(Arg::with_name("task-type")
                    .long("task-type")
                    .value_name("TYPE")
                    .help("Filter by task type"))
                .arg(Arg::with_name("status")
                    .long("status")
                    .value_name("STATUS")
                    .help("Filter by task status"))
                .arg(Arg::with_name("limit")
                    .long("limit")
                    .value_name("N")
                    .help("Maximum number of tasks to show")
                    .default_value("20"))
        )
        .subcommand(
            SubCommand::with_name("get-task")
                .about("Get detailed task information")
                .arg(Arg::with_name("task-id")
                    .help("Task ID")
                    .required(true))
        )
        .subcommand(
            SubCommand::with_name("submit-answer")
                .about("Submit answer to a task")
                .arg(Arg::with_name("task-id")
                    .help("Task ID")
                    .required(true))
                .arg(Arg::with_name("answer-file")
                    .long("answer-file")
                    .value_name("FILE")
                    .help("File containing the answer")
                    .required(true))
                .arg(Arg::with_name("stake")
                    .long("stake")
                    .value_name("AMOUNT")
                    .help("Stake amount")
                    .required(true))
        )
        .subcommand(
            SubCommand::with_name("validate-answer")
                .about("Validate a submitted answer")
                .arg(Arg::with_name("submission-id")
                    .help("Submission ID")
                    .required(true))
                .arg(Arg::with_name("score")
                    .long("score")
                    .value_name("SCORE")
                    .help("Quality score (0-100)")
                    .required(true))
                .arg(Arg::with_name("feedback")
                    .long("feedback")
                    .value_name("TEXT")
                    .help("Validation feedback"))
        )
        .subcommand(
            SubCommand::with_name("register-miner")
                .about("Register as an AI miner")
                .arg(Arg::with_name("specializations")
                    .long("specializations")
                    .value_name("TYPES")
                    .help("Comma-separated list of specializations")
                    .required(true))
                .arg(Arg::with_name("initial-stake")
                    .long("initial-stake")
                    .value_name("AMOUNT")
                    .help("Initial stake amount")
                    .required(true))
                .arg(Arg::with_name("contact-info")
                    .long("contact-info")
                    .value_name("JSON")
                    .help("Contact information (JSON format)"))
        )
        .subcommand(
            SubCommand::with_name("miner-profile")
                .about("Get miner profile")
                .arg(Arg::with_name("address")
                    .help("Miner address (optional, defaults to own address)"))
        )
        .subcommand(
            SubCommand::with_name("recommendations")
                .about("Get recommended tasks")
                .arg(Arg::with_name("limit")
                    .long("limit")
                    .value_name("N")
                    .help("Maximum number of recommendations")
                    .default_value("10"))
        )
        .subcommand(
            SubCommand::with_name("leaderboard")
                .about("View leaderboard")
                .arg(Arg::with_name("category")
                    .long("category")
                    .value_name("CATEGORY")
                    .help("Leaderboard category (reputation, tasks-completed, quality-score, earnings)")
                    .default_value("reputation"))
                .arg(Arg::with_name("period")
                    .long("period")
                    .value_name("PERIOD")
                    .help("Time period (daily, weekly, monthly, all-time)")
                    .default_value("all-time"))
                .arg(Arg::with_name("limit")
                    .long("limit")
                    .value_name("N")
                    .help("Number of entries to show")
                    .default_value("20"))
        )
        .subcommand(
            SubCommand::with_name("claim-reward")
                .about("Claim reward from completed task")
                .arg(Arg::with_name("task-id")
                    .help("Task ID")
                    .required(true))
        )
        .subcommand(
            SubCommand::with_name("network-stats")
                .about("View network statistics")
                .arg(Arg::with_name("period")
                    .long("period")
                    .value_name("PERIOD")
                    .help("Time period for statistics")
                    .default_value("all-time"))
        )
        .subcommand(
            SubCommand::with_name("submit-dispute")
                .about("Submit a dispute")
                .arg(Arg::with_name("task-id")
                    .help("Task ID")
                    .required(true))
                .arg(Arg::with_name("dispute-type")
                    .long("type")
                    .value_name("TYPE")
                    .help("Type of dispute")
                    .required(true))
                .arg(Arg::with_name("evidence-file")
                    .long("evidence")
                    .value_name("FILE")
                    .help("File containing evidence")
                    .required(true))
        )
}

pub async fn handle_ai_command(matches: &ArgMatches<'_>, client: &Client) -> Result<(), Box<dyn std::error::Error>> {
    match matches.subcommand() {
        ("publish-task", Some(sub_matches)) => {
            handle_publish_task(sub_matches, client).await
        },
        ("list-tasks", Some(sub_matches)) => {
            handle_list_tasks(sub_matches, client).await
        },
        ("get-task", Some(sub_matches)) => {
            handle_get_task(sub_matches, client).await
        },
        ("submit-answer", Some(sub_matches)) => {
            handle_submit_answer(sub_matches, client).await
        },
        ("validate-answer", Some(sub_matches)) => {
            handle_validate_answer(sub_matches, client).await
        },
        ("register-miner", Some(sub_matches)) => {
            handle_register_miner(sub_matches, client).await
        },
        ("miner-profile", Some(sub_matches)) => {
            handle_miner_profile(sub_matches, client).await
        },
        ("recommendations", Some(sub_matches)) => {
            handle_recommendations(sub_matches, client).await
        },
        ("leaderboard", Some(sub_matches)) => {
            handle_leaderboard(sub_matches, client).await
        },
        ("claim-reward", Some(sub_matches)) => {
            handle_claim_reward(sub_matches, client).await
        },
        ("network-stats", Some(sub_matches)) => {
            handle_network_stats(sub_matches, client).await
        },
        ("submit-dispute", Some(sub_matches)) => {
            handle_submit_dispute(sub_matches, client).await
        },
        _ => {
            println!("Unknown AI command. Use 'tos ai --help' for available commands.");
            Ok(())
        }
    }
}

// CLI command handler function implementations
async fn handle_publish_task(matches: &ArgMatches<'_>, client: &Client) -> Result<(), Box<dyn std::error::Error>> {
    let task_type = matches.value_of("task-type").unwrap();
    let reward: u64 = matches.value_of("reward").unwrap().parse()?;
    let deadline: u64 = matches.value_of("deadline").unwrap().parse()?;
    let data_file = matches.value_of("data-file").unwrap();
    let description = matches.value_of("description").unwrap();

    // Read task data file
    let task_data = std::fs::read(data_file)?;

    // Build publish task request
    let request = PublishTaskRequest {
        publisher: client.get_address()?,
        task_data: PublishTaskPayload {
            task_type: parse_task_type(task_type)?,
            description_hash: Hash::from(description.as_bytes()),
            encrypted_data: task_data,
            reward_amount: reward,
            deadline,
            stake_required: reward / 10, // Default 10% stake requirement
            max_participants: 50,
            verification_type: VerificationType::PeerReview {
                required_reviewers: 3,
                consensus_threshold: 0.6,
            },
            difficulty_level: DifficultyLevel::Intermediate,
            quality_threshold: 70,
        },
        current_block_height: client.get_current_block_height().await?,
    };

    // Send request
    let response = client.ai_publish_task(request).await?;

    println!("Task published successfully!");
    println!("Task ID: {}", response.task_id);
    println!("Status: {}", response.status);
    println!("Estimated participants: {}", response.estimated_participants);

    Ok(())
}

async fn handle_list_tasks(matches: &ArgMatches<'_>, client: &Client) -> Result<(), Box<dyn std::error::Error>> {
    let limit: u32 = matches.value_of("limit").unwrap().parse()?;

    let filter = TaskFilter {
        task_types: matches.value_of("task-type").map(|t| vec![parse_task_type(t).unwrap()]),
        difficulty_levels: None,
        status: matches.value_of("status").map(|s| vec![parse_task_status(s).unwrap()]),
        min_reward: None,
        max_reward: None,
        publisher: None,
        offset: 0,
        limit,
    };

    let task_list = client.ai_list_tasks(filter).await?;

    println!("Found {} tasks (showing {})", task_list.total_count, task_list.tasks.len());
    println!();

    for (i, task) in task_list.tasks.iter().enumerate() {
        println!("{}. Task ID: {}", i + 1, task.task_id);
        println!("   Type: {:?}", task.task_type);
        println!("   Reward: {} TOS", task.reward_amount);
        println!("   Status: {:?}", task.status);
        println!("   Participants: {}", task.participant_count);
        println!();
    }

    if task_list.has_more {
        println!("Use --limit to see more tasks");
    }

    Ok(())
}

// Other CLI handler function implementations...

fn parse_task_type(task_type: &str) -> Result<TaskType, Box<dyn std::error::Error>> {
    match task_type {
        "code-analysis" => Ok(TaskType::CodeAnalysis {
            language: ProgrammingLanguage::Rust,
            complexity: ComplexityLevel::Medium,
        }),
        "security-audit" => Ok(TaskType::SecurityAudit {
            scope: AuditScope::SmartContract,
            standards: vec![SecurityStandard::OWASP],
        }),
        "data-analysis" => Ok(TaskType::DataAnalysis {
            data_type: DataType::Structured,
            analysis_type: AnalysisType::Descriptive,
        }),
        _ => Err(format!("Unknown task type: {}", task_type).into()),
    }
}

fn parse_task_status(status: &str) -> Result<TaskStatus, Box<dyn std::error::Error>> {
    match status {
        "published" => Ok(TaskStatus::Published),
        "in-progress" => Ok(TaskStatus::InProgress),
        "completed" => Ok(TaskStatus::Completed),
        "expired" => Ok(TaskStatus::Expired),
        _ => Err(format!("Unknown task status: {}", status).into()),
    }
}
```

This API and RPC interface design provides:

1. **Complete RPC Interface**: Covers all AI mining functionality with RPC calls
2. **WebSocket Real-time API**: Supports real-time event pushing and subscriptions
3. **HTTP REST API**: Standard REST interfaces for easy integration
4. **CLI Command Line Tool**: Convenient command line operation interface
5. **Type Safety**: Complete request/response type definitions
6. **Error Handling**: Detailed error classification and handling mechanisms
7. **Validation and Filtering**: Parameter validation and data filtering functionality

Next, I will continue to improve the network communication mechanisms and testing strategies.