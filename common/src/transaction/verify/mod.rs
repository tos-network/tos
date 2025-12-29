mod contract;
mod error;
mod kyc;
mod state;
mod zkp_cache;

use std::{borrow::Cow, iter, sync::Arc};

use anyhow::anyhow;
// Balance simplification: RangeProof removed
// use bulletproofs::RangeProof;
use indexmap::IndexMap;
use log::{debug, trace};
use tos_kernel::ModuleValidator;

use super::{payload::EnergyPayload, ContractDeposit, Transaction, TransactionType};
use crate::{
    account::EnergyResource,
    config::{BURN_PER_CONTRACT, MAX_GAS_USAGE_PER_TX, TOS_ASSET},
    contract::ContractProvider,
    crypto::{
        elgamal::{DecompressionError, PublicKey},
        hash,
        proofs::ProofVerificationError,
        Hash,
        // Balance simplification: ProtocolTranscript removed - no longer needed
    },
    serializer::Serializer,
    transaction::{
        TxVersion, EXTRA_DATA_LIMIT_SIZE, EXTRA_DATA_LIMIT_SUM_SIZE, MAX_DEPOSIT_PER_INVOKE_CALL,
        MAX_MULTISIG_PARTICIPANTS, MAX_TRANSFER_COUNT,
    },
};
use contract::InvokeContract;

pub use error::*;
pub use state::*;
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
                    | TransactionType::AIMining(_)
                    | TransactionType::BindReferrer(_)
                    | TransactionType::BatchReferralReward(_)
                    // KYC transaction types
                    | TransactionType::SetKyc(_)
                    | TransactionType::RevokeKyc(_)
                    | TransactionType::RenewKyc(_)
                    | TransactionType::TransferKyc(_)
                    | TransactionType::AppealKyc(_)
                    | TransactionType::BootstrapCommittee(_)
                    | TransactionType::RegisterCommittee(_)
                    | TransactionType::UpdateCommittee(_)
                    | TransactionType::EmergencySuspend(_) => true,
                }
            }
        }
    }

    /// Get the new output ciphertext
    /// This is used to substract the amount from the sender's balance
    /// Get the new output amounts for the sender
    /// Balance simplification: Returns plain u64 amounts instead of ciphertexts
    pub fn get_expected_sender_outputs(&self) -> Result<Vec<(&Hash, u64)>, DecompressionError> {
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
    fn verify_invoke_contract<E>(
        &self,
        deposits: &IndexMap<Hash, ContractDeposit>,
        max_gas: u64,
    ) -> Result<(), VerificationError<E>> {
        if deposits.len() > MAX_DEPOSIT_PER_INVOKE_CALL {
            return Err(VerificationError::DepositCount);
        }

        if max_gas > MAX_GAS_USAGE_PER_TX {
            return Err(VerificationError::MaxGasReached);
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
        let _ = (deposits, _source_decompressed, _dest_pubkey); // Suppress unused warnings

        Ok(())
    }

    async fn verify_dynamic_parts<'a, E, B: BlockchainVerificationState<'a, E>>(
        &'a self,
        tx_hash: &'a Hash,
        state: &mut B,
    ) -> Result<(), VerificationError<E>> {
        // Balance simplification: No decompression needed for plaintext balances

        trace!("Pre-verifying transaction on state");
        state
            .pre_verify_tx(self)
            .await
            .map_err(VerificationError::State)?;

        // Atomically check and update nonce to prevent TOCTOU race condition
        let success = state
            .compare_and_swap_nonce(
                &self.source,
                self.nonce,     // Expected value
                self.nonce + 1, // New value
            )
            .await
            .map_err(VerificationError::State)?;

        if !success {
            // CAS failed, get current nonce for error reporting
            let current = state
                .get_account_nonce(&self.source)
                .await
                .map_err(VerificationError::State)?;
            return Err(VerificationError::InvalidNonce(
                tx_hash.clone(),
                self.nonce,
                current,
            ));
        }

        match &self.data {
            TransactionType::Transfers(_transfers) => {
                // Balance simplification: No decompression needed
            }
            TransactionType::Burn(_) => {}
            TransactionType::MultiSig(payload) => {
                let is_reset = payload.threshold == 0 && payload.participants.is_empty();
                // If the multisig is reset, we need to check if it was already configured
                if is_reset
                    && state
                        .get_multisig_state(&self.source)
                        .await
                        .map_err(VerificationError::State)?
                        .is_none()
                {
                    return Err(VerificationError::MultiSigNotConfigured);
                }
            }
            TransactionType::InvokeContract(payload) => {
                self.verify_invoke_contract(&payload.deposits, payload.max_gas)?;

                // We need to load the contract module if not already in cache
                if !self.is_contract_available(state, &payload.contract).await? {
                    return Err(VerificationError::ContractNotFound);
                }

                let (module, environment) = state
                    .get_contract_module_with_environment(&payload.contract)
                    .await
                    .map_err(VerificationError::State)?;

                // TAKO contracts: entry_id validation is handled by tos-tbpf at runtime
                // Just verify the parameters are valid ValueCells
                let validator = ModuleValidator::new(module, environment);
                for constant in payload.parameters.iter() {
                    validator
                        .verify_constant(constant)
                        .map_err(|err| VerificationError::ModuleError(format!("{err:#}")))?;
                }
            }
            TransactionType::DeployContract(payload) => {
                if let Some(invoke) = payload.invoke.as_ref() {
                    self.verify_invoke_contract(&invoke.deposits, invoke.max_gas)?;
                }

                let environment = state
                    .get_environment()
                    .await
                    .map_err(VerificationError::State)?;

                let validator = ModuleValidator::new(&payload.module, environment);
                validator
                    .verify()
                    .map_err(|err| VerificationError::ModuleError(format!("{err:#}")))?;
            }
            TransactionType::Energy(payload) => match payload {
                EnergyPayload::FreezeTos { amount, duration } => {
                    if *amount == 0 {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Freeze amount must be greater than zero"
                        )));
                    }

                    if *amount % crate::config::COIN_VALUE != 0 {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Freeze amount must be a whole number of TOS"
                        )));
                    }

                    if *amount < crate::config::MIN_FREEZE_TOS_AMOUNT {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Freeze amount must be at least 1 TOS"
                        )));
                    }

                    if !duration.is_valid() {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Freeze duration must be between 3 and 180 days"
                        )));
                    }
                }
                EnergyPayload::UnfreezeTos { amount } => {
                    if *amount == 0 {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Unfreeze amount must be greater than zero"
                        )));
                    }

                    if *amount % crate::config::COIN_VALUE != 0 {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Unfreeze amount must be a whole number of TOS"
                        )));
                    }

                    if *amount < crate::config::MIN_UNFREEZE_TOS_AMOUNT {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Unfreeze amount must be at least 1 TOS"
                        )));
                    }
                }
            },
            TransactionType::AIMining(_) => {
                // AI Mining transactions don't require special verification beyond basic checks for now
            }
            TransactionType::BindReferrer(_) => {
                // BindReferrer validation is handled by the referral provider
            }
            TransactionType::BatchReferralReward(payload) => {
                // BatchReferralReward validation
                if !payload.validate() {
                    return Err(VerificationError::AnyError(anyhow!(
                        "Invalid batch referral reward payload"
                    )));
                }
            }
            // KYC transaction types - structural validation (dynamic parts)
            // Uses state.get_verification_timestamp() for deterministic consensus validation
            // Block validation uses block timestamp; mempool uses system time
            TransactionType::SetKyc(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_set_kyc(payload, current_time)?;
            }
            TransactionType::RevokeKyc(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_revoke_kyc(payload, current_time)?;
            }
            TransactionType::RenewKyc(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_renew_kyc(payload, current_time)?;
            }
            TransactionType::TransferKyc(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_transfer_kyc(payload, current_time)?;
            }
            TransactionType::BootstrapCommittee(payload) => {
                kyc::verify_bootstrap_committee(payload)?;
            }
            TransactionType::RegisterCommittee(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_register_committee(payload, current_time)?;
            }
            TransactionType::UpdateCommittee(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_update_committee(payload, current_time)?;
            }
            TransactionType::EmergencySuspend(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_emergency_suspend(payload, current_time)?;
            }
            TransactionType::AppealKyc(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_appeal_kyc(payload, current_time)?;
            }
        };

        // SECURITY FIX: Verify sender has sufficient balance for all spending
        // Calculate total spending per asset
        // Use references to original Hash values in transaction (they live for 'a)
        let mut spending_per_asset: IndexMap<&'a Hash, u64> = IndexMap::new();

        match &self.data {
            TransactionType::Transfers(transfers) => {
                for transfer in transfers {
                    let asset = transfer.get_asset(); // Returns &Hash
                    let amount = transfer.get_amount();
                    let current = spending_per_asset.entry(asset).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::Burn(payload) => {
                let current = spending_per_asset.entry(&payload.asset).or_insert(0);
                *current = current
                    .checked_add(payload.amount)
                    .ok_or(VerificationError::Overflow)?;
            }
            TransactionType::InvokeContract(payload) => {
                // Add deposits
                for (asset, deposit) in &payload.deposits {
                    let amount = deposit
                        .get_amount()
                        .map_err(|e| VerificationError::AnyError(anyhow!(e)))?;
                    let current = spending_per_asset.entry(asset).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
                // Add max_gas to TOS spending
                let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                *current = current
                    .checked_add(payload.max_gas)
                    .ok_or(VerificationError::Overflow)?;
            }
            TransactionType::DeployContract(payload) => {
                // Add BURN_PER_CONTRACT to TOS spending
                let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                *current = current
                    .checked_add(BURN_PER_CONTRACT)
                    .ok_or(VerificationError::Overflow)?;

                // If invoking constructor, add deposits and max_gas
                if let Some(invoke) = &payload.invoke {
                    for (asset, deposit) in &invoke.deposits {
                        let amount = deposit
                            .get_amount()
                            .map_err(|e| VerificationError::AnyError(anyhow!(e)))?;
                        let current = spending_per_asset.entry(asset).or_insert(0);
                        *current = current
                            .checked_add(amount)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    // Add max_gas to TOS spending
                    let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                    *current = current
                        .checked_add(invoke.max_gas)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::Energy(payload) => {
                match payload {
                    EnergyPayload::FreezeTos { amount, .. } => {
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(*amount)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    EnergyPayload::UnfreezeTos { .. } => {
                        // Unfreeze doesn't spend, it releases frozen funds
                    }
                }
            }
            TransactionType::MultiSig(_)
            | TransactionType::AIMining(_)
            | TransactionType::BindReferrer(_)
            // KYC transactions don't spend assets directly (only fee)
            | TransactionType::SetKyc(_)
            | TransactionType::RevokeKyc(_)
            | TransactionType::RenewKyc(_)
            | TransactionType::TransferKyc(_)
            | TransactionType::AppealKyc(_)
            | TransactionType::BootstrapCommittee(_)
            | TransactionType::RegisterCommittee(_)
            | TransactionType::UpdateCommittee(_)
            | TransactionType::EmergencySuspend(_) => {
                // No asset spending for these types
            }
            TransactionType::BatchReferralReward(payload) => {
                // BatchReferralReward spends total_amount of the specified asset
                let current = spending_per_asset.entry(payload.get_asset()).or_insert(0);
                *current = current
                    .checked_add(payload.get_total_amount())
                    .ok_or(VerificationError::Overflow)?;
            }
        };

        // Add fee to TOS spending (unless using energy fee)
        if !self.get_fee_type().is_energy() {
            let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
            *current = current
                .checked_add(self.fee)
                .ok_or(VerificationError::Overflow)?;
        }

        // Verify sender has sufficient balance for each asset
        // CRITICAL: Mutate balance during verification (like old encrypted balance code)
        // This ensures mempool verification reduces cached balances, preventing
        // users from submitting sequential transactions that total more than their funds
        for (asset_hash, total_spending) in &spending_per_asset {
            // Use transaction's reference for balance check (pre-transaction state)
            let current_balance = state
                .get_sender_balance(&self.source, asset_hash, &self.reference)
                .await
                .map_err(VerificationError::State)?;

            if *current_balance < *total_spending {
                return Err(VerificationError::InsufficientFunds {
                    available: *current_balance,
                    required: *total_spending,
                });
            }

            // Deduct spending from balance immediately (matches old behavior)
            // This mutation updates mempool cached balances so subsequent txs see reduced funds
            *current_balance = current_balance
                .checked_sub(*total_spending)
                .ok_or(VerificationError::Overflow)?;
        }

        // Credit unfrozen TOS immediately in verification state (mempool/ChainState)
        if let TransactionType::Energy(EnergyPayload::UnfreezeTos { amount }) = &self.data {
            let balance = state
                .get_receiver_balance(Cow::Borrowed(self.get_source()), Cow::Borrowed(&TOS_ASSET))
                .await
                .map_err(VerificationError::State)?;

            *balance = balance
                .checked_add(*amount)
                .ok_or(VerificationError::Overflow)?;
        }

        Ok(())
    }

    // This method no longer needs to return transcript or commitments
    // Signature kept for compatibility during refactoring
    async fn pre_verify<'a, E, B: BlockchainVerificationState<'a, E>>(
        &'a self,
        tx_hash: &'a Hash,
        state: &mut B,
    ) -> Result<(), VerificationError<E>> {
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
                    let _nonce = state
                        .get_account_nonce(transfer.get_destination())
                        .await
                        .map_err(|_| VerificationError::InvalidFormat)?;
                }
            }
        }

        trace!("Pre-verifying transaction on state");
        state
            .pre_verify_tx(self)
            .await
            .map_err(VerificationError::State)?;

        // Atomically check and update nonce to prevent TOCTOU race condition
        let success = state
            .compare_and_swap_nonce(
                &self.source,
                self.nonce,     // Expected value
                self.nonce + 1, // New value
            )
            .await
            .map_err(VerificationError::State)?;

        if !success {
            // CAS failed, get current nonce for error reporting
            let current = state
                .get_account_nonce(&self.source)
                .await
                .map_err(VerificationError::State)?;
            return Err(VerificationError::InvalidNonce(
                tx_hash.clone(),
                self.nonce,
                current,
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
            }
            TransactionType::Burn(payload) => {
                let fee = self.fee;
                let amount = payload.amount;

                if amount == 0 {
                    return Err(VerificationError::InvalidFormat);
                }

                let total = fee
                    .checked_add(amount)
                    .ok_or(VerificationError::InvalidFormat)?;

                if total < fee || total < amount {
                    return Err(VerificationError::InvalidFormat);
                }
            }
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
                if is_reset
                    && state
                        .get_multisig_state(&self.source)
                        .await
                        .map_err(VerificationError::State)?
                        .is_none()
                {
                    return Err(VerificationError::MultiSigNotConfigured);
                }
            }
            TransactionType::InvokeContract(payload) => {
                self.verify_invoke_contract(&payload.deposits, payload.max_gas)?;

                // We need to load the contract module if not already in cache
                if !self.is_contract_available(state, &payload.contract).await? {
                    return Err(VerificationError::ContractNotFound);
                }

                let (module, environment) = state
                    .get_contract_module_with_environment(&payload.contract)
                    .await
                    .map_err(VerificationError::State)?;

                // TAKO contracts: entry_id validation is handled by tos-tbpf at runtime
                // Just verify the parameters are valid ValueCells
                let validator = ModuleValidator::new(module, environment);
                for constant in payload.parameters.iter() {
                    validator
                        .verify_constant(constant)
                        .map_err(|err| VerificationError::ModuleError(format!("{err:#}")))?;
                }
            }
            TransactionType::DeployContract(payload) => {
                if let Some(invoke) = payload.invoke.as_ref() {
                    self.verify_invoke_contract(&invoke.deposits, invoke.max_gas)?;
                }

                // Compute deterministic contract address using eBPF bytecode
                let bytecode = payload
                    .module
                    .get_bytecode()
                    .map(|b| b.to_vec())
                    .unwrap_or_default();

                let contract_address =
                    crate::crypto::compute_deterministic_contract_address(&self.source, &bytecode);

                // Check if contract already exists at this deterministic address
                if state
                    .load_contract_module(&contract_address)
                    .await
                    .map_err(VerificationError::State)?
                {
                    return Err(VerificationError::ContractAlreadyExists(
                        contract_address.clone(),
                    ));
                }

                // CRITICAL: Reserve address immediately to prevent front-running
                // This ensures subsequent TXs in the same block will see this address as occupied
                // during their pre-verification phase, blocking duplicate deployments
                state
                    .set_contract_module(&contract_address, &payload.module)
                    .await
                    .map_err(VerificationError::State)?;

                let environment = state
                    .get_environment()
                    .await
                    .map_err(VerificationError::State)?;

                let validator = ModuleValidator::new(&payload.module, environment);
                validator
                    .verify()
                    .map_err(|err| VerificationError::ModuleError(format!("{err:#}")))?;
            }
            TransactionType::Energy(_) => {
                // Energy transactions don't require special verification beyond basic checks
            }
            TransactionType::AIMining(_) => {
                // AI Mining transactions don't require special verification beyond basic checks for now
            }
            TransactionType::BindReferrer(_) => {
                // BindReferrer transactions are validated by the referral provider at execution time
            }
            TransactionType::BatchReferralReward(payload) => {
                // BatchReferralReward validation - check payload is valid
                if !payload.validate() {
                    return Err(VerificationError::AnyError(anyhow!(
                        "Invalid batch referral reward payload"
                    )));
                }
                // Authorization: sender must be the from_user (prevents abuse)
                if self.get_source() != payload.get_from_user() {
                    return Err(VerificationError::AnyError(anyhow!(
                        "BatchReferralReward sender must be the from_user"
                    )));
                }
            }
            // KYC transaction types - structural validation (pre-verify)
            // Uses state.get_verification_timestamp() for deterministic consensus validation
            TransactionType::SetKyc(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_set_kyc(payload, current_time)?;
            }
            TransactionType::RevokeKyc(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_revoke_kyc(payload, current_time)?;
            }
            TransactionType::RenewKyc(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_renew_kyc(payload, current_time)?;
            }
            TransactionType::TransferKyc(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_transfer_kyc(payload, current_time)?;
            }
            TransactionType::AppealKyc(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_appeal_kyc(payload, current_time)?;
            }
            TransactionType::BootstrapCommittee(payload) => {
                kyc::verify_bootstrap_committee(payload)?;
            }
            TransactionType::RegisterCommittee(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_register_committee(payload, current_time)?;
            }
            TransactionType::UpdateCommittee(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_update_committee(payload, current_time)?;
            }
            TransactionType::EmergencySuspend(payload) => {
                let current_time = state.get_verification_timestamp();
                kyc::verify_emergency_suspend(payload, current_time)?;
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
        if let Some(config) = state
            .get_multisig_state(&self.source)
            .await
            .map_err(VerificationError::State)?
        {
            let Some(multisig) = self.get_multisig() else {
                return Err(VerificationError::MultiSigNotFound);
            };

            if (config.threshold as usize) != multisig.len()
                || multisig.len() > MAX_MULTISIG_PARTICIPANTS
            {
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
                        debug!("Multisig signature verification failed for participant {index}");
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

        // NOTE: Transfer verification is implemented below in the spending_per_asset logic (lines 700-709)
        // where all transfer amounts are accumulated and verified against sender balances.
        match &self.data {
            TransactionType::Transfers(_transfers) => {
                // Transfer verification happens in spending_per_asset accumulation below
            }
            TransactionType::Burn(_payload) => {}
            TransactionType::MultiSig(payload) => {
                // Setup the multisig
                state
                    .set_multisig_state(&self.source, payload)
                    .await
                    .map_err(VerificationError::State)?;
            }
            TransactionType::InvokeContract(payload) => {
                let dest_pubkey = PublicKey::from_hash(&payload.contract);
                self.verify_contract_deposits(
                    &source_decompressed,
                    &dest_pubkey,
                    &payload.deposits,
                )?;
            }
            TransactionType::DeployContract(payload) => {
                // TAKO contracts: constructor invocation is optional
                // Entry point validation is handled by tos-tbpf at runtime

                // Compute deterministic contract address using eBPF bytecode
                let bytecode = payload
                    .module
                    .get_bytecode()
                    .map(|b| b.to_vec())
                    .unwrap_or_default();

                let contract_address =
                    crate::crypto::compute_deterministic_contract_address(&self.source, &bytecode);

                if let Some(invoke) = payload.invoke.as_ref() {
                    // Use deterministic contract address for deposit verification
                    let dest_pubkey = PublicKey::from_hash(&contract_address);
                    self.verify_contract_deposits(
                        &source_decompressed,
                        &dest_pubkey,
                        &invoke.deposits,
                    )?;
                }

                // Cache module under deterministic address (not tx_hash!)
                // Note: If pre-verification already reserved this address, this will succeed
                // because the module is already cached in mempool state
                // We check first to avoid unnecessary error handling
                if !state
                    .load_contract_module(&contract_address)
                    .await
                    .map_err(VerificationError::State)?
                {
                    // Address not yet cached (shouldn't happen after pre-verify, but handle it)
                    state
                        .set_contract_module(&contract_address, &payload.module)
                        .await
                        .map_err(VerificationError::State)?;
                }
            }
            TransactionType::Energy(payload) => {
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "Energy transaction verification - payload: {:?}, fee: {}, nonce: {}",
                        payload, self.fee, self.nonce
                    );
                }
            }
            TransactionType::AIMining(payload) => {
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "AI Mining transaction verification - payload: {:?}, fee: {}, nonce: {}",
                        payload, self.fee, self.nonce
                    );
                }
            }
            TransactionType::BindReferrer(payload) => {
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "BindReferrer transaction verification - referrer: {:?}, fee: {}, nonce: {}",
                        payload.get_referrer(), self.fee, self.nonce
                    );
                }
            }
            TransactionType::BatchReferralReward(payload) => {
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "BatchReferralReward verification - levels: {}, total_amount: {}, fee: {}",
                        payload.get_levels(),
                        payload.get_total_amount(),
                        self.fee
                    );
                }
            }
            // KYC transaction types
            TransactionType::SetKyc(_)
            | TransactionType::RevokeKyc(_)
            | TransactionType::RenewKyc(_)
            | TransactionType::TransferKyc(_)
            | TransactionType::AppealKyc(_)
            | TransactionType::BootstrapCommittee(_)
            | TransactionType::RegisterCommittee(_)
            | TransactionType::UpdateCommittee(_)
            | TransactionType::EmergencySuspend(_) => {
                // KYC transactions are logged at execution time
            }
        }

        // With plaintext balances, we don't need Bulletproofs range proofs
        // Balances are plain u64, always in valid range [0, 2^64)
        trace!("Skipping range proof verification (plaintext balances)");

        // SECURITY FIX: Check balances inline (can't call verify_dynamic_parts as it also does CAS nonce update)
        // Calculate total spending per asset and verify sender has sufficient balance
        let mut spending_per_asset: IndexMap<&'a Hash, u64> = IndexMap::new();

        match &self.data {
            TransactionType::Transfers(transfers) => {
                for transfer in transfers {
                    let asset = transfer.get_asset();
                    let amount = transfer.get_amount();
                    let current = spending_per_asset.entry(asset).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::Burn(payload) => {
                let current = spending_per_asset.entry(&payload.asset).or_insert(0);
                *current = current
                    .checked_add(payload.amount)
                    .ok_or(VerificationError::Overflow)?;
            }
            TransactionType::InvokeContract(payload) => {
                for (asset, deposit) in &payload.deposits {
                    let amount = deposit
                        .get_amount()
                        .map_err(|e| VerificationError::AnyError(anyhow!(e)))?;
                    let current = spending_per_asset.entry(asset).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
                // Add max_gas to TOS spending
                let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                *current = current
                    .checked_add(payload.max_gas)
                    .ok_or(VerificationError::Overflow)?;
            }
            TransactionType::DeployContract(payload) => {
                // Add BURN_PER_CONTRACT to TOS spending
                let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                *current = current
                    .checked_add(BURN_PER_CONTRACT)
                    .ok_or(VerificationError::Overflow)?;

                if let Some(invoke) = &payload.invoke {
                    for (asset, deposit) in &invoke.deposits {
                        let amount = deposit
                            .get_amount()
                            .map_err(|e| VerificationError::AnyError(anyhow!(e)))?;
                        let current = spending_per_asset.entry(asset).or_insert(0);
                        *current = current
                            .checked_add(amount)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    // Add max_gas to TOS spending
                    let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                    *current = current
                        .checked_add(invoke.max_gas)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::Energy(payload) => {
                match payload {
                    EnergyPayload::FreezeTos { amount, .. } => {
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(*amount)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    EnergyPayload::UnfreezeTos { .. } => {
                        // Unfreeze doesn't spend, it releases frozen funds
                    }
                }
            }
            TransactionType::MultiSig(_)
            | TransactionType::AIMining(_)
            | TransactionType::BindReferrer(_)
            // KYC transactions don't spend assets directly (only fee)
            | TransactionType::SetKyc(_)
            | TransactionType::RevokeKyc(_)
            | TransactionType::RenewKyc(_)
            | TransactionType::TransferKyc(_)
            | TransactionType::AppealKyc(_)
            | TransactionType::BootstrapCommittee(_)
            | TransactionType::RegisterCommittee(_)
            | TransactionType::UpdateCommittee(_)
            | TransactionType::EmergencySuspend(_) => {
                // No asset spending for these types
            }
            TransactionType::BatchReferralReward(payload) => {
                // BatchReferralReward spends total_amount of the specified asset
                let current = spending_per_asset.entry(payload.get_asset()).or_insert(0);
                *current = current
                    .checked_add(payload.get_total_amount())
                    .ok_or(VerificationError::Overflow)?;
            }
        };

        // Add fee to TOS spending (unless using energy fee)
        if !self.get_fee_type().is_energy() {
            let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
            *current = current
                .checked_add(self.fee)
                .ok_or(VerificationError::Overflow)?;
        }

        // Verify sender has sufficient balance for each asset
        // CRITICAL: Mutate balance during verification (like old encrypted balance code)
        // This ensures mempool verification reduces cached balances, preventing
        // users from submitting sequential transactions that total more than their funds
        for (asset_hash, total_spending) in &spending_per_asset {
            let current_balance = state
                .get_sender_balance(&self.source, asset_hash, &self.reference)
                .await
                .map_err(VerificationError::State)?;

            if *current_balance < *total_spending {
                return Err(VerificationError::InsufficientFunds {
                    available: *current_balance,
                    required: *total_spending,
                });
            }

            // Deduct spending from balance immediately (matches old behavior)
            // This mutation updates mempool cached balances so subsequent txs see reduced funds
            *current_balance = current_balance
                .checked_sub(*total_spending)
                .ok_or(VerificationError::Overflow)?;
        }

        if let TransactionType::Energy(EnergyPayload::UnfreezeTos { amount }) = &self.data {
            let balance = state
                .get_receiver_balance(Cow::Borrowed(self.get_source()), Cow::Borrowed(&TOS_ASSET))
                .await
                .map_err(VerificationError::State)?;

            *balance = balance
                .checked_add(*amount)
                .ok_or(VerificationError::Overflow)?;
        }

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
        C: ZKPCache<E>,
    {
        trace!("Verifying batch of transactions");
        for (tx, hash) in txs {
            let hash = hash.as_ref();

            // In case the cache already know this TX
            // we don't need to spend time reverifying it again
            // because a TX is immutable, we can just verify the mutable parts
            // (balance & nonce related)
            let dynamic_parts_only = cache
                .is_already_verified(hash)
                .await
                .map_err(VerificationError::State)?;
            if dynamic_parts_only {
                if log::log_enabled!(log::Level::Debug) {
                    debug!("TX {hash} is known from ZKPCache, verifying dynamic parts only");
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
        C: ZKPCache<E>,
    {
        let dynamic_parts_only = cache
            .is_already_verified(tx_hash)
            .await
            .map_err(VerificationError::State)?;
        if dynamic_parts_only {
            if log::log_enabled!(log::Level::Debug) {
                debug!("TX {tx_hash} is known from ZKPCache, verifying dynamic parts only");
            }
            self.verify_dynamic_parts(tx_hash, state).await?;
        } else {
            self.pre_verify(tx_hash, state).await?;
        };

        // With plaintext balances, no ZK proof verification needed
        trace!("Skipping proof verification (plaintext balances)");

        Ok(())
    }

    // Apply the transaction to the state
    // Arc is required around Self to be shared easily into the VM if needed
    async fn apply<'a, P: ContractProvider + Send, E, B: BlockchainApplyState<'a, P, E>>(
        self: &'a Arc<Self>,
        tx_hash: &'a Hash,
        state: &mut B,
    ) -> Result<(), VerificationError<E>> {
        trace!("Applying transaction data");
        // Update nonce
        state
            .update_account_nonce(self.get_source(), self.nonce + 1)
            .await
            .map_err(VerificationError::State)?;

        // SECURITY FIX: Deduct sender balances BEFORE adding to receivers
        // Calculate total spending per asset for sender deduction
        // Use references to original Hash values in transaction (they live for 'a)
        let mut spending_per_asset: IndexMap<&'a Hash, u64> = IndexMap::new();

        match &self.data {
            TransactionType::Transfers(transfers) => {
                for transfer in transfers {
                    let asset = transfer.get_asset(); // Returns &Hash
                    let amount = transfer.get_amount();
                    let current = spending_per_asset.entry(asset).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::Burn(payload) => {
                let current = spending_per_asset.entry(&payload.asset).or_insert(0);
                *current = current
                    .checked_add(payload.amount)
                    .ok_or(VerificationError::Overflow)?;
            }
            TransactionType::InvokeContract(payload) => {
                // Add deposits
                for (asset, deposit) in &payload.deposits {
                    let amount = deposit
                        .get_amount()
                        .map_err(|e| VerificationError::AnyError(anyhow!(e)))?;
                    let current = spending_per_asset.entry(asset).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
                // Add max_gas to TOS spending
                let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                *current = current
                    .checked_add(payload.max_gas)
                    .ok_or(VerificationError::Overflow)?;
            }
            TransactionType::DeployContract(payload) => {
                // Add BURN_PER_CONTRACT to TOS spending
                let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                *current = current
                    .checked_add(BURN_PER_CONTRACT)
                    .ok_or(VerificationError::Overflow)?;

                // If invoking constructor, add deposits and max_gas
                if let Some(invoke) = &payload.invoke {
                    for (asset, deposit) in &invoke.deposits {
                        let amount = deposit
                            .get_amount()
                            .map_err(|e| VerificationError::AnyError(anyhow!(e)))?;
                        let current = spending_per_asset.entry(asset).or_insert(0);
                        *current = current
                            .checked_add(amount)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    // Add max_gas to TOS spending
                    let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                    *current = current
                        .checked_add(invoke.max_gas)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::Energy(payload) => {
                match payload {
                    EnergyPayload::FreezeTos { amount, .. } => {
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(*amount)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    EnergyPayload::UnfreezeTos { .. } => {
                        // Unfreeze doesn't spend, it releases frozen funds
                        // Instead it will be added back to sender balance below
                    }
                }
            }
            TransactionType::MultiSig(_)
            | TransactionType::AIMining(_)
            | TransactionType::BindReferrer(_)
            // KYC transactions don't spend assets directly (only fee)
            | TransactionType::SetKyc(_)
            | TransactionType::RevokeKyc(_)
            | TransactionType::RenewKyc(_)
            | TransactionType::TransferKyc(_)
            | TransactionType::AppealKyc(_)
            | TransactionType::BootstrapCommittee(_)
            | TransactionType::RegisterCommittee(_)
            | TransactionType::UpdateCommittee(_)
            | TransactionType::EmergencySuspend(_) => {
                // No asset spending for these types
            }
            TransactionType::BatchReferralReward(payload) => {
                // BatchReferralReward spends total_amount of the specified asset
                let current = spending_per_asset.entry(payload.get_asset()).or_insert(0);
                *current = current
                    .checked_add(payload.get_total_amount())
                    .ok_or(VerificationError::Overflow)?;
            }
        };

        // Add fee to TOS spending (unless using energy fee)
        if !self.get_fee_type().is_energy() {
            let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
            *current = current
                .checked_add(self.fee)
                .ok_or(VerificationError::Overflow)?;

            // Add fee to gas fee counter
            state
                .add_gas_fee(self.fee)
                .await
                .map_err(VerificationError::State)?;
        }

        // Track sender outputs for final balance calculation
        // Note: Sender balance was already mutated during verification (pre_verify/verify_dynamic_parts)
        // so we only need to track outputs here, not mutate balance again (would cause double-subtract)
        for (asset_hash, total_spending) in &spending_per_asset {
            // IMPORTANT: Must call get_sender_balance first to ensure the asset exchange is loaded
            // This ensures the account assets are properly initialized before updating
            // Without this, internal_update_sender_exchange will fail with "account not found" error
            // because the account was created with empty assets HashMap
            let _ = state
                .get_sender_balance(&self.source, asset_hash, &self.reference)
                .await
                .map_err(VerificationError::State)?;

            // Track the spending in output_sum for final balance calculation
            state
                .add_sender_output(&self.source, asset_hash, *total_spending)
                .await
                .map_err(VerificationError::State)?;
        }

        // Handle energy consumption if this transaction uses energy for fees
        if self.get_fee_type().is_energy() {
            // Only transfer transactions can use energy fees
            if let TransactionType::Transfers(_) = &self.data {
                let energy_cost = self.calculate_energy_cost();

                // Get user's energy resource
                let energy_resource = state
                    .get_energy_resource(&self.source)
                    .await
                    .map_err(VerificationError::State)?;

                if let Some(mut energy_resource) = energy_resource {
                    // Check if user has enough energy
                    if !energy_resource.has_enough_energy(energy_cost) {
                        return Err(VerificationError::InsufficientEnergy(energy_cost));
                    }

                    // Consume energy
                    energy_resource
                        .consume_energy(energy_cost)
                        .map_err(|_| VerificationError::InsufficientEnergy(energy_cost))?;

                    // Update energy resource in state
                    state
                        .set_energy_resource(&self.source, energy_resource)
                        .await
                        .map_err(VerificationError::State)?;

                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Consumed {energy_cost} energy for transaction {tx_hash}");
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
                        )
                        .await
                        .map_err(VerificationError::State)?;

                    // Balance simplification: Add plain u64 amount to receiver's balance
                    let plain_amount = transfer.get_amount();
                    *current_balance = current_balance
                        .checked_add(plain_amount)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::Burn(payload) => {
                if payload.asset == TOS_ASSET {
                    state
                        .add_burned_coins(payload.amount)
                        .await
                        .map_err(VerificationError::State)?;
                }
            }
            TransactionType::MultiSig(payload) => {
                state
                    .set_multisig_state(&self.source, payload)
                    .await
                    .map_err(VerificationError::State)?;
            }
            TransactionType::InvokeContract(payload) => {
                if self.is_contract_available(state, &payload.contract).await? {
                    self.invoke_contract(
                        tx_hash,
                        state,
                        &payload.contract,
                        &payload.deposits,
                        payload.parameters.iter().cloned(),
                        payload.max_gas,
                        InvokeContract::Entry(payload.entry_id),
                    )
                    .await?;
                } else {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!(
                            "Contract {} invoked from {} not available",
                            payload.contract, tx_hash
                        );
                    }

                    // Nothing was spent, we must refund the gas and deposits
                    self.handle_gas(state, 0, payload.max_gas).await?;
                    self.refund_deposits(state, &payload.deposits).await?;
                }
            }
            TransactionType::DeployContract(payload) => {
                // Compute deterministic contract address using eBPF bytecode
                let bytecode = payload
                    .module
                    .get_bytecode()
                    .map(|b| b.to_vec())
                    .unwrap_or_default();

                let contract_address =
                    crate::crypto::compute_deterministic_contract_address(&self.source, &bytecode);

                // Deploy contract to deterministic address
                state
                    .set_contract_module(&contract_address, &payload.module)
                    .await
                    .map_err(VerificationError::State)?;

                if let Some(invoke) = payload.invoke.as_ref() {
                    let is_success = self
                        .invoke_contract(
                            tx_hash,
                            state,
                            &contract_address,
                            &invoke.deposits,
                            iter::empty(),
                            invoke.max_gas,
                            InvokeContract::Hook(0),
                        )
                        .await?;

                    // if it has failed, we don't want to deploy the contract
                    // TODO: we must handle this carefully
                    if !is_success {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("Contract deploy for {contract_address} failed");
                        }
                        state
                            .remove_contract_module(&contract_address)
                            .await
                            .map_err(VerificationError::State)?;
                    }
                }
            }
            TransactionType::Energy(payload) => {
                // Handle energy operations (freeze/unfreeze TOS)
                match payload {
                    EnergyPayload::FreezeTos { amount, duration } => {
                        // Get current energy resource for the account
                        let energy_resource = state
                            .get_energy_resource(&self.source)
                            .await
                            .map_err(VerificationError::State)?;

                        let mut energy_resource =
                            energy_resource.unwrap_or_else(EnergyResource::new);

                        // Freeze TOS for energy - get topoheight from the blockchain state
                        let topoheight = state.get_block().get_height(); // BlockDAG uses height
                                                                         // Use network-aware freeze duration (Devnet uses accelerated timing)
                        let network = state.get_network();
                        energy_resource.freeze_tos_for_energy_with_network(
                            *amount, *duration, topoheight, &network,
                        );

                        // Update energy resource in state
                        state
                            .set_energy_resource(&self.source, energy_resource)
                            .await
                            .map_err(VerificationError::State)?;

                        if log::log_enabled!(log::Level::Debug) {
                            debug!("FreezeTos applied: {} TOS frozen for {} duration, energy gained: {} units",
                                   amount, duration.name(), (*amount / crate::config::COIN_VALUE) * duration.reward_multiplier());
                        }
                    }
                    EnergyPayload::UnfreezeTos { amount } => {
                        // Get current energy resource for the account
                        let energy_resource = state
                            .get_energy_resource(&self.source)
                            .await
                            .map_err(VerificationError::State)?;

                        if let Some(mut energy_resource) = energy_resource {
                            // Unfreeze TOS - get topoheight from the blockchain state
                            let topoheight = state.get_block().get_height(); // BlockDAG uses height
                            energy_resource
                                .unfreeze_tos(*amount, topoheight)
                                .map_err(|_| {
                                    VerificationError::AnyError(anyhow::anyhow!(
                                        "Invalid energy operation"
                                    ))
                                })?;

                            // Update energy resource in state
                            state
                                .set_energy_resource(&self.source, energy_resource)
                                .await
                                .map_err(VerificationError::State)?;

                            // NOTE: Balance refund is done in verify phase (verify_dynamic_parts/pre_verify)
                            // to keep mempool state consistent. Do NOT add balance here to avoid double-refund.

                            if log::log_enabled!(log::Level::Debug) {
                                debug!("UnfreezeTos applied: {amount} TOS unfrozen (balance already refunded in verify phase)");
                            }
                        } else {
                            return Err(VerificationError::AnyError(anyhow::anyhow!(
                                "Invalid energy operation"
                            )));
                        }
                    }
                }
            }
            TransactionType::AIMining(payload) => {
                // Handle AI Mining operations with full validation
                use crate::ai_mining::AIMiningValidator;

                // Get or create AI mining state
                let mut ai_mining_state = state
                    .get_ai_mining_state()
                    .await
                    .map_err(VerificationError::State)?
                    .unwrap_or_default();

                // Create validator with current context
                let block_height = state.get_block().get_height();
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
                    validator.validate_and_apply(payload).map_err(|e| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "AI Mining validation failed: {e}"
                        ))
                    })?;

                    // Update tasks and process completions
                    validator.update_tasks().map_err(|e| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "AI Mining task update failed: {e}"
                        ))
                    })?;

                    validator.get_validation_summary()
                };

                // Save updated state back to blockchain
                state
                    .set_ai_mining_state(&ai_mining_state)
                    .await
                    .map_err(VerificationError::State)?;

                if log::log_enabled!(log::Level::Debug) {
                    debug!("AI Mining operation processed - payload: {:?}, miners: {}, active_tasks: {}, completed_tasks: {}",
                           payload, result.total_miners, result.active_tasks, result.completed_tasks);
                }
            }
            TransactionType::BindReferrer(payload) => {
                // Bind referrer to user via the ReferralProvider
                state
                    .bind_referrer(self.get_source(), payload.get_referrer(), tx_hash)
                    .await
                    .map_err(VerificationError::State)?;

                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "BindReferrer transaction applied - user: {:?}, referrer: {:?}",
                        self.get_source(),
                        payload.get_referrer()
                    );
                }
            }
            TransactionType::BatchReferralReward(payload) => {
                // Distribute rewards to uplines via the ReferralProvider
                let distribution_result = state
                    .distribute_referral_rewards(
                        payload.get_from_user(),
                        payload.get_asset(),
                        payload.get_total_amount(),
                        payload.get_ratios(),
                    )
                    .await
                    .map_err(VerificationError::State)?;

                // Credit rewards to each upline's balance
                // Note: distribution.recipient is already a CompressedPublicKey (aka crypto::PublicKey)
                for distribution in &distribution_result.distributions {
                    let balance = state
                        .get_receiver_balance(
                            std::borrow::Cow::Owned(distribution.recipient.clone()),
                            std::borrow::Cow::Borrowed(payload.get_asset()),
                        )
                        .await
                        .map_err(VerificationError::State)?;
                    *balance = balance
                        .checked_add(distribution.amount)
                        .ok_or(VerificationError::Overflow)?;
                }

                // Refund remainder to sender (prevents burning undistributed funds)
                let remainder = payload
                    .get_total_amount()
                    .saturating_sub(distribution_result.total_distributed);
                if remainder > 0 {
                    let sender_balance = state
                        .get_receiver_balance(
                            std::borrow::Cow::Borrowed(self.get_source()),
                            std::borrow::Cow::Borrowed(payload.get_asset()),
                        )
                        .await
                        .map_err(VerificationError::State)?;
                    *sender_balance = sender_balance
                        .checked_add(remainder)
                        .ok_or(VerificationError::Overflow)?;
                }

                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "BatchReferralReward transaction applied - levels: {}, total_amount: {}, distributed: {}, refunded: {}",
                        payload.get_levels(),
                        payload.get_total_amount(),
                        distribution_result.total_distributed,
                        remainder
                    );
                }
            }
            // KYC transaction types - execution handled by BlockchainApplyState
            // SECURITY: All KYC operations require approval verification
            TransactionType::SetKyc(payload) => {
                // SECURITY: Check if user already has KYC from a different committee
                // If so, they must use TransferKyc to change committees, not SetKyc
                // This prevents cross-committee hijacking of users
                if let Some(existing_committee) = state
                    .get_verifying_committee(payload.get_account())
                    .await
                    .map_err(VerificationError::State)?
                {
                    if &existing_committee != payload.get_committee_id() {
                        return Err(VerificationError::AnyError(anyhow::anyhow!(
                            "SetKyc: user already has KYC from committee {}. Use TransferKyc to change committees.",
                            existing_committee
                        )));
                    }
                }

                // Get the committee and verify approvals
                let committee = state
                    .get_committee(payload.get_committee_id())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "Committee not found: {}",
                            payload.get_committee_id()
                        ))
                    })?;

                // Verify approvals (signatures, membership, threshold)
                let current_time = state.get_verification_timestamp();
                crate::kyc::verify_set_kyc_approvals(
                    &committee,
                    payload.get_approvals(),
                    payload.get_account(),
                    payload.get_level(),
                    payload.get_data_hash(),
                    current_time,
                )
                .map_err(|e| VerificationError::AnyError(anyhow::anyhow!("{}", e)))?;

                state
                    .set_kyc(
                        payload.get_account(),
                        payload.get_level(),
                        payload.get_verified_at(),
                        payload.get_data_hash(),
                        payload.get_committee_id(),
                        tx_hash,
                    )
                    .await
                    .map_err(VerificationError::State)?;

                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "SetKyc applied - account: {:?}, level: {}",
                        payload.get_account(),
                        payload.get_level()
                    );
                }
            }
            TransactionType::RevokeKyc(payload) => {
                // SECURITY: Verify that the committee is the user's verifying committee
                // This prevents unauthorized committees from revoking KYC they didn't issue
                let user_verifying_committee = state
                    .get_verifying_committee(payload.get_account())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "User has no KYC record to revoke"
                        ))
                    })?;

                if &user_verifying_committee != payload.get_committee_id() {
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "RevokeKyc: committee {} is not the user's verifying committee {}",
                        payload.get_committee_id(),
                        user_verifying_committee
                    )));
                }

                // Get the committee and verify approvals
                let committee = state
                    .get_committee(payload.get_committee_id())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "Committee not found: {}",
                            payload.get_committee_id()
                        ))
                    })?;

                // Verify approvals (signatures, membership, threshold)
                let current_time = state.get_verification_timestamp();
                crate::kyc::verify_revoke_kyc_approvals(
                    &committee,
                    payload.get_approvals(),
                    payload.get_account(),
                    payload.get_reason_hash(),
                    current_time,
                )
                .map_err(|e| VerificationError::AnyError(anyhow::anyhow!("{}", e)))?;

                state
                    .revoke_kyc(payload.get_account(), payload.get_reason_hash(), tx_hash)
                    .await
                    .map_err(VerificationError::State)?;

                if log::log_enabled!(log::Level::Debug) {
                    debug!("RevokeKyc applied - account: {:?}", payload.get_account());
                }
            }
            TransactionType::RenewKyc(payload) => {
                // SECURITY: Verify that the committee is the user's verifying committee
                // This prevents unauthorized committees from renewing KYC they didn't issue
                let user_verifying_committee = state
                    .get_verifying_committee(payload.get_account())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "User has no KYC record to renew"
                        ))
                    })?;

                if &user_verifying_committee != payload.get_committee_id() {
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "RenewKyc: committee {} is not the user's verifying committee {}",
                        payload.get_committee_id(),
                        user_verifying_committee
                    )));
                }

                // Get the committee and verify approvals
                let committee = state
                    .get_committee(payload.get_committee_id())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "Committee not found: {}",
                            payload.get_committee_id()
                        ))
                    })?;

                // Verify approvals (signatures, membership, threshold)
                let current_time = state.get_verification_timestamp();
                crate::kyc::verify_renew_kyc_approvals(
                    &committee,
                    payload.get_approvals(),
                    payload.get_account(),
                    payload.get_data_hash(),
                    current_time,
                )
                .map_err(|e| VerificationError::AnyError(anyhow::anyhow!("{}", e)))?;

                state
                    .renew_kyc(
                        payload.get_account(),
                        payload.get_verified_at(),
                        payload.get_data_hash(),
                        tx_hash,
                    )
                    .await
                    .map_err(VerificationError::State)?;

                if log::log_enabled!(log::Level::Debug) {
                    debug!("RenewKyc applied - account: {:?}", payload.get_account());
                }
            }
            TransactionType::TransferKyc(payload) => {
                let current_time = state.get_verification_timestamp();

                // SECURITY: Verify that the user is currently bound to the source committee
                // This prevents unauthorized committees from transferring users they don't manage
                let user_verifying_committee = state
                    .get_verifying_committee(payload.get_account())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "User has no KYC record to transfer"
                        ))
                    })?;

                if &user_verifying_committee != payload.get_source_committee_id() {
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "TransferKyc: source committee {} does not match user's verifying committee {}",
                        payload.get_source_committee_id(),
                        user_verifying_committee
                    )));
                }

                // Get and verify source committee approvals
                let source_committee = state
                    .get_committee(payload.get_source_committee_id())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "Source committee not found: {}",
                            payload.get_source_committee_id()
                        ))
                    })?;

                crate::kyc::verify_transfer_kyc_source_approvals(
                    &source_committee,
                    payload.get_source_approvals(),
                    payload.get_dest_committee_id(),
                    payload.get_account(),
                    current_time,
                )
                .map_err(|e| VerificationError::AnyError(anyhow::anyhow!("Source: {}", e)))?;

                // Get and verify destination committee approvals
                let dest_committee = state
                    .get_committee(payload.get_dest_committee_id())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "Destination committee not found: {}",
                            payload.get_dest_committee_id()
                        ))
                    })?;

                crate::kyc::verify_transfer_kyc_dest_approvals(
                    &dest_committee,
                    payload.get_dest_approvals(),
                    payload.get_source_committee_id(),
                    payload.get_account(),
                    payload.get_new_data_hash(),
                    current_time,
                )
                .map_err(|e| VerificationError::AnyError(anyhow::anyhow!("Destination: {}", e)))?;

                // Transfer KYC to destination committee
                // The max_kyc_level check is done inside transfer_kyc to ensure
                // user's KYC level doesn't exceed destination committee's max level
                // SECURITY FIX (Issue #26): Pass current_time (block/verification time) instead
                // of payload time for suspension expiry check
                state
                    .transfer_kyc(
                        payload.get_account(),
                        payload.get_source_committee_id(),
                        payload.get_dest_committee_id(),
                        payload.get_new_data_hash(),
                        payload.get_transferred_at(),
                        tx_hash,
                        dest_committee.max_kyc_level,
                        current_time,
                    )
                    .await
                    .map_err(VerificationError::State)?;

                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "TransferKyc applied - account: {:?}, source: {}, dest: {}",
                        payload.get_account(),
                        payload.get_source_committee_id(),
                        payload.get_dest_committee_id()
                    );
                }
            }
            TransactionType::AppealKyc(payload) => {
                // Authorization check: only the account owner can submit their own appeal
                if self.get_source() != payload.get_account() {
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "AppealKyc: sender {:?} does not match appeal account {:?}",
                        self.get_source(),
                        payload.get_account()
                    )));
                }

                // SECURITY: Verify that the user's verifying committee matches original_committee_id
                // This prevents appeals against arbitrary committees
                let user_verifying_committee = state
                    .get_verifying_committee(payload.get_account())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "AppealKyc: user has no KYC record to appeal"
                        ))
                    })?;

                if &user_verifying_committee != payload.get_original_committee_id() {
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "AppealKyc: original_committee_id {} does not match user's verifying committee {}",
                        payload.get_original_committee_id(),
                        user_verifying_committee
                    )));
                }

                // SECURITY: Verify that user has Revoked status (appeals are for revoked KYC)
                let user_status = state
                    .get_kyc_status(payload.get_account())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "AppealKyc: user has no KYC record to appeal"
                        ))
                    })?;

                if user_status != crate::kyc::KycStatus::Revoked {
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "AppealKyc: user KYC status is {:?}, only Revoked status can be appealed",
                        user_status
                    )));
                }

                // Verify original committee exists and is active
                let original_committee = state
                    .get_committee(payload.get_original_committee_id())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "Original committee not found: {}",
                            payload.get_original_committee_id()
                        ))
                    })?;

                // SECURITY FIX (Issue #29): Verify original committee is Active
                // Appeals should only be accepted against active committees
                if original_committee.status != crate::kyc::CommitteeStatus::Active {
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "AppealKyc: original committee {} is not active (status: {:?})",
                        payload.get_original_committee_id(),
                        original_committee.status
                    )));
                }

                // Verify parent committee exists
                let parent_committee = state
                    .get_committee(payload.get_parent_committee_id())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "Parent committee not found: {}",
                            payload.get_parent_committee_id()
                        ))
                    })?;

                // SECURITY FIX (Issue #29): Verify parent committee is Active
                // Appeals must be submitted to active parent committees that can review them
                if parent_committee.status != crate::kyc::CommitteeStatus::Active {
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "AppealKyc: parent committee {} is not active (status: {:?})",
                        payload.get_parent_committee_id(),
                        parent_committee.status
                    )));
                }

                // Verify the original committee's parent matches the claimed parent
                if let Some(ref actual_parent_id) = original_committee.parent_id {
                    if actual_parent_id != &parent_committee.id {
                        return Err(VerificationError::AnyError(anyhow::anyhow!(
                            "AppealKyc: claimed parent {} does not match original committee's actual parent {}",
                            payload.get_parent_committee_id(),
                            actual_parent_id
                        )));
                    }
                } else {
                    // Original committee is the global committee (no parent)
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "AppealKyc: cannot appeal to parent - original committee {} is the global committee",
                        payload.get_original_committee_id()
                    )));
                }

                state
                    .submit_kyc_appeal(
                        payload.get_account(),
                        payload.get_original_committee_id(),
                        payload.get_parent_committee_id(),
                        payload.get_reason_hash(),
                        payload.get_documents_hash(),
                        payload.get_submitted_at(),
                        tx_hash,
                    )
                    .await
                    .map_err(VerificationError::State)?;

                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "AppealKyc submitted - account: {:?}, original: {}, parent: {}",
                        payload.get_account(),
                        payload.get_original_committee_id(),
                        payload.get_parent_committee_id()
                    );
                }
            }
            TransactionType::BootstrapCommittee(payload) => {
                // Convert CommitteeMemberInit to CommitteeMemberInfo
                let members: Vec<crate::kyc::CommitteeMemberInfo> = payload
                    .get_members()
                    .iter()
                    .map(|m| {
                        crate::kyc::CommitteeMemberInfo::new(
                            m.public_key.clone(),
                            m.name.clone(),
                            m.role,
                        )
                    })
                    .collect();

                let committee_id = state
                    .bootstrap_global_committee(
                        payload.get_name().to_string(),
                        members,
                        payload.get_threshold(),
                        payload.get_kyc_threshold(),
                        payload.get_max_kyc_level(),
                        tx_hash,
                    )
                    .await
                    .map_err(VerificationError::State)?;

                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "BootstrapCommittee applied - name: {}, id: {}",
                        payload.get_name(),
                        committee_id
                    );
                }
            }
            TransactionType::RegisterCommittee(payload) => {
                // Get parent committee and verify approvals
                let parent_committee = state
                    .get_committee(payload.get_parent_id())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "Parent committee not found: {}",
                            payload.get_parent_id()
                        ))
                    })?;

                // SECURITY: Verify parent committee can manage the requested region
                // Global committees can manage any region; regional committees can only
                // manage their own region. This prevents unauthorized cross-region registration.
                if !parent_committee.can_manage_region(&payload.get_region()) {
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "Parent committee {} (region: {}) cannot register committees in region {}",
                        payload.get_parent_id(),
                        parent_committee.region,
                        payload.get_region()
                    )));
                }

                // Compute config hash from payload to bind signatures to full configuration
                let members: Vec<_> = payload
                    .get_members()
                    .iter()
                    .map(|m| (m.public_key.clone(), m.name.clone(), m.role))
                    .collect();
                let config_hash = crate::kyc::CommitteeApproval::compute_register_config_hash(
                    &members,
                    payload.get_threshold(),
                    payload.get_kyc_threshold(),
                    payload.get_max_kyc_level(),
                );

                // Verify parent committee approvals with config binding
                let current_time = state.get_verification_timestamp();
                crate::kyc::verify_register_committee_approvals(
                    &parent_committee,
                    payload.get_approvals(),
                    payload.get_name(),
                    payload.get_region(),
                    &config_hash,
                    current_time,
                )
                .map_err(|e| VerificationError::AnyError(anyhow::anyhow!("{}", e)))?;

                // Convert NewCommitteeMember to CommitteeMemberInfo
                let members: Vec<crate::kyc::CommitteeMemberInfo> = payload
                    .get_members()
                    .iter()
                    .map(|m| {
                        crate::kyc::CommitteeMemberInfo::new(
                            m.public_key.clone(),
                            m.name.clone(),
                            m.role,
                        )
                    })
                    .collect();

                let committee_id = state
                    .register_committee(
                        payload.get_name().to_string(),
                        payload.get_region(),
                        members,
                        payload.get_threshold(),
                        payload.get_kyc_threshold(),
                        payload.get_max_kyc_level(),
                        payload.get_parent_id(),
                        tx_hash,
                    )
                    .await
                    .map_err(VerificationError::State)?;

                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "RegisterCommittee applied - name: {}, region: {:?}, id: {}",
                        payload.get_name(),
                        payload.get_region(),
                        committee_id
                    );
                }
            }
            TransactionType::UpdateCommittee(payload) => {
                // Get committee and verify approvals
                let committee = state
                    .get_committee(payload.get_committee_id())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "Committee not found: {}",
                            payload.get_committee_id()
                        ))
                    })?;

                // Validate governance constraints using committee state
                let committee_info = kyc::CommitteeGovernanceInfo {
                    member_count: committee.active_member_count(),
                    total_member_count: committee.total_member_count(),
                    threshold: committee.threshold,
                };
                let current_time = state.get_verification_timestamp();
                kyc::verify_update_committee_with_state(payload, &committee_info, current_time)?;

                // Compute hash of update data for signature verification
                let update_data_hash = {
                    use crate::serializer::Serializer;
                    let mut buffer = Vec::new();
                    let mut writer = crate::serializer::Writer::new(&mut buffer);
                    payload.get_update().write(&mut writer);
                    crate::crypto::Hash::new(blake3::hash(&buffer).into())
                };

                // Get update type for message building
                let update_type = match payload.get_update() {
                    crate::transaction::CommitteeUpdateData::AddMember { .. } => 0u8,
                    crate::transaction::CommitteeUpdateData::RemoveMember { .. } => 1u8,
                    crate::transaction::CommitteeUpdateData::UpdateMemberRole { .. } => 2u8,
                    crate::transaction::CommitteeUpdateData::UpdateMemberStatus { .. } => 3u8,
                    crate::transaction::CommitteeUpdateData::UpdateThreshold { .. } => 4u8,
                    crate::transaction::CommitteeUpdateData::UpdateKycThreshold { .. } => 5u8,
                    crate::transaction::CommitteeUpdateData::UpdateName { .. } => 6u8,
                    crate::transaction::CommitteeUpdateData::SuspendCommittee => 7u8,
                    crate::transaction::CommitteeUpdateData::ActivateCommittee => 8u8,
                };

                // Verify committee approvals
                crate::kyc::verify_update_committee_approvals(
                    &committee,
                    payload.get_approvals(),
                    update_type,
                    &update_data_hash,
                    current_time,
                )
                .map_err(|e| VerificationError::AnyError(anyhow::anyhow!("{}", e)))?;

                state
                    .update_committee(payload.get_committee_id(), payload.get_update())
                    .await
                    .map_err(VerificationError::State)?;

                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "UpdateCommittee applied - committee: {}, update: {:?}",
                        payload.get_committee_id(),
                        payload.get_update()
                    );
                }
            }
            TransactionType::EmergencySuspend(payload) => {
                // SECURITY: Verify that the committee is the user's verifying committee
                // This prevents cross-committee denial-of-service attacks where any committee
                // could suspend arbitrary users for 24 hours
                let user_verifying_committee = state
                    .get_verifying_committee(payload.get_account())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "EmergencySuspend: user has no KYC record to suspend"
                        ))
                    })?;

                if &user_verifying_committee != payload.get_committee_id() {
                    return Err(VerificationError::AnyError(anyhow::anyhow!(
                        "EmergencySuspend: committee {} is not the user's verifying committee {}",
                        payload.get_committee_id(),
                        user_verifying_committee
                    )));
                }

                // Get committee and verify approvals
                let committee = state
                    .get_committee(payload.get_committee_id())
                    .await
                    .map_err(VerificationError::State)?
                    .ok_or_else(|| {
                        VerificationError::AnyError(anyhow::anyhow!(
                            "Committee not found: {}",
                            payload.get_committee_id()
                        ))
                    })?;

                // Verify emergency suspend approvals (requires 2 members)
                let current_time = state.get_verification_timestamp();
                crate::kyc::verify_emergency_suspend_approvals(
                    &committee,
                    payload.get_approvals(),
                    payload.get_account(),
                    payload.get_reason_hash(),
                    payload.get_expires_at(),
                    current_time,
                )
                .map_err(|e| VerificationError::AnyError(anyhow::anyhow!("{}", e)))?;

                state
                    .emergency_suspend_kyc(
                        payload.get_account(),
                        payload.get_reason_hash(),
                        payload.get_expires_at(),
                        tx_hash,
                    )
                    .await
                    .map_err(VerificationError::State)?;

                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "EmergencySuspend applied - account: {:?}, expires_at: {}",
                        payload.get_account(),
                        payload.get_expires_at()
                    );
                }
            }
        }

        Ok(())
    }

    /// Assume the tx is valid, apply it to `state`. May panic if a ciphertext is ill-formed.
    ///
    /// BLOCKDAG alignment: This function now performs balance deduction inline,
    /// matching the original BLOCKDAG architecture where balance is deducted
    /// before calling add_sender_output.
    pub async fn apply_without_verify<
        'a,
        P: ContractProvider + Send,
        E,
        B: BlockchainApplyState<'a, P, E>,
    >(
        self: &'a Arc<Self>,
        tx_hash: &'a Hash,
        state: &mut B,
    ) -> Result<(), VerificationError<E>> {
        // Balance simplification: No decompression needed for plaintext balances
        // Private deposits are not supported, only Public deposits with plain u64 amounts

        // BLOCKDAG alignment: Calculate spending per asset and deduct from sender balance
        // This matches BLOCKDAG's approach where balance deduction happens here,
        // before calling add_sender_output.
        //
        // Original BLOCKDAG code (tos_common/src/transaction/verify/mod.rs:1321-1342):
        // for commitment in &self.source_commitments {
        //     let asset = commitment.get_asset();
        //     let current_source_balance = state.get_sender_balance(...).await?;
        //     let output = self.get_sender_output_ct(...)?;
        //     *current_source_balance -= &output;
        //     state.add_sender_output(&self.source, asset, output).await?;
        // }

        // Calculate spending per asset (same logic as pre_verify)
        let mut spending_per_asset: IndexMap<&'a Hash, u64> = IndexMap::new();

        match &self.data {
            TransactionType::Transfers(transfers) => {
                for transfer in transfers {
                    let asset = transfer.get_asset();
                    let amount = transfer.get_amount();
                    let current = spending_per_asset.entry(asset).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::Burn(payload) => {
                let current = spending_per_asset.entry(&payload.asset).or_insert(0);
                *current = current
                    .checked_add(payload.amount)
                    .ok_or(VerificationError::Overflow)?;
            }
            TransactionType::InvokeContract(payload) => {
                for (asset, deposit) in &payload.deposits {
                    let amount = deposit
                        .get_amount()
                        .map_err(|e| VerificationError::AnyError(anyhow!(e)))?;
                    let current = spending_per_asset.entry(asset).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
                // Add max_gas to TOS spending
                let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                *current = current
                    .checked_add(payload.max_gas)
                    .ok_or(VerificationError::Overflow)?;
            }
            TransactionType::DeployContract(payload) => {
                // Add BURN_PER_CONTRACT to TOS spending
                let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                *current = current
                    .checked_add(BURN_PER_CONTRACT)
                    .ok_or(VerificationError::Overflow)?;

                if let Some(invoke) = &payload.invoke {
                    for (asset, deposit) in &invoke.deposits {
                        let amount = deposit
                            .get_amount()
                            .map_err(|e| VerificationError::AnyError(anyhow!(e)))?;
                        let current = spending_per_asset.entry(asset).or_insert(0);
                        *current = current
                            .checked_add(amount)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    // Add max_gas to TOS spending
                    let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                    *current = current
                        .checked_add(invoke.max_gas)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::Energy(payload) => {
                match payload {
                    EnergyPayload::FreezeTos { amount, .. } => {
                        let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
                        *current = current
                            .checked_add(*amount)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    EnergyPayload::UnfreezeTos { .. } => {
                        // Unfreeze doesn't spend, it releases frozen funds
                    }
                }
            }
            TransactionType::MultiSig(_)
            | TransactionType::AIMining(_)
            | TransactionType::BindReferrer(_)
            // KYC transactions don't spend assets directly (only fee)
            | TransactionType::SetKyc(_)
            | TransactionType::RevokeKyc(_)
            | TransactionType::RenewKyc(_)
            | TransactionType::TransferKyc(_)
            | TransactionType::AppealKyc(_)
            | TransactionType::BootstrapCommittee(_)
            | TransactionType::RegisterCommittee(_)
            | TransactionType::UpdateCommittee(_)
            | TransactionType::EmergencySuspend(_) => {
                // No asset spending for these types
            }
            TransactionType::BatchReferralReward(payload) => {
                // BatchReferralReward spends total_amount of the specified asset
                let current = spending_per_asset.entry(payload.get_asset()).or_insert(0);
                *current = current
                    .checked_add(payload.get_total_amount())
                    .ok_or(VerificationError::Overflow)?;
            }
        };

        // Add fee to TOS spending (unless using energy fee)
        if !self.get_fee_type().is_energy() {
            let current = spending_per_asset.entry(&TOS_ASSET).or_insert(0);
            *current = current
                .checked_add(self.fee)
                .ok_or(VerificationError::Overflow)?;
        }

        // Deduct spending from sender balance
        // This matches BLOCKDAG's behavior where balance deduction happens before apply()
        // NOTE: We do NOT call add_sender_output() here because apply() already does that.
        // The apply() function tracks outputs for final balance calculation.
        for (asset, output_sum) in &spending_per_asset {
            let current_balance = state
                .get_sender_balance(&self.source, asset, &self.reference)
                .await
                .map_err(VerificationError::State)?;

            // Deduct spending from balance (BLOCKDAG: *current_source_balance -= &output)
            *current_balance = current_balance
                .checked_sub(*output_sum)
                .ok_or(VerificationError::Overflow)?;

            if log::log_enabled!(log::Level::Trace) {
                trace!(
                    "apply_without_verify: deducted {} from sender balance for asset {}",
                    output_sum,
                    asset
                );
            }
        }

        self.apply(tx_hash, state).await
    }

    /// Verify only that the final sender balance is the expected one for each commitment
    /// Then apply ciphertexts to the state
    /// Checks done are: commitment eq proofs only
    ///
    /// BLOCKDAG alignment: With plaintext balances, no proof verification is needed.
    /// This function delegates to apply_without_verify which handles balance deduction.
    pub async fn apply_with_partial_verify<
        'a,
        P: ContractProvider + Send,
        E,
        B: BlockchainApplyState<'a, P, E>,
    >(
        self: &'a Arc<Self>,
        tx_hash: &'a Hash,
        state: &mut B,
    ) -> Result<(), VerificationError<E>> {
        trace!("apply with partial verify");

        // Balance simplification: No decompression needed for plaintext balances
        // Private deposits are not supported, only Public deposits with plain u64 amounts
        trace!("Partial verify with plaintext balances - no proof verification needed");

        // Delegate to apply_without_verify which handles balance deduction
        // (BLOCKDAG alignment: both functions now perform inline balance deduction)
        self.apply_without_verify(tx_hash, state).await
    }
}
