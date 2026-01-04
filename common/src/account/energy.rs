use crate::{
    block::TopoHeight,
    crypto::PublicKey,
    serializer::{Reader, ReaderError, Serializer, Writer},
};
use serde::{Deserialize, Serialize};

/// Flexible freeze duration for TOS staking
/// Users can set custom days from 3 to 180 days
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
            return Err("Freeze duration must be between 3 and 365 days");
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

/// Individual delegatee entry within a batch delegation
///
/// # Fields
/// - `delegatee`: The account receiving delegated energy
/// - `amount`: Amount of TOS delegated to this delegatee
/// - `energy`: Energy amount delegated (calculated from amount and duration)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegateRecordEntry {
    /// Delegatee account (receives energy)
    pub delegatee: PublicKey,
    /// Amount of TOS delegated
    pub amount: u64,
    /// Energy delegated to this delegatee
    pub energy: u64,
}

impl Serializer for DelegateRecordEntry {
    fn write(&self, writer: &mut Writer) {
        self.delegatee.write(writer);
        writer.write_u64(&self.amount);
        writer.write_u64(&self.energy);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            delegatee: PublicKey::read(reader)?,
            amount: reader.read_u64()?,
            energy: reader.read_u64()?,
        })
    }

    fn size(&self) -> usize {
        self.delegatee.size() + self.amount.size() + self.energy.size()
    }
}

/// Delegation freeze record (supports batch delegation up to 500 delegatees)
///
/// # Design Notes
/// - One batch delegation = one freeze record (counts toward 32-record limit)
/// - All entries share the same duration and unlock time
/// - Energy is calculated per-delegatee: amount * 2 * days / COIN_VALUE
///
/// # Edge Cases
/// - Minimum 1 TOS per delegatee to prevent dust entries
/// - Delegator cannot include self in delegatees list
/// - No duplicate addresses in delegatees list
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegatedFreezeRecord {
    /// Individual delegatee entries
    pub entries: Vec<DelegateRecordEntry>,
    /// Freeze duration (shared by all entries)
    pub duration: FreezeDuration,
    /// Topoheight when frozen
    pub freeze_topoheight: TopoHeight,
    /// Topoheight when can be unlocked
    pub unlock_topoheight: TopoHeight,
    /// Total amount delegated (sum of all entry amounts)
    pub total_amount: u64,
    /// Total energy delegated (sum of all entry energies)
    pub total_energy: u64,
}

impl DelegatedFreezeRecord {
    /// Create a new delegated freeze record
    pub fn new(
        entries: Vec<DelegateRecordEntry>,
        duration: FreezeDuration,
        freeze_topoheight: TopoHeight,
        network: &crate::network::Network,
    ) -> Self {
        let unlock_topoheight =
            freeze_topoheight + duration.duration_in_blocks_for_network(network);
        let total_amount = entries.iter().map(|e| e.amount).sum();
        let total_energy = entries.iter().map(|e| e.energy).sum();

        Self {
            entries,
            duration,
            freeze_topoheight,
            unlock_topoheight,
            total_amount,
            total_energy,
        }
    }

    /// Check if this delegation record can be unlocked
    pub fn can_unlock(&self, current_topoheight: TopoHeight) -> bool {
        current_topoheight >= self.unlock_topoheight
    }

    /// Find entry by delegatee address
    pub fn find_entry(&self, delegatee: &PublicKey) -> Option<&DelegateRecordEntry> {
        self.entries.iter().find(|e| &e.delegatee == delegatee)
    }

    /// Find entry index by delegatee address
    pub fn find_entry_index(&self, delegatee: &PublicKey) -> Option<usize> {
        self.entries.iter().position(|e| &e.delegatee == delegatee)
    }
}

impl Serializer for DelegatedFreezeRecord {
    fn write(&self, writer: &mut Writer) {
        writer.write_u64(&(self.entries.len() as u64));
        for entry in &self.entries {
            entry.write(writer);
        }
        self.duration.write(writer);
        writer.write_u64(&self.freeze_topoheight);
        writer.write_u64(&self.unlock_topoheight);
        writer.write_u64(&self.total_amount);
        writer.write_u64(&self.total_energy);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let entries_count = reader.read_u64()? as usize;
        let mut entries = Vec::with_capacity(entries_count);
        for _ in 0..entries_count {
            entries.push(DelegateRecordEntry::read(reader)?);
        }
        Ok(Self {
            entries,
            duration: FreezeDuration::read(reader)?,
            freeze_topoheight: reader.read_u64()?,
            unlock_topoheight: reader.read_u64()?,
            total_amount: reader.read_u64()?,
            total_energy: reader.read_u64()?,
        })
    }

    fn size(&self) -> usize {
        8 + self.entries.iter().map(|e| e.size()).sum::<usize>()
            + self.duration.size()
            + self.freeze_topoheight.size()
            + self.unlock_topoheight.size()
            + self.total_amount.size()
            + self.total_energy.size()
    }
}

/// Pending unfreeze record for two-phase unfreeze
///
/// # Two-Phase Unfreeze Process
/// 1. Phase 1 (UnfreezeTos): Creates PendingUnfreeze, removes energy immediately
/// 2. Phase 2 (WithdrawUnfrozen): After cooldown, TOS returned to balance
///
/// # Design Notes
/// - 14-day cooldown prevents rapid stake/unstake cycling
/// - Energy is removed immediately in Phase 1 (not at withdraw time)
/// - PendingUnfreeze is source-agnostic (same structure for self-freeze and delegation)
/// - Maximum 32 pending unfreezes per account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingUnfreeze {
    /// Amount of TOS pending withdrawal
    pub amount: u64,
    /// Topoheight when this unfreeze can be withdrawn
    pub expire_topoheight: TopoHeight,
}

impl PendingUnfreeze {
    /// Create a new pending unfreeze record
    pub fn new(amount: u64, current_topoheight: TopoHeight) -> Self {
        Self {
            amount,
            expire_topoheight: current_topoheight + crate::config::UNFREEZE_COOLDOWN_BLOCKS,
        }
    }

    /// Check if this pending unfreeze has expired (ready for withdrawal)
    pub fn is_expired(&self, current_topoheight: TopoHeight) -> bool {
        current_topoheight >= self.expire_topoheight
    }
}

impl Serializer for PendingUnfreeze {
    fn write(&self, writer: &mut Writer) {
        writer.write_u64(&self.amount);
        writer.write_u64(&self.expire_topoheight);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            amount: reader.read_u64()?,
            expire_topoheight: reader.read_u64()?,
        })
    }

    fn size(&self) -> usize {
        self.amount.size() + self.expire_topoheight.size()
    }
}

/// Energy resource management for TOS
/// Enhanced with TRON-style freeze duration and reward multiplier system
///
/// # Energy Model Overview
/// - Energy is consumed for transfer operations (1 energy per transfer)
/// - Energy is gained by freezing TOS for a specified duration (self-freeze or delegation)
/// - Energy regenerates when used_energy is reset (24-hour reset cycle)
/// - Multiple freeze records with different durations can coexist
/// - Supports batch delegation (up to 500 delegatees per transaction)
///
/// # Energy Sources
/// - **total_energy**: From self-frozen TOS (sum of freeze_records.energy_gained)
/// - **delegated_energy**: From delegators (sum of delegations TO this account)
/// - **available_energy**: max(0, total_energy + delegated_energy - used_energy)
///
/// # Edge Cases and Limitations
/// - **Minimum freeze amount**: Only whole TOS amounts (multiples of COIN_VALUE)
/// - **Energy consumption**: Fails if insufficient energy available (no automatic TOS conversion)
/// - **Unfreezing constraints**: Only unlocked records can be unfrozen
/// - **Integer arithmetic**: All calculations use integers to avoid floating-point precision issues
/// - **Energy reset**: used_energy resets after 24 hours (lazy trigger on first tx)
/// - **Record limits**: Max 32 freeze records, max 32 pending unfreezes per account
///
/// # Two-Phase Unfreeze
/// - Phase 1 (UnfreezeTos): Creates PendingUnfreeze, removes energy immediately
/// - Phase 2 (WithdrawUnfrozen): After 14-day cooldown, TOS returned to balance
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EnergyResource {
    /// Total energy from self-frozen TOS (sum of freeze_records.energy_gained)
    pub total_energy: u64,
    /// Energy received from delegators (authoritative source)
    pub delegated_energy: u64,
    /// Used energy (reset every 24 hours)
    pub used_energy: u64,
    /// Total frozen TOS across all freeze records (self + delegated)
    pub frozen_tos: u64,
    /// Last update topoheight
    pub last_update: TopoHeight,
    /// Last energy reset topoheight (for 24-hour reset cycle)
    pub last_reset_topoheight: TopoHeight,
    /// Individual self-freeze records
    pub freeze_records: Vec<FreezeRecord>,
    /// Delegation freeze records (as delegator - what I delegated to others)
    pub delegated_records: Vec<DelegatedFreezeRecord>,
    /// Pending unfreeze records (14-day cooldown before withdrawal)
    pub pending_unfreezes: Vec<PendingUnfreeze>,
}

impl EnergyResource {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get available energy (includes delegated energy, uses saturating arithmetic)
    /// Formula: max(0, total_energy + delegated_energy - used_energy)
    pub fn available_energy(&self) -> u64 {
        let sum = self.total_energy.saturating_add(self.delegated_energy);
        sum.saturating_sub(self.used_energy)
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
        self.used_energy = self.used_energy.saturating_add(amount);
        Ok(())
    }

    /// Check and perform energy reset if due (24-hour cycle)
    /// Returns true if reset was performed
    pub fn maybe_reset_energy(&mut self, current_topoheight: TopoHeight) -> bool {
        let reset_period = crate::config::BLOCKS_PER_DAY;
        if current_topoheight >= self.last_reset_topoheight.saturating_add(reset_period) {
            self.used_energy = 0;
            self.last_reset_topoheight = current_topoheight;
            true
        } else {
            false
        }
    }

    /// Get total record count (self-freeze + delegation)
    pub fn total_record_count(&self) -> usize {
        self.freeze_records.len() + self.delegated_records.len()
    }

    /// Check if can add more freeze records
    pub fn can_add_freeze_record(&self) -> bool {
        self.total_record_count() < crate::config::MAX_FREEZE_RECORDS
    }

    /// Check if can add more pending unfreezes
    pub fn can_add_pending_unfreeze(&self) -> bool {
        self.pending_unfreezes.len() < crate::config::MAX_PENDING_UNFREEZES
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

    /// Unfreeze TOS from self-freeze records (Phase 1 of two-phase unfreeze)
    /// Creates a PendingUnfreeze with 14-day cooldown
    /// Energy is removed immediately, TOS returned after cooldown via WithdrawUnfrozen
    ///
    /// # Arguments
    /// - `tos_amount`: Amount of TOS to unfreeze (must be whole TOS multiples)
    /// - `current_topoheight`: Current blockchain topoheight
    ///
    /// # Returns
    /// - Ok((energy_removed, pending_amount)): Energy removed and TOS amount pending
    /// - Err: If insufficient unlocked TOS or record limit exceeded
    pub fn unfreeze_tos(
        &mut self,
        tos_amount: u64,
        current_topoheight: TopoHeight,
    ) -> Result<(u64, u64), String> {
        // Ensure requested amount is a whole number of TOS
        let whole_tos_amount = (tos_amount / crate::config::COIN_VALUE) * crate::config::COIN_VALUE;

        if whole_tos_amount == 0 {
            return Err("Cannot unfreeze 0 TOS".to_string());
        }

        if whole_tos_amount < crate::config::MIN_UNFREEZE_TOS_AMOUNT {
            return Err("Amount below minimum unfreeze amount".to_string());
        }

        if self.frozen_tos < whole_tos_amount {
            return Err("Insufficient frozen TOS".to_string());
        }

        // Check if can add pending unfreeze
        if !self.can_add_pending_unfreeze() {
            return Err("Maximum pending unfreezes reached".to_string());
        }

        // Find eligible freeze records (unlocked and with sufficient amount)
        let mut remaining_to_unfreeze = whole_tos_amount;
        let mut total_energy_removed: u64 = 0;
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

            // Calculate energy to remove using FLOOR rounding (integer division)
            let energy_to_remove =
                (unfreeze_amount / crate::config::COIN_VALUE) * record.duration.reward_multiplier();

            total_energy_removed = total_energy_removed.saturating_add(energy_to_remove);
            remaining_to_unfreeze = remaining_to_unfreeze.saturating_sub(unfreeze_amount);

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
            record.amount = record.amount.saturating_sub(*unfreeze_amount);

            // Recalculate energy for the remaining amount (FLOOR rounding)
            let remaining_energy =
                (record.amount / crate::config::COIN_VALUE) * record.duration.reward_multiplier();
            record.energy_gained = remaining_energy;
        }

        // Update totals - energy removed immediately
        self.frozen_tos = self.frozen_tos.saturating_sub(whole_tos_amount);
        self.total_energy = self.total_energy.saturating_sub(total_energy_removed);
        self.last_update = current_topoheight;

        // Create pending unfreeze (Phase 1 complete, Phase 2 after 14 days)
        let pending = PendingUnfreeze::new(whole_tos_amount, current_topoheight);
        self.pending_unfreezes.push(pending);

        Ok((total_energy_removed, whole_tos_amount))
    }

    /// Withdraw unfrozen TOS after cooldown period (Phase 2 of two-phase unfreeze)
    /// Returns expired pending unfreeze amounts to the caller's balance
    ///
    /// # Returns
    /// - Total TOS amount ready for withdrawal (sum of all expired pending unfreezes)
    pub fn withdraw_unfrozen(&mut self, current_topoheight: TopoHeight) -> u64 {
        let mut total_withdrawn = 0u64;
        let mut remaining_unfreezes = Vec::new();

        for pending in self.pending_unfreezes.drain(..) {
            if pending.is_expired(current_topoheight) {
                total_withdrawn = total_withdrawn.saturating_add(pending.amount);
            } else {
                remaining_unfreezes.push(pending);
            }
        }

        self.pending_unfreezes = remaining_unfreezes;
        self.last_update = current_topoheight;

        total_withdrawn
    }

    /// Get total pending unfreeze amount (not yet withdrawn)
    pub fn total_pending_unfreeze(&self) -> u64 {
        self.pending_unfreezes.iter().map(|p| p.amount).sum()
    }

    /// Get withdrawable unfreeze amount (expired pending unfreezes)
    pub fn withdrawable_unfreeze(&self, current_topoheight: TopoHeight) -> u64 {
        self.pending_unfreezes
            .iter()
            .filter(|p| p.is_expired(current_topoheight))
            .map(|p| p.amount)
            .sum()
    }

    /// Add delegated energy (called on delegatee's account when receiving delegation)
    pub fn add_delegated_energy(&mut self, energy_amount: u64, topoheight: TopoHeight) {
        self.delegated_energy = self.delegated_energy.saturating_add(energy_amount);
        self.last_update = topoheight;
    }

    /// Remove delegated energy (called on delegatee's account when delegator unfreezes)
    pub fn remove_delegated_energy(&mut self, energy_amount: u64, topoheight: TopoHeight) {
        self.delegated_energy = self.delegated_energy.saturating_sub(energy_amount);
        self.last_update = topoheight;
    }

    /// Create a delegated freeze record (as delegator)
    /// Returns the total energy delegated
    pub fn create_delegated_freeze(
        &mut self,
        entries: Vec<DelegateRecordEntry>,
        duration: FreezeDuration,
        total_amount: u64,
        topoheight: TopoHeight,
        network: &crate::network::Network,
    ) -> Result<u64, String> {
        if !self.can_add_freeze_record() {
            return Err("Maximum freeze records reached".to_string());
        }

        if entries.is_empty() {
            return Err("No delegatees specified".to_string());
        }

        if entries.len() > crate::config::MAX_DELEGATEES {
            return Err("Too many delegatees".to_string());
        }

        let record = DelegatedFreezeRecord::new(entries, duration, topoheight, network);
        let total_energy = record.total_energy;

        // Update frozen TOS (delegator's TOS is locked)
        self.frozen_tos = self.frozen_tos.saturating_add(total_amount);
        self.last_update = topoheight;

        // Add delegated freeze record
        self.delegated_records.push(record);

        Ok(total_energy)
    }

    /// Unfreeze from delegated records (Phase 1 of two-phase unfreeze for delegations)
    /// Returns (energy_removed_per_delegatee, pending_amount)
    #[allow(clippy::type_complexity)]
    pub fn unfreeze_delegated(
        &mut self,
        tos_amount: u64,
        current_topoheight: TopoHeight,
    ) -> Result<(Vec<(PublicKey, u64)>, u64), String> {
        let whole_tos_amount = (tos_amount / crate::config::COIN_VALUE) * crate::config::COIN_VALUE;

        if whole_tos_amount == 0 {
            return Err("Cannot unfreeze 0 TOS".to_string());
        }

        if whole_tos_amount < crate::config::MIN_UNFREEZE_TOS_AMOUNT {
            return Err("Amount below minimum unfreeze amount".to_string());
        }

        if !self.can_add_pending_unfreeze() {
            return Err("Maximum pending unfreezes reached".to_string());
        }

        // Calculate total delegated TOS
        let total_delegated: u64 = self
            .delegated_records
            .iter()
            .filter(|r| r.can_unlock(current_topoheight))
            .map(|r| r.total_amount)
            .sum();

        if total_delegated < whole_tos_amount {
            return Err("Insufficient unlocked delegated TOS".to_string());
        }

        let mut remaining_to_unfreeze = whole_tos_amount;
        let mut energy_per_delegatee: Vec<(PublicKey, u64)> = Vec::new();
        let mut records_to_remove = Vec::new();
        let mut records_to_modify: Vec<(usize, Vec<(usize, u64)>, Vec<usize>)> = Vec::new();

        for (record_idx, record) in self.delegated_records.iter().enumerate() {
            if !record.can_unlock(current_topoheight) {
                continue;
            }

            if remaining_to_unfreeze == 0 {
                break;
            }

            let mut entry_removals = Vec::new();
            let mut entry_modifications = Vec::new();
            let mut record_energy_changes: Vec<(PublicKey, u64)> = Vec::new();

            for (entry_idx, entry) in record.entries.iter().enumerate() {
                if remaining_to_unfreeze == 0 {
                    break;
                }

                let unfreeze_from_entry = std::cmp::min(remaining_to_unfreeze, entry.amount);
                let energy_to_remove = (unfreeze_from_entry / crate::config::COIN_VALUE)
                    * record.duration.reward_multiplier();

                remaining_to_unfreeze = remaining_to_unfreeze.saturating_sub(unfreeze_from_entry);
                record_energy_changes.push((entry.delegatee.clone(), energy_to_remove));

                if unfreeze_from_entry == entry.amount {
                    entry_removals.push(entry_idx);
                } else {
                    entry_modifications.push((entry_idx, unfreeze_from_entry));
                }
            }

            energy_per_delegatee.extend(record_energy_changes);

            if entry_removals.len() == record.entries.len() {
                records_to_remove.push(record_idx);
            } else if !entry_removals.is_empty() || !entry_modifications.is_empty() {
                records_to_modify.push((record_idx, entry_modifications, entry_removals));
            }
        }

        if remaining_to_unfreeze > 0 {
            return Err("Insufficient unlocked delegated TOS".to_string());
        }

        // Apply modifications in reverse order
        for (record_idx, modifications, removals) in records_to_modify.into_iter().rev() {
            let record = &mut self.delegated_records[record_idx];

            // Apply modifications
            for (entry_idx, unfreeze_amount) in modifications.iter().rev() {
                let entry = &mut record.entries[*entry_idx];
                let energy_removed = (*unfreeze_amount / crate::config::COIN_VALUE)
                    * record.duration.reward_multiplier();
                entry.amount = entry.amount.saturating_sub(*unfreeze_amount);
                entry.energy = entry.energy.saturating_sub(energy_removed);
            }

            // Remove entries in reverse order
            for entry_idx in removals.into_iter().rev() {
                record.entries.remove(entry_idx);
            }

            // Recalculate totals
            record.total_amount = record.entries.iter().map(|e| e.amount).sum();
            record.total_energy = record.entries.iter().map(|e| e.energy).sum();
        }

        // Remove empty records
        for idx in records_to_remove.into_iter().rev() {
            self.delegated_records.remove(idx);
        }

        // Update frozen TOS
        self.frozen_tos = self.frozen_tos.saturating_sub(whole_tos_amount);
        self.last_update = current_topoheight;

        // Create pending unfreeze
        let pending = PendingUnfreeze::new(whole_tos_amount, current_topoheight);
        self.pending_unfreezes.push(pending);

        Ok((energy_per_delegatee, whole_tos_amount))
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
    /// energy usage. In TRON-like systems, this typically happens every 24 hours.
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
        writer.write_u64(&self.delegated_energy);
        writer.write_u64(&self.used_energy);
        writer.write_u64(&self.frozen_tos);
        writer.write_u64(&self.last_update);
        writer.write_u64(&self.last_reset_topoheight);

        // Write self-freeze records
        writer.write_u64(&(self.freeze_records.len() as u64));
        for record in &self.freeze_records {
            record.write(writer);
        }

        // Write delegated freeze records
        writer.write_u64(&(self.delegated_records.len() as u64));
        for record in &self.delegated_records {
            record.write(writer);
        }

        // Write pending unfreezes
        writer.write_u64(&(self.pending_unfreezes.len() as u64));
        for pending in &self.pending_unfreezes {
            pending.write(writer);
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let total_energy = reader.read_u64()?;
        let delegated_energy = reader.read_u64()?;
        let used_energy = reader.read_u64()?;
        let frozen_tos = reader.read_u64()?;
        let last_update = reader.read_u64()?;
        let last_reset_topoheight = reader.read_u64()?;

        // Read self-freeze records
        let records_count = reader.read_u64()? as usize;
        let mut freeze_records = Vec::with_capacity(records_count);
        for _ in 0..records_count {
            freeze_records.push(FreezeRecord::read(reader)?);
        }

        // Read delegated freeze records
        let delegated_count = reader.read_u64()? as usize;
        let mut delegated_records = Vec::with_capacity(delegated_count);
        for _ in 0..delegated_count {
            delegated_records.push(DelegatedFreezeRecord::read(reader)?);
        }

        // Read pending unfreezes
        let pending_count = reader.read_u64()? as usize;
        let mut pending_unfreezes = Vec::with_capacity(pending_count);
        for _ in 0..pending_count {
            pending_unfreezes.push(PendingUnfreeze::read(reader)?);
        }

        Ok(Self {
            total_energy,
            delegated_energy,
            used_energy,
            frozen_tos,
            last_update,
            last_reset_topoheight,
            freeze_records,
            delegated_records,
            pending_unfreezes,
        })
    }

    fn size(&self) -> usize {
        let base_size = self.total_energy.size()
            + self.delegated_energy.size()
            + self.used_energy.size()
            + self.frozen_tos.size()
            + self.last_update.size()
            + self.last_reset_topoheight.size();
        let freeze_records_size = 8 + self.freeze_records.iter().map(|r| r.size()).sum::<usize>();
        let delegated_records_size = 8 + self
            .delegated_records
            .iter()
            .map(|r| r.size())
            .sum::<usize>();
        let pending_unfreezes_size = 8 + self
            .pending_unfreezes
            .iter()
            .map(|p| p.size())
            .sum::<usize>();
        base_size + freeze_records_size + delegated_records_size + pending_unfreezes_size
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

        // Unfreeze after unlock time (two-phase unfreeze)
        let (energy_removed, pending_amount) =
            resource.unfreeze_tos(100000000, unlock_topoheight).unwrap();
        assert_eq!(energy_removed, 14); // 1 TOS * 14 = 14 transfers
        assert_eq!(pending_amount, 100000000); // 1 TOS pending
        assert_eq!(resource.frozen_tos, 100000000); // 1 TOS still frozen
        assert_eq!(resource.total_energy, 14); // Energy reduced
        assert_eq!(resource.pending_unfreezes.len(), 1); // Pending unfreeze created
    }

    #[test]
    fn test_two_phase_unfreeze() {
        let mut resource = EnergyResource::new();
        let freeze_topoheight = 1000;
        let duration = FreezeDuration::new(7).unwrap();
        let unlock_topoheight = freeze_topoheight + duration.duration_in_blocks();
        let cooldown = crate::config::UNFREEZE_COOLDOWN_BLOCKS;

        // Freeze 1 TOS for 7 days
        resource.freeze_tos_for_energy(100000000, duration, freeze_topoheight);
        assert_eq!(resource.total_energy, 14);

        // Phase 1: Unfreeze (creates pending)
        let (energy_removed, pending) =
            resource.unfreeze_tos(100000000, unlock_topoheight).unwrap();
        assert_eq!(energy_removed, 14);
        assert_eq!(pending, 100000000);
        assert_eq!(resource.total_energy, 0);
        assert_eq!(resource.pending_unfreezes.len(), 1);

        // Try to withdraw before cooldown (should return 0)
        let withdrawn_early = resource.withdraw_unfrozen(unlock_topoheight + 1);
        assert_eq!(withdrawn_early, 0);
        assert_eq!(resource.pending_unfreezes.len(), 1);

        // Phase 2: Withdraw after cooldown
        let withdraw_time = unlock_topoheight + cooldown;
        let withdrawn = resource.withdraw_unfrozen(withdraw_time);
        assert_eq!(withdrawn, 100000000);
        assert_eq!(resource.pending_unfreezes.len(), 0);
    }

    #[test]
    fn test_pending_unfreeze_limits() {
        let mut resource = EnergyResource::new();
        let freeze_topoheight = 1000;
        let duration = FreezeDuration::new(3).unwrap();
        let unlock_topoheight = freeze_topoheight + duration.duration_in_blocks();

        // Freeze 33 TOS (more than MAX_PENDING_UNFREEZES limit)
        resource.freeze_tos_for_energy(33 * crate::config::COIN_VALUE, duration, freeze_topoheight);

        // Create 32 pending unfreezes (max limit)
        for _ in 0..crate::config::MAX_PENDING_UNFREEZES {
            let result = resource.unfreeze_tos(crate::config::COIN_VALUE, unlock_topoheight);
            assert!(result.is_ok());
        }

        assert_eq!(
            resource.pending_unfreezes.len(),
            crate::config::MAX_PENDING_UNFREEZES
        );

        // 33rd unfreeze should fail
        let result = resource.unfreeze_tos(crate::config::COIN_VALUE, unlock_topoheight);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Maximum pending unfreezes"));
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

    #[test]
    fn test_delegation_unfreeze() {
        use crate::crypto::KeyPair;

        let mut delegator_resource = EnergyResource::new();
        let freeze_topoheight = 1000;
        let duration = FreezeDuration::new(7).unwrap();
        let network = crate::network::Network::Mainnet;

        // Create delegatee keys (compressed)
        let delegatee1 = KeyPair::new().get_public_key().compress();
        let delegatee2 = KeyPair::new().get_public_key().compress();

        // Create delegation entries
        let entry1 = DelegateRecordEntry {
            delegatee: delegatee1.clone(),
            amount: 100000000, // 1 TOS
            energy: 14,        // 1 TOS * 2 * 7 days = 14 energy
        };
        let entry2 = DelegateRecordEntry {
            delegatee: delegatee2.clone(),
            amount: 200000000, // 2 TOS
            energy: 28,        // 2 TOS * 2 * 7 days = 28 energy
        };

        // Create delegated freeze record
        let result = delegator_resource.create_delegated_freeze(
            vec![entry1, entry2],
            duration,
            300000000, // 3 TOS total
            freeze_topoheight,
            &network,
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42); // Total energy = 14 + 28
        assert_eq!(delegator_resource.frozen_tos, 300000000);
        assert_eq!(delegator_resource.delegated_records.len(), 1);

        // Calculate unlock time
        let unlock_topoheight =
            freeze_topoheight + duration.duration_in_blocks_for_network(&network);

        // Try to unfreeze before unlock time (should fail)
        let result = delegator_resource.unfreeze_delegated(100000000, unlock_topoheight - 1);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("Insufficient unlocked delegated TOS"));

        // Unfreeze 1 TOS after unlock time
        let (energy_per_delegatee, pending_amount) = delegator_resource
            .unfreeze_delegated(100000000, unlock_topoheight)
            .unwrap();

        // Verify results
        assert_eq!(pending_amount, 100000000); // 1 TOS pending
        assert_eq!(delegator_resource.frozen_tos, 200000000); // 2 TOS still frozen
        assert_eq!(delegator_resource.pending_unfreezes.len(), 1);

        // Verify energy removed from delegatees
        assert_eq!(energy_per_delegatee.len(), 1); // Only 1 delegatee affected
        let (affected_delegatee, energy_removed) = &energy_per_delegatee[0];
        assert_eq!(affected_delegatee, &delegatee1);
        assert_eq!(*energy_removed, 14); // All energy from delegatee1

        // Unfreeze remaining 2 TOS
        let (energy_per_delegatee2, pending_amount2) = delegator_resource
            .unfreeze_delegated(200000000, unlock_topoheight)
            .unwrap();

        assert_eq!(pending_amount2, 200000000);
        assert_eq!(delegator_resource.frozen_tos, 0);
        assert_eq!(delegator_resource.pending_unfreezes.len(), 2);
        assert_eq!(energy_per_delegatee2.len(), 1); // delegatee2 affected
        assert_eq!(energy_per_delegatee2[0].1, 28); // 28 energy removed

        // Verify delegated records are now empty
        assert!(delegator_resource.delegated_records.is_empty());
    }

    #[test]
    fn test_delegation_partial_unfreeze() {
        use crate::crypto::KeyPair;

        let mut delegator_resource = EnergyResource::new();
        let freeze_topoheight = 1000;
        let duration = FreezeDuration::new(7).unwrap();
        let network = crate::network::Network::Mainnet;

        let delegatee = KeyPair::new().get_public_key().compress();

        // Create delegation entry for 3 TOS
        let entry = DelegateRecordEntry {
            delegatee: delegatee.clone(),
            amount: 300000000, // 3 TOS
            energy: 42,        // 3 TOS * 2 * 7 days = 42 energy
        };

        delegator_resource
            .create_delegated_freeze(
                vec![entry],
                duration,
                300000000,
                freeze_topoheight,
                &network,
            )
            .unwrap();

        let unlock_topoheight =
            freeze_topoheight + duration.duration_in_blocks_for_network(&network);

        // Unfreeze 1 TOS (partial)
        let (energy_per_delegatee, pending_amount) = delegator_resource
            .unfreeze_delegated(100000000, unlock_topoheight)
            .unwrap();

        assert_eq!(pending_amount, 100000000);
        assert_eq!(delegator_resource.frozen_tos, 200000000); // 2 TOS remaining
        assert_eq!(energy_per_delegatee[0].1, 14); // 1 TOS * 14 energy

        // Verify the delegated record is updated, not removed
        assert_eq!(delegator_resource.delegated_records.len(), 1);
        let record = &delegator_resource.delegated_records[0];
        assert_eq!(record.total_amount, 200000000); // 2 TOS remaining
        assert_eq!(record.entries.len(), 1);
        assert_eq!(record.entries[0].amount, 200000000);
        assert_eq!(record.entries[0].energy, 28); // 2 TOS * 14 = 28 energy
    }
}
