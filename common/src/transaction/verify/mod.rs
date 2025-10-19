mod state;
mod error;
mod contract;
mod zkp_cache;

use std::{
    borrow::Cow,
    iter,
    sync::Arc
};

use anyhow::anyhow;
// Balance simplification: RangeProof removed
// use bulletproofs::RangeProof;
use indexmap::IndexMap;
use log::{debug, trace};
use tos_vm::ModuleValidator;

use crate::{
    account::EnergyResource,
    config::{MAX_GAS_USAGE_PER_TX, TOS_ASSET},
    contract::ContractProvider,
    crypto::{
        elgamal::{
            DecompressionError,
            PublicKey
        },
        hash,
        proofs::ProofVerificationError,
        Hash,
        // Balance simplification: ProtocolTranscript removed - no longer needed
    },
    serializer::Serializer,
    transaction::{
        TxVersion,
        EXTRA_DATA_LIMIT_SIZE,
        EXTRA_DATA_LIMIT_SUM_SIZE,
        MAX_DEPOSIT_PER_INVOKE_CALL,
        MAX_MULTISIG_PARTICIPANTS,
        MAX_TRANSFER_COUNT
    }
};
use super::{
    ContractDeposit,
    Transaction,
    TransactionType,
    payload::EnergyPayload
};
use contract::InvokeContract;

pub use state::*;
pub use error::*;
pub use zkp_cache::*;


// Decompressed deposit ciphertext
// Transaction deposits are stored in a compressed format
// We need to decompress them only one time
// This struct will be removed when contract deposits are changed to plain u64

impl Transaction {
    pub fn has_valid_version_format(&self) -> bool {
        match self.version {
            TxVersion::T0 => {
                // T0 supports all transaction types
                match &self.data {
                    TransactionType::Transfers(_)
                    | TransactionType::Burn(_)
                    | TransactionType::MultiSig(_)
                    | TransactionType::InvokeContract(_)
                    | TransactionType::DeployContract(_)
                    | TransactionType::Energy(_)
                    | TransactionType::AIMining(_) => true,
                }
            }
        }
    }

    /// Get the new output ciphertext
    /// This is used to substract the amount from the sender's balance

    /// Get the new output amounts for the sender
    /// Balance simplification: Returns plain u64 amounts instead of ciphertexts
    pub fn get_expected_sender_outputs<'a>(&'a self) -> Result<Vec<(&'a Hash, u64)>, DecompressionError> {
        // This method previously collected sender output ciphertexts for each asset
        // With plaintext balances, no commitments or ciphertexts needed
        // Return empty vector for now (amounts are handled directly in apply())
        let outputs = Vec::new();
        Ok(outputs)
    }

    // These methods were used for ZKP proof generation with Merlin transcripts
    // With plaintext balances, no transcripts or proofs needed
    // Kept as no-ops for now to maintain call sites during refactoring

    // Verify that the commitment assets match the assets used in the tx

    // This method previously decompressed private contract deposits for proof verification
    // With plaintext balances (ContractDeposit::Public only), no decompression needed
    //
    // Previous functionality:
    // 1. Validated deposits.len() <= MAX_DEPOSIT_PER_INVOKE_CALL
    // 2. Validated max_gas <= MAX_GAS_USAGE_PER_TX
    // 3. For Public deposits: validated amount > 0
    // 4. For Private deposits: decompressed commitment, sender_handle, receiver_handle
    //
    // New plaintext approach:
    // 1. Validate deposit count and max_gas limits
    // 2. Validate all deposits are Public with amount > 0
    // 3. No decompression needed
    fn verify_invoke_contract<'a, E>(
        &self,
        deposits: &'a IndexMap<Hash, ContractDeposit>,
        max_gas: u64
    ) -> Result<(), VerificationError<E>> {
        if deposits.len() > MAX_DEPOSIT_PER_INVOKE_CALL {
            return Err(VerificationError::DepositCount);
        }

        if max_gas > MAX_GAS_USAGE_PER_TX {
            return Err(VerificationError::MaxGasReached.into())
        }

        // Validate all deposits are public with non-zero amounts
        // Balance simplification: Validate plaintext deposits
        for (_asset, deposit) in deposits.iter() {
            if deposit.amount() == 0 {
                return Err(VerificationError::InvalidFormat);
            }
        }

        Ok(())
    }

    // This method previously verified CiphertextValidityProof for private contract deposits
    // With plaintext balances (ContractDeposit::Public only), no proof verification needed
    //
    // Previous functionality:
    // 1. Decompressed deposit commitments and handles
    // 2. Verified CiphertextValidityProof.pre_verify() for each private deposit
    // 3. Added commitments to value_commitments for range proof verification
    //
    // New plaintext approach (to be implemented):
    // 1. All deposits are ContractDeposit::Public(amount)
    // 2. Simply validate amount > 0 (already done in verify_invoke_contract)
    // 3. Deduct amount from sender balance
    // 4. Add amount to contract balance
    fn verify_contract_deposits<E>(
        &self,
        _source_decompressed: &PublicKey,
        _dest_pubkey: &PublicKey,
        deposits: &IndexMap<Hash, ContractDeposit>,
    ) -> Result<(), VerificationError<E>> {
        // Stub implementation - proof verification removed
        // In production, implement plaintext deposit verification here
        trace!("Skipping contract deposit proof verification (plaintext balances)");

        // Balance simplification: All deposits are now plaintext
        // Basic validation is performed in verify_invoke_contract
        let _ = (deposits, _source_decompressed, _dest_pubkey);  // Suppress unused warnings

        Ok(())
    }

    async fn verify_dynamic_parts<'a, E, B: BlockchainVerificationState<'a, E>>(
        &'a self,
        tx_hash: &'a Hash,
        state: &mut B,
    ) -> Result<(), VerificationError<E>> {
        // Balance simplification: No decompression needed for plaintext balances

        trace!("Pre-verifying transaction on state");
        state.pre_verify_tx(&self).await
            .map_err(VerificationError::State)?;

        // Atomically check and update nonce to prevent TOCTOU race condition
        let success = state.compare_and_swap_nonce(
            &self.source,
            self.nonce,        // Expected value
            self.nonce + 1     // New value
        ).await.map_err(VerificationError::State)?;

        if !success {
            // CAS failed, get current nonce for error reporting
            let current = state.get_account_nonce(&self.source).await
                .map_err(VerificationError::State)?;
            return Err(VerificationError::InvalidNonce(
                tx_hash.clone(),
                current,
                self.nonce
            ));
        }

        match &self.data {
            TransactionType::Transfers(_transfers) => {
                // Balance simplification: No decompression needed
            },
            TransactionType::Burn(_) => {},
            TransactionType::MultiSig(payload) => {
                let is_reset = payload.threshold == 0 && payload.participants.is_empty();
                // If the multisig is reset, we need to check if it was already configured
                if is_reset && state.get_multisig_state(&self.source).await.map_err(VerificationError::State)?.is_none() {
                    return Err(VerificationError::MultiSigNotConfigured);
                }
            },
            TransactionType::InvokeContract(payload) => {
                self.verify_invoke_contract(
                    &payload.deposits,
                    payload.max_gas
                )?;

                // We need to load the contract module if not already in cache
                if !self.is_contract_available(state, &payload.contract).await? {
                    return Err(VerificationError::ContractNotFound);
                }

                let (module, environment) = state.get_contract_module_with_environment(&payload.contract).await
                    .map_err(VerificationError::State)?;

                if !module.is_entry_chunk(payload.chunk_id as usize) {
                    return Err(VerificationError::InvalidInvokeContract);
                }

                let validator = ModuleValidator::new(module, environment);
                for constant in payload.parameters.iter() {
                    validator.verify_constant(&constant)
                        .map_err(|err| VerificationError::ModuleError(format!("{:#}", err)))?;
                }
            },
            TransactionType::DeployContract(payload) => {
                if let Some(invoke) = payload.invoke.as_ref() {
                    self.verify_invoke_contract(
                        &invoke.deposits,
                        invoke.max_gas
                    )?;
                }

                let environment = state.get_environment().await
                    .map_err(VerificationError::State)?;

                let validator = ModuleValidator::new(&payload.module, environment);
                validator.verify()
                    .map_err(|err| VerificationError::ModuleError(format!("{:#}", err)))?;
            },
            TransactionType::Energy(payload) => {
                match payload {
                    EnergyPayload::FreezeTos { amount, duration } => {
                        if *amount == 0 {
                            return Err(VerificationError::AnyError(anyhow!("Freeze amount must be greater than zero")));
                        }

                        if *amount % crate::config::COIN_VALUE != 0 {
                            return Err(VerificationError::AnyError(anyhow!("Freeze amount must be a whole number of TOS")));
                        }

                        if *amount < crate::config::MIN_FREEZE_TOS_AMOUNT {
                            return Err(VerificationError::AnyError(anyhow!("Freeze amount must be at least 1 TOS")));
                        }

                        if !duration.is_valid() {
                            return Err(VerificationError::AnyError(anyhow!("Freeze duration must be between 3 and 180 days")));
                        }
                    },
                    EnergyPayload::UnfreezeTos { amount } => {
                        if *amount == 0 {
                            return Err(VerificationError::AnyError(anyhow!("Unfreeze amount must be greater than zero")));
                        }

                        if *amount % crate::config::COIN_VALUE != 0 {
                            return Err(VerificationError::AnyError(anyhow!("Unfreeze amount must be a whole number of TOS")));
                        }

                        if *amount < crate::config::MIN_UNFREEZE_TOS_AMOUNT {
                            return Err(VerificationError::AnyError(anyhow!("Unfreeze amount must be at least 1 TOS")));
                        }
                    }
                }
            },
            TransactionType::AIMining(_) => {
                // AI Mining transactions don't require special verification beyond basic checks for now
            }
        };

        // With plaintext balances, we no longer need Pedersen commitments or CommitmentEqProof
        // Instead, we'll directly verify balances using simple arithmetic

        // This section previously verified:
        // 1. Decompressed source commitments
        // 2. CommitmentEqProof for each asset
        // 3. Updated sender balances in state

        // New plaintext approach (to be implemented):
        // 1. Get current balance for each asset from state
        // 2. Calculate expected new balance = current - (transfers + deposits + fees)
        // 3. Verify new balance >= 0
        // 4. Update state with new balance

        Ok(())
    }

    // This method no longer needs to return transcript or commitments
    // Signature kept for compatibility during refactoring
    async fn pre_verify<'a, E, B: BlockchainVerificationState<'a, E>>(
        &'a self,
        tx_hash: &'a Hash,
        state: &mut B,
    ) -> Result<(), VerificationError<E>>
    {
        trace!("Pre-verifying transaction");
        if !self.has_valid_version_format() {
            return Err(VerificationError::InvalidFormat);
        }

        // Validate that Energy fee type can only be used with Transfer transactions
        if self.get_fee_type().is_energy() {
            if !matches!(self.data, TransactionType::Transfers(_)) {
                return Err(VerificationError::InvalidFormat);
            }
            
            // Validate that Energy fee type cannot be used for transfers to new addresses
            if let TransactionType::Transfers(transfers) = &self.data {
                for transfer in transfers {
                    // Try to get the account nonce to check if account exists
                    // If account doesn't exist, this will fail with AccountNotFound error
                    let _nonce = state.get_account_nonce(transfer.get_destination()).await
                        .map_err(|_| VerificationError::InvalidFormat)?;
                }
            }
        }

        trace!("Pre-verifying transaction on state");
        state.pre_verify_tx(&self).await
            .map_err(VerificationError::State)?;

        // Atomically check and update nonce to prevent TOCTOU race condition
        let success = state.compare_and_swap_nonce(
            &self.source,
            self.nonce,        // Expected value
            self.nonce + 1     // New value
        ).await.map_err(VerificationError::State)?;

        if !success {
            // CAS failed, get current nonce for error reporting
            let current = state.get_account_nonce(&self.source).await
                .map_err(VerificationError::State)?;
            return Err(VerificationError::InvalidNonce(
                tx_hash.clone(),
                current,
                self.nonce
            ));
        }

        // Balance simplification: No commitment verification needed
        // Balance simplification: No decompression needed for plaintext balances

        match &self.data {
            TransactionType::Transfers(transfers) => {
                if transfers.len() > MAX_TRANSFER_COUNT || transfers.is_empty() {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("incorrect transfers size: {}", transfers.len());
                    }
                    return Err(VerificationError::TransferCount);
                }

                let mut extra_data_size = 0;
                // Prevent sending to ourself
                for transfer in transfers.iter() {
                    if *transfer.get_destination() == self.source {
                        debug!("sender cannot be the receiver in the same TX");
                        return Err(VerificationError::SenderIsReceiver);
                    }

                    if let Some(extra_data) = transfer.get_extra_data() {
                        let size = extra_data.size();
                        if size > EXTRA_DATA_LIMIT_SIZE {
                            return Err(VerificationError::TransferExtraDataSize);
                        }
                        extra_data_size += size;
                    }

                    // Balance simplification: No decompression needed
                }

                // Check the sum of extra data size
                if extra_data_size > EXTRA_DATA_LIMIT_SUM_SIZE {
                    return Err(VerificationError::TransactionExtraDataSize);
                }
            },
            TransactionType::Burn(payload) => {
                let fee = self.fee;
                let amount = payload.amount;

                if amount == 0 {
                    return Err(VerificationError::InvalidFormat);
                }

                let total = fee.checked_add(amount)
                    .ok_or(VerificationError::InvalidFormat)?;

                if total < fee || total < amount {
                    return Err(VerificationError::InvalidFormat);
                }
            },
            TransactionType::MultiSig(payload) => {
                if payload.participants.len() > MAX_MULTISIG_PARTICIPANTS {
                    return Err(VerificationError::MultiSigParticipants);
                }

                // Threshold should be less than or equal to the number of participants
                if payload.threshold as usize > payload.participants.len() {
                    return Err(VerificationError::MultiSigThreshold);
                }

                // If the threshold is set to 0, while we have participants, its invalid
                // Threshold should be always > 0
                if payload.threshold == 0 && !payload.participants.is_empty() {
                    return Err(VerificationError::MultiSigThreshold);
                }

                // You can't contains yourself in the participants
                if payload.participants.contains(self.get_source()) {
                    return Err(VerificationError::MultiSigParticipants);
                }

                let is_reset = payload.threshold == 0 && payload.participants.is_empty();
                // If the multisig is reset, we need to check if it was already configured
                if is_reset && state.get_multisig_state(&self.source).await.map_err(VerificationError::State)?.is_none() {
                    return Err(VerificationError::MultiSigNotConfigured);
                }
            },
            TransactionType::InvokeContract(payload) => {
                self.verify_invoke_contract(
                    &payload.deposits,
                    payload.max_gas
                )?;

                // We need to load the contract module if not already in cache
                if !self.is_contract_available(state, &payload.contract).await? {
                    return Err(VerificationError::ContractNotFound);
                }

                let (module, environment) = state.get_contract_module_with_environment(&payload.contract).await
                    .map_err(VerificationError::State)?;

                if !module.is_entry_chunk(payload.chunk_id as usize) {
                    return Err(VerificationError::InvalidInvokeContract);
                }

                let validator = ModuleValidator::new(module, environment);
                for constant in payload.parameters.iter() {
                    validator.verify_constant(&constant)
                        .map_err(|err| VerificationError::ModuleError(format!("{:#}", err)))?;
                }
            },
            TransactionType::DeployContract(payload) => {
                if let Some(invoke) = payload.invoke.as_ref() {
                    self.verify_invoke_contract(
                        &invoke.deposits,
                        invoke.max_gas
                    )?;
                }

                let environment = state.get_environment().await
                    .map_err(VerificationError::State)?;

                let validator = ModuleValidator::new(&payload.module, environment);
                validator.verify()
                    .map_err(|err| VerificationError::ModuleError(format!("{:#}", err)))?;
            },
            TransactionType::Energy(_) => {
                // Energy transactions don't require special verification beyond basic checks
            },
            TransactionType::AIMining(_) => {
                // AI Mining transactions don't require special verification beyond basic checks for now
            }
        };

        let source_decompressed = self
            .source
            .decompress()
            .map_err(|err| VerificationError::Proof(err.into()))?;

        // 0.a Verify Signature
        let bytes = self.get_signing_bytes();
        if !self.signature.verify(&bytes, &source_decompressed) {
            debug!("transaction signature is invalid");
            return Err(VerificationError::InvalidSignature);
        }

        // 0.b Verify multisig
        if let Some(config) = state.get_multisig_state(&self.source).await.map_err(VerificationError::State)? {
            let Some(multisig) = self.get_multisig() else {
                return Err(VerificationError::MultiSigNotFound);
            };

            if (config.threshold as usize) != multisig.len() || multisig.len() > MAX_MULTISIG_PARTICIPANTS {
                return Err(VerificationError::MultiSigParticipants);
            }

            // Multisig participants sign the transaction data without the multisig field
            let multisig_bytes = self.get_multisig_signing_bytes();
            let hash = hash(&multisig_bytes);
            for sig in multisig.get_signatures() {
                // A participant can't sign more than once because of the IndexSet (SignatureId impl Hash on id)
                let index = sig.id as usize;
                let Some(key) = config.participants.get_index(index) else {
                    return Err(VerificationError::MultiSigParticipants);
                };

                let decompressed = key.decompress().map_err(ProofVerificationError::from)?;
                if !sig.signature.verify(hash.as_bytes(), &decompressed) {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Multisig signature verification failed for participant {}", index);
                    }
                    return Err(VerificationError::InvalidSignature);
                }
            }
        } else if self.get_multisig().is_some() {
            return Err(VerificationError::MultiSigNotConfigured);
        }

        // Balance verification handled by plaintext balance system
        trace!("Balance verification handled by plaintext balance system");

        // With plaintext balances, we no longer need:
        // - CiphertextValidityProof verification
        // - Pedersen commitment verification
        // - Twisted ElGamal decrypt handle verification

        // New plaintext approach (to be implemented):
        // For each transfer:
        // 1. Get receiver's current balance from state
        // 2. Add transfer.amount to receiver balance
        // 3. Update state with new receiver balance

        trace!("Processing transfers with plaintext amounts");

        match &self.data {
            TransactionType::Transfers(_transfers) => {
                // TODO: Implement plaintext transfer verification
                // for transfer in transfers {
                //     let current_balance = state.get_receiver_balance(transfer.destination, transfer.asset).await?;
                //     let new_balance = current_balance + transfer.amount;
                //     state.update_receiver_balance(transfer.destination, transfer.asset, new_balance).await?;
                // }
            },
            TransactionType::Burn(_payload) => {
            },
            TransactionType::MultiSig(payload) => {
                // Setup the multisig
                state.set_multisig_state(&self.source, payload).await
                    .map_err(VerificationError::State)?;
            },
            TransactionType::InvokeContract(payload) => {
                let dest_pubkey = PublicKey::from_hash(&payload.contract);
                self.verify_contract_deposits(
                    &source_decompressed,
                    &dest_pubkey,
                    &payload.deposits,
                )?;
            },
            TransactionType::DeployContract(payload) => {
                // Verify that if we have a constructor, we must have an invoke, and vice-versa
                if payload.invoke.is_none() != payload.module.get_chunk_id_of_hook(0).is_none() {
                    return Err(VerificationError::InvalidFormat);
                }

                if let Some(invoke) = payload.invoke.as_ref() {
                    let dest_pubkey = PublicKey::from_hash(&tx_hash);
                    self.verify_contract_deposits(
                        &source_decompressed,
                        &dest_pubkey,
                        &invoke.deposits,
                    )?;
                }

                state.set_contract_module(tx_hash, &payload.module).await
                    .map_err(VerificationError::State)?;
            },
            TransactionType::Energy(payload) => {
                if log::log_enabled!(log::Level::Debug) {
                    debug!("Energy transaction verification - payload: {:?}, fee: {}, nonce: {}",
                           payload, self.fee, self.nonce);
                }
            },
            TransactionType::AIMining(payload) => {
                if log::log_enabled!(log::Level::Debug) {
                    debug!("AI Mining transaction verification - payload: {:?}, fee: {}, nonce: {}",
                           payload, self.fee, self.nonce);
                }
            }
        }

        // With plaintext balances, we don't need Bulletproofs range proofs
        // Balances are plain u64, always in valid range [0, 2^64)
        trace!("Skipping range proof verification (plaintext balances)");

        Ok(())
    }

    pub async fn verify_batch<'a, H, E, B, C>(
        txs: impl Iterator<Item = &'a (Arc<Transaction>, H)>,
        state: &mut B,
        cache: &C,
    ) -> Result<(), VerificationError<E>>
    where
        H: AsRef<Hash> + 'a,
        B: BlockchainVerificationState<'a, E>,
        C: ZKPCache<E>
    {
        trace!("Verifying batch of transactions");
        for (tx, hash) in txs {
            let hash = hash.as_ref();

            // In case the cache already know this TX
            // we don't need to spend time reverifying it again
            // because a TX is immutable, we can just verify the mutable parts
            // (balance & nonce related)
            let dynamic_parts_only = cache.is_already_verified(hash).await
                .map_err(VerificationError::State)?;
            if dynamic_parts_only {
                if log::log_enabled!(log::Level::Debug) {
                    debug!("TX {} is known from ZKPCache, verifying dynamic parts only", hash);
                }
                tx.verify_dynamic_parts(hash, state).await?;
            } else {
                tx.pre_verify(hash, state).await?;
            }
        }

        // With plaintext balances, no ZK proofs to verify
        trace!("Skipping batch proof verification (plaintext balances)");

        Ok(())
    }

    /// Verify one transaction. Use `verify_batch` to verify a batch of transactions.
    pub async fn verify<'a, E, B, C>(
        self: &'a Arc<Self>,
        tx_hash: &'a Hash,
        state: &mut B,
        cache: &C,
    ) -> Result<(), VerificationError<E>>
    where
        B: BlockchainVerificationState<'a, E>,
        C: ZKPCache<E>
    {
        let dynamic_parts_only = cache.is_already_verified(tx_hash).await
            .map_err(VerificationError::State)?;
        if dynamic_parts_only {
            if log::log_enabled!(log::Level::Debug) {
                debug!("TX {} is known from ZKPCache, verifying dynamic parts only", tx_hash);
            }
            self.verify_dynamic_parts(tx_hash, state).await?;
        }
        else {
            self.pre_verify(tx_hash, state).await?;
        };

        // With plaintext balances, no ZK proof verification needed
        trace!("Skipping proof verification (plaintext balances)");

        Ok(())
    }

    // Apply the transaction to the state
    // Arc is required around Self to be shared easily into the VM if needed
    async fn apply<'a, P: ContractProvider, E, B: BlockchainApplyState<'a, P, E>>(
        self: &'a Arc<Self>,
        tx_hash: &'a Hash,
        state: &mut B,
    ) -> Result<(), VerificationError<E>> {
        trace!("Applying transaction data");
        // Update nonce
        state.update_account_nonce(self.get_source(), self.nonce + 1).await
            .map_err(VerificationError::State)?;

        // Handle energy consumption if this transaction uses energy for fees
        if self.get_fee_type().is_energy() {
            // Only transfer transactions can use energy fees
            if let TransactionType::Transfers(_) = &self.data {
                let energy_cost = self.calculate_energy_cost();
                
                // Get user's energy resource
                let energy_resource = state.get_energy_resource(&self.source).await
                    .map_err(VerificationError::State)?;
                
                if let Some(mut energy_resource) = energy_resource {
                    // Check if user has enough energy
                    if !energy_resource.has_enough_energy(energy_cost) {
                        return Err(VerificationError::InsufficientEnergy(energy_cost));
                    }
                    
                    // Consume energy
                    energy_resource.consume_energy(energy_cost)
                        .map_err(|_| VerificationError::InsufficientEnergy(energy_cost))?;
                    
                    // Update energy resource in state
                    state.set_energy_resource(&self.source, energy_resource).await
                        .map_err(VerificationError::State)?;
                    
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Consumed {} energy for transaction {}", energy_cost, tx_hash);
                    }
                } else {
                    return Err(VerificationError::InsufficientEnergy(energy_cost));
                }
            }
        }

        // Apply receiver balances
        match &self.data {
            TransactionType::Transfers(transfers) => {
                for transfer in transfers {
                    // Update receiver balance with plain u64 amount
                    let current_balance = state
                        .get_receiver_balance(
                            Cow::Borrowed(transfer.get_destination()),
                            Cow::Borrowed(transfer.get_asset()),
                        ).await
                        .map_err(VerificationError::State)?;

                    // Balance simplification: Add plain u64 amount to receiver's balance
                    let plain_amount = transfer.get_amount();
                    *current_balance = current_balance.checked_add(plain_amount)
                        .ok_or(VerificationError::Overflow)?;
                }
            },
            TransactionType::Burn(payload) => {
                if payload.asset == TOS_ASSET {
                    state.add_burned_coins(payload.amount).await
                        .map_err(VerificationError::State)?;
                }
            },
            TransactionType::MultiSig(payload) => {
                state.set_multisig_state(&self.source, payload).await.map_err(VerificationError::State)?;
            },
            TransactionType::InvokeContract(payload) => {
                if self.is_contract_available(state, &payload.contract).await? {
                    self.invoke_contract(
                        tx_hash,
                        state,
                        &payload.contract,
                        &payload.deposits,
                        payload.parameters.iter().cloned(),
                        payload.max_gas,
                        InvokeContract::Entry(payload.chunk_id)
                    ).await?;
                } else {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Contract {} invoked from {} not available", payload.contract, tx_hash);
                    }

                    // Nothing was spent, we must refund the gas and deposits
                    self.handle_gas(state, 0, payload.max_gas).await?;
                    self.refund_deposits(state, &payload.deposits).await?;
                }
            },
            TransactionType::DeployContract(payload) => {
                state.set_contract_module(tx_hash, &payload.module).await
                    .map_err(VerificationError::State)?;

                if let Some(invoke) = payload.invoke.as_ref() {
                    let is_success = self.invoke_contract(
                        tx_hash,
                        state,
                        tx_hash,
                        &invoke.deposits,
                        iter::empty(),
                        invoke.max_gas,
                        InvokeContract::Hook(0)
                    ).await?;

                    // if it has failed, we don't want to deploy the contract
                    // TODO: we must handle this carefully
                    if !is_success {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Contract deploy for {} failed", tx_hash);
                        }
                        state.remove_contract_module(tx_hash).await
                            .map_err(VerificationError::State)?;
                    }
                }
            },
            TransactionType::Energy(payload) => {
                // Handle energy operations (freeze/unfreeze TOS)
                match payload {
                    EnergyPayload::FreezeTos { amount, duration } => {
                        // Get current energy resource for the account
                        let energy_resource = state.get_energy_resource(&self.source).await
                            .map_err(VerificationError::State)?;
                        
                        let mut energy_resource = energy_resource.unwrap_or_else(EnergyResource::new);
                        
                        // Freeze TOS for energy - get topoheight from the blockchain state
                        let topoheight = state.get_block().get_blue_score() as u64; // Use blue_score for consensus
                        energy_resource.freeze_tos_for_energy(*amount, duration.clone(), topoheight);
                        
                        // Update energy resource in state
                        state.set_energy_resource(&self.source, energy_resource).await
                            .map_err(VerificationError::State)?;
                        
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("FreezeTos applied: {} TOS frozen for {} duration, energy gained: {} units",
                                   amount, duration.name(), (*amount / crate::config::COIN_VALUE) * duration.reward_multiplier());
                        }
                    },
                    EnergyPayload::UnfreezeTos { amount } => {
                        // Get current energy resource for the account
                        let energy_resource = state.get_energy_resource(&self.source).await
                            .map_err(VerificationError::State)?;
                        
                        if let Some(mut energy_resource) = energy_resource {
                            // Unfreeze TOS - get topoheight from the blockchain state
                            let topoheight = state.get_block().get_blue_score() as u64; // Use blue_score for consensus
                            energy_resource.unfreeze_tos(*amount, topoheight)
                                .map_err(|_| VerificationError::AnyError(anyhow::anyhow!("Invalid energy operation")))?;
                            
                            // Update energy resource in state
                            state.set_energy_resource(&self.source, energy_resource).await
                                .map_err(VerificationError::State)?;
                            
                            if log::log_enabled!(log::Level::Debug) {
                                debug!("UnfreezeTos applied: {} TOS unfrozen, energy removed: {} units", amount, amount);
                            }
                        } else {
                            return Err(VerificationError::AnyError(anyhow::anyhow!("Invalid energy operation")));
                        }
                    }
                }
            },
            TransactionType::AIMining(payload) => {
                // Handle AI Mining operations with full validation
                use crate::ai_mining::AIMiningValidator;

                // Get or create AI mining state
                let mut ai_mining_state = state.get_ai_mining_state().await
                    .map_err(VerificationError::State)?
                    .unwrap_or_default();

                // Create validator with current context
                let block_height = state.get_block().get_blue_score() as u64;
                let timestamp = state.get_block().get_timestamp();
                let source = self.source.clone();

                let result = {
                    let mut validator = AIMiningValidator::new(
                        &mut ai_mining_state,
                        block_height,
                        timestamp,
                        source,
                    );

                    // Validate and apply the AI mining operation
                    validator.validate_and_apply(payload)
                        .map_err(|e| VerificationError::AnyError(anyhow::anyhow!("AI Mining validation failed: {}", e)))?;

                    // Update tasks and process completions
                    validator.update_tasks()
                        .map_err(|e| VerificationError::AnyError(anyhow::anyhow!("AI Mining task update failed: {}", e)))?;

                    validator.get_validation_summary()
                };

                // Save updated state back to blockchain
                state.set_ai_mining_state(&ai_mining_state).await
                    .map_err(VerificationError::State)?;

                if log::log_enabled!(log::Level::Debug) {
                    debug!("AI Mining operation processed - payload: {:?}, miners: {}, active_tasks: {}, completed_tasks: {}",
                           payload, result.total_miners, result.active_tasks, result.completed_tasks);
                }
            }
        }

        Ok(())
    }

    /// Assume the tx is valid, apply it to `state`. May panic if a ciphertext is ill-formed.
    pub async fn apply_without_verify<'a, P: ContractProvider, E, B: BlockchainApplyState<'a, P, E>>(
        self: &'a Arc<Self>,
        tx_hash: &'a Hash,
        state: &mut B,
    ) -> Result<(), VerificationError<E>> {
        // Balance simplification: No decompression needed for plaintext balances
        // Private deposits are not supported, only Public deposits with plain u64 amounts

        // This section previously processed source commitments for each asset
        // With plaintext balances, no commitments to process
        //
        // Previous functionality:
        // for commitment in &self.source_commitments {
        //     let asset = commitment.get_asset();
        //     let current_source_balance = state.get_sender_balance(...).await?;
        //     let output = self.get_sender_output_ct(asset, &transfers_decompressed, &deposits_decompressed)?;
        //     *current_source_balance -= &output;
        //     state.add_sender_output(&self.source, asset, output).await?;
        // }
        //
        // New plaintext approach (implemented in apply()):
        // For each asset used in transaction:
        // 1. Get current sender balance (plain u64) from state
        // 2. Calculate total spent = sum(transfer.amount) + sum(deposit.amount)
        // 3. Verify balance >= total spent (done elsewhere in verification)
        // 4. Update state: new_balance = balance - total_spent
        trace!("Skipping source commitment processing (plaintext balances)");

        self.apply(tx_hash, state).await
    }

    /// Verify only that the final sender balance is the expected one for each commitment
    /// Then apply ciphertexts to the state
    /// Checks done are: commitment eq proofs only
    pub async fn apply_with_partial_verify<'a, P: ContractProvider, E, B: BlockchainApplyState<'a, P, E>>(
        self: &'a Arc<Self>,
        tx_hash: &'a Hash,
        state: &mut B
    ) -> Result<(), VerificationError<E>> {
        trace!("apply with partial verify");

        // Balance simplification: No decompression needed for plaintext balances
        // Private deposits are not supported, only Public deposits with plain u64 amounts
        trace!("Partial verify with plaintext balances - no proof verification needed");

        self.apply(tx_hash, state).await
    }
}
