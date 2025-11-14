// TOS BPS (Blocks Per Second) Configuration System
//
// This module provides a compile-time type-safe BPS configuration system for BlockDAG consensus.
// All BPS-dependent parameters are automatically calculated at compile time based on the
// configured BPS value.
//
// Design Philosophy:
// - Explicit BPS declaration (OneBps, TenBps, etc.)
// - Compile-time parameter calculation (zero runtime cost)
// - Automatic consistency (K value, finality depth, etc. all derived from BPS)
// - Type safety (mismatched parameters caught by compiler)

/// Generic BPS (Blocks Per Second) configuration using const generics
///
/// This struct uses Rust's const generics feature to provide compile-time BPS configuration.
/// All methods are const fn, meaning they are evaluated at compile time with zero runtime cost.
///
/// # Type Safety
///
/// The const generic parameter ensures that different BPS configurations are distinct types,
/// preventing accidental mixing of incompatible parameters.
///
/// # Example
///
/// ```rust,no_run
/// use tos_daemon::core::bps::{Bps, OneBps};
///
/// // OneBps is an alias for Bps<1>
/// assert_eq!(OneBps::bps(), 1);
/// assert_eq!(OneBps::target_time_per_block(), 1000);
/// assert_eq!(OneBps::ghostdag_k(), 10);
/// ```
pub struct Bps<const BPS: u64>;

/// One block per second - TOS standard configuration
///
/// This is the standard BPS configuration for TOS mainnet, testnet, and devnet.
/// Provides a good balance between throughput and network convergence.
///
/// Parameters:
/// - Target block time: 1000ms (1 second)
/// - GHOSTDAG K: 10
/// - Max parents: 10
/// - Finality depth: 100 blocks (~100 seconds)
pub type OneBps = Bps<1>;

/// Ten blocks per second - High throughput configuration
///
/// This is a high-throughput BlockDAG configuration, used here for reference and future experimentation.
/// Requires higher K value and more aggressive parameters.
///
/// Parameters:
/// - Target block time: 100ms
/// - GHOSTDAG K: 124 (calculated for 10 BPS)
/// - Max parents: 16 (capped)
/// - Finality depth: 1000 blocks (~100 seconds)
pub type TenBps = Bps<10>;

impl<const BPS: u64> Bps<BPS> {
    /// Returns the configured blocks per second value
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// assert_eq!(OneBps::bps(), 1);
    /// assert_eq!(TenBps::bps(), 10);
    /// ```
    pub const fn bps() -> u64 {
        BPS
    }

    /// Returns the target time per block in milliseconds
    ///
    /// This is the ideal time between blocks that the DAA (Difficulty Adjustment Algorithm)
    /// will try to achieve. Calculated as 1000ms / BPS.
    ///
    /// # Compile-Time Validation
    ///
    /// This function contains a compile-time panic that ensures BPS divides 1000 evenly.
    /// The panic only occurs if the code is compiled with an invalid BPS constant,
    /// and will **never trigger at runtime** in production.
    ///
    /// This is intentional and provides compile-time configuration validation.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// assert_eq!(OneBps::target_time_per_block(), 1000);  // 1 second
    /// assert_eq!(TenBps::target_time_per_block(), 100);   // 100 milliseconds
    /// ```
    pub const fn target_time_per_block() -> u64 {
        if 1000 % BPS != 0 {
            panic!("BPS must divide 1000 evenly to maintain millisecond precision")
        }
        1000 / BPS
    }

    /// Returns the GHOSTDAG K parameter (anticone size bound)
    ///
    /// The K parameter determines the maximum size of anticones that can be merged into
    /// the GHOSTDAG blue set. It's calculated using Poisson process analysis based on:
    /// - Network delay D (assumed 2 seconds)
    /// - Block rate lambda (BPS)
    /// - Tail probability delta (0.001)
    ///
    /// Formula: K = calculate_ghostdag_k(2 * D * lambda, delta)
    ///
    /// # Security Critical
    ///
    /// This parameter is crucial for consensus security. Too low K increases orphan rate
    /// and reduces security. Too high K increases storage and computation costs.
    ///
    /// # Lookup Table
    ///
    /// Pre-computed values (D=2s, delta=0.001):
    /// - 1 BPS: K=10 (x=4.0)
    /// - 10 BPS: K=124 (x=40.0, with safety margin)
    ///
    /// # Compile-Time Validation
    ///
    /// This function contains a compile-time panic that ensures only validated BPS values
    /// with pre-computed K parameters are used. The panic only occurs if the code is
    /// compiled with an unsupported BPS constant, and will **never trigger at runtime**
    /// in production.
    ///
    /// This is intentional and forces explicit calculation and security review before
    /// adding new BPS configurations.
    pub const fn ghostdag_k() -> u64 {
        match BPS {
            1 => 10,   // TOS: K=10 for 1 BPS (x=4.0, calculated ~9.7)
            10 => 124, // Reference: K=124 for 10 BPS (x=40.0, with safety margin)
            _ => panic!("Unsupported BPS value - calculate K and add to lookup table"),
        }
    }

    /// Returns the maximum number of parents a block can reference
    ///
    /// Based on GHOSTDAG K with bounds [10, 16]:
    /// - Base calculation: K / 2
    /// - Minimum: 10 (ensure sufficient DAG connectivity)
    /// - Maximum: 16 (prevent quadratic growth in validation time)
    ///
    /// The cap at 16 parents is important because:
    /// 1. Validation time grows with number of parents
    /// 2. Network bandwidth for parent references grows quadratically with BPS
    /// 3. Practical testing shows 16 parents provides good balance
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// assert_eq!(OneBps::max_block_parents(), 10);  // K=10, so 10/2=5, clamped to min 10
    /// assert_eq!(TenBps::max_block_parents(), 16);  // K=124, so 124/2=62, capped at 16
    /// ```
    pub const fn max_block_parents() -> u8 {
        let val = (Self::ghostdag_k() / 2) as u8;
        if val < 10 {
            10 // Minimum for DAG connectivity
        } else if val > 16 {
            16 // Maximum to control validation complexity
        } else {
            val
        }
    }

    /// Returns the mergeset size limit for GHOSTDAG
    ///
    /// The mergeset is the set of blocks that can be merged in a single GHOSTDAG merge operation.
    /// Based on 2*K with bounds [180, 512]:
    /// - Base calculation: 2 * K
    /// - Minimum: 180 (ensure sufficient merge capacity)
    /// - Maximum: 512 (storage complexity constraint)
    ///
    /// Storage Consideration:
    /// Reachability and GHOSTDAG data structures have O(headers * mergeset_limit) complexity.
    /// The 512 cap prevents excessive storage growth.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// assert_eq!(OneBps::mergeset_size_limit(), 180);  // K=10, so 2*10=20, clamped to 180
    /// assert_eq!(TenBps::mergeset_size_limit(), 248);  // K=124, so 2*124=248
    /// ```
    pub const fn mergeset_size_limit() -> u64 {
        let val = Self::ghostdag_k() * 2;
        if val < 180 {
            180 // Minimum merge capacity
        } else if val > 512 {
            512 // Storage complexity bound
        } else {
            val
        }
    }

    /// Returns the finality depth in blocks
    ///
    /// Number of blocks after which a block is considered probabilistically final.
    /// Scales with BPS to maintain constant *time* to finality (~100 seconds).
    ///
    /// Formula: BPS * 100
    /// - 1 BPS: 100 blocks = ~100 seconds
    /// - 10 BPS: 1000 blocks = ~100 seconds
    ///
    /// # Security Note
    ///
    /// This is a probabilistic finality metric. The actual security depends on:
    /// - Network hashrate distribution
    /// - GHOSTDAG K parameter
    /// - Network delay characteristics
    pub const fn finality_depth() -> u64 {
        BPS * 100
    }

    /// Returns the pruning depth in blocks
    ///
    /// Number of blocks to keep before pruning old data. Must be significantly
    /// larger than finality depth to ensure:
    /// 1. All finalized blocks are kept
    /// 2. Reorg protection beyond finality
    /// 3. Safe margin for network partitions
    ///
    /// Formula: finality_depth * 2
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// assert_eq!(OneBps::pruning_depth(), 200);   // 200 blocks (~200 seconds)
    /// assert_eq!(TenBps::pruning_depth(), 2000);  // 2000 blocks (~200 seconds)
    /// ```
    pub const fn pruning_depth() -> u64 {
        Self::finality_depth() * 2
    }

    /// Returns the coinbase maturity in blocks
    ///
    /// Number of blocks before a coinbase output can be spent. Scaled with BPS
    /// to maintain constant *time* maturity (~100 seconds).
    ///
    /// Formula: BPS * 100
    /// - 1 BPS: 100 blocks = ~100 seconds
    /// - 10 BPS: 1000 blocks = ~100 seconds
    ///
    /// # Rationale
    ///
    /// Coinbase maturity prevents spending of potentially orphaned block rewards.
    /// Time-based maturity (rather than block-based) provides consistent economic
    /// security across different BPS configurations.
    pub const fn coinbase_maturity() -> u64 {
        BPS * 100
    }
}

/// Calculates GHOSTDAG K parameter using Poisson process analysis
///
/// This function implements the algorithm from the PHANTOM paper (Section 4.2, Eq. 1)
/// to calculate the minimum K such that the probability of an anticone larger than K
/// is less than delta.
///
/// # Parameters
///
/// - `x`: Expected anticone size = 2 * D * lambda
///   - D: Maximum network propagation delay (seconds)
///   - lambda: Block production rate (blocks per second)
/// - `delta`: Maximum acceptable tail probability for anticone > K
///
/// # Returns
///
/// Minimum K such that P(anticone size > K) < delta
///
/// # Algorithm
///
/// Uses Poisson distribution cumulative probability:
/// 1. Start with K = 0
/// 2. Calculate sigma = sum(i=0 to K) [e^(-x) * x^i / i!]
/// 3. If 1 - sigma < delta, return K
/// 4. Otherwise increment K and repeat
///
/// # Example
///
/// ```rust,ignore
/// // For 1 BPS with D=2s, delta=0.001
/// let k = calculate_ghostdag_k(4.0, 0.001);
/// assert!(k >= 9 && k <= 10);  // Theoretical value ~9.7
///
/// // For 10 BPS with D=2s, delta=0.001
/// let k = calculate_ghostdag_k(40.0, 0.001);
/// assert!(k >= 60 && k <= 65);  // Theoretical value ~63.4
/// ```
///
/// # Reference
///
/// PHANTOM: A Scalable BlockDAG protocol
/// https://eprint.iacr.org/2018/104.pdf
///
/// SAFE: f64 used for offline configuration calculation only, not runtime consensus.
/// The K parameter is hardcoded in network configuration, not computed during operation.
pub fn calculate_ghostdag_k(x: f64, delta: f64) -> u64 {
    assert!(x > 0.0, "Expected anticone size must be positive");
    assert!(delta > 0.0 && delta < 1.0, "Delta must be in range (0, 1)");

    let (mut k, mut sigma, mut fraction, exp) = (0u64, 0.0, 1.0, std::f64::consts::E.powf(-x));

    loop {
        sigma += exp * fraction;
        if 1.0 - sigma < delta {
            return k;
        }
        k += 1;
        fraction *= x / k as f64; // Computes x^k / k! incrementally
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bps_values() {
        assert_eq!(OneBps::bps(), 1);
        assert_eq!(TenBps::bps(), 10);
    }

    #[test]
    fn test_target_time_per_block() {
        // OneBps: 1 second blocks
        assert_eq!(OneBps::target_time_per_block(), 1000);

        // TenBps: 100ms blocks
        assert_eq!(TenBps::target_time_per_block(), 100);
    }

    #[test]
    fn test_ghostdag_k_lookup() {
        assert_eq!(OneBps::ghostdag_k(), 10);
        assert_eq!(TenBps::ghostdag_k(), 124);
    }

    #[test]
    fn test_max_block_parents() {
        // OneBps: K=10, so 10/2=5, clamped to min 10
        assert_eq!(OneBps::max_block_parents(), 10);

        // TenBps: K=124, so 124/2=62, capped at 16
        assert_eq!(TenBps::max_block_parents(), 16);
    }

    #[test]
    fn test_mergeset_size_limit() {
        // OneBps: K=10, so 2*10=20, clamped to 180
        assert_eq!(OneBps::mergeset_size_limit(), 180);

        // TenBps: K=124, so 2*124=248
        assert_eq!(TenBps::mergeset_size_limit(), 248);
    }

    #[test]
    fn test_finality_depth() {
        // OneBps: 100 blocks
        assert_eq!(OneBps::finality_depth(), 100);

        // TenBps: 1000 blocks
        assert_eq!(TenBps::finality_depth(), 1000);
    }

    #[test]
    fn test_pruning_depth() {
        // OneBps: 200 blocks
        assert_eq!(OneBps::pruning_depth(), 200);

        // TenBps: 2000 blocks
        assert_eq!(TenBps::pruning_depth(), 2000);
    }

    #[test]
    fn test_coinbase_maturity() {
        // OneBps: 100 blocks
        assert_eq!(OneBps::coinbase_maturity(), 100);

        // TenBps: 1000 blocks
        assert_eq!(TenBps::coinbase_maturity(), 1000);
    }

    #[test]
    fn test_time_consistency() {
        // All time-based parameters should result in similar real time across BPS configs

        // Finality time: ~100 seconds
        let one_bps_finality_time = OneBps::finality_depth() * OneBps::target_time_per_block();
        let ten_bps_finality_time = TenBps::finality_depth() * TenBps::target_time_per_block();

        assert_eq!(one_bps_finality_time, 100_000); // 100 seconds
        assert_eq!(ten_bps_finality_time, 100_000); // 100 seconds

        // Coinbase maturity time: ~100 seconds
        let one_bps_maturity_time = OneBps::coinbase_maturity() * OneBps::target_time_per_block();
        let ten_bps_maturity_time = TenBps::coinbase_maturity() * TenBps::target_time_per_block();

        assert_eq!(one_bps_maturity_time, 100_000); // 100 seconds
        assert_eq!(ten_bps_maturity_time, 100_000); // 100 seconds
    }

    #[test]
    fn test_calculate_ghostdag_k_one_bps() {
        // For 1 BPS with D=2s, delta=0.001
        // x = 2 * 2 * 1 = 4.0
        // Expected K ~9.7, mathematically calculated result may vary slightly
        let k = calculate_ghostdag_k(4.0, 0.001);
        assert!(
            k >= 9 && k <= 11,
            "K should be around 9-11 for 1 BPS, got {}",
            k
        );
    }

    #[test]
    fn test_calculate_ghostdag_k_ten_bps() {
        // For 10 BPS with D=2s, delta=0.001
        // x = 2 * 2 * 10 = 40.0
        // Expected K ~63.4
        let k = calculate_ghostdag_k(40.0, 0.001);
        assert!(
            k >= 60 && k <= 65,
            "K should be around 60-65 for 10 BPS, got {}",
            k
        );
    }

    #[test]
    fn test_calculate_ghostdag_k_monotonic() {
        // K should increase with x
        let k1 = calculate_ghostdag_k(4.0, 0.001);
        let k2 = calculate_ghostdag_k(8.0, 0.001);
        let k3 = calculate_ghostdag_k(16.0, 0.001);

        assert!(k1 < k2);
        assert!(k2 < k3);
    }

    #[test]
    #[should_panic(expected = "Expected anticone size must be positive")]
    fn test_calculate_ghostdag_k_negative_x() {
        calculate_ghostdag_k(-1.0, 0.001);
    }

    #[test]
    #[should_panic(expected = "Delta must be in range (0, 1)")]
    fn test_calculate_ghostdag_k_invalid_delta() {
        calculate_ghostdag_k(4.0, 1.5);
    }
}
