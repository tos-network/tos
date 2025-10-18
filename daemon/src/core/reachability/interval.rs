// TOS Reachability Interval

use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

/// Interval representing a block's position in the reachability tree
///
/// An interval [start, end] is used to efficiently check chain ancestry:
/// Block A is a chain ancestor of Block B if A.interval.contains(B.interval)
///
/// The interval space is u64, with bounds [1, u64::MAX-1] to allow for
/// empty interval representation (end = start - 1).
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Serialize, Deserialize)]
pub struct Interval {
    pub start: u64,
    pub end: u64,
}

impl Display for Interval {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        write!(f, "[{}, {}]", self.start, self.end)
    }
}

impl Interval {
    /// Create a new interval
    ///
    /// # Invariants
    /// - start > 0 (to allow empty intervals with end = start - 1)
    /// - end < u64::MAX (to allow expansion)
    /// - end >= start - 1 (allow empty intervals)
    pub fn new(start: u64, end: u64) -> Self {
        debug_assert!(start > 0, "start must be > 0");
        debug_assert!(end < u64::MAX, "end must be < u64::MAX");
        debug_assert!(end >= start - 1, "end must be >= start - 1");
        Interval { start, end }
    }

    /// Create an empty interval
    pub fn empty() -> Self {
        Self::new(1, 0)
    }

    /// Create the maximal interval [1, u64::MAX-1]
    ///
    /// We leave a margin of 1 from both u64 bounds (0 and u64::MAX)
    /// to support reducing any interval to empty by setting end = start - 1
    pub fn maximal() -> Self {
        Self::new(1, u64::MAX - 1)
    }

    /// Get the size of the interval
    ///
    /// Empty intervals (end = start - 1) return 0.
    /// Will panic if end < start - 1 (invalid interval)
    pub fn size(&self) -> u64 {
        // Add 1 first to avoid overflow for empty intervals
        (self.end + 1) - self.start
    }

    /// Check if interval is empty
    pub fn is_empty(&self) -> bool {
        self.size() == 0
    }

    /// Check if this interval contains another interval
    ///
    /// Used for chain ancestry: A contains B means A is ancestor of B
    pub fn contains(&self, other: Interval) -> bool {
        self.start <= other.start && other.end <= self.end
    }

    /// Split this interval in half
    ///
    /// Returns (left_half, right_half) where:
    /// - union(left, right) = self
    /// - left.size ≈ right.size (within 1)
    pub fn split_half(&self) -> (Self, Self) {
        let size = self.size();
        let left_size = (size + 1) / 2; // Round up for left

        let left = Self::new(self.start, self.start + left_size - 1);
        let right = Self::new(self.start + left_size, self.end);

        (left, right)
    }

    /// Get the remaining interval after this one
    ///
    /// Used for allocating intervals to children:
    /// If parent has interval [1, 1000], and this child has [1, 500],
    /// then remaining is [501, 1000]
    ///
    /// Note: This assumes `self` is a sub-interval of some parent interval
    pub fn remaining_after(&self) -> Interval {
        if self.end >= u64::MAX - 1 {
            // No space remaining
            return Interval::empty();
        }

        Interval::new(self.end + 1, u64::MAX - 1)
    }

    /// Increase both start and end by offset
    pub fn increase(&self, offset: u64) -> Self {
        Self::new(self.start + offset, self.end + offset)
    }

    /// Decrease both start and end by offset
    pub fn decrease(&self, offset: u64) -> Self {
        Self::new(self.start - offset, self.end - offset)
    }

    /// Increase start by offset (shrink from left)
    pub fn increase_start(&self, offset: u64) -> Self {
        Self::new(self.start + offset, self.end)
    }

    /// Decrease start by offset (expand to left)
    pub fn decrease_start(&self, offset: u64) -> Self {
        Self::new(self.start - offset, self.end)
    }

    /// Increase end by offset (expand to right)
    pub fn increase_end(&self, offset: u64) -> Self {
        Self::new(self.start, self.end + offset)
    }

    /// Decrease end by offset (shrink from right)
    pub fn decrease_end(&self, offset: u64) -> Self {
        Self::new(self.start, self.end - offset)
    }

    /// Check if this interval strictly contains another interval
    ///
    /// Strict containment means self contains other AND they are not equal
    /// Used for parent-child relationship: parent must strictly contain child
    pub fn strictly_contains(&self, other: Interval) -> bool {
        self.contains(other) && *self != other
    }

    /// Split interval into exact sizes
    ///
    /// Splits this interval into sub-intervals with the given sizes.
    /// The sum of sizes must equal the interval size.
    ///
    /// # Arguments
    /// * `sizes` - Array of sizes for each sub-interval
    ///
    /// # Returns
    /// Vector of intervals with exact sizes
    ///
    /// # Panics
    /// Panics if sum(sizes) != self.size()
    pub fn split_exact(&self, sizes: &[u64]) -> Vec<Self> {
        let total_size: u64 = sizes.iter().sum();
        assert_eq!(
            total_size,
            self.size(),
            "sum of sizes ({}) must equal interval size ({})",
            total_size,
            self.size()
        );

        let mut result = Vec::with_capacity(sizes.len());
        let mut current_start = self.start;

        for &size in sizes {
            if size == 0 {
                // Empty interval
                result.push(Self::new(current_start, current_start - 1));
            } else {
                let current_end = current_start + size - 1;
                result.push(Self::new(current_start, current_end));
                current_start = current_end + 1;
            }
        }

        result
    }

    /// Split interval exponentially using subtree sizes
    ///
    /// This is the CRITICAL ALGORITHM for reindexing. It allocates interval space
    /// to children proportionally to 2^(subtree_size), giving larger subtrees
    /// exponentially more space since they're more likely to grow.
    ///
    /// # Algorithm
    /// 1. Each child gets AT LEAST its subtree size
    /// 2. Remaining space (slack) is distributed exponentially:
    ///    fraction[i] = 2^(size[i]) / Σ(2^(size[j]))
    /// 3. Larger subtrees get proportionally more slack
    ///
    /// # Arguments
    /// * `sizes` - Subtree sizes for each child
    ///
    /// # Returns
    /// Vector of intervals allocated to children
    ///
    /// # Panics
    /// Panics if self.size() < sum(sizes)
    pub fn split_exponential(&self, sizes: &[u64]) -> Vec<Self> {
        let interval_size = self.size();
        let sizes_sum: u64 = sizes.iter().sum();

        assert!(
            interval_size >= sizes_sum,
            "interval size ({}) must be >= sum of sizes ({})",
            interval_size,
            sizes_sum
        );

        // If exact match, no slack to distribute
        if interval_size == sizes_sum {
            return self.split_exact(sizes);
        }

        // Calculate exponential fractions for slack distribution using u128 scaled arithmetic
        let mut remaining_bias = interval_size - sizes_sum;
        let total_bias = remaining_bias as u128;
        let exp_fractions_scaled = exponential_fractions_scaled(sizes);

        // Add exponentially-biased slack to each size
        let mut biased_sizes = Vec::with_capacity(sizes.len());
        for (i, &fraction_scaled) in exp_fractions_scaled.iter().enumerate() {
            let bias: u64 = if i == exp_fractions_scaled.len() - 1 {
                // Last child gets all remaining bias (avoid rounding errors)
                remaining_bias
            } else {
                // Calculate: total_bias * fraction
                // fraction_scaled is already in range [0, SCALE], representing [0.0, 1.0]
                const SCALE: u128 = 10000;
                let bias_u128 = (total_bias * fraction_scaled) / SCALE;
                remaining_bias.min(bias_u128 as u64)
            };
            biased_sizes.push(sizes[i] + bias);
            remaining_bias = remaining_bias.saturating_sub(bias);
        }

        self.split_exact(&biased_sizes)
    }
}

/// Calculate exponential fractions for slack distribution using u128 scaled arithmetic
///
/// Returns fraction[i] = 2^(size[i]) / Σ(2^(size[j])), scaled by SCALE (10000)
///
/// This gives larger subtrees exponentially higher fractions,
/// following GHOSTDAG's principle that larger subtrees dominate growth.
///
/// # Algorithm
/// To avoid overflow with 2^size, we use:
/// 2^size[i] / Σ(2^size[j]) = 2^(size[i] - max_size) / Σ(2^(size[j] - max_size))
///
/// By subtracting max_size, all exponents become ≤ 0, preventing overflow.
///
/// Returns scaled fractions in range [0, SCALE] where SCALE = 10000 represents 1.0
fn exponential_fractions_scaled(sizes: &[u64]) -> Vec<u128> {
    const SCALE: u128 = 10000;

    if sizes.is_empty() {
        return Vec::new();
    }

    let max_size = sizes.iter().copied().max().unwrap_or(0);

    // Calculate 2^(size[i] - max_size) for each size, scaled by SCALE
    // Since size[i] - max_size ≤ 0, these are all ≤ 1, avoiding overflow
    let exp_values: Vec<u128> = sizes
        .iter()
        .map(|&s| {
            if max_size >= s {
                let diff = max_size - s;
                if diff >= 64 {
                    // 2^(-64) ≈ 0, avoid overflow in shift
                    0
                } else {
                    // Calculate: SCALE / 2^diff
                    SCALE >> diff
                }
            } else {
                // This should never happen since max_size is the maximum
                let diff = s - max_size;
                if diff >= 64 {
                    // Overflow protection
                    u128::MAX
                } else {
                    SCALE << diff
                }
            }
        })
        .collect();

    // Calculate sum of all exponential values
    let exp_sum: u128 = exp_values.iter().sum();

    // Normalize to sum to SCALE (representing fractions that sum to 1.0)
    let mut fractions_scaled: Vec<u128> = Vec::with_capacity(exp_values.len());
    if exp_sum > 0 {
        for &exp_val in &exp_values {
            // Calculate: (exp_val * SCALE) / exp_sum
            let fraction_scaled = (exp_val * SCALE) / exp_sum;
            fractions_scaled.push(fraction_scaled);
        }
    } else {
        // All zeros, return equal fractions
        let equal_fraction = SCALE / (sizes.len() as u128);
        fractions_scaled = vec![equal_fraction; sizes.len()];
    }

    fractions_scaled
}

/// Legacy f64-based exponential fractions calculation (deprecated)
///
/// This function is kept for reference but should not be used in consensus-critical code.
/// Use `exponential_fractions_scaled` instead for deterministic results.
#[allow(dead_code)]
fn exponential_fractions(sizes: &[u64]) -> Vec<f64> {
    if sizes.is_empty() {
        return Vec::new();
    }

    let max_size = sizes.iter().copied().max().unwrap_or(0);

    // Calculate 2^(size[i] - max_size) for each size
    // Since size[i] - max_size ≤ 0, these are all ≤ 1, avoiding overflow
    let mut fractions: Vec<f64> = sizes
        .iter()
        .map(|&s| {
            if max_size >= s {
                1f64 / 2f64.powi((max_size - s) as i32)
            } else {
                2f64.powi((s - max_size) as i32)
            }
        })
        .collect();

    // Normalize to sum to 1
    let fractions_sum: f64 = fractions.iter().sum();
    if fractions_sum > 0.0 {
        for fraction in &mut fractions {
            *fraction /= fractions_sum;
        }
    }

    fractions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interval_basics() {
        let interval = Interval::new(101, 164);
        assert_eq!(interval.size(), 64);
        assert!(!interval.is_empty());

        let increased = interval.increase(10);
        assert_eq!(increased.start, 111);
        assert_eq!(increased.end, 174);

        let decreased = increased.decrease(5);
        assert_eq!(decreased.start, 106);
        assert_eq!(decreased.end, 169);
    }

    #[test]
    fn test_empty_interval() {
        let empty = Interval::empty();
        assert_eq!(empty.size(), 0);
        assert!(empty.is_empty());

        let (left, right) = empty.split_half();
        assert!(left.is_empty());
        assert!(right.is_empty());
    }

    #[test]
    fn test_maximal_interval() {
        let maximal = Interval::maximal();
        assert_eq!(maximal.start, 1);
        assert_eq!(maximal.end, u64::MAX - 1);
        assert_eq!(maximal.size(), u64::MAX - 1);
    }

    #[test]
    fn test_contains() {
        let parent = Interval::new(1, 100);
        let child = Interval::new(10, 50);
        let sibling = Interval::new(51, 100);
        let outside = Interval::new(101, 200);

        assert!(parent.contains(parent)); // Self-containment
        assert!(parent.contains(child));
        assert!(parent.contains(sibling));
        assert!(!parent.contains(outside));
        assert!(!child.contains(parent));
        assert!(!child.contains(sibling));
    }

    #[test]
    fn test_split_half() {
        let interval = Interval::new(1, 100);
        let (left, right) = interval.split_half();

        assert_eq!(left.start, 1);
        assert_eq!(left.end, 50);
        assert_eq!(right.start, 51);
        assert_eq!(right.end, 100);

        assert!(interval.contains(left));
        assert!(interval.contains(right));
        assert!(!left.contains(right));
        assert!(!right.contains(left));
    }

    #[test]
    fn test_remaining_after() {
        let child = Interval::new(1, 500);
        let remaining = child.remaining_after();

        assert_eq!(remaining.start, 501);
        assert_eq!(remaining.end, u64::MAX - 1);

        // Edge case: no remaining space
        let maximal = Interval::maximal();
        let no_remaining = maximal.remaining_after();
        assert!(no_remaining.is_empty());
    }

    #[test]
    fn test_interval_manipulation() {
        let interval = Interval::new(100, 200);

        // Test increase_start (shrink from left)
        let shrunk_left = interval.increase_start(10);
        assert_eq!(shrunk_left.start, 110);
        assert_eq!(shrunk_left.end, 200);
        assert_eq!(shrunk_left.size(), 91);

        // Test decrease_end (shrink from right)
        let shrunk_right = interval.decrease_end(10);
        assert_eq!(shrunk_right.start, 100);
        assert_eq!(shrunk_right.end, 190);
        assert_eq!(shrunk_right.size(), 91);

        // Test increase_end (expand to right)
        let expanded_right = interval.increase_end(10);
        assert_eq!(expanded_right.start, 100);
        assert_eq!(expanded_right.end, 210);
        assert_eq!(expanded_right.size(), 111);

        // Test decrease_start (expand to left)
        let expanded_left = interval.decrease_start(10);
        assert_eq!(expanded_left.start, 90);
        assert_eq!(expanded_left.end, 200);
        assert_eq!(expanded_left.size(), 111);
    }

    #[test]
    fn test_strictly_contains() {
        let parent = Interval::new(1, 100);
        let child = Interval::new(10, 50);

        assert!(parent.strictly_contains(child));
        assert!(!parent.strictly_contains(parent)); // Not strict (equal)
        assert!(!child.strictly_contains(parent));
    }

    #[test]
    fn test_split_exact() {
        let interval = Interval::new(1, 100);
        let sizes = vec![20, 30, 50];

        let splits = interval.split_exact(&sizes);
        assert_eq!(splits.len(), 3);

        assert_eq!(splits[0], Interval::new(1, 20));
        assert_eq!(splits[1], Interval::new(21, 50));
        assert_eq!(splits[2], Interval::new(51, 100));

        // Verify sizes
        assert_eq!(splits[0].size(), 20);
        assert_eq!(splits[1].size(), 30);
        assert_eq!(splits[2].size(), 50);
    }

    #[test]
    fn test_split_exact_with_empty() {
        let interval = Interval::new(1, 50);
        let sizes = vec![20, 0, 30]; // Middle child gets empty interval

        let splits = interval.split_exact(&sizes);
        assert_eq!(splits.len(), 3);

        assert_eq!(splits[0].size(), 20);
        assert!(splits[1].is_empty());
        assert_eq!(splits[2].size(), 30);
    }

    #[test]
    #[should_panic(expected = "sum of sizes")]
    fn test_split_exact_wrong_sum() {
        let interval = Interval::new(1, 100);
        let sizes = vec![20, 30, 40]; // Sum = 90, not 100
        interval.split_exact(&sizes);
    }

    #[test]
    fn test_exponential_fractions() {
        let sizes = vec![10, 20, 40];
        let fractions = exponential_fractions(&sizes);

        assert_eq!(fractions.len(), 3);

        // Fractions should sum to ~1.0
        let sum: f64 = fractions.iter().sum();
        assert!((sum - 1.0).abs() < 0.0001);

        // Larger sizes should get larger fractions
        assert!(fractions[2] > fractions[1]);
        assert!(fractions[1] > fractions[0]);

        // For sizes [10, 20, 40]:
        // 2^10 = 1024, 2^20 = 1048576, 2^40 = 1099511627776
        // But we use 2^(size - max_size) to avoid overflow
        // So: 2^(10-40), 2^(20-40), 2^(40-40) = 2^-30, 2^-20, 2^0
        // fraction[0] ≈ 2^-30 / (2^-30 + 2^-20 + 1) ≈ 0
        // fraction[1] ≈ 2^-20 / (2^-30 + 2^-20 + 1) ≈ 0.001
        // fraction[2] ≈ 1 / (2^-30 + 2^-20 + 1) ≈ 0.999

        println!("Fractions: {:?}", fractions);
        assert!(fractions[2] > 0.99); // Largest subtree gets almost all slack
    }

    #[test]
    fn test_split_exponential() {
        let interval = Interval::new(1, 1000);
        let sizes = vec![10, 20, 40]; // Subtree sizes

        let splits = interval.split_exponential(&sizes);
        assert_eq!(splits.len(), 3);

        // Each child gets AT LEAST its subtree size
        assert!(splits[0].size() >= 10);
        assert!(splits[1].size() >= 20);
        assert!(splits[2].size() >= 40);

        // Larger subtree should get more space
        assert!(splits[2].size() > splits[1].size());
        assert!(splits[1].size() > splits[0].size());

        // Total should equal interval size
        let total_size: u64 = splits.iter().map(|s| s.size()).sum();
        assert_eq!(total_size, 1000);

        println!("Exponential splits: {:?}", splits);
        println!("Sizes: {:?}", splits.iter().map(|s| s.size()).collect::<Vec<_>>());
    }

    #[test]
    fn test_split_exponential_exact_match() {
        let interval = Interval::new(1, 70);
        let sizes = vec![10, 20, 40]; // Sum = 70, exact match

        let splits = interval.split_exponential(&sizes);

        // When exact match, should behave like split_exact
        assert_eq!(splits[0].size(), 10);
        assert_eq!(splits[1].size(), 20);
        assert_eq!(splits[2].size(), 40);
    }

    #[test]
    fn test_split_exponential_equal_sizes() {
        let interval = Interval::new(1, 1000);
        let sizes = vec![10, 10, 10]; // All equal

        let splits = interval.split_exponential(&sizes);

        // With equal sizes, slack should be distributed roughly equally
        // (within rounding error)
        let size_diff = splits[0].size().abs_diff(splits[1].size());
        assert!(size_diff <= 1);
        let size_diff = splits[1].size().abs_diff(splits[2].size());
        assert!(size_diff <= 1);
    }
}
