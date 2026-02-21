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
