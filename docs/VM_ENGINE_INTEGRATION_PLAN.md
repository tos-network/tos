# TOS VM Engine Integration Plan: TBPF (Solana-style eBPF VM)

**Date**: 2025-10-29
**Status**: Planning Phase
**Goal**: Integrate Solana's sBPF execution engine (adapted as TBPF) into TOS blockchain

---

## Executive Summary

**UPDATED STRATEGY**: Complete VM Replacement (No Backward Compatibility)

TOS is still in active development, so we will **completely replace** the current TOS VM with TBPF (TOS Berkeley Packet Filter) based on Solana's sBPF. This simplifies implementation by removing backward compatibility concerns.

**Key Decision**: Since TOS has no production contracts yet, we can adopt a clean-slate approach and build TBPF as the only execution engine.

This document outlines the complete replacement strategy, implementation steps, and technical considerations.

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

## Integration Strategy: Complete VM Replacement

**Selected Approach**: ✅ **Complete replacement of TOS VM with TBPF**

Since TOS is still in development with no production contracts, we adopt a clean-slate approach.

### Architecture Overview

**Simplified Contract Module**:
```rust
// File: common/src/transaction/payload/contract/deploy.rs

pub struct DeployContractPayload {
    /// TBPF ELF bytecode (only format supported)
    pub elf: Vec<u8>,

    /// Optional entry point function name (default: "entrypoint")
    pub entry_point: Option<String>,

    /// Optional constructor invocation
    pub invoke: Option<InvokeConstructorPayload>,
}
```

**Simplified Execution Flow**:
```rust
// File: common/src/transaction/verify/contract.rs

pub async fn invoke_contract(...) -> Result<bool, VerificationError<E>> {
    // Load TBPF ELF bytecode
    let elf = state.get_contract_elf(contract).await?;

    // Create TBPF VM
    let vm = TbpfVM::new(elf)?;

    // Prepare syscalls with TOS blockchain context
    let syscalls = TbpfSyscalls::new(chain_state, provider);

    // Execute contract
    let result = vm.execute("entrypoint", syscalls, max_gas, &input_data)?;

    // Process results
    Ok(result.exit_code == 0)
}
```

### Benefits of Complete Replacement

✅ **Simpler Architecture**:
- Single VM implementation to maintain
- No version branching logic
- Smaller binary size

✅ **Better Performance**:
- Register-based VM (faster than stack-based)
- Optional JIT compilation (10-50x speedup)
- Battle-tested in Solana production

✅ **Solana Compatibility**:
- Can run Solana contracts with minimal modifications
- Easier to port existing Solana tooling
- Access to Solana's developer ecosystem

✅ **Clean Codebase**:
- Remove old `tos-vm` VM engine completely
- Keep only `tos-builder` for syscall infrastructure
- Simplified contract storage (just ELF bytes)

### What Gets Replaced

| Component | Current | After Replacement |
|-----------|---------|-------------------|
| VM Engine | Custom stack-based VM | TBPF (eBPF register-based) |
| Bytecode Format | `tos_vm::Module` | ELF binary with sBPF |
| Execution | `VM::new()` → `vm.run()` | `TbpfVM::execute()` |
| Entry Points | Chunks + hooks | Single `entrypoint()` function |
| Gas Metering | Custom gas units | Compute units (CU) |

### What Gets Preserved

✅ **Keep existing TOS blockchain features**:
- Contract storage via `ContractProvider` trait
- Gas refunds and burning (TX_GAS_BURN_PERCENT)
- Deposits and transfers
- Event tracking (fire_event)
- Asset management

✅ **Keep TOS-specific syscalls**:
- All functions in `common/src/contract/mod.rs` become syscalls
- TOS account model (simpler than Solana's account model)
- Direct storage access (no account passing required)

---

## Implementation Phases (Simplified for Complete Replacement)

### Phase 1: TBPF VM Core Implementation (3-4 weeks)

**Goal**: Replace `tos-vm` VM engine with TBPF

#### 1.1 Repository Structure

**Replace** the existing VM implementation in `tos-vm` repository:

```
tos-vm/
├── vm/              # REMOVE: Delete old VM engine
├── tbpf/            # NEW: TBPF implementation (becomes main VM)
│   ├── src/
│   │   ├── vm.rs           # Core TBPF VM (adapted from Solana rbpf)
│   │   ├── jit.rs          # JIT compiler (optional, for performance)
│   │   ├── verifier.rs     # Bytecode verifier
│   │   ├── syscalls.rs     # Syscall interface
│   │   ├── error.rs        # Error types
│   │   └── lib.rs
│   ├── Cargo.toml
│   └── tests/
├── builder/         # KEEP: Refactor for TBPF syscalls
└── types/           # KEEP: Shared types
```

**Migration Strategy**:
1. Create `tos-vm/tbpf/` with TBPF implementation
2. Update `tos-vm/Cargo.toml` to use `tbpf` as default
3. Remove old `vm/` directory after TBPF is working
4. Update `tos-builder` to work with TBPF syscalls

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

### Phase 2: TOS Blockchain Integration (2-3 weeks)

**Goal**: Replace contract execution with TBPF VM

#### 2.1 Update Transaction Types (Simplified)

**File**: `common/src/transaction/payload/contract/deploy.rs`

**REMOVE** the old `Module` type, replace with simple ELF bytecode:

```rust
use serde::{Deserialize, Serialize};
use crate::serializer::*;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DeployContractPayload {
    /// TBPF ELF bytecode (replaces tos_vm::Module)
    pub elf: Vec<u8>,

    /// Optional entry point function name (default: "entrypoint")
    #[serde(default)]
    pub entry_point: Option<String>,

    /// Optional constructor invocation
    pub invoke: Option<InvokeConstructorPayload>,
}

impl Serializer for DeployContractPayload {
    fn write(&self, writer: &mut Writer) {
        // Write ELF bytes with length prefix
        writer.write_u32(self.elf.len() as u32);
        writer.write_bytes(&self.elf);

        // Write optional entry point
        self.entry_point.write(writer);

        // Write optional constructor invocation
        self.invoke.write(writer);
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        // Read ELF bytes
        let elf_len = reader.read_u32()? as usize;

        // Enforce max contract size (1 MB)
        const MAX_CONTRACT_SIZE: usize = 1_024 * 1024;
        if elf_len > MAX_CONTRACT_SIZE {
            return Err(ReaderError::InvalidSize);
        }

        let mut elf = vec![0u8; elf_len];
        reader.read_bytes_into(&mut elf)?;

        Ok(Self {
            elf,
            entry_point: Option::read(reader)?,
            invoke: Option::read(reader)?,
        })
    }

    fn size(&self) -> usize {
        4 + self.elf.len() + self.entry_point.size() + self.invoke.size()
    }
}
```

#### 2.2 Update Contract Execution (TBPF Only)

**File**: `common/src/transaction/verify/contract.rs`

**REPLACE** the entire execution logic with TBPF-only:

```rust
use tos_tbpf::{TbpfVM, TbpfSyscalls, TbpfContext, TbpfResult};
use crate::tokio::block_in_place_safe;

pub(super) async fn invoke_contract<'a, P: ContractProvider, E, B: BlockchainApplyState<'a, P, E>>(
    self: &'a Arc<Self>,
    tx_hash: &'a Hash,
    state: &mut B,
    contract: &'a Hash,
    deposits: &'a IndexMap<Hash, ContractDeposit>,
    parameters: Vec<u8>,  // Simplified: pre-serialized parameters
    max_gas: u64,
) -> Result<bool, VerificationError<E>> {
    if log::log_enabled!(log::Level::Debug) {
        debug!("Invoking TBPF contract {} from TX {}", contract, tx_hash);
    }

    // Get contract environment (provider + chain state)
    let (contract_environment, mut chain_state) = state
        .get_contract_environment_for(contract, deposits, tx_hash)
        .await
        .map_err(VerificationError::State)?;

    // Load ELF bytecode from storage
    let contract_data = state.load_contract_elf(contract).await
        .map_err(VerificationError::State)?;

    // Execute in blocking thread (CPU-intensive work)
    let (used_gas, exit_code) = block_in_place_safe::<_, Result<_, anyhow::Error>>(|| {
        // Create TBPF VM from ELF
        let mut vm = TbpfVM::new(&contract_data.elf)?;

        // Create execution context with blockchain state
        let mut context = TbpfContext {
            contract_hash: contract.clone(),
            block_hash: state.get_block_hash().clone(),
            block: state.get_block().clone(),
            tx_hash: tx_hash.clone(),
            topoheight: chain_state.topoheight,
            debug_mode: chain_state.debug_mode,
            mainnet: chain_state.mainnet,
            deposits: deposits.clone(),
            chain_state: &mut chain_state,
            provider: contract_environment.provider,
        };

        // Register TOS syscalls
        let syscalls = TbpfSyscalls::new();
        vm.bind_syscalls(syscalls);

        // Set compute budget (gas limit)
        vm.set_compute_budget(max_gas);

        // Execute entry point
        let entry_fn = contract_data.entry_point
            .as_deref()
            .unwrap_or("entrypoint");

        let result: TbpfResult = vm.execute(entry_fn, &parameters, &mut context)?;

        // Extract gas usage and exit code
        Ok((result.compute_units_consumed, Some(result.return_value)))
    })?;

    // Check if execution was successful
    let is_success = exit_code == Some(0);
    let mut outputs = chain_state.outputs;

    if is_success {
        // Merge contract state changes
        let cache = chain_state.cache;
        let tracker = chain_state.tracker;
        let assets = chain_state.assets;

        state.merge_contract_changes(&contract, cache, tracker, assets)
            .await
            .map_err(VerificationError::State)?;
    } else {
        // Execution failed, refund deposits
        outputs.clear();

        if !deposits.is_empty() {
            self.refund_deposits(state, deposits).await?;
            outputs.push(ContractOutput::RefundDeposits);
        }
    }

    // Add exit code to outputs
    outputs.push(ContractOutput::ExitCode(exit_code));

    // Handle gas refunds
    let refund_gas = self.handle_gas(state, used_gas, max_gas).await?;
    if log::log_enabled!(log::Level::Debug) {
        debug!("Used gas: {}, refund gas: {}", used_gas, refund_gas);
    }
    if refund_gas > 0 {
        outputs.push(ContractOutput::RefundGas { amount: refund_gas });
    }

    // Track outputs
    state.set_contract_outputs(tx_hash, outputs).await
        .map_err(VerificationError::State)?;

    Ok(is_success)
}
```

**Key Changes**:
- ❌ Remove `tos_vm::VM` import
- ✅ Use `tos_tbpf::TbpfVM` only
- ❌ Remove `ContractModule` enum
- ✅ Load ELF bytes directly
- ✅ Simplified parameter passing (pre-serialized bytes)

#### 2.3 Update Storage Interface (Simplified)

**File**: `daemon/src/core/storage/providers/contract/mod.rs`

**REPLACE** contract storage to use ELF bytes instead of `Module`:

```rust
#[derive(Clone, Debug)]
pub struct ContractData {
    /// ELF bytecode
    pub elf: Vec<u8>,

    /// Entry point function name
    pub entry_point: Option<String>,
}

#[async_trait]
pub trait ContractStorageProvider {
    // ... existing methods ...

    /// Store contract ELF bytecode
    async fn store_contract_elf(
        &mut self,
        hash: &Hash,
        data: &ContractData,
        topoheight: TopoHeight,
    ) -> Result<(), BlockchainError>;

    /// Load contract ELF bytecode
    async fn load_contract_elf(
        &self,
        hash: &Hash,
        topoheight: TopoHeight,
    ) -> Result<Option<ContractData>, BlockchainError>;

    /// Check if contract exists (no need to load full ELF)
    async fn contract_exists(
        &self,
        hash: &Hash,
        topoheight: TopoHeight,
    ) -> Result<bool, BlockchainError>;
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

## Performance Benchmarks (Target)

**TBPF Performance Goals**:

| Metric | Target Value | Notes |
|--------|--------------|-------|
| Execution Speed (Interpreter) | 50-100M instructions/sec | Baseline eBPF performance |
| Execution Speed (JIT) | 500M-1B instructions/sec | 10-20x faster with JIT |
| Gas Metering Overhead | ~5% | Minimal overhead per instruction |
| Contract Size Limit | 1 MB | Same as Solana (v1.15+) |
| Max Compute Units | 1.4M CU | Solana's upper limit |
| Default Compute Budget | 200k CU | Solana's default |

**Comparison with Solana**:
- ✅ Same bytecode format (eBPF/ELF)
- ✅ Similar performance characteristics
- ✅ Can use Solana's JIT compiler
- ⚠️ Different syscalls (TOS-specific)

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
   - Complete syscall reference
   - Deployment tutorial with examples

2. **SDK Documentation**
   - `tos-tbpf-sdk` API reference
   - Example contracts (token, NFT, DeFi, DAO)
   - Best practices for TBPF on TOS

3. **Comparison with Solana**
   - Differences between TOS TBPF and Solana sBPF
   - Account model vs TOS storage model
   - Porting guide for Solana contracts

### For Node Operators

1. **Deployment Guide**
   - How to deploy TOS daemon with TBPF support
   - Configuration options
   - Performance tuning and monitoring

---

## Timeline Summary (Complete Replacement)

**Faster timeline** due to simplified architecture (no dual-VM complexity):

| Phase | Duration | Deliverables |
|-------|----------|--------------|
| Phase 1: TBPF VM Core | 3-4 weeks | TBPF engine, verifier, syscall framework |
| Phase 2: TOS Integration | 2-3 weeks | Transaction types, execution, storage |
| Phase 3: Syscall Implementation | 3-4 weeks | All TOS syscalls, state management |
| Phase 4: Testing & Tooling | 2-3 weeks | SDK, test contracts, integration tests |
| **Total** | **10-14 weeks** | Production-ready TBPF (complete replacement) |

**Time savings**: 4-6 weeks faster than dual-VM approach due to:
- ✅ No backward compatibility code
- ✅ Simpler transaction types (no enum dispatch)
- ✅ Single VM to test and debug
- ✅ Cleaner codebase

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
