/// Direction for iteration
#[derive(Copy, Clone, Debug)]
pub enum Direction {
    Forward,
    Reverse,
}

impl From<Direction> for rocksdb::Direction {
    fn from(direction: Direction) -> Self {
        match direction {
            Direction::Forward => rocksdb::Direction::Forward,
            Direction::Reverse => rocksdb::Direction::Reverse,
        }
    }
}

/// Iterator mode for snapshot and storage iteration
#[derive(Copy, Clone, Debug)]
pub enum IteratorMode<'a> {
    /// Start from the beginning
    Start,
    /// Start from the end
    End,
    /// Start from a specific key
    From(&'a [u8], Direction),
    /// Iterate over keys with a specific prefix
    WithPrefix(&'a [u8], Direction),
    /// Iterate over a range of keys (upper bound is NOT included)
    Range {
        lower_bound: &'a [u8],
        upper_bound: &'a [u8],
        direction: Direction,
    },
}
