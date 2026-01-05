use crate::{
    account::FreezeDuration,
    crypto::PublicKey,
    serializer::{Reader, ReaderError, Serializer, Writer},
};
use serde::{Deserialize, Serialize};

/// Delegation entry for batch delegation in FreezeTos
///
/// # Fields
/// - `delegatee`: The account receiving delegated energy
/// - `amount`: Amount of TOS to delegate to this delegatee (must be whole TOS)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationEntry {
    /// Delegatee account (receives energy)
    pub delegatee: PublicKey,
    /// Amount of TOS to delegate
    pub amount: u64,
}

impl Serializer for DelegationEntry {
    fn write(&self, writer: &mut Writer) {
        self.delegatee.write(writer);
        writer.write_u64(&self.amount);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        Ok(Self {
            delegatee: PublicKey::read(reader)?,
            amount: reader.read_u64()?,
        })
    }

    fn size(&self) -> usize {
        self.delegatee.size() + self.amount.size()
    }
}

/// Energy-related transaction payloads for Transfer operations only
/// Enhanced with TRON-style freeze duration and reward multiplier system
///
/// # Supported Operations
/// - `FreezeTos`: Lock TOS to gain energy for free transfers (self-freeze)
/// - `FreezeTosDelegate`: Lock TOS and delegate energy to others (batch delegation)
/// - `UnfreezeTos`: Unlock previously frozen TOS (Phase 1: creates pending unfreeze)
/// - `WithdrawUnfrozen`: Retrieve TOS after 14-day cooldown (Phase 2)
///
/// # Fee Model
/// - All energy operations are FREE (no TOS fee, no energy consumption)
/// - This encourages staking participation and network security
/// - Only regular transfer transactions consume energy
///
/// # Edge Cases
/// - Freeze amounts must be whole TOS (minimum 1 TOS)
/// - Unfreeze only works after the lock period expires
/// - Multiple freeze operations with different durations are supported
/// - Batch delegation supports up to 500 delegatees per transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EnergyPayload {
    /// Freeze TOS to get energy for free transfers (self-freeze)
    FreezeTos {
        /// Amount of TOS to freeze (must be whole TOS, minimum 1 TOS)
        amount: u64,
        /// Freeze duration (3-365 days) affecting reward multiplier
        duration: FreezeDuration,
    },
    /// Freeze TOS and delegate energy to others (batch delegation)
    FreezeTosDelegate {
        /// List of delegatees and their amounts (max 500 entries)
        delegatees: Vec<DelegationEntry>,
        /// Freeze duration (3-365 days) affecting reward multiplier
        duration: FreezeDuration,
    },
    /// Unfreeze TOS - Phase 1: Creates pending unfreeze, energy removed immediately
    UnfreezeTos {
        /// Amount of TOS to unfreeze (must be whole TOS, minimum 1 TOS)
        amount: u64,
        /// Whether to unfreeze from delegation records (true) or self-freeze records (false)
        from_delegation: bool,
        /// Optional record index for selective unfreeze (0-based)
        /// - None: Use FIFO order for self-freeze, or implicit selection for single delegation
        /// - Some(idx): Unfreeze specific record at that index
        record_index: Option<u32>,
        /// Optional delegatee address for batch delegation unfreeze
        /// - Required when unfreezing from a batch delegation record
        /// - Specifies which delegatee's entry to unfreeze
        delegatee_address: Option<PublicKey>,
    },
    /// Withdraw unfrozen TOS - Phase 2: After 14-day cooldown, TOS returned to balance
    WithdrawUnfrozen,
}

impl EnergyPayload {
    /// Get the energy cost for this operation
    /// All Energy operations are FREE (no energy consumption)
    pub fn energy_cost(&self) -> u64 {
        // All energy operations are FREE - no energy consumption
        0
    }

    /// Get the TOS fee required for this operation
    /// All Energy operations are FREE (no TOS fee)
    pub fn tos_fee(&self) -> u64 {
        // All energy operations are FREE - encourages staking participation
        0
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
            Self::FreezeTosDelegate { delegatees, .. } => delegatees.iter().map(|d| d.amount).sum(),
            Self::UnfreezeTos { amount, .. } => *amount,
            Self::WithdrawUnfrozen => 0, // Amount determined at execution
        }
    }

    /// Get the freeze duration (only applicable to FreezeTos operations)
    pub fn get_duration(&self) -> Option<FreezeDuration> {
        match self {
            Self::FreezeTos { duration, .. } => Some(*duration),
            Self::FreezeTosDelegate { duration, .. } => Some(*duration),
            Self::UnfreezeTos { .. } => None,
            Self::WithdrawUnfrozen => None,
        }
    }

    /// Calculate the energy that would be gained from this freeze operation
    /// Returns None for unfreeze/withdraw operations
    pub fn calculate_energy_gain(&self) -> Option<u64> {
        match self {
            Self::FreezeTos { amount, duration } => {
                (amount / crate::config::COIN_VALUE).checked_mul(duration.reward_multiplier())
            }
            Self::FreezeTosDelegate {
                delegatees,
                duration,
            } => {
                let total_amount: u64 = delegatees
                    .iter()
                    .try_fold(0u64, |acc, d| acc.checked_add(d.amount))?;
                (total_amount / crate::config::COIN_VALUE).checked_mul(duration.reward_multiplier())
            }
            Self::UnfreezeTos { .. } => None,
            Self::WithdrawUnfrozen => None,
        }
    }

    /// Check if this is a delegation operation
    pub fn is_delegation(&self) -> bool {
        matches!(self, Self::FreezeTosDelegate { .. })
    }

    /// Get delegatees (only for FreezeTosDelegate)
    pub fn get_delegatees(&self) -> Option<&Vec<DelegationEntry>> {
        match self {
            Self::FreezeTosDelegate { delegatees, .. } => Some(delegatees),
            _ => None,
        }
    }

    /// Check if this is an unfreeze from delegation
    pub fn is_unfreeze_from_delegation(&self) -> bool {
        matches!(
            self,
            Self::UnfreezeTos {
                from_delegation: true,
                ..
            }
        )
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
            Self::FreezeTosDelegate {
                delegatees,
                duration,
            } => {
                writer.write_u8(1);
                writer.write_u64(&(delegatees.len() as u64));
                for entry in delegatees {
                    entry.write(writer);
                }
                duration.write(writer);
            }
            Self::UnfreezeTos {
                amount,
                from_delegation,
                record_index,
                delegatee_address,
            } => {
                writer.write_u8(2);
                writer.write_u64(amount);
                writer.write_bool(*from_delegation);
                // Write record_index as Option: 0 for None, 1 + value for Some
                match record_index {
                    None => writer.write_u8(0),
                    Some(idx) => {
                        writer.write_u8(1);
                        writer.write_u32(idx);
                    }
                }
                // Write delegatee_address as Option: 0 for None, 1 + pubkey for Some
                match delegatee_address {
                    None => writer.write_u8(0),
                    Some(addr) => {
                        writer.write_u8(1);
                        addr.write(writer);
                    }
                }
            }
            Self::WithdrawUnfrozen => {
                writer.write_u8(3);
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
                let count = reader.read_u64()? as usize;
                if count > crate::config::MAX_DELEGATEES {
                    return Err(ReaderError::InvalidValue);
                }
                let mut delegatees = Vec::with_capacity(count);
                for _ in 0..count {
                    delegatees.push(DelegationEntry::read(reader)?);
                }
                let duration = FreezeDuration::read(reader)?;
                Ok(Self::FreezeTosDelegate {
                    delegatees,
                    duration,
                })
            }
            2 => {
                let amount = reader.read_u64()?;
                let from_delegation = reader.read_bool()?;
                // Read record_index as Option: 0 for None, 1 + value for Some
                let record_index = match reader.read_u8()? {
                    0 => None,
                    1 => Some(reader.read_u32()?),
                    _ => return Err(ReaderError::InvalidValue),
                };
                // Read delegatee_address as Option: 0 for None, 1 + pubkey for Some
                let delegatee_address = match reader.read_u8()? {
                    0 => None,
                    1 => Some(PublicKey::read(reader)?),
                    _ => return Err(ReaderError::InvalidValue),
                };
                Ok(Self::UnfreezeTos {
                    amount,
                    from_delegation,
                    record_index,
                    delegatee_address,
                })
            }
            3 => Ok(Self::WithdrawUnfrozen),
            _ => Err(ReaderError::InvalidValue),
        }
    }

    fn size(&self) -> usize {
        match self {
            Self::FreezeTos { amount, duration } => 1 + amount.size() + duration.size(),
            Self::FreezeTosDelegate {
                delegatees,
                duration,
            } => 1 + 8 + delegatees.iter().map(|e| e.size()).sum::<usize>() + duration.size(),
            Self::UnfreezeTos {
                amount,
                record_index,
                delegatee_address,
                ..
            } => {
                // 1 (variant) + 8 (amount) + 1 (bool) + 1 (option tag) + optional 4 (u32)
                let record_index_size = match record_index {
                    None => 1,        // Just the 0 tag
                    Some(_) => 1 + 4, // 1 tag + 4 bytes for u32
                };
                // delegatee_address: 1 (option tag) + optional 32 (pubkey)
                let delegatee_size = match delegatee_address {
                    None => 1,
                    Some(addr) => 1 + addr.size(),
                };
                1 + amount.size() + 1 + record_index_size + delegatee_size
            }
            Self::WithdrawUnfrozen => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::COIN_VALUE;

    fn create_test_pubkey() -> PublicKey {
        // Create a dummy public key for testing
        let bytes = [0u8; 32];
        PublicKey::from_bytes(&bytes).unwrap_or_else(|_| {
            // Fallback: create from valid test bytes
            let mut valid_bytes = [0u8; 32];
            valid_bytes[0] = 1;
            PublicKey::from_bytes(&valid_bytes).expect("should create valid pubkey")
        })
    }

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
        assert!(!payload.is_delegation());
    }

    #[test]
    fn test_freeze_tos_delegate_payload() {
        let duration = FreezeDuration::new(7).unwrap();
        let delegatees = vec![
            DelegationEntry {
                delegatee: create_test_pubkey(),
                amount: 100000000, // 1 TOS
            },
            DelegationEntry {
                delegatee: create_test_pubkey(),
                amount: 200000000, // 2 TOS
            },
        ];

        let payload = EnergyPayload::FreezeTosDelegate {
            delegatees,
            duration,
        };

        assert_eq!(payload.get_amount(), 300000000); // 3 TOS total
        assert_eq!(payload.get_duration(), Some(duration));
        assert_eq!(payload.calculate_energy_gain(), Some(42)); // 3 TOS * 14 = 42 transfers
        assert!(payload.is_delegation());
    }

    #[test]
    fn test_unfreeze_tos_payload_creation() {
        let payload = EnergyPayload::UnfreezeTos {
            amount: COIN_VALUE,
            from_delegation: false,
            record_index: None,
            delegatee_address: None,
        };

        assert_eq!(payload.get_amount(), COIN_VALUE);
        assert_eq!(payload.get_duration(), None);
        assert_eq!(payload.calculate_energy_gain(), None);
        assert!(!payload.is_unfreeze_from_delegation());
    }

    #[test]
    fn test_unfreeze_tos_with_record_index() {
        let payload = EnergyPayload::UnfreezeTos {
            amount: COIN_VALUE,
            from_delegation: false,
            record_index: Some(2),
            delegatee_address: None,
        };

        assert_eq!(payload.get_amount(), COIN_VALUE);
        assert!(!payload.is_unfreeze_from_delegation());
    }

    #[test]
    fn test_unfreeze_from_delegation() {
        let payload = EnergyPayload::UnfreezeTos {
            amount: COIN_VALUE,
            from_delegation: true,
            record_index: None,
            delegatee_address: None,
        };

        assert!(payload.is_unfreeze_from_delegation());
    }

    #[test]
    fn test_unfreeze_with_delegatee_address() {
        let delegatee = create_test_pubkey();
        let payload = EnergyPayload::UnfreezeTos {
            amount: COIN_VALUE,
            from_delegation: true,
            record_index: Some(0),
            delegatee_address: Some(delegatee),
        };

        assert!(payload.is_unfreeze_from_delegation());
        assert_eq!(payload.get_amount(), COIN_VALUE);
    }

    #[test]
    fn test_withdraw_unfrozen_payload() {
        let payload = EnergyPayload::WithdrawUnfrozen;

        assert_eq!(payload.get_amount(), 0);
        assert_eq!(payload.get_duration(), None);
        assert_eq!(payload.calculate_energy_gain(), None);
    }

    #[test]
    fn test_energy_operations_are_free() {
        let duration = FreezeDuration::new(3).unwrap();

        let freeze_payload = EnergyPayload::FreezeTos {
            amount: COIN_VALUE,
            duration,
        };
        let delegate_payload = EnergyPayload::FreezeTosDelegate {
            delegatees: vec![],
            duration,
        };
        let unfreeze_payload = EnergyPayload::UnfreezeTos {
            amount: COIN_VALUE,
            from_delegation: false,
            record_index: None,
            delegatee_address: None,
        };
        let withdraw_payload = EnergyPayload::WithdrawUnfrozen;

        // All operations should be FREE
        assert_eq!(freeze_payload.energy_cost(), 0);
        assert_eq!(freeze_payload.tos_fee(), 0);
        assert_eq!(delegate_payload.energy_cost(), 0);
        assert_eq!(delegate_payload.tos_fee(), 0);
        assert_eq!(unfreeze_payload.energy_cost(), 0);
        assert_eq!(unfreeze_payload.tos_fee(), 0);
        assert_eq!(withdraw_payload.energy_cost(), 0);
        assert_eq!(withdraw_payload.tos_fee(), 0);
    }

    #[test]
    fn test_serialization_freeze_tos() {
        let duration = FreezeDuration::new(14).unwrap();
        let payload = EnergyPayload::FreezeTos {
            amount: COIN_VALUE,
            duration,
        };

        let mut bytes = Vec::new();
        let mut writer = crate::serializer::Writer::new(&mut bytes);
        payload.write(&mut writer);

        let mut reader = crate::serializer::Reader::new(&bytes);
        let deserialized = EnergyPayload::read(&mut reader).unwrap();

        match deserialized {
            EnergyPayload::FreezeTos {
                amount,
                duration: d,
            } => {
                assert_eq!(amount, COIN_VALUE);
                assert_eq!(d, duration);
            }
            _ => panic!("Expected FreezeTos payload"),
        }
    }

    #[test]
    fn test_serialization_unfreeze_tos() {
        // Test with record_index = None, delegatee_address = None
        let payload = EnergyPayload::UnfreezeTos {
            amount: COIN_VALUE,
            from_delegation: true,
            record_index: None,
            delegatee_address: None,
        };

        let mut bytes = Vec::new();
        let mut writer = crate::serializer::Writer::new(&mut bytes);
        payload.write(&mut writer);

        let mut reader = crate::serializer::Reader::new(&bytes);
        let deserialized = EnergyPayload::read(&mut reader).unwrap();

        match deserialized {
            EnergyPayload::UnfreezeTos {
                amount,
                from_delegation,
                record_index,
                delegatee_address,
            } => {
                assert_eq!(amount, COIN_VALUE);
                assert!(from_delegation);
                assert_eq!(record_index, None);
                assert_eq!(delegatee_address, None);
            }
            _ => panic!("Expected UnfreezeTos payload"),
        }
    }

    #[test]
    fn test_serialization_unfreeze_tos_with_index() {
        // Test with record_index = Some(5), delegatee_address = None
        let payload = EnergyPayload::UnfreezeTos {
            amount: COIN_VALUE * 3,
            from_delegation: false,
            record_index: Some(5),
            delegatee_address: None,
        };

        let mut bytes = Vec::new();
        let mut writer = crate::serializer::Writer::new(&mut bytes);
        payload.write(&mut writer);

        let mut reader = crate::serializer::Reader::new(&bytes);
        let deserialized = EnergyPayload::read(&mut reader).unwrap();

        match deserialized {
            EnergyPayload::UnfreezeTos {
                amount,
                from_delegation,
                record_index,
                delegatee_address,
            } => {
                assert_eq!(amount, COIN_VALUE * 3);
                assert!(!from_delegation);
                assert_eq!(record_index, Some(5));
                assert_eq!(delegatee_address, None);
            }
            _ => panic!("Expected UnfreezeTos payload"),
        }
    }

    #[test]
    fn test_serialization_unfreeze_tos_with_delegatee() {
        // Test with record_index and delegatee_address
        let delegatee = create_test_pubkey();
        let payload = EnergyPayload::UnfreezeTos {
            amount: COIN_VALUE * 2,
            from_delegation: true,
            record_index: Some(0),
            delegatee_address: Some(delegatee.clone()),
        };

        let mut bytes = Vec::new();
        let mut writer = crate::serializer::Writer::new(&mut bytes);
        payload.write(&mut writer);

        let mut reader = crate::serializer::Reader::new(&bytes);
        let deserialized = EnergyPayload::read(&mut reader).unwrap();

        match deserialized {
            EnergyPayload::UnfreezeTos {
                amount,
                from_delegation,
                record_index,
                delegatee_address,
            } => {
                assert_eq!(amount, COIN_VALUE * 2);
                assert!(from_delegation);
                assert_eq!(record_index, Some(0));
                assert_eq!(delegatee_address, Some(delegatee));
            }
            _ => panic!("Expected UnfreezeTos payload"),
        }
    }

    #[test]
    fn test_serialization_withdraw_unfrozen() {
        let payload = EnergyPayload::WithdrawUnfrozen;

        let mut bytes = Vec::new();
        let mut writer = crate::serializer::Writer::new(&mut bytes);
        payload.write(&mut writer);

        let mut reader = crate::serializer::Reader::new(&bytes);
        let deserialized = EnergyPayload::read(&mut reader).unwrap();

        assert!(matches!(deserialized, EnergyPayload::WithdrawUnfrozen));
    }

    #[test]
    fn test_different_duration_rewards() {
        let amounts = [COIN_VALUE, 2 * COIN_VALUE, 3 * COIN_VALUE]; // 1, 2, 3 TOS
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

                let expected_energy = (amount / COIN_VALUE)
                    .checked_mul(duration.reward_multiplier())
                    .expect("energy overflow");
                assert_eq!(payload.calculate_energy_gain(), Some(expected_energy));
            }
        }
    }
}
