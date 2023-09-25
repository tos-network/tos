// Copyright (c) Facebook, Inc.
// Copyright (c) Tos  Network.
// SPDX-License-Identifier: Apache-2.0

use super::{base_types::*, validators::Validators, error::*};

#[cfg(test)]
#[path = "unit_tests/messages_tests.rs"]
mod messages_tests;

use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    hash::{Hash, Hasher},
};

#[derive(Eq, PartialEq, Clone, Debug, Serialize, Deserialize)]
pub struct FundingTransaction {
    pub recipient: Address,
    pub primary_coins: Amount,
    // TODO: Authenticated by Primary sender.
}

#[derive(Eq, PartialEq, Clone, Debug, Serialize, Deserialize)]
pub struct PrimarySynchronizationOrder {
    pub recipient: Address,
    pub amount: Amount,
    pub transaction_index: VersionNumber,
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, Serialize, Deserialize)]
pub struct Transfer {
    pub sender: Address,
    pub recipient: Address,
    pub amount: Amount,
    pub sequence_number: Nonce,
    pub user_data: UserData,
}

#[derive(Eq, Clone, Debug, Serialize, Deserialize)]
pub struct TransferOrder {
    pub transfer: Transfer,
    pub signature: Signature,
}

#[derive(Eq, Clone, Debug, Serialize, Deserialize)]
pub struct SignedTransferOrder {
    pub value: TransferOrder,
    pub validator: ValidatorName,
    pub signature: Signature,
}

#[derive(Eq, Clone, Debug, Serialize, Deserialize)]
pub struct CertifiedTransferOrder {
    pub value: TransferOrder,
    pub signatures: Vec<(ValidatorName, Signature)>,
}

#[derive(Eq, PartialEq, Clone, Debug, Serialize, Deserialize)]
pub struct RedeemTransaction {
    pub transfer_certificate: CertifiedTransferOrder,
}

#[derive(Eq, PartialEq, Clone, Debug, Serialize, Deserialize)]
pub struct ConfirmationOrder {
    pub transfer_certificate: CertifiedTransferOrder,
}

#[derive(Eq, PartialEq, Clone, Debug, Serialize, Deserialize)]
pub struct AccountInfoRequest {
    pub sender: Address,
    pub request_sequence_number: Option<Nonce>,
    pub request_received_transfers_excluding_first_nth: Option<usize>,
}

#[derive(Eq, PartialEq, Clone, Debug, Serialize, Deserialize)]
pub struct AccountInfoResponse {
    pub sender: Address,
    pub balance: Balance,
    pub nonce: Nonce,
    pub pending_confirmation: Option<SignedTransferOrder>,
    pub requested_certificate: Option<CertifiedTransferOrder>,
    pub requested_received_transfers: Vec<CertifiedTransferOrder>,
}

#[derive(Eq, PartialEq, Clone, Debug, Serialize, Deserialize)]
pub struct CrossShardUpdate {
    pub shard_id: ShardId,
    pub transfer_certificate: CertifiedTransferOrder,
}

impl Hash for TransferOrder {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.transfer.hash(state);
    }
}

impl PartialEq for TransferOrder {
    fn eq(&self, other: &Self) -> bool {
        self.transfer == other.transfer
    }
}

impl Hash for SignedTransferOrder {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.value.hash(state);
        self.validator.hash(state);
    }
}

impl PartialEq for SignedTransferOrder {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value && self.validator == other.validator
    }
}

impl Hash for CertifiedTransferOrder {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.value.hash(state);
        self.signatures.len().hash(state);
        for (name, _) in self.signatures.iter() {
            name.hash(state);
        }
    }
}

impl PartialEq for CertifiedTransferOrder {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
            && self.signatures.len() == other.signatures.len()
            && self
                .signatures
                .iter()
                .map(|(name, _)| name)
                .eq(other.signatures.iter().map(|(name, _)| name))
    }
}

impl Transfer {
    pub fn key(&self) -> (Address, Nonce) {
        (self.sender, self.sequence_number)
    }
}

impl TransferOrder {
    pub fn new(transfer: Transfer, secret: &KeyPair) -> Self {
        let signature = Signature::new(&transfer, secret);
        Self {
            transfer,
            signature,
        }
    }

    pub fn check_signature(&self) -> Result<(), TosError> {
        self.signature.check(&self.transfer, self.transfer.sender)
    }
}

impl SignedTransferOrder {
    /// Use signing key to create a signed object.
    pub fn new(value: TransferOrder, validator: ValidatorName, secret: &KeyPair) -> Self {
        let signature = Signature::new(&value.transfer, secret);
        Self {
            value,
            validator,
            signature,
        }
    }

    /// Verify the signature and return the non-zero voting right of the validator.
    pub fn check(&self, validators: &Validators) -> Result<usize, TosError> {
        self.value.check_signature()?;
        let weight = validators.weight(&self.validator);
        fp_ensure!(weight > 0, TosError::UnknownSigner);
        self.signature.check(&self.value.transfer, self.validator)?;
        Ok(weight)
    }
}

pub struct SignatureAggregator<'a> {
    validators: &'a Validators,
    weight: usize,
    used_authorities: HashSet<ValidatorName>,
    partial: CertifiedTransferOrder,
}

impl<'a> SignatureAggregator<'a> {
    /// Start aggregating signatures for the given value into a certificate.
    pub fn try_new(value: TransferOrder, validators: &'a Validators) -> Result<Self, TosError> {
        value.check_signature()?;
        Ok(Self::new_unsafe(value, validators))
    }

    /// Same as try_new but we don't check the order.
    pub fn new_unsafe(value: TransferOrder, validators: &'a Validators) -> Self {
        Self {
            validators,
            weight: 0,
            used_authorities: HashSet::new(),
            partial: CertifiedTransferOrder {
                value,
                signatures: Vec::new(),
            },
        }
    }

    /// Try to append a signature to a (partial) certificate. Returns Some(certificate) if a quorum was reached.
    /// The resulting final certificate is guaranteed to be valid in the sense of `check` below.
    /// Returns an error if the signed value cannot be aggregated.
    pub fn append(
        &mut self,
        validator: ValidatorName,
        signature: Signature,
    ) -> Result<Option<CertifiedTransferOrder>, TosError> {
        signature.check(&self.partial.value.transfer, validator)?;
        // Check that each validator only appears once.
        fp_ensure!(
            !self.used_authorities.contains(&validator),
            TosError::CertificateValidatorReuse
        );
        self.used_authorities.insert(validator);
        // Update weight.
        let voting_rights = self.validators.weight(&validator);
        fp_ensure!(voting_rights > 0, TosError::UnknownSigner);
        self.weight += voting_rights;
        // Update certificate.
        self.partial.signatures.push((validator, signature));

        if self.weight >= self.validators.quorum_threshold() {
            Ok(Some(self.partial.clone()))
        } else {
            Ok(None)
        }
    }
}

impl CertifiedTransferOrder {
    pub fn key(&self) -> (Address, Nonce) {
        let transfer = &self.value.transfer;
        transfer.key()
    }

    /// Verify the certificate.
    pub fn check(&self, validators: &Validators) -> Result<(), TosError> {
        // Check the quorum.
        let mut weight = 0;
        let mut used_authorities = HashSet::new();
        for (validator, _) in self.signatures.iter() {
            // Check that each validator only appears once.
            fp_ensure!(
                !used_authorities.contains(validator),
                TosError::CertificateValidatorReuse
            );
            used_authorities.insert(*validator);
            // Update weight.
            let voting_rights = validators.weight(validator);
            fp_ensure!(voting_rights > 0, TosError::UnknownSigner);
            weight += voting_rights;
        }
        fp_ensure!(
            weight >= validators.quorum_threshold(),
            TosError::CertificateRequiresQuorum
        );
        // All what is left is checking signatures!
        let inner_sig = (self.value.transfer.sender, self.value.signature);
        Signature::verify_batch(
            &self.value.transfer,
            std::iter::once(&inner_sig).chain(&self.signatures),
        )
    }
}

impl RedeemTransaction {
    pub fn new(transfer_certificate: CertifiedTransferOrder) -> Self {
        Self {
            transfer_certificate,
        }
    }
}

impl ConfirmationOrder {
    pub fn new(transfer_certificate: CertifiedTransferOrder) -> Self {
        Self {
            transfer_certificate,
        }
    }
}

impl BcsSignable for Transfer {}
