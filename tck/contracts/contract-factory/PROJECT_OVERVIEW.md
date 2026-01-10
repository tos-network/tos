# Contract Factory - Project Overview

## 📁 Project Structure

```
contract-factory/
├── Cargo.toml                  # Factory contract manifest
├── src/
│   └── lib.rs                  # Factory contract implementation (~800 lines)
│
├── off-chain-service/          # Deployment service
│   ├── Cargo.toml
│   ├── src/
│   │   └── main.rs             # Service implementation (~400 lines)
│   └── bytecodes/              # Template bytecode storage (gitignored)
│
├── README.md                   # Comprehensive documentation
├── USAGE_EXAMPLE.md            # Step-by-step usage guide
├── PROJECT_OVERVIEW.md         # This file
├── test-factory.sh             # Automated test script
└── .gitignore                  # Git ignore rules

```

## 🎯 What This Example Demonstrates

### Core Concepts

1. **Factory Pattern on TAKO VM**
   - How to implement contract deployment factory
   - CREATE2-style deterministic address calculation
   - Event-driven off-chain deployment service

2. **Why Not In-Contract Deployment?**
   - TAKO VM intentionally doesn't provide CREATE/CREATE2 syscalls
   - Security: Prevents reentrancy attacks and gas bombs
   - Simplicity: Contract creation at blockchain layer
   - Gas Control: Predictable deployment costs

3. **Off-Chain Service Architecture**
   - Event-driven deployment automation
   - Bytecode storage and management
   - Transaction submission and verification

### Key Features Implemented

- ✅ **Deterministic Addresses**: CREATE2-compatible address calculation
- ✅ **Event Emission**: On-chain events for off-chain service
- ✅ **Storage Management**: Deployment records and state tracking
- ✅ **Access Control**: Owner-only administrative functions
- ✅ **Fee Support**: Optional deployment fees
- ✅ **Status Tracking**: Check deployment completion status
- ✅ **No Bytecode Storage**: Gas-efficient (bytecode stored off-chain)

## 📚 Documentation

### For Users

| Document | Purpose | Audience |
|----------|---------|----------|
| [README.md](README.md) | Complete reference guide | Everyone |
| [USAGE_EXAMPLE.md](USAGE_EXAMPLE.md) | Step-by-step walkthrough | New users |
| [PROJECT_OVERVIEW.md](PROJECT_OVERVIEW.md) | High-level overview | Decision makers |

### For Developers

| File | Description | Lines |
|------|-------------|-------|
| `src/lib.rs` | Factory contract implementation | ~800 |
| `off-chain-service/src/main.rs` | Deployment service | ~400 |
| `test-factory.sh` | Automated testing | ~200 |

## 🚀 Quick Start

### 1-Minute Demo

```bash
# Build everything and run tests
cd examples/contract-factory
./test-factory.sh
```

### 5-Minute Walkthrough

See [USAGE_EXAMPLE.md](USAGE_EXAMPLE.md) for a complete scenario with Alice (factory owner) and Bob (user).

### Production Deployment

See [README.md](README.md) for detailed deployment instructions.

## 🏗️ Architecture

### Components Interaction

```
┌─────────────────┐
│  User Wallet    │
└────────┬────────┘
         │ 1. request_deployment(salt, bytecode_hash)
         │    + deployment fee
         ▼
┌─────────────────┐
│ Factory Contract│ (on-chain)
│  - Compute      │
│    CREATE2      │
│    address      │
│  - Store record │
│  - Emit event   │
└────────┬────────┘
         │ 2. DeploymentRequested event
         │    {deployer, salt, bytecode_hash, predicted_address}
         ▼
┌─────────────────┐
│ Off-Chain       │
│ Service         │ (watching blockchain)
│  - Listen for   │
│    events       │
│  - Load         │
│    bytecode     │
│  - Send deploy  │
│    transaction  │
└────────┬────────┘
         │ 3. DeployContract transaction
         │    + bytecode
         ▼
┌─────────────────┐
│ TOS Blockchain  │
│  - Validate     │
│  - Deploy to    │
│    predicted    │
│    address      │
└────────┬────────┘
         │ 4. Contract deployed
         ▼
┌─────────────────┐
│ Off-Chain       │
│ Service         │
│  - Verify       │
│  - Call         │
│    mark_deployed│
└────────┬────────┘
         │ 5. mark_deployed(salt)
         ▼
┌─────────────────┐
│ Factory Contract│
│  - Update       │
│    record       │
│  - Emit event   │
└─────────────────┘
         │ 6. DeploymentCompleted event
         ▼
     Success! ✅
```

## 🔍 Code Highlights

### Factory Contract (src/lib.rs)

**Key Functions:**

1. **request_deployment()** - Users call this to deploy contracts
   ```rust
   // Computes CREATE2 address
   // Stores deployment record
   // Emits event for off-chain service
   ```

2. **compute_create2_address()** - Deterministic address calculation
   ```rust
   address = keccak256(0xFF || factory || salt || bytecode_hash)
   ```

3. **mark_deployed()** - Off-chain service marks deployment complete
   ```rust
   // Updates on-chain record
   // Emits completion event
   ```

**Storage Layout:**
- Deployment records: `PREFIX_DEPLOYMENT_RECORD || salt -> DeploymentRecord`
- Owner: `PREFIX_OWNER -> [u8; 32]`
- Template hash: `PREFIX_TEMPLATE_HASH -> [u8; 32]`
- Deployment count: `PREFIX_DEPLOYMENT_COUNT -> u64`
- Deployment fee: `PREFIX_DEPLOYMENT_FEE -> u64`

### Off-Chain Service (off-chain-service/src/main.rs)

**Key Components:**

1. **Event Listener** - Watches for DeploymentRequested events
2. **Bytecode Storage** - Loads and indexes contract bytecode
3. **Deployment Handler** - Sends DeployContract transactions
4. **Status Updater** - Marks deployments as complete

**Configuration:**
```bash
FACTORY_ADDRESS=tos1...     # Factory contract address
TOS_RPC_URL=http://...      # TOS RPC endpoint
WALLET_PATH=./owner.key     # Owner wallet for marking deployed
BYTECODE_DIR=./bytecodes    # Template bytecode directory
```

## 🎓 Learning Objectives

After studying this example, you will understand:

1. ✅ **Factory Pattern** - How to implement contract factories on TAKO VM
2. ✅ **Event-Driven Architecture** - Off-chain services listening to on-chain events
3. ✅ **CREATE2 Addresses** - Deterministic address calculation
4. ✅ **Storage Patterns** - Efficient on-chain data structures
5. ✅ **Access Control** - Owner-based permissions
6. ✅ **Gas Optimization** - Storing bytecode off-chain
7. ✅ **Service Design** - Reliable off-chain automation

## 🔒 Security Considerations

### Design Decisions

| Security Feature | Why It Matters |
|------------------|----------------|
| **No in-contract deployment** | Prevents reentrancy and gas bombs |
| **Owner-only mark_deployed** | Prevents false deployment claims |
| **Deployment record checks** | Prevents address collisions |
| **Bytecode hash verification** | Users know exactly what's deployed |
| **Event emission** | Transparency and auditability |

### Threat Model

| Threat | Mitigation |
|--------|------------|
| Front-running | Salts are user-specific |
| Address squatting | Factory prevents redeployment |
| Service downtime | Anyone can run the service |
| Malicious bytecode | Users verify hash before deployment |
| Owner misbehavior | Factory code is auditable on-chain |

## 📊 Comparison with Ethereum

| Feature | Ethereum CREATE2 | TAKO Factory |
|---------|------------------|--------------|
| **Address Calculation** | In EVM | In Factory Contract |
| **Deployment** | In EVM opcode | Via Transaction |
| **Bytecode Storage** | On-chain | Off-chain |
| **Gas Cost** | ~32000 + bytecode | Configurable fee |
| **Reentrancy Risk** | Yes | No |
| **Service Required** | No | Yes |
| **Deterministic** | Yes | Yes |

**Advantages of TAKO Factory:**
- ✅ More secure (no reentrancy in deployment)
- ✅ More flexible (can add custom logic)
- ✅ Gas efficient (bytecode off-chain)
- ✅ Auditable (all deployments recorded)

**Trade-offs:**
- ⚠️ Requires off-chain service
- ⚠️ Slight delay for deployment
- ⚠️ Service availability dependency

## 🛠️ Extending the Example

### Ideas for Enhancement

1. **Multi-Template Support**
   - Support multiple contract templates
   - Users specify which template to use
   - Different fees per template

2. **Deployment Governance**
   - Token holders vote on template updates
   - Multisig for administrative functions
   - Time-locks for sensitive operations

3. **Advanced Features**
   - Constructor arguments support
   - Initial value transfers
   - Batch deployments
   - Deployment scheduling

4. **Monitoring & Analytics**
   - Deployment statistics dashboard
   - Gas cost tracking
   - User activity metrics
   - Template popularity

5. **Integration Examples**
   - Token factory (ERC20)
   - NFT factory (ERC721)
   - DEX pair factory (Uniswap-style)
   - DAO factory

## 📈 Production Readiness

### Checklist Before Production

- [ ] Security audit of factory contract
- [ ] Security audit of off-chain service
- [ ] Load testing (concurrent deployments)
- [ ] Failure recovery testing
- [ ] Documentation review
- [ ] Emergency shutdown mechanism
- [ ] Monitoring and alerting setup
- [ ] Backup and disaster recovery plan

### Recommended Setup

**For Factory Owners:**
1. Run service on reliable infrastructure (3+ nodes)
2. Implement health checks and auto-restart
3. Set up monitoring (Prometheus + Grafana)
4. Configure alerting (PagerDuty, Slack)
5. Maintain bytecode backups
6. Use hardware wallet for owner key

**For Users:**
1. Verify factory code on-chain
2. Check factory reputation
3. Test with small amounts first
4. Monitor deployment status
5. Verify deployed contracts

## 🤝 Contributing

Ideas for community contributions:

1. **Templates**
   - ERC20 token template
   - ERC721 NFT template
   - Governance token template
   - Staking contract template

2. **Tools**
   - Web UI for factory interaction
   - CLI tool for deployment
   - Monitoring dashboard
   - Deployment explorer

3. **Documentation**
   - Video tutorials
   - More usage examples
   - Translation to other languages
   - FAQ section

## 📞 Support

- **Issues**: Open GitHub issue with "contract-factory" label
- **Questions**: TOS Discord #tako-vm channel
- **Documentation**: See README.md and USAGE_EXAMPLE.md

## 📄 License

This example is provided as-is for educational and demonstration purposes.

MIT License - See LICENSE file for details

---

**Last Updated**: 2025-11-25
**TAKO VM Version**: 0.2.1
**Status**: Production Example
