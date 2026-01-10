//! Minimal transient storage test

#![no_std]
#![no_main]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

// Transient storage syscalls
extern "C" {
    fn tstore(key_ptr: *const u8, key_len: u64, value_ptr: *const u8, value_len: u64, _arg5: u64) -> u64;
    fn tload(key_ptr: *const u8, key_len: u64, out_ptr: *mut u8, out_cap: u64, ret_len_ptr: *mut u64) -> u64;
}

/// Minimal test: store and load one value
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    let key: [u8; 32] = [1u8; 32];
    let value: [u8; 32] = [0x42u8; 32];
    let mut result: [u8; 32] = [0u8; 32];
    let mut ret_len: u64 = 0;

    // Store
    let store_result = unsafe {
        tstore(key.as_ptr(), 32, value.as_ptr(), 32, 0)
    };
    if store_result != 0 {
        return 1;
    }

    // Load
    let load_result = unsafe {
        tload(key.as_ptr(), 32, result.as_mut_ptr(), 32, &mut ret_len as *mut u64)
    };
    if load_result != 0 {
        return 2;
    }

    // Verify
    if result != value {
        return 3;
    }

    0 // Success
}
