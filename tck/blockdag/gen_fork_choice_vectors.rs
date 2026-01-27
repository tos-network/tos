// Generate BlockDAG Fork Choice test vectors
// Run: cd ~/tos/tck/blockdag && cargo run --release --bin gen_fork_choice_vectors
//
// Fork choice rule:
// - Select tip with highest cumulative difficulty
// - Tiebreaker: higher hash wins (for determinism)

use serde::Serialize;
use std::fs::File;
use std::io::Write;

struct Tip {
    hash: String,
    cumulative_diff: u64,
}

fn find_best_tip(tips: &[Tip]) -> String {
    if tips.is_empty() {
        return String::new();
    }

    tips.iter()
        .max_by(|a, b| {
            // Primary: compare by cumulative difficulty (higher wins)
            match a.cumulative_diff.cmp(&b.cumulative_diff) {
                std::cmp::Ordering::Equal => {
                    // Tiebreaker: higher hash wins
                    a.hash.cmp(&b.hash)
                }
                other => other,
            }
        })
        .map(|t| t.hash.clone())
        .unwrap_or_default()
}

#[derive(Serialize)]
struct TestVector {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    tips: String,            // "hash:diff,hash:diff,..." format
    expected_best_tip: String,
}

#[derive(Serialize)]
struct ForkChoiceTestFile {
    algorithm: String,
    test_vectors: Vec<TestVector>,
}

fn tips_to_string(tips: &[Tip]) -> String {
    tips.iter()
        .map(|t| format!("{}:{}", t.hash, t.cumulative_diff))
        .collect::<Vec<_>>()
        .join(",")
}

fn main() {
    let mut vectors = Vec::new();

    // Test 1: Single tip
    {
        let tips = vec![
            Tip { hash: "aa".to_string(), cumulative_diff: 1000000 },
        ];
        vectors.push(TestVector {
            name: "single_tip".to_string(),
            description: Some("Single tip - trivial choice".to_string()),
            tips: tips_to_string(&tips),
            expected_best_tip: find_best_tip(&tips),
        });
    }

    // Test 2: Two tips different difficulty
    {
        let tips = vec![
            Tip { hash: "aa".to_string(), cumulative_diff: 1000000 },
            Tip { hash: "bb".to_string(), cumulative_diff: 2000000 },
        ];
        vectors.push(TestVector {
            name: "two_tips_different_difficulty".to_string(),
            description: Some("Two tips with different difficulties".to_string()),
            tips: tips_to_string(&tips),
            expected_best_tip: find_best_tip(&tips),
        });
    }

    // Test 3: Two tips same difficulty - higher hash wins
    {
        let tips = vec![
            Tip { hash: "aa".to_string(), cumulative_diff: 1000000 },
            Tip { hash: "bb".to_string(), cumulative_diff: 1000000 },
        ];
        vectors.push(TestVector {
            name: "two_tips_same_difficulty_higher_hash_wins".to_string(),
            description: Some("Two tips with same difficulty - higher hash wins".to_string()),
            tips: tips_to_string(&tips),
            expected_best_tip: find_best_tip(&tips),
        });
    }

    // Test 4: Three tips clear winner
    {
        let tips = vec![
            Tip { hash: "aa".to_string(), cumulative_diff: 1000000 },
            Tip { hash: "bb".to_string(), cumulative_diff: 3000000 },
            Tip { hash: "cc".to_string(), cumulative_diff: 2000000 },
        ];
        vectors.push(TestVector {
            name: "three_tips_clear_winner".to_string(),
            description: Some("Three tips with clear difficulty winner".to_string()),
            tips: tips_to_string(&tips),
            expected_best_tip: find_best_tip(&tips),
        });
    }

    // Test 5: Three tips, two tied - higher hash wins
    {
        let tips = vec![
            Tip { hash: "aa".to_string(), cumulative_diff: 3000000 },
            Tip { hash: "cc".to_string(), cumulative_diff: 3000000 },
            Tip { hash: "bb".to_string(), cumulative_diff: 2000000 },
        ];
        vectors.push(TestVector {
            name: "three_tips_two_tied_higher_hash_wins".to_string(),
            description: Some("Three tips, two tied at highest - higher hash wins".to_string()),
            tips: tips_to_string(&tips),
            expected_best_tip: find_best_tip(&tips),
        });
    }

    // Test 6: Three tips all tied
    {
        let tips = vec![
            Tip { hash: "bb".to_string(), cumulative_diff: 5000000 },
            Tip { hash: "aa".to_string(), cumulative_diff: 5000000 },
            Tip { hash: "cc".to_string(), cumulative_diff: 5000000 },
        ];
        vectors.push(TestVector {
            name: "three_tips_all_tied".to_string(),
            description: Some("All three tips tied - highest hash wins".to_string()),
            tips: tips_to_string(&tips),
            expected_best_tip: find_best_tip(&tips),
        });
    }

    // Test 7: Extreme hash values
    {
        let tips = vec![
            Tip { hash: "01".to_string(), cumulative_diff: 1000000 },
            Tip { hash: "ff".to_string(), cumulative_diff: 1000000 },
        ];
        vectors.push(TestVector {
            name: "extreme_hash_values".to_string(),
            description: Some("Extreme hash values with same difficulty".to_string()),
            tips: tips_to_string(&tips),
            expected_best_tip: find_best_tip(&tips),
        });
    }

    // Test 8: Large difficulty values
    {
        let tips = vec![
            Tip { hash: "aa".to_string(), cumulative_diff: 10000000000000000 },
            Tip { hash: "bb".to_string(), cumulative_diff: 10000000000000001 },
        ];
        vectors.push(TestVector {
            name: "large_difficulty_values".to_string(),
            description: Some("Large difficulty values".to_string()),
            tips: tips_to_string(&tips),
            expected_best_tip: find_best_tip(&tips),
        });
    }

    // Test 9: Winner not first
    {
        let tips = vec![
            Tip { hash: "aa".to_string(), cumulative_diff: 1000000 },
            Tip { hash: "bb".to_string(), cumulative_diff: 500000 },
            Tip { hash: "cc".to_string(), cumulative_diff: 5000000 },
        ];
        vectors.push(TestVector {
            name: "winner_not_first".to_string(),
            description: Some("Best tip is not first in list".to_string()),
            tips: tips_to_string(&tips),
            expected_best_tip: find_best_tip(&tips),
        });
    }

    // Test 10: Winner in middle
    {
        let tips = vec![
            Tip { hash: "aa".to_string(), cumulative_diff: 1000000 },
            Tip { hash: "dd".to_string(), cumulative_diff: 9000000 },
            Tip { hash: "cc".to_string(), cumulative_diff: 5000000 },
        ];
        vectors.push(TestVector {
            name: "winner_in_middle".to_string(),
            description: Some("Best tip is in middle of list".to_string()),
            tips: tips_to_string(&tips),
            expected_best_tip: find_best_tip(&tips),
        });
    }

    // Test 11: Hash tiebreaker with numeric prefix (0-9 vs a-f)
    {
        let tips = vec![
            Tip { hash: "9f".to_string(), cumulative_diff: 1000000 },
            Tip { hash: "a0".to_string(), cumulative_diff: 1000000 },
        ];
        vectors.push(TestVector {
            name: "hash_tiebreaker_numeric_vs_alpha".to_string(),
            description: Some("Hash tiebreaker: '9f' vs 'a0' with same difficulty".to_string()),
            tips: tips_to_string(&tips),
            expected_best_tip: find_best_tip(&tips),
        });
    }

    // Test 12: Minimum difficulty (1)
    {
        let tips = vec![
            Tip { hash: "aa".to_string(), cumulative_diff: 1 },
            Tip { hash: "bb".to_string(), cumulative_diff: 2 },
        ];
        vectors.push(TestVector {
            name: "minimum_difficulty".to_string(),
            description: Some("Minimum difficulty values".to_string()),
            tips: tips_to_string(&tips),
            expected_best_tip: find_best_tip(&tips),
        });
    }

    // Test 13: Zero difficulty (edge case)
    {
        let tips = vec![
            Tip { hash: "aa".to_string(), cumulative_diff: 0 },
            Tip { hash: "bb".to_string(), cumulative_diff: 1 },
        ];
        vectors.push(TestVector {
            name: "zero_difficulty".to_string(),
            description: Some("Zero vs non-zero difficulty".to_string()),
            tips: tips_to_string(&tips),
            expected_best_tip: find_best_tip(&tips),
        });
    }

    // Test 14: All zero difficulty (tiebreaker only)
    {
        let tips = vec![
            Tip { hash: "bb".to_string(), cumulative_diff: 0 },
            Tip { hash: "aa".to_string(), cumulative_diff: 0 },
            Tip { hash: "cc".to_string(), cumulative_diff: 0 },
        ];
        vectors.push(TestVector {
            name: "all_zero_difficulty".to_string(),
            description: Some("All zero difficulty - pure hash tiebreaker".to_string()),
            tips: tips_to_string(&tips),
            expected_best_tip: find_best_tip(&tips),
        });
    }

    // Test 15: Consecutive difficulty values
    {
        let tips = vec![
            Tip { hash: "aa".to_string(), cumulative_diff: 100 },
            Tip { hash: "bb".to_string(), cumulative_diff: 101 },
            Tip { hash: "cc".to_string(), cumulative_diff: 102 },
        ];
        vectors.push(TestVector {
            name: "consecutive_difficulty".to_string(),
            description: Some("Consecutive difficulty values".to_string()),
            tips: tips_to_string(&tips),
            expected_best_tip: find_best_tip(&tips),
        });
    }

    // Test 16: Lower hash wins when difficulty is lower
    {
        let tips = vec![
            Tip { hash: "ff".to_string(), cumulative_diff: 1000 },
            Tip { hash: "aa".to_string(), cumulative_diff: 2000 },
        ];
        vectors.push(TestVector {
            name: "lower_hash_higher_diff_wins".to_string(),
            description: Some("Lower hash with higher difficulty wins".to_string()),
            tips: tips_to_string(&tips),
            expected_best_tip: find_best_tip(&tips),
        });
    }

    // Test 17: Reverse alphabetical order with same diff
    {
        let tips = vec![
            Tip { hash: "dd".to_string(), cumulative_diff: 5000000 },
            Tip { hash: "cc".to_string(), cumulative_diff: 5000000 },
            Tip { hash: "bb".to_string(), cumulative_diff: 5000000 },
            Tip { hash: "aa".to_string(), cumulative_diff: 5000000 },
        ];
        vectors.push(TestVector {
            name: "reverse_alphabetical_same_diff".to_string(),
            description: Some("Reverse alphabetical order with same difficulty".to_string()),
            tips: tips_to_string(&tips),
            expected_best_tip: find_best_tip(&tips),
        });
    }

    // Test 18: Near-max u64 values
    {
        let tips = vec![
            Tip { hash: "aa".to_string(), cumulative_diff: u64::MAX - 1 },
            Tip { hash: "bb".to_string(), cumulative_diff: u64::MAX },
        ];
        vectors.push(TestVector {
            name: "near_max_u64".to_string(),
            description: Some("Near maximum u64 cumulative difficulty".to_string()),
            tips: tips_to_string(&tips),
            expected_best_tip: find_best_tip(&tips),
        });
    }

    // Test 19: Difficulty difference of 1 at high values
    {
        let tips = vec![
            Tip { hash: "ff".to_string(), cumulative_diff: 9999999999999999 },
            Tip { hash: "aa".to_string(), cumulative_diff: 10000000000000000 },
        ];
        vectors.push(TestVector {
            name: "diff_of_one_high_values".to_string(),
            description: Some("Difficulty difference of 1 at high values".to_string()),
            tips: tips_to_string(&tips),
            expected_best_tip: find_best_tip(&tips),
        });
    }

    // Test 20: Mixed hex digits in hash
    {
        let tips = vec![
            Tip { hash: "0a".to_string(), cumulative_diff: 1000 },
            Tip { hash: "a0".to_string(), cumulative_diff: 1000 },
            Tip { hash: "1b".to_string(), cumulative_diff: 1000 },
        ];
        vectors.push(TestVector {
            name: "mixed_hex_digits".to_string(),
            description: Some("Mixed hex digits with same difficulty".to_string()),
            tips: tips_to_string(&tips),
            expected_best_tip: find_best_tip(&tips),
        });
    }

    let test_file = ForkChoiceTestFile {
        algorithm: "BlockDAG_Fork_Choice".to_string(),
        test_vectors: vectors,
    };

    let yaml = serde_yaml::to_string(&test_file).unwrap();
    println!("{}", yaml);

    let mut file = File::create("blockdag_fork_choice.yaml").unwrap();
    file.write_all(yaml.as_bytes()).unwrap();
    eprintln!("Written to blockdag_fork_choice.yaml");
}
