use crate::{
    account::AccountEnergy,
    config::{
        ENERGY_COST_BURN, ENERGY_COST_CONTRACT_DEPLOY_BASE, ENERGY_COST_CONTRACT_DEPLOY_PER_BYTE,
        ENERGY_COST_NEW_ACCOUNT, ENERGY_COST_TRANSFER_PER_OUTPUT, TOS_PER_ENERGY,
    },
};

/// Energy-based fee calculator for TOS Stake 2.0
///
/// # Stake 2.0 Energy Cost Model
/// - Transfer: size_bytes + outputs × 100
/// - Burn: 1,000 energy
/// - Create account: 25,000 energy additional
/// - Deploy contract: bytecode_size × 10 + 32,000
/// - Invoke contract: Actual CU used
/// - Energy operations: FREE (0 energy)
///
/// # Energy Consumption Priority
/// 1. Free quota (1,000/day)
/// 2. Frozen energy (proportional allocation)
/// 3. Auto-burn TOS (100 atomic/energy)
pub struct EnergyFeeCalculator;

impl EnergyFeeCalculator {
    /// Calculate energy cost for a transfer transaction
    ///
    /// Formula: size_bytes + outputs × ENERGY_COST_TRANSFER_PER_OUTPUT
    pub fn calculate_transfer_cost(tx_size: usize, output_count: usize) -> u64 {
        let base = tx_size as u64;
        let output_cost = output_count as u64 * ENERGY_COST_TRANSFER_PER_OUTPUT;
        base + output_cost
    }

    /// Calculate additional energy cost for new account creation
    pub fn calculate_new_account_cost(new_addresses: usize) -> u64 {
        new_addresses as u64 * ENERGY_COST_NEW_ACCOUNT
    }

    /// Calculate energy cost for burn transaction
    pub fn calculate_burn_cost() -> u64 {
        ENERGY_COST_BURN
    }

    /// Calculate energy cost for contract deployment
    ///
    /// Formula: bytecode_size × 10 + 32,000
    pub fn calculate_deploy_cost(bytecode_size: usize) -> u64 {
        ENERGY_COST_CONTRACT_DEPLOY_BASE
            + (bytecode_size as u64 * ENERGY_COST_CONTRACT_DEPLOY_PER_BYTE)
    }

    /// Calculate energy cost for UNO (privacy) transfers
    ///
    /// UNO transfers have higher costs due to ZK proof overhead
    /// Formula: size + outputs × 500
    pub fn calculate_uno_transfer_cost(tx_size: usize, output_count: usize) -> u64 {
        let base = tx_size as u64;
        let output_cost = output_count as u64 * 500;
        base + output_cost
    }

    /// Legacy compatibility: calculate_energy_cost
    ///
    /// For backward compatibility with existing code.
    /// Uses new Stake 2.0 calculation internally.
    pub fn calculate_energy_cost(tx_size: usize, output_count: usize, new_addresses: usize) -> u64 {
        Self::calculate_transfer_cost(tx_size, output_count)
            + Self::calculate_new_account_cost(new_addresses)
    }
}

/// Energy status for display purposes
#[derive(Debug, Clone)]
pub struct EnergyStatus {
    /// Current energy limit based on frozen balance
    pub energy_limit: u64,
    /// Current energy usage (decays over 24h)
    pub energy_usage: u64,
    /// Available energy (limit - usage after decay)
    pub available_energy: u64,
    /// Free quota available
    pub free_energy_available: u64,
    /// Total frozen TOS
    pub frozen_balance: u64,
}

/// Energy resource manager for Stake 2.0
///
/// Provides high-level operations for managing account energy:
/// - Consuming energy with priority (free → frozen → TOS burn)
/// - Getting energy status
/// - Calculating TOS cost for energy shortfall
pub struct EnergyResourceManager;

impl EnergyResourceManager {
    /// Get energy status for an account
    pub fn get_energy_status(
        account: &AccountEnergy,
        total_energy_weight: u64,
        now_ms: u64,
    ) -> EnergyStatus {
        let energy_limit = account.calculate_energy_limit(total_energy_weight);
        let available_energy =
            account.calculate_frozen_energy_available(now_ms, total_energy_weight);
        let free_energy_available = account.calculate_free_energy_available(now_ms);

        EnergyStatus {
            energy_limit,
            energy_usage: account.energy_usage,
            available_energy,
            free_energy_available,
            frozen_balance: account.frozen_balance,
        }
    }

    /// Calculate TOS cost for energy shortfall
    ///
    /// When energy is insufficient, user must burn TOS at rate of TOS_PER_ENERGY
    /// Uses saturating_mul to prevent overflow (returns u64::MAX on overflow)
    pub fn calculate_tos_cost_for_energy(energy_needed: u64) -> u64 {
        energy_needed.saturating_mul(TOS_PER_ENERGY)
    }

    /// Consume energy for a transaction with priority order
    ///
    /// Priority:
    /// 1. Free quota
    /// 2. Frozen energy
    /// 3. Auto-burn TOS (returns TOS amount needed)
    ///
    /// Returns: (energy_consumed, tos_to_burn)
    pub fn consume_transaction_energy(
        account: &mut AccountEnergy,
        required_energy: u64,
        total_energy_weight: u64,
        now_ms: u64,
    ) -> (u64, u64) {
        let result = Self::consume_transaction_energy_detailed(
            account,
            required_energy,
            total_energy_weight,
            now_ms,
        );
        (result.total_energy_from_stake(), result.fee)
    }

    /// Consume energy for a transaction with detailed result tracking (Stake 2.0)
    ///
    /// Priority:
    /// 1. Free quota
    /// 2. Frozen energy
    /// 3. Auto-burn TOS
    ///
    /// Returns: TransactionResult with detailed breakdown
    pub fn consume_transaction_energy_detailed(
        account: &mut AccountEnergy,
        required_energy: u64,
        total_energy_weight: u64,
        now_ms: u64,
    ) -> crate::transaction::TransactionResult {
        let mut remaining = required_energy;
        let mut free_energy_used = 0u64;
        let mut frozen_energy_used = 0u64;

        // 1. Consume free quota first
        let free_available = account.calculate_free_energy_available(now_ms);
        let free_to_use = free_available.min(remaining);
        if free_to_use > 0 {
            account.consume_free_energy(free_to_use, now_ms);
            free_energy_used = free_to_use;
            remaining -= free_to_use;
        }

        if remaining > 0 {
            // 2. Consume frozen energy
            let frozen_available =
                account.calculate_frozen_energy_available(now_ms, total_energy_weight);
            let frozen_to_use = frozen_available.min(remaining);
            if frozen_to_use > 0 {
                account.consume_frozen_energy(frozen_to_use, now_ms, total_energy_weight);
                frozen_energy_used = frozen_to_use;
                remaining -= frozen_to_use;
            }
        }

        // 3. Remaining must be paid in TOS (saturating_mul prevents overflow)
        let tos_cost = remaining.saturating_mul(TOS_PER_ENERGY);

        crate::transaction::TransactionResult {
            fee: tos_cost,
            energy_used: required_energy,
            free_energy_used,
            frozen_energy_used,
        }
    }

    /// Check if account has enough resources (energy + TOS) for transaction
    pub fn can_afford_transaction(
        account: &AccountEnergy,
        required_energy: u64,
        account_balance: u64,
        total_energy_weight: u64,
        now_ms: u64,
    ) -> bool {
        let free_available = account.calculate_free_energy_available(now_ms);
        let frozen_available =
            account.calculate_frozen_energy_available(now_ms, total_energy_weight);
        let total_energy = free_available + frozen_available;

        if total_energy >= required_energy {
            return true;
        }

        // Check if TOS can cover the shortfall (saturating_mul prevents overflow)
        let shortfall = required_energy - total_energy;
        let tos_needed = shortfall.saturating_mul(TOS_PER_ENERGY);
        account_balance >= tos_needed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transfer_cost_calculation() {
        // 100 bytes + 1 output = 100 + 100 = 200
        let cost = EnergyFeeCalculator::calculate_transfer_cost(100, 1);
        assert_eq!(cost, 200);

        // 100 bytes + 5 outputs = 100 + 500 = 600
        let cost = EnergyFeeCalculator::calculate_transfer_cost(100, 5);
        assert_eq!(cost, 600);
    }

    #[test]
    fn test_new_account_cost() {
        let cost = EnergyFeeCalculator::calculate_new_account_cost(1);
        assert_eq!(cost, ENERGY_COST_NEW_ACCOUNT);

        let cost = EnergyFeeCalculator::calculate_new_account_cost(3);
        assert_eq!(cost, 3 * ENERGY_COST_NEW_ACCOUNT);
    }

    #[test]
    fn test_burn_cost() {
        let cost = EnergyFeeCalculator::calculate_burn_cost();
        assert_eq!(cost, ENERGY_COST_BURN);
    }

    #[test]
    fn test_deploy_cost() {
        // 1000 bytes = 32000 + 10000 = 42000
        let cost = EnergyFeeCalculator::calculate_deploy_cost(1000);
        assert_eq!(cost, ENERGY_COST_CONTRACT_DEPLOY_BASE + 10000);
    }

    #[test]
    fn test_legacy_calculate_energy_cost() {
        // Should match transfer + new account
        let cost = EnergyFeeCalculator::calculate_energy_cost(100, 1, 1);
        let expected = EnergyFeeCalculator::calculate_transfer_cost(100, 1)
            + EnergyFeeCalculator::calculate_new_account_cost(1);
        assert_eq!(cost, expected);
    }

    #[test]
    fn test_tos_cost_calculation() {
        // 1000 energy = 1000 * 100 = 100,000 atomic TOS
        let tos_cost = EnergyResourceManager::calculate_tos_cost_for_energy(1000);
        assert_eq!(tos_cost, 1000 * TOS_PER_ENERGY);
    }

    #[test]
    fn test_energy_consumption_priority() {
        let mut account = AccountEnergy::new();
        account.frozen_balance = 1_000_000; // 1M frozen
        let total_weight = 100_000_000; // 100M total

        // Account has some energy limit
        let limit = account.calculate_energy_limit(total_weight);
        assert!(limit > 0);

        // With free quota, should use free first
        let now_ms = 1000u64;
        let (consumed, tos_needed) = EnergyResourceManager::consume_transaction_energy(
            &mut account,
            500, // Less than free quota
            total_weight,
            now_ms,
        );
        assert_eq!(consumed, 500);
        assert_eq!(tos_needed, 0);
        assert_eq!(account.free_energy_usage, 500);
    }
}
