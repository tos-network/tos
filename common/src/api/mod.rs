mod data;
pub mod wallet;
pub mod daemon;
pub mod query;

use std::borrow::Cow;
use serde::{Deserialize, Serialize};
use serde_json::Value;
// Balance simplification: bulletproofs removed
// use bulletproofs::RangeProof;
use crate::{
    account::Nonce,
    crypto::{
        Address,
        Hash,
        Signature
    },
    serializer::Serializer,
    contract::ContractOutput,
    transaction::{
        extra_data::UnknownExtraDataFormat,
        multisig::MultiSig,
        BurnPayload,
        EnergyPayload,
        InvokeContractPayload,
        DeployContractPayload,
        MultiSigPayload,
        Reference,
        Transaction,
        TransactionType,
        TransferPayload,
        TxVersion,
        FeeType,
    }
};
pub use data::*;

#[derive(Serialize, Deserialize)]
pub struct SubscribeParams<'a, E: Clone> {
    pub notify: Cow<'a, E>
}

#[derive(Serialize, Deserialize)]
pub struct EventResult<'a, E: Clone> {
    pub event: Cow<'a, E>,
    #[serde(flatten)]
    pub value: Value
}

#[derive(Serialize, Deserialize)]
pub struct DataHash<'a, T: Clone> {
    pub hash: Cow<'a, Hash>,
    #[serde(flatten)]
    pub data: Cow<'a, T>
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RPCTransferPayload<'a> {
    pub asset: Cow<'a, Hash>,
    pub destination: Address,
    pub amount: u64,  // Plaintext amount
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
    AIMining(Cow<'a, crate::ai_mining::AIMiningPayload>)
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
            },
            TransactionType::Burn(burn) => Self::Burn(Cow::Borrowed(burn)),
            TransactionType::MultiSig(payload) => Self::MultiSig(Cow::Borrowed(payload)),
            TransactionType::InvokeContract(payload) => Self::InvokeContract(Cow::Borrowed(payload)),
            TransactionType::DeployContract(payload) => Self::DeployContract(Cow::Borrowed(payload)),
            TransactionType::Energy(payload) => Self::Energy(Cow::Borrowed(payload)),
            TransactionType::AIMining(payload) => Self::AIMining(Cow::Borrowed(payload))
        }
    }
}

impl From<RPCTransactionType<'_>> for TransactionType {
    fn from(data: RPCTransactionType) -> Self {
        match data {
            RPCTransactionType::Transfers(transfers) => {
                TransactionType::Transfers(transfers.into_iter().map(|transfer| transfer.into()).collect::<Vec<TransferPayload>>())
            },
            RPCTransactionType::Burn(burn) => TransactionType::Burn(burn.into_owned()),
            RPCTransactionType::MultiSig(payload) => TransactionType::MultiSig(payload.into_owned()),
            RPCTransactionType::InvokeContract(payload) => TransactionType::InvokeContract(payload.into_owned()),
            RPCTransactionType::DeployContract(payload) => TransactionType::DeployContract(payload.into_owned()),
            RPCTransactionType::Energy(payload) => TransactionType::Energy(payload.into_owned()),
            RPCTransactionType::AIMining(payload) => TransactionType::AIMining(payload.into_owned())
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
    pub size: usize
}

impl<'a> RPCTransaction<'a> {
    pub fn from_tx(tx: &'a Transaction, hash: &'a Hash, mainnet: bool) -> Self {
        Self {
            hash: Cow::Borrowed(hash),
            version: tx.get_version(),
            source: tx.get_source().as_address(mainnet),
            data: RPCTransactionType::from_type(tx.get_data(), mainnet),
            fee: tx.get_fee(),
            nonce: tx.get_nonce(),
            reference: Cow::Borrowed(tx.get_reference()),
            multisig: Cow::Borrowed(tx.get_multisig()),
            signature: Cow::Borrowed(tx.get_signature()),
            size: tx.size()
        }
    }
}

impl<'a> From<RPCTransaction<'a>> for Transaction {
    fn from(tx: RPCTransaction<'a>) -> Self {
        Transaction::new(
            tx.version,
            tx.source.to_public_key(),
            tx.data.into(),
            tx.fee,
            FeeType::TOS,
            tx.nonce,
            tx.reference.into_owned(),
            tx.multisig.into_owned(),
            Vec::new(), // account_keys: empty, RPC transactions use T0 format
            tx.signature.into_owned()
        )
    }
}

// We create a type above it so for deserialize we can use this type directly
// and not have to specify the lifetime
pub type TransactionResponse = RPCTransaction<'static>;

#[derive(Serialize, Deserialize)]
pub struct SplitAddressParams {
    // address which must be in integrated form
    pub address: Address
}

#[derive(Serialize, Deserialize)]
pub struct SplitAddressResult {
    // Normal address
    pub address: Address,
    // Encoded data from address
    pub integrated_data: DataElement,
    // Integrated data size
    pub size: usize
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RPCContractOutput<'a> {
    RefundGas {
        amount: u64
    },
    Transfer {
        amount: u64,
        asset: Cow<'a, Hash>,
        destination: Cow<'a, Address>
    },
    Mint {
        asset: Cow<'a, Hash>,
        amount: u64
    },
    Burn {
        asset: Cow<'a, Hash>,
        amount: u64
    },
    NewAsset {
        asset: Cow<'a, Hash>
    },
    ExitCode(Option<u64>),
    RefundDeposits
}

impl<'a> RPCContractOutput<'a> {
    pub fn from_output(output: &'a ContractOutput, mainnet: bool) -> Self {
        match output {
            ContractOutput::RefundGas { amount } => RPCContractOutput::RefundGas { amount: *amount },
            ContractOutput::Transfer { amount, asset, destination } => RPCContractOutput::Transfer {
                amount: *amount,
                asset: Cow::Borrowed(asset),
                destination: Cow::Owned(destination.as_address(mainnet))
            },
            ContractOutput::Mint { asset, amount } => RPCContractOutput::Mint {
                asset: Cow::Borrowed(asset),
                amount: *amount
            },
            ContractOutput::Burn { asset, amount } => RPCContractOutput::Burn {
                asset: Cow::Borrowed(asset),
                amount: *amount
            },
            ContractOutput::NewAsset { asset } => RPCContractOutput::NewAsset {
                asset: Cow::Borrowed(asset)
            },
            ContractOutput::ExitCode(code) => RPCContractOutput::ExitCode(code.clone()),
            ContractOutput::RefundDeposits => RPCContractOutput::RefundDeposits,
        }
    }
}

impl<'a> From<RPCContractOutput<'a>> for ContractOutput {
    fn from(output: RPCContractOutput<'a>) -> Self {
        match output {
            RPCContractOutput::RefundGas { amount } => ContractOutput::RefundGas { amount },
            RPCContractOutput::Transfer { amount, asset, destination } => ContractOutput::Transfer {
                amount,
                asset: asset.into_owned(),
                destination: destination.into_owned().to_public_key()
            },
            RPCContractOutput::Mint { asset, amount } => ContractOutput::Mint {
                asset: asset.into_owned(),
                amount
            },
            RPCContractOutput::Burn { asset, amount } => ContractOutput::Burn {
                asset: asset.into_owned(),
                amount
            },
            RPCContractOutput::NewAsset { asset } => ContractOutput::NewAsset {
                asset: asset.into_owned()
            },
            RPCContractOutput::ExitCode(code) => ContractOutput::ExitCode(code),
            RPCContractOutput::RefundDeposits => ContractOutput::RefundDeposits,
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