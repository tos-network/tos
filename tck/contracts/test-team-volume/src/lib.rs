//! Team Volume Syscalls Test Contract - Multi-Asset Volume Tracking
//!
//! Tests the 4 team volume syscalls with:
//! 1. EXACT value verification (add 1000 -> verify volume increased by EXACTLY 1000)
//! 2. MULTI-ASSET tracking (3 different assets with independent volumes)

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use tako_sdk::{
    add_team_volume, get_caller, get_direct_volume, get_team_volume, get_zone_volumes, log,
};

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

// Define 3 different assets for multi-asset testing
const TOS_ASSET: [u8; 32] = [0u8; 32]; // Native TOS asset
const ASSET_A: [u8; 32] = [1u8; 32]; // Test asset A (all 0x01)
const ASSET_B: [u8; 32] = [2u8; 32]; // Test asset B (all 0x02)

/// Test 1: add_team_volume and verify EXACT increase
#[no_mangle]
pub extern "C" fn test_exact_team_volume_increase() -> u64 {
    log("Test: EXACT team_volume increase");
    let caller = get_caller();

    // Get initial volume
    let initial = match get_team_volume(&caller, &TOS_ASSET) {
        Ok(v) => v,
        Err(code) => {
            log("FAIL: get_team_volume (initial)");
            return 100 + code;
        }
    };

    // Add exactly 1000
    if let Err(code) = add_team_volume(&caller, &TOS_ASSET, 1000, 3) {
        log("FAIL: add_team_volume");
        return 200 + code;
    }

    // Get new volume
    let after = match get_team_volume(&caller, &TOS_ASSET) {
        Ok(v) => v,
        Err(code) => {
            log("FAIL: get_team_volume (after)");
            return 300 + code;
        }
    };

    // EXACT verification: after == initial + 1000
    let expected = initial + 1000;
    if after != expected {
        log("FAIL: Volume mismatch!");
        // Return encoded error: 1000 + difference
        // If after > expected, return 1000 + (after - expected)
        // If after < expected, return 2000 + (expected - after)
        if after > expected {
            return 1000 + (after - expected);
        } else {
            return 2000 + (expected - after);
        }
    }

    log("PASS: team_volume increased by EXACTLY 1000");
    0
}

/// Test 2: verify EXACT direct_volume increase
#[no_mangle]
pub extern "C" fn test_exact_direct_volume_increase() -> u64 {
    log("Test: EXACT direct_volume increase");
    let caller = get_caller();

    // Get initial direct volume
    let initial = match get_direct_volume(&caller, &TOS_ASSET) {
        Ok(v) => v,
        Err(code) => {
            log("FAIL: get_direct_volume (initial)");
            return 100 + code;
        }
    };

    // Add exactly 500 with levels=1 (should affect direct_volume)
    if let Err(code) = add_team_volume(&caller, &TOS_ASSET, 500, 1) {
        log("FAIL: add_team_volume");
        return 200 + code;
    }

    // Get new direct volume
    let after = match get_direct_volume(&caller, &TOS_ASSET) {
        Ok(v) => v,
        Err(code) => {
            log("FAIL: get_direct_volume (after)");
            return 300 + code;
        }
    };

    // EXACT verification
    let expected = initial + 500;
    if after != expected {
        log("FAIL: Direct volume mismatch!");
        if after > expected {
            return 1000 + (after - expected);
        } else {
            return 2000 + (expected - after);
        }
    }

    log("PASS: direct_volume increased by EXACTLY 500");
    0
}

/// Test 3: verify EXACT accumulation (add 300 three times = 900)
#[no_mangle]
pub extern "C" fn test_exact_accumulation() -> u64 {
    log("Test: EXACT accumulation 300x3=900");
    let caller = get_caller();

    // Get initial
    let initial = match get_team_volume(&caller, &TOS_ASSET) {
        Ok(v) => v,
        Err(code) => {
            log("FAIL: get_team_volume (initial)");
            return 100 + code;
        }
    };

    // Add 300 three times
    for i in 0..3u64 {
        if let Err(code) = add_team_volume(&caller, &TOS_ASSET, 300, 5) {
            log("FAIL: add_team_volume in loop");
            return 200 + code + i * 10;
        }
    }

    // Get final
    let after = match get_team_volume(&caller, &TOS_ASSET) {
        Ok(v) => v,
        Err(code) => {
            log("FAIL: get_team_volume (final)");
            return 300 + code;
        }
    };

    // EXACT verification: after == initial + 900
    let expected = initial + 900;
    if after != expected {
        log("FAIL: Accumulation mismatch!");
        if after > expected {
            return 1000 + (after - expected);
        } else {
            return 2000 + (expected - after);
        }
    }

    log("PASS: Accumulated EXACTLY 900 (300x3)");
    0
}

/// Test 4: get_zone_volumes functionality
#[no_mangle]
pub extern "C" fn test_get_zone_volumes() -> u64 {
    log("Test: get_zone_volumes");
    let caller = get_caller();

    match get_zone_volumes(&caller, &TOS_ASSET, 10) {
        Ok(_) => {
            log("PASS: get_zone_volumes succeeded");
            0
        }
        Err(code) => {
            log("FAIL: get_zone_volumes");
            400 + code
        }
    }
}

/// Test 5: Comprehensive - add specific amount and verify both team and direct
#[no_mangle]
pub extern "C" fn test_comprehensive_exact() -> u64 {
    log("Test: Comprehensive EXACT verification");
    let caller = get_caller();

    // Get initial values
    let init_team = match get_team_volume(&caller, &TOS_ASSET) {
        Ok(v) => v,
        Err(code) => return 100 + code,
    };
    let init_direct = match get_direct_volume(&caller, &TOS_ASSET) {
        Ok(v) => v,
        Err(code) => return 110 + code,
    };

    // Add exactly 7777 with levels=1
    let add_amount: u64 = 7777;
    if let Err(code) = add_team_volume(&caller, &TOS_ASSET, add_amount, 1) {
        log("FAIL: add_team_volume");
        return 200 + code;
    }

    // Verify team_volume
    let after_team = match get_team_volume(&caller, &TOS_ASSET) {
        Ok(v) => v,
        Err(code) => return 300 + code,
    };

    if after_team != init_team + add_amount {
        log("FAIL: team_volume != initial + 7777");
        return 400;
    }

    // Verify direct_volume
    let after_direct = match get_direct_volume(&caller, &TOS_ASSET) {
        Ok(v) => v,
        Err(code) => return 310 + code,
    };

    if after_direct != init_direct + add_amount {
        log("FAIL: direct_volume != initial + 7777");
        return 401;
    }

    log("PASS: Both team_volume and direct_volume increased by EXACTLY 7777");
    0
}

// ============================================================================
// MULTI-ASSET VOLUME TRACKING TESTS
// ============================================================================

/// Test 6: Multi-asset volume tracking - verify assets are tracked independently
#[no_mangle]
pub extern "C" fn test_multi_asset_independent() -> u64 {
    log("Test: Multi-asset independent tracking");
    let caller = get_caller();

    // Get initial volumes for all 3 assets
    let init_tos = get_team_volume(&caller, &TOS_ASSET).unwrap_or(0);
    let init_a = get_team_volume(&caller, &ASSET_A).unwrap_or(0);
    let init_b = get_team_volume(&caller, &ASSET_B).unwrap_or(0);

    // Add different amounts to each asset
    // TOS: +1000, ASSET_A: +2000, ASSET_B: +3000
    if let Err(code) = add_team_volume(&caller, &TOS_ASSET, 1000, 1) {
        log("FAIL: add_team_volume TOS");
        return 100 + code;
    }
    if let Err(code) = add_team_volume(&caller, &ASSET_A, 2000, 1) {
        log("FAIL: add_team_volume ASSET_A");
        return 200 + code;
    }
    if let Err(code) = add_team_volume(&caller, &ASSET_B, 3000, 1) {
        log("FAIL: add_team_volume ASSET_B");
        return 300 + code;
    }

    // Verify each asset has EXACTLY the expected volume
    let after_tos = match get_team_volume(&caller, &TOS_ASSET) {
        Ok(v) => v,
        Err(code) => return 400 + code,
    };
    let after_a = match get_team_volume(&caller, &ASSET_A) {
        Ok(v) => v,
        Err(code) => return 410 + code,
    };
    let after_b = match get_team_volume(&caller, &ASSET_B) {
        Ok(v) => v,
        Err(code) => return 420 + code,
    };

    // EXACT verification for each asset
    if after_tos != init_tos + 1000 {
        log("FAIL: TOS volume mismatch");
        return 500;
    }
    if after_a != init_a + 2000 {
        log("FAIL: ASSET_A volume mismatch");
        return 501;
    }
    if after_b != init_b + 3000 {
        log("FAIL: ASSET_B volume mismatch");
        return 502;
    }

    log("PASS: All 3 assets tracked independently with EXACT values");
    0
}

/// Test 7: Multi-asset - verify adding to one asset doesn't affect others
#[no_mangle]
pub extern "C" fn test_multi_asset_isolation() -> u64 {
    log("Test: Multi-asset isolation");
    let caller = get_caller();

    // Get current volumes
    let before_tos = get_team_volume(&caller, &TOS_ASSET).unwrap_or(0);
    let before_a = get_team_volume(&caller, &ASSET_A).unwrap_or(0);
    let before_b = get_team_volume(&caller, &ASSET_B).unwrap_or(0);

    // Add 5000 to ASSET_A ONLY
    if let Err(code) = add_team_volume(&caller, &ASSET_A, 5000, 1) {
        log("FAIL: add_team_volume ASSET_A");
        return 100 + code;
    }

    // Verify ASSET_A increased by exactly 5000
    let after_a = match get_team_volume(&caller, &ASSET_A) {
        Ok(v) => v,
        Err(code) => return 200 + code,
    };
    if after_a != before_a + 5000 {
        log("FAIL: ASSET_A didn't increase correctly");
        return 300;
    }

    // Verify TOS and ASSET_B are UNCHANGED
    let after_tos = match get_team_volume(&caller, &TOS_ASSET) {
        Ok(v) => v,
        Err(code) => return 210 + code,
    };
    let after_b = match get_team_volume(&caller, &ASSET_B) {
        Ok(v) => v,
        Err(code) => return 220 + code,
    };

    if after_tos != before_tos {
        log("FAIL: TOS volume changed unexpectedly!");
        return 400;
    }
    if after_b != before_b {
        log("FAIL: ASSET_B volume changed unexpectedly!");
        return 401;
    }

    log("PASS: Adding to one asset doesn't affect others");
    0
}

/// Test 8: Multi-asset direct_volume tracking
#[no_mangle]
pub extern "C" fn test_multi_asset_direct_volume() -> u64 {
    log("Test: Multi-asset direct_volume");
    let caller = get_caller();

    // Get initial direct volumes
    let init_tos = get_direct_volume(&caller, &TOS_ASSET).unwrap_or(0);
    let init_a = get_direct_volume(&caller, &ASSET_A).unwrap_or(0);

    // Add to both assets with levels=1 (affects direct_volume)
    if let Err(code) = add_team_volume(&caller, &TOS_ASSET, 1111, 1) {
        log("FAIL: add_team_volume TOS");
        return 100 + code;
    }
    if let Err(code) = add_team_volume(&caller, &ASSET_A, 2222, 1) {
        log("FAIL: add_team_volume ASSET_A");
        return 200 + code;
    }

    // Verify direct_volume for each
    let after_tos = match get_direct_volume(&caller, &TOS_ASSET) {
        Ok(v) => v,
        Err(code) => return 300 + code,
    };
    let after_a = match get_direct_volume(&caller, &ASSET_A) {
        Ok(v) => v,
        Err(code) => return 310 + code,
    };

    if after_tos != init_tos + 1111 {
        log("FAIL: TOS direct_volume mismatch");
        return 400;
    }
    if after_a != init_a + 2222 {
        log("FAIL: ASSET_A direct_volume mismatch");
        return 401;
    }

    log("PASS: Multi-asset direct_volume tracked correctly");
    0
}

/// Test 9: Multi-asset zone volumes
#[no_mangle]
pub extern "C" fn test_multi_asset_zone_volumes() -> u64 {
    log("Test: Multi-asset zone_volumes");
    let caller = get_caller();

    // Test zone_volumes for different assets
    if let Err(code) = get_zone_volumes(&caller, &TOS_ASSET, 5) {
        log("FAIL: get_zone_volumes TOS");
        return 100 + code;
    }
    if let Err(code) = get_zone_volumes(&caller, &ASSET_A, 5) {
        log("FAIL: get_zone_volumes ASSET_A");
        return 200 + code;
    }
    if let Err(code) = get_zone_volumes(&caller, &ASSET_B, 5) {
        log("FAIL: get_zone_volumes ASSET_B");
        return 300 + code;
    }

    log("PASS: get_zone_volumes works for multiple assets");
    0
}

/// Main entrypoint - runs all tests including multi-asset
#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    log("=== Team Volume Tests (with Multi-Asset) ===");

    // Original exact verification tests
    let t1 = test_exact_team_volume_increase();
    if t1 != 0 {
        log("FAILED: test_exact_team_volume_increase");
        return t1;
    }

    let t2 = test_exact_direct_volume_increase();
    if t2 != 0 {
        log("FAILED: test_exact_direct_volume_increase");
        return t2;
    }

    let t3 = test_exact_accumulation();
    if t3 != 0 {
        log("FAILED: test_exact_accumulation");
        return t3;
    }

    let t4 = test_get_zone_volumes();
    if t4 != 0 {
        log("FAILED: test_get_zone_volumes");
        return t4;
    }

    let t5 = test_comprehensive_exact();
    if t5 != 0 {
        log("FAILED: test_comprehensive_exact");
        return t5;
    }

    // Multi-asset tests
    let t6 = test_multi_asset_independent();
    if t6 != 0 {
        log("FAILED: test_multi_asset_independent");
        return t6;
    }

    let t7 = test_multi_asset_isolation();
    if t7 != 0 {
        log("FAILED: test_multi_asset_isolation");
        return t7;
    }

    let t8 = test_multi_asset_direct_volume();
    if t8 != 0 {
        log("FAILED: test_multi_asset_direct_volume");
        return t8;
    }

    let t9 = test_multi_asset_zone_volumes();
    if t9 != 0 {
        log("FAILED: test_multi_asset_zone_volumes");
        return t9;
    }

    log("=== ALL TESTS PASSED (9 tests) ===");
    0
}

/// Run only multi-asset tests
#[no_mangle]
pub extern "C" fn test_multi_asset_all() -> u64 {
    log("=== Multi-Asset Volume Tests ===");

    let t6 = test_multi_asset_independent();
    if t6 != 0 {
        log("FAILED: test_multi_asset_independent");
        return t6;
    }

    let t7 = test_multi_asset_isolation();
    if t7 != 0 {
        log("FAILED: test_multi_asset_isolation");
        return t7;
    }

    let t8 = test_multi_asset_direct_volume();
    if t8 != 0 {
        log("FAILED: test_multi_asset_direct_volume");
        return t8;
    }

    let t9 = test_multi_asset_zone_volumes();
    if t9 != 0 {
        log("FAILED: test_multi_asset_zone_volumes");
        return t9;
    }

    log("=== ALL MULTI-ASSET TESTS PASSED (4 tests) ===");
    0
}
