# Contract Factory Usage Example

This document provides a step-by-step walkthrough of deploying and using the contract factory.

## Scenario

Alice wants to create a Token Factory that allows users to deploy their own ERC20 tokens with deterministic addresses.

## Step 1: Prepare Template Contract

First, Alice creates a simple token template:

```bash
cd ~/tos-network/tako/examples
mkdir -p token-template/src

# Create a simple ERC20 token contract
cat > token-template/src/lib.rs << 'EOF'
#![no_std]
#![no_main]

// Simple ERC20 token template
// This will be deployed via factory

extern "C" {
    fn tos_storage_read(key_ptr: *const u8, key_len: u64, value_ptr: *mut u8, value_len: u64) -> u64;
    fn tos_storage_write(key_ptr: *const u8, key_len: u64, value_ptr: *const u8, value_len: u64) -> u64;
}

const PREFIX_BALANCE: u8 = 0x01;
const PREFIX_TOTAL_SUPPLY: u8 = 0x02;

#[no_mangle]
pub extern "C" fn constructor() {
    // Initialize token with 1,000,000 supply to deployer
    unsafe {
        let total_supply: u64 = 1_000_000;
        let key = [PREFIX_TOTAL_SUPPLY];
        tos_storage_write(
            key.as_ptr(),
            1,
            &total_supply as *const u64 as *const u8,
            8,
        );
    }
}

#[no_mangle]
pub extern "C" fn balance_of() -> u64 {
    // Return balance (simplified)
    unsafe {
        let mut balance: u64 = 0;
        let key = [PREFIX_BALANCE];
        tos_storage_read(
            key.as_ptr(),
            1,
            &mut balance as *mut u64 as *mut u8,
            8,
        );
        balance
    }
}
EOF

# Build the template
cd token-template
cargo build --release --target tbpf-tos-tos
```

The compiled template is at:
```
target/tbpf-tos-tos/release/token_template.so
```

## Step 2: Deploy Factory Contract

Alice deploys the factory contract:

```bash
cd ~/tos-network/tako/examples/contract-factory

# Build factory
cargo build --release --target tbpf-tos-tos

# Deploy to TOS blockchain
tos-cli deploy \
  --bytecode target/tbpf-tos-tos/release/contract_factory.so \
  --wallet alice.key \
  --gas-limit 5000000

# Output:
# ✓ Factory deployed successfully
# Address: tos1qf4ct0ryadd4r3ss
# Transaction: 0x1234...
```

## Step 3: Configure Factory

Alice sets the template bytecode hash:

```bash
# Calculate bytecode hash
TEMPLATE_HASH=$(sha256sum ../token-template/target/tbpf-tos-tos/release/token_template.so | cut -d' ' -f1)
echo "Template hash: $TEMPLATE_HASH"

# Set template hash in factory
tos-cli call tos1qf4ct0ryadd4r3ss \
  --function set_template_hash \
  --args template_hash=0x$TEMPLATE_HASH \
  --wallet alice.key

# Set deployment fee (1 TOS)
tos-cli call tos1qf4ct0ryadd4r3ss \
  --function set_deployment_fee \
  --args fee=1000000000 \
  --wallet alice.key
```

## Step 4: Setup Off-Chain Service

Alice sets up the deployment service:

```bash
cd off-chain-service

# Create bytecode directory
mkdir -p bytecodes

# Copy template bytecode
cp ../../token-template/target/tbpf-tos-tos/release/token_template.so \
   bytecodes/token_template.so

# Build service
cargo build --release

# Create environment configuration
cat > .env << EOF
FACTORY_ADDRESS=tos1qf4ct0ryadd4r3ss
TOS_RPC_URL=http://localhost:8080
WALLET_PATH=../alice.key
BYTECODE_DIR=./bytecodes
EOF

# Run service
source .env
./target/release/factory-daemon
```

Output:
```
╔═══════════════════════════════════════════════╗
║   TOS Contract Factory Deployment Service    ║
╚═══════════════════════════════════════════════╝

Configuration:
  Factory: tos1qf4ct0ryadd4r3ss
  RPC URL: http://localhost:8080
  Wallet: ../alice.key
  Bytecode Dir: ./bytecodes

Loading bytecodes from: ./bytecodes
  ✓ Loaded: token_template.so (12584 bytes, hash: 3a7f2c1b...)
Total bytecodes loaded: 1

Connecting to TOS RPC: http://localhost:8080
Listening for DeploymentRequested events...

🚀 Service is running...
   Press Ctrl+C to stop

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

## Step 5: User Deploys Token

Bob wants to deploy his own token using Alice's factory:

```bash
# Bob generates a salt for his token
SALT=$(openssl rand -hex 32)
echo "Bob's salt: $SALT"

# Bob calls factory to request deployment
tos-cli call tos1qf4ct0ryadd4r3ss \
  --function request_deployment \
  --args salt=0x$SALT,bytecode_hash=0x$TEMPLATE_HASH \
  --value 1000000000 \
  --wallet bob.key

# Output:
# ✓ Deployment requested
# Predicted address: tos1b0bt0k3nadd4r3ss
# Event emitted: DeploymentRequested
```

## Step 6: Automatic Deployment

Alice's off-chain service automatically detects and processes the deployment:

```
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
📦 New Deployment Request
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Deployer: tos1b0bwadd4r3ss
Salt: 3f7a1e...
Bytecode Hash: 3a7f2c1b...
Predicted Address: tos1b0bt0k3nadd4r3ss

✓ Found bytecode in storage (12584 bytes)

  → Sending DeployContract transaction...
    Bytecode size: 12584 bytes
    Salt: 3f7a1e...
  ✓ Transaction sent: 0x3f7a1e...

  → Marking deployment as complete...
    Salt: 3f7a1e...
  ✓ Deployment marked as complete

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
✅ Deployment Completed Successfully
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Transaction: 0x3f7a1e...
Contract Address: tos1b0bt0k3nadd4r3ss

```

## Step 7: Verify Deployment

Bob verifies his token was deployed:

```bash
# Check if deployment completed
tos-cli call tos1qf4ct0ryadd4r3ss \
  --function is_deployed \
  --args salt=0x$SALT

# Output: 1 (deployed)

# Get contract at predicted address
tos-cli get-contract tos1b0bt0k3nadd4r3ss

# Output:
# ✓ Contract found
# Address: tos1b0bt0k3nadd4r3ss
# Bytecode Hash: 3a7f2c1b...
# Deployed At: Block 12345

# Call token contract
tos-cli call tos1b0bt0k3nadd4r3ss \
  --function balance_of \
  --args account=tos1b0bwadd4r3ss

# Output: 1000000 (initial supply)
```

## Step 8: Another User Deploys

Charlie deploys his own token with a different salt:

```bash
# Charlie's salt
CHARLIE_SALT=$(openssl rand -hex 32)

# Request deployment
tos-cli call tos1qf4ct0ryadd4r3ss \
  --function request_deployment \
  --args salt=0x$CHARLIE_SALT,bytecode_hash=0x$TEMPLATE_HASH \
  --value 1000000000 \
  --wallet charlie.key

# Alice's service automatically deploys Charlie's token
# Charlie's token is deployed to: tos1char1i3st0k3nadd4r3ss
```

## Step 9: Check Factory Statistics

Anyone can check factory statistics:

```bash
# Get total deployments
tos-cli call tos1qf4ct0ryadd4r3ss \
  --function get_deployment_count

# Output: 2 (Bob's + Charlie's tokens)

# Get deployment fee
tos-cli call tos1qf4ct0ryadd4r3ss \
  --function get_deployment_fee

# Output: 1000000000 (1 TOS)
```

## Advanced: Multi-Template Factory

Alice can extend the factory to support multiple templates:

```bash
# Deploy NFT template
cd ~/tos-network/tako/examples
# ... create and build nft-template ...

# Copy to service bytecode directory
cp nft-template/target/tbpf-tos-tos/release/nft_template.so \
   contract-factory/off-chain-service/bytecodes/

# Restart service (it will auto-load the new template)
# Users can now deploy NFT contracts by specifying the NFT bytecode hash
```

## Security Best Practices

### For Factory Owners (Alice)

1. **Secure the owner key**:
   ```bash
   chmod 600 alice.key
   # Store in secure location
   # Use hardware wallet if available
   ```

2. **Monitor the service**:
   ```bash
   # Add logging
   ./target/release/factory-daemon > factory.log 2>&1

   # Monitor logs
   tail -f factory.log

   # Set up alerts for failures
   ```

3. **Backup configuration**:
   ```bash
   # Backup bytecodes
   tar -czf bytecodes-backup.tar.gz bytecodes/

   # Backup wallet
   cp alice.key alice.key.backup
   ```

### For Users (Bob, Charlie)

1. **Verify factory code**:
   ```bash
   # Get factory bytecode
   tos-cli get-contract tos1qf4ct0ryadd4r3ss --output bytecode

   # Verify it matches published hash
   sha256sum factory_bytecode.so
   ```

2. **Check template hash**:
   ```bash
   # Verify template bytecode hash before deployment
   echo $TEMPLATE_HASH
   # Compare with Alice's published hash
   ```

3. **Use unique salts**:
   ```bash
   # Always generate random salts
   SALT=$(openssl rand -hex 32)

   # Never reuse salts
   ```

## Troubleshooting

### Service Not Detecting Events

```bash
# Check service is running
ps aux | grep factory-daemon

# Check RPC connection
curl http://localhost:8080/health

# Check factory address is correct
echo $FACTORY_ADDRESS
```

### Deployment Not Completing

```bash
# Check service logs
tail -n 50 factory.log

# Check wallet has sufficient balance
tos-cli balance tos1alice...

# Check bytecode exists
ls -lh bytecodes/*.so
```

### Wrong Predicted Address

```bash
# Verify salt matches
# Verify bytecode_hash matches
# Verify factory address matches

# Predicted address is deterministic:
# address = keccak256(0xFF || factory || salt || bytecode_hash)
```

## Next Steps

- **Scale the service**: Run multiple instances for redundancy
- **Add monitoring**: Integrate with Prometheus/Grafana
- **Support upgrades**: Allow template version updates
- **Add governance**: Let token holders vote on factory changes
- **Create web UI**: Build a frontend for easy token deployment

## Summary

This example demonstrated:
1. ✅ Deploying a factory contract
2. ✅ Running an off-chain deployment service
3. ✅ Users requesting contract deployments
4. ✅ Automatic deployment with deterministic addresses
5. ✅ Verification and usage of deployed contracts

The factory pattern enables:
- 🎯 Deterministic contract addresses (like CREATE2)
- 🚀 Easy deployment for non-technical users
- 🔒 Secure by design (no in-contract deployment)
- 📦 Template reusability
- 💰 Optional monetization (deployment fees)
