//! VRF Lottery Contract for TAKO
//!
//! A fair lottery system using VRF for winner selection.
//! Supports 4 candidates (0-3) with equal probability.
//!
//! Winner selection: winner_index = random[0] % 4
//!
//! This demonstrates:
//! - VRF-based fair random selection
//! - Deterministic lottery with verifiable randomness
//! - Statistical fairness (each candidate has 25% chance)

#![no_std]
#![no_main]

use tako_sdk::{get_block_hash, log, storage_write, vrf_random, SUCCESS};

const KEY_WINNER: &[u8] = b"winner";
const KEY_WINNER_INDEX: &[u8] = b"winner_index";
const KEY_VRF_RANDOM: &[u8] = b"vrf_random";
const KEY_CANDIDATES: &[u8] = b"candidates";
const KEY_TOTAL_ROUNDS: &[u8] = b"total_rounds";

/// Number of candidates in the lottery
const NUM_CANDIDATES: u8 = 4;

/// Candidate names (stored as single bytes for simplicity)
const CANDIDATE_NAMES: [&[u8]; 4] = [b"Alice", b"Bob", b"Carol", b"Dave"];

/// Main contract entrypoint
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    // Get block hash as seed for VRF
    let block_hash = get_block_hash();
    let seed = block_hash.as_slice();

    // Get VRF random value
    let output = match vrf_random(seed) {
        Ok(output) => output,
        Err(_) => {
            log("vrf_random failed");
            return 1;
        }
    };

    // Store full random value for verification
    if storage_write(KEY_VRF_RANDOM, &output.random).is_err() {
        log("storage_write vrf_random failed");
        return 2;
    }

    // Calculate winner index using modulo
    // This gives each candidate equal probability (25% each)
    let winner_index = output.random[0] % NUM_CANDIDATES;

    // Store winner index
    if storage_write(KEY_WINNER_INDEX, &[winner_index]).is_err() {
        log("storage_write winner_index failed");
        return 3;
    }

    // Store winner name
    let winner_name = CANDIDATE_NAMES[winner_index as usize];
    if storage_write(KEY_WINNER, winner_name).is_err() {
        log("storage_write winner failed");
        return 4;
    }

    // Store number of candidates
    if storage_write(KEY_CANDIDATES, &[NUM_CANDIDATES]).is_err() {
        log("storage_write candidates failed");
        return 5;
    }

    // Log the winner
    log("Lottery winner selected");

    SUCCESS
}

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
