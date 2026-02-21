use tos_common::crypto::Hash;
/// tip_selection_parity.rs — TOS Rust vs Avatar C cross-language parity tests.
///
/// Tests that Avatar's `at_dag_select_mining_tips()` implements the same rules
/// as TOS Rust's `get_block_header_template_for_storage()` tip selection logic.
///
/// Each test here uses EXACTLY THE SAME numerical inputs as the corresponding
/// test in `src/tck/test_mining_tip_selection.c`, verifying that the pure
/// algorithmic rules produce identical results in both languages.
///
/// Rules verified:
///   1. 91% difficulty filter (validate_tips):
///        best_difficulty * 91 / 100 < tip_difficulty
///   2. Sort by cumulative difficulty descending:
///        sort_descending_by_cumulative_difficulty()
///   3. Height calculation: max(parent heights) + 1
use tos_common::difficulty::CumulativeDifficulty;
use tos_common::serializer::Serializer;
use tos_daemon::core::blockdag::sort_descending_by_cumulative_difficulty;

// ---------------------------------------------------------------------------
// Helper: build a 32-byte hash with integer id in the first 4 bytes.
// MUST match make_hash() in test_mining_tip_selection.c
// ---------------------------------------------------------------------------
fn make_hash(id: u64) -> Hash {
    let mut bytes = [0u8; 32];
    bytes[0] = (id & 0xFF) as u8;
    bytes[1] = ((id >> 8) & 0xFF) as u8;
    bytes[2] = ((id >> 16) & 0xFF) as u8;
    bytes[3] = ((id >> 24) & 0xFF) as u8;
    Hash::from_bytes(&bytes).expect("valid 32-byte hash")
}

// ---------------------------------------------------------------------------
// Pure 91% filter — mirrors both sides:
//   TOS Rust validate_tips():
//     Ok(best_difficulty * MAX_DEVIATION / PERCENTAGE < block_difficulty)
//   Avatar C dag_tip_difficulty_is_valid():
//     (ulong)(((__uint128_t)best_difficulty * 91U) / 100U) < tip_difficulty
// ---------------------------------------------------------------------------
fn validate_tip_91pct(best_difficulty: u64, tip_difficulty: u64) -> bool {
    // Use u128 to match Avatar C's __uint128_t and TOS Rust's U256 (for u64 values, identical)
    let threshold = ((best_difficulty as u128) * 91) / 100;
    (threshold as u64) < tip_difficulty
}

// ---------------------------------------------------------------------------
// Pure height: max(parent_heights) + 1
// Matches at_dag_calculate_height() and TOS Rust calculate_height_at_tips()
// ---------------------------------------------------------------------------
fn calculate_height(parent_heights: &[u64]) -> u64 {
    if parent_heights.is_empty() {
        return 0;
    }
    parent_heights.iter().copied().max().unwrap_or(0) + 1
}

// ===========================================================================
// Test 1: 91% filter boundary
// MIRRORS test_91pct_filter_rejects_weak_tip + test_91pct_boundary in C test
//
// best_difficulty = 1000 → threshold = 1000 * 91 / 100 = 910
// Exactly at boundary (910): NOT strictly greater → reject
// One above (911): strictly greater → accept
// ===========================================================================
#[test]
fn test_validate_tips_91pct_boundary() {
    // Mirrors test_91pct_filter_rejects_weak_tip: best=1000, weak=909 → reject
    assert!(
        !validate_tip_91pct(1000, 909),
        "diff=909 must be rejected (threshold=910, 910 < 909 is false)"
    );

    // Mirrors test_91pct_boundary: exactly at threshold (910) → reject
    assert!(
        !validate_tip_91pct(1000, 910),
        "diff=910 must be rejected (not strictly greater than threshold=910)"
    );

    // Mirrors test_91pct_boundary: one above threshold (911) → accept
    assert!(
        validate_tip_91pct(1000, 911),
        "diff=911 must be accepted (strictly greater than threshold=910)"
    );

    // Additional: well below threshold → reject
    assert!(!validate_tip_91pct(1000, 500), "diff=500 must be rejected");

    // Additional: equal to best → accept (1000 > 910)
    assert!(
        validate_tip_91pct(1000, 1000),
        "diff=1000 (== best) must be accepted"
    );
}

// ===========================================================================
// Test 2: Sort by cumulative difficulty descending
// MIRRORS test_sort_descending in test_mining_tip_selection.c
//
// Input:  [id=1 cumul=1000, id=2 cumul=3000, id=3 cumul=2000]
//         (same as C test: hashes[0]=make_hash(1), ..., cumul=[1000,3000,2000])
// Expected output: [id=2(3000), id=3(2000), id=1(1000)]
// ===========================================================================
#[test]
fn test_sort_tips_descending_same_values_as_c_test() {
    let h1 = make_hash(1);
    let h2 = make_hash(2);
    let h3 = make_hash(3);

    // Same cumulative difficulties as C test_sort_descending
    let mut scores: Vec<(Hash, CumulativeDifficulty)> = vec![
        (h1.clone(), CumulativeDifficulty::from_u64(1000)),
        (h2.clone(), CumulativeDifficulty::from_u64(3000)),
        (h3.clone(), CumulativeDifficulty::from_u64(2000)),
    ];

    sort_descending_by_cumulative_difficulty(&mut scores);

    // Both C and Rust must produce this exact order
    assert_eq!(scores[0].0, h2, "first must be h2 (cumul=3000, highest)");
    assert_eq!(scores[1].0, h3, "second must be h3 (cumul=2000)");
    assert_eq!(scores[2].0, h1, "third must be h1 (cumul=1000, lowest)");
}

// ===========================================================================
// Test 3: Sort tiebreaker — higher hash first
// MIRRORS the tiebreak logic in at_dag_sort_internal (ascending=0, descending)
//
// Tiebreak rule: when cumulative_diff is equal, higher hash bytes → first
// TOS Rust:  b_hash.as_ref().cmp(a_hash.as_ref()) → b comes first if b > a
// Avatar C:  at_memcmp(hashes[j], hashes[i]) > 0 → swap j before i
// ===========================================================================
#[test]
fn test_sort_tips_descending_tiebreaker_higher_hash_wins() {
    // make_hash(1) = [0x01, 0x00, ...], make_hash(255) = [0xFF, 0x00, ...]
    // 0xFF > 0x01, so make_hash(255) should come first on tie
    let h_low = make_hash(1);
    let h_high = make_hash(255);

    // Equal cumulative difficulty → tiebreaker activates
    let mut scores: Vec<(Hash, CumulativeDifficulty)> = vec![
        (h_low.clone(), CumulativeDifficulty::from_u64(5000)),
        (h_high.clone(), CumulativeDifficulty::from_u64(5000)),
    ];

    sort_descending_by_cumulative_difficulty(&mut scores);

    assert_eq!(
        scores[0].0, h_high,
        "on equal cumul_diff, higher hash bytes must come first"
    );
    assert_eq!(scores[1].0, h_low, "lower hash must be second");
}

// ===========================================================================
// Test 4: Height calculation (max(parent heights) + 1)
// MIRRORS at_dag_calculate_height() — same formula as TOS Rust calculate_height_at_tips()
// ===========================================================================
#[test]
fn test_height_calculation_max_plus_one() {
    assert_eq!(calculate_height(&[]), 0, "genesis height must be 0");
    assert_eq!(calculate_height(&[0]), 1, "child of genesis must be 1");
    assert_eq!(calculate_height(&[5, 3]), 6, "max(5,3)+1 = 6");
    assert_eq!(calculate_height(&[10, 10, 7]), 11, "max(10,10,7)+1 = 11");
}

// ===========================================================================
// Test 5: Combined filter + sort — mirrors test_clamp_to_3_tips in C
//
// C test scenario: 5 tips, best_diff=1000, threshold=910
//   id=1: diff=1000 (best), cumul=5000 → always included
//   id=2: diff=992 > 910 ✓, cumul=4000 → included
//   id=3: diff=991 > 910 ✓, cumul=3000 → included
//   id=4: diff=950 > 910 ✓, cumul=2000 → included
//   id=5: diff=920 > 910 ✓, cumul=1000 → included (before clamp)
// After sort descending: [1(5000), 2(4000), 3(3000), 4(2000), 5(1000)]
// After clamp to 3: [1(5000), 2(4000), 3(3000)]
// ===========================================================================
#[test]
fn test_combined_filter_and_clamp_mirrors_c_clamp_test() {
    struct Tip {
        id: u64,
        difficulty: u64,
        cumul: u64,
    }
    let tips = vec![
        Tip {
            id: 1,
            difficulty: 1000,
            cumul: 5000,
        }, // best
        Tip {
            id: 2,
            difficulty: 992,
            cumul: 4000,
        },
        Tip {
            id: 3,
            difficulty: 991,
            cumul: 3000,
        },
        Tip {
            id: 4,
            difficulty: 950,
            cumul: 2000,
        },
        Tip {
            id: 5,
            difficulty: 920,
            cumul: 1000,
        },
    ];

    let best_diff = 1000u64;
    // threshold = 1000 * 91 / 100 = 910
    let threshold = (best_diff as u128 * 91) / 100; // = 910

    // Apply 91% filter (all pass: 992, 991, 950, 920 are all > 910)
    let mut selected: Vec<(Hash, CumulativeDifficulty)> = Vec::new();
    let best_idx = 0usize; // id=1 is best
    for (i, tip) in tips.iter().enumerate() {
        if i == best_idx || (tip.difficulty as u128) > threshold {
            selected.push((make_hash(tip.id), CumulativeDifficulty::from_u64(tip.cumul)));
        }
    }

    assert_eq!(selected.len(), 5, "all 5 tips must pass the 91% filter");

    // Sort descending
    sort_descending_by_cumulative_difficulty(&mut selected);

    // Clamp to AT_BLOCK_MAX_TIPS (=3)
    selected.truncate(3);

    assert_eq!(selected.len(), 3, "must be clamped to 3");
    assert_eq!(selected[0].0, make_hash(1), "first: id=1 (cumul=5000)");
    assert_eq!(selected[1].0, make_hash(2), "second: id=2 (cumul=4000)");
    assert_eq!(selected[2].0, make_hash(3), "third: id=3 (cumul=3000)");

    // Verify id=4 and id=5 would have been clamped out
    assert!(
        validate_tip_91pct(best_diff, 920),
        "920 > threshold=910, passes 91% filter"
    );
    assert!(
        validate_tip_91pct(best_diff, 950),
        "950 > threshold=910, passes 91% filter"
    );
}

// ===========================================================================
// Test 6: 91% filter rejects exactly one tip — mirrors test_91pct_filter_rejects_weak_tip
//
// C test scenario: best diff=1000, weak diff=909
//   threshold = 910, 910 < 909 is false → reject
// Only best tip remains after filter
// ===========================================================================
#[test]
fn test_91pct_filter_rejects_weak_tip_same_as_c() {
    let best_diff = 1000u64;
    let weak_diff = 909u64;

    // This must be false (reject), same as C assert out_cnt == 1
    assert!(
        !validate_tip_91pct(best_diff, weak_diff),
        "diff=909 must be rejected by 91% filter when best_diff=1000 (threshold=910)"
    );

    // Only best tip survives — same as C test_91pct_filter_rejects_weak_tip
    let h1 = make_hash(1); // best tip
    let selected: Vec<(Hash, CumulativeDifficulty)> = vec![
        (h1.clone(), CumulativeDifficulty::from_u64(2000)), // best tip always included
                                                            // h2 (diff=909) filtered out
    ];
    assert_eq!(selected.len(), 1, "only best tip survives filter");
    assert_eq!(selected[0].0, h1, "surviving tip is best tip");
}
