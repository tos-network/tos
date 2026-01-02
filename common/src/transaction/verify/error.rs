use anyhow::Error as AnyError;
use thiserror::Error;

use crate::{
    account::Nonce,
    crypto::{proofs::ProofVerificationError, Hash},
};

#[derive(Error, Debug)]
pub enum VerificationError<T> {
    #[error("State error: {0}")]
    State(T),
    #[error("Invalid TX {} nonce, got {} expected {}", _0, _1, _2)]
    InvalidNonce(Hash, Nonce, Nonce),
    #[error("Sender is receiver")]
    SenderIsReceiver,
    #[error("Invalid signature")]
    InvalidSignature,
    #[error("Invalid chain ID: expected {expected}, got {got}")]
    InvalidChainId { expected: u8, got: u8 },
    #[error("Proof verification error: {0}")]
    Proof(#[from] ProofVerificationError),
    #[error("Extra Data is too big in transfer")]
    TransferExtraDataSize,
    #[error("Extra Data is too big in transaction")]
    TransactionExtraDataSize,
    #[error("Transfer count is invalid")]
    TransferCount,
    #[error("Deposit count is invalid")]
    DepositCount,
    #[error("Invalid commitments assets")]
    Commitments,
    #[error("Invalid multisig participants count")]
    MultiSigParticipants,
    #[error("Invalid multisig threshold")]
    MultiSigThreshold,
    #[error("MultiSig not configured")]
    MultiSigNotConfigured,
    #[error("MultiSig not found")]
    MultiSigNotFound,
    #[error("Invalid format")]
    InvalidFormat,
    #[error("Module error: {0}")]
    ModuleError(String),
    #[error(transparent)]
    AnyError(#[from] AnyError),
    #[error("Invalid invoke contract")]
    InvalidInvokeContract,
    #[error("overflow during gas calculation")]
    GasOverflow,
    #[error("Deposit decompressed not found")]
    DepositNotFound,
    #[error("Configured max gas is above the network limit")]
    MaxGasReached,
    #[error("Contract not found")]
    ContractNotFound,
    #[error("Contract already exists at address {0}")]
    ContractAlreadyExists(Hash),
    #[error("Insufficient energy: required {0}")]
    InsufficientEnergy(u64),
    #[error("Insufficient funds: available {available}, required {required}")]
    InsufficientFunds { available: u64, required: u64 },
    #[error("Arithmetic overflow during balance calculation")]
    Overflow,
    #[error("Arithmetic underflow during balance calculation")]
    Underflow,
    #[error("Invalid transfer amount")]
    InvalidTransferAmount,
    // Stake 2.0 Delegation errors
    #[error("Insufficient frozen balance for delegation")]
    InsufficientFrozenBalance,
    #[error("Delegation not found")]
    DelegationNotFound,
    #[error("Delegation is still locked")]
    DelegationStillLocked,
    #[error("Insufficient delegated balance")]
    InsufficientDelegatedBalance,
    // TOS-Only Fee errors
    #[error("Transfer amount too small for account creation: amount {amount}, fee required {fee}")]
    AmountTooSmallForAccountCreation { amount: u64, fee: u64 },
    #[error("Insufficient balance for multisig fee: available {available}, required {required}")]
    InsufficientBalanceForMultisigFee { available: u64, required: u64 },
    // Energy fee errors
    #[error("Insufficient fee_limit: required {required} TOS, provided {provided} TOS")]
    InsufficientFeeLimit { required: u64, provided: u64 },
}
