//! VRF Branching Contract for TAKO
//!
//! Demonstrates VRF-based execution path selection.
//! The contract reads VRF random value and executes different paths
//! based on the first byte of the random value.
//!
//! Path selection:
//! - If random[0] < 128: Execute path_a (store "path_a")
//! - If random[0] >= 128: Execute path_b (store "path_b")

#![no_std]
#![no_main]

use tako_sdk::{get_block_hash, log, storage_write, vrf_random, SUCCESS};

const KEY_RESULT: &[u8] = b"result";
const KEY_PATH: &[u8] = b"path";
const KEY_RANDOM_BYTE: &[u8] = b"random_byte";
const KEY_VRF_RANDOM: &[u8] = b"vrf_random";

/// Path A result value
const PATH_A: &[u8] = b"path_a";
/// Path B result value
const PATH_B: &[u8] = b"path_b";
/// Threshold for path selection (128 = 50% probability each path)
const THRESHOLD: u8 = 128;

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

    // Get the first byte for branching decision
    let random_byte = output.random[0];

    // Store the random byte used for decision
    if storage_write(KEY_RANDOM_BYTE, &[random_byte]).is_err() {
        log("storage_write random_byte failed");
        return 3;
    }

    // Branch based on random value
    let (path, result) = if random_byte < THRESHOLD {
        // Path A: random[0] < 128
        log("Executing path_a");
        (b"a".as_slice(), PATH_A)
    } else {
        // Path B: random[0] >= 128
        log("Executing path_b");
        (b"b".as_slice(), PATH_B)
    };

    // Store which path was taken
    if storage_write(KEY_PATH, path).is_err() {
        log("storage_write path failed");
        return 4;
    }

    // Store the result
    if storage_write(KEY_RESULT, result).is_err() {
        log("storage_write result failed");
        return 5;
    }

    log("vrf_branching completed");
    SUCCESS
}

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
