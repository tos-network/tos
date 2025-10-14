# TOS API Test Bug Report

**Date**: 2025-10-14
**Test Run**: Complete daemon API test suite
**Total Tests**: 87
**Passed**: 37 (43%)
**Failed**: 50 (57%)

## Executive Summary

After running comprehensive API tests, we discovered 50 test failures across all categories. The failures fall into several distinct patterns:

1. **Block API Structure Mismatch** (9 failures) - Block responses don't have separate `header` and `transactions` fields
2. **Balance/Account API Parameter Issues** (21 failures) - APIs expect different parameter structures than tested
3. **Type Mismatches** (8 failures) - APIs return strings where integers expected, or objects where primitives expected
4. **Network/Mempool API Issues** (6 failures) - Parameter structure and response format issues
5. **Utility API Issues** (6 failures) - Response format doesn't match expectations

## Category 1: Block API Structure Issues (9 failures)

### Root Cause
Block-related APIs return flat block structures, but tests expect nested structure with separate `header` and `transactions` fields.

### Failed Tests
```
FAILED daemon/test_block_apis.py::test_get_block_by_hash
FAILED daemon/test_block_apis.py::test_get_top_block
FAILED daemon/test_block_apis.py::test_get_blocks_at_blue_score
FAILED daemon/test_block_apis.py::test_block_header_structure
FAILED daemon/test_block_apis.py::test_block_transactions_structure
FAILED daemon/test_ghostdag_apis.py::test_get_block_at_topoheight
FAILED daemon/test_ghostdag_apis.py::test_block_header_ghostdag_fields
FAILED daemon/test_ghostdag_apis.py::test_block_header_parents_by_level
FAILED daemon/test_ghostdag_apis.py::test_genesis_block_special_case
```

### Example Error
```python
# Test code expects:
block = {
    "header": {
        "hash": "...",
        "topoheight": 123,
        "blue_score": 123,
        ...
    },
    "transactions": [...]
}

# Actual response:
KeyError: 'header'
# Indicates block is flat structure, not nested
```

### Action Required
- Investigate actual block response structure from daemon
- Option A: Update tests to match actual structure
- Option B: Change daemon to return nested structure (breaking change)

## Category 2: Balance/Account API Parameter Issues (21 failures)

### Root Cause
RPC Error -32602 "Invalid params" - Tests pass single parameters but APIs expect different structures.

### Failed Tests
```
FAILED daemon/test_balance_apis.py::test_get_balance
FAILED daemon/test_balance_apis.py::test_get_balance_at_topoheight
FAILED daemon/test_balance_apis.py::test_has_balance
FAILED daemon/test_balance_apis.py::test_get_stable_balance
FAILED daemon/test_balance_apis.py::test_get_nonce_at_topoheight
FAILED daemon/test_balance_apis.py::test_get_account_history
FAILED daemon/test_balance_apis.py::test_get_account_history_with_range
FAILED daemon/test_balance_apis.py::test_get_account_assets
FAILED daemon/test_balance_apis.py::test_is_account_registered
FAILED daemon/test_balance_apis.py::test_is_account_registered_nonexistent
FAILED daemon/test_balance_apis.py::test_get_account_registration_topoheight
FAILED daemon/test_balance_apis.py::test_get_accounts
FAILED daemon/test_balance_apis.py::test_get_accounts_with_pagination
FAILED daemon/test_balance_apis.py::test_get_accounts_with_minimum_balance
FAILED daemon/test_balance_apis.py::test_balance_consistency_across_topoheights
FAILED daemon/test_balance_apis.py::test_balance_never_decreases_in_past
FAILED daemon/test_balance_apis.py::test_get_balance_performance
```

### Example Errors

#### get_balance
```
RPC Error -32602: Invalid params: invalid length 1, expected struct GetBalanceParams with 2 elements
```
**Issue**: API expects `{address, asset}` but test sends `[address]`

#### get_balance_at_topoheight
```
RPC Error -32602: Invalid params: invalid length 2, expected struct GetBalanceParams with 3 elements
```
**Issue**: API expects `{address, topoheight, asset}` but test sends `[address, topoheight]`

#### get_account_history
```
RPC Error -32602: Invalid params: missing field `address`
```
**Issue**: Test sends positional array but API expects named object

### Action Required
- Check daemon Rust code for actual parameter structures:
  - `GetBalanceParams`
  - `GetBalanceAtTopoheightParams`
  - `GetAccountHistoryParams`
  - etc.
- Update test code to use correct parameter format
- Update API documentation

## Category 3: Type Mismatches (8 failures)

### 3.1 Network Name Mismatch

```
FAILED daemon/test_get_info.py::test_network_field
AssertionError: Network mismatch: got Dev, expected devnet
```

**Issue**: API returns abbreviated "Dev" but test expects full "devnet"

**Action**: Decide on standard (abbreviated or full names) and update either API or test

### 3.2 Difficulty Type Mismatch

```
FAILED daemon/test_get_info.py::test_difficulty_field
AssertionError: assert False
 +  where False = isinstance('1011', int)
```

**Issue**: API returns difficulty as string "1011" but test expects integer 1011

**Action**: Change API to return integer or update test to accept string

### 3.3 Nonce Type Issues

```
FAILED daemon/test_balance_apis.py::test_get_nonce
AssertionError: assert False
 +  where False = isinstance('0', int)

FAILED daemon/test_balance_apis.py::test_has_nonce
AssertionError: assert False
 +  where False = isinstance('0', int)

FAILED daemon/test_balance_apis.py::test_nonce_increases_monotonically
TypeError: '<' not supported between instances of 'str' and 'str'
```

**Issue**: Nonce returned as string "0" but test expects integer 0

**Action**: Change API to return integer nonce values

### 3.4 Asset Hash Format

```
FAILED daemon/test_balance_apis.py::test_get_balance_with_asset
AssertionError: assert 'tos' == '0000000000000000000000000000000000000000000000000000000000000000'
```

**Issue**: Native asset returns "tos" but test expects 64-character hash

**Action**: Document that native asset uses special "tos" identifier

### 3.5 Utility API Response Structures

```
FAILED daemon/test_utility_apis.py::test_get_difficulty
AssertionError: assert False
 +  where False = isinstance({'difficulty': '1049', 'hashrate': '1049', ...}, int)
```

**Issue**: API returns object with multiple fields, test expects single integer

```
FAILED daemon/test_utility_apis.py::test_get_size_on_disk
AssertionError: assert False
 +  where False = isinstance({'size_bytes': 45323768, 'size_formatted': '43.2 MiB'}, str)
```

**Issue**: API returns object with formatted size, test expects string

**Action**: Update tests to handle object responses with multiple fields

## Category 4: Network/Mempool API Issues (6 failures)

### Failed Tests
```
FAILED daemon/test_network_apis.py::test_get_peers
FAILED daemon/test_network_apis.py::test_get_peers_structure
FAILED daemon/test_network_apis.py::test_get_mempool
FAILED daemon/test_network_apis.py::test_get_mempool_summary
FAILED daemon/test_network_apis.py::test_get_mempool_cache
FAILED daemon/test_network_apis.py::test_mempool_count_consistency
```

### Example Errors

#### get_peers
```
AssertionError: assert False
 +  where False = isinstance({'our_id': 123, 'peers': []}, list)
```
**Issue**: API returns object with peer list, test expects direct array

#### get_mempool
```
RPC Error -32602: Invalid params: expected tuple of size 0, but got array of length 1
```
**Issue**: API expects no parameters, test sends `[False]`

### Action Required
- Check actual peer list response structure
- Update mempool API calls to send correct parameters

## Category 5: Utility API Issues (6 failures)

### Failed Tests
```
FAILED daemon/test_utility_apis.py::test_validate_address_invalid
FAILED daemon/test_utility_apis.py::test_validate_address_wrong_network
FAILED daemon/test_utility_apis.py::test_extract_key_from_address
FAILED daemon/test_utility_apis.py::test_make_integrated_address
FAILED daemon/test_utility_apis.py::test_get_difficulty
FAILED daemon/test_utility_apis.py::test_get_hard_forks
FAILED daemon/test_utility_apis.py::test_get_size_on_disk
```

### Example Errors

#### validate_address
```
RPC Error -32602: Invalid params: missing field `address`
```
**Issue**: Test sends positional parameter, API expects named object

#### get_hard_forks
```
AssertionError: assert 'block_time_target' in {'changelog': '...', 'height': 0, ...}
```
**Issue**: Hard fork structure doesn't include expected field

### Action Required
- Update validate_address calls to use named parameters
- Review hard fork response structure
- Update tests for actual API response formats

## Passed Tests (37)

### get_info.py (12/14 passed - 86%)
```
✓ test_get_info_basic
✓ test_get_info_bps_fields (TIP-2)
✓ test_bps_calculation (TIP-2)
✓ test_actual_bps_calculation (TIP-2)
✓ test_ghostdag_fields (TIP-2)
✓ test_supply_fields
✓ test_reward_fields
✓ test_mempool_size
✓ test_top_block_hash
✓ test_get_info_performance
✓ test_bps_target_matches_network_config
✓ test_block_time_target_matches_network_config
✗ test_network_field (returns "Dev" not "devnet")
✗ test_difficulty_field (string not int)
```

### balance_apis.py (4/25 passed - 16%)
```
✓ test_get_balance_invalid_address
✓ test_count_accounts
✓ test_get_balance_at_invalid_topoheight
✓ test_get_balance_negative_topoheight
✗ 21 tests with parameter structure issues
```

### block_apis.py (6/12 passed - 50%)
```
✓ test_get_blocks_range_by_topoheight
✓ test_get_blocks_range_by_blue_score
✓ test_get_blocks_range_single_block
✓ test_get_blocks_range_invalid_range
✓ test_get_blocks_range_too_large
✓ test_get_block_performance
✗ 6 tests with block structure issues
```

### ghostdag_apis.py (1/8 passed - 13%)
```
✓ test_topoheight_sequential
✗ 7 tests with block structure issues
```

### network_apis.py (5/11 passed - 45%)
```
✓ test_p2p_status
✓ test_get_estimated_fee_rates
✓ test_fee_rates_structure
✓ test_fee_rates_ordering
✓ test_network_load_factor
✗ 6 tests with parameter/structure issues
```

### utility_apis.py (9/15 passed - 60%)
```
✓ test_validate_address_valid
✓ test_split_address
✓ test_count_assets
✓ test_count_transactions
✓ test_count_blocks_by_address
✓ test_block_type_validator
✓ test_get_version
✓ test_get_dev_fee_threshold
✓ test_get_miner_work
✗ 6 tests with parameter/structure issues
```

## Priority Fixes

### P0 - Critical (Must Fix)
1. **Document actual API parameter formats** - Most failures are parameter mismatches
2. **Document actual response structures** - Tests assume wrong structures
3. **Fix type consistency** - String vs int for numeric values (difficulty, nonce)

### P1 - High (Should Fix Soon)
1. **Block structure** - Clarify if blocks have nested header/transactions or flat structure
2. **Balance API parameters** - GetBalanceParams structure needs documentation
3. **Network naming** - Standardize on "Dev" vs "devnet"

### P2 - Medium (Nice to Have)
1. **Native asset identifier** - Document "tos" special case
2. **Peer list structure** - Update tests for actual response format
3. **Hard fork fields** - Document actual hard fork response structure

## Recommended Next Steps

1. **Investigate Daemon Code** (1-2 hours)
   - Check `daemon/src/api/` for actual RPC handler implementations
   - Document actual parameter structures
   - Document actual response structures

2. **Update Tests** (2-3 hours)
   - Fix parameter formats to match actual API
   - Fix expected response structures
   - Fix type assertions

3. **Update Documentation** (1 hour)
   - Update `DAEMON_RPC_API_REFERENCE.md` with correct formats
   - Add examples showing actual request/response

4. **Rerun Tests** (30 minutes)
   - Verify all fixes work
   - Aim for >90% pass rate

5. **Consider API Improvements** (Future)
   - Standardize type usage (string vs int)
   - Improve error messages for invalid parameters
   - Add parameter validation

## Test Files to Update

1. `daemon/test_balance_apis.py` - Fix all parameter structures
2. `daemon/test_block_apis.py` - Fix block structure expectations
3. `daemon/test_ghostdag_apis.py` - Fix block structure expectations
4. `daemon/test_network_apis.py` - Fix peer list and mempool parameters
5. `daemon/test_utility_apis.py` - Fix address validation parameters
6. `daemon/test_get_info.py` - Fix network and difficulty type expectations

## Conclusion

While 37 tests passed successfully (43%), the 50 failures (57%) indicate significant mismatches between test expectations and actual API behavior. The good news is that most failures follow clear patterns:

- **Parameter structure mismatches** - Tests need correct parameter format
- **Response structure mismatches** - Tests need correct response format
- **Type mismatches** - Need consistency in numeric types

These are all fixable by updating the tests to match actual API behavior. The APIs themselves appear to be working correctly - we just need to align our tests with reality.

**Estimated time to fix all issues**: 4-6 hours of focused work.
