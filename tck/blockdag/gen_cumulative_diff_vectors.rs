// Generate BlockDAG Cumulative Difficulty test vectors
// Run: cd ~/tos/tck/blockdag && cargo run --release --bin gen_cumulative_diff_vectors
//
// cumulative_diff = max(parent_cumulative_diffs) + block_difficulty
// Genesis has cumulative_diff = difficulty

use serde::Serialize;
use std::fs::File;
use std::io::Write;

fn calculate_cumulative_diff(parent_cumulative_diffs: &[u64], block_difficulty: u64) -> u64 {
    if parent_cumulative_diffs.is_empty() {
        return block_difficulty;
    }
    parent_cumulative_diffs.iter().max().unwrap() + block_difficulty
}

#[derive(Serialize)]
struct TestVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    parent_cumulative_diffs: String, // Comma-separated
    block_difficulty: u64,
    expected_cumulative_diff: u64,
}

#[derive(Serialize)]
struct CumulativeDiffTestFile {
    algorithm: String,
    test_vectors: Vec<TestVector>,
}

fn to_csv(values: &[u64]) -> String {
    values.iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn main() {
    let mut vectors = Vec::new();

    // Test 1: Genesis block
    {
        let parents: &[u64] = &[];
        let diff = 100000u64;
        vectors.push(TestVector {
            name: "genesis_block".to_string(),
            description: Some("Genesis block (no parents)".to_string()),
            parent_cumulative_diffs: to_csv(parents),
            block_difficulty: diff,
            expected_cumulative_diff: calculate_cumulative_diff(parents, diff),
        });
    }

    // Test 2: Single parent
    {
        let parents = &[500000u64];
        let diff = 100000u64;
        vectors.push(TestVector {
            name: "single_parent".to_string(),
            description: Some("Single parent".to_string()),
            parent_cumulative_diffs: to_csv(parents),
            block_difficulty: diff,
            expected_cumulative_diff: calculate_cumulative_diff(parents, diff),
        });
    }

    // Test 3: Two parents same cumulative
    {
        let parents = &[1000000u64, 1000000];
        let diff = 200000u64;
        vectors.push(TestVector {
            name: "two_parents_same_cumulative".to_string(),
            description: Some("Two parents with same cumulative difficulty".to_string()),
            parent_cumulative_diffs: to_csv(parents),
            block_difficulty: diff,
            expected_cumulative_diff: calculate_cumulative_diff(parents, diff),
        });
    }

    // Test 4: Two parents different cumulative
    {
        let parents = &[1000000u64, 1500000];
        let diff = 200000u64;
        vectors.push(TestVector {
            name: "two_parents_different_cumulative".to_string(),
            description: Some("Two parents with different cumulative difficulty".to_string()),
            parent_cumulative_diffs: to_csv(parents),
            block_difficulty: diff,
            expected_cumulative_diff: calculate_cumulative_diff(parents, diff),
        });
    }

    // Test 5: Max first
    {
        let parents = &[5000000u64, 3000000, 4000000];
        let diff = 100000u64;
        vectors.push(TestVector {
            name: "three_parents_max_first".to_string(),
            description: Some("Maximum cumulative difficulty is first parent".to_string()),
            parent_cumulative_diffs: to_csv(parents),
            block_difficulty: diff,
            expected_cumulative_diff: calculate_cumulative_diff(parents, diff),
        });
    }

    // Test 6: Max middle
    {
        let parents = &[3000000u64, 5000000, 4000000];
        let diff = 100000u64;
        vectors.push(TestVector {
            name: "three_parents_max_middle".to_string(),
            description: Some("Maximum cumulative difficulty is middle parent".to_string()),
            parent_cumulative_diffs: to_csv(parents),
            block_difficulty: diff,
            expected_cumulative_diff: calculate_cumulative_diff(parents, diff),
        });
    }

    // Test 7: Max last
    {
        let parents = &[3000000u64, 4000000, 5000000];
        let diff = 100000u64;
        vectors.push(TestVector {
            name: "three_parents_max_last".to_string(),
            description: Some("Maximum cumulative difficulty is last parent".to_string()),
            parent_cumulative_diffs: to_csv(parents),
            block_difficulty: diff,
            expected_cumulative_diff: calculate_cumulative_diff(parents, diff),
        });
    }

    // Test 8: Minimum difficulty block
    {
        let parents = &[1000000u64];
        let diff = 100000u64;
        vectors.push(TestVector {
            name: "minimum_difficulty_block".to_string(),
            description: Some("Block with minimum mainnet difficulty".to_string()),
            parent_cumulative_diffs: to_csv(parents),
            block_difficulty: diff,
            expected_cumulative_diff: calculate_cumulative_diff(parents, diff),
        });
    }

    // Test 9: High difficulty block
    {
        let parents = &[1000000u64, 2000000, 1500000];
        let diff = 10000000u64;
        vectors.push(TestVector {
            name: "high_difficulty_block".to_string(),
            description: Some("Block with very high difficulty".to_string()),
            parent_cumulative_diffs: to_csv(parents),
            block_difficulty: diff,
            expected_cumulative_diff: calculate_cumulative_diff(parents, diff),
        });
    }

    // Test 10: All same cumulative
    {
        let parents = &[7777777u64, 7777777, 7777777];
        let diff = 123456u64;
        vectors.push(TestVector {
            name: "all_same_cumulative".to_string(),
            description: Some("All parents with same cumulative difficulty".to_string()),
            parent_cumulative_diffs: to_csv(parents),
            block_difficulty: diff,
            expected_cumulative_diff: calculate_cumulative_diff(parents, diff),
        });
    }

    let test_file = CumulativeDiffTestFile {
        algorithm: "BlockDAG_Cumulative_Difficulty".to_string(),
        test_vectors: vectors,
    };

    let yaml = serde_yaml::to_string(&test_file).unwrap();
    println!("{}", yaml);

    let mut file = File::create("blockdag_cumulative_diff.yaml").unwrap();
    file.write_all(yaml.as_bytes()).unwrap();
    eprintln!("Written to blockdag_cumulative_diff.yaml");
}
