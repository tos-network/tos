// Generate BlockDAG Height Calculation test vectors
// Run: cd ~/tos/tck/blockdag && cargo run --release --bin gen_height_vectors
//
// Height = max(parent_heights) + 1
// Genesis block has height 0 and no tips

use serde::Serialize;
use std::fs::File;
use std::io::Write;

fn calculate_height(parent_heights: &[u64]) -> u64 {
    if parent_heights.is_empty() {
        return 0;
    }
    parent_heights.iter().max().unwrap() + 1
}

#[derive(Serialize)]
struct TestVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    parent_heights: String, // Comma-separated for simple YAML parser
    expected_height: u64,
}

#[derive(Serialize)]
struct HeightTestFile {
    algorithm: String,
    test_vectors: Vec<TestVector>,
}

fn heights_to_csv(heights: &[u64]) -> String {
    heights.iter()
        .map(|h| h.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn main() {
    let mut vectors = Vec::new();

    // Test 1: Genesis block
    {
        let heights: &[u64] = &[];
        vectors.push(TestVector {
            name: "genesis_block".to_string(),
            description: Some("Genesis block has height 0 (no parents)".to_string()),
            parent_heights: heights_to_csv(heights),
            expected_height: calculate_height(heights),
        });
    }

    // Test 2: Single parent
    {
        let heights = &[5u64];
        vectors.push(TestVector {
            name: "single_parent".to_string(),
            description: Some("Single parent at height 5".to_string()),
            parent_heights: heights_to_csv(heights),
            expected_height: calculate_height(heights),
        });
    }

    // Test 3: Two parents same height
    {
        let heights = &[10u64, 10];
        vectors.push(TestVector {
            name: "two_parents_same_height".to_string(),
            description: Some("Two parents at same height".to_string()),
            parent_heights: heights_to_csv(heights),
            expected_height: calculate_height(heights),
        });
    }

    // Test 4: Two parents different height
    {
        let heights = &[5u64, 10];
        vectors.push(TestVector {
            name: "two_parents_different_height".to_string(),
            description: Some("Two parents at different heights".to_string()),
            parent_heights: heights_to_csv(heights),
            expected_height: calculate_height(heights),
        });
    }

    // Test 5: Three parents different heights
    {
        let heights = &[3u64, 7, 5];
        vectors.push(TestVector {
            name: "three_parents_different_heights".to_string(),
            description: Some("Three parents at different heights".to_string()),
            parent_heights: heights_to_csv(heights),
            expected_height: calculate_height(heights),
        });
    }

    // Test 6: Max height first
    {
        let heights = &[100u64, 50, 75];
        vectors.push(TestVector {
            name: "three_parents_max_first".to_string(),
            description: Some("Maximum height parent is first".to_string()),
            parent_heights: heights_to_csv(heights),
            expected_height: calculate_height(heights),
        });
    }

    // Test 7: Max height last
    {
        let heights = &[50u64, 75, 100];
        vectors.push(TestVector {
            name: "three_parents_max_last".to_string(),
            description: Some("Maximum height parent is last".to_string()),
            parent_heights: heights_to_csv(heights),
            expected_height: calculate_height(heights),
        });
    }

    // Test 8: Large heights
    {
        let heights = &[1000000u64, 999999, 1000000];
        vectors.push(TestVector {
            name: "large_heights".to_string(),
            description: Some("Large height values".to_string()),
            parent_heights: heights_to_csv(heights),
            expected_height: calculate_height(heights),
        });
    }

    // Test 9: Height one
    {
        let heights = &[0u64];
        vectors.push(TestVector {
            name: "height_one".to_string(),
            description: Some("Block at height 1 (parent is genesis)".to_string()),
            parent_heights: heights_to_csv(heights),
            expected_height: calculate_height(heights),
        });
    }

    // Test 10: All same height
    {
        let heights = &[42u64, 42, 42];
        vectors.push(TestVector {
            name: "all_same_height".to_string(),
            description: Some("All three parents at same height".to_string()),
            parent_heights: heights_to_csv(heights),
            expected_height: calculate_height(heights),
        });
    }

    let test_file = HeightTestFile {
        algorithm: "BlockDAG_Height".to_string(),
        test_vectors: vectors,
    };

    let yaml = serde_yaml::to_string(&test_file).unwrap();
    println!("{}", yaml);

    let mut file = File::create("blockdag_height.yaml").unwrap();
    file.write_all(yaml.as_bytes()).unwrap();
    eprintln!("Written to blockdag_height.yaml");
}
