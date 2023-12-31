// Copyright (c) Facebook, Inc.
// Copyright (c) Tos  Network.
// SPDX-License-Identifier: Apache-2.0

use crate::{base_types::*, messages::*};
use failure::Fail;
use serde::{Deserialize, Serialize};

#[macro_export]
macro_rules! fp_bail {
    ($e:expr) => {
        return Err($e)
    };
}

#[macro_export(local_inner_macros)]
macro_rules! fp_ensure {
    ($cond:expr, $e:expr) => {
        if !($cond) {
            fp_bail!($e);
        }
    };
}

#[derive(Eq, PartialEq, Clone, Debug, Serialize, Deserialize, Fail, Hash)]
/// Custom error type for Tos.
pub enum TosError {
    // Signature verification
    #[fail(display = "Signature is not valid: {}", error)]
    InvalidSignature { error: String },
    #[fail(display = "Value was not signed by a known validator")]
    UnknownSigner,
    // Certificate verification
    #[fail(display = "Signatures in a certificate must form a quorum")]
    CertificateRequiresQuorum,
    // Transfer processing
    #[fail(display = "Transfers must have positive amount")]
    IncorrectTransferAmount,
    #[fail(
        display = "The given sequence number must match the next expected sequence number of the account"
    )]
    UnexpectedNonce,
    #[fail(
        display = "The transferred amount must be not exceed the current account balance: {:?}",
        current_balance
    )]
    InsufficientFunding { current_balance: Balance },
    #[fail(
        display = "Cannot initiate transfer while a transfer tx is still pending confirmation: {:?}",
        pending_confirmation
    )]
    PreviousTransferMustBeConfirmedFirst { pending_confirmation: Transaction },
    #[fail(display = "Transfer tx was processed but no signature was produced by validator")]
    ErrorWhileProcessingTransaction,
    #[fail(
        display = "An invalid answer was returned by the validator while requesting a certificate"
    )]
    ErrorWhileRequestingCertificate,
    #[fail(
        display = "Cannot confirm a transfer while previous transfer txs are still pending confirmation: {:?}",
        current_nonce
    )]
    MissingEalierConfirmations {
        current_nonce: VersionNumber,
    },
    // Synchronization validation
    #[fail(display = "Transaction index must increase by one")]
    UnexpectedTransactionIndex,
    // Account access
    #[fail(display = "No certificate for this account and sequence number")]
    CertificateNotfound,
    #[fail(display = "Unknown sender's account")]
    UnknownSenderAccount,
    #[fail(display = "Signatures in a certificate must be from different validators.")]
    CertificateValidatorReuse,
    #[fail(display = "Sequence numbers above the maximal value are not usable for transfers.")]
    InvalidNonce,
    #[fail(display = "Sequence number overflow.")]
    SequenceOverflow,
    #[fail(display = "Sequence number underflow.")]
    SequenceUnderflow,
    #[fail(display = "Amount overflow.")]
    AmountOverflow,
    #[fail(display = "Amount underflow.")]
    AmountUnderflow,
    #[fail(display = "Account balance overflow.")]
    BalanceOverflow,
    #[fail(display = "Account balance underflow.")]
    BalanceUnderflow,
    #[fail(display = "Wrong shard used.")]
    WrongShard,
    #[fail(display = "Invalid cross shard update.")]
    InvalidCrossShardUpdate,
    #[fail(display = "Cannot deserialize.")]
    InvalidDecoding,
    #[fail(display = "Unexpected message.")]
    UnexpectedMessage,
    #[fail(display = "Network error while querying service: {:?}.", error)]
    ClientIoError { error: String },
}
