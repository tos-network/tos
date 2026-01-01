use crate::{
    api::DataElement,
    crypto::{elgamal::CompressedPublicKey, Address, Hash},
};
use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};
use tos_kernel::ValueCell;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TransferBuilder {
    pub asset: Hash,
    pub amount: u64,
    pub destination: Address,
    // we can put whatever we want up to EXTRA_DATA_LIMIT_SIZE bytes (128 bytes for memo/exchange IDs)
    // Balance simplification: Extra data is now always plaintext (no encryption)
    pub extra_data: Option<DataElement>,
}

/// Builder for UNO (privacy-preserving) transfers
/// Similar to TransferBuilder but builds encrypted transfers with ZK proofs
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UnoTransferBuilder {
    /// Asset hash to transfer
    pub asset: Hash,
    /// Amount to transfer (will be encrypted in the final transaction)
    pub amount: u64,
    /// Destination address (public key will be visible, amount will be hidden)
    pub destination: Address,
    /// Optional memo/extra data (plaintext, up to EXTRA_DATA_LIMIT_SIZE bytes)
    pub extra_data: Option<DataElement>,
    /// Whether to encrypt the extra data (default: false for UNO transfers)
    #[serde(default)]
    pub encrypt_extra_data: bool,
}

/// Builder for Shield transfers: TOS (plaintext) -> UNO (encrypted)
/// Converts plaintext balance to encrypted UNO balance.
/// The amount is publicly visible in the transaction, but the resulting
/// UNO balance is encrypted.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ShieldTransferBuilder {
    /// Asset hash to shield (must be TOS_ASSET for Phase 7)
    pub asset: Hash,
    /// Amount to shield (publicly visible in the transaction)
    pub amount: u64,
    /// Destination address to receive the encrypted UNO balance
    /// Can be self (same as sender) or another address
    pub destination: Address,
    /// Optional memo/extra data (plaintext, up to EXTRA_DATA_LIMIT_SIZE bytes)
    pub extra_data: Option<DataElement>,
}

impl ShieldTransferBuilder {
    /// Create a new shield transfer builder
    pub fn new(asset: Hash, amount: u64, destination: Address) -> Self {
        Self {
            asset,
            amount,
            destination,
            extra_data: None,
        }
    }

    /// Create a shield transfer builder with extra data
    pub fn with_extra_data(
        asset: Hash,
        amount: u64,
        destination: Address,
        extra_data: DataElement,
    ) -> Self {
        Self {
            asset,
            amount,
            destination,
            extra_data: Some(extra_data),
        }
    }
}

/// Builder for Unshield transfers: UNO (encrypted) -> TOS (plaintext)
/// Converts encrypted UNO balance back to plaintext TOS balance.
/// The amount is revealed in the transaction (exiting privacy mode).
/// Requires ZK proof that sender has sufficient UNO balance.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UnshieldTransferBuilder {
    /// Asset hash to unshield (must be TOS_ASSET for Phase 7)
    pub asset: Hash,
    /// Amount to unshield (publicly revealed)
    pub amount: u64,
    /// Destination address to receive the plaintext TOS balance
    /// Can be self (same as sender) or another address
    pub destination: Address,
    /// Optional memo/extra data (plaintext, up to EXTRA_DATA_LIMIT_SIZE bytes)
    pub extra_data: Option<DataElement>,
}

impl UnshieldTransferBuilder {
    /// Create a new unshield transfer builder
    pub fn new(asset: Hash, amount: u64, destination: Address) -> Self {
        Self {
            asset,
            amount,
            destination,
            extra_data: None,
        }
    }

    /// Create an unshield transfer builder with extra data
    pub fn with_extra_data(
        asset: Hash,
        amount: u64,
        destination: Address,
        extra_data: DataElement,
    ) -> Self {
        Self {
            asset,
            amount,
            destination,
            extra_data: Some(extra_data),
        }
    }
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
    pub entry_id: u16,
    pub parameters: Vec<ValueCell>,
    #[serde(default)]
    pub deposits: IndexMap<Hash, ContractDepositBuilder>,
    // Contract public key for private deposits
    // When provided, enables private deposits by encrypting deposit amounts
    // The contract key is derived from the contract hash
    // Stored in compressed form for serialization, decompressed when used
    #[serde(default)]
    pub contract_key: Option<CompressedPublicKey>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DeployContractBuilder {
    // Module to deploy
    pub module: String,
    // Inner invoke during the deploy
    pub invoke: Option<DeployContractInvokeBuilder>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DeployContractInvokeBuilder {
    pub max_gas: u64,
    #[serde(default)]
    pub deposits: IndexMap<Hash, ContractDepositBuilder>,
}

/// Energy operation type for Stake 2.0
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum EnergyOperationType {
    /// Freeze TOS to gain proportional energy
    FreezeTos,
    /// Start 14-day unfreeze queue
    UnfreezeTos,
    /// Withdraw expired unfreeze entries
    WithdrawExpireUnfreeze,
    /// Cancel all pending unfreeze
    CancelAllUnfreeze,
    /// Delegate energy to another account
    DelegateResource,
    /// Undelegate energy
    UndelegateResource,
}

/// Builder for energy-related transactions (Stake 2.0)
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EnergyBuilder {
    /// Type of energy operation
    pub operation: EnergyOperationType,
    /// Amount of TOS (for freeze/unfreeze/delegate/undelegate)
    #[serde(default)]
    pub amount: Option<u64>,
    /// Receiver for delegation operations
    #[serde(default)]
    pub receiver: Option<CompressedPublicKey>,
    /// Lock delegation for a period
    #[serde(default)]
    pub lock: bool,
    /// Lock period in days (0-365)
    #[serde(default)]
    pub lock_period: u32,
}

impl EnergyBuilder {
    /// Create a new freeze TOS builder
    pub fn freeze_tos(amount: u64) -> Self {
        Self {
            operation: EnergyOperationType::FreezeTos,
            amount: Some(amount),
            receiver: None,
            lock: false,
            lock_period: 0,
        }
    }

    /// Create a new unfreeze TOS builder
    pub fn unfreeze_tos(amount: u64) -> Self {
        Self {
            operation: EnergyOperationType::UnfreezeTos,
            amount: Some(amount),
            receiver: None,
            lock: false,
            lock_period: 0,
        }
    }

    /// Create a withdraw expired unfreeze builder
    pub fn withdraw_expire_unfreeze() -> Self {
        Self {
            operation: EnergyOperationType::WithdrawExpireUnfreeze,
            amount: None,
            receiver: None,
            lock: false,
            lock_period: 0,
        }
    }

    /// Create a cancel all unfreeze builder
    pub fn cancel_all_unfreeze() -> Self {
        Self {
            operation: EnergyOperationType::CancelAllUnfreeze,
            amount: None,
            receiver: None,
            lock: false,
            lock_period: 0,
        }
    }

    /// Create a delegate resource builder
    pub fn delegate_resource(
        receiver: CompressedPublicKey,
        amount: u64,
        lock: bool,
        lock_period: u32,
    ) -> Self {
        Self {
            operation: EnergyOperationType::DelegateResource,
            amount: Some(amount),
            receiver: Some(receiver),
            lock,
            lock_period,
        }
    }

    /// Create an undelegate resource builder
    pub fn undelegate_resource(receiver: CompressedPublicKey, amount: u64) -> Self {
        Self {
            operation: EnergyOperationType::UndelegateResource,
            amount: Some(amount),
            receiver: Some(receiver),
            lock: false,
            lock_period: 0,
        }
    }

    /// Validate the builder configuration
    pub fn validate(&self) -> Result<(), &'static str> {
        match self.operation {
            EnergyOperationType::FreezeTos | EnergyOperationType::UnfreezeTos => {
                let amount = self.amount.ok_or("Amount is required")?;
                if amount == 0 {
                    return Err("Amount must be greater than 0");
                }
                if amount < crate::config::MIN_FREEZE_TOS_AMOUNT {
                    return Err("Minimum amount is 1 TOS");
                }
                if !amount.is_multiple_of(crate::config::COIN_VALUE) {
                    return Err("Amount must be a whole number of TOS");
                }
            }
            EnergyOperationType::DelegateResource | EnergyOperationType::UndelegateResource => {
                let amount = self.amount.ok_or("Amount is required")?;
                if amount == 0 {
                    return Err("Amount must be greater than 0");
                }
                if self.receiver.is_none() {
                    return Err("Receiver is required for delegation operations");
                }
                if self.operation == EnergyOperationType::DelegateResource
                    && self.lock
                    && self.lock_period > 365
                {
                    return Err("Lock period cannot exceed 365 days");
                }
            }
            EnergyOperationType::WithdrawExpireUnfreeze
            | EnergyOperationType::CancelAllUnfreeze => {
                // No additional validation needed
            }
        }
        Ok(())
    }

    /// Build the EnergyPayload from this builder
    pub fn build(&self) -> crate::transaction::payload::EnergyPayload {
        use crate::transaction::payload::EnergyPayload;

        match self.operation {
            EnergyOperationType::FreezeTos => EnergyPayload::FreezeTos {
                amount: self.amount.unwrap_or(0),
            },
            EnergyOperationType::UnfreezeTos => EnergyPayload::UnfreezeTos {
                amount: self.amount.unwrap_or(0),
            },
            EnergyOperationType::WithdrawExpireUnfreeze => EnergyPayload::WithdrawExpireUnfreeze,
            EnergyOperationType::CancelAllUnfreeze => EnergyPayload::CancelAllUnfreeze,
            EnergyOperationType::DelegateResource => {
                // SAFE: validate() ensures receiver is Some for delegate operations
                EnergyPayload::DelegateResource {
                    receiver: self.receiver.clone().expect("receiver required"),
                    amount: self.amount.unwrap_or(0),
                    lock: self.lock,
                    lock_period: self.lock_period,
                }
            }
            EnergyOperationType::UndelegateResource => {
                // SAFE: validate() ensures receiver is Some for undelegate operations
                EnergyPayload::UndelegateResource {
                    receiver: self.receiver.clone().expect("receiver required"),
                    amount: self.amount.unwrap_or(0),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::COIN_VALUE;

    #[test]
    fn test_energy_builder_freeze() {
        let builder = EnergyBuilder::freeze_tos(COIN_VALUE); // 1 TOS

        assert_eq!(builder.operation, EnergyOperationType::FreezeTos);
        assert_eq!(builder.amount, Some(COIN_VALUE));
        assert!(builder.validate().is_ok());
    }

    #[test]
    fn test_energy_builder_unfreeze() {
        let builder = EnergyBuilder::unfreeze_tos(COIN_VALUE); // 1 TOS

        assert_eq!(builder.operation, EnergyOperationType::UnfreezeTos);
        assert_eq!(builder.amount, Some(COIN_VALUE));
        assert!(builder.validate().is_ok());
    }

    #[test]
    fn test_energy_builder_withdraw_expire() {
        let builder = EnergyBuilder::withdraw_expire_unfreeze();

        assert_eq!(
            builder.operation,
            EnergyOperationType::WithdrawExpireUnfreeze
        );
        assert!(builder.amount.is_none());
        assert!(builder.validate().is_ok());
    }

    #[test]
    fn test_energy_builder_cancel_all() {
        let builder = EnergyBuilder::cancel_all_unfreeze();

        assert_eq!(builder.operation, EnergyOperationType::CancelAllUnfreeze);
        assert!(builder.amount.is_none());
        assert!(builder.validate().is_ok());
    }

    #[test]
    fn test_energy_builder_validation() {
        // Test zero amount for freeze
        let mut builder = EnergyBuilder::freeze_tos(0);
        assert!(builder.validate().is_err());

        // Test minimum freeze amount (less than 1 TOS)
        builder = EnergyBuilder::freeze_tos(COIN_VALUE / 2);
        assert!(builder.validate().is_err());

        // Test freeze with decimal amount (1.5 TOS)
        builder = EnergyBuilder::freeze_tos(COIN_VALUE + COIN_VALUE / 2);
        assert!(builder.validate().is_err());

        // Test valid whole TOS amounts
        let valid_amounts = [COIN_VALUE, COIN_VALUE * 2, COIN_VALUE * 10];
        for amount in valid_amounts {
            let builder = EnergyBuilder::freeze_tos(amount);
            assert!(
                builder.validate().is_ok(),
                "Amount {amount} should be valid"
            );
        }

        // Test unfreeze validation
        let builder = EnergyBuilder::unfreeze_tos(COIN_VALUE + 1); // 1.00000001 TOS
        assert!(builder.validate().is_err());
    }

    // UnoTransferBuilder tests

    use crate::crypto::{elgamal::KeyPair, Address, AddressType};

    fn create_test_address() -> Address {
        let keypair = KeyPair::new();
        Address::new(
            false,
            AddressType::Normal,
            keypair.get_public_key().compress(),
        )
    }

    #[test]
    fn test_uno_transfer_builder_creation() {
        use crate::config::UNO_ASSET;

        let builder = UnoTransferBuilder {
            asset: UNO_ASSET,
            amount: 1000,
            destination: create_test_address(),
            extra_data: None,
            encrypt_extra_data: false,
        };

        assert_eq!(builder.asset, UNO_ASSET);
        assert_eq!(builder.amount, 1000);
        assert!(!builder.encrypt_extra_data);
        assert!(builder.extra_data.is_none());
    }

    #[test]
    fn test_uno_transfer_builder_with_extra_data() {
        use crate::api::DataValue;
        use crate::config::UNO_ASSET;

        let memo = DataElement::Value(DataValue::Blob(vec![1, 2, 3, 4]));
        let builder = UnoTransferBuilder {
            asset: UNO_ASSET,
            amount: 5000,
            destination: create_test_address(),
            extra_data: Some(memo.clone()),
            encrypt_extra_data: false,
        };

        assert_eq!(builder.amount, 5000);
        assert!(builder.extra_data.is_some());
        assert!(!builder.encrypt_extra_data);
    }

    #[test]
    fn test_uno_transfer_builder_serialization() {
        use crate::config::UNO_ASSET;

        let builder = UnoTransferBuilder {
            asset: UNO_ASSET,
            amount: 12345,
            destination: create_test_address(),
            extra_data: None,
            encrypt_extra_data: true,
        };

        // Test serde serialization roundtrip
        let json = serde_json::to_string(&builder).unwrap();
        let restored: UnoTransferBuilder = serde_json::from_str(&json).unwrap();

        assert_eq!(builder.asset, restored.asset);
        assert_eq!(builder.amount, restored.amount);
        assert_eq!(builder.destination, restored.destination);
        assert_eq!(builder.encrypt_extra_data, restored.encrypt_extra_data);
    }

    #[test]
    fn test_uno_transfer_builder_different_from_transfer() {
        use crate::config::{TOS_ASSET, UNO_ASSET};

        let dest = create_test_address();

        // UnoTransferBuilder for privacy transfers
        let uno_builder = UnoTransferBuilder {
            asset: UNO_ASSET,
            amount: 1000,
            destination: dest.clone(),
            extra_data: None,
            encrypt_extra_data: false,
        };

        // Regular TransferBuilder for plaintext transfers
        let transfer_builder = TransferBuilder {
            asset: TOS_ASSET,
            amount: 1000,
            destination: dest,
            extra_data: None,
        };

        // Different asset types
        assert_ne!(uno_builder.asset, transfer_builder.asset);
        assert_eq!(uno_builder.asset, UNO_ASSET);
        assert_eq!(transfer_builder.asset, TOS_ASSET);
    }

    #[test]
    fn test_uno_transfer_builder_zero_amount() {
        use crate::config::UNO_ASSET;

        // Zero amount transfer - allowed at builder level, rejected at verification
        let builder = UnoTransferBuilder {
            asset: UNO_ASSET,
            amount: 0,
            destination: create_test_address(),
            extra_data: None,
            encrypt_extra_data: false,
        };

        assert_eq!(builder.amount, 0);
    }

    #[test]
    fn test_uno_transfer_builder_max_amount() {
        use crate::config::UNO_ASSET;

        // Maximum amount transfer
        let builder = UnoTransferBuilder {
            asset: UNO_ASSET,
            amount: u64::MAX,
            destination: create_test_address(),
            extra_data: None,
            encrypt_extra_data: false,
        };

        assert_eq!(builder.amount, u64::MAX);
    }

    // ShieldTransferBuilder tests

    #[test]
    fn test_shield_transfer_builder_creation() {
        use crate::config::TOS_ASSET;

        let dest = create_test_address();
        let builder = ShieldTransferBuilder::new(TOS_ASSET, 1000, dest.clone());

        assert_eq!(builder.asset, TOS_ASSET);
        assert_eq!(builder.amount, 1000);
        assert_eq!(builder.destination, dest);
        assert!(builder.extra_data.is_none());
    }

    #[test]
    fn test_shield_transfer_builder_with_extra_data() {
        use crate::api::DataValue;
        use crate::config::TOS_ASSET;

        let dest = create_test_address();
        let memo = DataElement::Value(DataValue::Blob(vec![1, 2, 3, 4]));
        let builder =
            ShieldTransferBuilder::with_extra_data(TOS_ASSET, 5000, dest.clone(), memo.clone());

        assert_eq!(builder.asset, TOS_ASSET);
        assert_eq!(builder.amount, 5000);
        assert_eq!(builder.destination, dest);
        assert!(builder.extra_data.is_some());
    }

    #[test]
    fn test_shield_transfer_builder_serialization() {
        use crate::config::TOS_ASSET;

        let builder = ShieldTransferBuilder::new(TOS_ASSET, 12345, create_test_address());

        // Test serde serialization roundtrip
        let json = serde_json::to_string(&builder).unwrap();
        let restored: ShieldTransferBuilder = serde_json::from_str(&json).unwrap();

        assert_eq!(builder.asset, restored.asset);
        assert_eq!(builder.amount, restored.amount);
        assert_eq!(builder.destination, restored.destination);
    }

    // UnshieldTransferBuilder tests

    #[test]
    fn test_unshield_transfer_builder_creation() {
        use crate::config::TOS_ASSET;

        let dest = create_test_address();
        let builder = UnshieldTransferBuilder::new(TOS_ASSET, 2000, dest.clone());

        assert_eq!(builder.asset, TOS_ASSET);
        assert_eq!(builder.amount, 2000);
        assert_eq!(builder.destination, dest);
        assert!(builder.extra_data.is_none());
    }

    #[test]
    fn test_unshield_transfer_builder_with_extra_data() {
        use crate::api::DataValue;
        use crate::config::TOS_ASSET;

        let dest = create_test_address();
        let memo = DataElement::Value(DataValue::Blob(vec![5, 6, 7, 8]));
        let builder =
            UnshieldTransferBuilder::with_extra_data(TOS_ASSET, 7500, dest.clone(), memo.clone());

        assert_eq!(builder.asset, TOS_ASSET);
        assert_eq!(builder.amount, 7500);
        assert_eq!(builder.destination, dest);
        assert!(builder.extra_data.is_some());
    }

    #[test]
    fn test_unshield_transfer_builder_serialization() {
        use crate::config::TOS_ASSET;

        let builder = UnshieldTransferBuilder::new(TOS_ASSET, 54321, create_test_address());

        // Test serde serialization roundtrip
        let json = serde_json::to_string(&builder).unwrap();
        let restored: UnshieldTransferBuilder = serde_json::from_str(&json).unwrap();

        assert_eq!(builder.asset, restored.asset);
        assert_eq!(builder.amount, restored.amount);
        assert_eq!(builder.destination, restored.destination);
    }

    #[test]
    fn test_shield_unshield_builders_different() {
        use crate::config::TOS_ASSET;

        let dest = create_test_address();
        let amount = 1000u64;

        let shield_builder = ShieldTransferBuilder::new(TOS_ASSET, amount, dest.clone());
        let unshield_builder = UnshieldTransferBuilder::new(TOS_ASSET, amount, dest.clone());

        // Both should have same values but different types
        assert_eq!(shield_builder.asset, unshield_builder.asset);
        assert_eq!(shield_builder.amount, unshield_builder.amount);
        assert_eq!(shield_builder.destination, unshield_builder.destination);
    }
}
