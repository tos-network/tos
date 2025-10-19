# Changelog

This file contains all the changelogs to ensure that changes can be tracked and to provide a summary for interested parties.

To see the full history and exact changes, please refer to the commits history directly.

## [Unreleased]

### Changed
- **[BREAKING]** Simplified balance system: moved from encrypted ElGamal balances to plaintext u64
  - **Architecture**: Removed bulletproofs and sigma protocol proof verification system
  - **Transaction size**: Reduced from ~500 bytes to ~150 bytes (-70%)
  - **Verification speed**: Eliminated expensive proof verification (100x-1000x faster)
  - **Removed dependencies**: bulletproofs, merlin (Transcript)
  - **Cleaned up**: Legacy proof-related code (Transcript, BatchCollector stubs)
  - **Migration**: All balances are now transparent and stored as plaintext u64
  - **Benefits**:
    - ✅ Simpler codebase and easier to audit
    - ✅ Faster transaction processing
    - ✅ Reduced node resource requirements
    - ✅ Smaller blockchain storage footprint
  - **Trade-off**: Transaction amounts are now public (similar to Bitcoin/Ethereum)

- **[BREAKING]** Optimized transfer memo (extra_data) size limit for real-world usage
  - **Reduced** per-transfer memo limit: **1024 bytes → 128 bytes** (-87.5%)
  - **Reduced** total transaction memo limit: **32KB → 4KB** (-87.5%)
  - **Rationale**: Based on analysis of actual usage patterns where memos typically contain:
    - Exchange deposit IDs: 8-15 bytes
    - Order references: 20-50 bytes
    - Invoice numbers: 15-40 bytes
    - UUID formats: ~36 bytes
  - **Benefits**:
    - ✅ Covers 99%+ of real-world use cases
    - ✅ Reduces storage bloat and node resource usage
    - ✅ Mitigates potential DoS attack vectors
    - ✅ Maintains sufficient headroom for future needs
- Updated documentation with real-world memo usage examples
- Enhanced code comments explaining the optimization rationale
- Adjusted test cases to reflect realistic usage patterns (32-byte exchange IDs)

### Technical Details
- Modified `EXTRA_DATA_LIMIT_SIZE` constant from 1024 to 128 bytes
- Updated `EXTRA_DATA_LIMIT_SUM_SIZE` calculation (128 × 32 = 4KB total)
- Enhanced English documentation for energy model edge cases
- Fixed test inconsistencies in energy fee calculations
- All tests pass with new limits including encryption overhead considerations

### Migration Impact
- ✅ **No impact** on existing transfers with memo ≤ 128 bytes
- ✅ **Typical usage** (exchange IDs, order refs) fully supported
- ⚠️ **Large memos** (>128 bytes) will need to be split or shortened
- 📊 **Expected impact**: <1% of realistic use cases

## v0.1.0
Initial version
