// TOS Reachability Interval
// Based on Kaspa's interval.rs
// Reference: rusty-kaspa/consensus/src/processes/reachability/interval.rs

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
}
