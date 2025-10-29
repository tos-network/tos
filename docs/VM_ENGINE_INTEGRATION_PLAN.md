# TOS VM Engine Integration Plan: TBPF (Solana-style eBPF VM)

**Date**: 2025-10-29
**Status**: Planning Phase
**Goal**: Integrate Solana's sBPF execution engine (adapted as TBPF) into TOS blockchain

---

## Executive Summary

TOS currently has a custom VM implementation in the `tos-vm` repository. To support Solana-compatible smart contracts, we need to integrate a TBPF (TOS Berkeley Packet Filter) execution engine based on Solana's sBPF.

This document outlines the integration strategy, implementation steps, and technical considerations.

---

## Current TOS VM Architecture

### Components

1. **tos-vm** (https://github.com/tos-network/tos-vm)
   - Core VM execution engine
   - Module loading and execution
   - Gas metering system
   - Context management

2. **tos-builder**
   - Environment builder for contract execution
   - Native function registration
   - Opaque type system

3. **tos-environment**
   - Runtime environment with stdlib
   - Syscall implementations

### Current Execution Flow

```rust
// File: common/src/transaction/verify/contract.rs:59-133

1. Create VM instance
   let mut vm = VM::new(contract_environment.environment);

2. Load contract module (bytecode)
   vm.append_module(contract_environment.module)?;

3. Invoke entry point
   vm.invoke_entry_chunk(entry)?;

4. Execute with gas limit
   context.set_gas_limit(max_gas);
   let result = vm.run();

5. Process results and gas refunds
   let gas_usage = vm.context().current_gas_usage().min(max_gas);
```

### Integration Points

**Transaction Types**:
- `DeployContract`: Deploys contract bytecode as `Module`
- `InvokeContract`: Executes contract with parameters and deposits

**State Management**:
- `BlockchainVerificationState`: Read-only state queries
- `BlockchainApplyState`: State mutations during execution
- `ContractProvider`: Abstract storage interface

---

## Solana sBPF Architecture

### Core Concepts

1. **sBPF Bytecode**: ELF binary with custom relocations
2. **Register-based VM**: 11 64-bit registers (r0-r10)
3. **Memory Model**: Stack + heap with bounds checking
4. **Syscalls**: Predefined functions for blockchain operations
5. **Compute Units**: Gas-equivalent metering (200k default, 1.4M max)

### Key Differences from Current TOS VM

| Aspect | Current TOS VM | Solana sBPF |
|--------|---------------|-------------|
| Execution Model | Stack-based (likely) | Register-based |
| Bytecode Format | Custom Module | ELF binary with sBPF |
| Calling Convention | Entry chunks + hooks | `entrypoint()` function |
| State Access | Opaque types + providers | Account data slices |
| Gas Metering | Gas units | Compute units (CU) |

### Solana Execution Flow

```
1. Transaction submitted with program_id + accounts + instruction_data
2. Runtime loads program (BPF bytecode) from program_id account
3. BPF Loader initializes VM with program bytecode
4. VM executes entrypoint(program_id, accounts, instruction_data)
5. Program uses syscalls (sol_log, sol_invoke_signed, etc.)
6. Runtime applies account mutations
7. Return success/error + compute units consumed
```

---

## Integration Strategy

### Option 1: Replace Current VM (High Risk, High Reward)

**Approach**: Completely replace `tos-vm` with TBPF engine

**Pros**:
- Native Solana compatibility
- Battle-tested VM (sBPF is production-proven)
- Better performance (register-based VM)

**Cons**:
- Breaking change for existing TOS contracts
- Requires rewriting all contract stdlib
- High migration risk

**Recommendation**: ❌ **Not recommended** for existing production network

---

### Option 2: Dual-VM Support (Recommended)

**Approach**: Support both current VM and TBPF side-by-side

**Architecture**:
```rust
// File: common/src/transaction/payload/contract/deploy.rs

pub enum ContractModule {
    // Current TOS VM bytecode
    TosVM(tos_vm::Module),

    // New TBPF bytecode
    TBPF(Vec<u8>),  // ELF binary
}

pub struct DeployContractPayload {
    pub module: ContractModule,
    pub invoke: Option<InvokeConstructorPayload>,
}
```

**Execution Dispatch**:
```rust
// File: common/src/transaction/verify/contract.rs

match contract_module {
    ContractModule::TosVM(module) => {
        // Current execution path
        let mut vm = VM::new(environment);
        vm.append_module(module)?;
        vm.run()
    }
    ContractModule::TBPF(elf_bytes) => {
        // New TBPF execution path
        let vm = TbpfVM::new(elf_bytes)?;
        vm.execute(entry_point, syscalls, gas_limit)
    }
}
```

**Pros**:
- Backward compatible
- Gradual migration path
- Can deprecate old VM later

**Cons**:
- Increased code complexity
- Two VM implementations to maintain
- Larger binary size

**Recommendation**: ✅ **Recommended** for production migration

---

### Option 3: TBPF-Only for New Contracts (Hybrid)

**Approach**: Freeze current VM, only allow TBPF for new deployments

**Implementation**:
```rust
// At certain block height, enforce TBPF-only
if block_height >= TBPF_ACTIVATION_HEIGHT {
    match contract_module {
        ContractModule::TosVM(_) => {
            return Err("TosVM contracts no longer supported");
        }
        ContractModule::TBPF(elf) => {
            // Execute TBPF
        }
    }
}
```

**Pros**:
- Clean cutover
- Simpler long-term maintenance

**Cons**:
- Existing contracts become read-only
- Requires migration tooling

**Recommendation**: ⚠️ **Consider** for testnets first

---

## Implementation Phases

### Phase 1: TBPF VM Core Integration (4-6 weeks)

**Goal**: Get basic TBPF execution working

#### 1.1 Repository Structure

Create `tbpf` directory in `tos-vm` repository:

```
tos-vm/
├── vm/              # Current VM (keep as-is)
├── tbpf/            # NEW: TBPF implementation
│   ├── src/
│   │   ├── vm.rs           # Core TBPF VM (adapted from Solana rbpf)
│   │   ├── jit.rs          # JIT compiler (optional, for performance)
│   │   ├── verifier.rs     # Bytecode verifier
│   │   ├── syscalls.rs     # Syscall interface
│   │   └── lib.rs
│   ├── Cargo.toml
│   └── tests/
├── builder/         # Environment builder
└── types/           # Shared types
```

#### 1.2 Dependencies

Update `tos-vm/tbpf/Cargo.toml`:

```toml
[dependencies]
# Core TBPF VM (adapted from Solana)
solana-rbpf = { version = "0.8", optional = true }
# Or use your own TBPF fork
# tbpf-vm = { path = "../tbpf-vm" }

# ELF parsing
goblin = "0.8"

# Syscalls
log = "0.4"
thiserror = "2.0"
```

#### 1.3 Core VM Implementation

**File**: `tos-vm/tbpf/src/vm.rs`

```rust
use solana_rbpf::{
    ebpf,
    vm::{Config, EbpfVm},
    verifier,
};

/// TBPF VM for TOS blockchain
pub struct TbpfVM {
    /// ELF bytecode
    elf_bytes: Vec<u8>,

    /// VM configuration
    config: Config,
}

impl TbpfVM {
    /// Create new TBPF VM from ELF bytecode
    pub fn new(elf_bytes: Vec<u8>) -> Result<Self, TbpfError> {
        // Verify bytecode
        verifier::check(&elf_bytes)?;

        Ok(Self {
            elf_bytes,
            config: Config::default(),
        })
    }

    /// Execute contract with gas limit
    pub fn execute(
        &self,
        entry_point: &str,
        syscalls: TbpfSyscalls,
        compute_budget: u64,
    ) -> Result<u64, TbpfError> {
        // Create VM instance
        let mut vm = EbpfVm::new(
            &self.elf_bytes,
            &self.config,
            syscalls.into_registry(),
        )?;

        // Set compute budget (gas limit)
        vm.context_object_mut().set_compute_budget(compute_budget);

        // Execute
        let result = vm.execute_program()?;

        // Return exit code and compute units consumed
        Ok(result)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TbpfError {
    #[error("Verification failed: {0}")]
    VerificationFailed(String),

    #[error("Execution error: {0}")]
    ExecutionError(String),

    #[error("Out of compute units")]
    OutOfComputeUnits,
}
```

#### 1.4 Syscall Interface

**File**: `tos-vm/tbpf/src/syscalls.rs`

```rust
use solana_rbpf::{
    syscalls::SyscallObject,
    vm::SyscallRegistry,
};

/// TOS-specific syscalls for TBPF VM
pub struct TbpfSyscalls {
    /// Blockchain context (block hash, height, etc.)
    pub chain_context: ChainContext,

    /// Contract state provider
    pub state_provider: Box<dyn ContractProvider>,
}

impl TbpfSyscalls {
    /// Convert to syscall registry
    pub fn into_registry(self) -> SyscallRegistry {
        let mut registry = SyscallRegistry::default();

        // Register TOS syscalls
        registry.register_syscall_by_name(
            b"tos_log",
            TosLog::new,
        );

        registry.register_syscall_by_name(
            b"tos_get_balance",
            TosGetBalance::new,
        );

        registry.register_syscall_by_name(
            b"tos_transfer",
            TosTransfer::new,
        );

        // ... more syscalls

        registry
    }
}

/// Syscall: tos_log (print debug messages)
struct TosLog;

impl TosLog {
    fn new() -> Self {
        Self
    }
}

impl SyscallObject<ChainContext> for TosLog {
    fn call(
        &mut self,
        msg_ptr: u64,
        msg_len: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        memory: &mut [u8],
        context: &mut ChainContext,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        // Read message from VM memory
        let msg_bytes = &memory[msg_ptr as usize..(msg_ptr + msg_len) as usize];
        let msg = std::str::from_utf8(msg_bytes)?;

        // Log to blockchain (if debug mode enabled)
        if context.debug_mode {
            log::info!("[Contract {}]: {}", context.contract_hash, msg);
        }

        Ok(0)
    }
}

/// Syscall: tos_get_balance (query account balance)
struct TosGetBalance;

impl SyscallObject<ChainContext> for TosGetBalance {
    fn call(
        &mut self,
        asset_hash_ptr: u64,
        balance_out_ptr: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        memory: &mut [u8],
        context: &mut ChainContext,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        // Read asset hash from VM memory (32 bytes)
        let asset_bytes = &memory[asset_hash_ptr as usize..asset_hash_ptr as usize + 32];
        let asset_hash = Hash::from_bytes(asset_bytes)?;

        // Query balance from state
        let balance = context.state_provider
            .get_contract_balance_for_asset(
                &context.contract_hash,
                &asset_hash,
                context.topoheight,
            )?
            .map(|(_, balance)| balance)
            .unwrap_or(0);

        // Write balance to output pointer (8 bytes, little-endian)
        let balance_bytes = balance.to_le_bytes();
        memory[balance_out_ptr as usize..balance_out_ptr as usize + 8]
            .copy_from_slice(&balance_bytes);

        Ok(0)
    }
}

// ... more syscalls (tos_transfer, tos_storage_load, tos_storage_store, etc.)
```

---

### Phase 2: TOS Integration (3-4 weeks)

**Goal**: Integrate TBPF VM into TOS blockchain execution

#### 2.1 Update Transaction Types

**File**: `common/src/transaction/payload/contract/deploy.rs`

```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ContractModule {
    /// Current TOS VM bytecode
    #[serde(rename = "tos_vm")]
    TosVM(tos_vm::Module),

    /// TBPF ELF bytecode
    #[serde(rename = "tbpf")]
    TBPF {
        /// ELF binary
        elf: Vec<u8>,

        /// Entry point function name (default: "entrypoint")
        entry_point: Option<String>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DeployContractPayload {
    pub module: ContractModule,
    pub invoke: Option<InvokeConstructorPayload>,
}

impl Serializer for ContractModule {
    fn write(&self, writer: &mut Writer) {
        match self {
            ContractModule::TosVM(module) => {
                writer.write_u8(0);  // Discriminator
                module.write(writer);
            }
            ContractModule::TBPF { elf, entry_point } => {
                writer.write_u8(1);  // Discriminator
                writer.write_bytes(elf);
                entry_point.write(writer);
            }
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        match reader.read_u8()? {
            0 => Ok(ContractModule::TosVM(Module::read(reader)?)),
            1 => Ok(ContractModule::TBPF {
                elf: reader.read_bytes()?,
                entry_point: Option::read(reader)?,
            }),
            _ => Err(ReaderError::InvalidValue),
        }
    }

    fn size(&self) -> usize {
        1 + match self {
            ContractModule::TosVM(m) => m.size(),
            ContractModule::TBPF { elf, entry_point } => {
                8 + elf.len() + entry_point.size()
            }
        }
    }
}
```

#### 2.2 Update Contract Execution

**File**: `common/src/transaction/verify/contract.rs`

```rust
use tos_vm::{ValueCell, VM};
use tos_tbpf::{TbpfVM, TbpfSyscalls, ChainContext};

pub(super) async fn invoke_contract<'a, P: ContractProvider, E, B: BlockchainApplyState<'a, P, E>>(
    self: &'a Arc<Self>,
    tx_hash: &'a Hash,
    state: &mut B,
    contract: &'a Hash,
    deposits: &'a IndexMap<Hash, ContractDeposit>,
    parameters: impl DoubleEndedIterator<Item = ValueCell>,
    max_gas: u64,
    invoke: InvokeContract,
) -> Result<bool, VerificationError<E>> {
    // Get contract environment and module
    let (contract_environment, mut chain_state) = state
        .get_contract_environment_for(contract, deposits, tx_hash)
        .await
        .map_err(VerificationError::State)?;

    // Load module (cached in state)
    let contract_module = state.get_contract_module(contract).await
        .map_err(VerificationError::State)?;

    // Execute based on module type
    let (used_gas, exit_code) = match contract_module {
        // ===== Current TOS VM =====
        ContractModule::TosVM(module) => {
            block_in_place_safe::<_, Result<_, anyhow::Error>>(|| {
                let mut vm = VM::new(contract_environment.environment);
                vm.append_module(module)?;

                // ... existing TOS VM execution logic ...

                Ok((gas_usage, exit_code))
            })?
        }

        // ===== NEW: TBPF VM =====
        ContractModule::TBPF { elf, entry_point } => {
            block_in_place_safe::<_, Result<_, anyhow::Error>>(|| {
                // Create TBPF VM
                let vm = TbpfVM::new(elf.clone())?;

                // Prepare syscalls with chain context
                let syscalls = TbpfSyscalls {
                    chain_context: ChainContext {
                        contract_hash: contract.clone(),
                        block_hash: state.get_block_hash().clone(),
                        topoheight: chain_state.topoheight,
                        debug_mode: chain_state.debug_mode,
                        mainnet: chain_state.mainnet,
                        tx_hash: tx_hash.clone(),
                        deposits: deposits.clone(),
                    },
                    state_provider: contract_environment.provider,
                };

                // Serialize parameters to input buffer
                let input_data = serialize_parameters(parameters)?;

                // Execute contract
                let entry_fn = entry_point.as_deref().unwrap_or("entrypoint");
                let result = vm.execute(
                    entry_fn,
                    syscalls,
                    max_gas,  // Compute budget
                    &input_data,
                )?;

                // Extract results
                let compute_units_used = result.compute_units_consumed;
                let exit_code = result.return_value;

                Ok((compute_units_used, Some(exit_code)))
            })?
        }
    };

    // ... rest of the execution logic (gas refunds, state merging) ...

    Ok(exit_code == Some(0))
}

/// Serialize parameters for TBPF input
fn serialize_parameters(
    params: impl DoubleEndedIterator<Item = ValueCell>
) -> Result<Vec<u8>, anyhow::Error> {
    // Convert ValueCell parameters to borsh-serialized bytes
    // This matches Solana's instruction_data format
    let mut buf = Vec::new();
    for param in params {
        // Serialize each parameter using borsh or bincode
        borsh::to_writer(&mut buf, &param)?;
    }
    Ok(buf)
}
```

#### 2.3 Update Storage Interface

**File**: `daemon/src/core/storage/providers/contract/mod.rs`

```rust
#[async_trait]
pub trait ContractStorageProvider {
    // ... existing methods ...

    /// Store contract module (supports both TosVM and TBPF)
    async fn store_contract_module(
        &mut self,
        hash: &Hash,
        module: &ContractModule,  // Changed from &Module
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    /// Load contract module
    async fn load_contract_module(
        &self,
        hash: &Hash,
        topoheight: TopoHeight,
    ) -> Result<Option<ContractModule>, BlockchainError>;
}
```

---

### Phase 3: Syscall Implementation (4-6 weeks)

**Goal**: Implement all TOS-specific syscalls

#### 3.1 Core Syscalls

Implement the following syscalls (see `common/src/contract/mod.rs` for reference):

| Syscall Name | Purpose | TOS Native Equivalent |
|-------------|---------|----------------------|
| `tos_log` | Debug logging | `println()`, `debug()` |
| `tos_get_contract_hash` | Get current contract | `get_contract_hash()` |
| `tos_get_balance` | Get contract balance | `get_balance_for_asset()` |
| `tos_transfer` | Transfer assets | `transfer()` |
| `tos_burn` | Burn assets | `burn()` |
| `tos_storage_load` | Load from storage | `Storage::load()` |
| `tos_storage_store` | Store to storage | `Storage::store()` |
| `tos_storage_delete` | Delete from storage | `Storage::delete()` |
| `tos_get_tx_hash` | Get transaction hash | `Transaction::current().hash()` |
| `tos_get_tx_source` | Get transaction sender | `Transaction::current().source()` |
| `tos_get_block_hash` | Get block hash | `Block::current().hash()` |
| `tos_get_block_height` | Get block height | `Block::current().height()` |
| `tos_asset_create` | Create new asset | `Asset::create()` |
| `tos_asset_mint` | Mint asset supply | `Asset::mint()` |
| `tos_fire_event` | Emit contract event | `fire_event()` |

#### 3.2 Memory Safety

All syscalls must validate memory access:

```rust
fn validate_memory_access(
    ptr: u64,
    len: u64,
    memory: &[u8],
) -> Result<(), TbpfError> {
    let end = ptr.checked_add(len)
        .ok_or(TbpfError::MemoryAccessViolation)?;

    if end as usize > memory.len() {
        return Err(TbpfError::MemoryAccessViolation);
    }

    Ok(())
}
```

---

### Phase 4: Testing & Tooling (3-4 weeks)

#### 4.1 Test Contracts

Create reference TBPF contracts for testing:

**File**: `tests/contracts/tbpf/hello_world.c`

```c
#include <tos/entrypoint.h>
#include <tos/syscalls.h>

// Entry point for TBPF contract
uint64_t entrypoint(const uint8_t *input, uint64_t input_len) {
    tos_log("Hello from TBPF contract!");

    // Get contract balance
    uint8_t asset_hash[32] = {0}; // TOS native asset
    uint64_t balance = 0;
    tos_get_balance(asset_hash, &balance);

    tos_log("Contract balance: %llu", balance);

    return 0; // Success
}
```

Compile with:
```bash
clang -target bpf -O2 -emit-llvm -c hello_world.c -o hello_world.bc
llc -march=bpf -filetype=obj -o hello_world.o hello_world.bc
```

#### 4.2 SDK for Contract Developers

Create `tos-tbpf-sdk` crate:

```rust
// tos-tbpf-sdk/src/lib.rs

/// Entry point macro for TBPF contracts
#[macro_export]
macro_rules! entrypoint {
    ($process_instruction:ident) => {
        #[no_mangle]
        pub unsafe extern "C" fn entrypoint(input: *const u8, input_len: u64) -> u64 {
            let input_slice = std::slice::from_raw_parts(input, input_len as usize);
            match $process_instruction(input_slice) {
                Ok(_) => 0,
                Err(e) => e.into(),
            }
        }
    };
}

/// Syscall bindings
pub mod syscalls {
    extern "C" {
        pub fn tos_log(msg_ptr: *const u8, msg_len: u64);
        pub fn tos_get_balance(asset_hash_ptr: *const u8, balance_out: *mut u64) -> u64;
        pub fn tos_transfer(dest_ptr: *const u8, amount: u64, asset_ptr: *const u8) -> u64;
    }
}
```

---

## Key Technical Considerations

### 1. Gas vs Compute Units

**Solana**: Uses "compute units" (CU) with fixed costs per instruction
**TOS**: Uses "gas" with dynamic costs per operation

**Solution**: Map compute units to gas:

```rust
// 1 compute unit = 1 gas (for simplicity)
const COMPUTE_UNIT_TO_GAS_RATIO: u64 = 1;

// Or use Solana's default limits
const DEFAULT_COMPUTE_UNITS: u64 = 200_000;
const MAX_COMPUTE_UNITS: u64 = 1_400_000;
```

### 2. Account Model vs Balance Model

**Solana**: Programs are stateless, all state in accounts
**TOS**: Contracts have direct storage access

**Solution**:
- Keep TOS model (simpler for developers)
- Map `tos_storage_*` syscalls to contract storage
- Don't require account passing like Solana

### 3. Cross-Contract Calls

**Solana**: Cross-Program Invocation (CPI)
**TOS**: Not currently implemented

**Future Work**:
```rust
// Syscall: tos_invoke_contract
fn tos_invoke_contract(
    contract_hash: &Hash,
    entry_point: u16,
    params: &[ValueCell],
    gas_limit: u64,
) -> Result<u64, TbpfError>;
```

### 4. Bytecode Size Limits

**Recommendation**:
- Max ELF size: 1 MB (same as Solana before v1.16)
- Store ELF compressed in blockchain
- Decompress during execution

```rust
// common/src/transaction/payload/contract/deploy.rs

const MAX_CONTRACT_SIZE: usize = 1_024 * 1024; // 1 MB

impl DeployContractPayload {
    pub fn validate(&self) -> Result<(), ValidationError> {
        match &self.module {
            ContractModule::TBPF { elf, .. } => {
                if elf.len() > MAX_CONTRACT_SIZE {
                    return Err(ValidationError::ContractTooLarge);
                }
            }
            _ => {}
        }
        Ok(())
    }
}
```

### 5. Determinism

**CRITICAL**: All VM operations MUST be deterministic

**Risks**:
- Floating-point operations (MUST be disabled)
- Syscall randomness (use deterministic RNG from block hash)
- Time-based operations (use block timestamp, not system time)

**Mitigation**:
```rust
// Disable non-deterministic instructions in verifier
fn verify_bytecode(elf: &[u8]) -> Result<(), TbpfError> {
    let instructions = parse_elf(elf)?;

    for insn in instructions {
        // Check for floating-point instructions
        if is_float_instruction(insn) {
            return Err(TbpfError::NonDeterministicInstruction);
        }
    }

    Ok(())
}
```

---

## Security Considerations

### 1. Bytecode Verification

Before execution, verify:
- Valid ELF format
- No malicious relocations
- No unbounded loops (static analysis)
- Memory access bounds

### 2. Syscall Safety

All syscalls MUST:
- Validate pointer bounds
- Prevent reentrancy attacks
- Check gas before expensive operations
- Use saturating arithmetic (no overflow)

### 3. Gas Exhaustion Protection

```rust
// Example: Charge gas for memory allocation
fn tos_storage_store(key_ptr: u64, value_ptr: u64, value_len: u64) -> Result<u64> {
    // Charge gas based on value size
    let storage_cost = value_len * COST_PER_BYTE_STORED;
    charge_gas(storage_cost)?;

    // ... perform storage operation ...
}
```

---

## Migration Path for Existing Contracts

### Option A: Recompile to TBPF

If existing TOS contracts are written in a high-level language:
1. Recompile source code to TBPF target
2. Deploy new TBPF version
3. Migrate state if needed

### Option B: Keep TosVM Forever

If migration is too complex:
- Keep both VMs running
- Mark TosVM contracts as "legacy"
- New contracts MUST use TBPF

---

## Performance Benchmarks (Target)

| Metric | Current TOS VM | TBPF (Target) |
|--------|---------------|---------------|
| Execution Speed | Baseline | 2-5x faster |
| JIT Compilation | N/A | 10-50x faster |
| Gas Metering Overhead | ~10% | ~5% |
| Contract Size Limit | 512 KB | 1 MB |

---

## Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tbpf_hello_world() {
        let elf = include_bytes!("../tests/contracts/hello_world.o");
        let vm = TbpfVM::new(elf.to_vec()).unwrap();

        let result = vm.execute(
            "entrypoint",
            mock_syscalls(),
            100_000,
            &[],
        ).unwrap();

        assert_eq!(result.return_value, 0);
    }
}
```

### Integration Tests

```rust
// Test TBPF contract deployment and execution on testnet
#[tokio::test]
async fn test_tbpf_deploy_and_invoke() {
    let daemon = TestDaemon::new().await;

    // Deploy TBPF contract
    let elf = compile_contract("tests/contracts/counter.c");
    let tx = deploy_tbpf_contract(elf).await;
    daemon.submit_transaction(tx).await;

    // Invoke contract
    let contract_hash = tx.hash();
    let invoke_tx = invoke_contract(contract_hash, 0, vec![]).await;
    daemon.submit_transaction(invoke_tx).await;

    // Verify state changes
    assert_eq!(daemon.get_contract_storage(&contract_hash, "counter").await, Some(1));
}
```

---

## Documentation Requirements

### For Contract Developers

1. **TBPF Quick Start Guide**
   - How to write TBPF contracts in C/Rust
   - Syscall reference
   - Deployment tutorial

2. **Migration Guide**
   - TosVM → TBPF migration checklist
   - Code examples (before/after)
   - Common pitfalls

3. **SDK Documentation**
   - `tos-tbpf-sdk` API reference
   - Example contracts (token, NFT, AMM)

### For Node Operators

1. **Upgrade Guide**
   - How to upgrade daemon to support TBPF
   - Backward compatibility notes
   - Performance tuning

---

## Timeline Summary

| Phase | Duration | Deliverables |
|-------|----------|--------------|
| Phase 1: TBPF VM Core | 4-6 weeks | VM engine, verifier, basic syscalls |
| Phase 2: TOS Integration | 3-4 weeks | Transaction types, execution dispatch, storage |
| Phase 3: Syscall Implementation | 4-6 weeks | All TOS syscalls, state management |
| Phase 4: Testing & Tooling | 3-4 weeks | SDK, test contracts, integration tests |
| **Total** | **14-20 weeks** | Production-ready TBPF support |

---

## Next Steps

### Immediate Actions

1. ✅ **Review this plan** with the core team
2. ⏳ **Set up `tos-vm/tbpf` directory** structure
3. ⏳ **Choose base TBPF implementation**:
   - Fork Solana's `rbpf` crate?
   - Use existing TBPF implementation?
   - Build from scratch?
4. ⏳ **Create RFC** (Request for Comments) for community feedback
5. ⏳ **Assign development team** and timeline

### Questions to Resolve

1. **Compatibility Level**: Full Solana bytecode compatibility or TOS-specific?
2. **JIT vs Interpreter**: Should we implement JIT compilation for performance?
3. **Activation Height**: When to enable TBPF on mainnet?
4. **Fee Structure**: How to price TBPF execution gas costs?

---

## References

- [Solana sBPF Documentation](https://solana.com/docs/programs/faq#berkeley-packet-filter-bpf)
- [rbpf GitHub](https://github.com/solana-labs/rbpf)
- [Solana Runtime](https://github.com/solana-labs/solana/tree/master/runtime)
- [eBPF Instruction Set](https://www.kernel.org/doc/html/latest/bpf/instruction-set.html)

---

**Document Version**: 1.0
**Last Updated**: 2025-10-29
**Maintainer**: TOS Development Team
