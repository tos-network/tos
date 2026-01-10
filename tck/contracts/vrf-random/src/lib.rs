//! VRF Randomness Example Contract for TAKO
//!
//! Calls VRF syscalls, verifies proof, and stores outputs in contract storage.

#![no_std]
#![no_main]

use tako_sdk::{
    get_block_hash, log, storage_write, vrf_public_key, vrf_random, vrf_verify, SUCCESS,
};

const KEY_RANDOM: &[u8] = b"vrf_random";
const KEY_PRE_OUTPUT: &[u8] = b"vrf_pre_output";
const KEY_PROOF: &[u8] = b"vrf_proof";
const KEY_PUBLIC_KEY: &[u8] = b"vrf_public_key";
const KEY_BLOCK_HASH: &[u8] = b"vrf_block_hash";
const KEY_VERIFIED: &[u8] = b"vrf_verified";

/// Main contract entrypoint
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    let block_hash = get_block_hash();
    let seed = block_hash.as_slice();

    let output = match vrf_random(seed) {
        Ok(output) => output,
        Err(_) => {
            log("vrf_random failed");
            return 1;
        }
    };

    let public_key = match vrf_public_key() {
        Ok(pk) => pk,
        Err(_) => {
            log("vrf_public_key failed");
            return 2;
        }
    };

    if vrf_verify(&public_key, &block_hash, &output.pre_output, &output.proof).is_err() {
        log("vrf_verify failed");
        return 3;
    }

    if storage_write(KEY_RANDOM, &output.random).is_err()
        || storage_write(KEY_PRE_OUTPUT, &output.pre_output).is_err()
        || storage_write(KEY_PROOF, &output.proof).is_err()
        || storage_write(KEY_PUBLIC_KEY, &public_key).is_err()
        || storage_write(KEY_BLOCK_HASH, &block_hash).is_err()
        || storage_write(KEY_VERIFIED, &[1u8]).is_err()
    {
        log("storage_write failed");
        return 4;
    }

    log("vrf_random ok");
    log("vrf_verify ok");

    SUCCESS
}

/// Panic handler (required for no_std)
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
