use crate::{
    block::TopoHeight,
    config::{
        ENERGY_RECOVERY_WINDOW_MS, FREE_ENERGY_QUOTA, MAX_UNFREEZING_LIST_SIZE,
        TOTAL_ENERGY_LIMIT, UNFREEZE_DELAY_DAYS,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl Default for AccountEnergy {
    fn default() -> Self {
        Self {
            frozen_balance: 0,
            delegated_frozen_balance: 0,
            acquired_delegated_balance: 0,
            energy_usage: 0,
            latest_consume_time: 0,
            free_energy_usage: 0,
            latest_free_consume_time: 0,
            unfreezing_list: Vec::new(),
        }
    }
}

impl AccountEnergy {
    pub fn new() -> Self {
        Self::default()
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
        let recovered =
            (self.free_energy_usage as u128 * elapsed as u128 / ENERGY_RECOVERY_WINDOW_MS as u128)
                as u64;
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
        let recovered =
            (self.energy_usage as u128 * elapsed as u128 / ENERGY_RECOVERY_WINDOW_MS as u128)
                as u64;
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
    pub fn start_unfreeze(&mut self, amount: u64, now_ms: u64) -> Result<(), &'static str> {
        if self.unfreezing_list.len() >= MAX_UNFREEZING_LIST_SIZE {
            return Err("Unfreezing queue is full (max 32 entries)");
        }

        if amount > self.frozen_balance {
            return Err("Insufficient frozen balance");
        }

        // Move from frozen to unfreezing queue
        self.frozen_balance = self.frozen_balance.saturating_sub(amount);

        let expire_time = now_ms + (UNFREEZE_DELAY_DAYS as u64 * 24 * 60 * 60 * 1000);
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
            .map(|r| r.unfreeze_amount)
            .sum()
    }

    /// Get amount that can be withdrawn now (expired entries)
    pub fn withdrawable_amount(&self, now_ms: u64) -> u64 {
        self.unfreezing_list
            .iter()
            .filter(|r| r.unfreeze_expire_time <= now_ms)
            .map(|r| r.unfreeze_amount)
            .sum()
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
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GlobalEnergyState {
    /// Total energy limit (constant: 18.4 billion)
    pub total_energy_limit: u64,
    /// Sum of all frozen TOS across all accounts (atomic units)
    pub total_energy_weight: u64,
    /// Last update topoheight
    pub last_update: TopoHeight,
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
// LEGACY ENERGY MODEL (deprecated)
// ============================================================================
//
// The following structures are deprecated and kept for backward compatibility.
// New code should use AccountEnergy, GlobalEnergyState, and DelegatedResource.

/// Flexible freeze duration for TOS staking
/// Users can set custom days from 3 to 180 days
///
/// # Deprecated
/// This struct is deprecated in favor of Stake 2.0 model which removes
/// duration-based freezing. Use `AccountEnergy` instead.
///
/// # Edge Cases
/// - Duration below 3 days will be rejected
/// - Duration above 180 days will be rejected
/// - Default duration is 3 days (minimum)
///
/// # Energy Calculation
/// Energy gained = 1 TOS × (2 × freeze_days)
/// Examples:
/// - 3 days: 1 TOS → 6 energy (6 free transfers)
/// - 7 days: 1 TOS → 14 energy (14 free transfers)
/// - 30 days: 1 TOS → 60 energy (60 free transfers)
#[deprecated(note = "Use AccountEnergy (Stake 2.0) instead")]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct FreezeDuration {
    /// Number of days to freeze (3-180 days)
    pub days: u32,
}

impl FreezeDuration {
    /// Create a new freeze duration with validation
    pub fn new(days: u32) -> Result<Self, &'static str> {
        if !(crate::config::MIN_FREEZE_DURATION_DAYS..=crate::config::MAX_FREEZE_DURATION_DAYS)
            .contains(&days)
        {
            return Err("Freeze duration must be between 3 and 180 days");
        }
        Ok(Self { days })
    }

    /// Get the reward multiplier for this freeze duration
    /// New energy model: 1 TOS = 2 * days energy (integer calculation)
    /// Examples:
    /// - 3 days: 6 energy (6 transfers)
    /// - 7 days: 14 energy (14 transfers)
    /// - 14 days: 28 energy (28 transfers)
    /// - 30 days: 60 energy (60 transfers)
    /// - 60 days: 120 energy (120 transfers)
    /// - 90 days: 180 energy (180 transfers)
    pub fn reward_multiplier(&self) -> u64 {
        (self.days * 2) as u64
    }

    /// Get the duration in blocks (assuming 1 block per second)
    /// Note: For network-specific duration, use `duration_in_blocks_for_network`
    pub fn duration_in_blocks(&self) -> u64 {
        self.days as u64 * 24 * 60 * 60 // days * 24 hours * 60 minutes * 60 seconds
    }

    /// Get the duration in blocks for a specific network
    /// - Mainnet/Testnet: 1 day = 86400 blocks
    /// - Devnet: 1 day = 10 blocks (accelerated for testing)
    pub fn duration_in_blocks_for_network(&self, network: &crate::network::Network) -> u64 {
        self.days as u64 * network.freeze_duration_multiplier()
    }

    /// Get the duration name for display
    pub fn name(&self) -> String {
        format!("{} days", self.days)
    }

    /// Get the number of days
    pub fn get_days(&self) -> u32 {
        self.days
    }

    /// Check if duration is valid (3-180 days)
    pub fn is_valid(&self) -> bool {
        self.days >= crate::config::MIN_FREEZE_DURATION_DAYS
            && self.days <= crate::config::MAX_FREEZE_DURATION_DAYS
    }
}

impl Default for FreezeDuration {
    fn default() -> Self {
        Self { days: 3 } // Default to minimum 3 days
    }
}

impl Serializer for FreezeDuration {
    fn write(&self, writer: &mut Writer) {
        writer.write_u32(&self.days);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let days = reader.read_u32()?;
        FreezeDuration::new(days).map_err(|_| ReaderError::InvalidValue)
    }

    fn size(&self) -> usize {
        self.days.size()
    }
}

/// Freeze record for tracking individual freeze operations
///
/// # Deprecated
/// This struct is deprecated in favor of Stake 2.0 model.
/// Use `AccountEnergy` with `UnfreezingRecord` instead.
///
/// # Edge Cases
/// - Only whole TOS amounts can be frozen (fractional parts are discarded)
/// - Unfreezing is only allowed after the unlock_topoheight is reached
/// - Partial unfreezing is supported - you can unfreeze less than the full amount
/// - Energy is calculated using integer arithmetic to avoid precision issues
///
/// # Important Notes
/// - Each freeze record tracks its own unlock time based on the freeze duration
/// - Energy gained is immutable once frozen (doesn't change with time)
/// - Unfreezing removes energy proportionally to the amount unfrozen
#[deprecated(note = "Use AccountEnergy with UnfreezingRecord (Stake 2.0) instead")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreezeRecord {
    /// Amount of TOS frozen
    pub amount: u64,
    /// Freeze duration
    pub duration: FreezeDuration,
    /// Topoheight when frozen
    pub freeze_topoheight: TopoHeight,
    /// Topoheight when can be unlocked
    pub unlock_topoheight: TopoHeight,
    /// Energy gained from this freeze
    pub energy_gained: u64,
}

impl FreezeRecord {
    /// Create a new freeze record (uses default mainnet timing)
    /// Ensures TOS amount is a whole number (multiple of COIN_VALUE)
    pub fn new(amount: u64, duration: FreezeDuration, freeze_topoheight: TopoHeight) -> Self {
        let unlock_topoheight = freeze_topoheight + duration.duration_in_blocks();

        // Ensure amount is a whole number of TOS (multiple of COIN_VALUE)
        let whole_tos_amount = (amount / crate::config::COIN_VALUE) * crate::config::COIN_VALUE;

        // Calculate energy gained using integer arithmetic
        let energy_gained =
            (whole_tos_amount / crate::config::COIN_VALUE) * duration.reward_multiplier();

        Self {
            amount: whole_tos_amount,
            duration,
            freeze_topoheight,
            unlock_topoheight,
            energy_gained,
        }
    }

    /// Create a new freeze record with network-specific timing
    /// - Mainnet/Testnet: Uses standard day-to-block conversion
    /// - Devnet: Uses accelerated timing for testing (1 day = 10 blocks)
    pub fn new_for_network(
        amount: u64,
        duration: FreezeDuration,
        freeze_topoheight: TopoHeight,
        network: &crate::network::Network,
    ) -> Self {
        let unlock_topoheight =
            freeze_topoheight + duration.duration_in_blocks_for_network(network);

        // Ensure amount is a whole number of TOS (multiple of COIN_VALUE)
        let whole_tos_amount = (amount / crate::config::COIN_VALUE) * crate::config::COIN_VALUE;

        // Calculate energy gained using integer arithmetic
        let energy_gained =
            (whole_tos_amount / crate::config::COIN_VALUE) * duration.reward_multiplier();

        Self {
            amount: whole_tos_amount,
            duration,
            freeze_topoheight,
            unlock_topoheight,
            energy_gained,
        }
    }

    /// Check if this freeze record can be unlocked at the given topoheight
    pub fn can_unlock(&self, current_topoheight: TopoHeight) -> bool {
        current_topoheight >= self.unlock_topoheight
    }

    /// Get remaining lock time in blocks
    pub fn remaining_blocks(&self, current_topoheight: TopoHeight) -> u64 {
        self.unlock_topoheight.saturating_sub(current_topoheight)
    }
}

impl Serializer for FreezeRecord {
    fn write(&self, writer: &mut Writer) {
        self.amount.write(writer);
        self.duration.write(writer);
        writer.write_u64(&self.freeze_topoheight);
        writer.write_u64(&self.unlock_topoheight);
        writer.write_u64(&self.energy_gained);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            amount: reader.read_u64()?,
            duration: FreezeDuration::read(reader)?,
            freeze_topoheight: reader.read_u64()?,
            unlock_topoheight: reader.read_u64()?,
            energy_gained: reader.read_u64()?,
        })
    }

    fn size(&self) -> usize {
        self.amount.size()
            + self.duration.size()
            + self.freeze_topoheight.size()
            + self.unlock_topoheight.size()
            + self.energy_gained.size()
    }
}

/// Energy resource management for TOS
/// Enhanced with freeze duration and reward multiplier system
///
/// # Deprecated
/// This struct is deprecated in favor of Stake 2.0 model.
/// Use `AccountEnergy` instead which provides:
/// - Proportional energy allocation
/// - 24-hour linear decay recovery
/// - Free quota support
/// - Delegation support
///
/// # Energy Model Overview (Legacy)
/// - Energy is consumed for transfer operations (1 energy per transfer)
/// - Energy is gained by freezing TOS for a specified duration
/// - Energy regenerates when used_energy is reset (periodic reset mechanism)
/// - Multiple freeze records with different durations can coexist
///
/// # Edge Cases and Limitations
/// - **Minimum freeze amount**: Only whole TOS amounts (multiples of COIN_VALUE)
/// - **Energy consumption**: Fails if insufficient energy available (no automatic TOS conversion)
/// - **Unfreezing constraints**: Only unlocked records can be unfrozen
/// - **Integer arithmetic**: All calculations use integers to avoid floating-point precision issues
/// - **Energy reset**: used_energy is reset periodically (timing depends on external calls)
///
/// # Behavioral Notes
/// - Freezing 0.5 TOS will actually freeze 0 TOS (rounded down)
/// - Unfreezing from multiple records follows FIFO order for unlocked records
/// - Energy total decreases when unfreezing (proportional to amount unfrozen)
#[deprecated(note = "Use AccountEnergy (Stake 2.0) instead")]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EnergyResource {
    /// Total energy available
    pub total_energy: u64,
    /// Used energy
    pub used_energy: u64,
    /// Total frozen TOS across all freeze records
    pub frozen_tos: u64,
    /// Last update topoheight
    pub last_update: TopoHeight,
    /// Individual freeze records for tracking duration-based rewards
    pub freeze_records: Vec<FreezeRecord>,
}

impl EnergyResource {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get available energy
    pub fn available_energy(&self) -> u64 {
        self.total_energy.saturating_sub(self.used_energy)
    }

    /// Check if has enough energy
    pub fn has_enough_energy(&self, required: u64) -> bool {
        self.available_energy() >= required
    }

    /// Consume energy
    pub fn consume_energy(&mut self, amount: u64) -> Result<(), &'static str> {
        if self.available_energy() < amount {
            return Err("Insufficient energy");
        }
        self.used_energy += amount;
        Ok(())
    }

    /// Freeze TOS to get energy with duration-based rewards
    /// Ensures TOS amount is a whole number (multiple of COIN_VALUE)
    /// Returns the actual amount of TOS frozen (may be less than requested if not whole number)
    pub fn freeze_tos_for_energy(
        &mut self,
        tos_amount: u64,
        duration: FreezeDuration,
        topoheight: TopoHeight,
    ) -> u64 {
        // Use default mainnet timing
        self.freeze_tos_for_energy_with_network(
            tos_amount,
            duration,
            topoheight,
            &crate::network::Network::Mainnet,
        )
    }

    /// Freeze TOS to get energy with network-specific timing
    /// - Mainnet/Testnet: Uses standard day-to-block conversion
    /// - Devnet: Uses accelerated timing for testing (1 day = 10 blocks)
    pub fn freeze_tos_for_energy_with_network(
        &mut self,
        tos_amount: u64,
        duration: FreezeDuration,
        topoheight: TopoHeight,
        network: &crate::network::Network,
    ) -> u64 {
        if tos_amount < crate::config::MIN_FREEZE_TOS_AMOUNT {
            return 0;
        }

        if !tos_amount.is_multiple_of(crate::config::COIN_VALUE) {
            return 0;
        }

        // Create a new freeze record with network-specific timing
        let freeze_record =
            FreezeRecord::new_for_network(tos_amount, duration, topoheight, network);
        let energy_gained = freeze_record.energy_gained;
        let actual_tos_frozen = freeze_record.amount;

        if actual_tos_frozen == 0 {
            return 0;
        }

        // Add to freeze records
        self.freeze_records.push(freeze_record);

        // Update totals
        self.frozen_tos += actual_tos_frozen;
        self.total_energy += energy_gained;
        self.last_update = topoheight;

        energy_gained
    }

    /// Unfreeze TOS from a specific freeze record
    /// Can only unfreeze records that have reached their unlock time
    /// Ensures TOS amount is a whole number (multiple of COIN_VALUE)
    pub fn unfreeze_tos(
        &mut self,
        tos_amount: u64,
        current_topoheight: TopoHeight,
    ) -> Result<u64, String> {
        // Ensure requested amount is a whole number of TOS
        let whole_tos_amount = (tos_amount / crate::config::COIN_VALUE) * crate::config::COIN_VALUE;

        if whole_tos_amount == 0 {
            return Err("Cannot unfreeze 0 TOS".to_string());
        }

        if self.frozen_tos < whole_tos_amount {
            return Err("Insufficient frozen TOS".to_string());
        }

        // Find eligible freeze records (unlocked and with sufficient amount)
        let mut remaining_to_unfreeze = whole_tos_amount;
        let mut total_energy_removed = 0;
        let mut records_to_remove = Vec::new();
        let mut records_to_modify = Vec::new();

        for (index, record) in self.freeze_records.iter().enumerate() {
            if !record.can_unlock(current_topoheight) {
                continue; // Skip records that haven't reached unlock time
            }

            if remaining_to_unfreeze == 0 {
                break;
            }

            let unfreeze_amount = std::cmp::min(remaining_to_unfreeze, record.amount);

            // Calculate energy to remove using integer arithmetic
            let energy_to_remove =
                (unfreeze_amount / crate::config::COIN_VALUE) * record.duration.reward_multiplier();

            total_energy_removed += energy_to_remove;
            remaining_to_unfreeze -= unfreeze_amount;

            // Mark record for removal if fully unfrozen
            if unfreeze_amount == record.amount {
                records_to_remove.push(index);
            } else {
                // Partially unfreeze the record
                records_to_modify.push((index, unfreeze_amount));
            }
        }

        if remaining_to_unfreeze > 0 {
            return Err("Insufficient unlocked TOS to unfreeze".to_string());
        }

        // Remove marked records (in reverse order to maintain indices)
        for &index in records_to_remove.iter().rev() {
            self.freeze_records.remove(index);
        }

        // Modify partially unfrozen records
        for (index, unfreeze_amount) in records_to_modify.iter().rev() {
            let record = &mut self.freeze_records[*index];
            record.amount -= unfreeze_amount;

            // Recalculate energy for the remaining amount
            let remaining_energy =
                (record.amount / crate::config::COIN_VALUE) * record.duration.reward_multiplier();
            record.energy_gained = remaining_energy;
        }

        // Update totals
        self.frozen_tos -= whole_tos_amount;
        self.total_energy = self.total_energy.saturating_sub(total_energy_removed);
        self.last_update = current_topoheight;

        Ok(total_energy_removed)
    }

    /// Get all freeze records that can be unlocked at the current topoheight
    pub fn get_unlockable_records(&self, current_topoheight: TopoHeight) -> Vec<&FreezeRecord> {
        self.freeze_records
            .iter()
            .filter(|record| record.can_unlock(current_topoheight))
            .collect()
    }

    /// Get total unlockable TOS amount at the current topoheight
    pub fn get_unlockable_tos(&self, current_topoheight: TopoHeight) -> u64 {
        self.get_unlockable_records(current_topoheight)
            .iter()
            .map(|record| record.amount)
            .sum()
    }

    /// Get freeze records grouped by duration
    pub fn get_freeze_records_by_duration(
        &self,
    ) -> std::collections::HashMap<FreezeDuration, Vec<&FreezeRecord>> {
        let mut grouped: std::collections::HashMap<FreezeDuration, Vec<&FreezeRecord>> =
            std::collections::HashMap::new();

        for record in &self.freeze_records {
            grouped.entry(record.duration).or_default().push(record);
        }

        grouped
    }

    /// Reset used energy (called periodically by the network)
    ///
    /// # When to call
    /// This should be called periodically by the network (e.g., daily) to restore
    /// energy usage. This typically happens every 24 hours.
    ///
    /// # Edge Cases
    /// - Resets used_energy to 0, making all total_energy available again
    /// - Does not affect frozen TOS or total energy amounts
    /// - Updates last_update timestamp to current topoheight
    /// - No validation on timing - caller must implement reset schedule
    pub fn reset_used_energy(&mut self, topoheight: TopoHeight) {
        self.used_energy = 0;
        self.last_update = topoheight;
    }
}

/// Energy lease contract
///
/// # Deprecated
/// This struct is deprecated and unused. Use `DelegatedResource` instead
/// for energy delegation in Stake 2.0 model.
#[deprecated(note = "Use DelegatedResource (Stake 2.0) instead")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnergyLease {
    /// Lessor (energy provider)
    pub lessor: PublicKey,
    /// Lessee (energy consumer)
    pub lessee: PublicKey,
    /// Amount of energy leased
    pub energy_amount: u64,
    /// Lease duration in blocks
    pub duration: u64,
    /// Start topoheight
    pub start_topoheight: TopoHeight,
    /// Price per energy unit
    pub price_per_energy: u64,
}

impl EnergyLease {
    pub fn new(
        lessor: PublicKey,
        lessee: PublicKey,
        energy_amount: u64,
        duration: u64,
        start_topoheight: TopoHeight,
        price_per_energy: u64,
    ) -> Self {
        Self {
            lessor,
            lessee,
            energy_amount,
            duration,
            start_topoheight,
            price_per_energy,
        }
    }

    /// Check if lease is still valid
    pub fn is_valid(&self, current_topoheight: TopoHeight) -> bool {
        current_topoheight < self.start_topoheight + self.duration
    }

    /// Calculate total cost
    pub fn total_cost(&self) -> u64 {
        self.energy_amount * self.price_per_energy
    }
}

impl Serializer for EnergyResource {
    fn write(&self, writer: &mut Writer) {
        writer.write_u64(&self.total_energy);
        writer.write_u64(&self.used_energy);
        writer.write_u64(&self.frozen_tos);
        writer.write_u64(&self.last_update);

        // Write freeze records
        writer.write_u64(&(self.freeze_records.len() as u64));
        for record in &self.freeze_records {
            record.write(writer);
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let total_energy = reader.read_u64()?;
        let used_energy = reader.read_u64()?;
        let frozen_tos = reader.read_u64()?;
        let last_update = reader.read_u64()?;

        // Read freeze records
        let records_count = reader.read_u64()? as usize;
        let mut freeze_records = Vec::with_capacity(records_count);
        for _ in 0..records_count {
            freeze_records.push(FreezeRecord::read(reader)?);
        }

        Ok(Self {
            total_energy,
            used_energy,
            frozen_tos,
            last_update,
            freeze_records,
        })
    }

    fn size(&self) -> usize {
        let base_size = self.total_energy.size()
            + self.used_energy.size()
            + self.frozen_tos.size()
            + self.last_update.size();
        let records_size = 8 + self.freeze_records.iter().map(|r| r.size()).sum::<usize>();
        base_size + records_size
    }
}

impl Serializer for EnergyLease {
    fn write(&self, writer: &mut Writer) {
        self.lessor.write(writer);
        self.lessee.write(writer);
        writer.write_u64(&self.energy_amount);
        writer.write_u64(&self.duration);
        writer.write_u64(&self.start_topoheight);
        writer.write_u64(&self.price_per_energy);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            lessor: PublicKey::read(reader)?,
            lessee: PublicKey::read(reader)?,
            energy_amount: reader.read_u64()?,
            duration: reader.read_u64()?,
            start_topoheight: reader.read_u64()?,
            price_per_energy: reader.read_u64()?,
        })
    }

    fn size(&self) -> usize {
        self.lessor.size()
            + self.lessee.size()
            + self.energy_amount.size()
            + self.duration.size()
            + self.start_topoheight.size()
            + self.price_per_energy.size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_freeze_duration_reward_multipliers() {
        let duration = FreezeDuration::new(3).unwrap();
        assert_eq!(duration.reward_multiplier(), 6);
        let duration = FreezeDuration::new(7).unwrap();
        assert_eq!(duration.reward_multiplier(), 14);
        let duration = FreezeDuration::new(14).unwrap();
        assert_eq!(duration.reward_multiplier(), 28);
        let duration = FreezeDuration::new(30).unwrap();
        assert_eq!(duration.reward_multiplier(), 60);
        let duration = FreezeDuration::new(60).unwrap();
        assert_eq!(duration.reward_multiplier(), 120);
        let duration = FreezeDuration::new(90).unwrap();
        assert_eq!(duration.reward_multiplier(), 180);
    }

    #[test]
    fn test_freeze_duration_blocks() {
        let duration = FreezeDuration::new(3).unwrap();
        assert_eq!(duration.duration_in_blocks(), 3 * 24 * 60 * 60);
        let duration = FreezeDuration::new(7).unwrap();
        assert_eq!(duration.duration_in_blocks(), 7 * 24 * 60 * 60);
        let duration = FreezeDuration::new(14).unwrap();
        assert_eq!(duration.duration_in_blocks(), 14 * 24 * 60 * 60);
        let duration = FreezeDuration::new(30).unwrap();
        assert_eq!(duration.duration_in_blocks(), 30 * 24 * 60 * 60);
        let duration = FreezeDuration::new(60).unwrap();
        assert_eq!(duration.duration_in_blocks(), 60 * 24 * 60 * 60);
        let duration = FreezeDuration::new(90).unwrap();
        assert_eq!(duration.duration_in_blocks(), 90 * 24 * 60 * 60);
    }

    #[test]
    fn test_freeze_record_creation() {
        let duration = FreezeDuration::new(7).unwrap();
        let record = FreezeRecord::new(100000000, duration, 100); // 1 TOS
        assert_eq!(record.amount, 100000000);
        assert_eq!(record.duration, duration);
        assert_eq!(record.freeze_topoheight, 100);
        assert_eq!(record.unlock_topoheight, 100 + 7 * 24 * 60 * 60);
        assert_eq!(record.energy_gained, 14); // 1 TOS * 14 = 14 transfers
    }

    #[test]
    fn test_freeze_record_unlock_check() {
        let duration = FreezeDuration::new(3).unwrap();
        let record = FreezeRecord::new(1000, duration, 100);
        let unlock_time = 100 + 3 * 24 * 60 * 60;

        assert!(!record.can_unlock(unlock_time - 1));
        assert!(record.can_unlock(unlock_time));
        assert!(record.can_unlock(unlock_time + 1000));
    }

    #[test]
    fn test_energy_resource_freeze_with_duration() {
        let mut resource = EnergyResource::new();
        let topoheight = 1000;

        // Freeze 1 TOS for 7 days
        let duration = FreezeDuration::new(7).unwrap();
        let energy_gained = resource.freeze_tos_for_energy(100000000, duration, topoheight);
        assert_eq!(energy_gained, 14); // 1 TOS * 14 = 14 transfers
        assert_eq!(resource.frozen_tos, 100000000);
        assert_eq!(resource.total_energy, 14);
        assert_eq!(resource.freeze_records.len(), 1);

        // Freeze 1 TOS for 14 days
        let duration = FreezeDuration::new(14).unwrap();
        let energy_gained2 = resource.freeze_tos_for_energy(100000000, duration, topoheight);
        assert_eq!(energy_gained2, 28); // 1 TOS * 28 = 28 transfers
        assert_eq!(resource.frozen_tos, 200000000);
        assert_eq!(resource.total_energy, 42);
        assert_eq!(resource.freeze_records.len(), 2);
    }

    #[test]
    fn test_energy_resource_unfreeze() {
        let mut resource = EnergyResource::new();
        let freeze_topoheight = 1000;
        let duration = FreezeDuration::new(7).unwrap();
        let unlock_topoheight = freeze_topoheight + duration.duration_in_blocks();

        // Freeze 2 TOS for 7 days
        resource.freeze_tos_for_energy(200000000, duration, freeze_topoheight);

        // Try to unfreeze before unlock time (should fail)
        let result = resource.unfreeze_tos(100000000, unlock_topoheight - 1);
        assert!(result.is_err());

        // Unfreeze after unlock time
        let energy_removed = resource.unfreeze_tos(100000000, unlock_topoheight).unwrap();
        assert_eq!(energy_removed, 14); // 1 TOS * 14 = 14 transfers
        assert_eq!(resource.frozen_tos, 100000000);
        assert_eq!(resource.total_energy, 14);
    }

    #[test]
    fn test_get_unlockable_records() {
        let mut resource = EnergyResource::new();
        let topoheight = 1000;

        // Freeze with different durations
        let duration3 = FreezeDuration::new(3).unwrap();
        resource.freeze_tos_for_energy(100000000, duration3, topoheight); // 1 TOS

        let duration7 = FreezeDuration::new(7).unwrap();
        resource.freeze_tos_for_energy(100000000, duration7, topoheight); // 1 TOS

        let duration14 = FreezeDuration::new(14).unwrap();
        resource.freeze_tos_for_energy(100000000, duration14, topoheight); // 1 TOS

        // Check unlockable records at different times
        let unlockable_3d = resource.get_unlockable_records(topoheight + 3 * 24 * 60 * 60);
        assert_eq!(unlockable_3d.len(), 1);

        let unlockable_7d = resource.get_unlockable_records(topoheight + 7 * 24 * 60 * 60);
        assert_eq!(unlockable_7d.len(), 2);

        let unlockable_14d = resource.get_unlockable_records(topoheight + 14 * 24 * 60 * 60);
        assert_eq!(unlockable_14d.len(), 3);
    }

    #[test]
    fn test_serialization() {
        let mut resource = EnergyResource::new();
        let duration = FreezeDuration::new(7).unwrap();
        resource.freeze_tos_for_energy(100000000, duration, 1000); // 1 TOS

        let duration = FreezeDuration::new(14).unwrap();
        resource.freeze_tos_for_energy(100000000, duration, 1000); // 1 TOS

        let mut bytes = Vec::new();
        let mut writer = crate::serializer::Writer::new(&mut bytes);
        resource.write(&mut writer);

        let mut reader = crate::serializer::Reader::new(&bytes);
        let deserialized = EnergyResource::read(&mut reader).unwrap();

        assert_eq!(resource.total_energy, deserialized.total_energy);
        assert_eq!(resource.frozen_tos, deserialized.frozen_tos);
        assert_eq!(
            resource.freeze_records.len(),
            deserialized.freeze_records.len()
        );
    }

    #[test]
    fn test_freeze_duration_serialization() {
        let durations = [
            FreezeDuration::new(3).unwrap(),
            FreezeDuration::new(7).unwrap(),
            FreezeDuration::new(14).unwrap(),
        ];

        for duration in &durations {
            let mut bytes = Vec::new();
            let mut writer = crate::serializer::Writer::new(&mut bytes);
            duration.write(&mut writer);

            let mut reader = crate::serializer::Reader::new(&bytes);
            let deserialized = FreezeDuration::read(&mut reader).unwrap();

            assert_eq!(duration, &deserialized);
        }
    }

    #[test]
    fn test_freeze_record_serialization() {
        let duration = FreezeDuration::new(7).unwrap();
        let record = FreezeRecord::new(100000000, duration, 100); // 1 TOS

        let mut bytes = Vec::new();
        let mut writer = crate::serializer::Writer::new(&mut bytes);
        record.write(&mut writer);

        let mut reader = crate::serializer::Reader::new(&bytes);
        let deserialized = FreezeRecord::read(&mut reader).unwrap();

        assert_eq!(record.amount, deserialized.amount);
        assert_eq!(record.duration, deserialized.duration);
        assert_eq!(record.energy_gained, deserialized.energy_gained);
    }
}
