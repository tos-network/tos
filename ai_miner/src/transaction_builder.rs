// Allow some clippy lints for AI miner code
#![allow(clippy::too_many_arguments)]

use anyhow::Result;
use log::{debug, info, warn};

use tos_common::{
    ai_mining::AIMiningPayload,
    crypto::{elgamal::CompressedPublicKey, Hash},
    network::Network,
};

/// AI Mining transaction metadata
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AIMiningTransactionMetadata {
    pub payload: AIMiningPayload,
    pub estimated_fee: u64,
    pub estimated_size: usize,
    pub network: Network,
    pub nonce: u64,
}

/// AI Mining transaction builder
pub struct AIMiningTransactionBuilder {
    network: Network,
}

#[allow(dead_code)]
impl AIMiningTransactionBuilder {
    /// Create a new AI mining transaction builder
    pub fn new(network: Network) -> Self {
        Self { network }
    }

    /// Build a register miner transaction metadata
    pub fn build_register_miner_transaction(
        &self,
        miner_address: CompressedPublicKey,
        registration_fee: u64,
        nonce: u64,
        fee: u64,
    ) -> Result<AIMiningTransactionMetadata> {
        if log::log_enabled!(log::Level::Debug) {
            debug!("Building register miner transaction metadata with nonce: {nonce}");
        }

        let payload = AIMiningPayload::RegisterMiner {
            miner_address,
            registration_fee,
        };

        let estimated_size = self.estimate_payload_size(&payload);
        let estimated_fee = if fee > 0 {
            fee
        } else {
            self.estimate_fee_with_payload_type(estimated_size, Some(&payload))
        };

        if log::log_enabled!(log::Level::Info) {
            info!("Built register miner transaction metadata - Fee: {} nanoTOS, Nonce: {}, Network: {:?}", estimated_fee, nonce, self.network);
        }
        Ok(AIMiningTransactionMetadata {
            payload,
            estimated_fee,
            estimated_size,
            network: self.network,
            nonce,
        })
    }

    /// Build a publish task transaction metadata
    pub fn build_publish_task_transaction(
        &self,
        task_id: Hash,
        reward_amount: u64,
        difficulty: tos_common::ai_mining::DifficultyLevel,
        deadline: u64,
        description: String,
        nonce: u64,
        fee: u64,
    ) -> Result<AIMiningTransactionMetadata> {
        if log::log_enabled!(log::Level::Debug) {
            debug!("Building publish task transaction metadata with nonce: {nonce}");
        }

        let payload = AIMiningPayload::PublishTask {
            task_id: task_id.clone(),
            reward_amount,
            difficulty,
            deadline,
            description,
        };

        let estimated_size = self.estimate_payload_size(&payload);
        let estimated_fee = if fee > 0 {
            fee
        } else {
            self.estimate_fee_with_payload_type(estimated_size, Some(&payload))
        };

        if log::log_enabled!(log::Level::Info) {
            info!("Built publish task transaction metadata - Task: {}, Fee: {} nanoTOS, Nonce: {}, Network: {:?}",
                  hex::encode(task_id.as_bytes()), estimated_fee, nonce, self.network);
        }
        Ok(AIMiningTransactionMetadata {
            payload,
            estimated_fee,
            estimated_size,
            network: self.network,
            nonce,
        })
    }

    /// Build a submit answer transaction metadata
    pub fn build_submit_answer_transaction(
        &self,
        task_id: Hash,
        answer_content: String,
        answer_hash: Hash,
        stake_amount: u64,
        nonce: u64,
        fee: u64,
    ) -> Result<AIMiningTransactionMetadata> {
        if log::log_enabled!(log::Level::Debug) {
            debug!("Building submit answer transaction metadata with nonce: {nonce}");
        }

        let payload = AIMiningPayload::SubmitAnswer {
            task_id: task_id.clone(),
            answer_content,
            answer_hash,
            stake_amount,
        };

        let estimated_size = self.estimate_payload_size(&payload);
        let estimated_fee = if fee > 0 {
            fee
        } else {
            self.estimate_fee_with_payload_type(estimated_size, Some(&payload))
        };

        if log::log_enabled!(log::Level::Info) {
            info!("Built submit answer transaction metadata - Task: {}, Fee: {} nanoTOS, Nonce: {}, Network: {:?}",
                  hex::encode(task_id.as_bytes()), estimated_fee, nonce, self.network);
        }
        Ok(AIMiningTransactionMetadata {
            payload,
            estimated_fee,
            estimated_size,
            network: self.network,
            nonce,
        })
    }

    /// Build a validate answer transaction metadata
    pub fn build_validate_answer_transaction(
        &self,
        task_id: Hash,
        answer_id: Hash,
        validation_score: u8,
        nonce: u64,
        fee: u64,
    ) -> Result<AIMiningTransactionMetadata> {
        if log::log_enabled!(log::Level::Debug) {
            debug!("Building validate answer transaction metadata with nonce: {nonce}");
        }

        let payload = AIMiningPayload::ValidateAnswer {
            task_id: task_id.clone(),
            answer_id,
            validation_score,
        };

        let estimated_size = self.estimate_payload_size(&payload);
        let estimated_fee = if fee > 0 {
            fee
        } else {
            self.estimate_fee_with_payload_type(estimated_size, Some(&payload))
        };

        if log::log_enabled!(log::Level::Info) {
            info!("Built validate answer transaction metadata - Task: {}, Fee: {} nanoTOS, Nonce: {}, Network: {:?}",
                  hex::encode(task_id.as_bytes()), estimated_fee, nonce, self.network);
        }
        Ok(AIMiningTransactionMetadata {
            payload,
            estimated_fee,
            estimated_size,
            network: self.network,
            nonce,
        })
    }

    /// Estimate transaction fee based on transaction size, type, and network
    pub fn estimate_fee(&self, tx_size_bytes: usize) -> u64 {
        self.estimate_fee_with_payload_type(tx_size_bytes, None)
    }

    /// Estimate transaction fee with payload type consideration
    pub fn estimate_fee_with_payload_type(
        &self,
        tx_size_bytes: usize,
        payload_type: Option<&AIMiningPayload>,
    ) -> u64 {
        let network_multiplier = self.get_network_fee_multiplier();
        let payload_complexity_multiplier = self.get_payload_complexity_multiplier(payload_type);

        // Network-specific base fees (in nanoTOS)
        let base_fee = match self.network {
            Network::Mainnet => 5000,  // Higher base fee for mainnet
            Network::Testnet => 1000,  // Lower for testnet
            Network::Devnet => 100,    // Minimal for development
            Network::Stagenet => 2000, // Moderate for staging
        };

        // Network-specific per-byte fees
        let per_byte_fee = match self.network {
            Network::Mainnet => 500,  // Premium per-byte fee
            Network::Testnet => 100,  // Reduced for testing
            Network::Devnet => 10,    // Minimal for dev
            Network::Stagenet => 250, // Moderate for staging
        };

        // Calculate total fee with multipliers
        let size_fee = tx_size_bytes as u64 * per_byte_fee;
        let total_fee =
            (base_fee + size_fee) as f64 * network_multiplier * payload_complexity_multiplier;

        if log::log_enabled!(log::Level::Debug) {
            debug!("Fee calculation - Network: {:?}, Base: {}, Size: {} bytes, Per-byte: {}, Total: {}",
                   self.network, base_fee, tx_size_bytes, per_byte_fee, total_fee as u64);
        }

        // Ensure minimum fee
        std::cmp::max(total_fee as u64, base_fee)
    }

    /// Get network-specific fee multiplier
    fn get_network_fee_multiplier(&self) -> f64 {
        match self.network {
            Network::Mainnet => 1.2,  // 20% premium for mainnet
            Network::Testnet => 0.5,  // 50% discount for testing
            Network::Devnet => 0.1,   // 90% discount for development
            Network::Stagenet => 1.0, // Standard rate for staging
        }
    }

    /// Get payload complexity multiplier based on operation type
    fn get_payload_complexity_multiplier(&self, payload_type: Option<&AIMiningPayload>) -> f64 {
        match payload_type {
            Some(AIMiningPayload::RegisterMiner { .. }) => 1.0, // Standard complexity
            Some(AIMiningPayload::PublishTask { .. }) => 1.5,   // Higher complexity (task creation)
            Some(AIMiningPayload::SubmitAnswer { .. }) => 1.2, // Moderate complexity (answer submission)
            Some(AIMiningPayload::ValidateAnswer { .. }) => 1.3, // Higher complexity (validation work)
            None => 1.0, // Default multiplier when payload type is unknown
        }
    }

    /// Estimate the payload size using actual serialization
    fn estimate_payload_size(&self, payload: &AIMiningPayload) -> usize {
        // Try to serialize the payload to get accurate size
        match serde_json::to_vec(payload) {
            Ok(serialized) => {
                let json_size = serialized.len();
                if log::log_enabled!(log::Level::Debug) {
                    debug!("Payload JSON serialization size: {json_size} bytes");
                }

                // Account for binary serialization overhead (typically more compact than JSON)
                // Estimate binary serialization as ~70% of JSON size plus fixed overhead
                let estimated_binary_size =
                    ((json_size as f64 * 0.7) as usize) + self.get_serialization_overhead(payload);

                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "Estimated binary payload size: {estimated_binary_size} bytes (from JSON: {json_size} bytes)"
                    );
                }
                estimated_binary_size
            }
            Err(_) => {
                // Fallback to manual calculation if serialization fails
                warn!("Failed to serialize payload, using fallback size estimation");
                self.estimate_payload_size_manual(payload)
            }
        }
    }

    /// Manual payload size estimation as fallback
    fn estimate_payload_size_manual(&self, payload: &AIMiningPayload) -> usize {
        let base_overhead = 64; // Transaction structure overhead
        match payload {
            AIMiningPayload::RegisterMiner {
                miner_address: _,
                registration_fee: _,
            } => {
                base_overhead + 32 + 8 // PublicKey (32 bytes) + fee (8 bytes)
            }
            AIMiningPayload::PublishTask {
                task_id: _,
                reward_amount: _,
                difficulty: _,
                deadline: _,
                description,
            } => {
                base_overhead + 32 + 8 + 1 + 8 + description.len() // Hash + u64 + enum + u64 + description
            }
            AIMiningPayload::SubmitAnswer {
                task_id: _,
                answer_content,
                answer_hash: _,
                stake_amount: _,
            } => {
                base_overhead + 32 + answer_content.len() + 32 + 8 // Hash + String + Hash + u64
            }
            AIMiningPayload::ValidateAnswer {
                task_id: _,
                answer_id: _,
                validation_score: _,
            } => {
                base_overhead + 32 + 32 + 1 // Hash + Hash + u8
            }
        }
    }

    /// Get serialization overhead based on payload type
    fn get_serialization_overhead(&self, payload: &AIMiningPayload) -> usize {
        match payload {
            AIMiningPayload::RegisterMiner { .. } => 48, // Lower overhead for simple structure
            AIMiningPayload::PublishTask { .. } => 72,   // Higher overhead for complex structure
            AIMiningPayload::SubmitAnswer { .. } => 56,  // Medium overhead
            AIMiningPayload::ValidateAnswer { .. } => 48, // Lower overhead for simple validation
        }
    }

    /// Get the current network
    pub fn network(&self) -> Network {
        self.network
    }
}

/// Helper to estimate transaction size for a payload with network context
#[allow(dead_code)]
pub fn estimate_transaction_size(payload: &AIMiningPayload) -> Result<usize> {
    estimate_transaction_size_for_network(payload, Network::Mainnet)
}

/// Helper to estimate transaction size for a payload on specific network
#[allow(dead_code)]
pub fn estimate_transaction_size_for_network(
    payload: &AIMiningPayload,
    network: Network,
) -> Result<usize> {
    let builder = AIMiningTransactionBuilder::new(network);
    Ok(builder.estimate_payload_size(payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fee_estimation() {
        let builder = AIMiningTransactionBuilder::new(Network::Mainnet);

        let fee = builder.estimate_fee(1000);
        assert!(fee > 1000); // Should be more than base fee

        let fee_small = builder.estimate_fee(100);
        let fee_large = builder.estimate_fee(2000);
        assert!(fee_large > fee_small); // Larger transactions cost more
    }

    #[test]
    fn test_payload_size_estimation() {
        use tos_common::crypto::PublicKey;
        use tos_common::serializer::Serializer;

        let payload = AIMiningPayload::RegisterMiner {
            miner_address: PublicKey::from_bytes(&[0u8; 32]).unwrap(),
            registration_fee: 1000,
        };

        let size = estimate_transaction_size(&payload);
        assert!(size.is_ok());
        assert!(size.unwrap() > 0);
    }
}
