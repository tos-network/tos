// Generate BlockDAG Difficulty V2 (Kalman filter) test vectors
// Run: cd ~/tos/tck/blockdag && cargo run --release --bin gen_difficulty_vectors
//
// This generates YAML test vectors with expected values computed by TOS Rust.
// Avatar C should produce identical results for cross-language verification.

use primitive_types::U256;
use serde::Serialize;
use std::fs::File;
use std::io::Write;

// Constants matching TOS Rust daemon/src/core/difficulty/v2.rs
const MILLIS_PER_SECOND: u64 = 1000;
const SHIFT: u64 = 20;
const LEFT_SHIFT: u64 = 1 << SHIFT;
const PROCESS_NOISE_COVAR: u64 = (1 << SHIFT) * SHIFT / MILLIS_PER_SECOND;
const INITIAL_P: u64 = LEFT_SHIFT;

// Minimum difficulties per network
const MIN_MAINNET: u64 = 100_000;
const MIN_TESTNET: u64 = 10_000;
const MIN_DEVNET: u64 = 1_000;

fn kalman_filter(
    z: U256,
    x_est_prev: U256,
    p_prev: U256,
) -> (U256, U256) {
    let left_shift = U256::from(LEFT_SHIFT);
    let process_noise = U256::from(PROCESS_NOISE_COVAR);

    // Scale up
    let z = z * left_shift;
    let r = z * 2;
    let x_est_prev_scaled = x_est_prev * left_shift;

    // Prediction step
    let p_pred = ((x_est_prev_scaled * process_noise) >> SHIFT) + p_prev;

    // Update step
    let k = (p_pred << SHIFT) / (p_pred + r + U256::one());

    // Ensure positive numbers only
    let mut x_est_new = if z >= x_est_prev_scaled {
        x_est_prev_scaled + ((k * (z - x_est_prev_scaled)) >> SHIFT)
    } else {
        x_est_prev_scaled - ((k * (x_est_prev_scaled - z)) >> SHIFT)
    };

    let p_new = ((left_shift - k) * p_pred) >> SHIFT;

    // Scale down
    x_est_new >>= SHIFT;

    (x_est_new, p_new)
}

fn calculate_difficulty_v2(
    solve_time: u64,
    previous_difficulty: u64,
    p: u64,
    minimum_difficulty: u64,
    block_time_target: u64,
) -> (u64, u64) {
    let prev_diff = U256::from(previous_difficulty);
    let p_var = U256::from(p);

    // Avoid division by zero
    let solve_time = if solve_time == 0 { 1 } else { solve_time };

    let z = prev_diff * MILLIS_PER_SECOND / solve_time;

    let (x_est_new, p_new) = kalman_filter(
        z,
        prev_diff * MILLIS_PER_SECOND / block_time_target,
        p_var,
    );

    let difficulty = x_est_new * block_time_target / MILLIS_PER_SECOND;

    // Convert to u64 for comparison
    let diff_u64 = difficulty.low_u64();
    let p_new_u64 = p_new.low_u64();

    if diff_u64 < minimum_difficulty {
        return (minimum_difficulty, INITIAL_P);
    }

    (diff_u64, p_new_u64)
}

#[derive(Serialize)]
struct TestVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    solve_time_ms: u64,
    previous_difficulty: u64,
    previous_covariance: u64,
    minimum_difficulty: u64,
    block_time_target_ms: u64,
    expected_difficulty: u64,
    expected_covariance: u64,
}

#[derive(Serialize)]
struct DifficultyTestFile {
    algorithm: String,
    constants: Constants,
    test_vectors: Vec<TestVector>,
}

#[derive(Serialize)]
struct Constants {
    shift: u64,
    left_shift: u64,
    process_noise: u64,
    initial_p: u64,
    min_mainnet: u64,
    min_testnet: u64,
    min_devnet: u64,
}

fn main() {
    let mut vectors = Vec::new();
    let target = 1000u64; // 1 second block time

    // Test 1: Exact target time - difficulty unchanged
    {
        let (diff, cov) = calculate_difficulty_v2(1000, 500000, INITIAL_P, MIN_MAINNET, target);
        vectors.push(TestVector {
            name: "exact_target_time".to_string(),
            description: Some("Solve time equals target - difficulty unchanged".to_string()),
            solve_time_ms: 1000,
            previous_difficulty: 500000,
            previous_covariance: INITIAL_P,
            minimum_difficulty: MIN_MAINNET,
            block_time_target_ms: target,
            expected_difficulty: diff,
            expected_covariance: cov,
        });
    }

    // Test 2: Double target time - difficulty decreases
    {
        let (diff, cov) = calculate_difficulty_v2(2000, 500000, INITIAL_P, MIN_MAINNET, target);
        vectors.push(TestVector {
            name: "double_target_time".to_string(),
            description: Some("Solve time is 2x target - difficulty decreases".to_string()),
            solve_time_ms: 2000,
            previous_difficulty: 500000,
            previous_covariance: INITIAL_P,
            minimum_difficulty: MIN_MAINNET,
            block_time_target_ms: target,
            expected_difficulty: diff,
            expected_covariance: cov,
        });
    }

    // Test 3: Half target time - difficulty increases
    {
        let (diff, cov) = calculate_difficulty_v2(500, 500000, INITIAL_P, MIN_MAINNET, target);
        vectors.push(TestVector {
            name: "half_target_time".to_string(),
            description: Some("Solve time is 0.5x target - difficulty increases".to_string()),
            solve_time_ms: 500,
            previous_difficulty: 500000,
            previous_covariance: INITIAL_P,
            minimum_difficulty: MIN_MAINNET,
            block_time_target_ms: target,
            expected_difficulty: diff,
            expected_covariance: cov,
        });
    }

    // Test 4: Minimum clamp mainnet
    {
        let (diff, cov) = calculate_difficulty_v2(100000, 100000, INITIAL_P, MIN_MAINNET, target);
        vectors.push(TestVector {
            name: "minimum_clamp_mainnet".to_string(),
            description: Some("Difficulty clamped to mainnet minimum".to_string()),
            solve_time_ms: 100000,
            previous_difficulty: 100000,
            previous_covariance: INITIAL_P,
            minimum_difficulty: MIN_MAINNET,
            block_time_target_ms: target,
            expected_difficulty: diff,
            expected_covariance: cov,
        });
    }

    // Test 5: Minimum clamp devnet
    {
        let (diff, cov) = calculate_difficulty_v2(100000, 5000, INITIAL_P, MIN_DEVNET, target);
        vectors.push(TestVector {
            name: "minimum_clamp_devnet".to_string(),
            description: Some("Difficulty clamped to devnet minimum".to_string()),
            solve_time_ms: 100000,
            previous_difficulty: 5000,
            previous_covariance: INITIAL_P,
            minimum_difficulty: MIN_DEVNET,
            block_time_target_ms: target,
            expected_difficulty: diff,
            expected_covariance: cov,
        });
    }

    // Test 6: Very fast block
    {
        let (diff, cov) = calculate_difficulty_v2(10, 500000, INITIAL_P, MIN_MAINNET, target);
        vectors.push(TestVector {
            name: "very_fast_block".to_string(),
            description: Some("Very fast solve time (10ms vs 1000ms target)".to_string()),
            solve_time_ms: 10,
            previous_difficulty: 500000,
            previous_covariance: INITIAL_P,
            minimum_difficulty: MIN_MAINNET,
            block_time_target_ms: target,
            expected_difficulty: diff,
            expected_covariance: cov,
        });
    }

    // Test 7: Covariance growth
    {
        let (diff, cov) = calculate_difficulty_v2(1000, 500000, 2000000, MIN_MAINNET, target);
        vectors.push(TestVector {
            name: "covariance_growth".to_string(),
            description: Some("Covariance grows with consecutive adjustments".to_string()),
            solve_time_ms: 1000,
            previous_difficulty: 500000,
            previous_covariance: 2000000,
            minimum_difficulty: MIN_MAINNET,
            block_time_target_ms: target,
            expected_difficulty: diff,
            expected_covariance: cov,
        });
    }

    // Test 8: Large difficulty
    {
        let (diff, cov) = calculate_difficulty_v2(1000, 1000000000000, INITIAL_P, MIN_MAINNET, target);
        vectors.push(TestVector {
            name: "large_difficulty".to_string(),
            description: Some("Large difficulty value".to_string()),
            solve_time_ms: 1000,
            previous_difficulty: 1000000000000,
            previous_covariance: INITIAL_P,
            minimum_difficulty: MIN_MAINNET,
            block_time_target_ms: target,
            expected_difficulty: diff,
            expected_covariance: cov,
        });
    }

    // Test 9: Recovery from minimum
    {
        let (diff, cov) = calculate_difficulty_v2(100, 100000, INITIAL_P, MIN_MAINNET, target);
        vectors.push(TestVector {
            name: "slow_recovery_from_minimum".to_string(),
            description: Some("Recovery from minimum difficulty".to_string()),
            solve_time_ms: 100,
            previous_difficulty: 100000,
            previous_covariance: INITIAL_P,
            minimum_difficulty: MIN_MAINNET,
            block_time_target_ms: target,
            expected_difficulty: diff,
            expected_covariance: cov,
        });
    }

    // Test 10: Genesis-like first block
    {
        let (diff, cov) = calculate_difficulty_v2(800, 100000, INITIAL_P, MIN_MAINNET, target);
        vectors.push(TestVector {
            name: "genesis_like".to_string(),
            description: Some("First block after genesis".to_string()),
            solve_time_ms: 800,
            previous_difficulty: 100000,
            previous_covariance: INITIAL_P,
            minimum_difficulty: MIN_MAINNET,
            block_time_target_ms: target,
            expected_difficulty: diff,
            expected_covariance: cov,
        });
    }

    let test_file = DifficultyTestFile {
        algorithm: "BlockDAG_Difficulty_V2".to_string(),
        constants: Constants {
            shift: SHIFT,
            left_shift: LEFT_SHIFT,
            process_noise: PROCESS_NOISE_COVAR,
            initial_p: INITIAL_P,
            min_mainnet: MIN_MAINNET,
            min_testnet: MIN_TESTNET,
            min_devnet: MIN_DEVNET,
        },
        test_vectors: vectors,
    };

    let yaml = serde_yaml::to_string(&test_file).unwrap();
    println!("{}", yaml);

    let mut file = File::create("blockdag_difficulty.yaml").unwrap();
    file.write_all(yaml.as_bytes()).unwrap();
    eprintln!("Written to blockdag_difficulty.yaml");
}
