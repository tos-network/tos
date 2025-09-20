use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};
use tos_vm::ValueCell;
use crate::{
    api::DataElement,
    crypto::{Address, Hash},
    account::FreezeDuration,
};

fn default_bool_true() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TransferBuilder {
    pub asset: Hash,
    pub amount: u64,
    pub destination: Address,
    // we can put whatever we want up to EXTRA_DATA_LIMIT_SIZE bytes
    pub extra_data: Option<DataElement>,
    // Encrypt the extra data by default
    // Set to false if you want to keep it public
    #[serde(default = "default_bool_true")]
    pub encrypt_extra_data: bool
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MultiSigBuilder {
    pub participants: IndexSet<Address>,
    pub threshold: u8,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ContractDepositBuilder {
    pub amount: u64,
    #[serde(default)]
    pub private: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct InvokeContractBuilder {
    pub contract: Hash,
    pub max_gas: u64,
    pub chunk_id: u16,
    pub parameters: Vec<ValueCell>,
    #[serde(default)]
    pub deposits: IndexMap<Hash, ContractDepositBuilder>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DeployContractBuilder {
    // Module to deploy
    pub module: String,
    // Inner invoke during the deploy
    pub invoke: Option<DeployContractInvokeBuilder>
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DeployContractInvokeBuilder {
    pub max_gas: u64,
    #[serde(default)]
    pub deposits: IndexMap<Hash, ContractDepositBuilder>,
}

/// Builder for energy-related transactions (FreezeTos/UnfreezeTos)
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EnergyBuilder {
    /// Amount of TOS to freeze or unfreeze
    pub amount: u64,
    /// Whether this is a freeze operation (true) or unfreeze operation (false)
    pub is_freeze: bool,
    /// Freeze duration for freeze operations (3, 7, or 14 days)
    /// This affects the reward multiplier: 1.0x, 1.1x, or 1.2x respectively
    /// Only used when is_freeze is true
    #[serde(default)]
    pub freeze_duration: Option<FreezeDuration>,
}

impl EnergyBuilder {
    /// Create a new freeze TOS builder with specified duration
    pub fn freeze_tos(amount: u64, duration: FreezeDuration) -> Self {
        Self {
            amount,
            is_freeze: true,
            freeze_duration: Some(duration),
        }
    }

    /// Create a new unfreeze TOS builder
    pub fn unfreeze_tos(amount: u64) -> Self {
        Self {
            amount,
            is_freeze: false,
            freeze_duration: None,
        }
    }

    /// Get the freeze duration for this operation
    pub fn get_duration(&self) -> Option<&FreezeDuration> {
        self.freeze_duration.as_ref()
    }

    /// Calculate the energy that would be gained from this freeze operation
    pub fn calculate_energy_gain(&self) -> Option<u64> {
        if self.is_freeze {
            self.freeze_duration.as_ref().map(|duration| {
                (self.amount / crate::config::COIN_VALUE) * duration.reward_multiplier()
            })
        } else {
            None
        }
    }

    /// Validate the builder configuration
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.amount == 0 {
            return Err("Amount must be greater than 0");
        }

        // Check minimum freeze amount (1 TOS) and ensure whole TOS amounts
        if self.is_freeze {
            if self.amount < crate::config::MIN_FREEZE_TOS_AMOUNT {
                return Err("Minimum freeze amount is 1 TOS");
            }
            
            // Check if amount is a whole number of TOS (no decimals)
            if self.amount % crate::config::COIN_VALUE != 0 {
                return Err("Freeze amount must be a whole number of TOS (no decimals)");
            }
            
            if self.freeze_duration.is_none() {
                return Err("Freeze duration must be specified for freeze operations");
            }
            
            // Validate freeze duration (3-180 days)
            if let Some(duration) = &self.freeze_duration {
                if !duration.is_valid() {
                    return Err("Freeze duration must be between 3 and 180 days");
                }
            }
        } else {
            // Check if unfreeze amount is a whole number of TOS (no decimals)
            if self.amount % crate::config::COIN_VALUE != 0 {
                return Err("Unfreeze amount must be a whole number of TOS (no decimals)");
            }
            
            // Check minimum unfreeze amount (1 TOS)
            if self.amount < crate::config::MIN_UNFREEZE_TOS_AMOUNT {
                return Err("Minimum unfreeze amount is 1 TOS");
            }
            
            if self.freeze_duration.is_some() {
                return Err("Freeze duration should not be specified for unfreeze operations");
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::COIN_VALUE;

    #[test]
    fn test_energy_builder_freeze() {
        let duration = FreezeDuration::new(7).unwrap();
        let builder = EnergyBuilder::freeze_tos(100000000, duration); // 1 TOS
        
        assert_eq!(builder.amount, 100000000);
        assert!(builder.is_freeze);
        assert_eq!(builder.get_duration(), Some(&duration));
        assert_eq!(builder.calculate_energy_gain(), Some(14)); // 1 TOS * 14 = 14 transfers
        assert!(builder.validate().is_ok());
    }

    #[test]
    fn test_energy_builder_unfreeze() {
        let builder = EnergyBuilder::unfreeze_tos(100000000); // 1 TOS
        
        assert_eq!(builder.amount, 100000000);
        assert!(!builder.is_freeze);
        assert_eq!(builder.get_duration(), None);
        assert_eq!(builder.calculate_energy_gain(), None);
        assert!(builder.validate().is_ok());
    }

    #[test]
    fn test_energy_builder_validation() {
        // Test zero amount
        let duration = FreezeDuration::new(3).unwrap();
        let builder = EnergyBuilder::freeze_tos(0, duration);
        assert!(builder.validate().is_err());

        // Test minimum freeze amount (less than 1 TOS)
        let duration = FreezeDuration::new(3).unwrap();
        let builder = EnergyBuilder::freeze_tos(50000000, duration); // 0.5 TOS
        assert!(builder.validate().is_err());

        // Test freeze with decimal amount (1.5 TOS)
        let duration = FreezeDuration::new(3).unwrap();
        let builder = EnergyBuilder::freeze_tos(150000000, duration); // 1.5 TOS
        assert!(builder.validate().is_err());

        // Test freeze with decimal amount (1.1 TOS)
        let duration = FreezeDuration::new(3).unwrap();
        let builder = EnergyBuilder::freeze_tos(110000000, duration); // 1.1 TOS
        assert!(builder.validate().is_err());

        // Test freeze without duration
        let builder = EnergyBuilder {
            amount: 1000,
            is_freeze: true,
            freeze_duration: None,
        };
        assert!(builder.validate().is_err());

        // Test freeze with invalid duration (less than 3 days)
        let builder = EnergyBuilder {
            amount: 100000000,
            is_freeze: true,
            freeze_duration: Some(FreezeDuration { days: 2 }),
        };
        assert!(builder.validate().is_err());

        // Test freeze with invalid duration (more than 180 days)
        let builder = EnergyBuilder {
            amount: 100000000,
            is_freeze: true,
            freeze_duration: Some(FreezeDuration { days: 181 }),
        };
        assert!(builder.validate().is_err());

        // Test unfreeze with duration
        let duration = FreezeDuration::new(7).unwrap();
        let builder = EnergyBuilder {
            amount: 1000,
            is_freeze: false,
            freeze_duration: Some(duration),
        };
        assert!(builder.validate().is_err());

        // Test unfreeze with decimal amount (1.5 TOS)
        let builder = EnergyBuilder::unfreeze_tos(150000000); // 1.5 TOS
        assert!(builder.validate().is_err());

        // Test unfreeze with decimal amount (1.1 TOS)
        let builder = EnergyBuilder::unfreeze_tos(110000000); // 1.1 TOS
        assert!(builder.validate().is_err());
    }

    #[test]
    fn test_different_duration_rewards() {
        let amounts = [100000000, 200000000, 300000000]; // 1, 2, 3 TOS
        let durations = [FreezeDuration::new(3).unwrap(), FreezeDuration::new(7).unwrap(), FreezeDuration::new(14).unwrap()];
        
        for amount in amounts {
            for duration in &durations {
                let builder = EnergyBuilder::freeze_tos(amount, duration.clone());
                let expected_energy = (amount / COIN_VALUE) * duration.reward_multiplier();
                assert_eq!(builder.calculate_energy_gain(), Some(expected_energy));
            }
        }
    }

    #[test]
    fn test_minimum_freeze_amount_boundary() {
        let duration = FreezeDuration::new(3).unwrap();
        
        // Test exactly 1 TOS (should pass)
        let builder = EnergyBuilder::freeze_tos(COIN_VALUE, duration.clone());
        assert!(builder.validate().is_ok());
        
        // Test slightly less than 1 TOS (should fail)
        let builder = EnergyBuilder::freeze_tos(COIN_VALUE - 1, duration.clone());
        assert!(builder.validate().is_err());
        
        // Test 0.5 TOS (should fail)
        let builder = EnergyBuilder::freeze_tos(COIN_VALUE / 2, duration.clone());
        assert!(builder.validate().is_err());
        
        // Test 2 TOS (should pass)
        let builder = EnergyBuilder::freeze_tos(COIN_VALUE * 2, duration.clone());
        assert!(builder.validate().is_ok());
    }

    #[test]
    fn test_whole_tos_amount_validation() {
        let duration = FreezeDuration::new(3).unwrap();
        
        // Test valid whole TOS amounts for freeze
        let valid_amounts = [COIN_VALUE, COIN_VALUE * 2, COIN_VALUE * 3, COIN_VALUE * 10]; // 1, 2, 3, 10 TOS
        for amount in valid_amounts {
            let builder = EnergyBuilder::freeze_tos(amount, duration.clone());
            assert!(builder.validate().is_ok(), "Freeze amount {} should be valid", amount);
        }
        
        // Test invalid decimal amounts for freeze
        let invalid_amounts = [
            COIN_VALUE + COIN_VALUE / 2, // 1.5 TOS
            COIN_VALUE + COIN_VALUE / 10, // 1.1 TOS
            COIN_VALUE * 2 + COIN_VALUE / 2, // 2.5 TOS
            COIN_VALUE + 1, // 1.00000001 TOS
            COIN_VALUE * 2 - 1, // 1.99999999 TOS
        ];
        for amount in invalid_amounts {
            let builder = EnergyBuilder::freeze_tos(amount, duration.clone());
            assert!(builder.validate().is_err(), "Freeze amount {} should be invalid", amount);
        }
    }

    #[test]
    fn test_unfreeze_whole_tos_validation() {
        // Test valid whole TOS amounts for unfreeze
        let valid_amounts = [COIN_VALUE, COIN_VALUE * 2, COIN_VALUE * 3, COIN_VALUE * 10]; // 1, 2, 3, 10 TOS
        for amount in valid_amounts {
            let builder = EnergyBuilder::unfreeze_tos(amount);
            assert!(builder.validate().is_ok(), "Unfreeze amount {} should be valid", amount);
        }
        
        // Test invalid decimal amounts for unfreeze
        let invalid_amounts = [
            COIN_VALUE + COIN_VALUE / 2, // 1.5 TOS
            COIN_VALUE + COIN_VALUE / 10, // 1.1 TOS
            COIN_VALUE * 2 + COIN_VALUE / 2, // 2.5 TOS
            COIN_VALUE + 1, // 1.00000001 TOS
            COIN_VALUE * 2 - 1, // 1.99999999 TOS
        ];
        for amount in invalid_amounts {
            let builder = EnergyBuilder::unfreeze_tos(amount);
            assert!(builder.validate().is_err(), "Unfreeze amount {} should be invalid", amount);
        }
    }

    #[test]
    fn test_freeze_duration_validation() {
        // Test valid freeze durations
        let valid_durations = [3, 7, 14, 30, 60, 90, 120, 150, 180]; // 3-180 days
        for days in valid_durations {
            let duration = FreezeDuration::new(days).unwrap();
            let builder = EnergyBuilder::freeze_tos(100000000, duration); // 1 TOS
            assert!(builder.validate().is_ok(), "Duration {} days should be valid", days);
        }
        
        // Test invalid freeze durations
        let invalid_durations = [1, 2, 181, 182, 365]; // Less than 3 or more than 180 days
        for days in invalid_durations {
            let duration = FreezeDuration { days };
            let builder = EnergyBuilder::freeze_tos(100000000, duration); // 1 TOS
            assert!(builder.validate().is_err(), "Duration {} days should be invalid", days);
        }
    }
}