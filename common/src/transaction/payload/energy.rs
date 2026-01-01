use crate::{
    account::FreezeDuration,
    serializer::{Reader, ReaderError, Serializer, Writer},
};
use serde::{Deserialize, Serialize};

/// Energy-related transaction payloads for Transfer operations only
/// Enhanced with freeze duration and reward multiplier system
///
/// # Supported Operations
/// - `FreezeTos`: Lock TOS to gain energy for free transfers
/// - `UnfreezeTos`: Unlock previously frozen TOS (after lock period expires)
///
/// # Fee Model
/// - Energy operations themselves don't consume energy
/// - Small TOS fees are required to prevent spam/abuse
/// - Only regular transfer transactions consume energy
///
/// # Edge Cases
/// - Freeze amounts must be whole TOS (fractional parts ignored)
/// - Unfreeze only works after the lock period expires
/// - Multiple freeze operations with different durations are supported
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EnergyPayload {
    /// Freeze TOS to get energy for free transfers with duration-based rewards
    FreezeTos {
        /// Amount of TOS to freeze
        amount: u64,
        /// Freeze duration (3, 7, or 14 days) affecting reward multiplier
        duration: FreezeDuration,
    },
    /// Unfreeze TOS (release frozen TOS) - can only unfreeze after lock period
    UnfreezeTos {
        /// Amount of TOS to unfreeze
        amount: u64,
    },
}

impl EnergyPayload {
    /// Get the energy cost for this operation
    /// FreezeTos and UnfreezeTos operations don't consume energy but require TOS fees
    /// Only Transfer transactions consume energy
    pub fn energy_cost(&self) -> u64 {
        match self {
            // FreezeTos and UnfreezeTos don't consume energy
            // They require TOS fees to prevent abuse
            Self::FreezeTos { .. } => 0,
            Self::UnfreezeTos { .. } => 0,
        }
    }

    /// Get the TOS fee required for this operation
    /// FreezeTos and UnfreezeTos require small TOS fees to prevent abuse
    pub fn tos_fee(&self) -> u64 {
        use crate::config::FEE_PER_TRANSFER;

        match self {
            // Small TOS fee to prevent frequent freeze/unfreeze abuse
            Self::FreezeTos { .. } => FEE_PER_TRANSFER,
            Self::UnfreezeTos { .. } => FEE_PER_TRANSFER,
        }
    }

    /// Check if this operation requires account activation
    /// Energy operations don't require special activation
    pub fn requires_activation(&self) -> bool {
        false
    }

    /// Get the amount of TOS involved in this operation
    pub fn get_amount(&self) -> u64 {
        match self {
            Self::FreezeTos { amount, .. } => *amount,
            Self::UnfreezeTos { amount } => *amount,
        }
    }

    /// Get the freeze duration (only applicable to FreezeTos operations)
    pub fn get_duration(&self) -> Option<FreezeDuration> {
        match self {
            Self::FreezeTos { duration, .. } => Some(*duration),
            Self::UnfreezeTos { .. } => None,
        }
    }

    /// Calculate the energy that would be gained from this freeze operation
    /// Returns None for unfreeze operations
    pub fn calculate_energy_gain(&self) -> Option<u64> {
        match self {
            Self::FreezeTos { amount, duration } => {
                Some((*amount / crate::config::COIN_VALUE) * duration.reward_multiplier())
            }
            Self::UnfreezeTos { .. } => None,
        }
    }
}

impl Serializer for EnergyPayload {
    fn write(&self, writer: &mut Writer) {
        match self {
            Self::FreezeTos { amount, duration } => {
                writer.write_u8(0);
                writer.write_u64(amount);
                duration.write(writer);
            }
            Self::UnfreezeTos { amount } => {
                writer.write_u8(1);
                writer.write_u64(amount);
            }
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        let variant = reader.read_u8()?;
        match variant {
            0 => {
                let amount = reader.read_u64()?;
                let duration = FreezeDuration::read(reader)?;
                Ok(Self::FreezeTos { amount, duration })
            }
            1 => {
                let amount = reader.read_u64()?;
                Ok(Self::UnfreezeTos { amount })
            }
            _ => Err(ReaderError::InvalidValue),
        }
    }

    fn size(&self) -> usize {
        match self {
            Self::FreezeTos { amount, duration } => 1 + amount.size() + duration.size(),
            Self::UnfreezeTos { amount } => 1 + amount.size(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::COIN_VALUE;

    #[test]
    fn test_freeze_tos_payload_creation() {
        let duration = FreezeDuration::new(7).unwrap();
        let payload = EnergyPayload::FreezeTos {
            amount: 100000000, // 1 TOS
            duration,
        };

        assert_eq!(payload.get_amount(), 100000000);
        assert_eq!(payload.get_duration(), Some(duration));
        assert_eq!(payload.calculate_energy_gain(), Some(14)); // 1 TOS * 14 = 14 transfers
    }

    #[test]
    fn test_unfreeze_tos_payload_creation() {
        let payload = EnergyPayload::UnfreezeTos { amount: 500 };

        assert_eq!(payload.get_amount(), 500);
        assert_eq!(payload.get_duration(), None);
        assert_eq!(payload.calculate_energy_gain(), None);
    }

    #[test]
    fn test_energy_cost() {
        let duration = FreezeDuration::new(3).unwrap();
        let freeze_payload = EnergyPayload::FreezeTos {
            amount: 1000,
            duration,
        };
        let unfreeze_payload = EnergyPayload::UnfreezeTos { amount: 500 };

        assert_eq!(freeze_payload.energy_cost(), 0);
        assert_eq!(unfreeze_payload.energy_cost(), 0);
    }

    #[test]
    fn test_serialization() {
        let duration = FreezeDuration::new(14).unwrap();
        let payload = EnergyPayload::FreezeTos {
            amount: 1000,
            duration,
        };

        let mut bytes = Vec::new();
        let mut writer = crate::serializer::Writer::new(&mut bytes);
        payload.write(&mut writer);

        let mut reader = crate::serializer::Reader::new(&bytes);
        let deserialized = EnergyPayload::read(&mut reader).unwrap();

        match deserialized {
            EnergyPayload::FreezeTos { amount, duration } => {
                assert_eq!(amount, 1000);
                assert_eq!(duration, duration);
            }
            _ => panic!("Expected FreezeTos payload"),
        }
    }

    #[test]
    fn test_different_duration_rewards() {
        let amounts = [100000000, 200000000, 300000000]; // 1, 2, 3 TOS
        let durations = [
            FreezeDuration::new(3).unwrap(),
            FreezeDuration::new(7).unwrap(),
            FreezeDuration::new(14).unwrap(),
        ];

        for amount in amounts {
            for duration in &durations {
                let payload = EnergyPayload::FreezeTos {
                    amount,
                    duration: *duration,
                };

                let expected_energy = (amount / COIN_VALUE) * duration.reward_multiplier();
                assert_eq!(payload.calculate_energy_gain(), Some(expected_energy));
            }
        }
    }
}
