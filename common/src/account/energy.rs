use crate::{
    block::TopoHeight,
    crypto::PublicKey,
    serializer::{Reader, ReaderError, Serializer, Writer},
};
use serde::{Deserialize, Serialize};

/// Flexible freeze duration for TOS staking
/// Users can set custom days from 3 to 365 days
///
/// # Edge Cases
/// - Duration below 3 days will be rejected
/// - Duration above 365 days will be rejected
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
    /// Number of days to freeze (3-365 days)
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

    /// Check if duration is valid (3-365 days)
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
    ) -> Result<Self, String> {
        let unlock_topoheight =
            freeze_topoheight + duration.duration_in_blocks_for_network(network);
        let total_amount = entries
            .iter()
            .try_fold(0u64, |acc, entry| acc.checked_add(entry.amount))
            .ok_or_else(|| "Delegated record amount overflow".to_string())?;
        let total_energy = entries
            .iter()
            .try_fold(0u64, |acc, entry| acc.checked_add(entry.energy))
            .ok_or_else(|| "Delegated record energy overflow".to_string())?;

        Ok(Self {
            entries,
            duration,
            freeze_topoheight,
            unlock_topoheight,
            total_amount,
            total_energy,
        })
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

/// Result of freeze operation with recycling
///
/// # Fields
/// - `new_energy`: Energy gained from balance portion only (recycled TOS doesn't generate new energy)
/// - `recycled_tos`: Amount of TOS recycled from expired freeze records
/// - `balance_tos`: Amount of TOS taken from available balance
/// - `recycled_energy`: Energy preserved from recycled expired records
#[derive(Debug, Clone, Default)]
pub struct FreezeWithRecyclingResult {
    /// New energy gained (only from balance portion, not recycled)
    pub new_energy: u64,
    /// TOS recycled from expired records
    pub recycled_tos: u64,
    /// TOS taken from balance
    pub balance_tos: u64,
    /// Energy preserved from recycled records
    pub recycled_energy: u64,
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
    /// Energy pending activation (available from next block)
    pub pending_energy: u64,
    /// Topoheight when pending energy was added
    pub pending_energy_topoheight: TopoHeight,
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

    /// Get available energy at the current topoheight (excludes same-block pending energy)
    pub fn available_energy_at(&self, current_topoheight: TopoHeight) -> u64 {
        let active_total =
            if self.pending_energy > 0 && current_topoheight <= self.pending_energy_topoheight {
                self.total_energy.saturating_sub(self.pending_energy)
            } else {
                self.total_energy
            };
        let sum = active_total.saturating_add(self.delegated_energy);
        sum.saturating_sub(self.used_energy)
    }

    /// Check if has enough energy at the current topoheight
    pub fn has_enough_energy(&self, current_topoheight: TopoHeight, required: u64) -> bool {
        self.available_energy_at(current_topoheight) >= required
    }

    /// Clear pending energy marker if we've moved to a new block
    pub fn clear_pending_energy_if_ready(&mut self, current_topoheight: TopoHeight) {
        if self.pending_energy > 0 && current_topoheight > self.pending_energy_topoheight {
            self.pending_energy = 0;
        }
    }

    /// Consume energy
    pub fn consume_energy(
        &mut self,
        amount: u64,
        current_topoheight: TopoHeight,
    ) -> Result<(), &'static str> {
        if self.available_energy_at(current_topoheight) < amount {
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

    /// Get total recyclable TOS from expired freeze records
    /// Returns the sum of all expired self-freeze record amounts
    pub fn get_recyclable_tos(&self, current_topoheight: TopoHeight) -> u64 {
        self.freeze_records
            .iter()
            .filter(|r| r.can_unlock(current_topoheight))
            .map(|r| r.amount)
            .sum()
    }

    /// Merge with existing same-duration record or create new record
    ///
    /// # Record Merging Rule
    /// When a freeze record with the same duration already exists:
    /// - Merge amounts together
    /// - Merge energy together
    /// - Use the LATER unlock_topoheight (prevents early unfreeze of new funds)
    ///
    /// # Returns
    /// true if merged with existing record, false if new record created
    fn merge_or_create_freeze_record(
        &mut self,
        amount: u64,
        duration: FreezeDuration,
        freeze_topoheight: TopoHeight,
        new_unlock_topoheight: TopoHeight,
        energy: u64,
    ) -> bool {
        // Find existing record with same duration
        if let Some(record) = self
            .freeze_records
            .iter_mut()
            .find(|r| r.duration == duration)
        {
            // Merge: use later unlock_topoheight to prevent early unfreeze
            record.unlock_topoheight =
                std::cmp::max(record.unlock_topoheight, new_unlock_topoheight);
            record.amount = record.amount.saturating_add(amount);
            record.energy_gained = record.energy_gained.saturating_add(energy);
            true
        } else {
            // No mergeable record found, create new
            let new_record = FreezeRecord {
                amount,
                duration,
                freeze_topoheight,
                unlock_topoheight: new_unlock_topoheight,
                energy_gained: energy,
            };
            self.freeze_records.push(new_record);
            false
        }
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
        // Use recycling version with 0 balance to force all from balance (legacy behavior)
        let result = self.freeze_tos_with_recycling(tos_amount, duration, topoheight, network);
        result.new_energy
    }

    /// Freeze TOS with expired freeze recycling (self-freeze only)
    ///
    /// # Expired Freeze Recycling
    /// - Prioritizes recycling TOS from expired freeze records before using balance
    /// - Recycled TOS preserves its existing Energy (no new Energy granted)
    /// - Only TOS from balance generates new Energy
    ///
    /// # Arguments
    /// - `tos_amount`: Total amount of TOS to freeze
    /// - `duration`: Freeze duration (3-365 days)
    /// - `topoheight`: Current blockchain topoheight
    /// - `network`: Network for timing calculations
    ///
    /// # Returns
    /// FreezeWithRecyclingResult with new_energy, recycled_tos, balance_tos, recycled_energy
    pub fn freeze_tos_with_recycling(
        &mut self,
        tos_amount: u64,
        duration: FreezeDuration,
        topoheight: TopoHeight,
        network: &crate::network::Network,
    ) -> FreezeWithRecyclingResult {
        if tos_amount < crate::config::MIN_FREEZE_TOS_AMOUNT {
            return FreezeWithRecyclingResult {
                new_energy: 0,
                recycled_tos: 0,
                balance_tos: 0,
                recycled_energy: 0,
            };
        }

        if !tos_amount.is_multiple_of(crate::config::COIN_VALUE) {
            return FreezeWithRecyclingResult {
                new_energy: 0,
                recycled_tos: 0,
                balance_tos: 0,
                recycled_energy: 0,
            };
        }

        // Step 1: Find expired records and calculate recyclable amount
        let mut expired_indices: Vec<usize> = Vec::new();
        let mut total_recyclable: u64 = 0;

        for (idx, record) in self.freeze_records.iter().enumerate() {
            if record.can_unlock(topoheight) {
                expired_indices.push(idx);
                total_recyclable = total_recyclable.saturating_add(record.amount);
            }
        }

        // Step 2: Calculate recycle and balance amounts
        let recycle_amount = std::cmp::min(tos_amount, total_recyclable);
        let balance_amount = tos_amount.saturating_sub(recycle_amount);

        // Step 3: Process recycling - collect energy from recycled portions
        let mut remaining_recycle = recycle_amount;
        let mut energy_recycled: u64 = 0;
        let mut records_to_remove: Vec<usize> = Vec::new();
        let mut records_to_modify: Vec<(usize, u64, u64)> = Vec::new(); // (idx, new_amount, new_energy)

        for &idx in &expired_indices {
            if remaining_recycle == 0 {
                break;
            }

            let record = &self.freeze_records[idx];
            let unfreeze_from_record = std::cmp::min(remaining_recycle, record.amount);

            if unfreeze_from_record == record.amount {
                // Full record recycled - keep all its energy
                energy_recycled = energy_recycled.saturating_add(record.energy_gained);
                records_to_remove.push(idx);
            } else {
                // Partial recycle - calculate proportional energy
                let energy_for_recycled = ((record.energy_gained as u128)
                    * unfreeze_from_record as u128
                    / record.amount as u128) as u64;
                energy_recycled = energy_recycled.saturating_add(energy_for_recycled);

                // Remaining record
                let new_amount = record.amount.saturating_sub(unfreeze_from_record);
                let new_energy = record.energy_gained.saturating_sub(energy_for_recycled);
                records_to_modify.push((idx, new_amount, new_energy));
            }

            remaining_recycle = remaining_recycle.saturating_sub(unfreeze_from_record);
        }

        // Step 4: Apply record modifications (in reverse order to maintain indices)
        for &idx in records_to_remove.iter().rev() {
            self.freeze_records.remove(idx);
        }

        // Need to adjust indices after removals
        for (idx, new_amount, new_energy) in records_to_modify.iter() {
            // Calculate adjusted index after removals
            let removed_before = records_to_remove.iter().filter(|&&r| r < *idx).count();
            let adjusted_idx = idx.saturating_sub(removed_before);
            if adjusted_idx < self.freeze_records.len() {
                self.freeze_records[adjusted_idx].amount = *new_amount;
                self.freeze_records[adjusted_idx].energy_gained = *new_energy;
            }
        }

        // Step 5: Reduce total_energy by recycled energy (will be added back with new record)
        self.total_energy = self.total_energy.saturating_sub(energy_recycled);

        // Reduce frozen_tos by recycled amount (will be added back)
        self.frozen_tos = self.frozen_tos.saturating_sub(recycle_amount);

        // Step 6: Calculate new energy (only from balance portion)
        let new_energy =
            (balance_amount / crate::config::COIN_VALUE) * duration.reward_multiplier();

        // Step 7: Create new freeze record OR merge with existing same-duration record
        let unlock_topoheight = topoheight + duration.duration_in_blocks_for_network(network);
        let record_energy = energy_recycled.saturating_add(new_energy);

        // Try to find and merge with existing record of same duration
        let _merged = self.merge_or_create_freeze_record(
            tos_amount,
            duration,
            topoheight,
            unlock_topoheight,
            record_energy,
        );

        // Step 8: Update totals
        // frozen_tos: we removed recycle_amount, now add full tos_amount
        // But balance_amount should come from user's balance
        self.frozen_tos = self.frozen_tos.saturating_add(tos_amount);
        self.total_energy = self.total_energy.saturating_add(record_energy);
        self.clear_pending_energy_if_ready(topoheight);
        if new_energy > 0 {
            self.pending_energy = self.pending_energy.saturating_add(new_energy);
            self.pending_energy_topoheight = topoheight;
        }
        self.last_update = topoheight;

        FreezeWithRecyclingResult {
            new_energy,
            recycled_tos: recycle_amount,
            balance_tos: balance_amount,
            recycled_energy: energy_recycled,
        }
    }

    /// Unfreeze TOS from self-freeze records (Phase 1 of two-phase unfreeze)
    /// Creates a PendingUnfreeze with 14-day cooldown
    /// Energy is removed immediately, TOS returned after cooldown via WithdrawUnfrozen
    ///
    /// # Arguments
    /// - `tos_amount`: Amount of TOS to unfreeze (must be whole TOS multiples)
    /// - `current_topoheight`: Current blockchain topoheight
    /// - `record_index`: Optional record index for selective unfreeze
    ///   - None: Use FIFO order (oldest unlocked records first)
    ///   - Some(idx): Unfreeze from specific record at that index
    ///
    /// # Returns
    /// - Ok((energy_removed, pending_amount)): Energy removed and TOS amount pending
    /// - Err: If insufficient unlocked TOS or record limit exceeded
    pub fn unfreeze_tos(
        &mut self,
        tos_amount: u64,
        current_topoheight: TopoHeight,
        record_index: Option<u32>,
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

        let mut remaining_to_unfreeze = whole_tos_amount;
        let mut total_energy_removed: u64 = 0;
        let mut records_to_remove = Vec::new();
        let mut records_to_modify: Vec<(usize, u64, u64)> = Vec::new();

        match record_index {
            // Selective unfreeze: specific record by index
            Some(idx) => {
                let idx = idx as usize;
                if idx >= self.freeze_records.len() {
                    return Err("Record index out of bounds".to_string());
                }

                let record = &self.freeze_records[idx];

                // Check if record is unlocked
                if !record.can_unlock(current_topoheight) {
                    return Err("Record is still locked".to_string());
                }

                // Check if record has sufficient amount
                if record.amount < whole_tos_amount {
                    return Err("Insufficient amount in specified record".to_string());
                }

                let energy_to_remove = if whole_tos_amount >= record.amount {
                    record.energy_gained
                } else {
                    ((record.energy_gained as u128 * whole_tos_amount as u128)
                        / record.amount as u128) as u64
                };

                total_energy_removed = energy_to_remove;
                remaining_to_unfreeze = 0;

                // Mark record for removal or modification
                if whole_tos_amount == record.amount {
                    records_to_remove.push(idx);
                } else {
                    records_to_modify.push((idx, whole_tos_amount, energy_to_remove));
                }
            }

            // FIFO mode: oldest unlocked records first
            None => {
                for (index, record) in self.freeze_records.iter().enumerate() {
                    if !record.can_unlock(current_topoheight) {
                        continue; // Skip records that haven't reached unlock time
                    }

                    if remaining_to_unfreeze == 0 {
                        break;
                    }

                    let unfreeze_amount = std::cmp::min(remaining_to_unfreeze, record.amount);

                    let energy_to_remove = if unfreeze_amount >= record.amount {
                        record.energy_gained
                    } else {
                        ((record.energy_gained as u128 * unfreeze_amount as u128)
                            / record.amount as u128) as u64
                    };

                    total_energy_removed = total_energy_removed.saturating_add(energy_to_remove);
                    remaining_to_unfreeze = remaining_to_unfreeze.saturating_sub(unfreeze_amount);

                    // Mark record for removal if fully unfrozen
                    if unfreeze_amount == record.amount {
                        records_to_remove.push(index);
                    } else {
                        // Partially unfreeze the record
                        records_to_modify.push((index, unfreeze_amount, energy_to_remove));
                    }
                }
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
        for (index, unfreeze_amount, energy_removed) in records_to_modify.iter().rev() {
            let record = &mut self.freeze_records[*index];
            record.amount = record.amount.saturating_sub(*unfreeze_amount);
            record.energy_gained = record.energy_gained.saturating_sub(*energy_removed);
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
    pub fn withdraw_unfrozen(
        &mut self,
        current_topoheight: TopoHeight,
    ) -> Result<u64, &'static str> {
        let mut total_withdrawn = 0u64;
        for pending in &self.pending_unfreezes {
            if pending.is_expired(current_topoheight) {
                total_withdrawn = total_withdrawn
                    .checked_add(pending.amount)
                    .ok_or("Overflow in withdraw calculation")?;
            }
        }

        self.pending_unfreezes
            .retain(|pending| !pending.is_expired(current_topoheight));
        self.last_update = current_topoheight;

        Ok(total_withdrawn)
    }

    /// Get total pending unfreeze amount (not yet withdrawn)
    pub fn total_pending_unfreeze(&self) -> Result<u64, &'static str> {
        self.pending_unfreezes
            .iter()
            .try_fold(0u64, |acc, pending| acc.checked_add(pending.amount))
            .ok_or("Overflow in pending unfreeze total")
    }

    /// Get withdrawable unfreeze amount (expired pending unfreezes)
    pub fn withdrawable_unfreeze(
        &self,
        current_topoheight: TopoHeight,
    ) -> Result<u64, &'static str> {
        self.pending_unfreezes
            .iter()
            .filter(|p| p.is_expired(current_topoheight))
            .try_fold(0u64, |acc, pending| acc.checked_add(pending.amount))
            .ok_or("Overflow in withdrawable unfreeze total")
    }

    /// Add delegated energy (called on delegatee's account when receiving delegation)
    pub fn add_delegated_energy(
        &mut self,
        energy_amount: u64,
        topoheight: TopoHeight,
    ) -> Result<(), &'static str> {
        self.delegated_energy = self
            .delegated_energy
            .checked_add(energy_amount)
            .ok_or("Delegated energy overflow")?;
        self.last_update = topoheight;
        Ok(())
    }

    /// Remove delegated energy (called on delegatee's account when delegator unfreezes)
    pub fn remove_delegated_energy(
        &mut self,
        energy_amount: u64,
        topoheight: TopoHeight,
    ) -> Result<(), &'static str> {
        if energy_amount > self.delegated_energy {
            return Err("Cannot remove more delegated energy than available");
        }
        self.delegated_energy = self.delegated_energy.saturating_sub(energy_amount);
        self.last_update = topoheight;
        Ok(())
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

        let record = DelegatedFreezeRecord::new(entries, duration, topoheight, network)?;
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
            .try_fold(0u64, |acc, record| acc.checked_add(record.total_amount))
            .ok_or_else(|| "Delegated record total overflow".to_string())?;

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
            record.total_amount = record
                .entries
                .iter()
                .try_fold(0u64, |acc, entry| acc.checked_add(entry.amount))
                .ok_or_else(|| "Delegated record amount overflow".to_string())?;
            record.total_energy = record
                .entries
                .iter()
                .try_fold(0u64, |acc, entry| acc.checked_add(entry.energy))
                .ok_or_else(|| "Delegated record energy overflow".to_string())?;
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

    /// Unfreeze a specific delegatee's entry from a batch delegation record
    ///
    /// This function supports selective unfreeze of a single delegatee from a batch
    /// delegation that may contain multiple delegatees. It is required when the
    /// delegation record has more than one entry.
    ///
    /// # Parameters
    /// - `tos_amount`: Amount to unfreeze (must be whole TOS, minimum 1 TOS)
    /// - `current_topoheight`: Current block height for lock checking
    /// - `record_index`: Which delegation record (required if multiple records exist)
    /// - `delegatee_address`: The delegatee whose entry to unfreeze
    ///
    /// # Returns
    /// - `Ok((delegatee, energy_removed, pending_amount))` on success
    /// - `Err(String)` on failure
    ///
    /// # Errors
    /// - "Cannot unfreeze 0 TOS" if amount rounds to 0
    /// - "Amount below minimum unfreeze amount" if < 1 TOS
    /// - "Maximum pending unfreezes reached" if at 32 pending limit
    /// - "No delegated records found" if no delegation records exist
    /// - "Multiple delegation records exist, record_index required" if > 1 records
    /// - "Record index out of bounds" if record_index >= records.len()
    /// - "Record is still locked" if unlock_topoheight not reached
    /// - "Delegatee not found in record" if delegatee_address not in entries
    /// - "Amount exceeds entry amount" if trying to unfreeze more than entry has
    pub fn unfreeze_delegated_entry(
        &mut self,
        tos_amount: u64,
        current_topoheight: TopoHeight,
        record_index: Option<u32>,
        delegatee_address: &PublicKey,
    ) -> Result<(PublicKey, u64, u64), String> {
        // Validate amount is whole TOS
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

        // Determine which record to use
        if self.delegated_records.is_empty() {
            return Err("No delegated records found".to_string());
        }

        let record_idx = match record_index {
            Some(idx) => {
                if idx as usize >= self.delegated_records.len() {
                    return Err("Record index out of bounds".to_string());
                }
                idx as usize
            }
            None => {
                if self.delegated_records.len() > 1 {
                    return Err(
                        "Multiple delegation records exist, record_index required".to_string()
                    );
                }
                0
            }
        };

        // Check if record is unlocked
        if !self.delegated_records[record_idx].can_unlock(current_topoheight) {
            return Err("Record is still locked".to_string());
        }

        // Find the entry for the specified delegatee
        let entry_idx = self.delegated_records[record_idx]
            .find_entry_index(delegatee_address)
            .ok_or_else(|| "Delegatee not found in record".to_string())?;

        let record = &self.delegated_records[record_idx];
        let entry = &record.entries[entry_idx];

        // Validate amount doesn't exceed entry amount
        if whole_tos_amount > entry.amount {
            return Err("Amount exceeds entry amount".to_string());
        }

        // Calculate energy to remove (proportional using high-precision math)
        let energy_to_remove = if whole_tos_amount == entry.amount {
            // Full unfreeze of entry - remove all energy
            entry.energy
        } else {
            // Partial unfreeze - proportional energy removal with FLOOR rounding
            const PRECISION: u128 = 1_000_000_000_000;
            let energy_ratio = whole_tos_amount as u128 * PRECISION / entry.amount as u128;
            ((entry.energy as u128 * energy_ratio) / PRECISION) as u64
        };

        let delegatee = entry.delegatee.clone();
        let is_full_entry_unfreeze = whole_tos_amount >= entry.amount;

        // Now mutably modify the record
        let record = &mut self.delegated_records[record_idx];

        if is_full_entry_unfreeze {
            // Remove entry entirely
            record.entries.remove(entry_idx);
        } else {
            // Partial unfreeze - update entry
            let entry = &mut record.entries[entry_idx];
            entry.amount = entry.amount.saturating_sub(whole_tos_amount);
            entry.energy = entry.energy.saturating_sub(energy_to_remove);
        }

        // Update record totals
        record.total_amount = record.total_amount.saturating_sub(whole_tos_amount);
        record.total_energy = record.total_energy.saturating_sub(energy_to_remove);

        // If no entries remain, remove the entire record
        let record_empty = record.entries.is_empty();
        if record_empty {
            self.delegated_records.remove(record_idx);
        }

        // Update frozen TOS
        self.frozen_tos = self.frozen_tos.saturating_sub(whole_tos_amount);
        self.last_update = current_topoheight;

        // Create pending unfreeze
        let pending = PendingUnfreeze::new(whole_tos_amount, current_topoheight);
        self.pending_unfreezes.push(pending);

        Ok((delegatee, energy_to_remove, whole_tos_amount))
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

        // Write pending energy markers (optional in older data)
        writer.write_u64(&self.pending_energy);
        writer.write_u64(&self.pending_energy_topoheight);
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

        let (pending_energy, pending_energy_topoheight) = if reader.size() >= 16 {
            (reader.read_u64()?, reader.read_u64()?)
        } else {
            (0, 0)
        };

        Ok(Self {
            total_energy,
            delegated_energy,
            used_energy,
            pending_energy,
            pending_energy_topoheight,
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
        base_size
            + freeze_records_size
            + delegated_records_size
            + pending_unfreezes_size
            + self.pending_energy.size()
            + self.pending_energy_topoheight.size()
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
        let result = resource.unfreeze_tos(100000000, unlock_topoheight - 1, None);
        assert!(result.is_err());

        // Unfreeze after unlock time (two-phase unfreeze)
        let (energy_removed, pending_amount) = resource
            .unfreeze_tos(100000000, unlock_topoheight, None)
            .unwrap();
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
        let (energy_removed, pending) = resource
            .unfreeze_tos(100000000, unlock_topoheight, None)
            .unwrap();
        assert_eq!(energy_removed, 14);
        assert_eq!(pending, 100000000);
        assert_eq!(resource.total_energy, 0);
        assert_eq!(resource.pending_unfreezes.len(), 1);

        // Try to withdraw before cooldown (should return 0)
        let withdrawn_early = resource.withdraw_unfrozen(unlock_topoheight + 1).unwrap();
        assert_eq!(withdrawn_early, 0);
        assert_eq!(resource.pending_unfreezes.len(), 1);

        // Phase 2: Withdraw after cooldown
        let withdraw_time = unlock_topoheight + cooldown;
        let withdrawn = resource.withdraw_unfrozen(withdraw_time).unwrap();
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
            let result = resource.unfreeze_tos(crate::config::COIN_VALUE, unlock_topoheight, None);
            assert!(result.is_ok());
        }

        assert_eq!(
            resource.pending_unfreezes.len(),
            crate::config::MAX_PENDING_UNFREEZES
        );

        // 33rd unfreeze should fail
        let result = resource.unfreeze_tos(crate::config::COIN_VALUE, unlock_topoheight, None);
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
        assert_eq!(resource.pending_energy, deserialized.pending_energy);
        assert_eq!(
            resource.pending_energy_topoheight,
            deserialized.pending_energy_topoheight
        );
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

    // ==================== Expired Freeze Recycling Tests ====================

    #[test]
    fn test_get_recyclable_tos() {
        let mut resource = EnergyResource::new();
        let network = crate::network::Network::Mainnet;
        let freeze_topoheight = 1000;
        let duration = FreezeDuration::new(3).unwrap(); // 3-day lock

        // Initially no recyclable TOS
        assert_eq!(resource.get_recyclable_tos(freeze_topoheight), 0);

        // Freeze 5 TOS
        resource.freeze_tos_for_energy_with_network(
            500000000,
            duration,
            freeze_topoheight,
            &network,
        );

        // Before expiration: no recyclable TOS
        let before_unlock =
            freeze_topoheight + duration.duration_in_blocks_for_network(&network) - 1;
        assert_eq!(resource.get_recyclable_tos(before_unlock), 0);

        // After expiration: 5 TOS recyclable
        let after_unlock = freeze_topoheight + duration.duration_in_blocks_for_network(&network);
        assert_eq!(resource.get_recyclable_tos(after_unlock), 500000000);
    }

    #[test]
    fn test_freeze_with_full_recycling() {
        let mut resource = EnergyResource::new();
        let network = crate::network::Network::Mainnet;
        let freeze_topoheight = 1000;
        let duration = FreezeDuration::new(7).unwrap();

        // Freeze 3 TOS
        resource.freeze_tos_for_energy_with_network(
            300000000,
            duration,
            freeze_topoheight,
            &network,
        );
        let initial_energy = resource.total_energy;
        assert_eq!(initial_energy, 42); // 3 TOS * 2 * 7 days = 42

        // After lock expires
        let unlock_topoheight =
            freeze_topoheight + duration.duration_in_blocks_for_network(&network);

        // Freeze 3 TOS again (should fully recycle)
        let new_duration = FreezeDuration::new(14).unwrap();
        let result = resource.freeze_tos_with_recycling(
            300000000,
            new_duration,
            unlock_topoheight,
            &network,
        );

        // Should recycle all 3 TOS, no balance needed
        assert_eq!(result.recycled_tos, 300000000);
        assert_eq!(result.balance_tos, 0);
        assert_eq!(result.new_energy, 0); // No new energy from recycled portion
        assert_eq!(result.recycled_energy, 42); // Preserved from old record

        // Total energy preserved (old energy kept)
        assert_eq!(resource.total_energy, 42);
        assert_eq!(resource.frozen_tos, 300000000);
        assert_eq!(resource.freeze_records.len(), 1); // Old record removed, new one added
    }

    #[test]
    fn test_freeze_with_mixed_recycling() {
        let mut resource = EnergyResource::new();
        let network = crate::network::Network::Mainnet;
        let freeze_topoheight = 1000;
        let duration = FreezeDuration::new(7).unwrap();

        // Freeze 3 TOS
        resource.freeze_tos_for_energy_with_network(
            300000000,
            duration,
            freeze_topoheight,
            &network,
        );
        let initial_energy = resource.total_energy;
        assert_eq!(initial_energy, 42);

        // After lock expires
        let unlock_topoheight =
            freeze_topoheight + duration.duration_in_blocks_for_network(&network);

        // Freeze 5 TOS (3 TOS recycled + 2 TOS from balance)
        let new_duration = FreezeDuration::new(14).unwrap();
        let result = resource.freeze_tos_with_recycling(
            500000000,
            new_duration,
            unlock_topoheight,
            &network,
        );

        // Should recycle 3 TOS, use 2 TOS from balance
        assert_eq!(result.recycled_tos, 300000000);
        assert_eq!(result.balance_tos, 200000000);
        assert_eq!(result.recycled_energy, 42); // Preserved from old record
                                                // New energy: 2 TOS * 2 * 14 days = 56
        assert_eq!(result.new_energy, 56);

        // Total energy = recycled (42) + new (56) = 98
        assert_eq!(resource.total_energy, 98);
        assert_eq!(resource.frozen_tos, 500000000);
    }

    #[test]
    fn test_freeze_with_partial_recycling() {
        let mut resource = EnergyResource::new();
        let network = crate::network::Network::Mainnet;
        let freeze_topoheight = 1000;
        let duration = FreezeDuration::new(7).unwrap();

        // Freeze 5 TOS
        resource.freeze_tos_for_energy_with_network(
            500000000,
            duration,
            freeze_topoheight,
            &network,
        );
        let initial_energy = resource.total_energy;
        assert_eq!(initial_energy, 70); // 5 TOS * 2 * 7 days = 70

        // After lock expires
        let unlock_topoheight =
            freeze_topoheight + duration.duration_in_blocks_for_network(&network);

        // Freeze 2 TOS (partial recycle from 5 TOS expired record)
        let new_duration = FreezeDuration::new(14).unwrap();
        let result = resource.freeze_tos_with_recycling(
            200000000,
            new_duration,
            unlock_topoheight,
            &network,
        );

        // Should recycle 2 TOS from the 5 TOS expired record
        assert_eq!(result.recycled_tos, 200000000);
        assert_eq!(result.balance_tos, 0);
        // Energy recycled: 2 TOS * 2 * 7 days = 28 (FLOOR calculation)
        assert_eq!(result.recycled_energy, 28);
        assert_eq!(result.new_energy, 0); // All from recycled

        // Total energy = remaining (70-28=42) + new record (28) = 70
        // Wait, let me reconsider: the old record with 5 TOS and 70 energy
        // We recycle 2 TOS worth 28 energy, leaving 3 TOS with 42 energy in old record
        // New record: 2 TOS with 28 energy
        // Total = 42 + 28 = 70
        assert_eq!(resource.total_energy, 70);
        assert_eq!(resource.frozen_tos, 500000000); // Still 5 TOS total (3 old + 2 new)
        assert_eq!(resource.freeze_records.len(), 2); // One remaining + one new
    }

    #[test]
    fn test_freeze_recycling_energy_preservation() {
        let mut resource = EnergyResource::new();
        let network = crate::network::Network::Mainnet;
        let freeze_topoheight = 1000;

        // Freeze 3 TOS for 30 days (high energy multiplier: 60)
        let long_duration = FreezeDuration::new(30).unwrap();
        resource.freeze_tos_for_energy_with_network(
            300000000,
            long_duration,
            freeze_topoheight,
            &network,
        );
        let long_energy = resource.total_energy;
        assert_eq!(long_energy, 180); // 3 TOS * 2 * 30 days = 180

        // After lock expires
        let unlock_topoheight =
            freeze_topoheight + long_duration.duration_in_blocks_for_network(&network);

        // Re-freeze for shorter period (3 days, multiplier: 6)
        // Energy should be PRESERVED from old record, not recalculated
        let short_duration = FreezeDuration::new(3).unwrap();
        let result = resource.freeze_tos_with_recycling(
            300000000,
            short_duration,
            unlock_topoheight,
            &network,
        );

        // All recycled, energy preserved
        assert_eq!(result.recycled_tos, 300000000);
        assert_eq!(result.balance_tos, 0);
        assert_eq!(result.recycled_energy, 180); // Preserved 30-day energy
        assert_eq!(result.new_energy, 0);

        // Total energy = preserved energy (180), not recalculated (18)
        assert_eq!(resource.total_energy, 180);
    }

    #[test]
    fn test_freeze_no_recycling_when_not_expired() {
        let mut resource = EnergyResource::new();
        let network = crate::network::Network::Mainnet;
        let freeze_topoheight = 1000;
        let duration = FreezeDuration::new(7).unwrap();

        // Freeze 3 TOS
        resource.freeze_tos_for_energy_with_network(
            300000000,
            duration,
            freeze_topoheight,
            &network,
        );

        // Before expiration, try to freeze more
        let before_unlock = freeze_topoheight + 10; // Way before unlock
        let result =
            resource.freeze_tos_with_recycling(200000000, duration, before_unlock, &network);

        // No recycling, all from balance
        assert_eq!(result.recycled_tos, 0);
        assert_eq!(result.balance_tos, 200000000);
        assert_eq!(result.recycled_energy, 0);
        // New energy: 2 TOS * 2 * 7 days = 28
        assert_eq!(result.new_energy, 28);

        // Now have 1 record (same-duration records are merged)
        assert_eq!(resource.freeze_records.len(), 1);
        // Total: 3 + 2 = 5 TOS frozen
        assert_eq!(resource.frozen_tos, 500000000);
        // Total energy: 42 + 28 = 70
        assert_eq!(resource.total_energy, 70);

        // Verify merge used the later unlock_topoheight
        let record = &resource.freeze_records[0];
        assert_eq!(record.amount, 500000000); // 3 + 2 TOS merged
        assert_eq!(record.energy_gained, 70); // 42 + 28 merged
                                              // unlock_topoheight should be the later one (from second freeze)
        let expected_unlock = before_unlock + duration.duration_in_blocks_for_network(&network);
        assert_eq!(record.unlock_topoheight, expected_unlock);
    }

    #[test]
    fn test_freeze_record_merging_same_duration() {
        let mut resource = EnergyResource::new();
        let network = crate::network::Network::Mainnet;
        let duration = FreezeDuration::new(14).unwrap();

        // First freeze: 5 TOS at topoheight 1000
        let first_topoheight = 1000;
        resource.freeze_tos_for_energy_with_network(
            500000000,
            duration,
            first_topoheight,
            &network,
        );
        assert_eq!(resource.freeze_records.len(), 1);
        assert_eq!(resource.total_energy, 140); // 5 * 2 * 14 = 140

        let first_unlock = first_topoheight + duration.duration_in_blocks_for_network(&network);

        // Second freeze: 3 TOS at topoheight 2000 with SAME duration
        let second_topoheight = 2000;
        let result =
            resource.freeze_tos_with_recycling(300000000, duration, second_topoheight, &network);

        // No recycling (first record not expired yet)
        assert_eq!(result.recycled_tos, 0);
        assert_eq!(result.balance_tos, 300000000);
        assert_eq!(result.new_energy, 84); // 3 * 2 * 14 = 84

        // Records merged: still only 1 record
        assert_eq!(resource.freeze_records.len(), 1);
        assert_eq!(resource.frozen_tos, 800000000); // 5 + 3 = 8 TOS

        // Energy merged
        assert_eq!(resource.total_energy, 224); // 140 + 84 = 224
        assert_eq!(resource.freeze_records[0].energy_gained, 224);

        // Amount merged
        assert_eq!(resource.freeze_records[0].amount, 800000000);

        // Unlock time uses LATER value (from second freeze)
        let second_unlock = second_topoheight + duration.duration_in_blocks_for_network(&network);
        assert!(second_unlock > first_unlock);
        assert_eq!(resource.freeze_records[0].unlock_topoheight, second_unlock);
    }

    #[test]
    fn test_freeze_no_merging_different_duration() {
        let mut resource = EnergyResource::new();
        let network = crate::network::Network::Mainnet;

        // First freeze: 5 TOS with 7-day duration
        let duration_7 = FreezeDuration::new(7).unwrap();
        resource.freeze_tos_for_energy_with_network(500000000, duration_7, 1000, &network);
        assert_eq!(resource.freeze_records.len(), 1);
        assert_eq!(resource.total_energy, 70); // 5 * 2 * 7 = 70

        // Second freeze: 3 TOS with 14-day duration (different)
        let duration_14 = FreezeDuration::new(14).unwrap();
        let result = resource.freeze_tos_with_recycling(300000000, duration_14, 2000, &network);

        // No recycling (first record not expired)
        assert_eq!(result.recycled_tos, 0);
        assert_eq!(result.balance_tos, 300000000);
        assert_eq!(result.new_energy, 84); // 3 * 2 * 14 = 84

        // NOT merged: 2 separate records (different durations)
        assert_eq!(resource.freeze_records.len(), 2);
        assert_eq!(resource.frozen_tos, 800000000);
        assert_eq!(resource.total_energy, 154); // 70 + 84 = 154
    }

    #[test]
    fn test_freeze_merging_with_recycling() {
        let mut resource = EnergyResource::new();
        let network = crate::network::Network::Mainnet;
        let duration = FreezeDuration::new(7).unwrap();

        // First freeze: 5 TOS at topoheight 1000
        resource.freeze_tos_for_energy_with_network(500000000, duration, 1000, &network);
        let initial_unlock = 1000 + duration.duration_in_blocks_for_network(&network);

        // Wait for expiration
        let after_unlock = initial_unlock + 100;

        // Second freeze with same duration: 3 TOS (should recycle expired + merge)
        let result =
            resource.freeze_tos_with_recycling(300000000, duration, after_unlock, &network);

        // Full recycling of expired record
        assert_eq!(result.recycled_tos, 300000000); // min(500000000, 300000000)
        assert_eq!(result.balance_tos, 0);
        assert_eq!(result.recycled_energy, 42); // (300000000/500000000) * 70 = 42

        // Old record partially consumed, new record created and merged
        // Since old record had 5 TOS and we only needed 3 TOS, 2 TOS remains
        // New 3 TOS goes to new record with same duration
        // But wait - the old record is only partially consumed, not fully removed
        // So we have: old record (2 TOS remaining) + new record (3 TOS)
        // These should NOT merge because old record is partially consumed, not replaced

        // Actually, let me re-check the recycling logic...
        // The recycling takes from expired records and removes them if fully used
        // Then creates a new record. If same duration, new record merges with existing.
        // But the "existing" is the old expired record that was partially consumed.

        // With the current implementation:
        // - Old record (5 TOS) is partially consumed (3 TOS recycled, 2 TOS remains)
        // - New record (3 TOS) is created
        // - New record checks for same-duration merge, but the old record (2 TOS) has same duration
        // - They should merge: 2 + 3 = 5 TOS

        // Total frozen should be 5 TOS (2 remaining + 3 new)
        assert_eq!(resource.frozen_tos, 500000000);
        // One merged record
        assert_eq!(resource.freeze_records.len(), 1);

        // Energy: recycled (42) + new (0 since all from recycled) = 42
        // Plus remaining energy from old record: 70 - 42 = 28
        // Total: 42 + 28 = 70
        assert_eq!(resource.total_energy, 70);
    }

    #[test]
    fn test_unfreeze_with_record_index() {
        let mut resource = EnergyResource::new();
        let freeze_topoheight = 1000;
        let duration_7 = FreezeDuration::new(7).unwrap();
        let duration_14 = FreezeDuration::new(14).unwrap();

        // Create two freeze records with different durations
        resource.freeze_tos_for_energy(200000000, duration_7, freeze_topoheight); // 2 TOS, 28 energy
        resource.freeze_tos_for_energy(300000000, duration_14, freeze_topoheight); // 3 TOS, 84 energy

        assert_eq!(resource.freeze_records.len(), 2);
        assert_eq!(resource.total_energy, 112); // 28 + 84

        // After 14-day unlock (both records are unlocked)
        let unlock_topoheight = freeze_topoheight + duration_14.duration_in_blocks();

        // Unfreeze from record at index 1 (the 3 TOS record with 14-day duration)
        let (energy_removed, pending) = resource
            .unfreeze_tos(100000000, unlock_topoheight, Some(1))
            .unwrap();

        // Should remove energy from the 14-day record: 1 TOS * 28 = 28
        assert_eq!(energy_removed, 28);
        assert_eq!(pending, 100000000);
        assert_eq!(resource.freeze_records.len(), 2); // Both records still exist
        assert_eq!(resource.frozen_tos, 400000000); // 5 - 1 = 4 TOS

        // Check that the correct record was modified
        assert_eq!(resource.freeze_records[0].amount, 200000000); // Unchanged
        assert_eq!(resource.freeze_records[1].amount, 200000000); // 3 - 1 = 2 TOS
    }

    #[test]
    fn test_unfreeze_record_index_out_of_bounds() {
        let mut resource = EnergyResource::new();
        let freeze_topoheight = 1000;
        let duration = FreezeDuration::new(7).unwrap();
        let unlock_topoheight = freeze_topoheight + duration.duration_in_blocks();

        resource.freeze_tos_for_energy(100000000, duration, freeze_topoheight);
        assert_eq!(resource.freeze_records.len(), 1);

        // Try to unfreeze from non-existent index
        let result = resource.unfreeze_tos(100000000, unlock_topoheight, Some(5));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("out of bounds"));
    }

    #[test]
    fn test_unfreeze_record_still_locked() {
        let mut resource = EnergyResource::new();
        let freeze_topoheight = 1000;
        let duration = FreezeDuration::new(7).unwrap();
        let unlock_topoheight = freeze_topoheight + duration.duration_in_blocks();

        resource.freeze_tos_for_energy(100000000, duration, freeze_topoheight);

        // Try to unfreeze before unlock using record_index
        let before_unlock = unlock_topoheight - 1;
        let result = resource.unfreeze_tos(100000000, before_unlock, Some(0));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("still locked"));
    }

    #[test]
    fn test_unfreeze_fifo_mode() {
        let mut resource = EnergyResource::new();
        let freeze_topoheight = 1000;
        let duration_3 = FreezeDuration::new(3).unwrap();
        let duration_7 = FreezeDuration::new(7).unwrap();

        // Create two records: 3-day record first (unlocks sooner), 7-day record second
        resource.freeze_tos_for_energy(100000000, duration_3, freeze_topoheight); // 1 TOS, 6 energy
        resource.freeze_tos_for_energy(200000000, duration_7, freeze_topoheight); // 2 TOS, 28 energy

        assert_eq!(resource.freeze_records.len(), 2);
        assert_eq!(resource.total_energy, 34); // 6 + 28

        // Unlock time after 7 days (both unlocked)
        let unlock_both = freeze_topoheight + duration_7.duration_in_blocks();

        // FIFO mode (record_index = None): should unfreeze from first record (3-day, 1 TOS)
        let (energy_removed, pending) =
            resource.unfreeze_tos(100000000, unlock_both, None).unwrap();

        // FIFO removes from first unlocked record (3-day record with 6 energy)
        assert_eq!(energy_removed, 6);
        assert_eq!(pending, 100000000);
        assert_eq!(resource.freeze_records.len(), 1); // First record fully removed
        assert_eq!(resource.freeze_records[0].duration, duration_7); // Only 7-day record remains
    }

    #[test]
    fn test_unfreeze_selective_vs_fifo() {
        let mut resource = EnergyResource::new();
        let freeze_topoheight = 1000;
        let duration_7 = FreezeDuration::new(7).unwrap();
        let duration_14 = FreezeDuration::new(14).unwrap();

        // Create two records
        resource.freeze_tos_for_energy(100000000, duration_7, freeze_topoheight); // Index 0: 1 TOS, 14 energy
        resource.freeze_tos_for_energy(100000000, duration_14, freeze_topoheight); // Index 1: 1 TOS, 28 energy

        assert_eq!(resource.freeze_records.len(), 2);
        assert_eq!(resource.total_energy, 42);

        // After both unlock
        let unlock_both = freeze_topoheight + duration_14.duration_in_blocks();

        // Selective: unfreeze from index 1 (14-day record)
        let (energy_removed, _) = resource
            .unfreeze_tos(100000000, unlock_both, Some(1))
            .unwrap();
        assert_eq!(energy_removed, 28); // 14-day record energy

        // Verify the 7-day record (index 0) is still intact
        assert_eq!(resource.freeze_records.len(), 1);
        assert_eq!(resource.freeze_records[0].duration, duration_7);
        assert_eq!(resource.freeze_records[0].energy_gained, 14);
    }

    // ==================== WithdrawUnfrozen Tests ====================

    #[test]
    fn test_withdraw_no_pending() {
        let mut resource = EnergyResource::new();

        // No pending unfreezes - withdraw should return 0
        let withdrawn = resource.withdraw_unfrozen(1000).unwrap();
        assert_eq!(withdrawn, 0);
        assert_eq!(resource.pending_unfreezes.len(), 0);
    }

    #[test]
    fn test_withdraw_none_expired() {
        let mut resource = EnergyResource::new();
        let freeze_topoheight = 1000;
        let duration = FreezeDuration::new(7).unwrap();
        let unlock_topoheight = freeze_topoheight + duration.duration_in_blocks();
        let cooldown = crate::config::UNFREEZE_COOLDOWN_BLOCKS;

        // Freeze and unfreeze to create pending
        resource.freeze_tos_for_energy(200000000, duration, freeze_topoheight);
        resource
            .unfreeze_tos(100000000, unlock_topoheight, None)
            .unwrap();
        resource
            .unfreeze_tos(100000000, unlock_topoheight, None)
            .unwrap();

        assert_eq!(resource.pending_unfreezes.len(), 2);

        // Try to withdraw before any have expired
        let before_expire = unlock_topoheight + cooldown - 1;
        let withdrawn = resource.withdraw_unfrozen(before_expire).unwrap();

        assert_eq!(withdrawn, 0);
        assert_eq!(resource.pending_unfreezes.len(), 2); // All still pending
    }

    #[test]
    fn test_withdraw_single_expired() {
        let mut resource = EnergyResource::new();
        let freeze_topoheight = 1000;
        let duration = FreezeDuration::new(7).unwrap();
        let unlock_topoheight = freeze_topoheight + duration.duration_in_blocks();
        let cooldown = crate::config::UNFREEZE_COOLDOWN_BLOCKS;

        // Freeze and unfreeze
        resource.freeze_tos_for_energy(100000000, duration, freeze_topoheight);
        resource
            .unfreeze_tos(100000000, unlock_topoheight, None)
            .unwrap();

        assert_eq!(resource.pending_unfreezes.len(), 1);

        // Withdraw after cooldown
        let after_expire = unlock_topoheight + cooldown;
        let withdrawn = resource.withdraw_unfrozen(after_expire).unwrap();

        assert_eq!(withdrawn, 100000000); // 1 TOS
        assert_eq!(resource.pending_unfreezes.len(), 0);
    }

    #[test]
    fn test_withdraw_partial_expired() {
        let mut resource = EnergyResource::new();
        let freeze_topoheight = 1000;
        let duration = FreezeDuration::new(7).unwrap();
        let unlock_topoheight = freeze_topoheight + duration.duration_in_blocks();
        let cooldown = crate::config::UNFREEZE_COOLDOWN_BLOCKS;

        // Freeze 3 TOS
        resource.freeze_tos_for_energy(300000000, duration, freeze_topoheight);

        // Create 3 pending unfreezes at different times
        // First unfreeze at unlock_topoheight
        resource
            .unfreeze_tos(100000000, unlock_topoheight, None)
            .unwrap();

        // Second unfreeze 100 blocks later
        resource
            .unfreeze_tos(100000000, unlock_topoheight + 100, None)
            .unwrap();

        // Third unfreeze 200 blocks later
        resource
            .unfreeze_tos(100000000, unlock_topoheight + 200, None)
            .unwrap();

        assert_eq!(resource.pending_unfreezes.len(), 3);

        // Withdraw when only first has expired (after first cooldown, before second)
        let partial_expire = unlock_topoheight + cooldown + 50;
        let withdrawn = resource.withdraw_unfrozen(partial_expire).unwrap();

        assert_eq!(withdrawn, 100000000); // Only first 1 TOS expired
        assert_eq!(resource.pending_unfreezes.len(), 2); // 2 still pending
    }

    #[test]
    fn test_withdraw_all_expired() {
        let mut resource = EnergyResource::new();
        let freeze_topoheight = 1000;
        let duration = FreezeDuration::new(7).unwrap();
        let unlock_topoheight = freeze_topoheight + duration.duration_in_blocks();
        let cooldown = crate::config::UNFREEZE_COOLDOWN_BLOCKS;

        // Freeze 3 TOS
        resource.freeze_tos_for_energy(300000000, duration, freeze_topoheight);

        // Create 3 pending unfreezes
        resource
            .unfreeze_tos(100000000, unlock_topoheight, None)
            .unwrap();
        resource
            .unfreeze_tos(100000000, unlock_topoheight + 100, None)
            .unwrap();
        resource
            .unfreeze_tos(100000000, unlock_topoheight + 200, None)
            .unwrap();

        assert_eq!(resource.pending_unfreezes.len(), 3);

        // Withdraw when all have expired
        let all_expired = unlock_topoheight + 200 + cooldown + 1;
        let withdrawn = resource.withdraw_unfrozen(all_expired).unwrap();

        assert_eq!(withdrawn, 300000000); // All 3 TOS
        assert_eq!(resource.pending_unfreezes.len(), 0);
    }

    #[test]
    fn test_withdraw_multiple_times() {
        let mut resource = EnergyResource::new();
        let freeze_topoheight = 1000;
        let duration = FreezeDuration::new(7).unwrap();
        let unlock_topoheight = freeze_topoheight + duration.duration_in_blocks();
        let cooldown = crate::config::UNFREEZE_COOLDOWN_BLOCKS;

        // Freeze 2 TOS
        resource.freeze_tos_for_energy(200000000, duration, freeze_topoheight);

        // Create 2 pending unfreezes at different times
        resource
            .unfreeze_tos(100000000, unlock_topoheight, None)
            .unwrap();
        resource
            .unfreeze_tos(100000000, unlock_topoheight + 1000, None)
            .unwrap();

        assert_eq!(resource.pending_unfreezes.len(), 2);

        // First withdraw - only first expired
        let first_withdraw_time = unlock_topoheight + cooldown;
        let first_withdrawn = resource.withdraw_unfrozen(first_withdraw_time).unwrap();
        assert_eq!(first_withdrawn, 100000000);
        assert_eq!(resource.pending_unfreezes.len(), 1);

        // Second withdraw - second now expired
        let second_withdraw_time = unlock_topoheight + 1000 + cooldown;
        let second_withdrawn = resource.withdraw_unfrozen(second_withdraw_time).unwrap();
        assert_eq!(second_withdrawn, 100000000);
        assert_eq!(resource.pending_unfreezes.len(), 0);
    }

    #[test]
    fn test_withdraw_idempotent() {
        let mut resource = EnergyResource::new();
        let freeze_topoheight = 1000;
        let duration = FreezeDuration::new(7).unwrap();
        let unlock_topoheight = freeze_topoheight + duration.duration_in_blocks();
        let cooldown = crate::config::UNFREEZE_COOLDOWN_BLOCKS;

        // Freeze and unfreeze
        resource.freeze_tos_for_energy(100000000, duration, freeze_topoheight);
        resource
            .unfreeze_tos(100000000, unlock_topoheight, None)
            .unwrap();

        let after_expire = unlock_topoheight + cooldown;

        // First withdraw
        let first = resource.withdraw_unfrozen(after_expire).unwrap();
        assert_eq!(first, 100000000);

        // Second withdraw at same time - should return 0
        let second = resource.withdraw_unfrozen(after_expire).unwrap();
        assert_eq!(second, 0);

        // Third withdraw later - still 0
        let third = resource.withdraw_unfrozen(after_expire + 1000).unwrap();
        assert_eq!(third, 0);
    }

    // ==================== Batch Delegation Unfreeze Tests ====================

    #[test]
    fn test_batch_delegation_unfreeze_single_entry() {
        use crate::crypto::KeyPair;

        let mut delegator_resource = EnergyResource::new();
        let freeze_topoheight = 1000;
        let duration = FreezeDuration::new(30).unwrap(); // 30 days
        let network = crate::network::Network::Mainnet;

        // Create 3 delegatees (B: 50 TOS, C: 30 TOS, D: 20 TOS)
        let delegatee_b = KeyPair::new().get_public_key().compress();
        let delegatee_c = KeyPair::new().get_public_key().compress();
        let delegatee_d = KeyPair::new().get_public_key().compress();

        let entry_b = DelegateRecordEntry {
            delegatee: delegatee_b.clone(),
            amount: 50 * crate::config::COIN_VALUE,
            energy: 50 * 60, // 50 TOS * 2 * 30 days = 3000 energy
        };
        let entry_c = DelegateRecordEntry {
            delegatee: delegatee_c.clone(),
            amount: 30 * crate::config::COIN_VALUE,
            energy: 30 * 60, // 30 TOS * 2 * 30 days = 1800 energy
        };
        let entry_d = DelegateRecordEntry {
            delegatee: delegatee_d.clone(),
            amount: 20 * crate::config::COIN_VALUE,
            energy: 20 * 60, // 20 TOS * 2 * 30 days = 1200 energy
        };

        // Create batch delegation with all 3 entries
        let total_amount = 100 * crate::config::COIN_VALUE;
        delegator_resource
            .create_delegated_freeze(
                vec![entry_b, entry_c, entry_d],
                duration,
                total_amount,
                freeze_topoheight,
                &network,
            )
            .unwrap();

        assert_eq!(delegator_resource.frozen_tos, total_amount);
        assert_eq!(delegator_resource.delegated_records.len(), 1);
        assert_eq!(delegator_resource.delegated_records[0].entries.len(), 3);

        // Calculate unlock time
        let unlock_topoheight =
            freeze_topoheight + duration.duration_in_blocks_for_network(&network);

        // Unfreeze B's entry (50 TOS) using selective batch delegation unfreeze
        let (affected_delegatee, energy_removed, pending_amount) = delegator_resource
            .unfreeze_delegated_entry(
                50 * crate::config::COIN_VALUE,
                unlock_topoheight,
                Some(0), // First (and only) delegation record
                &delegatee_b,
            )
            .unwrap();

        // Verify results
        assert_eq!(affected_delegatee, delegatee_b);
        assert_eq!(energy_removed, 50 * 60); // 3000 energy removed
        assert_eq!(pending_amount, 50 * crate::config::COIN_VALUE);
        assert_eq!(
            delegator_resource.frozen_tos,
            50 * crate::config::COIN_VALUE
        ); // 50 TOS remaining
        assert_eq!(delegator_resource.pending_unfreezes.len(), 1);

        // Verify record now has 2 entries (C and D)
        assert_eq!(delegator_resource.delegated_records.len(), 1);
        assert_eq!(delegator_resource.delegated_records[0].entries.len(), 2);
        assert_eq!(
            delegator_resource.delegated_records[0].total_amount,
            50 * crate::config::COIN_VALUE
        );

        // Verify B's entry is removed
        assert!(delegator_resource.delegated_records[0]
            .find_entry(&delegatee_b)
            .is_none());

        // Verify C and D still exist
        assert!(delegator_resource.delegated_records[0]
            .find_entry(&delegatee_c)
            .is_some());
        assert!(delegator_resource.delegated_records[0]
            .find_entry(&delegatee_d)
            .is_some());
    }

    #[test]
    fn test_batch_delegation_partial_entry_unfreeze() {
        use crate::crypto::KeyPair;

        let mut delegator_resource = EnergyResource::new();
        let freeze_topoheight = 1000;
        let duration = FreezeDuration::new(30).unwrap();
        let network = crate::network::Network::Mainnet;

        // Create 2 delegatees (B: 50 TOS, C: 50 TOS)
        let delegatee_b = KeyPair::new().get_public_key().compress();
        let delegatee_c = KeyPair::new().get_public_key().compress();

        let entry_b = DelegateRecordEntry {
            delegatee: delegatee_b.clone(),
            amount: 50 * crate::config::COIN_VALUE,
            energy: 50 * 60, // 3000 energy
        };
        let entry_c = DelegateRecordEntry {
            delegatee: delegatee_c.clone(),
            amount: 50 * crate::config::COIN_VALUE,
            energy: 50 * 60, // 3000 energy
        };

        let total_amount = 100 * crate::config::COIN_VALUE;
        delegator_resource
            .create_delegated_freeze(
                vec![entry_b, entry_c],
                duration,
                total_amount,
                freeze_topoheight,
                &network,
            )
            .unwrap();

        let unlock_topoheight =
            freeze_topoheight + duration.duration_in_blocks_for_network(&network);

        // Partial unfreeze from B (30 TOS out of 50 TOS)
        let (affected_delegatee, energy_removed, pending_amount) = delegator_resource
            .unfreeze_delegated_entry(
                30 * crate::config::COIN_VALUE,
                unlock_topoheight,
                Some(0),
                &delegatee_b,
            )
            .unwrap();

        // Verify results
        assert_eq!(affected_delegatee, delegatee_b);
        // Energy removed proportionally: 3000 * 30/50 = 1800
        assert_eq!(energy_removed, 30 * 60);
        assert_eq!(pending_amount, 30 * crate::config::COIN_VALUE);
        assert_eq!(
            delegator_resource.frozen_tos,
            70 * crate::config::COIN_VALUE
        ); // 70 TOS remaining

        // Verify record still has 2 entries
        assert_eq!(delegator_resource.delegated_records[0].entries.len(), 2);

        // Verify B's entry is updated (20 TOS remaining)
        let b_entry = delegator_resource.delegated_records[0]
            .find_entry(&delegatee_b)
            .unwrap();
        assert_eq!(b_entry.amount, 20 * crate::config::COIN_VALUE);
        assert_eq!(b_entry.energy, 20 * 60); // 1200 energy remaining

        // Verify C's entry is unchanged
        let c_entry = delegator_resource.delegated_records[0]
            .find_entry(&delegatee_c)
            .unwrap();
        assert_eq!(c_entry.amount, 50 * crate::config::COIN_VALUE);
        assert_eq!(c_entry.energy, 50 * 60);
    }

    #[test]
    fn test_batch_delegation_unfreeze_last_entry_removes_record() {
        use crate::crypto::KeyPair;

        let mut delegator_resource = EnergyResource::new();
        let freeze_topoheight = 1000;
        let duration = FreezeDuration::new(7).unwrap();
        let network = crate::network::Network::Mainnet;

        // Create single delegatee
        let delegatee = KeyPair::new().get_public_key().compress();

        let entry = DelegateRecordEntry {
            delegatee: delegatee.clone(),
            amount: 10 * crate::config::COIN_VALUE,
            energy: 10 * 14, // 140 energy
        };

        delegator_resource
            .create_delegated_freeze(
                vec![entry],
                duration,
                10 * crate::config::COIN_VALUE,
                freeze_topoheight,
                &network,
            )
            .unwrap();

        let unlock_topoheight =
            freeze_topoheight + duration.duration_in_blocks_for_network(&network);

        // Unfreeze entire entry
        let result = delegator_resource.unfreeze_delegated_entry(
            10 * crate::config::COIN_VALUE,
            unlock_topoheight,
            None, // Single record, no index needed
            &delegatee,
        );

        assert!(result.is_ok());
        let (_, energy_removed, _) = result.unwrap();
        assert_eq!(energy_removed, 140);

        // Verify record is completely removed
        assert!(delegator_resource.delegated_records.is_empty());
        assert_eq!(delegator_resource.frozen_tos, 0);
    }

    #[test]
    fn test_batch_delegation_unfreeze_requires_address() {
        use crate::crypto::KeyPair;

        let mut delegator_resource = EnergyResource::new();
        let freeze_topoheight = 1000;
        let duration = FreezeDuration::new(7).unwrap();
        let network = crate::network::Network::Mainnet;

        // Create 2 delegatees
        let delegatee_a = KeyPair::new().get_public_key().compress();
        let delegatee_b = KeyPair::new().get_public_key().compress();

        let entries = vec![
            DelegateRecordEntry {
                delegatee: delegatee_a,
                amount: crate::config::COIN_VALUE,
                energy: 14,
            },
            DelegateRecordEntry {
                delegatee: delegatee_b,
                amount: crate::config::COIN_VALUE,
                energy: 14,
            },
        ];

        delegator_resource
            .create_delegated_freeze(
                entries,
                duration,
                2 * crate::config::COIN_VALUE,
                freeze_topoheight,
                &network,
            )
            .unwrap();

        let unlock_topoheight =
            freeze_topoheight + duration.duration_in_blocks_for_network(&network);

        // Try to use FIFO unfreeze (unfreeze_delegated) - should work but uses FIFO order
        // The unfreeze_delegated_entry requires explicit delegatee_address
        // If we pass a wrong address, it should fail
        let wrong_delegatee = KeyPair::new().get_public_key().compress();
        let result = delegator_resource.unfreeze_delegated_entry(
            crate::config::COIN_VALUE,
            unlock_topoheight,
            Some(0),
            &wrong_delegatee,
        );

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("Delegatee not found in record"));
    }

    #[test]
    fn test_batch_delegation_unfreeze_amount_exceeds_entry() {
        use crate::crypto::KeyPair;

        let mut delegator_resource = EnergyResource::new();
        let freeze_topoheight = 1000;
        let duration = FreezeDuration::new(7).unwrap();
        let network = crate::network::Network::Mainnet;

        let delegatee = KeyPair::new().get_public_key().compress();

        let entry = DelegateRecordEntry {
            delegatee: delegatee.clone(),
            amount: 5 * crate::config::COIN_VALUE, // 5 TOS
            energy: 5 * 14,
        };

        delegator_resource
            .create_delegated_freeze(
                vec![entry],
                duration,
                5 * crate::config::COIN_VALUE,
                freeze_topoheight,
                &network,
            )
            .unwrap();

        let unlock_topoheight =
            freeze_topoheight + duration.duration_in_blocks_for_network(&network);

        // Try to unfreeze 10 TOS from 5 TOS entry
        let result = delegator_resource.unfreeze_delegated_entry(
            10 * crate::config::COIN_VALUE,
            unlock_topoheight,
            None,
            &delegatee,
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Amount exceeds entry amount"));
    }

    #[test]
    fn test_batch_delegation_unfreeze_locked() {
        use crate::crypto::KeyPair;

        let mut delegator_resource = EnergyResource::new();
        let freeze_topoheight = 1000;
        let duration = FreezeDuration::new(30).unwrap();
        let network = crate::network::Network::Mainnet;

        let delegatee = KeyPair::new().get_public_key().compress();

        let entry = DelegateRecordEntry {
            delegatee: delegatee.clone(),
            amount: crate::config::COIN_VALUE,
            energy: 60,
        };

        delegator_resource
            .create_delegated_freeze(
                vec![entry],
                duration,
                crate::config::COIN_VALUE,
                freeze_topoheight,
                &network,
            )
            .unwrap();

        // Try to unfreeze before unlock (should fail)
        let result = delegator_resource.unfreeze_delegated_entry(
            crate::config::COIN_VALUE,
            freeze_topoheight + 100, // Still locked
            None,
            &delegatee,
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Record is still locked"));
    }

    #[test]
    fn test_batch_delegation_multiple_records_requires_index() {
        use crate::crypto::KeyPair;

        let mut delegator_resource = EnergyResource::new();
        let freeze_topoheight = 1000;
        let duration = FreezeDuration::new(7).unwrap();
        let network = crate::network::Network::Mainnet;

        let delegatee1 = KeyPair::new().get_public_key().compress();
        let delegatee2 = KeyPair::new().get_public_key().compress();

        // Create first delegation record
        let entry1 = DelegateRecordEntry {
            delegatee: delegatee1.clone(),
            amount: crate::config::COIN_VALUE,
            energy: 14,
        };
        delegator_resource
            .create_delegated_freeze(
                vec![entry1],
                duration,
                crate::config::COIN_VALUE,
                freeze_topoheight,
                &network,
            )
            .unwrap();

        // Create second delegation record
        let entry2 = DelegateRecordEntry {
            delegatee: delegatee2.clone(),
            amount: crate::config::COIN_VALUE,
            energy: 14,
        };
        delegator_resource
            .create_delegated_freeze(
                vec![entry2],
                duration,
                crate::config::COIN_VALUE,
                freeze_topoheight + 1000,
                &network,
            )
            .unwrap();

        assert_eq!(delegator_resource.delegated_records.len(), 2);

        let unlock_topoheight =
            freeze_topoheight + 1000 + duration.duration_in_blocks_for_network(&network);

        // Try to unfreeze without specifying record_index (should fail)
        let result = delegator_resource.unfreeze_delegated_entry(
            crate::config::COIN_VALUE,
            unlock_topoheight,
            None, // No index when multiple records exist
            &delegatee2,
        );

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("Multiple delegation records exist"));

        // With correct index, should succeed
        let result = delegator_resource.unfreeze_delegated_entry(
            crate::config::COIN_VALUE,
            unlock_topoheight,
            Some(1), // Second record
            &delegatee2,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_batch_delegation_max_delegatees() {
        use crate::crypto::KeyPair;

        let mut delegator_resource = EnergyResource::new();
        let freeze_topoheight = 1000;
        let duration = FreezeDuration::new(7).unwrap();
        let network = crate::network::Network::Mainnet;

        // Create exactly MAX_DELEGATEES entries (should succeed)
        let max_entries: Vec<DelegateRecordEntry> = (0..crate::config::MAX_DELEGATEES)
            .map(|_| DelegateRecordEntry {
                delegatee: KeyPair::new().get_public_key().compress(),
                amount: crate::config::COIN_VALUE,
                energy: 14,
            })
            .collect();

        let total_amount = crate::config::MAX_DELEGATEES as u64 * crate::config::COIN_VALUE;

        let result = delegator_resource.create_delegated_freeze(
            max_entries,
            duration,
            total_amount,
            freeze_topoheight,
            &network,
        );
        assert!(result.is_ok());

        // Reset for next test
        let mut delegator_resource2 = EnergyResource::new();

        // Create MAX_DELEGATEES + 1 entries (should fail)
        let too_many_entries: Vec<DelegateRecordEntry> = (0..crate::config::MAX_DELEGATEES + 1)
            .map(|_| DelegateRecordEntry {
                delegatee: KeyPair::new().get_public_key().compress(),
                amount: crate::config::COIN_VALUE,
                energy: 14,
            })
            .collect();

        let total_amount2 = (crate::config::MAX_DELEGATEES + 1) as u64 * crate::config::COIN_VALUE;

        let result2 = delegator_resource2.create_delegated_freeze(
            too_many_entries,
            duration,
            total_amount2,
            freeze_topoheight,
            &network,
        );
        assert!(result2.is_err());
        assert!(result2.unwrap_err().contains("Too many delegatees"));
    }

    // Note: Self-delegation rejection is tested at the transaction verification layer
    // in common/src/transaction/verify/mod.rs. The check compares entry.delegatee == self.source
    // and rejects with "Cannot delegate energy to yourself" error.
    // This cannot be easily unit tested here as it requires transaction context.
    // See integration tests for full verification testing.
}
