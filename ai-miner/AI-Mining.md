# Tos AI Mining Rewards (Low-Frequency IoT Mining)

## Overview
This document describes the design of **low-frequency AI mining rewards** for IoT devices such as ESP32, Arduino, and Raspberry Pi.
AI mining does **not participate in block production or consensus**. Instead, it provides a parallel reward channel for devices that contribute *useful AI services* with **30-minute task cycles** and **daily reward distribution**.
The mechanism ensures rewards are **verifiable on-chain** while maintaining energy efficiency and extending device lifespan through low-frequency operation.

---

## High-Level Architecture

```
[Consensus Nodes] ─────► [Blockchain] ─────► Block Rewards

[IoT Devices] ──► 30min Task ──► [AI Work Proof] ──► [Blockchain]
(ESP32/Arduino)                                           │
                                                         ▼
                                              [Reward Accumulator]
                                                         │
                                              24h+ aged rewards
                                                         ▼
[Device Owner] ◄──── Self-Claim ◄──── [ClaimRewardTx] ◄──┘
```

**Components:**
- **Consensus Nodes**: produce blocks via Tos consensus (unaffected by IoT mining).
- **IoT Devices**: perform 30-minute AI tasks and submit proofs directly to blockchain.
- **Reward Accumulator**: blockchain automatically tracks device contributions over time.
- **Self-Claiming System**: device owners can claim any accumulated rewards older than 24 hours.
- **No Oracle Required**: fully decentralized design with on-chain proof verification.

---

## Key Components

### 1. Mining Cycles
- **Task Cycle**: 30 minutes - devices receive and complete AI tasks
- **Reward Cycle**: 24 hours - accumulated contributions are scored and rewards distributed
- **Daily Epochs**: 48 task periods per day (24h ÷ 30min = 48)

### 2. Device Classification
```rust
enum DeviceClass {
    // Microcontrollers (Ultra-low power)
    Arduino,        // Arduino Uno/Nano/Mega (~1-10 H/s)
    STM32,          // STM32 family (~5-20 H/s)

    // WiFi-enabled MCUs (Low power)
    ESP8266,        // NodeMCU, Wemos (~100-1K H/s)
    ESP32,          // ESP-WROOM, ESP32-CAM (~1-10 KH/s)

    // Advanced MCUs (Medium power)
    Teensy,         // Teensy 4.1 (~5-50 KH/s)

    // Single Board Computers (Higher power)
    RaspberryPi,    // Pi Zero/3/4/5 (~10-100 KH/s)
    OrangePi,       // Orange Pi variants (~20-80 KH/s)

    // PC/Server class (Highest power)
    PC,             // Desktop/laptop miners (~100KH-1MH/s)
}
```
- Each device has a **device key pair** and **verified capability class**
- `device_id = Hash(dev_pubkey)`
- Device registration includes owner mapping, device class, and hardware fingerprint

### 3. AI Task Types
```rust
enum AITask {
    CoverageProof {
        area_id: Hash,
        duration: 30min,     // Prove 30min online presence
    },
    RelayService {
        message_quota: u32,  // Relay N messages in 30min
        bandwidth_proof: Vec<u8>,
    },
    InferenceTask {
        model_hash: Hash,
        input_data: Vec<u8>,
        verification: Option<Hash>,
    },
}
```

### 4. Automatic Reward Calculation
- **Real-time Accumulation**: Rewards calculated instantly when proof submitted
- **Allocation Formula** (per 30-min epoch):
  ```rust
  epoch_reward = base_reward(device_class)
                * quality_multiplier(0.5-1.5)
                * consistency_bonus(1.0-1.1)
                * network_value_bonus(1.0-1.2)
  ```
- **24-hour Maturation**: Prevents gaming, allows challenge period
- **Self-Service Claiming**: No dependency on external services

### 5. On-Chain Reward Accumulation
```rust
// Blockchain automatically tracks device contributions
struct DeviceRewardTracker {
    device_id: DeviceId,
    pending_rewards: Vec<EpochReward>,
    total_accumulated: u64,
    last_claim_time: Timestamp,
}

struct EpochReward {
    epoch_timestamp: Timestamp,
    reward_amount: u64,
    task_count: u32,
    quality_score: f64,
    claimable_after: Timestamp,  // epoch_timestamp + 24h
}

// Direct proof submission to blockchain
struct AIWorkProofTx {
    device_id: DeviceId,
    task_epoch: u32,
    proof_data: ProofData,
    device_signature: Signature,
}
```

---

## Transaction Types

1. **RegisterDeviceTx**
   - Registers device: `(device_id, dev_pubkey, owner_addr, device_class)`
   - Optional staking requirement based on device class
   - Creates initial `DeviceRewardTracker` entry

2. **RotateDeviceKeyTx**
   - Rotates device public key for security
   - Increments `revocation_nonce` to prevent old key reuse

3. **TransferDeviceOwnerTx**
   - Transfers device ownership to new address
   - Maintains device history and accumulated rewards

4. **AIWorkProofTx** *(30-minute Submissions)*
   - Device submits proof of completed AI work
   - Blockchain automatically calculates and accumulates rewards
   - Rewards become claimable after 24-hour maturation period
   - No external Oracle required - all verification on-chain

5. **ClaimAccumulatedRewardsTx** *(Self-Service)*
   - Device owner claims all rewards older than 24 hours
   - Can claim multiple days of accumulated rewards at once
   - Requires device signature for security
   - Automatically transfers accumulated TOS to owner's balance

---

## Security Measures

### Anti-Sybil Protection
- **Device Class Verification**: Hardware fingerprinting to prevent virtualization
- **Staking Requirements**: Higher stakes for more capable devices
- **Geographic Distribution**: Location-based task assignment prevents concentration
- **Cross-Validation**: Multiple devices verify coverage proofs in same area

### Proof Integrity
- **Time-Bound Tasks**: 30-minute minimum presence requirement
- **Energy Consistency**: Power consumption patterns must match device class
- **Signature Chain**: Device signs each task completion
- **Temporal Verification**: Tasks must be completed within assigned time window

### Economic Security
- **Daily Caps**: Maximum rewards per device per day (prevents grinding)
- **Quality Scoring**: Poor performance reduces future task assignments
- **Reputation System**: Long-term device behavior affects reward multipliers
- **Slashing**: Detected cheating forfeits accumulated daily TOS rewards

---

## Economics

### Daily Reward Pool
```rust
struct DailyRewardPool {
    total_budget: u64,           // Fixed daily AI mining budget
    base_allocation: 60%,        // Participation rewards
    performance_bonus: 30%,      // Quality-based bonus
    network_value: 10%,          // Unique contribution bonus
}
```

### Device-Class Rewards
| Device Class | Daily Cap | Base Rate | Staking Required | Target Use Case | % of Block Mining |
|--------------|-----------|-----------|------------------|-----------------|------------------|
| Arduino      | 100 TOS   | 1x        | None            | Coverage proof  | 0.15%           |
| STM32        | 150 TOS   | 1.5x      | 200 TOS         | Coverage proof  | 0.22%           |
| ESP8266      | 300 TOS   | 2x        | 500 TOS         | Relay service   | 0.44%           |
| ESP32        | 600 TOS   | 3x        | 800 TOS         | Light inference | 0.88%           |
| Teensy       | 800 TOS   | 4x        | 1000 TOS        | Fast computation| 1.17%           |
| RaspberryPi  | 1200 TOS  | 6x        | 1500 TOS        | Full AI tasks   | 1.75%           |
| OrangePi     | 1000 TOS  | 5x        | 1200 TOS        | Full AI tasks   | 1.46%           |
| PC           | 2000 TOS  | 10x       | 3000 TOS        | Heavy inference | 2.92%           |

**Total Daily AI Mining Budget: ~8,150 TOS (~12% of daily block mining)**

### Incentive Mechanisms
- **Newcomer Boost**: 2x TOS rewards for first 7 days
- **Consistency Bonus**: +10% TOS for devices online >20 task periods/day
- **Network Growth**: Bonus TOS for devices in underserved geographic areas
- **Quality Multiplier**: 0.5x to 1.5x TOS based on task completion quality

---

## Implementation Roadmap

### Phase 1: Self-Claiming Foundation (MVP)
- **Blockchain Layer**:
  - Implement `AIWorkProofTx` and `ClaimAccumulatedRewardsTx`
  - Add `DeviceRewardTracker` state management
  - On-chain reward calculation and accumulation
- **Core Features**:
  - 30-minute proof submission system
  - Automatic reward calculation per proof
  - 24-hour maturation period
  - Self-service reward claiming
- **Testing**: Deploy on testnet with basic coverage proofs

### Phase 2: Device Integration & Security
- **Hardware Support**:
  - ESP32/Arduino firmware with cryptographic signatures
  - Device fingerprinting and class verification
  - Lightweight proof generation libraries
- **Security Enhancements**:
  - Anti-Sybil protection mechanisms
  - Quality scoring algorithms
  - Challenge/dispute mechanisms for fraudulent proofs

### Phase 3: Advanced AI Tasks
- **Task Expansion**:
  - Complex inference frameworks
  - Multi-device collaborative computation
  - Cross-validation networks
  - Geographic coverage requirements
- **Economic Optimization**:
  - Dynamic difficulty adjustment
  - Network-based bonus calculations
  - Reputation systems

### Phase 4: Full Ecosystem
- **Governance Integration**:
  - Community-controlled parameters
  - Dynamic reward adjustments via voting
  - Geographic incentive programs
- **Developer Tools**:
  - SDKs for new device types
  - Proof verification libraries
  - Analytics and monitoring tools

---

## Conclusion

This **low-frequency AI mining design** enables sustainable IoT participation in the Tos ecosystem through:

### Key Benefits
- **Energy Efficient**: 30-minute task cycles vs continuous mining
- **Device Longevity**: Reduced wear from low-frequency operation
- **Real Value Creation**: AI services instead of pure computational waste
- **Scalable Economics**: Daily reward distribution supports thousands of devices
- **Fair Distribution**: Multi-tier rewards based on device capabilities

### Differentiation from Traditional Mining
Unlike high-frequency proof-of-work systems, Tos AI mining focuses on **useful computation** with **sustainable participation**. The 30-minute/24-hour cycle ensures devices provide genuine AI services while maintaining economic viability for device owners.

This design positions Tos as the first blockchain to successfully integrate IoT devices for meaningful AI work while preserving the security and decentralization of the core consensus mechanism.
