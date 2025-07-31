use serde::{Deserialize, Serialize};
use crate::{
    crypto::PublicKey,
    serializer::{Serializer, Writer, Reader, ReaderError},
    block::TopoHeight,
};

/// Flexible freeze duration for TOS staking
/// Users can set custom days from 3 to 90 days
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct FreezeDuration {
    /// Number of days to freeze (3-90 days)
    pub days: u32,
}

impl FreezeDuration {
    /// Create a new freeze duration with validation
    pub fn new(days: u32) -> Result<Self, &'static str> {
        if days < crate::config::MIN_FREEZE_DURATION_DAYS || days > crate::config::MAX_FREEZE_DURATION_DAYS {
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
    pub fn duration_in_blocks(&self) -> u64 {
        self.days as u64 * 24 * 60 * 60  // days * 24 hours * 60 minutes * 60 seconds
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
        self.days >= crate::config::MIN_FREEZE_DURATION_DAYS && self.days <= crate::config::MAX_FREEZE_DURATION_DAYS
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
        Ok(Self { days })
    }

    fn size(&self) -> usize {
        self.days.size()
    }
}

/// Freeze record for tracking individual freeze operations
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
    /// Create a new freeze record
    /// Ensures TOS amount is a whole number (multiple of COIN_VALUE)
    pub fn new(amount: u64, duration: FreezeDuration, freeze_topoheight: TopoHeight) -> Self {
        let unlock_topoheight = freeze_topoheight + duration.duration_in_blocks();
        
        // Ensure amount is a whole number of TOS (multiple of COIN_VALUE)
        let whole_tos_amount = (amount / crate::config::COIN_VALUE) * crate::config::COIN_VALUE;
        
        // Calculate energy gained using integer arithmetic
        let energy_gained = (whole_tos_amount / crate::config::COIN_VALUE) * duration.reward_multiplier();
        
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
        if current_topoheight >= self.unlock_topoheight {
            0
        } else {
            self.unlock_topoheight - current_topoheight
        }
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
        self.amount.size() + self.duration.size() + 
        self.freeze_topoheight.size() + self.unlock_topoheight.size() + 
        self.energy_gained.size()
    }
}

/// Energy resource management for Terminos
/// Enhanced with TRON-style freeze duration and reward multiplier system
#[derive(Debug, Clone, Serialize, Deserialize)]
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
        Self {
            total_energy: 0,
            used_energy: 0,
            frozen_tos: 0,
            last_update: 0,
            freeze_records: Vec::new(),
        }
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
    pub fn freeze_tos_for_energy(&mut self, tos_amount: u64, duration: FreezeDuration, topoheight: TopoHeight) -> u64 {
        // Create a new freeze record (this will ensure whole number TOS)
        let freeze_record = FreezeRecord::new(tos_amount, duration, topoheight);
        let energy_gained = freeze_record.energy_gained;
        let actual_tos_frozen = freeze_record.amount;
        
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
    pub fn unfreeze_tos(&mut self, tos_amount: u64, current_topoheight: TopoHeight) -> Result<u64, String> {
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
            let energy_to_remove = (unfreeze_amount / crate::config::COIN_VALUE) * record.duration.reward_multiplier();

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
            let remaining_energy = (record.amount / crate::config::COIN_VALUE) * record.duration.reward_multiplier();
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
        self.freeze_records.iter()
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
    pub fn get_freeze_records_by_duration(&self) -> std::collections::HashMap<FreezeDuration, Vec<&FreezeRecord>> {
        let mut grouped: std::collections::HashMap<FreezeDuration, Vec<&FreezeRecord>> = std::collections::HashMap::new();
        
        for record in &self.freeze_records {
            grouped.entry(record.duration.clone()).or_insert_with(Vec::new).push(record);
        }
        
        grouped
    }

    /// Reset used energy (called periodically)
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
        let base_size = self.total_energy.size() + self.used_energy.size() + 
                       self.frozen_tos.size() + self.last_update.size();
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
        self.lessor.size() + self.lessee.size() + self.energy_amount.size() + 
        self.duration.size() + self.start_topoheight.size() + self.price_per_energy.size()
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
        assert_eq!(resource.freeze_records.len(), deserialized.freeze_records.len());
    }

    #[test]
    fn test_freeze_duration_serialization() {
        let durations = [FreezeDuration::new(3).unwrap(), FreezeDuration::new(7).unwrap(), FreezeDuration::new(14).unwrap()];
        
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