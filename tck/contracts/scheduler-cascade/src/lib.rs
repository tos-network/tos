//! Cascade Scheduler Contract for TAKO
//!
//! Demonstrates cascade scheduling: Contract A reads VRF, schedules Contract B.
//! Used to test that scheduled executions can themselves schedule more executions.

#![no_std]
#![no_main]

use tako_sdk::{
    get_block_hash, get_input_data, log, offer_call, storage_read, storage_write, vrf_random,
    SUCCESS,
};

// Storage keys for Contract A (scheduler)
const KEY_SCHEDULED_HANDLE: &[u8] = b"cascade_handle";
const KEY_SCHEDULER_VRF: &[u8] = b"scheduler_vrf";
const KEY_TARGET_CONTRACT: &[u8] = b"target_contract";
const KEY_TARGET_TOPO: &[u8] = b"target_topo";

// Storage keys for Contract B (target)
const KEY_TARGET_VRF: &[u8] = b"target_vrf";
const KEY_TARGET_BLOCK_HASH: &[u8] = b"target_block_hash";
const KEY_EXECUTION_COUNT: &[u8] = b"cascade_exec_count";
const KEY_CALLER_VRF: &[u8] = b"caller_vrf";

/// Main contract entrypoint
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    let mut input_buffer = [0u8; 128];
    let input_len = get_input_data(&mut input_buffer) as usize;

    if input_len < 2 {
        log("input too short");
        return 1;
    }

    let entry_id = u16::from_le_bytes([input_buffer[0], input_buffer[1]]);
    let params = &input_buffer[2..input_len];

    match entry_id {
        0 => cascade_schedule(params),
        1 => on_cascade_executed(params),
        _ => {
            log("unknown entry point");
            2
        }
    }
}

/// Entry 0: Read VRF and schedule another contract
/// Input: target_contract (32 bytes) + target_topo (8 bytes)
fn cascade_schedule(params: &[u8]) -> u64 {
    if params.len() < 40 {
        log("need 40 bytes: target_contract(32) + target_topo(8)");
        return 3;
    }

    // Parse target contract address (32 bytes)
    let mut target_contract = [0u8; 32];
    target_contract.copy_from_slice(&params[0..32]);

    // Parse target topoheight (8 bytes LE)
    let target_topo = u64::from_le_bytes([
        params[32], params[33], params[34], params[35], params[36], params[37], params[38],
        params[39],
    ]);

    // Read VRF at current block
    let block_hash = get_block_hash();
    let vrf_output = match vrf_random(block_hash.as_slice()) {
        Ok(output) => output,
        Err(_) => {
            log("vrf_random failed");
            return 10;
        }
    };

    log("scheduler read vrf");

    // Store scheduler's VRF for verification
    if storage_write(KEY_SCHEDULER_VRF, &vrf_output.random).is_err() {
        log("failed to store scheduler vrf");
        return 4;
    }

    // Store target info
    if storage_write(KEY_TARGET_CONTRACT, &target_contract).is_err() {
        log("failed to store target contract");
        return 5;
    }
    if storage_write(KEY_TARGET_TOPO, &target_topo.to_le_bytes()).is_err() {
        log("failed to store target topo");
        return 6;
    }

    // Schedule target contract with VRF random as input data
    // Entry 1 = on_cascade_executed, input = scheduler's vrf_random (32 bytes)
    let result = offer_call(
        &target_contract,
        1,                   // entry_id for on_cascade_executed
        &vrf_output.random,  // pass scheduler's VRF as input
        100_000,             // max_gas
        0,                   // offer_amount
        target_topo,
    );

    match result {
        Ok(handle) => {
            log("cascade offer_call succeeded");
            if storage_write(KEY_SCHEDULED_HANDLE, &handle.to_le_bytes()).is_err() {
                log("failed to store handle");
                return 7;
            }
            SUCCESS
        }
        Err(code) => {
            log("cascade offer_call failed");
            code
        }
    }
}

/// Entry 1: Called when this contract is scheduled by another
/// Input: caller's VRF random (32 bytes)
fn on_cascade_executed(params: &[u8]) -> u64 {
    log("cascade execution triggered");

    // Store caller's VRF (passed as input)
    if params.len() >= 32 {
        if storage_write(KEY_CALLER_VRF, &params[0..32]).is_err() {
            log("failed to store caller vrf");
            return 1;
        }
    }

    // Read VRF at execution time
    let block_hash = get_block_hash();
    let vrf_output = match vrf_random(block_hash.as_slice()) {
        Ok(output) => output,
        Err(_) => {
            log("target vrf_random failed");
            return 10;
        }
    };

    log("target read vrf");

    // Store target's VRF and block hash
    if storage_write(KEY_TARGET_VRF, &vrf_output.random).is_err() {
        log("failed to store target vrf");
        return 2;
    }
    if storage_write(KEY_TARGET_BLOCK_HASH, &block_hash).is_err() {
        log("failed to store target block hash");
        return 3;
    }

    // Update execution count
    let mut buffer = [0u8; 8];
    let bytes_read = storage_read(KEY_EXECUTION_COUNT, &mut buffer);
    let count = if bytes_read > 0 {
        u64::from_le_bytes(buffer)
    } else {
        0
    };
    let new_count = count.saturating_add(1);
    if storage_write(KEY_EXECUTION_COUNT, &new_count.to_le_bytes()).is_err() {
        log("failed to update count");
        return 4;
    }

    log("cascade target completed");
    SUCCESS
}

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
