// Copyright (c) Facebook, Inc.
// Copyright (c) Tos  Network.
// SPDX-License-Identifier: Apache-2.0

use crate::{base_types::*, validators::Validators, downloader::*, error::TosError, messages::*};
use failure::{bail, ensure};
use futures::{future, StreamExt};
use rand::seq::SliceRandom;
use std::{
    collections::{btree_map, BTreeMap, BTreeSet, HashMap},
    convert::TryFrom,
};

#[cfg(test)]
#[path = "unit_tests/client_tests.rs"]
mod client_tests;

pub type AsyncResult<'a, T, E> = future::BoxFuture<'a, Result<T, E>>;

pub trait ValidatorClient {
    /// Initiate a new transfer to a Tos or Primary account.
    fn handle_transfer_tx(
        &mut self,
        tx: Transaction,
    ) -> AsyncResult<AccountInfoResponse, TosError>;

    /// Confirm a transfer to a Tos or Primary account.
    fn handle_confirmation_tx(
        &mut self,
        tx: ConfirmationTx,
    ) -> AsyncResult<AccountInfoResponse, TosError>;

    /// Handle information requests for this account.
    fn handle_account_info_request(
        &mut self,
        request: AccountInfoRequest,
    ) -> AsyncResult<AccountInfoResponse, TosError>;
}

pub struct ClientState<ValidatorClient> {
    /// Our Tos address.
    address: Address,
    /// Our signature key.
    secret: KeyPair,
    /// Our Tos validators.
    validators: Validators,
    /// How to talk to this validators.
    validator_clients: HashMap<ValidatorName, ValidatorClient>,
    /// Expected sequence number for the next certified transfer.
    /// This is also the number of transfer certificates that we have created.
    nonce: Nonce,
    /// Pending transfer.
    pending_transfer: Option<Transaction>,

    // The remaining fields are used to minimize networking, and may not always be persisted locally.
    /// Transfer certificates that we have created ("sent").
    /// Normally, `sent` should contain one certificate for each index in `0..nonce`.
    sent: Vec<CertifiedTransaction>,
    /// Known received certificates, indexed by sender and sequence number.
    /// TODO: API to search and download yet unknown `received`.
    received: BTreeMap<(Address, Nonce), CertifiedTransaction>,
    /// The known spendable balance (including a possible initial funding, excluding unknown sent
    /// or received certificates).
    balance: Balance,
}

// Operations are considered successful when they successfully reach a quorum of validators.
pub trait Client {
    /// Send money to a Tos account.
    fn transfer_to_tos(
        &mut self,
        amount: Amount,
        recipient: Address,
        user_data: UserData,
    ) -> AsyncResult<CertifiedTransaction, failure::Error>;

    /// Receive money from Tos.
    fn receive_from_tos(
        &mut self,
        certificate: CertifiedTransaction,
    ) -> AsyncResult<(), failure::Error>;

    /// Send money to a Tos account.
    /// Do not check balance. (This may block the client)
    /// Do not confirm the transaction.
    fn transfer_to_tos_unsafe_unconfirmed(
        &mut self,
        amount: Amount,
        recipient: Address,
        user_data: UserData,
    ) -> AsyncResult<CertifiedTransaction, failure::Error>;

    /// Find how much money we can spend.
    /// TODO: Currently, this value only reflects received transfers that were
    /// locally processed by `receive_from_tos`.
    fn get_spendable_amount(&mut self) -> AsyncResult<Amount, failure::Error>;
}

impl<A> ClientState<A> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        address: Address,
        secret: KeyPair,
        validators: Validators,
        validator_clients: HashMap<ValidatorName, A>,
        nonce: Nonce,
        sent: Vec<CertifiedTransaction>,
        received: Vec<CertifiedTransaction>,
        balance: Balance,
    ) -> Self {
        Self {
            address,
            secret,
            validators,
            validator_clients,
            nonce,
            pending_transfer: None,
            sent,
            received: received
                .into_iter()
                .map(|cert| (cert.key(), cert))
                .collect(),
            balance,
        }
    }

    pub fn address(&self) -> Address {
        self.address
    }

    pub fn nonce(&self) -> Nonce {
        self.nonce
    }

    pub fn balance(&self) -> Balance {
        self.balance
    }

    pub fn pending_transfer(&self) -> &Option<Transaction> {
        &self.pending_transfer
    }

    pub fn sent(&self) -> &Vec<CertifiedTransaction> {
        &self.sent
    }

    pub fn received(&self) -> impl Iterator<Item = &CertifiedTransaction> {
        self.received.values()
    }
}

#[derive(Clone)]
struct CertificateRequester<A> {
    validators: Validators,
    validator_clients: Vec<A>,
    sender: Address,
}

impl<A> CertificateRequester<A> {
    fn new(validators: Validators, validator_clients: Vec<A>, sender: Address) -> Self {
        Self {
            validators,
            validator_clients,
            sender,
        }
    }
}

impl<A> Requester for CertificateRequester<A>
where
    A: ValidatorClient + Send + Sync + 'static + Clone,
{
    type Key = Nonce;
    type Value = Result<CertifiedTransaction, TosError>;

    /// Try to find a certificate for the given sender and sequence number.
    fn query(
        &mut self,
        nonce: Nonce,
    ) -> AsyncResult<CertifiedTransaction, TosError> {
        Box::pin(async move {
            let request = AccountInfoRequest {
                sender: self.sender,
                request_nonce: Some(nonce),
                request_received_transfers_excluding_first_nth: None,
            };
            // Sequentially try each validator in random tx.
            self.validator_clients.shuffle(&mut rand::thread_rng());
            for client in self.validator_clients.iter_mut() {
                let result = client.handle_account_info_request(request.clone()).await;
                if let Ok(AccountInfoResponse {
                    requested_certificate: Some(certificate),
                    ..
                }) = &result
                {
                    if certificate.check(&self.validators).is_ok() {
                        let transfer = &certificate.value.transfer;
                        if transfer.sender == self.sender
                            && transfer.nonce == nonce
                        {
                            return Ok(certificate.clone());
                        }
                    }
                }
            }
            Err(TosError::ErrorWhileRequestingCertificate)
        })
    }
}

/// Used for communicate_transfers
#[derive(Clone)]
enum CommunicateAction {
    SendTx(Transaction),
    SynchronizeNextNonce(Nonce),
}

impl<A> ClientState<A>
where
    A: ValidatorClient + Send + Sync + 'static + Clone,
{
    #[cfg(test)]
    async fn request_certificate(
        &mut self,
        sender: Address,
        nonce: Nonce,
    ) -> Result<CertifiedTransaction, TosError> {
        CertificateRequester::new(
            self.validators.clone(),
            self.validator_clients.values().cloned().collect(),
            sender,
        )
        .query(nonce)
        .await
    }

    /// Find the highest sequence number that is known to a quorum of validators.
    /// NOTE: This is only reliable in the synchronous model, with a sufficient timeout value.
    #[cfg(test)]
    async fn get_strong_majority_nonce(
        &mut self,
        sender: Address,
    ) -> Nonce {
        let request = AccountInfoRequest {
            sender,
            request_nonce: None,
            request_received_transfers_excluding_first_nth: None,
        };
        let numbers: futures::stream::FuturesUnordered<_> = self
            .validator_clients
            .iter_mut()
            .map(|(name, client)| {
                let fut = client.handle_account_info_request(request.clone());
                async move {
                    match fut.await {
                        Ok(info) => Some((*name, info.nonce)),
                        _ => None,
                    }
                }
            })
            .collect();
        self.validators.get_strong_majority_lower_bound(
            numbers.filter_map(|x| async move { x }).collect().await,
        )
    }

    /// Find the highest balance that is backed by a quorum of validators.
    /// NOTE: This is only reliable in the synchronous model, with a sufficient timeout value.
    #[cfg(test)]
    async fn get_strong_majority_balance(&mut self) -> Balance {
        let request = AccountInfoRequest {
            sender: self.address,
            request_nonce: None,
            request_received_transfers_excluding_first_nth: None,
        };
        let numbers: futures::stream::FuturesUnordered<_> = self
            .validator_clients
            .iter_mut()
            .map(|(name, client)| {
                let fut = client.handle_account_info_request(request.clone());
                async move {
                    match fut.await {
                        Ok(info) => Some((*name, info.balance)),
                        _ => None,
                    }
                }
            })
            .collect();
        self.validators.get_strong_majority_lower_bound(
            numbers.filter_map(|x| async move { x }).collect().await,
        )
    }

    /// Execute a sequence of actions in parallel for a quorum of validators.
    async fn communicate_with_quorum<'a, V, F>(
        &'a mut self,
        execute: F,
    ) -> Result<Vec<V>, failure::Error>
    where
        F: Fn(ValidatorName, &'a mut A) -> AsyncResult<'a, V, TosError> + Clone,
    {
        let validators = &self.validators;
        let validator_clients = &mut self.validator_clients;
        let mut responses: futures::stream::FuturesUnordered<_> = validator_clients
            .iter_mut()
            .map(|(name, client)| {
                let execute = execute.clone();
                async move { (*name, execute(*name, client).await) }
            })
            .collect();

        let mut values = Vec::new();
        let mut value_score = 0;
        let mut error_scores = HashMap::new();
        while let Some((name, result)) = responses.next().await {
            match result {
                Ok(value) => {
                    values.push(value);
                    value_score += validators.weight(&name);
                    if value_score >= validators.quorum_threshold() {
                        // Success!
                        return Ok(values);
                    }
                }
                Err(err) => {
                    let entry = error_scores.entry(err.clone()).or_insert(0);
                    *entry += validators.weight(&name);
                    if *entry >= validators.validity_threshold() {
                        // At least one honest node returned this error.
                        // No quorum can be reached, so return early.
                        bail!(
                            "Failed to communicate with a quorum of validators: {}",
                            err
                        );
                    }
                }
            }
        }

        bail!("Failed to communicate with a quorum of validators (multiple errors)");
    }

    /// Broadcast confirmation txs and optionally one more transfer tx.
    /// The corresponding sequence numbers should be consecutive and increasing.
    async fn communicate_transfers(
        &mut self,
        sender: Address,
        known_certificates: Vec<CertifiedTransaction>,
        action: CommunicateAction,
    ) -> Result<Vec<CertifiedTransaction>, failure::Error> {
        let target_nonce = match &action {
            CommunicateAction::SendTx(tx) => tx.transfer.nonce,
            CommunicateAction::SynchronizeNextNonce(seq) => *seq,
        };
        let requester = CertificateRequester::new(
            self.validators.clone(),
            self.validator_clients.values().cloned().collect(),
            sender,
        );
        let (task, mut handle) = Downloader::start(
            requester,
            known_certificates.into_iter().filter_map(|cert| {
                if cert.value.transfer.sender == sender {
                    Some((cert.value.transfer.nonce, Ok(cert)))
                } else {
                    None
                }
            }),
        );
        let validators = self.validators.clone();
        let votes = self
            .communicate_with_quorum(|name, client| {
                let mut handle = handle.clone();
                let action = action.clone();
                let validators = &validators;
                Box::pin(async move {
                    // Figure out which certificates this validator is missing.
                    let request = AccountInfoRequest {
                        sender,
                        request_nonce: None,
                        request_received_transfers_excluding_first_nth: None,
                    };
                    let response = client.handle_account_info_request(request).await?;
                    let current_nonce = response.nonce;
                    // Download each missing certificate in reverse tx using the downloader.
                    let mut missing_certificates = Vec::new();
                    let mut number = target_nonce.decrement();
                    while let Ok(value) = number {
                        if value < current_nonce {
                            break;
                        }
                        let certificate = handle
                            .query(value)
                            .await
                            .map_err(|_| TosError::ErrorWhileRequestingCertificate)??;
                        missing_certificates.push(certificate);
                        number = value.decrement();
                    }
                    // Send all missing confirmation txs.
                    missing_certificates.reverse();
                    for certificate in missing_certificates {
                        client
                            .handle_confirmation_tx(ConfirmationTx::new(certificate))
                            .await?;
                    }
                    // Send the transfer tx (if any) and return a vote.
                    if let CommunicateAction::SendTx(tx) = action {
                        let result = client.handle_transfer_tx(tx).await;
                        match result {
                            Ok(AccountInfoResponse {
                                pending_confirmation: Some(signed_tx),
                                ..
                            }) => {
                                fp_ensure!(
                                    signed_tx.validator == name,
                                    TosError::ErrorWhileProcessingTransaction
                                );
                                signed_tx.check(validators)?;
                                return Ok(Some(signed_tx));
                            }
                            Err(err) => return Err(err),
                            _ => return Err(TosError::ErrorWhileProcessingTransaction),
                        }
                    }
                    Ok(None)
                })
            })
            .await?;
        // Terminate downloader task and retrieve the content of the cache.
        handle.stop().await?;
        let mut certificates: Vec<_> = task.await.unwrap().filter_map(Result::ok).collect();
        if let CommunicateAction::SendTx(tx) = action {
            let certificate = CertifiedTransaction {
                value: tx,
                signatures: votes
                    .into_iter()
                    .filter_map(|vote| match vote {
                        Some(signed_tx) => {
                            Some((signed_tx.validator, signed_tx.signature))
                        }
                        None => None,
                    })
                    .collect(),
            };
            // Certificate is valid because
            // * `communicate_with_quorum` ensured a sufficient "weight" of (non-error) answers were returned by validators.
            // * each answer is a vote signed by the expected validator.
            certificates.push(certificate);
        }
        Ok(certificates)
    }

    /// Make sure we have all our certificates with sequence number
    /// in the range 0..self.nonce
    async fn download_sent(
        &self,
    ) -> Result<Vec<CertifiedTransaction>, TosError> {
        let mut requester = CertificateRequester::new(
            self.validators.clone(),
            self.validator_clients.values().cloned().collect(),
            self.address,
        );
        let known_nonces: BTreeSet<_> = self
            .sent
            .iter()
            .map(|cert| cert.value.transfer.nonce)
            .collect();
        let mut sent = self.sent.clone();
        let mut number = Nonce::from(0);
        while number < self.nonce {
            if !known_nonces.contains(&number) {
                let certificate = requester.query(number).await?;
                sent.push(certificate);
            }
            number = number.increment().unwrap_or_else(|_| Nonce::max());
        }
        sent.sort_by_key(|cert| cert.value.transfer.nonce);
        Ok(sent)
    }

    /// Send money to a Tos or Primary recipient.
    async fn transfer(
        &mut self,
        amount: Amount,
        recipient: Address,
        user_data: UserData,
    ) -> Result<CertifiedTransaction, failure::Error> {
        // Trying to overspend may block the account. To prevent this, we compare with
        // the balance as we know it.
        let safe_amount = self.get_spendable_amount().await?;
        ensure!(
            amount <= safe_amount,
            "Requested amount ({:?}) is not backed by sufficient funds ({:?})",
            amount,
            safe_amount
        );
        let transfer = Transfer {
            sender: self.address,
            recipient,
            amount,
            nonce: self.nonce,
            user_data,
        };
        let tx = Transaction::new(transfer, &self.secret);
        let certificate = self
            .execute_transfer(tx, /* with_confirmation */ true)
            .await?;
        Ok(certificate)
    }

    /// Update our view of sent certificates. Adjust the local balance and the next sequence number accordingly.
    /// NOTE: This is only useful in the eventuality of missing local data.
    /// We assume certificates to be valid and sent by us, and their sequence numbers to be unique.
    fn update_sent(
        &mut self,
        sent: Vec<CertifiedTransaction>,
    ) -> Result<(), TosError> {
        let mut new_balance = self.balance;
        let mut new_nonce = self.nonce;
        for new_cert in &sent {
            new_balance = new_balance.try_sub(new_cert.value.transfer.amount.into())?;
            if new_cert.value.transfer.nonce >= new_nonce {
                new_nonce = new_cert
                    .value
                    .transfer
                    .nonce
                    .increment()
                    .unwrap_or_else(|_| Nonce::max());
            }
        }
        for old_cert in &self.sent {
            new_balance = new_balance.try_add(old_cert.value.transfer.amount.into())?;
        }
        // Atomic update
        self.sent = sent;
        self.balance = new_balance;
        self.nonce = new_nonce;
        // Sanity check
        assert_eq!(
            self.sent.len(),
            self.nonce.into()
        );
        Ok(())
    }

    /// Execute (or retry) a transfer tx. Update local balance.
    async fn execute_transfer(
        &mut self,
        tx: Transaction,
        with_confirmation: bool,
    ) -> Result<CertifiedTransaction, failure::Error> {
        ensure!(
            self.pending_transfer == None || self.pending_transfer.as_ref() == Some(&tx),
            "Client state has a different pending transfer",
        );
        ensure!(
            tx.transfer.nonce == self.nonce,
            "Unexpected sequence number"
        );
        self.pending_transfer = Some(tx.clone());
        let new_sent = self
            .communicate_transfers(
                self.address,
                self.sent.clone(),
                CommunicateAction::SendTx(tx.clone()),
            )
            .await?;
        assert_eq!(new_sent.last().unwrap().value, tx);
        // Clear `pending_transfer` and update `sent`,
        // `balance`, and `nonce`. (Note that if we were using persistent
        // storage, we should ensure update atomicity in the eventuality of a crash.)
        self.pending_transfer = None;
        self.update_sent(new_sent)?;
        // Confirm last transfer certificate if needed.
        if with_confirmation {
            self.communicate_transfers(
                self.address,
                self.sent.clone(),
                CommunicateAction::SynchronizeNextNonce(self.nonce),
            )
            .await?;
        }
        Ok(self.sent.last().unwrap().clone())
    }
}

impl<A> Client for ClientState<A>
where
    A: ValidatorClient + Send + Sync + Clone + 'static,
{
    fn transfer_to_tos(
        &mut self,
        amount: Amount,
        recipient: Address,
        user_data: UserData,
    ) -> AsyncResult<CertifiedTransaction, failure::Error> {
        Box::pin(self.transfer(amount, recipient, user_data))
    }

    fn get_spendable_amount(&mut self) -> AsyncResult<Amount, failure::Error> {
        Box::pin(async move {
            if let Some(tx) = self.pending_transfer.clone() {
                // Finish executing the previous transfer.
                self.execute_transfer(tx, /* with_confirmation */ false)
                    .await?;
            }
            if self.sent.len() < self.nonce.into() {
                // Recover missing sent certificates.
                let new_sent = self.download_sent().await?;
                self.update_sent(new_sent)?;
            }
            let amount = if self.balance < Balance::zero() {
                Amount::zero()
            } else {
                Amount::try_from(self.balance).unwrap_or_else(|_| std::u64::MAX.into())
            };
            Ok(amount)
        })
    }

    fn receive_from_tos(
        &mut self,
        certificate: CertifiedTransaction,
    ) -> AsyncResult<(), failure::Error> {
        Box::pin(async move {
            certificate.check(&self.validators)?;
            let transfer = &certificate.value.transfer;
            ensure!(
                transfer.recipient == self.address,
                "Transfer should be received by us."
            );
            self.communicate_transfers(
                transfer.sender,
                vec![certificate.clone()],
                CommunicateAction::SynchronizeNextNonce(
                    certificate.value.transfer.nonce.increment()?,
                ),
            )
            .await?;
            // Everything worked: update the local balance.
            let transfer = &certificate.value.transfer;
            if let btree_map::Entry::Vacant(entry) =
                self.received.entry(transfer.key())
            {
                self.balance = self.balance.try_add(transfer.amount.into())?;
                entry.insert(certificate);
            }
            Ok(())
        })
    }

    fn transfer_to_tos_unsafe_unconfirmed(
        &mut self,
        amount: Amount,
        recipient: Address,
        user_data: UserData,
    ) -> AsyncResult<CertifiedTransaction, failure::Error> {
        Box::pin(async move {
            let transfer = Transfer {
                sender: self.address,
                recipient: recipient,
                amount,
                nonce: self.nonce,
                user_data,
            };
            let tx = Transaction::new(transfer, &self.secret);
            let new_certificate = self
                .execute_transfer(tx, /* with_confirmation */ false)
                .await?;
            Ok(new_certificate)
        })
    }
}
