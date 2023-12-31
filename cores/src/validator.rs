// Copyright (c) Facebook, Inc.
// Copyright (c) Tos  Network.
// SPDX-License-Identifier: Apache-2.0

use crate::{base_types::*, validators::Validators, error::TosError, messages::*};
use std::{collections::BTreeMap, convert::TryInto};

#[cfg(test)]
#[path = "unit_tests/validator_tests.rs"]
mod validator_tests;

#[derive(Eq, PartialEq, Debug)]
pub struct AccountOffchainState {
    /// Balance of the Tos account.
    pub balance: Balance,
    /// Sequence number tracking spending actions.
    pub nonce: Nonce,
    /// Whether we have signed a transfer for this sequence number already.
    pub pending_confirmation: Option<SignedTransaction>,
    /// All confirmed certificates for this sender.
    pub confirmed_log: Vec<CertifiedTransaction>,
    /// All confirmed certificates as a receiver.
    pub received_log: Vec<CertifiedTransaction>,
}

pub struct ValidatorState {
    /// The name of this autority.
    pub name: ValidatorName,
    /// Validators of this Tos instance.
    pub validators: Validators,
    /// The signature key of the validator.
    pub secret: KeyPair,
    /// Offchain states of Tos accounts.
    pub accounts: BTreeMap<Address, AccountOffchainState>,
    /// The latest transaction index of the blockchain that the validator has seen.
    pub last_transaction_index: VersionNumber,
    /// The sharding ID of this validator shard. 0 if one shard.
    pub shard_id: ShardId,
    /// The number of shards. 1 if single shard.
    pub number_of_shards: u32,
}

/// Interface provided by each (shard of an) validator.
/// All commands return either the current account info or an error.
/// Repeating commands produces no changes and returns no error.
pub trait Validator {
    /// Initiate a new transfer to a Tos or Primary account.
    fn handle_transfer_tx(
        &mut self,
        tx: Transaction,
    ) -> Result<AccountInfoResponse, TosError>;

    /// Confirm a transfer to a Tos or Primary account.
    fn handle_confirmation_tx(
        &mut self,
        tx: ConfirmationTx,
    ) -> Result<(AccountInfoResponse, Option<CrossShardUpdate>), TosError>;

    /// Handle information requests for this account.
    fn handle_account_info_request(
        &self,
        request: AccountInfoRequest,
    ) -> Result<AccountInfoResponse, TosError>;

    /// Handle cross updates from another shard of the same validator.
    /// This relies on deliver-once semantics of a trusted channel between shards.
    fn handle_cross_shard_recipient_commit(
        &mut self,
        certificate: CertifiedTransaction,
    ) -> Result<(), TosError>;
}

impl Validator for ValidatorState {
    /// Initiate a new transfer.
    fn handle_transfer_tx(
        &mut self,
        tx: Transaction,
    ) -> Result<AccountInfoResponse, TosError> {
        // Check the sender's signature and retrieve the transfer data.
        fp_ensure!(
            self.in_shard(&tx.transfer.sender),
            TosError::WrongShard
        );
        tx.check_signature()?;
        let transfer = &tx.transfer;
        let sender = transfer.sender;
        fp_ensure!(
            transfer.nonce <= Nonce::max(),
            TosError::InvalidNonce
        );
        fp_ensure!(
            transfer.amount > Amount::zero(),
            TosError::IncorrectTransferAmount
        );
        match self.accounts.get_mut(&sender) {
            None => fp_bail!(TosError::UnknownSenderAccount),
            Some(account) => {
                if let Some(pending_confirmation) = &account.pending_confirmation {
                    fp_ensure!(
                        &pending_confirmation.value.transfer == transfer,
                        TosError::PreviousTransferMustBeConfirmedFirst {
                            pending_confirmation: pending_confirmation.value.clone()
                        }
                    );
                    // This exact transfer tx was already signed. Return the previous value.
                    return Ok(account.make_account_info(sender));
                }
                fp_ensure!(
                    account.nonce == transfer.nonce,
                    TosError::UnexpectedNonce
                );
                fp_ensure!(
                    account.balance >= transfer.amount.into(),
                    TosError::InsufficientFunding {
                        current_balance: account.balance
                    }
                );
                let signed_tx = SignedTransaction::new(tx, self.name, &self.secret);
                account.pending_confirmation = Some(signed_tx);
                Ok(account.make_account_info(sender))
            }
        }
    }

    /// Confirm a transfer.
    fn handle_confirmation_tx(
        &mut self,
        confirmation_tx: ConfirmationTx,
    ) -> Result<(AccountInfoResponse, Option<CrossShardUpdate>), TosError> {
        let certificate = confirmation_tx.ctx;
        // Check the certificate and retrieve the transfer data.
        fp_ensure!(
            self.in_shard(&certificate.value.transfer.sender),
            TosError::WrongShard
        );
        certificate.check(&self.validators)?;
        let transfer = certificate.value.transfer.clone();

        // First we copy all relevant data from sender.
        let sender_account = self
            .accounts
            .entry(transfer.sender)
            .or_insert_with(AccountOffchainState::new);
        let mut sender_nonce = sender_account.nonce;
        let mut sender_balance = sender_account.balance;

        // Check and update the copied state
        if sender_nonce < transfer.nonce {
            fp_bail!(TosError::MissingEalierConfirmations {
                current_nonce: sender_nonce
            });
        }
        if sender_nonce > transfer.nonce {
            // Transfer was already confirmed.
            return Ok((sender_account.make_account_info(transfer.sender), None));
        }
        sender_balance = sender_balance.try_sub(transfer.amount.into())?;
        sender_nonce = sender_nonce.increment()?;

        // Commit sender state back to the database (Must never fail!)
        sender_account.balance = sender_balance;
        sender_account.nonce = sender_nonce;
        sender_account.pending_confirmation = None;
        sender_account.confirmed_log.push(certificate.clone());
        let info = sender_account.make_account_info(transfer.sender);

        // Update Tos recipient state locally or issue a cross-shard update (Must never fail!)
        let recipient = match transfer.recipient {
            recipient => recipient,
        };
        // If the recipient is in the same shard, read and update the account.
        if self.in_shard(&recipient) {
            let recipient_account = self
                .accounts
                .entry(recipient)
                .or_insert_with(AccountOffchainState::new);
            recipient_account.balance = recipient_account
                .balance
                .try_add(transfer.amount.into())
                .unwrap_or_else(|_| Balance::max());
            recipient_account.received_log.push(certificate);
            // Done updating recipient.
            return Ok((info, None));
        }
        // Otherwise, we need to send a cross-shard update.
        let cross_shard = Some(CrossShardUpdate {
            shard_id: self.which_shard(&recipient),
            ctx: certificate,
        });
        Ok((info, cross_shard))
    }

    // NOTE: Need to rely on deliver-once semantics from comms channel
    fn handle_cross_shard_recipient_commit(
        &mut self,
        certificate: CertifiedTransaction,
    ) -> Result<(), TosError> {
        // TODO: check certificate again?
        let transfer = &certificate.value.transfer;

        let recipient = match transfer.recipient {
            recipient => recipient,
        };
        fp_ensure!(self.in_shard(&recipient), TosError::WrongShard);
        let recipient_account = self
            .accounts
            .entry(recipient)
            .or_insert_with(AccountOffchainState::new);
        recipient_account.balance = recipient_account
            .balance
            .try_add(transfer.amount.into())
            .unwrap_or_else(|_| Balance::max());
        recipient_account.received_log.push(certificate);
        Ok(())
    }

    fn handle_account_info_request(
        &self,
        request: AccountInfoRequest,
    ) -> Result<AccountInfoResponse, TosError> {
        fp_ensure!(self.in_shard(&request.sender), TosError::WrongShard);
        let account = self.account_state(&request.sender)?;
        let mut response = account.make_account_info(request.sender);
        if let Some(seq) = request.request_nonce {
            if let Some(cert) = account.confirmed_log.get(usize::from(seq)) {
                response.requested_certificate = Some(cert.clone());
            } else {
                fp_bail!(TosError::CertificateNotfound)
            }
        }
        if let Some(idx) = request.request_received_transfers_excluding_first_nth {
            response.requested_received_transfers = account.received_log[idx..].to_vec();
        }
        Ok(response)
    }
}

impl Default for AccountOffchainState {
    fn default() -> Self {
        Self {
            balance: Balance::zero(),
            nonce: Nonce::new(),
            pending_confirmation: None,
            confirmed_log: Vec::new(),
            received_log: Vec::new(),
        }
    }
}

impl AccountOffchainState {
    pub fn new() -> Self {
        Self::default()
    }

    fn make_account_info(&self, sender: Address) -> AccountInfoResponse {
        AccountInfoResponse {
            sender,
            balance: self.balance,
            nonce: self.nonce,
            pending_confirmation: self.pending_confirmation.clone(),
            requested_certificate: None,
            requested_received_transfers: Vec::new(),
        }
    }

    #[cfg(test)]
    pub fn new_with_balance(balance: Balance, received_log: Vec<CertifiedTransaction>) -> Self {
        Self {
            balance,
            nonce: Nonce::new(),
            pending_confirmation: None,
            confirmed_log: Vec::new(),
            received_log,
        }
    }
}

impl ValidatorState {
    pub fn new(validators: Validators, name: ValidatorName, secret: KeyPair) -> Self {
        ValidatorState {
            validators,
            name,
            secret,
            accounts: BTreeMap::new(),
            last_transaction_index: VersionNumber::new(),
            shard_id: 0,
            number_of_shards: 1,
        }
    }

    pub fn new_shard(
        validators: Validators,
        name: ValidatorName,
        secret: KeyPair,
        shard_id: u32,
        number_of_shards: u32,
    ) -> Self {
        ValidatorState {
            validators,
            name,
            secret,
            accounts: BTreeMap::new(),
            last_transaction_index: VersionNumber::new(),
            shard_id,
            number_of_shards,
        }
    }

    pub fn in_shard(&self, address: &Address) -> bool {
        self.which_shard(address) == self.shard_id
    }

    pub fn get_shard(shards: u32, address: &Address) -> u32 {
        const LAST_INTEGER_INDEX: usize = std::mem::size_of::<Address>() - 4;
        u32::from_le_bytes(address.0[LAST_INTEGER_INDEX..].try_into().expect("4 bytes"))
            % shards
    }

    pub fn which_shard(&self, address: &Address) -> u32 {
        Self::get_shard(self.number_of_shards, address)
    }

    fn account_state(
        &self,
        address: &Address,
    ) -> Result<&AccountOffchainState, TosError> {
        self.accounts
            .get(address)
            .ok_or(TosError::UnknownSenderAccount)
    }

    #[cfg(test)]
    pub fn accounts_mut(&mut self) -> &mut BTreeMap<Address, AccountOffchainState> {
        &mut self.accounts
    }
}
