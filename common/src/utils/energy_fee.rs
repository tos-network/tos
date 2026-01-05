use crate::{
    account::{EnergyResource, FreezeDuration, FreezeRecord},
    block::TopoHeight,
    config::ENERGY_PER_TRANSFER,
};

/// Energy-based fee calculator for TOS
/// Implements TRON-style energy model without bandwidth
///
/// # Energy Cost Model
/// - Transfer operations: 1 energy per transfer (regardless of transaction size)
/// - Account creation: 0 energy (no energy cost for new addresses)
/// - Transaction size: Ignored in energy calculation (unlike TRON's bandwidth)
///
/// # Edge Cases
/// - Large transactions consume the same energy as small ones (size-independent)
/// - Multiple outputs scale linearly (N outputs = N energy)
/// - New account creation doesn't consume additional energy
/// - Zero outputs result in zero energy cost
pub struct EnergyFeeCalculator;

impl EnergyFeeCalculator {
    /// Calculate energy cost for a transaction (only transfer operations consume energy)
    /// Each transfer consumes exactly 1 energy, regardless of transaction size
    ///
    /// # Parameters
    /// - `_tx_size`: Transaction size in bytes (ignored in current implementation)
    /// - `output_count`: Number of transfer outputs (each costs 1 energy)
    /// - `new_addresses`: Number of new addresses created (currently costs 0 energy)
    ///
    /// # Edge Cases
    /// - Transaction size is completely ignored (unlike TRON's bandwidth model)
    /// - New address creation is free in terms of energy
    /// - Zero outputs = zero energy cost
    /// - Large transactions with 1 output = same cost as small transactions with 1 output
    pub fn calculate_energy_cost(
        _tx_size: usize,
        output_count: usize,
        _new_addresses: usize,
    ) -> u64 {
        // Energy cost for transfers (1 energy per transfer, regardless of size)
        // Note: new_addresses parameter is intentionally unused as new account
        // creation is free in the current energy model
        output_count as u64 * ENERGY_PER_TRANSFER
    }
}

/// Energy resource manager for accounts
///
/// # Purpose
/// Provides high-level operations for managing energy resources, including:
/// - Freezing TOS to gain energy
/// - Unfreezing TOS (with time constraints)
/// - Consuming energy for transactions
/// - Resetting energy usage periodically
///
/// # Edge Cases and Error Handling
/// - All TOS amounts must be whole numbers (fractional parts discarded)
/// - Energy consumption fails if insufficient energy available
/// - Unfreezing only works on unlocked freeze records
/// - Energy reset timing must be managed externally
pub struct EnergyResourceManager;

impl EnergyResourceManager {
    /// Create new energy resource for an account
    pub fn create_energy_resource() -> EnergyResource {
        EnergyResource::new()
    }

    /// Freeze TOS to get energy with duration-based rewards
    pub fn freeze_tos_for_energy(
        energy_resource: &mut EnergyResource,
        tos_amount: u64,
        duration: FreezeDuration,
        topoheight: TopoHeight,
    ) -> Result<u64, String> {
        energy_resource.freeze_tos_for_energy(tos_amount, duration, topoheight)
    }

    /// Unfreeze TOS (two-phase unfreeze - returns energy removed and pending amount)
    ///
    /// # Arguments
    /// - `record_index`: Optional record index for selective unfreeze (None = FIFO mode)
    pub fn unfreeze_tos(
        energy_resource: &mut EnergyResource,
        tos_amount: u64,
        topoheight: TopoHeight,
        record_index: Option<u32>,
        network: &crate::network::Network,
    ) -> Result<(u64, u64), String> {
        energy_resource.unfreeze_tos(tos_amount, topoheight, record_index, network)
    }

    /// Withdraw unfrozen TOS after cooldown period
    pub fn withdraw_unfrozen(
        energy_resource: &mut EnergyResource,
        topoheight: TopoHeight,
    ) -> Result<u64, &'static str> {
        energy_resource.withdraw_unfrozen(topoheight)
    }

    /// Consume energy for transaction
    pub fn consume_energy_for_transaction(
        energy_resource: &mut EnergyResource,
        energy_cost: u64,
        topoheight: TopoHeight,
        network: &crate::network::Network,
    ) -> Result<(), &'static str> {
        energy_resource.clear_pending_energy_if_ready(topoheight);
        energy_resource.maybe_reset_energy(topoheight, network);
        energy_resource.consume_energy(energy_cost, topoheight)
    }

    /// Reset energy usage (called periodically)
    pub fn reset_energy_usage(energy_resource: &mut EnergyResource, topoheight: TopoHeight) {
        energy_resource.reset_used_energy(topoheight);
    }

    /// Get energy status for an account
    pub fn get_energy_status(energy_resource: &EnergyResource) -> EnergyStatus {
        EnergyStatus {
            total_energy: energy_resource.total_energy,
            used_energy: energy_resource.used_energy,
            available_energy: energy_resource.available_energy(),
            frozen_tos: energy_resource.frozen_tos,
        }
    }

    /// Get unlockable TOS amount at current topoheight
    pub fn get_unlockable_tos(
        energy_resource: &EnergyResource,
        current_topoheight: TopoHeight,
    ) -> u64 {
        energy_resource.get_unlockable_tos(current_topoheight)
    }

    /// Get freeze records grouped by duration
    pub fn get_freeze_records_by_duration(
        energy_resource: &EnergyResource,
    ) -> std::collections::HashMap<FreezeDuration, Vec<&FreezeRecord>> {
        energy_resource.get_freeze_records_by_duration()
    }
}

/// Energy status information
#[derive(Debug, Clone)]
pub struct EnergyStatus {
    pub total_energy: u64,
    pub used_energy: u64,
    pub available_energy: u64,
    pub frozen_tos: u64,
}

impl EnergyStatus {
    /// Calculate energy usage percentage
    pub fn usage_percentage(&self) -> f64 {
        if self.total_energy == 0 {
            0.0
        } else {
            (self.used_energy as f64 / self.total_energy as f64) * 100.0
        }
    }

    /// Check if energy is low (less than 10% available)
    pub fn is_energy_low(&self) -> bool {
        self.available_energy < self.total_energy / 10
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_energy_cost_calculation() {
        let cost = EnergyFeeCalculator::calculate_energy_cost(1024, 1, 0);
        assert_eq!(cost, ENERGY_PER_TRANSFER);
    }

    #[test]
    fn test_transfer_energy_cost_is_one() {
        // Test that each transfer consumes exactly 1 energy
        let single_transfer_cost = EnergyFeeCalculator::calculate_energy_cost(100, 1, 0);
        assert_eq!(single_transfer_cost, ENERGY_PER_TRANSFER); // Should be 1 energy

        // Test multiple transfers
        let multiple_transfer_cost = EnergyFeeCalculator::calculate_energy_cost(100, 5, 0);
        assert_eq!(multiple_transfer_cost, 5 * ENERGY_PER_TRANSFER); // Should be 5 energy

        // Test with new addresses (new addresses don't consume energy in current implementation)
        let transfer_with_new_address = EnergyFeeCalculator::calculate_energy_cost(100, 1, 2);
        assert_eq!(transfer_with_new_address, ENERGY_PER_TRANSFER); // Only 1 energy for the transfer

        // Verify the constant is set to 1
        assert_eq!(ENERGY_PER_TRANSFER, 1);
    }

    #[test]
    fn test_transfer_energy_cost_independent_of_size() {
        // Test that energy cost is independent of transaction size
        let small_tx_cost = EnergyFeeCalculator::calculate_energy_cost(100, 1, 0);
        let large_tx_cost = EnergyFeeCalculator::calculate_energy_cost(10000, 1, 0);
        let huge_tx_cost = EnergyFeeCalculator::calculate_energy_cost(100000, 1, 0);

        // All should cost exactly 1 energy regardless of size
        assert_eq!(small_tx_cost, 1);
        assert_eq!(large_tx_cost, 1);
        assert_eq!(huge_tx_cost, 1);

        // Multiple transfers should scale linearly
        let multiple_small = EnergyFeeCalculator::calculate_energy_cost(100, 3, 0);
        let multiple_large = EnergyFeeCalculator::calculate_energy_cost(10000, 3, 0);

        assert_eq!(multiple_small, 3);
        assert_eq!(multiple_large, 3);
    }

    #[test]
    fn test_energy_resource_management() {
        let mut resource = EnergyResourceManager::create_energy_resource();

        // Freeze TOS to get energy with duration
        let duration = FreezeDuration::new(7).unwrap();
        let energy_gained = EnergyResourceManager::freeze_tos_for_energy(
            &mut resource,
            100000000, // 1 TOS
            duration,
            1000,
        )
        .unwrap();
        assert_eq!(energy_gained, 14); // 1 TOS * 14 = 14 transfers
        assert_eq!(resource.available_energy(), 14);

        // Consume energy (must be in next block due to pending energy gating)
        let result = EnergyResourceManager::consume_energy_for_transaction(
            &mut resource,
            5,    // 5 transfers
            1001, // Next block - energy is now available
            &crate::network::Network::Mainnet,
        );
        assert!(result.is_ok());
        assert_eq!(resource.available_energy(), 9); // 14 - 5 = 9 transfers remaining
    }
}
