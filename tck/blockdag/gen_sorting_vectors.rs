// Generate BlockDAG Sorting test vectors
// Run: cd ~/tos/tck/blockdag && cargo run --release --bin gen_sorting_vectors
//
// Sorting rules:
// - Primary: by cumulative difficulty
// - Tiebreaker (same cumulative difficulty):
//   - Ascending order: lower hash wins (appears first)
//   - Descending order: higher hash wins (appears first)

use serde::Serialize;
use std::cmp::Ordering;
use std::fs::File;
use std::io::Write;

#[derive(Clone)]
struct Block {
    hash: String, // Short hex for simplicity
    cumulative_diff: u64,
}

fn sort_blocks(blocks: &mut [Block], ascending: bool) {
    blocks.sort_by(|a, b| {
        // Primary: compare by cumulative difficulty
        let diff_cmp = if ascending {
            a.cumulative_diff.cmp(&b.cumulative_diff)
        } else {
            b.cumulative_diff.cmp(&a.cumulative_diff)
        };

        if diff_cmp != Ordering::Equal {
            return diff_cmp;
        }

        // Tiebreaker: compare hashes
        // Ascending: lower hash first
        // Descending: higher hash first
        if ascending {
            a.hash.cmp(&b.hash)
        } else {
            b.hash.cmp(&a.hash)
        }
    });
}

#[derive(Serialize)]
struct TestVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    ascending: bool,
    blocks: String,         // "hash:diff,hash:diff,..." format
    expected_order: String, // "hash,hash,..." format
}

#[derive(Serialize)]
struct SortingTestFile {
    algorithm: String,
    test_vectors: Vec<TestVector>,
}

fn blocks_to_string(blocks: &[Block]) -> String {
    blocks.iter()
        .map(|b| format!("{}:{}", b.hash, b.cumulative_diff))
        .collect::<Vec<_>>()
        .join(",")
}

fn hashes_to_string(blocks: &[Block]) -> String {
    blocks.iter()
        .map(|b| b.hash.clone())
        .collect::<Vec<_>>()
        .join(",")
}

fn main() {
    let mut vectors = Vec::new();

    // Test 1: Ascending distinct difficulties
    {
        let mut blocks = vec![
            Block { hash: "aa".to_string(), cumulative_diff: 3000000 },
            Block { hash: "bb".to_string(), cumulative_diff: 1000000 },
            Block { hash: "cc".to_string(), cumulative_diff: 2000000 },
        ];
        let input = blocks_to_string(&blocks);
        sort_blocks(&mut blocks, true);
        vectors.push(TestVector {
            name: "ascending_distinct_difficulties".to_string(),
            description: Some("Ascending sort with distinct difficulties".to_string()),
            ascending: true,
            blocks: input,
            expected_order: hashes_to_string(&blocks),
        });
    }

    // Test 2: Ascending same difficulty - lower hash first
    {
        let mut blocks = vec![
            Block { hash: "cc".to_string(), cumulative_diff: 1000000 },
            Block { hash: "aa".to_string(), cumulative_diff: 1000000 },
            Block { hash: "bb".to_string(), cumulative_diff: 1000000 },
        ];
        let input = blocks_to_string(&blocks);
        sort_blocks(&mut blocks, true);
        vectors.push(TestVector {
            name: "ascending_same_difficulty_tiebreaker".to_string(),
            description: Some("Ascending sort with same difficulty - lower hash first".to_string()),
            ascending: true,
            blocks: input,
            expected_order: hashes_to_string(&blocks),
        });
    }

    // Test 3: Ascending mixed tiebreaker
    {
        let mut blocks = vec![
            Block { hash: "dd".to_string(), cumulative_diff: 2000000 },
            Block { hash: "aa".to_string(), cumulative_diff: 1000000 },
            Block { hash: "cc".to_string(), cumulative_diff: 2000000 },
            Block { hash: "bb".to_string(), cumulative_diff: 1000000 },
        ];
        let input = blocks_to_string(&blocks);
        sort_blocks(&mut blocks, true);
        vectors.push(TestVector {
            name: "ascending_mixed_tiebreaker".to_string(),
            description: Some("Ascending with some tied difficulties".to_string()),
            ascending: true,
            blocks: input,
            expected_order: hashes_to_string(&blocks),
        });
    }

    // Test 4: Descending distinct difficulties
    {
        let mut blocks = vec![
            Block { hash: "aa".to_string(), cumulative_diff: 1000000 },
            Block { hash: "bb".to_string(), cumulative_diff: 3000000 },
            Block { hash: "cc".to_string(), cumulative_diff: 2000000 },
        ];
        let input = blocks_to_string(&blocks);
        sort_blocks(&mut blocks, false);
        vectors.push(TestVector {
            name: "descending_distinct_difficulties".to_string(),
            description: Some("Descending sort with distinct difficulties".to_string()),
            ascending: false,
            blocks: input,
            expected_order: hashes_to_string(&blocks),
        });
    }

    // Test 5: Descending same difficulty - higher hash first
    {
        let mut blocks = vec![
            Block { hash: "aa".to_string(), cumulative_diff: 1000000 },
            Block { hash: "cc".to_string(), cumulative_diff: 1000000 },
            Block { hash: "bb".to_string(), cumulative_diff: 1000000 },
        ];
        let input = blocks_to_string(&blocks);
        sort_blocks(&mut blocks, false);
        vectors.push(TestVector {
            name: "descending_same_difficulty_tiebreaker".to_string(),
            description: Some("Descending sort with same difficulty - higher hash first".to_string()),
            ascending: false,
            blocks: input,
            expected_order: hashes_to_string(&blocks),
        });
    }

    // Test 6: Descending mixed tiebreaker
    {
        let mut blocks = vec![
            Block { hash: "aa".to_string(), cumulative_diff: 2000000 },
            Block { hash: "dd".to_string(), cumulative_diff: 1000000 },
            Block { hash: "bb".to_string(), cumulative_diff: 1000000 },
            Block { hash: "cc".to_string(), cumulative_diff: 2000000 },
        ];
        let input = blocks_to_string(&blocks);
        sort_blocks(&mut blocks, false);
        vectors.push(TestVector {
            name: "descending_mixed_tiebreaker".to_string(),
            description: Some("Descending with some tied difficulties".to_string()),
            ascending: false,
            blocks: input,
            expected_order: hashes_to_string(&blocks),
        });
    }

    // Test 7: Two blocks ascending same difficulty
    {
        let mut blocks = vec![
            Block { hash: "ff".to_string(), cumulative_diff: 1000000 },
            Block { hash: "01".to_string(), cumulative_diff: 1000000 },
        ];
        let input = blocks_to_string(&blocks);
        sort_blocks(&mut blocks, true);
        vectors.push(TestVector {
            name: "two_blocks_ascending".to_string(),
            description: Some("Two blocks ascending with same difficulty".to_string()),
            ascending: true,
            blocks: input,
            expected_order: hashes_to_string(&blocks),
        });
    }

    // Test 8: Two blocks descending same difficulty
    {
        let mut blocks = vec![
            Block { hash: "01".to_string(), cumulative_diff: 1000000 },
            Block { hash: "ff".to_string(), cumulative_diff: 1000000 },
        ];
        let input = blocks_to_string(&blocks);
        sort_blocks(&mut blocks, false);
        vectors.push(TestVector {
            name: "two_blocks_descending".to_string(),
            description: Some("Two blocks descending with same difficulty".to_string()),
            ascending: false,
            blocks: input,
            expected_order: hashes_to_string(&blocks),
        });
    }

    // Test 9: Single block
    {
        let mut blocks = vec![
            Block { hash: "ab".to_string(), cumulative_diff: 5000000 },
        ];
        let input = blocks_to_string(&blocks);
        sort_blocks(&mut blocks, true);
        vectors.push(TestVector {
            name: "single_block".to_string(),
            description: Some("Single block (trivial sort)".to_string()),
            ascending: true,
            blocks: input,
            expected_order: hashes_to_string(&blocks),
        });
    }

    // Test 10: Already sorted ascending
    {
        let mut blocks = vec![
            Block { hash: "aa".to_string(), cumulative_diff: 1000000 },
            Block { hash: "bb".to_string(), cumulative_diff: 2000000 },
            Block { hash: "cc".to_string(), cumulative_diff: 3000000 },
        ];
        let input = blocks_to_string(&blocks);
        sort_blocks(&mut blocks, true);
        vectors.push(TestVector {
            name: "already_sorted_ascending".to_string(),
            description: Some("Already sorted in ascending order".to_string()),
            ascending: true,
            blocks: input,
            expected_order: hashes_to_string(&blocks),
        });
    }

    // Test 11: Already sorted descending
    {
        let mut blocks = vec![
            Block { hash: "cc".to_string(), cumulative_diff: 3000000 },
            Block { hash: "bb".to_string(), cumulative_diff: 2000000 },
            Block { hash: "aa".to_string(), cumulative_diff: 1000000 },
        ];
        let input = blocks_to_string(&blocks);
        sort_blocks(&mut blocks, false);
        vectors.push(TestVector {
            name: "already_sorted_descending".to_string(),
            description: Some("Already sorted in descending order".to_string()),
            ascending: false,
            blocks: input,
            expected_order: hashes_to_string(&blocks),
        });
    }

    // Test 12: Reverse sorted ascending
    {
        let mut blocks = vec![
            Block { hash: "cc".to_string(), cumulative_diff: 3000000 },
            Block { hash: "bb".to_string(), cumulative_diff: 2000000 },
            Block { hash: "aa".to_string(), cumulative_diff: 1000000 },
        ];
        let input = blocks_to_string(&blocks);
        sort_blocks(&mut blocks, true);
        vectors.push(TestVector {
            name: "reverse_sorted_ascending".to_string(),
            description: Some("Reverse sorted - needs full reorder for ascending".to_string()),
            ascending: true,
            blocks: input,
            expected_order: hashes_to_string(&blocks),
        });
    }

    // Test 13: Zero difficulty values
    {
        let mut blocks = vec![
            Block { hash: "bb".to_string(), cumulative_diff: 0 },
            Block { hash: "aa".to_string(), cumulative_diff: 0 },
            Block { hash: "cc".to_string(), cumulative_diff: 0 },
        ];
        let input = blocks_to_string(&blocks);
        sort_blocks(&mut blocks, true);
        vectors.push(TestVector {
            name: "all_zero_difficulty_ascending".to_string(),
            description: Some("All zero difficulty - pure hash tiebreaker ascending".to_string()),
            ascending: true,
            blocks: input,
            expected_order: hashes_to_string(&blocks),
        });
    }

    // Test 14: Zero difficulty descending
    {
        let mut blocks = vec![
            Block { hash: "bb".to_string(), cumulative_diff: 0 },
            Block { hash: "aa".to_string(), cumulative_diff: 0 },
            Block { hash: "cc".to_string(), cumulative_diff: 0 },
        ];
        let input = blocks_to_string(&blocks);
        sort_blocks(&mut blocks, false);
        vectors.push(TestVector {
            name: "all_zero_difficulty_descending".to_string(),
            description: Some("All zero difficulty - pure hash tiebreaker descending".to_string()),
            ascending: false,
            blocks: input,
            expected_order: hashes_to_string(&blocks),
        });
    }

    // Test 15: Large cumulative difficulty values
    {
        let mut blocks = vec![
            Block { hash: "aa".to_string(), cumulative_diff: 10000000000000000 },
            Block { hash: "bb".to_string(), cumulative_diff: 10000000000000001 },
            Block { hash: "cc".to_string(), cumulative_diff: 9999999999999999 },
        ];
        let input = blocks_to_string(&blocks);
        sort_blocks(&mut blocks, true);
        vectors.push(TestVector {
            name: "large_values_ascending".to_string(),
            description: Some("Large cumulative difficulty values ascending".to_string()),
            ascending: true,
            blocks: input,
            expected_order: hashes_to_string(&blocks),
        });
    }

    // Test 16: Numeric vs alpha hash ordering
    {
        let mut blocks = vec![
            Block { hash: "a0".to_string(), cumulative_diff: 1000 },
            Block { hash: "9f".to_string(), cumulative_diff: 1000 },
            Block { hash: "0a".to_string(), cumulative_diff: 1000 },
        ];
        let input = blocks_to_string(&blocks);
        sort_blocks(&mut blocks, true);
        vectors.push(TestVector {
            name: "numeric_vs_alpha_hash_ascending".to_string(),
            description: Some("Numeric vs alpha hash tiebreaker ascending".to_string()),
            ascending: true,
            blocks: input,
            expected_order: hashes_to_string(&blocks),
        });
    }

    // Test 17: Four blocks mixed
    {
        let mut blocks = vec![
            Block { hash: "dd".to_string(), cumulative_diff: 2000 },
            Block { hash: "aa".to_string(), cumulative_diff: 3000 },
            Block { hash: "cc".to_string(), cumulative_diff: 1000 },
            Block { hash: "bb".to_string(), cumulative_diff: 2000 },
        ];
        let input = blocks_to_string(&blocks);
        sort_blocks(&mut blocks, true);
        vectors.push(TestVector {
            name: "four_blocks_mixed_ascending".to_string(),
            description: Some("Four blocks with mixed order ascending".to_string()),
            ascending: true,
            blocks: input,
            expected_order: hashes_to_string(&blocks),
        });
    }

    // Test 18: Four blocks mixed descending
    {
        let mut blocks = vec![
            Block { hash: "dd".to_string(), cumulative_diff: 2000 },
            Block { hash: "aa".to_string(), cumulative_diff: 3000 },
            Block { hash: "cc".to_string(), cumulative_diff: 1000 },
            Block { hash: "bb".to_string(), cumulative_diff: 2000 },
        ];
        let input = blocks_to_string(&blocks);
        sort_blocks(&mut blocks, false);
        vectors.push(TestVector {
            name: "four_blocks_mixed_descending".to_string(),
            description: Some("Four blocks with mixed order descending".to_string()),
            ascending: false,
            blocks: input,
            expected_order: hashes_to_string(&blocks),
        });
    }

    // Test 19: Consecutive difficulties ascending
    {
        let mut blocks = vec![
            Block { hash: "cc".to_string(), cumulative_diff: 102 },
            Block { hash: "aa".to_string(), cumulative_diff: 100 },
            Block { hash: "bb".to_string(), cumulative_diff: 101 },
        ];
        let input = blocks_to_string(&blocks);
        sort_blocks(&mut blocks, true);
        vectors.push(TestVector {
            name: "consecutive_diff_ascending".to_string(),
            description: Some("Consecutive difficulty values ascending".to_string()),
            ascending: true,
            blocks: input,
            expected_order: hashes_to_string(&blocks),
        });
    }

    // Test 20: All same hash prefix different suffix
    {
        let mut blocks = vec![
            Block { hash: "a3".to_string(), cumulative_diff: 1000 },
            Block { hash: "a1".to_string(), cumulative_diff: 1000 },
            Block { hash: "a2".to_string(), cumulative_diff: 1000 },
        ];
        let input = blocks_to_string(&blocks);
        sort_blocks(&mut blocks, true);
        vectors.push(TestVector {
            name: "same_prefix_different_suffix_ascending".to_string(),
            description: Some("Same hash prefix, different suffix ascending".to_string()),
            ascending: true,
            blocks: input,
            expected_order: hashes_to_string(&blocks),
        });
    }

    let test_file = SortingTestFile {
        algorithm: "BlockDAG_Sorting".to_string(),
        test_vectors: vectors,
    };

    let yaml = serde_yaml::to_string(&test_file).unwrap();
    println!("{}", yaml);

    let mut file = File::create("blockdag_sorting.yaml").unwrap();
    file.write_all(yaml.as_bytes()).unwrap();
    eprintln!("Written to blockdag_sorting.yaml");
}
