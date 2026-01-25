//! Scheduler Contract for TAKO
//!
//! Demonstrates the `offer_call` syscall for scheduling future contract executions.
//! This contract schedules itself to be called at a future topoheight.

#![no_std]
#![no_main]

use tako_sdk::{
    get_contract_hash, get_input_data, log, offer_call, offer_call_block_end, storage_read,
    storage_write, SUCCESS,
};

const KEY_HANDLE: &[u8] = b"scheduled_handle";
const KEY_TARGET_TOPO: &[u8] = b"target_topoheight";
const KEY_EXECUTION_COUNT: &[u8] = b"execution_count";
const KEY_LAST_ERROR: &[u8] = b"last_error";

/// Main contract entrypoint
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    // Read input data into buffer
    let mut input_buffer = [0u8; 64];
    let input_len = get_input_data(&mut input_buffer) as usize;

    // Parse entry point from input (first 2 bytes)
    if input_len < 2 {
        log("input too short");
        return 1;
    }

    let entry_id = u16::from_le_bytes([input_buffer[0], input_buffer[1]]);
    let params = &input_buffer[2..input_len];

    match entry_id {
        0 => schedule_future(params),
        1 => schedule_block_end(params),
        2 => on_scheduled_execution(),
        _ => {
            log("unknown entry point");
            2
        }
    }
}

/// Schedule execution at a future topoheight
fn schedule_future(params: &[u8]) -> u64 {
    // Parse target topoheight from params
    if params.len() < 8 {
        log("need 8 bytes for target_topoheight");
        let _ = storage_write(KEY_LAST_ERROR, b"need 8 bytes");
        return 3;
    }

    let target_topo = u64::from_le_bytes([
        params[0], params[1], params[2], params[3], params[4], params[5], params[6], params[7],
    ]);

    // Get this contract's address (we schedule ourselves)
    let contract_address = get_contract_hash();

    // Schedule execution at target topoheight
    // Entry ID 2 = on_scheduled_execution
    let result = offer_call(
        &contract_address,
        2,           // entry_id for on_scheduled_execution
        &[],         // no input data
        50_000,      // max_gas
        0,           // offer_amount (no priority fee for test)
        target_topo,
    );

    match result {
        Ok(handle) => {
            log("offer_call succeeded");

            // Store handle and target for verification
            if storage_write(KEY_HANDLE, &handle.to_le_bytes()).is_err() {
                log("failed to store handle");
                return 4;
            }
            if storage_write(KEY_TARGET_TOPO, &target_topo.to_le_bytes()).is_err() {
                log("failed to store target_topo");
                return 5;
            }

            SUCCESS
        }
        Err(code) => {
            log("offer_call failed");
            let _ = storage_write(KEY_LAST_ERROR, &code.to_le_bytes());
            code
        }
    }
}

/// Schedule execution at block end
fn schedule_block_end(_params: &[u8]) -> u64 {
    // Get this contract's address
    let contract_address = get_contract_hash();

    // Schedule execution at block end
    let result = offer_call_block_end(
        &contract_address,
        2,      // entry_id for on_scheduled_execution
        &[],    // no input data
        50_000, // max_gas
        0,      // offer_amount
    );

    match result {
        Ok(handle) => {
            log("offer_call_block_end succeeded");

            // Store handle for verification
            if storage_write(KEY_HANDLE, &handle.to_le_bytes()).is_err() {
                log("failed to store handle");
                return 4;
            }

            SUCCESS
        }
        Err(code) => {
            log("offer_call_block_end failed");
            let _ = storage_write(KEY_LAST_ERROR, &code.to_le_bytes());
            code
        }
    }
}

/// Called when scheduled execution triggers
fn on_scheduled_execution() -> u64 {
    log("scheduled execution triggered!");

    // Read current execution count
    let mut buffer = [0u8; 8];
    let bytes_read = storage_read(KEY_EXECUTION_COUNT, &mut buffer);

    let count = if bytes_read > 0 {
        u64::from_le_bytes(buffer)
    } else {
        0
    };

    let new_count = count.saturating_add(1);

    if storage_write(KEY_EXECUTION_COUNT, &new_count.to_le_bytes()).is_err() {
        log("failed to update execution count");
        return 1;
    }

    log("execution count updated");
    SUCCESS
}

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
