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
    #[error("Agent account already registered")]
    AgentAccountAlreadyRegistered,
    #[error("Invalid agent account controller")]
    AgentAccountInvalidController,
    #[error("Agent account is frozen")]
    AgentAccountFrozen,
    #[error("Agent account unauthorized")]
    AgentAccountUnauthorized,
    #[error("Agent account policy violation")]
    AgentAccountPolicyViolation,
    #[error("Agent session key expired")]
    AgentAccountSessionKeyExpired,
    #[error("Agent session key not found")]
    AgentAccountSessionKeyNotFound,
    #[error("Agent session key already exists")]
    AgentAccountSessionKeyExists,
    #[error("Agent account invalid parameter")]
    AgentAccountInvalidParameter,
    #[error("overflow during gas calculation")]
    GasOverflow,
    #[error("overflow during gas refund")]
    GasRefundOverflow,
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
    #[error("UNO balance overflow during verification")]
    UnoBalanceOverflow,
    #[error("Invalid transfer amount")]
    InvalidTransferAmount,
    #[error("Shield amount must be at least 100 TOS")]
    ShieldAmountTooLow,
    #[error("Invalid fee: expected {0}, got {1}")]
    InvalidFee(u64, u64),
    #[error("Too many contract events: {count} (max {max})")]
    TooManyContractEvents { count: usize, max: usize },

    // ===== TNS Name Registration Errors =====
    #[error("Invalid name length: {0} (expected 3-64)")]
    InvalidNameLength(usize),
    #[error("Name must start with a letter")]
    InvalidNameStart,
    #[error("Name cannot end with separator")]
    InvalidNameEnd,
    #[error("Invalid character in name: {0}")]
    InvalidNameCharacter(char),
    #[error("Consecutive separators not allowed")]
    ConsecutiveSeparators,
    #[error("Reserved name: {0}")]
    ReservedName(String),
    #[error("Confusing name (phishing risk): {0}")]
    ConfusingName(String),
    #[error("Name already registered")]
    NameAlreadyRegistered,
    #[error("Account already has a registered name")]
    AccountAlreadyHasName,

    // ===== Ephemeral Message Errors =====
    #[error("Invalid message TTL: {0} (expected 100-86400)")]
    InvalidMessageTTL(u32),
    #[error("Message too large: {0} bytes (max 188)")]
    MessageTooLarge(usize),
    #[error("Message cannot be empty")]
    EmptyMessage,
    #[error("Recipient name not registered")]
    RecipientNotFound,
    #[error("Sender must have a registered TNS name")]
    SenderNotRegistered,
    #[error("Sender name hash mismatch (forged payload)")]
    InvalidSender,
    #[error("Cannot send message to yourself")]
    SelfMessage,
    #[error("Message with this nonce already exists (replay attack)")]
    MessageAlreadyExists,
    #[error("Message nonce must equal transaction nonce")]
    InvalidMessageNonce,
    #[error("Invalid receiver handle (not a valid curve point)")]
    InvalidReceiverHandle,
    #[error("Insufficient TNS fee: required {required}, provided {provided}")]
    InsufficientTnsFee { required: u64, provided: u64 },

    // ===== Arbiter Registration Errors =====
    #[error("Arbiter name length {len} exceeds max {max}")]
    ArbiterNameLength { len: usize, max: usize },
    #[error("Arbiter fee basis points invalid: {0}")]
    ArbiterInvalidFee(u16),
    #[error("Arbiter stake too low: required {required}, found {found}")]
    ArbiterStakeTooLow { required: u64, found: u64 },
    #[error("Arbiter escrow range invalid: min {min} > max {max}")]
    ArbiterEscrowRangeInvalid { min: u64, max: u64 },
    #[error("Arbiter already registered")]
    ArbiterAlreadyRegistered,
    #[error("Arbiter not found")]
    ArbiterNotFound,
    #[error("Arbiter status update not allowed")]
    ArbiterInvalidStatus,
    #[error("Arbiter deactivation cannot add stake")]
    ArbiterDeactivateWithStake,
    #[error("Arbiter has no stake to withdraw")]
    ArbiterNoStakeToWithdraw,
    #[error("Arbiter not in exit process")]
    ArbiterNotInExitProcess,
    #[error("Arbiter cooldown not complete: current {current}, required {required}")]
    ArbiterCooldownNotComplete { current: u64, required: u64 },
    #[error("Arbiter has active cases: {count}")]
    ArbiterHasActiveCases { count: u64 },
    #[error("Arbiter already removed")]
    ArbiterAlreadyRemoved,
}
