//! Implementation Contract
//!
//! This is the implementation side of the proxy pattern.
//! It contains the business logic but NO state storage.
//! All state operations affect the proxy's storage due to DELEGATECALL semantics.

#![no_std]
#![no_main]

use core::panic::PanicInfo;

const COUNTER_KEY: &[u8] = b"counter";

extern "C" {
    fn log(msg_ptr: *const u8, msg_len: u64);
    fn storage_read(
        key_ptr: *const u8,
        key_len: u64,
        output_ptr: *mut u8,
        output_len: u64,
    ) -> u64;
    fn storage_write(
        key_ptr: *const u8,
        key_len: u64,
        value_ptr: *const u8,
        value_len: u64,
    ) -> u64;
    fn get_caller(output_ptr: *mut u8) -> u64;
    fn set_return_data(data_ptr: *const u8, data_len: u64) -> u64;
}

fn log(msg: &str) {
    unsafe {
        log(msg.as_ptr(), msg.len() as u64);
    }
}

fn read_counter() -> u64 {
    let mut buffer = [0u8; 8];
    let len = unsafe {
        storage_read(
            COUNTER_KEY.as_ptr(),
            COUNTER_KEY.len() as u64,
            buffer.as_mut_ptr(),
            8,
        )
    };

    if len == 8 {
        u64::from_le_bytes(buffer)
    } else {
        0
    }
}

fn write_counter(value: u64) -> bool {
    let bytes = value.to_le_bytes();
    let result = unsafe {
        storage_write(
            COUNTER_KEY.as_ptr(),
            COUNTER_KEY.len() as u64,
            bytes.as_ptr(),
            8,
        )
    };
    result == 0
}

/// Implementation contract entrypoint
///
/// IMPORTANT: When called via DELEGATECALL from proxy:
/// - Storage operations affect the PROXY's storage
/// - msg.sender is the ORIGINAL caller (not the proxy)
/// - This contract has NO persistent state of its own
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    log("=== Implementation Contract (via DELEGATECALL) ===");
    log("Storage operations will affect the PROXY's storage");

    // Read current counter (from proxy's storage!)
    let current = read_counter();
    log("Current counter (from proxy storage):");

    // Increment counter
    let new_value = current.saturating_add(1);

    // Write back (to proxy's storage!)
    if write_counter(new_value) {
        log("Counter incremented successfully");
        log("IMPORTANT: This modified the PROXY's storage, not ours!");

        // Return new value
        let bytes = new_value.to_le_bytes();
        unsafe {
            set_return_data(bytes.as_ptr(), 8);
        }

        // Show who the caller is
        let mut caller = [0u8; 32];
        unsafe {
            get_caller(caller.as_mut_ptr());
        }
        log("Caller is the ORIGINAL user, not the proxy!");

        0
    } else {
        log("ERROR: Failed to increment counter");
        1
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
