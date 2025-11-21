# Aave V3 Pool - Instruction Format Reference

## Instruction Enum

```rust
pub enum Instruction {
    Initialize = 0,
    InitReserve = 1,
    SetReserveConfiguration = 2,
    Supply = 10,
    Withdraw = 11,
    Borrow = 12,
    Repay = 13,
    Liquidate = 14,
    SetUserCollateral = 20,
    GetUserSupply = 100,
    GetUserBorrow = 101,
    GetHealthFactor = 102,
    GetReserveData = 103,
    GetUserAccountData = 104,
}
```

## Instruction Formats

### Initialize (0)
**Purpose**: Initialize the lending pool
**Format**: `[0]` (1 byte total)
**Parameters**: None
**Returns**: Success (0) or Error

### InitReserve (1)
**Purpose**: Initialize a reserve asset with default configuration
**Format**: `[1] + asset(32)` (33 bytes total)
**Parameters**:
- `asset`: 32-byte address

**Default Configuration**:
- LTV: 75% (7500 bps)
- Liquidation Threshold: 80% (8000 bps)
- Liquidation Bonus: 5% (500 bps)

**Returns**: Success (0) or Error

### SetReserveConfiguration (2)
**Purpose**: Set custom reserve configuration (LTV, liquidation params)
**Format**: `[2] + asset(32) + ltv(8) + liquidation_threshold(8) + liquidation_bonus(8)` (58 bytes total)
**Parameters**:
- `asset`: 32-byte address
- `ltv`: u64 (basis points, e.g., 7500 = 75%)
- `liquidation_threshold`: u64 (basis points, e.g., 8000 = 80%)
- `liquidation_bonus`: u64 (basis points, e.g., 500 = 5%)

**Validation**:
- ltv <= liquidation_threshold
- liquidation_threshold <= 10000 (100%)
- liquidation_bonus <= 2000 (20%)

**Returns**: Success (0) or Error

### Supply (10)
**Purpose**: Supply assets to the pool
**Format**: `[10] + asset(32) + amount(8) + on_behalf_of(32)` (73 bytes total)
**Parameters**:
- `asset`: 32-byte reserve asset address
- `amount`: u64 amount to supply
- `on_behalf_of`: 32-byte beneficiary address

**Note**: To use as collateral, call SetUserCollateral(20) separately

**Returns**: Success (0) or Error

### Withdraw (11)
**Purpose**: Withdraw supplied assets from the pool
**Format**: `[11] + asset(32) + amount(8)` (41 bytes total)
**Parameters**:
- `asset`: 32-byte reserve asset address
- `amount`: u64 amount to withdraw

**Validation**:
- User must have sufficient supply balance
- Withdrawal must not cause health factor < 1.0

**Returns**: Withdrawn amount (8 bytes)

### Borrow (12)
**Purpose**: Borrow assets from the pool
**Format**: `[12] + asset(32) + amount(8)` (41 bytes total)
**Parameters**:
- `asset`: 32-byte reserve asset address
- `amount`: u64 amount to borrow

**Validation**:
- Sufficient liquidity available
- Health factor must remain >= 1.0 after borrow

**Returns**: Success (0) or Error

### Repay (13)
**Purpose**: Repay borrowed assets
**Format**: `[13] + asset(32) + amount(8) + on_behalf_of(32)` (73 bytes total)
**Parameters**:
- `asset`: 32-byte reserve asset address
- `amount`: u64 amount to repay (can be > debt, will repay max)
- `on_behalf_of`: 32-byte borrower address

**Returns**: Actual repaid amount (8 bytes)

### Liquidate (14)
**Purpose**: Liquidate undercollateralized position
**Format**: `[14] + collateral_asset(32) + debt_asset(32) + user(32) + debt_to_cover(8)` (105 bytes total)
**Parameters**:
- `collateral_asset`: 32-byte collateral asset address
- `debt_asset`: 32-byte debt asset address
- `user`: 32-byte user address to liquidate
- `debt_to_cover`: u64 amount of debt to cover

**Validation**:
- User health factor < 1.0
- Max liquidation: 50% of debt (close factor)
- Liquidator receives 5% bonus

**Returns**: [actual_debt_covered(8) + collateral_seized(8)] (16 bytes)

### SetUserCollateral (20)
**Purpose**: Enable/disable asset as collateral
**Format**: `[20] + asset(32) + enabled(1)` (34 bytes total)
**Parameters**:
- `asset`: 32-byte reserve asset address
- `enabled`: u8 (1 = enabled, 0 = disabled)

**Returns**: Success (0) or Error

---

## Query Instructions (Read-Only)

### GetUserSupply (100)
**Format**: `[100] + user(32) + asset(32)` (65 bytes total)
**Returns**: Supply balance (8 bytes)

### GetUserBorrow (101)
**Format**: `[101] + user(32) + asset(32)` (65 bytes total)
**Returns**: Borrow balance (8 bytes)

### GetHealthFactor (102)
**Format**: `[102] + user(32)` (33 bytes total)
**Returns**: Health factor in RAY precision (8 bytes)

### GetReserveData (103)
**Format**: `[103] + asset(32)` (33 bytes total)
**Returns**: Reserve data (64 bytes):
- liquidity_index (8)
- borrow_index (8)
- total_liquidity (8)
- total_debt (8)
- borrow_rate (8)
- supply_rate (8)
- last_update (8)
- reserved (8)

### GetUserAccountData (104)
**Format**: `[104] + user(32)` (33 bytes total) or `[104]` (1 byte, uses caller)
**Returns**: User account data (40 bytes):
- total_collateral (8)
- total_debt (8)
- available_borrow (8)
- current_ltv (8)
- health_factor (8)

---

## Error Codes

- `0`: Success
- `1`: StorageError
- `2`: InvalidInput
- `3`: Overflow
- `5`: InsufficientLiquidity
- `6`: DivisionByZero
- `7`: CannotLiquidateHealthyPosition
- `8`: InsufficientBalance
- `10`: InvalidConfiguration
- `11`: ReserveNotInitialized
- `12`: InvalidReserveData
- `13`: InsufficientBalance
- `14`: Underflow
- `15`: HealthFactorTooLow
- `16`: HealthFactorTooLowToBorrow
- `17`: NothingToRepay
- `18`: PoolNotInitialized

---

## Example Usage

### Initialize Pool and Supply
```rust
// 1. Initialize pool
let input = vec![0u8];
execute(&bytecode, &mut provider, topoheight, &contract_hash, ..., &input, None);

// 2. Initialize ETH reserve (uses default 75% LTV, 80% LT)
let mut input = vec![1u8];
input.extend_from_slice(&eth_address);
execute(..., &input, ...);

// 3. Supply 10 ETH
let mut input = vec![10u8];
input.extend_from_slice(&eth_address);
input.extend_from_slice(&(10_000_000_000u64).to_le_bytes()); // 10 ETH
input.extend_from_slice(&user_address);
execute(..., &input, ...);

// 4. Enable ETH as collateral
let mut input = vec![20u8];
input.extend_from_slice(&eth_address);
input.push(1u8); // enabled
execute(..., &input, ...);
```

### Borrow and Repay
```rust
// 5. Borrow 5000 USDC
let mut input = vec![12u8];
input.extend_from_slice(&usdc_address);
input.extend_from_slice(&5000u64.to_le_bytes());
execute(..., &input, ...);

// 6. Repay 2500 USDC
let mut input = vec![13u8];
input.extend_from_slice(&usdc_address);
input.extend_from_slice(&2500u64.to_le_bytes());
input.extend_from_slice(&user_address);
execute(..., &input, ...);
```
