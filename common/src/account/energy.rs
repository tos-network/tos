use crate::{
    block::TopoHeight,
    config::{
        ENERGY_RECOVERY_WINDOW_MS, FREE_ENERGY_QUOTA, MAX_UNFREEZING_LIST_SIZE, TOTAL_ENERGY_LIMIT,
        UNFREEZE_DELAY_DAYS,
    },
    crypto::PublicKey,
    serializer::{Reader, ReaderError, Serializer, Writer},
};
use serde::{Deserialize, Serialize};

// ============================================================================
// STAKE 2.0 ENERGY MODEL
// ============================================================================
//
// New proportional energy allocation model:
// - Energy = (frozen_balance / total_energy_weight) × TOTAL_ENERGY_LIMIT
// - 24-hour linear decay recovery
// - Free quota for casual users (~3 transfers/day)
// - 14-day unfreeze delay queue (max 32 entries)
// - Delegation support (DelegateResource / UndelegateResource)

/// Account energy state for Stake 2.0 model
///
/// # Energy Calculation
/// - Energy limit = (frozen_balance + acquired_delegated) / total_weight × TOTAL_ENERGY_LIMIT
/// - Available energy = limit - current_usage (after decay recovery)
///
/// # 24-Hour Linear Decay Recovery
/// - Energy usage decays linearly over 24 hours
/// - After 24 hours, full energy limit is available again
/// - Partial recovery: recovered = usage × (elapsed_ms / 86,400,000)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccountEnergy {
    // === Own frozen balance ===
    /// TOS frozen by this account (atomic units)
    pub frozen_balance: u64,

    // === Delegation ===
    /// TOS delegated TO others (reduces own energy, gives to receiver)
    pub delegated_frozen_balance: u64,
    /// TOS received FROM others via delegation (increases own energy)
    pub acquired_delegated_balance: u64,

    // === Energy usage (24h decay) ===
    /// Current energy usage (decays over 24 hours)
    pub energy_usage: u64,
    /// Timestamp (ms) of last energy consumption
    pub latest_consume_time: u64,

    // === Free quota ===
    /// Free energy usage (resets daily)
    pub free_energy_usage: u64,
    /// Timestamp (ms) of last free energy consumption
    pub latest_free_consume_time: u64,

    // === Unfreezing queue (Stake 2.0) ===
    /// Pending unfreeze requests (max 32 entries)
    /// Each entry waits 14 days before TOS can be withdrawn
    pub unfreezing_list: Vec<UnfreezingRecord>,
}

impl AccountEnergy {
    pub fn new() -> Self {
        Self::default()
    }

    /// Validate delegation invariants
    ///
    /// Returns true if the state is valid:
    /// - delegated_frozen_balance <= frozen_balance (can't delegate more than frozen)
    ///
    /// This should be called during verify/apply phases to detect invalid states.
    pub fn is_delegation_valid(&self) -> bool {
        self.delegated_frozen_balance <= self.frozen_balance
    }

    /// Get the maximum amount that can be delegated (frozen - already delegated)
    pub fn available_for_delegation(&self) -> u64 {
        self.frozen_balance
            .saturating_sub(self.delegated_frozen_balance)
    }

    /// Calculate effective frozen balance (own + acquired - delegated)
    pub fn effective_frozen_balance(&self) -> u64 {
        self.frozen_balance
            .saturating_add(self.acquired_delegated_balance)
            .saturating_sub(self.delegated_frozen_balance)
    }

    /// Calculate energy limit based on stake proportion
    ///
    /// Formula: (effective_frozen / total_weight) × TOTAL_ENERGY_LIMIT
    pub fn calculate_energy_limit(&self, total_energy_weight: u64) -> u64 {
        if total_energy_weight == 0 {
            return 0;
        }
        let effective = self.effective_frozen_balance();
        // Use u128 to prevent overflow
        ((effective as u128 * TOTAL_ENERGY_LIMIT as u128) / total_energy_weight as u128) as u64
    }

    /// Calculate available free energy (with 24h decay recovery)
    pub fn calculate_free_energy_available(&self, now_ms: u64) -> u64 {
        let elapsed = now_ms.saturating_sub(self.latest_free_consume_time);

        // Full recovery after 24 hours
        if elapsed >= ENERGY_RECOVERY_WINDOW_MS {
            return FREE_ENERGY_QUOTA;
        }

        // Linear decay: recovered = usage × (elapsed / window)
        let recovered = (self.free_energy_usage as u128 * elapsed as u128
            / ENERGY_RECOVERY_WINDOW_MS as u128) as u64;
        let current_usage = self.free_energy_usage.saturating_sub(recovered);

        FREE_ENERGY_QUOTA.saturating_sub(current_usage)
    }

    /// Calculate available frozen energy (with 24h decay recovery)
    pub fn calculate_frozen_energy_available(&self, now_ms: u64, total_energy_weight: u64) -> u64 {
        let limit = self.calculate_energy_limit(total_energy_weight);
        let elapsed = now_ms.saturating_sub(self.latest_consume_time);

        // Full recovery after 24 hours
        if elapsed >= ENERGY_RECOVERY_WINDOW_MS {
            return limit;
        }

        // Linear decay: recovered = usage × (elapsed / window)
        let recovered = (self.energy_usage as u128 * elapsed as u128
            / ENERGY_RECOVERY_WINDOW_MS as u128) as u64;
        let current_usage = self.energy_usage.saturating_sub(recovered);

        limit.saturating_sub(current_usage)
    }

    /// Consume free energy
    /// Returns the amount actually consumed
    pub fn consume_free_energy(&mut self, amount: u64, now_ms: u64) -> u64 {
        let available = self.calculate_free_energy_available(now_ms);
        let to_consume = amount.min(available);

        if to_consume > 0 {
            // Update usage with decay applied
            let elapsed = now_ms.saturating_sub(self.latest_free_consume_time);
            if elapsed >= ENERGY_RECOVERY_WINDOW_MS {
                self.free_energy_usage = to_consume;
            } else {
                let recovered = (self.free_energy_usage as u128 * elapsed as u128
                    / ENERGY_RECOVERY_WINDOW_MS as u128) as u64;
                self.free_energy_usage = self
                    .free_energy_usage
                    .saturating_sub(recovered)
                    .saturating_add(to_consume);
            }
            self.latest_free_consume_time = now_ms;
        }

        to_consume
    }

    /// Consume frozen energy
    /// Returns the amount actually consumed
    pub fn consume_frozen_energy(
        &mut self,
        amount: u64,
        now_ms: u64,
        total_energy_weight: u64,
    ) -> u64 {
        let available = self.calculate_frozen_energy_available(now_ms, total_energy_weight);
        let to_consume = amount.min(available);

        if to_consume > 0 {
            // Update usage with decay applied
            let elapsed = now_ms.saturating_sub(self.latest_consume_time);
            if elapsed >= ENERGY_RECOVERY_WINDOW_MS {
                self.energy_usage = to_consume;
            } else {
                let recovered = (self.energy_usage as u128 * elapsed as u128
                    / ENERGY_RECOVERY_WINDOW_MS as u128) as u64;
                self.energy_usage = self
                    .energy_usage
                    .saturating_sub(recovered)
                    .saturating_add(to_consume);
            }
            self.latest_consume_time = now_ms;
        }

        to_consume
    }

    /// Add TOS to frozen balance
    pub fn freeze(&mut self, amount: u64) {
        self.frozen_balance = self.frozen_balance.saturating_add(amount);
    }

    /// Start unfreezing process (adds to queue, waits 14 days)
    ///
    /// Validation:
    /// - amount > 0 (reject zero-amount entries)
    /// - amount <= available_for_delegation() (can't unfreeze delegated TOS)
    /// - queue not full (max 32 entries)
    pub fn start_unfreeze(&mut self, amount: u64, now_ms: u64) -> Result<(), &'static str> {
        // Reject zero-amount unfreeze requests
        if amount == 0 {
            return Err("Unfreeze amount must be greater than zero");
        }

        if self.unfreezing_list.len() >= MAX_UNFREEZING_LIST_SIZE {
            return Err("Unfreezing queue is full (max 32 entries)");
        }

        // Can't unfreeze more than available (frozen - delegated)
        // This prevents unfreezing TOS that is delegated to others
        let available = self.available_for_delegation();
        if amount > available {
            return Err("Cannot unfreeze delegated TOS");
        }

        // Move from frozen to unfreezing queue
        self.frozen_balance = self.frozen_balance.saturating_sub(amount);

        // BUG-008 FIX: Use saturating arithmetic to prevent overflow with extreme timestamps
        let delay_ms = (UNFREEZE_DELAY_DAYS as u64).saturating_mul(24 * 60 * 60 * 1000);
        let expire_time = now_ms.saturating_add(delay_ms);
        self.unfreezing_list.push(UnfreezingRecord {
            unfreeze_amount: amount,
            unfreeze_expire_time: expire_time,
        });

        Ok(())
    }

    /// Withdraw all expired unfreeze entries
    /// Returns total TOS withdrawn
    pub fn withdraw_expired_unfreeze(&mut self, now_ms: u64) -> u64 {
        let mut total_withdrawn = 0u64;

        self.unfreezing_list.retain(|record| {
            if record.unfreeze_expire_time <= now_ms {
                total_withdrawn = total_withdrawn.saturating_add(record.unfreeze_amount);
                false // Remove from list
            } else {
                true // Keep in list
            }
        });

        total_withdrawn
    }

    /// Cancel all pending unfreeze operations
    /// Expired entries go to balance, unexpired go back to frozen
    /// Returns (withdrawn_to_balance, cancelled_to_frozen)
    pub fn cancel_all_unfreeze(&mut self, now_ms: u64) -> (u64, u64) {
        let mut withdrawn = 0u64;
        let mut cancelled = 0u64;

        for record in &self.unfreezing_list {
            if record.unfreeze_expire_time <= now_ms {
                withdrawn = withdrawn.saturating_add(record.unfreeze_amount);
            } else {
                cancelled = cancelled.saturating_add(record.unfreeze_amount);
            }
        }

        // Move cancelled amount back to frozen
        self.frozen_balance = self.frozen_balance.saturating_add(cancelled);
        self.unfreezing_list.clear();

        (withdrawn, cancelled)
    }

    /// Get total amount in unfreezing queue
    pub fn total_unfreezing(&self) -> u64 {
        self.unfreezing_list
            .iter()
            .fold(0u64, |acc, r| acc.saturating_add(r.unfreeze_amount))
    }

    /// Get amount that can be withdrawn now (expired entries)
    pub fn withdrawable_amount(&self, now_ms: u64) -> u64 {
        self.unfreezing_list
            .iter()
            .filter(|r| r.unfreeze_expire_time <= now_ms)
            .fold(0u64, |acc, r| acc.saturating_add(r.unfreeze_amount))
    }
}

impl Serializer for AccountEnergy {
    fn write(&self, writer: &mut Writer) {
        writer.write_u64(&self.frozen_balance);
        writer.write_u64(&self.delegated_frozen_balance);
        writer.write_u64(&self.acquired_delegated_balance);
        writer.write_u64(&self.energy_usage);
        writer.write_u64(&self.latest_consume_time);
        writer.write_u64(&self.free_energy_usage);
        writer.write_u64(&self.latest_free_consume_time);

        // Write unfreezing list
        writer.write_u8(self.unfreezing_list.len() as u8);
        for record in &self.unfreezing_list {
            record.write(writer);
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let frozen_balance = reader.read_u64()?;
        let delegated_frozen_balance = reader.read_u64()?;
        let acquired_delegated_balance = reader.read_u64()?;
        let energy_usage = reader.read_u64()?;
        let latest_consume_time = reader.read_u64()?;
        let free_energy_usage = reader.read_u64()?;
        let latest_free_consume_time = reader.read_u64()?;

        let list_len = reader.read_u8()? as usize;
        if list_len > MAX_UNFREEZING_LIST_SIZE {
            return Err(ReaderError::InvalidValue);
        }

        let mut unfreezing_list = Vec::with_capacity(list_len);
        for _ in 0..list_len {
            unfreezing_list.push(UnfreezingRecord::read(reader)?);
        }

        Ok(Self {
            frozen_balance,
            delegated_frozen_balance,
            acquired_delegated_balance,
            energy_usage,
            latest_consume_time,
            free_energy_usage,
            latest_free_consume_time,
            unfreezing_list,
        })
    }

    fn size(&self) -> usize {
        8 * 7  // 7 u64 fields
            + 1  // u8 for list length
            + self.unfreezing_list.iter().map(|r| r.size()).sum::<usize>()
    }
}

/// Record for pending unfreeze operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnfreezingRecord {
    /// Amount of TOS being unfrozen
    pub unfreeze_amount: u64,
    /// Timestamp (ms) when TOS can be withdrawn
    pub unfreeze_expire_time: u64,
}

impl Serializer for UnfreezingRecord {
    fn write(&self, writer: &mut Writer) {
        writer.write_u64(&self.unfreeze_amount);
        writer.write_u64(&self.unfreeze_expire_time);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            unfreeze_amount: reader.read_u64()?,
            unfreeze_expire_time: reader.read_u64()?,
        })
    }

    fn size(&self) -> usize {
        16 // 2 × u64
    }
}

/// Global energy state for the network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalEnergyState {
    /// Total energy limit (constant: 18.4 billion)
    pub total_energy_limit: u64,
    /// Sum of all frozen TOS across all accounts (atomic units)
    pub total_energy_weight: u64,
    /// Last update topoheight
    pub last_update: TopoHeight,
}

// BUG-007 FIX: Implement Default manually to use TOTAL_ENERGY_LIMIT instead of 0
impl Default for GlobalEnergyState {
    fn default() -> Self {
        Self::new()
    }
}

impl GlobalEnergyState {
    pub fn new() -> Self {
        Self {
            total_energy_limit: TOTAL_ENERGY_LIMIT,
            total_energy_weight: 0,
            last_update: 0,
        }
    }

    /// Add to total weight (when TOS is frozen)
    pub fn add_weight(&mut self, amount: u64, topoheight: TopoHeight) {
        self.total_energy_weight = self.total_energy_weight.saturating_add(amount);
        self.last_update = topoheight;
    }

    /// Remove from total weight (when TOS is unfrozen)
    pub fn remove_weight(&mut self, amount: u64, topoheight: TopoHeight) {
        self.total_energy_weight = self.total_energy_weight.saturating_sub(amount);
        self.last_update = topoheight;
    }
}

impl Serializer for GlobalEnergyState {
    fn write(&self, writer: &mut Writer) {
        writer.write_u64(&self.total_energy_limit);
        writer.write_u64(&self.total_energy_weight);
        writer.write_u64(&self.last_update);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            total_energy_limit: reader.read_u64()?,
            total_energy_weight: reader.read_u64()?,
            last_update: reader.read_u64()?,
        })
    }

    fn size(&self) -> usize {
        24 // 3 × u64
    }
}

/// Delegated resource record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegatedResource {
    /// Delegator (who delegated)
    pub from: PublicKey,
    /// Receiver (who receives energy)
    pub to: PublicKey,
    /// Amount of TOS delegated
    pub frozen_balance: u64,
    /// Lock expiry time (ms), 0 = not locked
    pub expire_time: u64,
}

impl DelegatedResource {
    pub fn new(from: PublicKey, to: PublicKey, amount: u64, expire_time: u64) -> Self {
        Self {
            from,
            to,
            frozen_balance: amount,
            expire_time,
        }
    }

    /// Check if delegation is locked
    pub fn is_locked(&self, now_ms: u64) -> bool {
        self.expire_time > 0 && now_ms < self.expire_time
    }

    /// Check if delegation can be undelegated
    pub fn can_undelegate(&self, now_ms: u64) -> bool {
        !self.is_locked(now_ms)
    }
}

impl Serializer for DelegatedResource {
    fn write(&self, writer: &mut Writer) {
        self.from.write(writer);
        self.to.write(writer);
        writer.write_u64(&self.frozen_balance);
        writer.write_u64(&self.expire_time);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            from: PublicKey::read(reader)?,
            to: PublicKey::read(reader)?,
            frozen_balance: reader.read_u64()?,
            expire_time: reader.read_u64()?,
        })
    }

    fn size(&self) -> usize {
        self.from.size() + self.to.size() + 16 // 2 × u64
    }
}

// ============================================================================
// COMPREHENSIVE TESTS (test-scenario.md implementation)
// ============================================================================

#[cfg(test)]
#[path = "energy_comprehensive_tests.rs"]
mod comprehensive_tests;

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_energy_default() {
        let energy = AccountEnergy::new();
        assert_eq!(energy.frozen_balance, 0);
        assert_eq!(energy.delegated_frozen_balance, 0);
        assert_eq!(energy.acquired_delegated_balance, 0);
        assert_eq!(energy.energy_usage, 0);
        assert_eq!(energy.free_energy_usage, 0);
        assert!(energy.unfreezing_list.is_empty());
    }

    #[test]
    fn test_effective_frozen_balance() {
        let mut energy = AccountEnergy::new();
        energy.frozen_balance = 100;
        energy.acquired_delegated_balance = 50;
        energy.delegated_frozen_balance = 30;

        // effective = 100 + 50 - 30 = 120
        assert_eq!(energy.effective_frozen_balance(), 120);
    }

    #[test]
    fn test_energy_limit_calculation() {
        let mut energy = AccountEnergy::new();
        energy.frozen_balance = 1_000_000;

        // 1M out of 100M total = 1% of 18.4B = 184M energy
        let limit = energy.calculate_energy_limit(100_000_000);
        assert_eq!(limit, 184_000_000);
    }

    #[test]
    fn test_energy_limit_zero_weight() {
        let energy = AccountEnergy::new();
        assert_eq!(energy.calculate_energy_limit(0), 0);
    }

    #[test]
    fn test_free_energy_availability() {
        let mut energy = AccountEnergy::new();
        let now_ms = 100_000_000u64; // 100 seconds

        // No usage, full free quota available
        assert_eq!(
            energy.calculate_free_energy_available(now_ms),
            FREE_ENERGY_QUOTA
        );

        // Use some free energy
        energy.consume_free_energy(500, now_ms);
        assert_eq!(energy.free_energy_usage, 500);
        // FREE_ENERGY_QUOTA (1500) - usage (500) = 1000 available
        assert_eq!(energy.calculate_free_energy_available(now_ms), 1000);

        // After 24 hours, free quota resets
        let next_day = now_ms + ENERGY_RECOVERY_WINDOW_MS;
        assert_eq!(
            energy.calculate_free_energy_available(next_day),
            FREE_ENERGY_QUOTA
        );
    }

    #[test]
    fn test_energy_decay_recovery() {
        let mut energy = AccountEnergy::new();
        energy.frozen_balance = 1_000_000;
        energy.energy_usage = 1000;
        energy.latest_consume_time = 0;

        let total_weight = 100_000_000u64;

        // Immediately after use, most usage remains
        assert!(
            energy.calculate_frozen_energy_available(1, total_weight)
                < energy.calculate_energy_limit(total_weight)
        );

        // After 12 hours (50%), usage is halved
        let half_day_ms = ENERGY_RECOVERY_WINDOW_MS / 2;
        let available = energy.calculate_frozen_energy_available(half_day_ms, total_weight);
        let limit = energy.calculate_energy_limit(total_weight);
        // Should be close to limit - 500 (half of 1000 usage recovered)
        assert!(available > limit - 600);
        assert!(available < limit);

        // After 24 hours, full energy available
        let full_day_ms = ENERGY_RECOVERY_WINDOW_MS;
        assert_eq!(
            energy.calculate_frozen_energy_available(full_day_ms, total_weight),
            limit
        );
    }

    #[test]
    fn test_freeze_and_unfreeze() {
        let mut energy = AccountEnergy::new();
        let now_ms = 0u64;

        // Freeze 1000 TOS
        energy.freeze(1000);
        assert_eq!(energy.frozen_balance, 1000);

        // Start unfreeze
        energy.start_unfreeze(500, now_ms).expect("test");
        assert_eq!(energy.frozen_balance, 500);
        assert_eq!(energy.unfreezing_list.len(), 1);
        assert_eq!(energy.unfreezing_list[0].unfreeze_amount, 500);

        // Cannot withdraw before expiry
        assert_eq!(energy.withdraw_expired_unfreeze(now_ms), 0);

        // Can withdraw after 14 days
        let after_14_days = now_ms + (UNFREEZE_DELAY_DAYS as u64 * 24 * 60 * 60 * 1000);
        let withdrawn = energy.withdraw_expired_unfreeze(after_14_days);
        assert_eq!(withdrawn, 500);
        assert!(energy.unfreezing_list.is_empty());
    }

    #[test]
    fn test_unfreeze_queue_limit() {
        let mut energy = AccountEnergy::new();
        energy.frozen_balance = 100_000;

        // Fill up the queue
        for i in 0..MAX_UNFREEZING_LIST_SIZE {
            energy.start_unfreeze(1, i as u64).expect("test");
        }

        // Queue is now full
        assert_eq!(energy.unfreezing_list.len(), MAX_UNFREEZING_LIST_SIZE);

        // Cannot add more
        let result = energy.start_unfreeze(1, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_cancel_all_unfreeze() {
        let mut energy = AccountEnergy::new();
        energy.frozen_balance = 1000;
        let now_ms = 0u64;

        // Start multiple unfreezes
        energy.start_unfreeze(100, now_ms).expect("test");
        energy.start_unfreeze(200, now_ms + 1000).expect("test");

        let after_14_days = now_ms + (UNFREEZE_DELAY_DAYS as u64 * 24 * 60 * 60 * 1000);

        // Cancel all - first one is expired, second is not
        let (withdrawn, cancelled) = energy.cancel_all_unfreeze(after_14_days);
        assert_eq!(withdrawn, 100); // Expired goes to balance
        assert_eq!(cancelled, 200); // Not expired goes back to frozen
        assert_eq!(energy.frozen_balance, 700 + 200); // Original 700 + cancelled 200
        assert!(energy.unfreezing_list.is_empty());
    }

    #[test]
    fn test_delegated_resource() {
        use crate::crypto::KeyPair;

        let from = KeyPair::new().get_public_key().compress();
        let to = KeyPair::new().get_public_key().compress();

        let delegation = DelegatedResource::new(from.clone(), to.clone(), 1000, 0);
        assert!(!delegation.is_locked(0));
        assert!(delegation.can_undelegate(0));

        // With lock
        let locked_delegation = DelegatedResource::new(from, to, 1000, 100_000);
        assert!(locked_delegation.is_locked(0));
        assert!(!locked_delegation.can_undelegate(0));
        assert!(!locked_delegation.is_locked(100_001));
        assert!(locked_delegation.can_undelegate(100_001));
    }

    #[test]
    fn test_global_energy_state() {
        let mut state = GlobalEnergyState::new();
        assert_eq!(state.total_energy_limit, TOTAL_ENERGY_LIMIT);
        assert_eq!(state.total_energy_weight, 0);

        state.add_weight(1000, 1);
        assert_eq!(state.total_energy_weight, 1000);
        assert_eq!(state.last_update, 1);

        state.remove_weight(500, 2);
        assert_eq!(state.total_energy_weight, 500);
        assert_eq!(state.last_update, 2);
    }

    #[test]
    fn test_account_energy_serialization() {
        let mut energy = AccountEnergy::new();
        energy.frozen_balance = 1000;
        energy.energy_usage = 500;
        energy.latest_consume_time = 12345;
        energy.unfreezing_list.push(UnfreezingRecord {
            unfreeze_amount: 100,
            unfreeze_expire_time: 999999,
        });

        let bytes = energy.to_bytes();
        let mut reader = crate::serializer::Reader::new(&bytes);
        let restored = AccountEnergy::read(&mut reader).expect("test");

        assert_eq!(energy.frozen_balance, restored.frozen_balance);
        assert_eq!(energy.energy_usage, restored.energy_usage);
        assert_eq!(energy.latest_consume_time, restored.latest_consume_time);
        assert_eq!(energy.unfreezing_list.len(), restored.unfreezing_list.len());
    }

    #[test]
    fn test_global_energy_state_serialization() {
        let mut state = GlobalEnergyState::new();
        state.total_energy_weight = 5000;
        state.last_update = 100;

        let bytes = state.to_bytes();
        let mut reader = crate::serializer::Reader::new(&bytes);
        let restored = GlobalEnergyState::read(&mut reader).expect("test");

        assert_eq!(state.total_energy_limit, restored.total_energy_limit);
        assert_eq!(state.total_energy_weight, restored.total_energy_weight);
        assert_eq!(state.last_update, restored.last_update);
    }

    #[test]
    fn test_delegated_resource_serialization() {
        use crate::crypto::KeyPair;

        let from = KeyPair::new().get_public_key().compress();
        let to = KeyPair::new().get_public_key().compress();

        // Test without lock
        let delegation = DelegatedResource::new(from.clone(), to.clone(), 1_000_000, 0);
        let bytes = delegation.to_bytes();
        let mut reader = crate::serializer::Reader::new(&bytes);
        let restored = DelegatedResource::read(&mut reader).expect("test");

        assert_eq!(delegation.from, restored.from);
        assert_eq!(delegation.to, restored.to);
        assert_eq!(delegation.frozen_balance, restored.frozen_balance);
        assert_eq!(delegation.expire_time, restored.expire_time);

        // Test with lock
        let locked_delegation = DelegatedResource::new(from, to, 500_000, 1_000_000_000);
        let bytes = locked_delegation.to_bytes();
        let mut reader = crate::serializer::Reader::new(&bytes);
        let restored = DelegatedResource::read(&mut reader).expect("test");

        assert_eq!(locked_delegation.frozen_balance, restored.frozen_balance);
        assert_eq!(locked_delegation.expire_time, restored.expire_time);
    }

    #[test]
    fn test_delegation_affects_energy_limit() {
        let total_weight = 100_000_000u64; // 100M TOS total staked

        // Account with 1M frozen (1% of total)
        let mut energy = AccountEnergy::new();
        energy.frozen_balance = 1_000_000;

        // Base energy = 1% of 18.4B = 184M
        let base_limit = energy.calculate_energy_limit(total_weight);
        assert_eq!(base_limit, 184_000_000);

        // After receiving delegation of 1M (doubles effective stake)
        energy.acquired_delegated_balance = 1_000_000;
        let boosted_limit = energy.calculate_energy_limit(total_weight);
        assert_eq!(boosted_limit, 368_000_000); // 2% = 368M

        // After delegating out 500K (reduces effective stake by half of delegation)
        energy.delegated_frozen_balance = 500_000;
        let reduced_limit = energy.calculate_energy_limit(total_weight);
        // effective = 1M + 1M - 0.5M = 1.5M = 1.5% of 18.4B = 276M
        assert_eq!(reduced_limit, 276_000_000);
    }

    #[test]
    fn test_account_energy_with_delegation_serialization() {
        let mut energy = AccountEnergy::new();
        energy.frozen_balance = 1_000_000;
        energy.delegated_frozen_balance = 200_000;
        energy.acquired_delegated_balance = 300_000;
        energy.energy_usage = 50_000;
        energy.latest_consume_time = 12345;
        energy.free_energy_usage = 500;
        energy.latest_free_consume_time = 12340;
        energy.unfreezing_list.push(UnfreezingRecord {
            unfreeze_amount: 100_000,
            unfreeze_expire_time: 999999,
        });

        let bytes = energy.to_bytes();
        let mut reader = crate::serializer::Reader::new(&bytes);
        let restored = AccountEnergy::read(&mut reader).expect("test");

        assert_eq!(energy.frozen_balance, restored.frozen_balance);
        assert_eq!(
            energy.delegated_frozen_balance,
            restored.delegated_frozen_balance
        );
        assert_eq!(
            energy.acquired_delegated_balance,
            restored.acquired_delegated_balance
        );
        assert_eq!(energy.energy_usage, restored.energy_usage);
        assert_eq!(energy.latest_consume_time, restored.latest_consume_time);
        assert_eq!(energy.free_energy_usage, restored.free_energy_usage);
        assert_eq!(
            energy.latest_free_consume_time,
            restored.latest_free_consume_time
        );
        assert_eq!(energy.unfreezing_list.len(), restored.unfreezing_list.len());
    }
}
