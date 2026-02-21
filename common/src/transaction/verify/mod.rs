mod contract;
mod error;
mod state;
mod tns;
mod zkp_cache;

use std::{borrow::Cow, iter, sync::Arc};

use anyhow::{anyhow, Context};
use indexmap::IndexMap;
use log::{debug, trace};
use tos_crypto::{
    bulletproofs::RangeProof,
    curve25519_dalek::{ristretto::CompressedRistretto, traits::Identity, RistrettoPoint, Scalar},
    merlin::Transcript,
};
use tos_kernel::ModuleValidator;

use super::{ContractDeposit, Role, Transaction, TransactionType};
use crate::{
    config::{
        BURN_PER_CONTRACT, MAX_GAS_USAGE_PER_TX, MIN_SHIELD_TOS_AMOUNT, TOS_ASSET, UNO_ASSET,
        UNO_BURN_FEE_PER_TRANSFER,
    },
    contract::ContractProvider,
    crypto::{
        elgamal::{Ciphertext, DecompressionError, DecryptHandle, PedersenCommitment, PublicKey},
        hash,
        proofs::{BatchCollector, ProofVerificationError, BP_GENS, BULLET_PROOF_SIZE, PC_GENS},
        Hash, ProtocolTranscript,
    },
    serializer::Serializer,
    tokio::spawn_blocking_safe,
    transaction::{
        payload::{UnoTransferPayload, UnshieldTransferPayload},
        TxVersion, EXTRA_DATA_LIMIT_SIZE, EXTRA_DATA_LIMIT_SUM_SIZE, MAX_DEPOSIT_PER_INVOKE_CALL,
        MAX_MULTISIG_PARTICIPANTS, MAX_TRANSFER_COUNT,
    },
};
use contract::InvokeContract;

pub use error::*;
pub use state::*;
pub use tns::*;
pub use zkp_cache::*;

/// Prepared UNO transaction data for batch range proof verification
type UnoPreparedData = (
    Arc<Transaction>,
    Transcript,
    Vec<(RistrettoPoint, CompressedRistretto)>,
);

// Decompressed UNO transfer ciphertext
// UNO transfers are stored in a compressed format for efficiency
// We decompress them once for verification and balance updates
struct DecompressedUnoTransferCt {
    commitment: PedersenCommitment,
    sender_handle: DecryptHandle,
    receiver_handle: DecryptHandle,
}

impl DecompressedUnoTransferCt {
    fn decompress(transfer: &UnoTransferPayload) -> Result<Self, DecompressionError> {
        Ok(Self {
            commitment: transfer.get_commitment().decompress()?,
            sender_handle: transfer.get_sender_handle().decompress()?,
            receiver_handle: transfer.get_receiver_handle().decompress()?,
        })
    }

    fn get_ciphertext(&self, role: Role) -> Ciphertext {
        let handle = match role {
            Role::Receiver => self.receiver_handle.clone(),
            Role::Sender => self.sender_handle.clone(),
        };
        Ciphertext::new(self.commitment.clone(), handle)
    }
}

// Decompressed Unshield transfer ciphertext
// Unshield transfers have commitment and sender_handle (no receiver_handle since receiver gets plaintext)
struct DecompressedUnshieldTransferCt {
    commitment: PedersenCommitment,
    sender_handle: DecryptHandle,
}

impl DecompressedUnshieldTransferCt {
    fn decompress(transfer: &UnshieldTransferPayload) -> Result<Self, DecompressionError> {
        Ok(Self {
            commitment: transfer.get_commitment().decompress()?,
            sender_handle: transfer.get_sender_handle().decompress()?,
        })
    }

    fn get_sender_ciphertext(&self) -> Ciphertext {
        Ciphertext::new(self.commitment.clone(), self.sender_handle.clone())
    }
}

fn add_uno_balance<E>(
    current_balance: &mut Ciphertext,
    receiver_ct: &Ciphertext,
) -> Result<(), VerificationError<E>> {
    *current_balance = current_balance
        .checked_add(receiver_ct)
        .ok_or(VerificationError::UnoBalanceOverflow)?;
    Ok(())
}

impl Transaction {
    pub fn has_valid_version_format(&self) -> bool {
        match self.version {
            TxVersion::T1 => {
                // T1 includes chain_id for cross-network replay protection
                match &self.data {
                    TransactionType::Transfers(_)
                    | TransactionType::Burn(_)
                    | TransactionType::MultiSig(_)
                    | TransactionType::InvokeContract(_)
                    | TransactionType::DeployContract(_)
                    | TransactionType::UnoTransfers(_)
                    | TransactionType::ShieldTransfers(_)
                    | TransactionType::UnshieldTransfers(_)
                    | TransactionType::RegisterName(_) => true,
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

    /// Get the UNO output ciphertext for a specific asset
    /// This is used to calculate the total spending from encrypted balance
    fn get_uno_sender_output_ct<E>(
        &self,
        asset: &Hash,
        decompressed_transfers: &[DecompressedUnoTransferCt],
    ) -> Result<Ciphertext, VerificationError<E>> {
        let mut output = Ciphertext::zero();

        let transfers = match &self.data {
            TransactionType::UnoTransfers(transfers) => transfers,
            _ => {
                if self.get_fee_type().is_uno() {
                    return Err(VerificationError::InvalidFormat);
                }
                return Ok(output);
            }
        };

        debug_assert_eq!(
            transfers.len(),
            decompressed_transfers.len(),
            "Length mismatch: {} transfers but {} decompressed",
            transfers.len(),
            decompressed_transfers.len()
        );

        if *asset == UNO_ASSET && self.get_fee_type().is_uno() {
            let uno_fee = UNO_BURN_FEE_PER_TRANSFER
                .checked_mul(transfers.len() as u64)
                .ok_or(VerificationError::Overflow)?;
            output += Scalar::from(uno_fee);
        }

        for (transfer, d) in transfers.iter().zip(decompressed_transfers.iter()) {
            if asset == transfer.get_asset() {
                output += d.get_ciphertext(Role::Sender);
            }
        }

        Ok(output)
    }

    /// Verify that source commitment assets match the UNO transfer assets
    /// UNO is a single dedicated asset - all transfers must use UNO_ASSET only
    fn verify_uno_commitment_assets(&self) -> bool {
        let has_commitment_for_asset = |asset: &Hash| {
            self.source_commitments
                .iter()
                .any(|c| c.get_asset() == asset)
        };

        // UNO_ASSET is required for fees (paid from encrypted balance)
        // Since UNO is a single dedicated asset, we only need UNO_ASSET
        if !has_commitment_for_asset(&UNO_ASSET) {
            return false;
        }

        // Check for duplicates in source commitments
        if self.source_commitments.iter().enumerate().any(|(i, c)| {
            self.source_commitments
                .iter()
                .enumerate()
                .any(|(i2, c2)| i != i2 && c.get_asset() == c2.get_asset())
        }) {
            return false;
        }

        // All UNO transfers must use UNO_ASSET only
        // This enforces UNO as a single dedicated privacy asset
        if let TransactionType::UnoTransfers(transfers) = &self.data {
            return transfers
                .iter()
                .all(|transfer| *transfer.get_asset() == UNO_ASSET);
        }

        true
    }

    /// Get the total sender output ciphertext for Unshield transfers
    /// This is used to calculate the total spending from encrypted balance
    fn get_unshield_sender_output_ct(
        &self,
        decompressed_transfers: &[DecompressedUnshieldTransferCt],
    ) -> Ciphertext {
        let mut output = Ciphertext::zero();

        // Sum up all Unshield transfers (all use UNO_ASSET)
        if let TransactionType::UnshieldTransfers(transfers) = &self.data {
            for (_, d) in transfers.iter().zip(decompressed_transfers.iter()) {
                output += d.get_sender_ciphertext();
            }
        }

        output
    }

    /// Verify that source commitment assets match the Unshield transfer assets
    /// Unshield transfers always use UNO_ASSET (converting UNO -> TOS)
    fn verify_unshield_commitment_assets(&self) -> bool {
        let has_commitment_for_asset = |asset: &Hash| {
            self.source_commitments
                .iter()
                .any(|c| c.get_asset() == asset)
        };

        // UNO_ASSET is required for unshield (deducting from encrypted balance)
        if !has_commitment_for_asset(&UNO_ASSET) {
            return false;
        }

        // Check for duplicates in source commitments
        if self.source_commitments.iter().enumerate().any(|(i, c)| {
            self.source_commitments
                .iter()
                .enumerate()
                .any(|(i2, c2)| i != i2 && c.get_asset() == c2.get_asset())
        }) {
            return false;
        }

        // All Unshield transfers must use UNO_ASSET
        // Unshield converts encrypted UNO to plaintext TOS
        if let TransactionType::UnshieldTransfers(transfers) = &self.data {
            return transfers
                .iter()
                .all(|transfer| *transfer.get_asset() == UNO_ASSET);
        }

        true
    }

    async fn verify_signature_and_multisig<'a, E, B: BlockchainVerificationState<'a, E> + Send>(
        &'a self,
        state: &mut B,
        source_decompressed: &PublicKey,
    ) -> Result<(), VerificationError<E>> {
        let bytes = self.get_signing_bytes();
        if !self.signature.verify(&bytes, source_decompressed) {
            if log::log_enabled!(log::Level::Debug) {
                debug!("transaction signature is invalid");
            }
            return Err(VerificationError::InvalidSignature);
        }

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

            let multisig_bytes = self.get_multisig_signing_bytes();
            let hash_val = hash(&multisig_bytes);
            for sig in multisig.get_signatures() {
                let index = sig.id as usize;
                let Some(key) = config.participants.get_index(index) else {
                    return Err(VerificationError::MultiSigParticipants);
                };

                let decompressed = key.decompress().map_err(ProofVerificationError::from)?;
                if !sig.signature.verify(hash_val.as_bytes(), &decompressed) {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("Multisig signature verification failed for participant {index}");
                    }
                    return Err(VerificationError::InvalidSignature);
                }
            }
        } else if self.get_multisig().is_some() {
            return Err(VerificationError::MultiSigNotConfigured);
        }

        Ok(())
    }

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
        if log::log_enabled!(log::Level::Trace) {
            trace!("Skipping contract deposit proof verification (plaintext balances)");
        }

        // Balance simplification: All deposits are now plaintext
        // Basic validation is performed in verify_invoke_contract
        let _ = (deposits, _source_decompressed, _dest_pubkey); // Suppress unused warnings

        Ok(())
    }

    async fn verify_dynamic_parts<'a, E, B: BlockchainVerificationState<'a, E> + Send>(
        &'a self,
        tx_hash: &'a Hash,
        state: &mut B,
    ) -> Result<(), VerificationError<E>> {
        // Balance simplification: No decompression needed for plaintext balances

        if log::log_enabled!(log::Level::Trace) {
            trace!("Pre-verifying transaction on state");
        }
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

        let source_decompressed = self
            .source
            .decompress()
            .map_err(|err| VerificationError::Proof(err.into()))?;

        let bytes = self.get_signing_bytes();
        if !self.signature.verify(&bytes, &source_decompressed) {
            if log::log_enabled!(log::Level::Debug) {
                debug!("transaction signature is invalid");
            }
            return Err(VerificationError::InvalidSignature);
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
            TransactionType::UnoTransfers(transfers) => {
                // UNO transfers: privacy-preserving transfers with ZKP proofs
                // Validation of extra data size and destination checks
                if transfers.is_empty() || transfers.len() > MAX_TRANSFER_COUNT {
                    return Err(VerificationError::TransferCount);
                }

                let mut extra_data_size = 0;
                for transfer in transfers {
                    if *transfer.get_destination() == self.source {
                        return Err(VerificationError::SenderIsReceiver);
                    }

                    if let Some(extra_data) = transfer.get_extra_data() {
                        let size = extra_data.size();
                        if size > EXTRA_DATA_LIMIT_SIZE {
                            return Err(VerificationError::TransferExtraDataSize);
                        }
                        extra_data_size += size;
                    }
                }

                if extra_data_size > EXTRA_DATA_LIMIT_SUM_SIZE {
                    return Err(VerificationError::TransactionExtraDataSize);
                }
            }
            TransactionType::ShieldTransfers(transfers) => {
                // Shield transfers: TOS -> UNO (enter privacy mode)
                // Amount is public, commitment proof required to prevent forged commitments
                if transfers.is_empty() || transfers.len() > MAX_TRANSFER_COUNT {
                    return Err(VerificationError::TransferCount);
                }

                let mut extra_data_size = 0;
                for transfer in transfers {
                    // Validate amount is non-zero
                    if transfer.get_amount() == 0 {
                        return Err(VerificationError::InvalidTransferAmount);
                    }

                    // Validate minimum Shield amount (anti-money-laundering measure)
                    if transfer.get_amount() < MIN_SHIELD_TOS_AMOUNT {
                        return Err(VerificationError::ShieldAmountTooLow);
                    }

                    // Shield transfers only support TOS asset
                    // UNO is a single-asset privacy layer for TOS only
                    if *transfer.get_asset() != TOS_ASSET {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Shield transfers only support TOS asset"
                        )));
                    }

                    // SECURITY: Verify Shield commitment proof
                    // This ensures the commitment is correctly formed for the claimed amount
                    // and prevents inflation attacks via forged commitments
                    let commitment = transfer
                        .get_commitment()
                        .decompress()
                        .map_err(|_| VerificationError::InvalidFormat)?;
                    let receiver_pubkey = transfer
                        .get_destination()
                        .decompress()
                        .map_err(|_| VerificationError::InvalidFormat)?;
                    let receiver_handle = transfer
                        .get_receiver_handle()
                        .decompress()
                        .map_err(|_| VerificationError::InvalidFormat)?;

                    let mut transcript = Transcript::new(b"shield_commitment_proof");
                    transfer
                        .get_proof()
                        .verify(
                            &commitment,
                            &receiver_pubkey,
                            &receiver_handle,
                            transfer.get_amount(),
                            &mut transcript,
                        )
                        .map_err(|e| {
                            if log::log_enabled!(log::Level::Debug) {
                                debug!("Shield commitment proof verification failed: {:?}", e);
                            }
                            VerificationError::Proof(e)
                        })?;

                    if let Some(extra_data) = transfer.get_extra_data() {
                        let size = extra_data.size();
                        if size > EXTRA_DATA_LIMIT_SIZE {
                            return Err(VerificationError::TransferExtraDataSize);
                        }
                        extra_data_size += size;
                    }
                }

                if extra_data_size > EXTRA_DATA_LIMIT_SUM_SIZE {
                    return Err(VerificationError::TransactionExtraDataSize);
                }
            }
            TransactionType::UnshieldTransfers(transfers) => {
                // Unshield transfers: UNO -> TOS (exit privacy mode)
                // Amount is revealed, requires CiphertextValidityProof
                if transfers.is_empty() || transfers.len() > MAX_TRANSFER_COUNT {
                    return Err(VerificationError::TransferCount);
                }

                let mut extra_data_size = 0;
                for transfer in transfers {
                    // Validate amount is non-zero
                    if transfer.get_amount() == 0 {
                        return Err(VerificationError::InvalidTransferAmount);
                    }

                    if let Some(extra_data) = transfer.get_extra_data() {
                        let size = extra_data.size();
                        if size > EXTRA_DATA_LIMIT_SIZE {
                            return Err(VerificationError::TransferExtraDataSize);
                        }
                        extra_data_size += size;
                    }
                }

                if extra_data_size > EXTRA_DATA_LIMIT_SUM_SIZE {
                    return Err(VerificationError::TransactionExtraDataSize);
                }
            }
            TransactionType::RegisterName(payload) => {
                // TNS RegisterName: stateless format validation + stateful checks
                verify_register_name_format::<E>(payload)?;

                // Fee verification: minimum registration fee required
                verify_register_name_fee::<E>(self.fee)?;

                // Stateful checks: name not taken, account doesn't have name
                let name_hash = get_register_name_hash(payload)
                    .ok_or_else(|| VerificationError::InvalidFormat)?;

                if state
                    .is_name_registered(&name_hash)
                    .await
                    .map_err(VerificationError::State)?
                {
                    return Err(VerificationError::NameAlreadyRegistered);
                }

                if state
                    .account_has_name(&self.source)
                    .await
                    .map_err(VerificationError::State)?
                {
                    return Err(VerificationError::AccountAlreadyHasName);
                }
            }
        };

        // SECURITY FIX: Verify sender has sufficient balance for all spending
        // Calculate total spending per asset
        // Use references to original Hash values in transaction (they live for 'a)
        let mut spending_per_asset: IndexMap<Hash, u64> = IndexMap::new();

        match &self.data {
            TransactionType::Transfers(transfers) => {
                for transfer in transfers {
                    let asset = transfer.get_asset(); // Returns &Hash
                    let amount = transfer.get_amount();
                    let current = spending_per_asset.entry(asset.clone()).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::Burn(payload) => {
                let current = spending_per_asset.entry(payload.asset.clone()).or_insert(0);
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
                    let current = spending_per_asset.entry(asset.clone()).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
                // Add max_gas to TOS spending
                let current = spending_per_asset.entry(TOS_ASSET.clone()).or_insert(0);
                *current = current
                    .checked_add(payload.max_gas)
                    .ok_or(VerificationError::Overflow)?;
            }
            TransactionType::DeployContract(payload) => {
                // Add BURN_PER_CONTRACT to TOS spending
                let current = spending_per_asset.entry(TOS_ASSET.clone()).or_insert(0);
                *current = current
                    .checked_add(BURN_PER_CONTRACT)
                    .ok_or(VerificationError::Overflow)?;

                // If invoking constructor, add deposits and max_gas
                if let Some(invoke) = &payload.invoke {
                    for (asset, deposit) in &invoke.deposits {
                        let amount = deposit
                            .get_amount()
                            .map_err(|e| VerificationError::AnyError(anyhow!(e)))?;
                        let current = spending_per_asset.entry(asset.clone()).or_insert(0);
                        *current = current
                            .checked_add(amount)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    // Add max_gas to TOS spending
                    let current = spending_per_asset.entry(TOS_ASSET.clone()).or_insert(0);
                    *current = current
                        .checked_add(invoke.max_gas)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::MultiSig(_) => {
                // No asset spending for these types
            }
            TransactionType::UnoTransfers(_)
            | TransactionType::ShieldTransfers(_)
            | TransactionType::UnshieldTransfers(_) => {
                // Privacy transfers spend from encrypted balances
                // Spending verification is done through ZKP proofs (CommitmentEqProof)
                // No plaintext spending to verify here
                // Shield/Unshield: actual balance checks happen in apply()
            }
            TransactionType::RegisterName(_) => {
                // TNS transactions: fees are paid from TOS balance
                // Registration fee and message fee are added to TOS spending via the fee field
                // Actual name/message verification happens in verify_register_name/verify_ephemeral_message
            }
        };

        // For Shield transfers, add TOS spending (plaintext balance deduction)
        if let TransactionType::ShieldTransfers(transfers) = &self.data {
            for transfer in transfers {
                let asset = transfer.get_asset();
                let amount = transfer.get_amount();
                let current = spending_per_asset.entry(asset.clone()).or_insert(0);
                *current = current
                    .checked_add(amount)
                    .ok_or(VerificationError::Overflow)?;
            }
        }

        // Add fee to TOS spending
        if self.fee > 0 {
            let current = spending_per_asset.entry(TOS_ASSET.clone()).or_insert(0);
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
                .get_sender_balance(
                    Cow::Borrowed(&self.source),
                    Cow::Owned(asset_hash.clone()),
                    &self.reference,
                )
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

        // Deduct sender UNO balance for UnoTransfers
        // This ensures cached UNO transactions also update balance during verification
        // Previously, only pre_verify_uno updated balance, but cached TXs use verify_dynamic_parts
        if let TransactionType::UnoTransfers(transfers) = &self.data {
            if log::log_enabled!(log::Level::Trace) {
                trace!(
                    "verify_dynamic_parts: Processing UnoTransfers for source {:?}",
                    self.source
                );
            }

            // Decompress transfer ciphertexts to compute total spending
            let mut transfers_decompressed = Vec::with_capacity(transfers.len());
            for transfer in transfers.iter() {
                let decompressed = DecompressedUnoTransferCt::decompress(transfer)
                    .map_err(ProofVerificationError::from)?;
                transfers_decompressed.push(decompressed);
            }

            let output = self.get_uno_sender_output_ct(&UNO_ASSET, &transfers_decompressed)?;

            // Get sender's UNO balance and deduct spending
            let sender_uno_balance = state
                .get_sender_uno_balance(&self.source, &UNO_ASSET, &self.reference)
                .await
                .map_err(VerificationError::State)?;

            if log::log_enabled!(log::Level::Trace) {
                trace!(
                    "verify_dynamic_parts: UnoTransfer deducting from UNO balance for source {:?}",
                    self.source
                );
            }

            *sender_uno_balance -= &output;

            // Track sender output for final balance calculation
            state
                .add_sender_uno_output(&self.source, &UNO_ASSET, output)
                .await
                .map_err(VerificationError::State)?;
        }

        // Deduct sender UNO balance for UnshieldTransfers
        // Similar to pre_verify_uno but without CommitmentEqProof (amount is plaintext)
        if let TransactionType::UnshieldTransfers(transfers) = &self.data {
            if log::log_enabled!(log::Level::Trace) {
                trace!(
                    "verify_dynamic_parts: Processing UnshieldTransfers for source {:?}",
                    self.source
                );
            }
            for transfer in transfers {
                // Create sender ciphertext from commitment and sender handle
                let commitment = transfer
                    .get_commitment()
                    .decompress()
                    .map_err(|_| VerificationError::InvalidFormat)?;
                let sender_handle = transfer
                    .get_sender_handle()
                    .decompress()
                    .map_err(|_| VerificationError::InvalidFormat)?;
                let sender_ct = Ciphertext::new(commitment, sender_handle);

                // Get sender's UNO balance and deduct
                let sender_uno_balance = state
                    .get_sender_uno_balance(&self.source, &UNO_ASSET, &self.reference)
                    .await
                    .map_err(VerificationError::State)?;

                if log::log_enabled!(log::Level::Trace) {
                    trace!("verify_dynamic_parts: UnshieldTransfer deducting from UNO balance for source {:?}", self.source);
                }

                *sender_uno_balance -= sender_ct.clone();

                // Track sender output for final balance calculation
                state
                    .add_sender_uno_output(&self.source, &UNO_ASSET, sender_ct)
                    .await
                    .map_err(VerificationError::State)?;
            }
        }

        Ok(())
    }

    /// Pre-verify UNO (privacy-preserving) transaction with ZK proof verification
    /// Returns the transcript and commitments needed for range proof verification
    async fn pre_verify_uno<'a, E, B: BlockchainVerificationState<'a, E> + Send>(
        &'a self,
        tx_hash: &'a Hash,
        state: &mut B,
        sigma_batch_collector: &mut BatchCollector,
    ) -> Result<(Transcript, Vec<(RistrettoPoint, CompressedRistretto)>), VerificationError<E>>
    {
        if log::log_enabled!(log::Level::Trace) {
            trace!("Pre-verifying UNO transaction");
        }

        if !self.has_valid_version_format() {
            return Err(VerificationError::InvalidFormat);
        }

        if self.get_fee_type().is_uno() && self.fee != 0 {
            return Err(VerificationError::InvalidFee(0, self.fee));
        }

        // UNO transactions must have source commitments and range proof
        if self.source_commitments.is_empty() {
            return Err(VerificationError::Commitments);
        }

        let Some(ref _range_proof) = self.range_proof else {
            return Err(VerificationError::Proof(ProofVerificationError::Format));
        };

        // Verify source commitment assets match UNO transfer assets
        if !self.verify_uno_commitment_assets() {
            if log::log_enabled!(log::Level::Debug) {
                debug!("Invalid UNO commitment assets");
            }
            return Err(VerificationError::Commitments);
        }

        // Pre-verify on state (nonce check, etc.)
        state
            .pre_verify_tx(self)
            .await
            .map_err(VerificationError::State)?;

        // Atomically check and update nonce
        let success = state
            .compare_and_swap_nonce(&self.source, self.nonce, self.nonce + 1)
            .await
            .map_err(VerificationError::State)?;

        if !success {
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

        // Decompress UNO transfers
        let TransactionType::UnoTransfers(transfers) = &self.data else {
            return Err(VerificationError::InvalidFormat);
        };

        if transfers.len() > MAX_TRANSFER_COUNT || transfers.is_empty() {
            if log::log_enabled!(log::Level::Debug) {
                debug!("incorrect UNO transfers size: {}", transfers.len());
            }
            return Err(VerificationError::TransferCount);
        }

        // Validate extra data and decompress transfers
        let mut extra_data_size = 0;
        let mut transfers_decompressed = Vec::with_capacity(transfers.len());

        for transfer in transfers.iter() {
            if *transfer.get_destination() == self.source {
                if log::log_enabled!(log::Level::Debug) {
                    debug!("sender cannot be the receiver in the same TX");
                }
                return Err(VerificationError::SenderIsReceiver);
            }

            if let Some(extra_data) = transfer.get_extra_data() {
                let size = extra_data.size();
                if size > EXTRA_DATA_LIMIT_SIZE {
                    return Err(VerificationError::TransferExtraDataSize);
                }
                extra_data_size += size;
            }

            let decompressed = DecompressedUnoTransferCt::decompress(transfer)
                .map_err(ProofVerificationError::from)?;
            transfers_decompressed.push(decompressed);
        }

        if extra_data_size > EXTRA_DATA_LIMIT_SUM_SIZE {
            return Err(VerificationError::TransactionExtraDataSize);
        }

        // Decompress source commitments
        let new_source_commitments_decompressed = self
            .source_commitments
            .iter()
            .map(|c| c.get_commitment().decompress())
            .collect::<Result<Vec<_>, DecompressionError>>()
            .map_err(ProofVerificationError::from)?;

        let source_decompressed = self
            .source
            .decompress()
            .map_err(|err| VerificationError::Proof(err.into()))?;

        // Prepare transcript for proof verification
        let mut transcript = Self::prepare_transcript(
            self.version,
            &self.source,
            self.fee,
            &self.fee_type,
            self.nonce,
        );

        self.verify_signature_and_multisig(state, &source_decompressed)
            .await?;

        // 1. Verify CommitmentEqProofs for source balances
        if log::log_enabled!(log::Level::Trace) {
            trace!("verifying UNO commitments eq proofs");
        }

        for (commitment, new_source_commitment) in self
            .source_commitments
            .iter()
            .zip(&new_source_commitments_decompressed)
        {
            // Calculate output ciphertext (total spending for this asset)
            let output =
                self.get_uno_sender_output_ct(commitment.get_asset(), &transfers_decompressed)?;

            // Get sender's UNO balance ciphertext
            let source_verification_ciphertext = state
                .get_sender_uno_balance(&self.source, commitment.get_asset(), &self.reference)
                .await
                .map_err(VerificationError::State)?;

            let source_ct_compressed = source_verification_ciphertext.compress();

            // Compute new balance: old_balance - output
            *source_verification_ciphertext -= &output;

            // Prepare transcript for CommitmentEqProof
            transcript.new_commitment_eq_proof_domain_separator();
            transcript.append_hash(b"new_source_commitment_asset", commitment.get_asset());
            transcript.append_commitment(b"new_source_commitment", commitment.get_commitment());
            transcript.append_ciphertext(b"source_ct", &source_ct_compressed);

            // Pre-verify the equality proof (adds to batch collector)
            commitment.get_proof().pre_verify(
                &source_decompressed,
                source_verification_ciphertext,
                new_source_commitment,
                &mut transcript,
                sigma_batch_collector,
            )?;

            // Track sender output for final balance
            state
                .add_sender_uno_output(&self.source, commitment.get_asset(), output)
                .await
                .map_err(VerificationError::State)?;
        }

        // 2. Verify CiphertextValidityProofs for transfers
        if log::log_enabled!(log::Level::Trace) {
            trace!("verifying UNO transfer ciphertext validity proofs");
        }

        let mut value_commitments: Vec<(RistrettoPoint, CompressedRistretto)> = Vec::new();

        for (transfer, decompressed) in transfers.iter().zip(&transfers_decompressed) {
            let receiver = transfer
                .get_destination()
                .decompress()
                .map_err(ProofVerificationError::from)?;

            // Update receiver's UNO balance
            let current_balance = state
                .get_receiver_uno_balance(
                    Cow::Borrowed(transfer.get_destination()),
                    Cow::Borrowed(transfer.get_asset()),
                )
                .await
                .map_err(VerificationError::State)?;

            let receiver_ct = decompressed.get_ciphertext(Role::Receiver);
            add_uno_balance(current_balance, &receiver_ct)?;

            // Prepare transcript for CiphertextValidityProof
            transcript.transfer_proof_domain_separator();
            transcript.append_public_key(b"dest_pubkey", transfer.get_destination());
            transcript.append_commitment(b"amount_commitment", transfer.get_commitment());
            transcript.append_handle(b"amount_sender_handle", transfer.get_sender_handle());
            transcript.append_handle(b"amount_receiver_handle", transfer.get_receiver_handle());

            // Pre-verify the validity proof (adds to batch collector)
            transfer.get_proof().pre_verify(
                &decompressed.commitment,
                &receiver,
                &source_decompressed,
                &decompressed.receiver_handle,
                &decompressed.sender_handle,
                self.version,
                &mut transcript,
                sigma_batch_collector,
            )?;

            // Collect commitment for range proof
            value_commitments.push((
                *decompressed.commitment.as_point(),
                *transfer.get_commitment().as_point(),
            ));
        }

        // 3. Prepare commitments for range proof verification
        // Count total commitments (source + transfer)
        let n_commitments = self.source_commitments.len() + value_commitments.len();
        let n_dud_commitments = n_commitments
            .checked_next_power_of_two()
            .ok_or(ProofVerificationError::Format)?
            - n_commitments;

        // Combine source and transfer commitments
        let final_commitments = self
            .source_commitments
            .iter()
            .zip(new_source_commitments_decompressed)
            .map(|(commitment, new_source_commitment)| {
                (
                    new_source_commitment.to_point(),
                    *commitment.get_commitment().as_point(),
                )
            })
            .chain(value_commitments)
            .chain(iter::repeat_n(
                (RistrettoPoint::identity(), CompressedRistretto::identity()),
                n_dud_commitments,
            ))
            .collect();

        Ok((transcript, final_commitments))
    }

    /// Pre-verify an Unshield transaction (UNO -> TOS)
    /// Returns the transcript and commitments needed for range proof batch verification
    async fn pre_verify_unshield<'a, E, B: BlockchainVerificationState<'a, E> + Send>(
        &'a self,
        tx_hash: &'a Hash,
        state: &mut B,
        sigma_batch_collector: &mut BatchCollector,
    ) -> Result<(Transcript, Vec<(RistrettoPoint, CompressedRistretto)>), VerificationError<E>>
    {
        if log::log_enabled!(log::Level::Trace) {
            trace!("Pre-verifying Unshield transaction");
        }

        if !self.has_valid_version_format() {
            return Err(VerificationError::InvalidFormat);
        }

        // Unshield transactions must have source commitments and range proof
        if self.source_commitments.is_empty() {
            return Err(VerificationError::Commitments);
        }

        let Some(ref _range_proof) = self.range_proof else {
            return Err(VerificationError::Proof(ProofVerificationError::Format));
        };

        // Verify source commitment assets match Unshield transfer assets
        if !self.verify_unshield_commitment_assets() {
            if log::log_enabled!(log::Level::Debug) {
                debug!("Invalid Unshield commitment assets");
            }
            return Err(VerificationError::Commitments);
        }

        // Pre-verify on state (nonce check, etc.)
        state
            .pre_verify_tx(self)
            .await
            .map_err(VerificationError::State)?;

        // Atomically check and update nonce
        let success = state
            .compare_and_swap_nonce(&self.source, self.nonce, self.nonce + 1)
            .await
            .map_err(VerificationError::State)?;

        if !success {
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

        // Decompress Unshield transfers
        let TransactionType::UnshieldTransfers(transfers) = &self.data else {
            return Err(VerificationError::InvalidFormat);
        };

        if transfers.len() > MAX_TRANSFER_COUNT || transfers.is_empty() {
            if log::log_enabled!(log::Level::Debug) {
                debug!("incorrect Unshield transfers size: {}", transfers.len());
            }
            return Err(VerificationError::TransferCount);
        }

        // Validate extra data and decompress transfers
        // Note: For Unshield, sender == receiver is valid (unshielding to own address)
        let mut extra_data_size = 0;
        let mut transfers_decompressed = Vec::with_capacity(transfers.len());

        for transfer in transfers.iter() {
            if let Some(extra_data) = transfer.get_extra_data() {
                let size = extra_data.size();
                if size > EXTRA_DATA_LIMIT_SIZE {
                    return Err(VerificationError::TransferExtraDataSize);
                }
                extra_data_size += size;
            }

            let decompressed = DecompressedUnshieldTransferCt::decompress(transfer)
                .map_err(ProofVerificationError::from)?;
            transfers_decompressed.push(decompressed);
        }

        if extra_data_size > EXTRA_DATA_LIMIT_SUM_SIZE {
            return Err(VerificationError::TransactionExtraDataSize);
        }

        // Decompress source commitments
        let new_source_commitments_decompressed = self
            .source_commitments
            .iter()
            .map(|c| c.get_commitment().decompress())
            .collect::<Result<Vec<_>, DecompressionError>>()
            .map_err(ProofVerificationError::from)?;

        let source_decompressed = self
            .source
            .decompress()
            .map_err(|err| VerificationError::Proof(err.into()))?;

        // Prepare transcript for proof verification
        let mut transcript = Self::prepare_transcript(
            self.version,
            &self.source,
            self.fee,
            &self.fee_type,
            self.nonce,
        );

        self.verify_signature_and_multisig(state, &source_decompressed)
            .await?;

        // IMPORTANT: Proof verification order MUST match proof generation order in build_unshield_unsigned!
        // Generation order: 1) CiphertextValidityProofs for transfers, 2) CommitmentEqProof for source
        // The transcript state must be identical between generation and verification.

        // 1. Verify CiphertextValidityProofs for transfers (FIRST - matches generation order)
        if log::log_enabled!(log::Level::Trace) {
            trace!("verifying Unshield transfer ciphertext validity proofs");
        }

        let mut value_commitments: Vec<(RistrettoPoint, CompressedRistretto)> = Vec::new();

        for (transfer, decompressed) in transfers.iter().zip(&transfers_decompressed) {
            // For Unshield, the destination receives plaintext TOS, not encrypted UNO
            // So we don't update receiver's UNO balance here

            // Prepare transcript for CiphertextValidityProof
            transcript.transfer_proof_domain_separator();
            transcript.append_public_key(b"dest_pubkey", transfer.get_destination());
            transcript.append_commitment(b"amount_commitment", transfer.get_commitment());
            transcript.append_handle(b"amount_sender_handle", transfer.get_sender_handle());

            // For Unshield, we verify the sender handle proves the commitment matches the amount
            // The receiver is a plaintext recipient (no receiver handle needed)
            let dest_pubkey = transfer
                .get_destination()
                .decompress()
                .map_err(ProofVerificationError::from)?;

            // Pre-verify the validity proof (adds to batch collector)
            // Use sender handle for both since receiver gets plaintext
            transfer.get_proof().pre_verify(
                &decompressed.commitment,
                &source_decompressed,
                &dest_pubkey,
                &decompressed.sender_handle,
                &decompressed.sender_handle,
                self.version,
                &mut transcript,
                sigma_batch_collector,
            )?;

            // Collect commitment for range proof
            value_commitments.push((
                *decompressed.commitment.as_point(),
                *transfer.get_commitment().as_point(),
            ));
        }

        // 2. Verify CommitmentEqProofs for source balances (SECOND - matches generation order)
        if log::log_enabled!(log::Level::Trace) {
            trace!("verifying Unshield commitments eq proofs");
        }

        for (commitment, new_source_commitment) in self
            .source_commitments
            .iter()
            .zip(&new_source_commitments_decompressed)
        {
            // Calculate output ciphertext (total spending for this asset)
            let output = self.get_unshield_sender_output_ct(&transfers_decompressed);

            // Get sender's UNO balance ciphertext
            let source_verification_ciphertext = state
                .get_sender_uno_balance(&self.source, commitment.get_asset(), &self.reference)
                .await
                .map_err(VerificationError::State)?;

            let source_ct_compressed = source_verification_ciphertext.compress();

            // Compute new balance: old_balance - output
            *source_verification_ciphertext -= &output;

            // Prepare transcript for CommitmentEqProof
            transcript.new_commitment_eq_proof_domain_separator();
            transcript.append_hash(b"new_source_commitment_asset", commitment.get_asset());
            transcript.append_commitment(b"new_source_commitment", commitment.get_commitment());

            // Only append source_ct for version >= T1 (matches generation)
            if self.version >= TxVersion::T1 {
                transcript.append_ciphertext(b"source_ct", &source_ct_compressed);
            }

            // Pre-verify the equality proof (adds to batch collector)
            commitment.get_proof().pre_verify(
                &source_decompressed,
                source_verification_ciphertext,
                new_source_commitment,
                &mut transcript,
                sigma_batch_collector,
            )?;

            // Track sender output for final balance
            state
                .add_sender_uno_output(&self.source, commitment.get_asset(), output)
                .await
                .map_err(VerificationError::State)?;
        }

        // 3. Prepare commitments for range proof verification
        // Count total commitments (source + transfer)
        let n_commitments = self.source_commitments.len() + value_commitments.len();
        let n_dud_commitments = n_commitments
            .checked_next_power_of_two()
            .ok_or(ProofVerificationError::Format)?
            - n_commitments;

        // Combine source and transfer commitments
        let final_commitments = self
            .source_commitments
            .iter()
            .zip(new_source_commitments_decompressed)
            .map(|(commitment, new_source_commitment)| {
                (
                    new_source_commitment.to_point(),
                    *commitment.get_commitment().as_point(),
                )
            })
            .chain(value_commitments)
            .chain(iter::repeat_n(
                (RistrettoPoint::identity(), CompressedRistretto::identity()),
                n_dud_commitments,
            ))
            .collect();

        Ok((transcript, final_commitments))
    }

    // This method no longer needs to return transcript or commitments
    // Signature kept for compatibility during refactoring
    async fn pre_verify<'a, E, B: BlockchainVerificationState<'a, E> + Send>(
        &'a self,
        tx_hash: &'a Hash,
        state: &mut B,
    ) -> Result<(), VerificationError<E>> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("Pre-verifying transaction");
        }
        if !self.has_valid_version_format() {
            return Err(VerificationError::InvalidFormat);
        }

        if self.get_fee_type().is_uno() {
            match &self.data {
                TransactionType::UnoTransfers(_) => {
                    if self.fee != 0 {
                        return Err(VerificationError::InvalidFee(0, self.fee));
                    }

                    if !self
                        .source_commitments
                        .iter()
                        .any(|c| c.get_asset() == &UNO_ASSET)
                    {
                        return Err(VerificationError::Commitments);
                    }
                }
                _ => {
                    return Err(VerificationError::InvalidFormat);
                }
            }
        }

        if log::log_enabled!(log::Level::Trace) {
            trace!("Pre-verifying transaction on state");
        }
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
                    return Err(VerificationError::InvalidTransferAmount);
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
            TransactionType::UnoTransfers(transfers) => {
                // UNO transfers: privacy-preserving transfers with ZKP proofs
                if transfers.len() > MAX_TRANSFER_COUNT || transfers.is_empty() {
                    if log::log_enabled!(log::Level::Debug) {
                        debug!("incorrect UNO transfers size: {}", transfers.len());
                    }
                    return Err(VerificationError::TransferCount);
                }

                let mut extra_data_size = 0;
                for transfer in transfers.iter() {
                    if *transfer.get_destination() == self.source {
                        if log::log_enabled!(log::Level::Debug) {
                            debug!("sender cannot be the receiver in the same TX");
                        }
                        return Err(VerificationError::SenderIsReceiver);
                    }

                    if let Some(extra_data) = transfer.get_extra_data() {
                        let size = extra_data.size();
                        if size > EXTRA_DATA_LIMIT_SIZE {
                            return Err(VerificationError::TransferExtraDataSize);
                        }
                        extra_data_size += size;
                    }
                }

                if extra_data_size > EXTRA_DATA_LIMIT_SUM_SIZE {
                    return Err(VerificationError::TransactionExtraDataSize);
                }
            }
            TransactionType::ShieldTransfers(transfers) => {
                // Shield transfers: TOS -> UNO
                if transfers.len() > MAX_TRANSFER_COUNT || transfers.is_empty() {
                    return Err(VerificationError::TransferCount);
                }

                let mut extra_data_size = 0;
                for transfer in transfers.iter() {
                    if transfer.get_amount() == 0 {
                        return Err(VerificationError::InvalidTransferAmount);
                    }

                    // Validate minimum Shield amount (anti-money-laundering measure)
                    if transfer.get_amount() < MIN_SHIELD_TOS_AMOUNT {
                        return Err(VerificationError::ShieldAmountTooLow);
                    }

                    // Shield transfers only support TOS asset
                    // UNO is a single-asset privacy layer for TOS only
                    if *transfer.get_asset() != TOS_ASSET {
                        return Err(VerificationError::AnyError(anyhow!(
                            "Shield transfers only support TOS asset"
                        )));
                    }

                    // SECURITY: Verify Shield commitment proof
                    // This ensures the commitment is correctly formed for the claimed amount
                    // and prevents inflation attacks via forged commitments
                    let commitment = transfer
                        .get_commitment()
                        .decompress()
                        .map_err(|_| VerificationError::InvalidFormat)?;
                    let receiver_pubkey = transfer
                        .get_destination()
                        .decompress()
                        .map_err(|_| VerificationError::InvalidFormat)?;
                    let receiver_handle = transfer
                        .get_receiver_handle()
                        .decompress()
                        .map_err(|_| VerificationError::InvalidFormat)?;

                    let mut transcript = Transcript::new(b"shield_commitment_proof");
                    transfer
                        .get_proof()
                        .verify(
                            &commitment,
                            &receiver_pubkey,
                            &receiver_handle,
                            transfer.get_amount(),
                            &mut transcript,
                        )
                        .map_err(|e| {
                            if log::log_enabled!(log::Level::Debug) {
                                debug!("Shield commitment proof verification failed: {:?}", e);
                            }
                            VerificationError::Proof(e)
                        })?;

                    if let Some(extra_data) = transfer.get_extra_data() {
                        let size = extra_data.size();
                        if size > EXTRA_DATA_LIMIT_SIZE {
                            return Err(VerificationError::TransferExtraDataSize);
                        }
                        extra_data_size += size;
                    }
                }

                if extra_data_size > EXTRA_DATA_LIMIT_SUM_SIZE {
                    return Err(VerificationError::TransactionExtraDataSize);
                }
            }
            TransactionType::UnshieldTransfers(transfers) => {
                // Unshield transfers: UNO -> TOS
                if transfers.len() > MAX_TRANSFER_COUNT || transfers.is_empty() {
                    return Err(VerificationError::TransferCount);
                }

                let mut extra_data_size = 0;
                for transfer in transfers.iter() {
                    if transfer.get_amount() == 0 {
                        return Err(VerificationError::InvalidTransferAmount);
                    }

                    if let Some(extra_data) = transfer.get_extra_data() {
                        let size = extra_data.size();
                        if size > EXTRA_DATA_LIMIT_SIZE {
                            return Err(VerificationError::TransferExtraDataSize);
                        }
                        extra_data_size += size;
                    }
                }

                if extra_data_size > EXTRA_DATA_LIMIT_SUM_SIZE {
                    return Err(VerificationError::TransactionExtraDataSize);
                }
            }
            TransactionType::RegisterName(payload) => {
                // TNS RegisterName: stateless format validation + stateful checks
                verify_register_name_format::<E>(payload)?;

                // Fee verification: minimum registration fee required
                verify_register_name_fee::<E>(self.fee)?;

                // Stateful checks: name not taken, account doesn't have name
                let name_hash = get_register_name_hash(payload)
                    .ok_or_else(|| VerificationError::InvalidFormat)?;

                if state
                    .is_name_registered(&name_hash)
                    .await
                    .map_err(VerificationError::State)?
                {
                    return Err(VerificationError::NameAlreadyRegistered);
                }

                if state
                    .account_has_name(&self.source)
                    .await
                    .map_err(VerificationError::State)?
                {
                    return Err(VerificationError::AccountAlreadyHasName);
                }
            }
        };

        // 0.a Verify chain_id for T1+ transactions (cross-network replay protection)
        if self.version >= TxVersion::T1 {
            let expected_chain_id = state.get_network().chain_id() as u8;
            if self.chain_id != expected_chain_id {
                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "transaction chain_id mismatch: expected {}, got {}",
                        expected_chain_id, self.chain_id
                    );
                }
                return Err(VerificationError::InvalidChainId {
                    expected: expected_chain_id,
                    got: self.chain_id,
                });
            }
        }

        // 0.b Verify signature and multisig requirements
        let source_decompressed = self
            .source
            .decompress()
            .map_err(|err| VerificationError::Proof(err.into()))?;
        self.verify_signature_and_multisig(state, &source_decompressed)
            .await?;

        // Balance verification handled by plaintext balance system
        if log::log_enabled!(log::Level::Trace) {
            trace!("Balance verification handled by plaintext balance system");
        }

        // With plaintext balances, we no longer need:
        // - CiphertextValidityProof verification
        // - Pedersen commitment verification
        // - Twisted ElGamal decrypt handle verification

        // New plaintext approach (to be implemented):
        // For each transfer:
        // 1. Get receiver's current balance from state
        // 2. Add transfer.amount to receiver balance
        // 3. Update state with new receiver balance

        if log::log_enabled!(log::Level::Trace) {
            trace!("Processing transfers with plaintext amounts");
        }

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
            TransactionType::UnoTransfers(_)
            | TransactionType::ShieldTransfers(_)
            | TransactionType::UnshieldTransfers(_) => {
                // UNO/Shield/Unshield transfers are verified through ZKP proofs
                // Logging handled during apply phase
            }
            TransactionType::RegisterName(_) => {
                // TNS transactions: verification handled in dedicated functions
            }
        }

        // With plaintext balances, we don't need Bulletproofs range proofs
        // Balances are plain u64, always in valid range [0, 2^64)
        if log::log_enabled!(log::Level::Trace) {
            trace!("Skipping range proof verification (plaintext balances)");
        }

        // SECURITY FIX: Check balances inline (can't call verify_dynamic_parts as it also does CAS nonce update)
        // Calculate total spending per asset and verify sender has sufficient balance
        let mut spending_per_asset: IndexMap<Hash, u64> = IndexMap::new();

        match &self.data {
            TransactionType::Transfers(transfers) => {
                for transfer in transfers {
                    let asset = transfer.get_asset();
                    let amount = transfer.get_amount();
                    let current = spending_per_asset.entry(asset.clone()).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::Burn(payload) => {
                let current = spending_per_asset.entry(payload.asset.clone()).or_insert(0);
                *current = current
                    .checked_add(payload.amount)
                    .ok_or(VerificationError::Overflow)?;
            }
            TransactionType::InvokeContract(payload) => {
                for (asset, deposit) in &payload.deposits {
                    let amount = deposit
                        .get_amount()
                        .map_err(|e| VerificationError::AnyError(anyhow!(e)))?;
                    let current = spending_per_asset.entry(asset.clone()).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
                // Add max_gas to TOS spending
                let current = spending_per_asset.entry(TOS_ASSET.clone()).or_insert(0);
                *current = current
                    .checked_add(payload.max_gas)
                    .ok_or(VerificationError::Overflow)?;
            }
            TransactionType::DeployContract(payload) => {
                // Add BURN_PER_CONTRACT to TOS spending
                let current = spending_per_asset.entry(TOS_ASSET.clone()).or_insert(0);
                *current = current
                    .checked_add(BURN_PER_CONTRACT)
                    .ok_or(VerificationError::Overflow)?;

                if let Some(invoke) = &payload.invoke {
                    for (asset, deposit) in &invoke.deposits {
                        let amount = deposit
                            .get_amount()
                            .map_err(|e| VerificationError::AnyError(anyhow!(e)))?;
                        let current = spending_per_asset.entry(asset.clone()).or_insert(0);
                        *current = current
                            .checked_add(amount)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    // Add max_gas to TOS spending
                    let current = spending_per_asset.entry(TOS_ASSET.clone()).or_insert(0);
                    *current = current
                        .checked_add(invoke.max_gas)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::MultiSig(_) => {
                // No asset spending for these types
            }
            TransactionType::UnoTransfers(_) => {
                // UNO transfers spend from encrypted balances
                // Spending verification is done through ZKP proofs (CommitmentEqProof)
                // No plaintext spending to verify here
            }
            TransactionType::ShieldTransfers(transfers) => {
                // Shield transfers spend from plaintext TOS balance
                // Amount is public and verifiable
                for transfer in transfers {
                    let asset = transfer.get_asset();
                    let amount = transfer.get_amount();
                    let current = spending_per_asset.entry(asset.clone()).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::UnshieldTransfers(_) => {
                // Unshield transfers spend from encrypted UNO balances
                // Spending verification is done through ZKP proofs
                // No plaintext spending to verify here (adds to plaintext balance)
            }
            TransactionType::RegisterName(_) => {
                // TNS transactions: registration/message fees are paid via the fee field
                // No additional spending verification needed here
            }
        };

        // Add fee to TOS spending
        if self.fee > 0 {
            let current = spending_per_asset.entry(TOS_ASSET.clone()).or_insert(0);
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
                .get_sender_balance(
                    Cow::Borrowed(&self.source),
                    Cow::Owned(asset_hash.clone()),
                    &self.reference,
                )
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

        Ok(())
    }

    pub async fn verify_batch<'a, H, E, B, C>(
        txs: impl Iterator<Item = &'a (Arc<Transaction>, H)>,
        state: &mut B,
        cache: &C,
    ) -> Result<(), VerificationError<E>>
    where
        H: AsRef<Hash> + 'a,
        B: BlockchainVerificationState<'a, E> + Send,
        C: ZKPCache<E>,
    {
        if log::log_enabled!(log::Level::Trace) {
            trace!("Verifying batch of transactions");
        }

        // Batch collector for sigma proofs (CommitmentEqProof, CiphertextValidityProof)
        let mut sigma_batch_collector = BatchCollector::default();

        // Prepared UNO transactions for range proof verification
        let mut uno_prepared: Vec<UnoPreparedData> = Vec::new();

        for (tx, hash) in txs {
            let hash = hash.as_ref();

            // In case the cache already knows this TX
            // we don't need to spend time reverifying it again
            // because a TX is immutable, we can just verify the mutable parts
            // (balance & nonce related)
            let dynamic_parts_only = cache
                .is_already_verified(hash)
                .await
                .map_err(VerificationError::State)?;

            // Check if this is a UNO or Unshield transaction (both require ZKP verification)
            let is_uno = matches!(tx.data, TransactionType::UnoTransfers(_));
            let is_unshield = matches!(tx.data, TransactionType::UnshieldTransfers(_));

            if dynamic_parts_only {
                if log::log_enabled!(log::Level::Debug) {
                    debug!("TX {hash} is known from ZKPCache, verifying dynamic parts only");
                }
                tx.verify_dynamic_parts(hash, state).await?;
            } else if is_uno {
                // UNO transactions require ZKP verification
                let (transcript, commitments) = tx
                    .pre_verify_uno(hash, state, &mut sigma_batch_collector)
                    .await?;
                uno_prepared.push((tx.clone(), transcript, commitments));
            } else if is_unshield {
                // Unshield transactions require ZKP verification (UNO -> TOS)
                let (transcript, commitments) = tx
                    .pre_verify_unshield(hash, state, &mut sigma_batch_collector)
                    .await?;
                uno_prepared.push((tx.clone(), transcript, commitments));
            } else {
                // Regular plaintext transaction
                tx.pre_verify(hash, state).await?;
            }
        }

        // Verify ZK proofs if there are any UNO transactions
        if !uno_prepared.is_empty() {
            // Spawn a dedicated thread for the ZK Proofs verification
            // to prevent blocking the async runtime
            spawn_blocking_safe(move || {
                // Verify sigma proofs (CommitmentEqProof, CiphertextValidityProof)
                sigma_batch_collector
                    .verify()
                    .map_err(|_| ProofVerificationError::GenericProof)?;

                // Verify range proofs in batch
                // First collect all verification views, checking for missing proofs
                let verification_views: Vec<_> = uno_prepared
                    .iter_mut()
                    .map(|(tx, transcript, commitments)| {
                        tx.range_proof
                            .as_ref()
                            .ok_or(ProofVerificationError::MissingRangeProof)
                            .map(|proof| {
                                proof.verification_view(transcript, commitments, BULLET_PROOF_SIZE)
                            })
                    })
                    .collect::<Result<_, _>>()?;

                RangeProof::verify_batch(verification_views.into_iter(), &BP_GENS, &PC_GENS)
                    .map_err(ProofVerificationError::from)
            })
            .await
            .context("spawning blocking thread for ZK verification")??;
        }

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
        B: BlockchainVerificationState<'a, E> + Send,
        C: ZKPCache<E>,
    {
        let dynamic_parts_only = cache
            .is_already_verified(tx_hash)
            .await
            .map_err(VerificationError::State)?;

        // Check if this is a UNO or Unshield transaction (both require ZKP verification)
        let is_uno = matches!(self.data, TransactionType::UnoTransfers(_));
        let is_unshield = matches!(self.data, TransactionType::UnshieldTransfers(_));

        if dynamic_parts_only {
            if log::log_enabled!(log::Level::Debug) {
                debug!("TX {tx_hash} is known from ZKPCache, verifying dynamic parts only");
            }
            self.verify_dynamic_parts(tx_hash, state).await?;
        } else if is_uno {
            // UNO transactions require full ZKP verification
            let mut sigma_batch_collector = BatchCollector::default();
            let (mut transcript, commitments) = self
                .pre_verify_uno(tx_hash, state, &mut sigma_batch_collector)
                .await?;

            // Verify ZK proofs synchronously for single transaction
            let tx_clone = Arc::clone(self);
            spawn_blocking_safe(move || {
                if log::log_enabled!(log::Level::Trace) {
                    trace!("Verifying UNO sigma proofs");
                }
                sigma_batch_collector
                    .verify()
                    .map_err(|_| ProofVerificationError::GenericProof)?;

                if log::log_enabled!(log::Level::Trace) {
                    trace!("Verifying UNO range proof");
                }
                let range_proof = tx_clone
                    .range_proof
                    .as_ref()
                    .ok_or(ProofVerificationError::MissingRangeProof)?;
                RangeProof::verify_multiple(
                    range_proof,
                    &BP_GENS,
                    &PC_GENS,
                    &mut transcript,
                    &commitments,
                    BULLET_PROOF_SIZE,
                )
                .map_err(ProofVerificationError::from)
            })
            .await
            .context("spawning blocking thread for ZK verification")??;
        } else if is_unshield {
            // Unshield transactions require full ZKP verification (UNO -> TOS)
            let mut sigma_batch_collector = BatchCollector::default();
            let (mut transcript, commitments) = self
                .pre_verify_unshield(tx_hash, state, &mut sigma_batch_collector)
                .await?;

            // Verify ZK proofs synchronously for single transaction
            let tx_clone = Arc::clone(self);
            spawn_blocking_safe(move || {
                if log::log_enabled!(log::Level::Trace) {
                    trace!("Verifying Unshield sigma proofs");
                }
                sigma_batch_collector
                    .verify()
                    .map_err(|_| ProofVerificationError::GenericProof)?;

                if log::log_enabled!(log::Level::Trace) {
                    trace!("Verifying Unshield range proof");
                }
                let range_proof = tx_clone
                    .range_proof
                    .as_ref()
                    .ok_or(ProofVerificationError::MissingRangeProof)?;
                RangeProof::verify_multiple(
                    range_proof,
                    &BP_GENS,
                    &PC_GENS,
                    &mut transcript,
                    &commitments,
                    BULLET_PROOF_SIZE,
                )
                .map_err(ProofVerificationError::from)
            })
            .await
            .context("spawning blocking thread for ZK verification")??;
        } else {
            // Regular plaintext transaction
            self.pre_verify(tx_hash, state).await?;
        };

        Ok(())
    }

    // Apply the transaction to the state
    // Arc is required around Self to be shared easily into the VM if needed
    async fn apply<'a, P: ContractProvider + Send, E, B: BlockchainApplyState<'a, P, E> + Send>(
        self: &'a Arc<Self>,
        tx_hash: &'a Hash,
        state: &mut B,
    ) -> Result<(), VerificationError<E>> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("Applying transaction data");
        }
        // Update nonce
        state
            .update_account_nonce(self.get_source(), self.nonce + 1)
            .await
            .map_err(VerificationError::State)?;

        // SECURITY FIX: Deduct sender balances BEFORE adding to receivers
        // Calculate total spending per asset for sender deduction
        // Use references to original Hash values in transaction (they live for 'a)
        let mut spending_per_asset: IndexMap<Hash, u64> = IndexMap::new();

        match &self.data {
            TransactionType::Transfers(transfers) => {
                for transfer in transfers {
                    let asset = transfer.get_asset(); // Returns &Hash
                    let amount = transfer.get_amount();
                    let current = spending_per_asset.entry(asset.clone()).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::Burn(payload) => {
                let current = spending_per_asset.entry(payload.asset.clone()).or_insert(0);
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
                    let current = spending_per_asset.entry(asset.clone()).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
                // Add max_gas to TOS spending
                let current = spending_per_asset.entry(TOS_ASSET.clone()).or_insert(0);
                *current = current
                    .checked_add(payload.max_gas)
                    .ok_or(VerificationError::Overflow)?;
            }
            TransactionType::DeployContract(payload) => {
                // Add BURN_PER_CONTRACT to TOS spending
                let current = spending_per_asset.entry(TOS_ASSET.clone()).or_insert(0);
                *current = current
                    .checked_add(BURN_PER_CONTRACT)
                    .ok_or(VerificationError::Overflow)?;

                // If invoking constructor, add deposits and max_gas
                if let Some(invoke) = &payload.invoke {
                    for (asset, deposit) in &invoke.deposits {
                        let amount = deposit
                            .get_amount()
                            .map_err(|e| VerificationError::AnyError(anyhow!(e)))?;
                        let current = spending_per_asset.entry(asset.clone()).or_insert(0);
                        *current = current
                            .checked_add(amount)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    // Add max_gas to TOS spending
                    let current = spending_per_asset.entry(TOS_ASSET.clone()).or_insert(0);
                    *current = current
                        .checked_add(invoke.max_gas)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::MultiSig(_) => {
                // No asset spending for these types
            }
            TransactionType::UnoTransfers(_) => {
                // UNO transfers spend from encrypted balances
                // Spending verification is done through ZKP proofs (CommitmentEqProof)
                // No plaintext spending to verify here
            }
            TransactionType::ShieldTransfers(transfers) => {
                // Shield transfers spend from plaintext TOS balance
                // Amount is public and verifiable
                for transfer in transfers {
                    let asset = transfer.get_asset();
                    let amount = transfer.get_amount();
                    let current = spending_per_asset.entry(asset.clone()).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::UnshieldTransfers(_) => {
                // Unshield transfers spend from encrypted UNO balances
                // Spending verification is done through ZKP proofs
                // No plaintext spending to verify here (adds to plaintext balance)
            }
            TransactionType::RegisterName(_) => {
                // TNS transactions: registration/message fees are paid via the fee field
                // No additional spending verification needed here
            }
        };

        // Add fee to TOS spending
        if self.fee > 0 {
            let current = spending_per_asset.entry(TOS_ASSET.clone()).or_insert(0);
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
                .get_sender_balance(
                    Cow::Borrowed(&self.source),
                    Cow::Owned(asset_hash.clone()),
                    &self.reference,
                )
                .await
                .map_err(VerificationError::State)?;

            // Track the spending in output_sum for final balance calculation
            state
                .add_sender_output(
                    Cow::Borrowed(&self.source),
                    Cow::Owned(asset_hash.clone()),
                    *total_spending,
                )
                .await
                .map_err(VerificationError::State)?;
        }

        // Handle UNO (encrypted) balance spending for UnshieldTransfers
        // This is separate from plaintext spending because it uses homomorphic ciphertext operations
        if let TransactionType::UnshieldTransfers(transfers) = &self.data {
            if log::log_enabled!(log::Level::Trace) {
                trace!(
                    "apply: Processing UnshieldTransfers UNO spending for source {:?}",
                    self.source
                );
            }

            // Decompress transfer ciphertexts to compute total spending
            let mut output = Ciphertext::zero();
            for transfer in transfers.iter() {
                let decompressed = DecompressedUnshieldTransferCt::decompress(transfer)
                    .map_err(ProofVerificationError::from)?;
                output += decompressed.get_sender_ciphertext();
            }

            // Get sender's UNO balance and deduct spending
            let source_uno_balance = state
                .get_sender_uno_balance(&self.source, &UNO_ASSET, &self.reference)
                .await
                .map_err(VerificationError::State)?;

            // Subtract output from UNO balance (homomorphic subtraction)
            *source_uno_balance -= &output;

            // Track the spending for final balance calculation
            state
                .add_sender_uno_output(&self.source, &UNO_ASSET, output)
                .await
                .map_err(VerificationError::State)?;
        }

        // Handle UNO (encrypted) balance spending for UnoTransfers
        // This is separate from plaintext spending because it uses homomorphic ciphertext operations
        if let TransactionType::UnoTransfers(transfers) = &self.data {
            if log::log_enabled!(log::Level::Trace) {
                trace!(
                    "apply: Processing UnoTransfers UNO spending for source {:?}",
                    self.source
                );
            }

            // Decompress transfer ciphertexts to compute total spending
            let mut transfers_decompressed = Vec::with_capacity(transfers.len());
            for transfer in transfers.iter() {
                let decompressed = DecompressedUnoTransferCt::decompress(transfer)
                    .map_err(ProofVerificationError::from)?;
                transfers_decompressed.push(decompressed);
            }

            let output = self.get_uno_sender_output_ct(&UNO_ASSET, &transfers_decompressed)?;

            // Get sender's UNO balance and deduct spending
            let source_uno_balance = state
                .get_sender_uno_balance(&self.source, &UNO_ASSET, &self.reference)
                .await
                .map_err(VerificationError::State)?;

            // Subtract output from UNO balance (homomorphic subtraction)
            *source_uno_balance -= &output;

            // Track the spending for final balance calculation
            state
                .add_sender_uno_output(&self.source, &UNO_ASSET, output)
                .await
                .map_err(VerificationError::State)?;

            if self.get_fee_type().is_uno() {
                let uno_fee = UNO_BURN_FEE_PER_TRANSFER
                    .checked_mul(transfers.len() as u64)
                    .ok_or(VerificationError::Overflow)?;
                state
                    .add_burned_coins(uno_fee)
                    .await
                    .map_err(VerificationError::State)?;
            }
        }

        // Apply receiver balances
        match &self.data {
            TransactionType::Transfers(transfers) => {
                for transfer in transfers {
                    // Update receiver balance with plain u64 amount
                    let current_balance = Box::pin(state.get_receiver_balance(
                        Cow::Borrowed(transfer.get_destination()),
                        Cow::Borrowed(transfer.get_asset()),
                    ))
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
            TransactionType::UnoTransfers(transfers) => {
                // UNO transfers: privacy-preserving transfers
                // Update receiver balances with encrypted ciphertexts
                for transfer in transfers {
                    let receiver_ct = transfer
                        .get_ciphertext(Role::Receiver)
                        .decompress()
                        .map_err(ProofVerificationError::from)?;

                    // Get receiver's UNO balance and add the ciphertext
                    let current_balance = state
                        .get_receiver_uno_balance(
                            Cow::Borrowed(transfer.get_destination()),
                            Cow::Borrowed(transfer.get_asset()),
                        )
                        .await
                        .map_err(VerificationError::State)?;

                    add_uno_balance(current_balance, &receiver_ct)?;

                    if log::log_enabled!(log::Level::Debug) {
                        debug!(
                            "UNO transfer applied - receiver: {:?}, asset: {:?}",
                            transfer.get_destination(),
                            transfer.get_asset()
                        );
                    }
                }
            }
            TransactionType::ShieldTransfers(transfers) => {
                // Shield transfers: TOS -> UNO
                // TOS is deducted via spending_per_asset (plaintext deduction)
                // Add encrypted UNO balance to receiver
                for transfer in transfers {
                    // Create ciphertext from commitment and receiver handle
                    // Ciphertext = (Commitment, ReceiverHandle)
                    let commitment = transfer
                        .get_commitment()
                        .decompress()
                        .map_err(|_| VerificationError::InvalidFormat)?;
                    let receiver_handle = transfer
                        .get_receiver_handle()
                        .decompress()
                        .map_err(|_| VerificationError::InvalidFormat)?;
                    let receiver_ct = Ciphertext::new(commitment, receiver_handle);

                    // Get receiver's UNO balance and add the ciphertext
                    let current_balance = state
                        .get_receiver_uno_balance(
                            Cow::Borrowed(transfer.get_destination()),
                            Cow::Borrowed(&UNO_ASSET),
                        )
                        .await
                        .map_err(VerificationError::State)?;

                    add_uno_balance(current_balance, &receiver_ct)?;

                    if log::log_enabled!(log::Level::Debug) {
                        debug!(
                            "Shield transfer applied - receiver: {:?}, amount: {}",
                            transfer.get_destination(),
                            transfer.get_amount()
                        );
                    }
                }
            }
            TransactionType::UnshieldTransfers(transfers) => {
                // Unshield transfers: UNO -> TOS
                // Sender UNO balance is deducted in verify_dynamic_parts
                // Here we only add plaintext TOS balance to receiver
                for transfer in transfers {
                    // Add plaintext amount to receiver's TOS balance
                    let current_balance = state
                        .get_receiver_balance(
                            Cow::Borrowed(transfer.get_destination()),
                            Cow::Borrowed(&TOS_ASSET),
                        )
                        .await
                        .map_err(VerificationError::State)?;

                    *current_balance = current_balance
                        .checked_add(transfer.get_amount())
                        .ok_or(VerificationError::Overflow)?;

                    if log::log_enabled!(log::Level::Debug) {
                        debug!(
                            "Unshield transfer applied - receiver: {:?}, amount: {}",
                            transfer.get_destination(),
                            transfer.get_amount()
                        );
                    }
                }
            }
            TransactionType::RegisterName(payload) => {
                // TNS RegisterName: store name->account mapping
                let name_hash = get_register_name_hash(payload)
                    .ok_or_else(|| VerificationError::InvalidFormat)?;

                if log::log_enabled!(log::Level::Debug) {
                    debug!(
                        "RegisterName applying - owner: {:?}, name_hash: {}",
                        self.source, name_hash
                    );
                }

                state
                    .register_name(name_hash, &self.source)
                    .await
                    .map_err(VerificationError::State)?;
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
        B: BlockchainApplyState<'a, P, E> + Send,
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
        let mut spending_per_asset: IndexMap<Hash, u64> = IndexMap::new();

        match &self.data {
            TransactionType::Transfers(transfers) => {
                for transfer in transfers {
                    let asset = transfer.get_asset();
                    let amount = transfer.get_amount();
                    let current = spending_per_asset.entry(asset.clone()).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::Burn(payload) => {
                let current = spending_per_asset.entry(payload.asset.clone()).or_insert(0);
                *current = current
                    .checked_add(payload.amount)
                    .ok_or(VerificationError::Overflow)?;
            }
            TransactionType::InvokeContract(payload) => {
                for (asset, deposit) in &payload.deposits {
                    let amount = deposit
                        .get_amount()
                        .map_err(|e| VerificationError::AnyError(anyhow!(e)))?;
                    let current = spending_per_asset.entry(asset.clone()).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
                // Add max_gas to TOS spending
                let current = spending_per_asset.entry(TOS_ASSET.clone()).or_insert(0);
                *current = current
                    .checked_add(payload.max_gas)
                    .ok_or(VerificationError::Overflow)?;
            }
            TransactionType::DeployContract(payload) => {
                // Add BURN_PER_CONTRACT to TOS spending
                let current = spending_per_asset.entry(TOS_ASSET.clone()).or_insert(0);
                *current = current
                    .checked_add(BURN_PER_CONTRACT)
                    .ok_or(VerificationError::Overflow)?;

                if let Some(invoke) = &payload.invoke {
                    for (asset, deposit) in &invoke.deposits {
                        let amount = deposit
                            .get_amount()
                            .map_err(|e| VerificationError::AnyError(anyhow!(e)))?;
                        let current = spending_per_asset.entry(asset.clone()).or_insert(0);
                        *current = current
                            .checked_add(amount)
                            .ok_or(VerificationError::Overflow)?;
                    }
                    // Add max_gas to TOS spending
                    let current = spending_per_asset.entry(TOS_ASSET.clone()).or_insert(0);
                    *current = current
                        .checked_add(invoke.max_gas)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::MultiSig(_) => {
                // No asset spending for these types
            }
            TransactionType::UnoTransfers(_) => {
                // UNO transfers spend from encrypted balances
                // Spending verification is done through ZKP proofs (CommitmentEqProof)
                // No plaintext spending to verify here
            }
            TransactionType::ShieldTransfers(transfers) => {
                // Shield transfers spend from plaintext TOS balance
                // Amount is public and verifiable
                for transfer in transfers {
                    let asset = transfer.get_asset();
                    let amount = transfer.get_amount();
                    let current = spending_per_asset.entry(asset.clone()).or_insert(0);
                    *current = current
                        .checked_add(amount)
                        .ok_or(VerificationError::Overflow)?;
                }
            }
            TransactionType::UnshieldTransfers(_) => {
                // Unshield transfers spend from encrypted UNO balances
                // Spending verification is done through ZKP proofs
                // No plaintext spending to verify here (adds to plaintext balance)
            }
            TransactionType::RegisterName(_) => {
                // TNS transactions: registration/message fees are paid via the fee field
                // No additional spending verification needed here
            }
        };

        // Add fee to TOS spending
        if self.fee > 0 {
            let current = spending_per_asset.entry(TOS_ASSET.clone()).or_insert(0);
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
                .get_sender_balance(
                    Cow::Borrowed(&self.source),
                    Cow::Owned(asset.clone()),
                    &self.reference,
                )
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

        Box::pin(self.apply(tx_hash, state)).await
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
        B: BlockchainApplyState<'a, P, E> + Send,
    >(
        self: &'a Arc<Self>,
        tx_hash: &'a Hash,
        state: &mut B,
    ) -> Result<(), VerificationError<E>> {
        if log::log_enabled!(log::Level::Trace) {
            trace!("apply with partial verify");
        }

        // Balance simplification: No decompression needed for plaintext balances
        // Private deposits are not supported, only Public deposits with plain u64 amounts
        if log::log_enabled!(log::Level::Trace) {
            trace!("Partial verify with plaintext balances - no proof verification needed");
        }

        // Delegate to apply_without_verify which handles balance deduction
        // (BLOCKDAG alignment: both functions now perform inline balance deduction)
        self.apply_without_verify(tx_hash, state).await
    }
}
