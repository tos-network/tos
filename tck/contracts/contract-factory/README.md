# Contract Factory Example

A production-ready factory pattern for deploying contracts on TOS blockchain, similar to Ethereum's CREATE2 opcode.

## 📋 Table of Contents

- [Overview](#overview)
- [Architecture](#architecture)
- [Features](#features)
- [Quick Start](#quick-start)
- [Usage Guide](#usage-guide)
- [Off-Chain Service](#off-chain-service)
- [Security Considerations](#security-considerations)
- [API Reference](#api-reference)

## Overview

Since TAKO VM doesn't support in-contract deployment (by design for security and simplicity), this factory uses an **event-driven off-chain deployment service pattern**.

### Why Not In-Contract Deployment?

TAKO VM intentionally doesn't provide CREATE/CREATE2 syscalls because:

1. ✅ **Security**: Prevents reentrancy attacks and gas bombs
2. ✅ **Simplicity**: Contract creation is handled at blockchain layer
3. ✅ **Gas Control**: Deployment costs are predictable and transparent
4. ✅ **State Management**: Cleaner state transitions without nested deployments

### How Factory Pattern Works

```
┌─────────────────────────────────────────────────────────────┐
│ 1. User calls Factory.request_deployment(salt, bytecode_hash)│
└───────────────────────┬─────────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────────────────┐
│ 2. Factory emits "DeploymentRequested" event                 │
│    - Computes predicted address using CREATE2 formula        │
│    - Stores deployment record on-chain                       │
└───────────────────────┬─────────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────────────────┐
│ 3. Off-chain service listens for events                      │
│    - Watches blockchain for DeploymentRequested events       │
│    - Loads bytecode from local storage/IPFS                  │
└───────────────────────┬─────────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────────────────┐
│ 4. Off-chain service sends DeployContract transaction        │
│    - Uses TOS blockchain's native deployment mechanism       │
│    - Contract deployed to predicted address                  │
└───────────────────────┬─────────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────────────────┐
│ 5. Service calls Factory.mark_deployed(salt)                 │
│    - Updates on-chain record to mark deployment complete     │
│    - Emits "DeploymentCompleted" event                       │
└─────────────────────────────────────────────────────────────┘
```

## Architecture

### Components

1. **Factory Contract** (on-chain):
   - Stores deployment records
   - Computes CREATE2-style addresses
   - Emits events for off-chain service
   - Tracks deployment status

2. **Off-Chain Service** (daemon):
   - Listens for DeploymentRequested events
   - Loads contract bytecode
   - Sends DeployContract transactions
   - Marks deployments as completed

3. **Bytecode Storage** (IPFS/local):
   - Stores template contract bytecode
   - Indexed by bytecode hash

### Address Calculation (CREATE2-compatible)

```rust
address = keccak256(0xFF || factory_address || salt || bytecode_hash)
```

This is **identical** to Ethereum's CREATE2, ensuring:
- ✅ Deterministic addresses
- ✅ Cross-chain compatibility
- ✅ Same address for same inputs

## Features

- ✅ **Deterministic Addresses**: CREATE2-style address calculation
- ✅ **Gas Efficient**: No need to store bytecode on-chain
- ✅ **Flexible**: Works with any contract template
- ✅ **Secure**: Prevents deployment to already-used addresses
- ✅ **Auditable**: All deployments recorded on-chain
- ✅ **Fee Support**: Optional deployment fees
- ✅ **Owner Controls**: Only factory owner can update settings

## Quick Start

### 1. Build the Factory Contract

```bash
cd examples/contract-factory
cargo build --release --target tbpf-tos-tos
```

The compiled contract will be at:
```
target/tbpf-tos-tos/release/contract_factory.so
```

### 2. Deploy Factory to TOS

```bash
# Deploy factory contract
tos-cli deploy \
  --bytecode target/tbpf-tos-tos/release/contract_factory.so \
  --wallet factory-owner.key

# Output:
# Factory deployed to: tos1factory_address_here
```

### 3. Set Template Bytecode Hash

```bash
# First, hash your template contract
TEMPLATE_HASH=$(sha256sum my-token-template.so | cut -d' ' -f1)

# Set template hash in factory
tos-cli call tos1factory_address_here \
  --function set_template_hash \
  --args template_hash=$TEMPLATE_HASH \
  --wallet factory-owner.key
```

### 4. Run Off-Chain Deployment Service

```bash
# Build the service (see off-chain-service/ directory)
cd off-chain-service
cargo build --release

# Run the service
./target/release/factory-daemon \
  --factory-address tos1factory_address_here \
  --rpc-url http://localhost:8080 \
  --wallet factory-owner.key \
  --bytecode-dir ./bytecodes/
```

### 5. Request Deployment

```bash
# User requests deployment with custom salt
tos-cli call tos1factory_address_here \
  --function request_deployment \
  --args salt=0x1234...,bytecode_hash=$TEMPLATE_HASH \
  --wallet user.key

# Output:
# Predicted address: tos1predicted_contract_address
# Event emitted: DeploymentRequested

# Off-chain service will automatically deploy the contract
```

### 6. Verify Deployment

```bash
# Check if deployment completed
tos-cli call tos1factory_address_here \
  --function is_deployed \
  --args salt=0x1234...

# Output: 1 (deployed) or 0 (pending)

# Get contract at predicted address
tos-cli get-contract tos1predicted_contract_address
```

## Usage Guide

### For Factory Owners

#### Set Deployment Fee

```bash
tos-cli call tos1factory_address \
  --function set_deployment_fee \
  --args fee=1000000000 \  # 1 TOS in nanoTOS
  --wallet factory-owner.key
```

#### Update Template

```bash
tos-cli call tos1factory_address \
  --function set_template_hash \
  --args template_hash=0xnew_hash... \
  --wallet factory-owner.key
```

### For Users

#### Request Deployment

```rust
// Rust SDK example
use tos_sdk::{TosClient, ContractCall};

let client = TosClient::new("http://localhost:8080").await?;

let salt: [u8; 32] = [1, 2, 3, ...]; // Your custom salt
let bytecode_hash: [u8; 32] = [...]; // Hash of the contract to deploy

let tx = client
    .call_contract("tos1factory_address")
    .function("request_deployment")
    .args(&[
        ("salt", &salt),
        ("bytecode_hash", &bytecode_hash),
    ])
    .value(1_000_000_000) // Include deployment fee if required
    .sign(&user_wallet)
    .await?;

let result = client.send_transaction(tx).await?;
println!("Predicted address: {}", result.predicted_address);
```

#### Check Deployment Status

```bash
# Get deployment count
tos-cli call tos1factory_address --function get_deployment_count

# Check specific deployment
tos-cli call tos1factory_address \
  --function is_deployed \
  --args salt=0x1234...
```

## Off-Chain Service

The off-chain deployment service is a critical component. Here's a minimal implementation:

### Directory Structure

```
off-chain-service/
├── Cargo.toml
├── src/
│   ├── main.rs          # Service entry point
│   ├── event_listener.rs # Event monitoring
│   ├── deployer.rs      # Deployment logic
│   └── storage.rs       # Bytecode storage
└── bytecodes/           # Template bytecode directory
    ├── token.so
    ├── nft.so
    └── ...
```

### Minimal Implementation

```rust
// off-chain-service/src/main.rs

use tos_sdk::{TosClient, Event, Transaction};
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let factory_address = std::env::var("FACTORY_ADDRESS")?;
    let rpc_url = std::env::var("TOS_RPC_URL")?;
    let wallet_path = std::env::var("WALLET_PATH")?;

    let client = TosClient::new(&rpc_url).await?;
    let wallet = load_wallet(&wallet_path)?;

    // Load bytecode templates
    let bytecodes = load_bytecodes("./bytecodes")?;

    println!("Factory Daemon started");
    println!("Factory: {}", factory_address);
    println!("Wallet: {}", wallet.address());
    println!("Loaded {} bytecode templates", bytecodes.len());

    // Listen for deployment events
    let mut event_stream = client
        .subscribe_events(&factory_address, "DeploymentRequested")
        .await?;

    while let Some(event) = event_stream.next().await {
        match handle_deployment(
            &client,
            &wallet,
            &factory_address,
            &bytecodes,
            event,
        ).await {
            Ok(address) => {
                println!("✓ Deployed contract to {}", address);
            }
            Err(e) => {
                eprintln!("✗ Deployment failed: {}", e);
            }
        }
    }

    Ok(())
}

async fn handle_deployment(
    client: &TosClient,
    wallet: &Wallet,
    factory_address: &str,
    bytecodes: &HashMap<[u8; 32], Vec<u8>>,
    event: Event,
) -> Result<String, Box<dyn std::error::Error>> {
    // Parse event data
    let deployer: [u8; 32] = event.get_data(0..32)?;
    let salt: [u8; 32] = event.get_data(32..64)?;
    let bytecode_hash: [u8; 32] = event.get_data(64..96)?;
    let predicted_address: [u8; 32] = event.get_data(96..128)?;

    println!("Deployment requested:");
    println!("  Deployer: {}", hex::encode(deployer));
    println!("  Salt: {}", hex::encode(salt));
    println!("  Predicted: {}", hex::encode(predicted_address));

    // Get bytecode
    let bytecode = bytecodes
        .get(&bytecode_hash)
        .ok_or("Bytecode not found")?;

    // Deploy contract
    let deploy_tx = client
        .build_deploy_transaction()
        .bytecode(bytecode.clone())
        .salt(salt)
        .sign(wallet)
        .await?;

    let tx_hash = client.send_transaction(deploy_tx).await?;
    println!("  Deploy TX: {}", tx_hash);

    // Wait for confirmation
    client.wait_for_confirmation(&tx_hash, 10).await?;

    // Mark as deployed in factory
    let mark_tx = client
        .call_contract(factory_address)
        .function("mark_deployed")
        .args(&[("salt", &salt)])
        .sign(wallet)
        .await?;

    client.send_transaction(mark_tx).await?;

    Ok(hex::encode(predicted_address))
}

fn load_bytecodes(dir: &str) -> Result<HashMap<[u8; 32], Vec<u8>>, std::io::Error> {
    let mut bytecodes = HashMap::new();

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("so") {
            let bytecode = std::fs::read(&path)?;
            let hash = sha256(&bytecode);
            bytecodes.insert(hash, bytecode);

            println!("Loaded bytecode: {} ({})",
                path.display(),
                hex::encode(hash)
            );
        }
    }

    Ok(bytecodes)
}
```

### Running the Service

```bash
# Set environment variables
export FACTORY_ADDRESS=tos1factory_address_here
export TOS_RPC_URL=http://localhost:8080
export WALLET_PATH=./factory-owner.key

# Run the service
cargo run --release

# Output:
# Factory Daemon started
# Factory: tos1factory...
# Wallet: tos1owner...
# Loaded 3 bytecode templates
# Listening for deployment events...
```

## Security Considerations

### For Factory Owners

1. **Protect Owner Key**: The owner key can:
   - Update template bytecode hash
   - Change deployment fee
   - Mark deployments as completed

2. **Validate Bytecode**: Ensure template bytecode is:
   - Properly audited
   - Free of malicious code
   - Compatible with TAKO VM

3. **Monitor Service**: Off-chain service should:
   - Log all deployments
   - Alert on failures
   - Have backup/redundancy

### For Users

1. **Verify Factory**: Before using a factory:
   - Check factory owner reputation
   - Verify template bytecode hash
   - Review deployment records

2. **Salt Selection**: Choose salts carefully:
   - Use random salts for privacy
   - Use predictable salts for upgradeability
   - Never reuse salts

3. **Fee Awareness**: Check deployment fee:
   ```bash
   tos-cli call tos1factory --function get_deployment_fee
   ```

### Attack Vectors

| Attack | Mitigation |
|--------|------------|
| **Front-running** | Salts are user-specific, front-running doesn't benefit attacker |
| **Address squatting** | Factory prevents redeployment to same address |
| **Malicious bytecode** | Users can verify bytecode_hash before deployment |
| **Service downtime** | Anyone can run the off-chain service (code is open) |
| **Owner misbehavior** | Users can verify factory code on-chain |

## API Reference

### Constructor

```rust
fn constructor()
```

Initializes the factory with caller as owner.

### set_template_hash (owner only)

```rust
fn set_template_hash(template_hash: [u8; 32])
```

Sets the bytecode hash for the template contract.

**Parameters**:
- `template_hash`: SHA-256 hash of the contract bytecode

**Requires**: Caller must be factory owner

### set_deployment_fee (owner only)

```rust
fn set_deployment_fee(fee: u64)
```

Sets the deployment fee in nanoTOS.

**Parameters**:
- `fee`: Fee amount (max 10 TOS = 10_000_000_000 nanoTOS)

**Requires**: Caller must be factory owner

### request_deployment

```rust
fn request_deployment(salt: [u8; 32], bytecode_hash: [u8; 32]) -> [u8; 32]
```

Requests deployment of a new contract.

**Parameters**:
- `salt`: 32-byte salt for deterministic address
- `bytecode_hash`: Hash of bytecode to deploy

**Returns**: Predicted contract address

**Events**: Emits `DeploymentRequested` event

**Requires**: Send deployment fee with transaction (if fee > 0)

### mark_deployed (owner only)

```rust
fn mark_deployed(salt: [u8; 32])
```

Marks a deployment as completed.

**Parameters**:
- `salt`: Salt used in deployment request

**Requires**: Caller must be factory owner

**Events**: Emits `DeploymentCompleted` event

### get_deployment_count (view)

```rust
fn get_deployment_count() -> u64
```

Returns total number of deployment requests.

**Returns**: Deployment count

### get_deployment_fee (view)

```rust
fn get_deployment_fee() -> u64
```

Returns current deployment fee.

**Returns**: Fee in nanoTOS

### is_deployed (view)

```rust
fn is_deployed(salt: [u8; 32]) -> bool
```

Checks if deployment is completed.

**Parameters**:
- `salt`: Salt to check

**Returns**: `true` if deployed, `false` otherwise

## Events

### DeploymentRequested

```
topic: "DeploymentRequested"
data:
  - deployer: [u8; 32]       // Who requested
  - salt: [u8; 32]           // Deployment salt
  - bytecode_hash: [u8; 32]  // Bytecode to deploy
  - predicted_address: [u8; 32] // Where it will be deployed
```

### DeploymentCompleted

```
topic: "DeploymentCompleted"
data:
  - salt: [u8; 32]  // Deployment salt
```

## Examples

See `examples/` directory for:
- Token factory example
- NFT factory example
- Multi-template factory

## License

MIT License - see LICENSE file for details
