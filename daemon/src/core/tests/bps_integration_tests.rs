// TOS BPS System Integration Tests
//
// This module tests the integration of the BPS (Blocks Per Second) configuration system
// with the rest of the TOS consensus layer, particularly:
// - Hard fork configuration
// - Difficulty adjustment
// - GHOSTDAG parameters
// - Block validation

use crate::core::bps::{OneBps, TenBps, calculate_ghostdag_k};
use crate::core::hard_fork::get_block_time_target_for_version;
use tos_common::block::BlockVersion;

#[test]
fn test_bps_hard_fork_integration() {
    // V0 should use legacy 60-second blocks
    assert_eq!(get_block_time_target_for_version(BlockVersion::V0), 60_000);

    // V1/V2/V3 should use OneBps configuration (1 second blocks)
    assert_eq!(get_block_time_target_for_version(BlockVersion::V1), OneBps::target_time_per_block());
    assert_eq!(get_block_time_target_for_version(BlockVersion::V2), OneBps::target_time_per_block());
    assert_eq!(get_block_time_target_for_version(BlockVersion::V3), OneBps::target_time_per_block());

    // Verify OneBps is 1000ms
    assert_eq!(OneBps::target_time_per_block(), 1000);
}

#[test]
fn test_bps_parameter_consistency() {
    // Test OneBps parameter consistency
    let one_bps = OneBps::bps();
    let one_target_time = OneBps::target_time_per_block();
    let one_k = OneBps::ghostdag_k();
    let one_finality = OneBps::finality_depth();
    let one_maturity = OneBps::coinbase_maturity();

    assert_eq!(one_bps, 1);
    assert_eq!(one_target_time, 1000);
    assert_eq!(one_k, 10);
    assert_eq!(one_finality, 100);
    assert_eq!(one_maturity, 100);

    // Test TenBps parameter consistency
    let ten_bps = TenBps::bps();
    let ten_target_time = TenBps::target_time_per_block();
    let ten_k = TenBps::ghostdag_k();
    let ten_finality = TenBps::finality_depth();
    let ten_maturity = TenBps::coinbase_maturity();

    assert_eq!(ten_bps, 10);
    assert_eq!(ten_target_time, 100);
    assert_eq!(ten_k, 124);
    assert_eq!(ten_finality, 1000);
    assert_eq!(ten_maturity, 1000);
}

#[test]
fn test_bps_time_invariance() {
    // Verify that time-based parameters remain constant across different BPS configs

    // Finality time should be ~100 seconds for both
    let one_finality_time_ms = OneBps::finality_depth() * OneBps::target_time_per_block();
    let ten_finality_time_ms = TenBps::finality_depth() * TenBps::target_time_per_block();

    assert_eq!(one_finality_time_ms, 100_000);  // 100 seconds
    assert_eq!(ten_finality_time_ms, 100_000);  // 100 seconds

    // Coinbase maturity should be ~100 seconds for both
    let one_maturity_time_ms = OneBps::coinbase_maturity() * OneBps::target_time_per_block();
    let ten_maturity_time_ms = TenBps::coinbase_maturity() * TenBps::target_time_per_block();

    assert_eq!(one_maturity_time_ms, 100_000);  // 100 seconds
    assert_eq!(ten_maturity_time_ms, 100_000);  // 100 seconds

    // Pruning depth should be ~200 seconds for both
    let one_pruning_time_ms = OneBps::pruning_depth() * OneBps::target_time_per_block();
    let ten_pruning_time_ms = TenBps::pruning_depth() * TenBps::target_time_per_block();

    assert_eq!(one_pruning_time_ms, 200_000);  // 200 seconds
    assert_eq!(ten_pruning_time_ms, 200_000);  // 200 seconds
}

#[test]
fn test_bps_ghostdag_k_scaling() {
    // K should scale appropriately with BPS
    // For 1 BPS: K=10
    // For 10 BPS: K=124

    assert_eq!(OneBps::ghostdag_k(), 10);
    assert_eq!(TenBps::ghostdag_k(), 124);

    // K ratio should be roughly proportional to BPS ratio
    let bps_ratio = TenBps::bps() as f64 / OneBps::bps() as f64;
    let k_ratio = TenBps::ghostdag_k() as f64 / OneBps::ghostdag_k() as f64;

    // K should grow slower than linear (logarithmic behavior)
    assert!(k_ratio < bps_ratio * 2.0);
    assert!(k_ratio > bps_ratio * 0.5);
}

#[test]
fn test_bps_calculate_ghostdag_k_accuracy() {
    // Test the calculate_ghostdag_k function against known values

    // For 1 BPS with D=2s, delta=0.001
    // x = 2 * D * lambda = 2 * 2 * 1 = 4.0
    // Expected K ~9.7, mathematically calculated result may vary slightly
    let k_one_bps = calculate_ghostdag_k(4.0, 0.001);
    assert!(k_one_bps >= 9 && k_one_bps <= 11, "K for 1 BPS should be 9-11, got {}", k_one_bps);

    // For 10 BPS with D=2s, delta=0.001
    // x = 2 * D * lambda = 2 * 2 * 10 = 40.0
    // Expected K ~63.4, Kaspa uses 124 (with safety margin)
    let k_ten_bps = calculate_ghostdag_k(40.0, 0.001);
    assert!(k_ten_bps >= 60 && k_ten_bps <= 65, "K for 10 BPS should be 60-65, got {}", k_ten_bps);
}

#[test]
fn test_bps_max_parents_bounds() {
    // Test that max_block_parents respects bounds [10, 16]

    // OneBps: K=10, so 10/2=5, clamped to min 10
    assert_eq!(OneBps::max_block_parents(), 10);
    assert!(OneBps::max_block_parents() >= 10);

    // TenBps: K=124, so 124/2=62, capped at 16
    assert_eq!(TenBps::max_block_parents(), 16);
    assert!(TenBps::max_block_parents() <= 16);
}

#[test]
fn test_bps_mergeset_size_limit_bounds() {
    // Test that mergeset_size_limit respects bounds [180, 512]

    // OneBps: K=10, so 2*10=20, clamped to 180
    assert_eq!(OneBps::mergeset_size_limit(), 180);
    assert!(OneBps::mergeset_size_limit() >= 180);

    // TenBps: K=124, so 2*124=248
    assert_eq!(TenBps::mergeset_size_limit(), 248);
    assert!(TenBps::mergeset_size_limit() >= 180);
    assert!(TenBps::mergeset_size_limit() <= 512);
}

#[test]
fn test_bps_relationship_to_block_versions() {
    // Ensure all modern block versions use consistent BPS configuration

    let v1_target = get_block_time_target_for_version(BlockVersion::V1);
    let v2_target = get_block_time_target_for_version(BlockVersion::V2);
    let v3_target = get_block_time_target_for_version(BlockVersion::V3);

    // All should be equal (OneBps)
    assert_eq!(v1_target, v2_target);
    assert_eq!(v2_target, v3_target);

    // All should be 1000ms
    assert_eq!(v1_target, 1000);
}

#[test]
fn test_bps_compile_time_evaluation() {
    // This test verifies that BPS functions are const and can be used in const contexts

    const ONE_BPS: u64 = OneBps::bps();
    const ONE_TARGET: u64 = OneBps::target_time_per_block();
    const ONE_K: u64 = OneBps::ghostdag_k();
    const ONE_PARENTS: u8 = OneBps::max_block_parents();
    const ONE_MERGESET: u64 = OneBps::mergeset_size_limit();
    const ONE_FINALITY: u64 = OneBps::finality_depth();
    const ONE_PRUNING: u64 = OneBps::pruning_depth();
    const ONE_MATURITY: u64 = OneBps::coinbase_maturity();

    assert_eq!(ONE_BPS, 1);
    assert_eq!(ONE_TARGET, 1000);
    assert_eq!(ONE_K, 10);
    assert_eq!(ONE_PARENTS, 10);
    assert_eq!(ONE_MERGESET, 180);
    assert_eq!(ONE_FINALITY, 100);
    assert_eq!(ONE_PRUNING, 200);
    assert_eq!(ONE_MATURITY, 100);
}

#[test]
fn test_bps_zero_runtime_cost() {
    // This test demonstrates that BPS functions have zero runtime cost
    // by verifying they can be used in const contexts (compile-time only)

    // If these compile, they are evaluated at compile time
    const _: () = {
        let _ = OneBps::bps();
        let _ = OneBps::target_time_per_block();
        let _ = OneBps::ghostdag_k();
        let _ = OneBps::max_block_parents();
        let _ = OneBps::mergeset_size_limit();
        let _ = OneBps::finality_depth();
        let _ = OneBps::pruning_depth();
        let _ = OneBps::coinbase_maturity();
    };
}

#[test]
fn test_bps_type_safety() {
    // This test demonstrates type-level distinction between BPS configurations
    // OneBps and TenBps are distinct types, preventing accidental mixing

    use std::any::TypeId;
    use crate::core::bps::Bps;

    // OneBps and TenBps should have different TypeIds
    assert_ne!(TypeId::of::<Bps<1>>(), TypeId::of::<Bps<10>>());

    // This ensures compile-time safety - you can't accidentally use
    // parameters from one BPS config with another
}

/// Benchmark-style test to measure BPS parameter calculation overhead
/// (Should be zero since everything is const)
#[test]
fn test_bps_performance() {
    use std::time::Instant;

    let iterations = 1_000_000;
    let start = Instant::now();

    for _ in 0..iterations {
        // These calls should be optimized away since they're const
        let _ = OneBps::bps();
        let _ = OneBps::target_time_per_block();
        let _ = OneBps::ghostdag_k();
        let _ = OneBps::max_block_parents();
    }

    let duration = start.elapsed();

    // Should complete very quickly (optimized to nothing)
    // On a modern CPU, 1M iterations should take < 1ms if properly optimized
    println!("BPS parameter access: {:?} for {} iterations", duration, iterations);

    // Loose assertion - should be negligible
    assert!(duration.as_millis() < 100, "BPS parameter access taking too long: {:?}", duration);
}

#[cfg(test)]
mod property_tests {
    use super::*;

    #[test]
    fn property_finality_time_equals_coinbase_maturity_time() {
        // Property: For any BPS config, finality time == coinbase maturity time

        assert_eq!(
            OneBps::finality_depth() * OneBps::target_time_per_block(),
            OneBps::coinbase_maturity() * OneBps::target_time_per_block()
        );

        assert_eq!(
            TenBps::finality_depth() * TenBps::target_time_per_block(),
            TenBps::coinbase_maturity() * TenBps::target_time_per_block()
        );
    }

    #[test]
    fn property_pruning_depth_greater_than_finality() {
        // Property: pruning_depth > finality_depth (safety margin)

        assert!(OneBps::pruning_depth() > OneBps::finality_depth());
        assert!(TenBps::pruning_depth() > TenBps::finality_depth());

        // Should be exactly 2x
        assert_eq!(OneBps::pruning_depth(), OneBps::finality_depth() * 2);
        assert_eq!(TenBps::pruning_depth(), TenBps::finality_depth() * 2);
    }

    #[test]
    fn property_max_parents_derived_from_k() {
        // Property: max_parents is derived from K with bounds

        let one_k = OneBps::ghostdag_k();
        let one_parents = OneBps::max_block_parents();

        // Should be K/2 with bounds [10, 16]
        let expected = ((one_k / 2) as u8).max(10).min(16);
        assert_eq!(one_parents, expected);

        let ten_k = TenBps::ghostdag_k();
        let ten_parents = TenBps::max_block_parents();

        let expected = ((ten_k / 2) as u8).max(10).min(16);
        assert_eq!(ten_parents, expected);
    }
}
