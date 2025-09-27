# TOS AI Mining - Security and Reputation System

## Overview

The TOS AI Mining system implements a comprehensive security framework designed to prevent various attack vectors while maintaining a positive user experience for legitimate participants. The system combines economic incentives, reputation scoring, anti-Sybil mechanisms, and rate limiting to create a robust defense against malicious activities.

**Version**: 1.2.0
**Status**: ✅ Production Ready
**Last Updated**: September 27, 2025

## Table of Contents

1. [Security Architecture](#security-architecture)
2. [Reputation System](#reputation-system)
3. [Anti-Sybil Protection](#anti-sybil-protection)
4. [Economic Security Measures](#economic-security-measures)
5. [Rate Limiting](#rate-limiting)
6. [Gas Fee Security Model](#gas-fee-security-model)
7. [Attack Scenario Analysis](#attack-scenario-analysis)
8. [Implementation Details](#implementation-details)
9. [Configuration Parameters](#configuration-parameters)
10. [Monitoring and Detection](#monitoring-and-detection)

## Security Architecture

### Multi-Layer Defense Strategy

The TOS AI Mining security system employs a defense-in-depth approach with multiple layers:

```
┌─────────────────────────────────────────┐
│           Application Layer             │
│  ┌─────────────────────────────────────┐ │
│  │        Rate Limiting Layer          │ │
│  │  ┌─────────────────────────────────┐ │ │
│  │  │      Economic Security Layer    │ │ │
│  │  │  ┌─────────────────────────────┐ │ │ │
│  │  │  │    Reputation Layer         │ │ │ │
│  │  │  │  ┌─────────────────────────┐ │ │ │ │
│  │  │  │  │   Anti-Sybil Layer      │ │ │ │ │
│  │  │  │  └─────────────────────────┘ │ │ │ │
│  │  │  └─────────────────────────────┘ │ │ │
│  │  └─────────────────────────────────┘ │ │
│  └─────────────────────────────────────┘ │
└─────────────────────────────────────────┘
```

### Core Security Principles

1. **Economic Rationality**: Make attacks economically unviable
2. **Progressive Trust**: Build trust over time through consistent behavior
3. **Risk-Based Access**: Grant access based on reputation and stake
4. **Transparent Costs**: Predictable fee structure based on risk assessment
5. **Dynamic Adaptation**: Adjust security parameters based on network conditions

## Reputation System

### Account Reputation Structure

```rust
pub struct AccountReputation {
    pub account: CompressedPublicKey,
    pub created_at: u64,                    // Account creation timestamp
    pub transaction_count: u64,             // Historical transaction count
    pub stake_amount: u64,                  // Current stake amount
    pub last_submission_time: u64,          // Last submission timestamp
    pub reputation_score: f64,              // Calculated score (0.0-1.0)
    pub total_rewards_earned: u64,          // Cumulative rewards
    pub successful_validations: u64,        // Successful validation count
    pub total_validations: u64,             // Total validation attempts
}
```

### Reputation Calculation Algorithm

The reputation score is calculated using a weighted formula:

```
Reputation Score = (Age Score × 0.3) + (History Score × 0.4) + (Stake Score × 0.3) + Bonuses
```

#### Component Breakdown

**1. Age Score (30% weight)**
```rust
let account_age_days = (current_time - created_at) / (24 * 3600);
let age_score = (account_age_days as f64 / 30.0).min(1.0);  // 30 days to max score
```

**2. Transaction History Score (40% weight)**
```rust
let history_score = (transaction_count as f64 / 100.0).min(1.0);  // 100 transactions to max score
```

**3. Stake Score (30% weight)**
```rust
let stake_score = (stake_amount as f64 / 1_000_000.0).min(1.0);  // 0.001 TOS to max score
```

**4. Validation Accuracy Bonus (up to +20%)**
```rust
let validation_bonus = if total_validations > 10 {
    let accuracy = successful_validations as f64 / total_validations as f64;
    ((accuracy - 0.8).max(0.0) * 1.0).min(0.2)  // 80%+ accuracy gets bonus
} else {
    0.0
};
```

**5. Long-term Participation Bonus (+10%)**
```rust
let long_term_bonus = if account_age_days > 90 { 0.1 } else { 0.0 };
```

### Reputation Thresholds

| Reputation Level | Score Range | Access Levels |
|------------------|-------------|---------------|
| **New User** | 0.0 - 0.1 | Basic (restricted) |
| **Novice** | 0.1 - 0.3 | Basic + Intermediate |
| **Regular** | 0.3 - 0.5 | Basic + Intermediate |
| **Experienced** | 0.5 - 0.7 | All except Expert |
| **Expert** | 0.7 - 0.9 | All levels |
| **Elite** | 0.9 - 1.0 | All levels + premium benefits |

### Task Access Control

```rust
pub fn can_participate_in_difficulty(&self, difficulty: &DifficultyLevel) -> bool {
    match difficulty {
        DifficultyLevel::Basic => self.reputation_score >= 0.1,        // Min: 0.1
        DifficultyLevel::Intermediate => self.reputation_score >= 0.3, // Min: 0.3
        DifficultyLevel::Advanced => self.reputation_score >= 0.5,     // Min: 0.5
        DifficultyLevel::Expert => self.reputation_score >= 0.7,       // Min: 0.7
    }
}
```

## Anti-Sybil Protection

### Detection Algorithm

The anti-Sybil detector uses multiple risk factors to identify potentially malicious accounts:

```rust
pub struct AntiSybilResult {
    pub is_valid: bool,                    // Pass/fail result
    pub risk_level: RiskLevel,             // Low/Medium/High/Critical
    pub details: Vec<String>,              // Specific risk factors
    pub recommended_action: RecommendedAction, // System response
}
```

### Risk Assessment Factors

**1. Account Age Check**
- < 24 hours: +0.4 risk score ("Account created less than 24 hours ago")
- < 1 week: +0.2 risk score ("Account created less than 1 week ago")

**2. Transaction History Check**
- 0 transactions: +0.3 risk score ("No transaction history")
- < 5 transactions: +0.15 risk score ("Limited transaction history")

**3. Stake Amount Check**
- 0 stake: +0.3 risk score ("No stake amount")
- < LOW_STAKE_THRESHOLD: +0.2 risk score ("Stake amount too low")

**4. Reputation Score Check**
- < 0.1: +0.5 risk score ("Reputation score extremely low")
- < 0.3: +0.2 risk score ("Reputation score low")

**5. Rate Limit Check**
- Violating cooldown: +0.1 risk score ("Still in cooldown period")

### Risk Level Determination

| Risk Score | Risk Level | Recommended Action |
|------------|------------|-------------------|
| 0.8+ | **Critical** | Reject operation |
| 0.6 - 0.8 | **High** | Increase fee 5x |
| 0.4 - 0.6 | **Medium** | Increase fee 2x |
| 0.2 - 0.4 | **Low** | Increase fee 1.2x |
| < 0.2 | **Low** | Allow operation |

## Economic Security Measures

### Secure Gas Pricing Model

The gas fee calculation incorporates multiple security factors:

```rust
pub fn calculate_secure_gas_cost(
    content_length: usize,
    difficulty: &DifficultyLevel,
    reputation: &AccountReputation,
    stake_amount: u64,
) -> u64
```

#### Fee Components

**1. Base Fee**
```rust
let base_fee = 2500u64;  // 2,500 nanoTOS minimum
```

**2. Content Storage Cost (Tiered Pricing)**
```rust
let content_cost = if content_length <= 200 {
    content_length as u64 * 500        // 0.0000005 TOS/byte
} else if content_length <= 1000 {
    (200 * 500) + ((content_length - 200) * 1000)  // 0.000001 TOS/byte
} else {
    (200 * 500) + (800 * 1000) + ((content_length - 1000) * 2000)  // 0.000002 TOS/byte
};
```

**3. Difficulty Multipliers**
- Basic: 1.0x
- Intermediate: 1.2x
- Advanced: 1.5x
- Expert: 2.0x

**4. Stake-Based Penalties**
- Low stake (< 10K nanoTOS): 5.0x multiplier
- Medium stake (< 100K nanoTOS): 2.0x multiplier
- High stake (≥ 100K nanoTOS): 1.0x multiplier

**5. Reputation Modifiers**
- High reputation (≥ 0.9): 0.5x discount
- Medium reputation (≥ 0.7): 0.7x discount
- Low reputation (< 0.3): 2.0x penalty
- Normal reputation: 1.0x standard rate

### Minimum Economic Threshold

```rust
const MIN_TRANSACTION_COST: u64 = 50_000;  // 0.00005 TOS minimum
```

This ensures that even the smallest operations have a meaningful economic cost, preventing spam attacks.

## Rate Limiting

### Dynamic Cooldown Periods

Cooldown periods are based on reputation scores:

```rust
pub fn get_cooldown_period(&self) -> u64 {
    if self.reputation_score >= 0.8 {
        300   // 5 minutes (high reputation)
    } else if self.reputation_score >= 0.5 {
        900   // 15 minutes (medium reputation)
    } else if self.reputation_score >= 0.3 {
        1800  // 30 minutes (low reputation)
    } else {
        3600  // 1 hour (very low reputation)
    }
}
```

### Submission Validation

```rust
pub fn can_submit_now(&self, current_time: u64) -> bool {
    let cooldown = self.get_cooldown_period();
    current_time.saturating_sub(self.last_submission_time) >= cooldown
}
```

### Remaining Cooldown Calculation

```rust
pub fn get_remaining_cooldown(&self, current_time: u64) -> u64 {
    let cooldown = self.get_cooldown_period();
    let elapsed = current_time.saturating_sub(self.last_submission_time);
    cooldown.saturating_sub(elapsed)
}
```

## Gas Fee Security Model

### Progressive Content Pricing

The system uses a tiered pricing model to balance accessibility with spam prevention:

```rust
// Short content (0-200 bytes): Encouraged
content_cost = length * 500  // 0.0000005 TOS/byte

// Medium content (200-1000 bytes): Standard rate
content_cost = (200 * 500) + ((length - 200) * 1000)  // 0.000001 TOS/byte

// Long content (1000+ bytes): Higher rate
content_cost = (200 * 500) + (800 * 1000) + ((length - 1000) * 2000)  // 0.000002 TOS/byte
```

### Economic Attack Prevention

**Spam Attack Prevention**
- Minimum fee of 50,000 nanoTOS makes bulk spam expensive
- Progressive pricing discourages unnecessarily long content

**Sybil Attack Prevention**
- New accounts pay significantly higher fees (up to 5x multiplier)
- Reputation building requires time and consistent behavior
- Stake requirements create economic barriers

**Quality Incentivization**
- High reputation users receive fee discounts
- Validation accuracy bonuses encourage quality participation
- Long-term participation rewards loyalty

## Attack Scenario Analysis

### 1. Spam Content Attack

**Attack**: Submit many low-quality, short answers to earn easy rewards.

**Defense**:
- Minimum transaction cost (50,000 nanoTOS)
- Rate limiting based on reputation
- Progressive fee increases for repeated submissions
- Quality scoring through validation system

**Economic Impact**: Attacker needs significant capital to sustain attack, making it unprofitable.

### 2. Sybil Attack

**Attack**: Create many fake accounts to manipulate the system.

**Defense**:
- Account age requirements (30 days for full reputation)
- Transaction history requirements (100 transactions for full reputation)
- Stake amount requirements (0.001 TOS for full reputation)
- High fees for new accounts (up to 5x multiplier)

**Economic Impact**: Building reputation across multiple accounts requires substantial time and capital investment.

### 3. Content Farm Attack

**Attack**: Generate bulk low-quality content to maximize volume.

**Defense**:
- Validation system ensures quality scoring
- Reputation system rewards consistent quality
- Fee discounts only for high-reputation accounts
- Rate limiting prevents bulk submissions

**Economic Impact**: Low-quality content receives low validation scores, reducing profitability.

### 4. Collusion Attack

**Attack**: Coordinate between miners and validators to inflate scores.

**Defense**:
- Transparent validation history tracking
- Statistical analysis of validation patterns
- Reputation penalties for suspicious behavior
- Economic penalties through fee adjustments

**Economic Impact**: Coordinated attacks require significant capital and risk reputation damage.

## Implementation Details

### Core Data Structures

```rust
// Reputation thresholds
pub const MIN_REPUTATION_FOR_BASIC: f64 = 0.1;
pub const MIN_REPUTATION_FOR_INTERMEDIATE: f64 = 0.3;
pub const MIN_REPUTATION_FOR_ADVANCED: f64 = 0.5;
pub const MIN_REPUTATION_FOR_EXPERT: f64 = 0.7;

// Reputation level thresholds
pub const LOW_REPUTATION_THRESHOLD: f64 = 0.3;
pub const MEDIUM_REPUTATION_THRESHOLD: f64 = 0.5;
pub const HIGH_REPUTATION_THRESHOLD: f64 = 0.8;

// Economic thresholds
pub const LOW_STAKE_THRESHOLD: u64 = 100_000;      // 0.0001 TOS
pub const MEDIUM_STAKE_THRESHOLD: u64 = 1_000_000; // 0.001 TOS

// Fee modifiers
pub const LOW_STAKE_PENALTY_MULTIPLIER: f64 = 5.0;
pub const MEDIUM_STAKE_PENALTY_MULTIPLIER: f64 = 2.0;
pub const HIGH_REPUTATION_DISCOUNT: f64 = 0.5;
pub const MEDIUM_REPUTATION_DISCOUNT: f64 = 0.7;
pub const LOW_REPUTATION_PENALTY: f64 = 2.0;
```

### Gas Calculation Constants

```rust
// Content pricing tiers
pub const SHORT_CONTENT_THRESHOLD: usize = 200;
pub const MEDIUM_CONTENT_THRESHOLD: usize = 1000;
pub const SHORT_CONTENT_GAS_RATE: u64 = 500;       // 0.0000005 TOS/byte
pub const MEDIUM_CONTENT_GAS_RATE: u64 = 1000;     // 0.000001 TOS/byte
pub const LONG_CONTENT_GAS_RATE: u64 = 2000;       // 0.000002 TOS/byte

// Minimum costs
pub const MIN_TRANSACTION_COST: u64 = 50_000;      // 0.00005 TOS
```

### Reward Calculation Constants

```rust
// Base rewards by difficulty
pub const BASIC_TASK_BASE_REWARD: u64 = 50_000_000;      // 0.05 TOS
pub const INTERMEDIATE_TASK_BASE_REWARD: u64 = 100_000_000;  // 0.1 TOS
pub const ADVANCED_TASK_BASE_REWARD: u64 = 200_000_000;  // 0.2 TOS
pub const EXPERT_TASK_BASE_REWARD: u64 = 500_000_000;    // 0.5 TOS

// Quality thresholds
pub const HIGH_QUALITY_SCORE_THRESHOLD: u8 = 90;
pub const MEDIUM_QUALITY_SCORE_THRESHOLD: u8 = 80;
pub const HIGH_QUALITY_SCARCITY_BONUS: f64 = 1.5;
pub const MEDIUM_QUALITY_SCARCITY_BONUS: f64 = 1.2;
```

## Configuration Parameters

### Security Configuration

```rust
pub struct SecurityConfig {
    // Reputation system
    pub min_reputation_for_expert: f64,
    pub reputation_calculation_weights: ReputationWeights,

    // Anti-Sybil parameters
    pub min_account_age_hours: u64,
    pub min_transaction_count: u64,
    pub min_stake_amount: u64,

    // Rate limiting
    pub base_cooldown_period: u64,
    pub reputation_cooldown_modifiers: Vec<(f64, u64)>,

    // Economic security
    pub min_transaction_cost: u64,
    pub stake_penalty_multipliers: Vec<(u64, f64)>,
    pub reputation_discount_tiers: Vec<(f64, f64)>,
}
```

### Production Recommended Settings

```toml
[security]
min_reputation_for_expert = 0.7
min_account_age_hours = 24
min_transaction_count = 5
min_stake_amount = 100000
base_cooldown_period = 3600
min_transaction_cost = 50000

[reputation_weights]
age_weight = 0.3
history_weight = 0.4
stake_weight = 0.3
validation_bonus_max = 0.2
long_term_bonus = 0.1
```

## Monitoring and Detection

### Security Metrics

The system tracks various security-related metrics:

```rust
pub struct SecurityMetrics {
    pub sybil_detections: u64,
    pub rejected_submissions: u64,
    pub reputation_penalties_applied: u64,
    pub average_reputation_score: f64,
    pub fee_increases_applied: u64,
    pub cooldown_violations: u64,
}
```

### Alert Conditions

- Unusual spike in new account registrations
- High rate of low-reputation submissions
- Patterns suggesting coordinated attacks
- Abnormal validation score distributions
- Economic anomalies in fee payments

### Audit Trail

All security-related actions are logged for analysis:

```rust
pub struct SecurityEvent {
    pub timestamp: u64,
    pub account: CompressedPublicKey,
    pub event_type: SecurityEventType,
    pub risk_score: f64,
    pub action_taken: RecommendedAction,
    pub details: String,
}
```

## Future Enhancements

### Planned Security Improvements

1. **Machine Learning Integration**
   - Behavioral pattern analysis
   - Anomaly detection algorithms
   - Predictive risk scoring

2. **Dynamic Parameter Adjustment**
   - Automatic threshold adjustment based on network conditions
   - Seasonal adaptation for varying participation levels
   - Emergency response capabilities

3. **Enhanced Validation**
   - Cross-validation between multiple validators
   - Consensus-based quality scoring
   - Proof-of-stake validation selection

4. **Advanced Economic Models**
   - Dynamic pricing based on network demand
   - Prediction markets for task difficulty
   - Insurance mechanisms for high-value tasks

## Conclusion

The TOS AI Mining security and reputation system provides comprehensive protection against common attack vectors while maintaining accessibility for legitimate users. The multi-layered approach ensures that attacks become economically unviable while rewarding honest participation with improved access and reduced costs.

The system is designed to be adaptive and upgradeable, allowing for continuous improvement based on real-world experience and emerging threats. Regular monitoring and analysis of security metrics will inform future enhancements and parameter adjustments.

---

**For Technical Support**: Refer to the [AI_MINER_API_REFERENCE.md](./AI_MINER_API_REFERENCE.md) for implementation details.

**For Integration**: See [QUICK_START_GUIDE.md](./QUICK_START_GUIDE.md) for practical examples.

**For System Status**: Check [AI_MINING_IMPLEMENTATION_STATUS.md](./AI_MINING_IMPLEMENTATION_STATUS.md) for current features.