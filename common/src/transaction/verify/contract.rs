use std::{borrow::Cow, sync::Arc};

use indexmap::IndexMap;
use log::{debug, trace};
use tos_kernel::ValueCell;

use crate::{
    config::{TOS_ASSET, TX_GAS_BURN_PERCENT},
    contract::ContractProvider,
    crypto::Hash,
    nft::{NftCacheStorage, NftStorageProvider},
    transaction::{ContractDeposit, Transaction},
};

use super::{BlockchainApplyState, BlockchainVerificationState, VerificationError};

const MAX_EVENTS_PER_TX: usize = 1000;

#[derive(Debug)]
pub enum InvokeContract {
    Entry(u16),
    Hook(u8),
}

impl Transaction {
    // Load and check if a contract is available
    // This is needed in case a contract has been removed or wasn't deployed due to the constructor error
    pub(super) async fn is_contract_available<'a, E, B: BlockchainVerificationState<'a, E>>(
        &'a self,
        state: &mut B,
        contract: &'a Hash,
    ) -> Result<bool, VerificationError<E>> {
        state
            .load_contract_module(contract)
            .await
            .map_err(VerificationError::State)
    }

    /// Invoke a contract from a transaction using the ContractExecutor trait.
    ///
    /// # Security Invariants
    ///
    /// ## Staged Changeset Pattern (Atomic Rollback)
    ///
    /// All contract state changes are staged in a `ContractCache` during execution:
    /// - Storage writes go to `cache.storage`
    /// - Balance changes go to `cache.balances`
    /// - Events go through separate `add_contract_events()` call
    ///
    /// **Critical invariant**: Only when `is_success == true` (exit_code == Some(0))
    /// does the cache get merged via `merge_contract_changes()`. On failure, the
    /// cache is simply dropped, providing automatic atomic rollback.
    ///
    /// ## Gas Consumption on Failure
    ///
    /// There are two failure modes with different gas semantics:
    ///
    /// **Executor Error (Err)**: VM crash, invalid bytecode, etc.
    /// - Forced `gas_used = max_gas` to prevent "free invoke" attacks
    /// - 30% burned, 70% to miners, 0% refunded
    ///
    /// **Non-zero Exit Code**: Contract logic failure (revert, assert, etc.)
    /// - `gas_used` = actual VM consumption before failure
    /// - Remaining gas (`max_gas - gas_used`) is refunded to caller
    /// - This is standard EVM/SVM semantics for reverts
    ///
    /// ## Deposit Handling
    ///
    /// Deposits are held in `chain_state.cache.balances` during execution.
    /// On success: merged to persistent storage.
    /// On failure: `refund_deposits()` returns all deposits to sender.
    ///
    /// # Arguments
    ///
    /// * `tx_hash` - Transaction hash for tracking
    /// * `state` - Blockchain state accessor
    /// * `contract` - Contract address to invoke
    /// * `deposits` - Asset deposits made with this call
    /// * `user_parameters` - Call data from transaction
    /// * `max_gas` - Maximum gas units for execution
    /// * `invoke` - Entry point or hook to invoke
    ///
    /// # Returns
    ///
    /// * `Ok(true)` - Contract executed successfully (exit_code == 0)
    /// * `Ok(false)` - Contract execution failed (exit_code != 0 or execution error)
    /// * `Err(_)` - System error (state access failure)
    pub(super) async fn invoke_contract<
        'a,
        P: ContractProvider + NftStorageProvider + Send,
        E,
        B: BlockchainApplyState<'a, P, E>,
    >(
        self: &'a Arc<Self>,
        tx_hash: &'a Hash,
        state: &mut B,
        contract: &Hash,
        deposits: &'a IndexMap<Hash, ContractDeposit>,
        user_parameters: impl DoubleEndedIterator<Item = ValueCell>,
        max_gas: u64,
        invoke: InvokeContract,
    ) -> Result<bool, VerificationError<E>> {
        if log::log_enabled!(log::Level::Debug) {
            debug!("Invoking contract {contract} from TX {tx_hash}: {invoke:?}");
        }

        // Collect user parameters (bytes) from the transaction
        // For TAKO contracts, the first parameter should contain the call data
        let user_data: Vec<u8> = user_parameters
            .filter_map(|cell| {
                if let ValueCell::Bytes(bytes) = cell {
                    Some(bytes)
                } else {
                    None
                }
            })
            .flatten()
            .collect();

        if log::log_enabled!(log::Level::Trace) {
            trace!(
                "User data for contract invocation: {} bytes",
                user_data.len()
            );
        }

        // Get the contract module to extract bytecode
        // Extract bytecode into owned Vec to avoid borrowing conflicts
        let bytecode: Vec<u8> = {
            let (module, _environment) = state
                .get_contract_module_with_environment(contract)
                .await
                .map_err(VerificationError::State)?;

            // Extract bytecode from module
            // For TOS Kernel(TAKO) contracts, this will be ELF bytecode
            // For legacy contracts, this will be None (not supported in new executor)
            let bytecode = module.get_bytecode().ok_or_else(|| {
                VerificationError::ModuleError(
                    "Contract does not have eBPF bytecode. Legacy contracts are no longer supported. Please redeploy with TOS Kernel(TAKO) format.".to_string()
                )
            })?;

            bytecode.to_vec()
        };

        // Get the executor before any mutable borrows to avoid borrow conflicts
        let executor = state.get_contract_executor();

        // Get the contract environment for state access
        let (contract_environment, mut chain_state) = state
            .get_contract_environment_for(contract, deposits, tx_hash)
            .await
            .map_err(VerificationError::State)?;

        // Get block information from chain state
        let topoheight = chain_state.topoheight;
        let block_hash = chain_state.block_hash;
        let block = chain_state.block;
        // Convert timestamp from milliseconds to seconds for contract execution
        let block_timestamp = block.get_header().get_timestamp() / 1000;

        // Build input data for the contract
        // The user_data contains [instruction_byte] + [args] for TAKO contracts
        // The entry_id/hook_id is used for TOS-level routing but the contract
        // handles its own dispatch via the instruction byte in user_data
        let parameters = match invoke {
            InvokeContract::Entry(_entry_id) => {
                // Pass user_data directly - it already contains instruction + args
                Some(user_data)
            }
            InvokeContract::Hook(_hook_id) => {
                // Pass user_data directly for hooks as well
                Some(user_data)
            }
        };

        // Execute the contract
        // Convert execution errors to is_success=false instead of returning Err.
        // This prevents state corruption when contract execution fails - balance was already
        // deducted in apply_without_verify, so returning Err would leave state inconsistent.
        // Instead, treat execution errors as failed executions with max_gas consumed.
        let execution_result = {
            let provider = contract_environment.provider;
            let mut nft_storage =
                NftCacheStorage::new(provider, topoheight, &mut chain_state.nft_cache);
            let tx_sender_hash = Hash::new(*self.get_source().as_bytes());
            match executor
                .execute(
                    &bytecode,
                    provider,
                    topoheight,
                    contract,
                    block_hash,
                    topoheight, // Use topoheight as block_height for now
                    block_timestamp,
                    tx_hash,
                    &tx_sender_hash,
                    max_gas,
                    parameters,
                    Some(&mut nft_storage),
                )
                .await
            {
                Ok(result) => result,
                Err(e) => {
                    // Log the error but don't propagate it as Err
                    // This ensures deposits are refunded and state remains consistent
                    if log::log_enabled!(log::Level::Error) {
                        log::error!(
                            "Contract {} execution error (treating as failure): {e:#}",
                            contract
                        );
                    }
                    // Truncate error message to prevent chain bloat (DoS protection)
                    const MAX_ERROR_RETURN_DATA: usize = 4096;
                    let mut error_msg = format!("Execution error: {e}").into_bytes();
                    if error_msg.len() > MAX_ERROR_RETURN_DATA {
                        error_msg.truncate(MAX_ERROR_RETURN_DATA);
                    }
                    // Return a failure result with max_gas consumed
                    crate::contract::ContractExecutionResult {
                        exit_code: None,
                        gas_used: max_gas,
                        return_data: Some(error_msg),
                        transfers: vec![],
                        events: vec![],
                        cache: None, // No cache to merge on error
                    }
                }
            }
        };

        let used_gas = execution_result.gas_used;
        let exit_code = execution_result.exit_code;
        let return_data = execution_result.return_data;
        let transfers = execution_result.transfers;
        let events = execution_result.events;
        let vm_cache = execution_result.cache;

        if events.len() > MAX_EVENTS_PER_TX {
            return Err(VerificationError::TooManyContractEvents {
                count: events.len(),
                max: MAX_EVENTS_PER_TX,
            });
        }

        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "Contract {} execution result: gas_used={}, exit_code={:?}, return_data={}, transfers={}, events={}",
                contract,
                used_gas,
                exit_code,
                return_data.as_ref().map(|d| d.len()).unwrap_or(0),
                transfers.len(),
                events.len()
            );
        }

        let is_success = exit_code == Some(0);
        let mut outputs = chain_state.outputs;

        // Convert TAKO transfers to ContractOutput::Transfer
        // This ensures transfers staged during contract execution are persisted to the ledger
        if is_success {
            for transfer in transfers {
                outputs.push(crate::contract::ContractOutput::Transfer {
                    amount: transfer.amount,
                    asset: transfer.asset,
                    destination: transfer.destination,
                });
            }
        }

        // If the contract execution was successful, merge the changes
        if is_success {
            let mut cache = chain_state.cache;
            // Merge VM storage writes into chain_state cache
            // Only storage is merged - balances/events/memory are handled separately
            if let Some(vm_cache) = vm_cache {
                // Safety check: VM cache should only contain storage writes.
                // Balances are managed by chain_state.cache, events via add_contract_events(),
                // and memory is transient. If TAKO starts writing to these fields, we need
                // to update the merge strategy to avoid conflicts.
                debug_assert!(
                    vm_cache.balances.is_empty(),
                    "VM cache must not write balances"
                );
                debug_assert!(vm_cache.events.is_empty(), "VM cache must not write events");
                debug_assert!(vm_cache.memory.is_empty(), "VM cache must not write memory");
                cache.merge_overlay_storage_and_tokens(vm_cache);
            }
            let tracker = chain_state.tracker;
            let assets = chain_state.assets;
            let nft_cache = chain_state.nft_cache;
            state
                .merge_contract_changes(contract, cache, nft_cache, tracker, assets)
                .await
                .map_err(VerificationError::State)?;

            // Store events from contract execution (LOG0-LOG4 syscalls)
            // Events are only persisted if the contract execution was successful
            // This must be done after chain_state is consumed to avoid borrow conflicts
            if !events.is_empty() {
                state
                    .add_contract_events(events, contract, tx_hash)
                    .await
                    .map_err(VerificationError::State)?;
            }
        } else {
            // Otherwise, something went wrong, delete the outputs made by the contract
            outputs.clear();

            if !deposits.is_empty() {
                // It was not successful, we need to refund the deposits
                self.refund_deposits(state, deposits).await?;
                outputs.push(crate::contract::ContractOutput::RefundDeposits);
            }
        }

        // Store return data from contract execution (if any)
        // Return data is persisted regardless of success/failure since it may contain
        // error messages on failure or result data on success
        // This must be AFTER the success/failure branch to survive outputs.clear()
        if let Some(data) = return_data {
            if !data.is_empty() {
                outputs.push(crate::contract::ContractOutput::ReturnData { data });
            }
        }

        // Push the exit code to the outputs
        outputs.push(crate::contract::ContractOutput::ExitCode(exit_code));

        // Handle gas refunds
        let refund_gas = self.handle_gas(state, used_gas, max_gas).await?;
        if log::log_enabled!(log::Level::Debug) {
            debug!("used gas: {used_gas}, refund gas: {refund_gas}");
        }
        if refund_gas > 0 {
            outputs.push(crate::contract::ContractOutput::RefundGas { amount: refund_gas });
        }

        // Track the outputs
        state
            .set_contract_outputs(tx_hash, outputs)
            .await
            .map_err(VerificationError::State)?;

        Ok(is_success)
    }

    pub(super) async fn handle_gas<
        'a,
        P: ContractProvider,
        E,
        B: BlockchainApplyState<'a, P, E>,
    >(
        &'a self,
        state: &mut B,
        used_gas: u64,
        max_gas: u64,
    ) -> Result<u64, VerificationError<E>> {
        // Part of the gas is burned
        let burned_gas = used_gas * TX_GAS_BURN_PERCENT / 100;
        // Part of the gas is given to the miners as fees
        let gas_fee = used_gas
            .checked_sub(burned_gas)
            .ok_or(VerificationError::GasOverflow)?;
        // The remaining gas is refunded to the sender
        let refund_gas = max_gas
            .checked_sub(used_gas)
            .ok_or(VerificationError::GasOverflow)?;

        if log::log_enabled!(log::Level::Debug) {
            debug!(
                "Invoke contract used gas: {used_gas}, burned: {burned_gas}, fee: {gas_fee}, refund: {refund_gas}"
            );
        }
        state
            .add_burned_coins(burned_gas)
            .await
            .map_err(VerificationError::State)?;

        state
            .add_gas_fee(gas_fee)
            .await
            .map_err(VerificationError::State)?;

        if refund_gas > 0 {
            // If we have some funds to refund, we add it to the sender balance
            // But to prevent any front running, we add to the sender balance by considering him as a receiver.
            let balance = state
                .get_receiver_balance(Cow::Borrowed(self.get_source()), Cow::Owned(TOS_ASSET))
                .await
                .map_err(VerificationError::State)?;

            *balance = balance
                .checked_add(refund_gas)
                .ok_or(VerificationError::GasRefundOverflow)?;
        }

        Ok(refund_gas)
    }

    /// Refund the deposits made by the user to the contract on execution failure.
    ///
    /// # Overflow Safety
    ///
    /// The `checked_add` below is kept as defensive programming, but overflow is
    /// mathematically impossible in the current design:
    ///
    /// 1. Each deposit amount was deducted from sender's balance in `apply_without_verify`
    /// 2. The receiver IS the sender (refund goes back to original account)
    /// 3. Refunding cannot exceed the balance that existed before deduction
    ///
    /// Proof: Let B_before = sender's balance before TX, D_total = sum of deposits
    /// - After deduction: B_after = B_before - D_total
    /// - After refund: B_refund = B_after + D_total = B_before
    /// - Since B_before <= u64::MAX (valid balance), B_refund <= u64::MAX
    ///
    /// The checked_add is retained because:
    /// - It documents the overflow concern for future maintainers
    /// - It provides defense-in-depth if the invariants are ever broken
    /// - The runtime cost is negligible (single branch prediction)
    pub(super) async fn refund_deposits<
        'a,
        P: ContractProvider,
        E,
        B: BlockchainApplyState<'a, P, E>,
    >(
        &'a self,
        state: &mut B,
        deposits: &'a IndexMap<Hash, ContractDeposit>,
    ) -> Result<(), VerificationError<E>> {
        for (asset, deposit) in deposits.iter() {
            if log::log_enabled!(log::Level::Trace) {
                let source_address = self
                    .get_source()
                    .decompress()
                    .map_err(|_| VerificationError::InvalidFormat)?
                    .to_address(state.is_mainnet());
                trace!("Refunding deposit {deposit:?} for asset: {asset} to {source_address}");
            }

            let balance = state
                .get_receiver_balance(Cow::Borrowed(self.get_source()), Cow::Borrowed(asset))
                .await
                .map_err(VerificationError::State)?;

            // Balance simplification: Extract amount from deposit
            // Private deposits are not supported in plaintext balance system
            let amount = deposit
                .get_amount()
                .map_err(|e| VerificationError::ModuleError(e.to_string()))?;

            *balance = balance
                .checked_add(amount)
                .ok_or(VerificationError::Overflow)?;
        }

        Ok(())
    }
}
