//! AI Mining blockchain state management

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use crate::{
    crypto::{Hash, elgamal::CompressedPublicKey},
    ai_mining::{AIMiningTask, AIMiner, AIMiningError, AIMiningResult, TaskStatus}
};

/// AI Mining state stored in blockchain
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AIMiningState {
    /// Map of task_id -> AIMiningTask
    pub tasks: HashMap<Hash, AIMiningTask>,
    /// Map of miner_address -> AIMiner
    pub miners: HashMap<CompressedPublicKey, AIMiner>,
    /// Statistics for the AI mining system
    pub statistics: AIMiningStatistics,
}

/// System-wide AI mining statistics
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AIMiningStatistics {
    /// Total number of tasks published
    pub total_tasks: u64,
    /// Total number of active tasks
    pub active_tasks: u64,
    /// Total number of completed tasks
    pub completed_tasks: u64,
    /// Total number of registered miners
    pub total_miners: u64,
    /// Total TOS rewards distributed
    pub total_rewards_distributed: u64,
    /// Total TOS staked in the system
    pub total_staked: u64,
}

impl Default for AIMiningState {
    fn default() -> Self {
        Self {
            tasks: HashMap::new(),
            miners: HashMap::new(),
            statistics: AIMiningStatistics::default(),
        }
    }
}

impl Default for AIMiningStatistics {
    fn default() -> Self {
        Self {
            total_tasks: 0,
            active_tasks: 0,
            completed_tasks: 0,
            total_miners: 0,
            total_rewards_distributed: 0,
            total_staked: 0,
        }
    }
}

impl AIMiningState {
    /// Create a new empty AI mining state
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new miner
    pub fn register_miner(
        &mut self,
        address: CompressedPublicKey,
        registration_fee: u64,
        registered_at: u64,
    ) -> AIMiningResult<()> {
        if self.miners.contains_key(&address) {
            return Err(AIMiningError::ValidationFailed(
                "Miner already registered".to_string()
            ));
        }

        let miner = AIMiner::new(address.clone(), registration_fee, registered_at);
        self.miners.insert(address, miner);
        self.statistics.total_miners += 1;

        Ok(())
    }

    /// Get a miner by address
    pub fn get_miner(&self, address: &CompressedPublicKey) -> Option<&AIMiner> {
        self.miners.get(address)
    }

    /// Get a mutable reference to a miner
    pub fn get_miner_mut(&mut self, address: &CompressedPublicKey) -> Option<&mut AIMiner> {
        self.miners.get_mut(address)
    }

    /// Check if a miner is registered
    pub fn is_miner_registered(&self, address: &CompressedPublicKey) -> bool {
        self.miners.contains_key(address)
    }

    /// Publish a new task
    pub fn publish_task(&mut self, task: AIMiningTask) -> AIMiningResult<()> {
        if self.tasks.contains_key(&task.task_id) {
            return Err(AIMiningError::ValidationFailed(
                "Task ID already exists".to_string()
            ));
        }

        // Verify publisher is registered
        if !self.is_miner_registered(&task.publisher) {
            return Err(AIMiningError::MinerNotRegistered(task.publisher));
        }

        // Update miner stats
        if let Some(miner) = self.get_miner_mut(&task.publisher) {
            miner.tasks_published += 1;
        }

        // Update statistics
        self.statistics.total_tasks += 1;
        self.statistics.active_tasks += 1;

        let task_id = task.task_id.clone();
        self.tasks.insert(task_id, task);
        Ok(())
    }

    /// Get a task by ID
    pub fn get_task(&self, task_id: &Hash) -> Option<&AIMiningTask> {
        self.tasks.get(task_id)
    }

    /// Get a mutable reference to a task
    pub fn get_task_mut(&mut self, task_id: &Hash) -> Option<&mut AIMiningTask> {
        self.tasks.get_mut(task_id)
    }

    /// Update task status and statistics based on current time
    pub fn update_task_statuses(&mut self, current_time: u64) {
        for task in self.tasks.values_mut() {
            let old_status = task.status.clone();
            task.update_status(current_time);

            // Update statistics if status changed
            match (&old_status, &task.status) {
                (TaskStatus::Active, TaskStatus::Expired) => {
                    self.statistics.active_tasks -= 1;
                }
                (TaskStatus::Active, TaskStatus::Completed) => {
                    self.statistics.active_tasks -= 1;
                    self.statistics.completed_tasks += 1;
                    self.statistics.total_rewards_distributed += task.reward_amount;
                }
                _ => {}
            }
        }
    }

    /// Get all active tasks
    pub fn get_active_tasks(&self) -> Vec<&AIMiningTask> {
        self.tasks
            .values()
            .filter(|task| task.status == TaskStatus::Active)
            .collect()
    }

    /// Get tasks by difficulty level
    pub fn get_tasks_by_difficulty(&self, difficulty: &crate::ai_mining::DifficultyLevel) -> Vec<&AIMiningTask> {
        self.tasks
            .values()
            .filter(|task| &task.difficulty == difficulty)
            .collect()
    }

    /// Get tasks published by a specific miner
    pub fn get_tasks_by_publisher(&self, publisher: &CompressedPublicKey) -> Vec<&AIMiningTask> {
        self.tasks
            .values()
            .filter(|task| &task.publisher == publisher)
            .collect()
    }

    /// Calculate total staked amount across all tasks
    pub fn calculate_total_staked(&self) -> u64 {
        self.tasks
            .values()
            .flat_map(|task| &task.submitted_answers)
            .map(|answer| answer.stake_amount)
            .sum()
    }

    /// Update statistics (should be called periodically)
    pub fn refresh_statistics(&mut self) {
        self.statistics.total_staked = self.calculate_total_staked();
    }

    /// Get top miners by reputation
    pub fn get_top_miners(&self, limit: usize) -> Vec<&AIMiner> {
        let mut miners: Vec<&AIMiner> = self.miners.values().collect();
        miners.sort_by_key(|m| std::cmp::Reverse(m.reputation));
        miners.into_iter().take(limit).collect()
    }

    /// Cleanup old expired tasks (for maintenance)
    pub fn cleanup_old_tasks(&mut self, cutoff_time: u64, max_age: u64) {
        let old_count = self.tasks.len();

        self.tasks.retain(|_, task| {
            let keep = match task.status {
                TaskStatus::Expired => cutoff_time - task.published_at < max_age,
                _ => true,
            };
            keep
        });

        let removed_count = old_count - self.tasks.len();
        if removed_count > 0 {
            // Recalculate statistics after cleanup
            self.recalculate_statistics();
        }
    }

    /// Recalculate all statistics from current state
    pub fn recalculate_statistics(&mut self) {
        let mut stats = AIMiningStatistics::default();
        stats.total_miners = self.miners.len() as u64;
        stats.total_tasks = self.tasks.len() as u64;

        for task in self.tasks.values() {
            match task.status {
                TaskStatus::Active => stats.active_tasks += 1,
                TaskStatus::Completed => {
                    stats.completed_tasks += 1;
                    stats.total_rewards_distributed += task.reward_amount;
                }
                _ => {}
            }
        }

        stats.total_staked = self.calculate_total_staked();
        self.statistics = stats;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_mining::{DifficultyLevel, SubmittedAnswer};

    #[test]
    fn test_miner_registration() {
        let mut state = AIMiningState::new();
        let address = create_test_pubkey([1u8; 32]);

        assert!(state.register_miner(address.clone(), 1_000_000_000, 100).is_ok());
        assert!(state.is_miner_registered(&address));
        assert_eq!(state.statistics.total_miners, 1);

        // Test duplicate registration
        assert!(state.register_miner(address.clone(), 1_000_000_000, 100).is_err());
    }

    #[test]
    fn test_task_publishing() {
        let mut state = AIMiningState::new();
        let publisher = create_test_pubkey([1u8; 32]);

        // Register miner first
        state.register_miner(publisher.clone(), 1_000_000_000, 100).unwrap();

        let task = create_test_task(publisher);
        let task_id = task.task_id.clone();

        assert!(state.publish_task(task).is_ok());
        assert!(state.get_task(&task_id).is_some());
        assert_eq!(state.statistics.total_tasks, 1);
        assert_eq!(state.statistics.active_tasks, 1);
    }

    #[test]
    fn test_unregistered_publisher() {
        let mut state = AIMiningState::new();
        let publisher = create_test_pubkey([1u8; 32]);
        let task = create_test_task(publisher);

        // Should fail because publisher is not registered
        assert!(state.publish_task(task).is_err());
    }

    #[test]
    fn test_task_status_updates() {
        let mut state = AIMiningState::new();
        let publisher = create_test_pubkey([1u8; 32]);

        state.register_miner(publisher.clone(), 1_000_000_000, 100).unwrap();

        let task = create_test_task(publisher);
        let task_id = task.task_id.clone();
        state.publish_task(task).unwrap();

        assert_eq!(state.statistics.active_tasks, 1);

        // Update with time past deadline
        state.update_task_statuses(2000);

        let task = state.get_task(&task_id).unwrap();
        assert_eq!(task.status, TaskStatus::Expired);
        assert_eq!(state.statistics.active_tasks, 0);
    }

    #[test]
    fn test_statistics_calculation() {
        let mut state = AIMiningState::new();
        let publisher = create_test_pubkey([1u8; 32]);

        state.register_miner(publisher.clone(), 1_000_000_000, 100).unwrap();

        let mut task = create_test_task(publisher);
        let answer = SubmittedAnswer::new(
            Hash::new([3u8; 32]),
            "Test answer content for validation".to_string(),
            Hash::new([4u8; 32]),
            create_test_pubkey([5u8; 32]),
            2_000_000_000,
            150,
        );
        task.add_answer(answer).unwrap();

        state.publish_task(task).unwrap();
        state.refresh_statistics();

        assert_eq!(state.statistics.total_staked, 2_000_000_000);
    }

    // Helper functions
    fn create_test_pubkey(bytes: [u8; 32]) -> CompressedPublicKey {
        use curve25519_dalek::ristretto::CompressedRistretto;
        CompressedPublicKey::new(CompressedRistretto::from_slice(&bytes).unwrap())
    }

    fn create_test_task(publisher: CompressedPublicKey) -> AIMiningTask {
        use crate::ai_mining::AIMiningTask;

        AIMiningTask::new(
            Hash::new([1u8; 32]),
            publisher,
            "Test task".to_string(),
            10_000_000_000,
            DifficultyLevel::Beginner,
            1000, // deadline
            100,  // published_at
        ).unwrap()
    }
}