//! VRF Prediction Market Contract for TAKO
//!
//! A simplified prediction market inspired by Polymarket.
//! Uses VRF for fair and verifiable market resolution.
//!
//! Features:
//! - Binary outcome markets (YES/NO)
//! - VRF-based resolution with configurable probability
//! - Bet tracking and winner determination
//!
//! Commands (via input data):
//! - 0x01: Create market (probability threshold in next byte)
//! - 0x02: Place YES bet (amount in next 8 bytes)
//! - 0x03: Place NO bet (amount in next 8 bytes)
//! - 0x04: Resolve market (uses VRF)
//! - 0x00: Query status

#![no_std]
#![no_main]

use tako_sdk::{get_block_hash, get_input_data, log, storage_read, storage_write, vrf_random, SUCCESS};

// Storage keys
const KEY_STATUS: &[u8] = b"status";
const KEY_THRESHOLD: &[u8] = b"threshold";
const KEY_YES_POOL: &[u8] = b"yes_pool";
const KEY_NO_POOL: &[u8] = b"no_pool";
const KEY_OUTCOME: &[u8] = b"outcome";
const KEY_VRF_RANDOM: &[u8] = b"vrf_random";
const KEY_VRF_BYTE: &[u8] = b"vrf_byte";
const KEY_RESOLVED_BLOCK: &[u8] = b"resolved_block";

// Market status
const STATUS_NONE: u8 = 0;
const STATUS_OPEN: u8 = 1;
const STATUS_RESOLVED: u8 = 2;

// Outcomes
const OUTCOME_YES: u8 = 1;
const OUTCOME_NO: u8 = 0;

// Commands
const CMD_QUERY: u8 = 0x00;
const CMD_CREATE: u8 = 0x01;
const CMD_BET_YES: u8 = 0x02;
const CMD_BET_NO: u8 = 0x03;
const CMD_RESOLVE: u8 = 0x04;

/// Read u64 from storage
fn read_u64(key: &[u8]) -> u64 {
    let mut buf = [0u8; 8];
    let len = storage_read(key, &mut buf);
    if len >= 8 {
        u64::from_le_bytes(buf)
    } else {
        0
    }
}

/// Write u64 to storage
fn write_u64(key: &[u8], value: u64) {
    let _ = storage_write(key, &value.to_le_bytes());
}

/// Read u8 from storage
fn read_u8(key: &[u8]) -> u8 {
    let mut buf = [0u8; 1];
    let len = storage_read(key, &mut buf);
    if len >= 1 {
        buf[0]
    } else {
        0
    }
}

/// Main contract entrypoint
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    // Read input data into buffer
    let mut input_buf = [0u8; 256];
    let input_len = get_input_data(&mut input_buf);
    let raw_input = &input_buf[..input_len as usize];

    // Skip 2-byte entry_id prefix from call_contract
    let input = if raw_input.len() >= 2 {
        &raw_input[2..]
    } else {
        raw_input
    };

    // Default command is query
    let cmd = if input.is_empty() { CMD_QUERY } else { input[0] };

    match cmd {
        CMD_QUERY => {
            // Just return current status
            log("Query: returning status");
            SUCCESS
        }
        CMD_CREATE => {
            // Create new market with probability threshold
            let status = read_u8(KEY_STATUS);
            if status != STATUS_NONE {
                log("Market already exists");
                return 1;
            }

            // Threshold: 0-255, where 128 = 50% probability for YES
            let threshold = if input.len() > 1 { input[1] } else { 128 };

            storage_write(KEY_STATUS, &[STATUS_OPEN]);
            storage_write(KEY_THRESHOLD, &[threshold]);
            write_u64(KEY_YES_POOL, 0);
            write_u64(KEY_NO_POOL, 0);

            log("Market created");
            SUCCESS
        }
        CMD_BET_YES => {
            // Place YES bet
            let status = read_u8(KEY_STATUS);
            if status != STATUS_OPEN {
                log("Market not open");
                return 3;
            }

            // Parse bet amount (simplified: use input bytes 1-8)
            let amount = if input.len() >= 9 {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(&input[1..9]);
                u64::from_le_bytes(buf)
            } else {
                1 // Default bet amount
            };

            let current = read_u64(KEY_YES_POOL);
            write_u64(KEY_YES_POOL, current.saturating_add(amount));

            log("YES bet placed");
            SUCCESS
        }
        CMD_BET_NO => {
            // Place NO bet
            let status = read_u8(KEY_STATUS);
            if status != STATUS_OPEN {
                log("Market not open");
                return 3;
            }

            let amount = if input.len() >= 9 {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(&input[1..9]);
                u64::from_le_bytes(buf)
            } else {
                1
            };

            let current = read_u64(KEY_NO_POOL);
            write_u64(KEY_NO_POOL, current.saturating_add(amount));

            log("NO bet placed");
            SUCCESS
        }
        CMD_RESOLVE => {
            // Resolve market using VRF
            let status = read_u8(KEY_STATUS);
            if status != STATUS_OPEN {
                log("Market not open for resolution");
                return 5;
            }

            // Get VRF random value
            let block_hash = get_block_hash();
            let output = match vrf_random(block_hash.as_slice()) {
                Ok(output) => output,
                Err(_) => {
                    log("vrf_random failed");
                    return 6;
                }
            };

            // Store VRF data
            storage_write(KEY_VRF_RANDOM, &output.random);

            // Get threshold and compare with VRF byte
            let threshold = read_u8(KEY_THRESHOLD);
            let vrf_byte = output.random[0];

            // Store the VRF byte used for decision
            storage_write(KEY_VRF_BYTE, &[vrf_byte]);

            // Determine outcome: YES if vrf_byte < threshold
            let outcome = if vrf_byte < threshold {
                OUTCOME_YES
            } else {
                OUTCOME_NO
            };

            // Update market status
            storage_write(KEY_STATUS, &[STATUS_RESOLVED]);
            storage_write(KEY_OUTCOME, &[outcome]);
            storage_write(KEY_RESOLVED_BLOCK, &block_hash);

            if outcome == OUTCOME_YES {
                log("Market resolved: YES wins");
            } else {
                log("Market resolved: NO wins");
            }

            SUCCESS
        }
        _ => {
            log("Unknown command");
            10
        }
    }
}

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
