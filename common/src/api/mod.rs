pub mod callback;
pub mod daemon;
mod data;
pub mod payment;
pub mod query;
pub mod wallet;

use log::warn;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::borrow::Cow;
// Balance simplification: bulletproofs removed
// use bulletproofs::RangeProof;
use crate::{
    account::Nonce,
    contract::ContractOutput,
    crypto::{Address, Hash, Signature},
    serializer::Serializer,
    transaction::{
        extra_data::UnknownExtraDataFormat, multisig::MultiSig, AgentAccountPayload, BurnPayload,
        DeployContractPayload, EnergyPayload, FeeType, InvokeContractPayload, MultiSigPayload,
        Reference, RegisterNamePayload, ShieldTransferPayload, Transaction, TransactionType,
        TransferPayload, TxVersion, UnoTransferPayload, UnshieldTransferPayload,
    },
};
pub use data::*;

#[derive(Serialize, Deserialize)]
pub struct SubscribeParams<'a, E: Clone> {
    pub notify: Cow<'a, E>,
}

#[derive(Serialize, Deserialize)]
pub struct EventResult<'a, E: Clone> {
    pub event: Cow<'a, E>,
    #[serde(flatten)]
    pub value: Value,
}

#[derive(Serialize, Deserialize)]
pub struct DataHash<'a, T: Clone> {
    pub hash: Cow<'a, Hash>,
    #[serde(flatten)]
    pub data: Cow<'a, T>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RPCTransferPayload<'a> {
    pub asset: Cow<'a, Hash>,
    pub destination: Address,
    pub amount: u64, // Plaintext amount
    pub extra_data: Cow<'a, Option<UnknownExtraDataFormat>>,
}

impl<'a> From<RPCTransferPayload<'a>> for TransferPayload {
    fn from(transfer: RPCTransferPayload<'a>) -> Self {
        TransferPayload::new(
            transfer.asset.into_owned(),
            transfer.destination.to_public_key(),
            transfer.amount,
            transfer.extra_data.into_owned(),
        )
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum RPCTransactionType<'a> {
    Transfers(Vec<RPCTransferPayload<'a>>),
    Burn(Cow<'a, BurnPayload>),
    MultiSig(Cow<'a, MultiSigPayload>),
    InvokeContract(Cow<'a, InvokeContractPayload>),
    DeployContract(Cow<'a, DeployContractPayload>),
    Energy(Cow<'a, EnergyPayload>),
    AgentAccount(Cow<'a, AgentAccountPayload>),
    // UNO (Privacy Balance) transaction types
    UnoTransfers(Cow<'a, Vec<UnoTransferPayload>>),
    ShieldTransfers(Cow<'a, Vec<ShieldTransferPayload>>),
    UnshieldTransfers(Cow<'a, Vec<UnshieldTransferPayload>>),
    // TNS (TOS Name Service) transaction types
    RegisterName(Cow<'a, RegisterNamePayload>),
}

impl<'a> RPCTransactionType<'a> {
    pub fn from_type(data: &'a TransactionType, mainnet: bool) -> Self {
        match data {
            TransactionType::Transfers(transfers) => {
                let mut rpc_transfers = Vec::new();
                for transfer in transfers {
                    rpc_transfers.push(RPCTransferPayload {
                        asset: Cow::Borrowed(transfer.get_asset()),
                        destination: transfer.get_destination().as_address(mainnet),
                        amount: transfer.get_amount(),
                        extra_data: Cow::Borrowed(transfer.get_extra_data()),
                    });
                }
                Self::Transfers(rpc_transfers)
            }
            TransactionType::Burn(burn) => Self::Burn(Cow::Borrowed(burn)),
            TransactionType::MultiSig(payload) => Self::MultiSig(Cow::Borrowed(payload)),
            TransactionType::InvokeContract(payload) => {
                Self::InvokeContract(Cow::Borrowed(payload))
            }
            TransactionType::DeployContract(payload) => {
                Self::DeployContract(Cow::Borrowed(payload))
            }
            TransactionType::Energy(payload) => Self::Energy(Cow::Borrowed(payload)),
            TransactionType::AgentAccount(payload) => Self::AgentAccount(Cow::Borrowed(payload)),
            TransactionType::UnoTransfers(transfers) => {
                Self::UnoTransfers(Cow::Borrowed(transfers))
            }
            TransactionType::ShieldTransfers(transfers) => {
                Self::ShieldTransfers(Cow::Borrowed(transfers))
            }
            TransactionType::UnshieldTransfers(transfers) => {
                Self::UnshieldTransfers(Cow::Borrowed(transfers))
            }
            TransactionType::RegisterName(payload) => Self::RegisterName(Cow::Borrowed(payload)),
        }
    }
}

impl From<RPCTransactionType<'_>> for TransactionType {
    fn from(data: RPCTransactionType) -> Self {
        match data {
            RPCTransactionType::Transfers(transfers) => TransactionType::Transfers(
                transfers
                    .into_iter()
                    .map(|transfer| transfer.into())
                    .collect::<Vec<TransferPayload>>(),
            ),
            RPCTransactionType::Burn(burn) => TransactionType::Burn(burn.into_owned()),
            RPCTransactionType::MultiSig(payload) => {
                TransactionType::MultiSig(payload.into_owned())
            }
            RPCTransactionType::InvokeContract(payload) => {
                TransactionType::InvokeContract(payload.into_owned())
            }
            RPCTransactionType::DeployContract(payload) => {
                TransactionType::DeployContract(payload.into_owned())
            }
            RPCTransactionType::Energy(payload) => TransactionType::Energy(payload.into_owned()),
            RPCTransactionType::AgentAccount(payload) => {
                TransactionType::AgentAccount(payload.into_owned())
            }
            RPCTransactionType::UnoTransfers(transfers) => {
                TransactionType::UnoTransfers(transfers.into_owned())
            }
            RPCTransactionType::ShieldTransfers(transfers) => {
                TransactionType::ShieldTransfers(transfers.into_owned())
            }
            RPCTransactionType::UnshieldTransfers(transfers) => {
                TransactionType::UnshieldTransfers(transfers.into_owned())
            }
            RPCTransactionType::RegisterName(payload) => {
                TransactionType::RegisterName(payload.into_owned())
            }
        }
    }
}

// This is exactly the same as the one in tos_common/src/transaction/mod.rs
// We use this one for serde (de)serialization
// So we have addresses displayed as strings and not Public Key as bytes
// This is much more easier for developers relying on the API
#[derive(Serialize, Deserialize, Clone)]
pub struct RPCTransaction<'a> {
    pub hash: Cow<'a, Hash>,
    /// Version of the transaction
    pub version: TxVersion,
    /// Chain ID for cross-network replay protection (T1+)
    pub chain_id: u8,
    // Source of the transaction
    pub source: Address,
    /// Type of the transaction
    pub data: RPCTransactionType<'a>,
    /// Fees in TOS
    pub fee: u64,
    /// nonce must be equal to the one on chain account
    /// used to prevent replay attacks and have ordered transactions
    pub nonce: Nonce,
    /// Reference at which block the transaction was built
    pub reference: Cow<'a, Reference>,
    /// Multisig data if the transaction is a multisig transaction
    pub multisig: Cow<'a, Option<MultiSig>>,
    /// Signature of the transaction
    pub signature: Cow<'a, Signature>,
    /// TX size in bytes
    pub size: usize,
}

impl<'a> RPCTransaction<'a> {
    pub fn from_tx(tx: &'a Transaction, hash: &'a Hash, mainnet: bool) -> Self {
        Self {
            hash: Cow::Borrowed(hash),
            version: tx.get_version(),
            chain_id: tx.get_chain_id(),
            source: tx.get_source().as_address(mainnet),
            data: RPCTransactionType::from_type(tx.get_data(), mainnet),
            fee: tx.get_fee(),
            nonce: tx.get_nonce(),
            reference: Cow::Borrowed(tx.get_reference()),
            multisig: Cow::Borrowed(tx.get_multisig()),
            signature: Cow::Borrowed(tx.get_signature()),
            size: tx.size(),
        }
    }
}

impl<'a> From<RPCTransaction<'a>> for Transaction {
    fn from(tx: RPCTransaction<'a>) -> Self {
        Transaction::new(
            tx.version,
            tx.chain_id,
            tx.source.to_public_key(),
            tx.data.into(),
            tx.fee,
            FeeType::TOS,
            tx.nonce,
            tx.reference.into_owned(),
            tx.multisig.into_owned(),
            tx.signature.into_owned(),
        )
    }
}

// We create a type above it so for deserialize we can use this type directly
// and not have to specify the lifetime
pub type TransactionResponse = RPCTransaction<'static>;

#[derive(Serialize, Deserialize)]
pub struct SplitAddressParams {
    // address which must be in integrated form
    pub address: Address,
}

#[derive(Serialize, Deserialize)]
pub struct SplitAddressResult {
    // Normal address
    pub address: Address,
    // Encoded data from address
    pub integrated_data: DataElement,
    // Integrated data size
    pub size: usize,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RPCContractOutput<'a> {
    RefundGas {
        amount: u64,
    },
    Transfer {
        amount: u64,
        asset: Cow<'a, Hash>,
        destination: Cow<'a, Address>,
    },
    Mint {
        asset: Cow<'a, Hash>,
        amount: u64,
    },
    Burn {
        asset: Cow<'a, Hash>,
        amount: u64,
    },
    NewAsset {
        asset: Cow<'a, Hash>,
    },
    ExitCode(Option<u64>),
    RefundDeposits,
    ReturnData {
        /// Hex-encoded return data from contract execution
        data: String,
    },
}

impl<'a> RPCContractOutput<'a> {
    pub fn from_output(output: &'a ContractOutput, mainnet: bool) -> Self {
        match output {
            ContractOutput::RefundGas { amount } => {
                RPCContractOutput::RefundGas { amount: *amount }
            }
            ContractOutput::Transfer {
                amount,
                asset,
                destination,
            } => RPCContractOutput::Transfer {
                amount: *amount,
                asset: Cow::Borrowed(asset),
                destination: Cow::Owned(destination.as_address(mainnet)),
            },
            ContractOutput::Mint { asset, amount } => RPCContractOutput::Mint {
                asset: Cow::Borrowed(asset),
                amount: *amount,
            },
            ContractOutput::Burn { asset, amount } => RPCContractOutput::Burn {
                asset: Cow::Borrowed(asset),
                amount: *amount,
            },
            ContractOutput::NewAsset { asset } => RPCContractOutput::NewAsset {
                asset: Cow::Borrowed(asset),
            },
            ContractOutput::ExitCode(code) => RPCContractOutput::ExitCode(*code),
            ContractOutput::RefundDeposits => RPCContractOutput::RefundDeposits,
            ContractOutput::ReturnData { data } => RPCContractOutput::ReturnData {
                data: hex::encode(data),
            },
        }
    }
}

impl<'a> From<RPCContractOutput<'a>> for ContractOutput {
    fn from(output: RPCContractOutput<'a>) -> Self {
        match output {
            RPCContractOutput::RefundGas { amount } => ContractOutput::RefundGas { amount },
            RPCContractOutput::Transfer {
                amount,
                asset,
                destination,
            } => ContractOutput::Transfer {
                amount,
                asset: asset.into_owned(),
                destination: destination.into_owned().to_public_key(),
            },
            RPCContractOutput::Mint { asset, amount } => ContractOutput::Mint {
                asset: asset.into_owned(),
                amount,
            },
            RPCContractOutput::Burn { asset, amount } => ContractOutput::Burn {
                asset: asset.into_owned(),
                amount,
            },
            RPCContractOutput::NewAsset { asset } => ContractOutput::NewAsset {
                asset: asset.into_owned(),
            },
            RPCContractOutput::ExitCode(code) => ContractOutput::ExitCode(code),
            RPCContractOutput::RefundDeposits => ContractOutput::RefundDeposits,
            RPCContractOutput::ReturnData { data } => ContractOutput::ReturnData {
                // Hex-decode the string back to bytes; invalid hex is treated as empty
                data: hex::decode(&data).unwrap_or_else(|e| {
                    if log::log_enabled!(log::Level::Warn) {
                        warn!("Invalid hex in RPCContractOutput::ReturnData: {e}");
                    }
                    Vec::new()
                }),
            },
        }
    }
}

// :(
// We are forced to create function for the default value path requested by serde
fn default_true_value() -> bool {
    true
}

// same here
fn default_false_value() -> bool {
    false
}
