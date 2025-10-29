# TOS-VM TBPF Refactoring Plan

**Date**: 2025-10-29
**Strategy**: Interface-Compatible Internal Refactoring
**Repository**: https://github.com/tos-network/tos-vm

---

## Executive Summary

**Objective**: Refactor `tos-vm` repository to use TBPF (eBPF) as the execution engine while maintaining **100% API compatibility** with existing TOS blockchain code.

**Approach**: Create a new branch in `tos-vm` repository, replace internal VM implementation with TBPF, keep all public interfaces unchanged.

**Key Benefit**: TOS blockchain code requires **minimal changes** (just dependency version update).

---

## Current tos-vm Architecture

### Repository Structure

```
tos-vm/
├── vm/              # Current VM implementation (stack-based)
├── builder/         # EnvironmentBuilder (syscall registration)
├── environment/     # Runtime environment
├── types/           # Type system (ValueCell, Primitive, Type)
└── README.md
```

### Public API Used by TOS

Based on analysis of TOS blockchain code, these are the **critical interfaces** that must remain unchanged:

#### Core Types (from `tos-vm` crate)

```rust
// Main VM instance
pub struct VM { ... }

// Bytecode module
pub struct Module { ... }

// Execution context
pub struct Context { ... }

// Value types
pub enum ValueCell { ... }
pub enum Primitive { ... }
pub enum Type { ... }

// Function interfaces
pub type FnInstance = ...;
pub type FnParams = ...;
pub type FnReturnType = ...;

// Opaque type wrapper
pub struct OpaqueWrapper<T> { ... }
```

#### Core API Methods

**VM Creation and Execution**:
```rust
impl VM {
    // Create VM with environment
    pub fn new(environment: &Environment) -> Self;

    // Load bytecode module
    pub fn append_module(&mut self, module: &Module) -> Result<()>;

    // Invoke entry point by ID
    pub fn invoke_entry_chunk(&mut self, entry: u16) -> Result<()>;

    // Invoke hook by ID
    pub fn invoke_hook_id(&mut self, hook: u8) -> Result<bool>;

    // Push parameter to stack
    pub fn push_stack(&mut self, value: ValueCell) -> Result<()>;

    // Get mutable context
    pub fn context_mut(&mut self) -> &mut Context;

    // Get immutable context
    pub fn context(&self) -> &Context;

    // Execute VM
    pub fn run(&mut self) -> Result<ValueCell>;
}
```

**Context Management**:
```rust
impl Context {
    // Set gas limit
    pub fn set_gas_limit(&mut self, limit: u64);

    // Get current gas usage
    pub fn current_gas_usage(&self) -> u64;

    // Increase gas usage
    pub fn increase_gas_usage(&mut self, amount: u64) -> Result<()>;

    // Insert context data by reference
    pub fn insert_ref<T: 'static>(&mut self, value: &T);

    // Insert context data by mutable reference
    pub fn insert_mut<T: 'static>(&mut self, value: &mut T);

    // Insert context data by value
    pub fn insert<T: 'static>(&mut self, value: T);

    // Get context data
    pub fn get<T: 'static>(&self) -> Option<&T>;

    // Get mutable context data
    pub fn get_mut<T: 'static>(&mut self) -> Option<&mut T>;
}
```

#### Builder API (from `tos-builder` crate)

```rust
pub struct EnvironmentBuilder { ... }

impl EnvironmentBuilder {
    // Create new builder
    pub fn default() -> Self;

    // Register opaque type
    pub fn register_opaque<T>(&mut self, name: &str, allow_as_input: bool) -> TypeId;

    // Register static function
    pub fn register_static_function(
        &mut self,
        name: &str,
        type_id: Type,
        params: Vec<(&str, Type)>,
        handler: fn(...),
        gas_cost: u64,
        return_type: Option<Type>,
    );

    // Register native function (method)
    pub fn register_native_function(
        &mut self,
        name: &str,
        on_type: Option<Type>,
        params: Vec<(&str, Type)>,
        handler: fn(...),
        gas_cost: u64,
        return_type: Option<Type>,
    );

    // Build environment
    pub fn build(self) -> Environment;
}
```

### Current Execution Flow (in TOS blockchain)

**File**: `common/src/transaction/verify/contract.rs:59-133`

```rust
// 1. Create VM with environment
let mut vm = VM::new(contract_environment.environment);

// 2. Load bytecode module
vm.append_module(contract_environment.module)?;

// 3. Invoke entry point or hook
match invoke {
    InvokeContract::Entry(entry) => {
        vm.invoke_entry_chunk(entry)?;
    }
    InvokeContract::Hook(hook) => {
        vm.invoke_hook_id(hook)?;
    }
}

// 4. Push parameters (reverse order for stack-based VM)
for constant in parameters.rev() {
    vm.push_stack(constant)?;
}

// 5. Configure context
let context = vm.context_mut();
context.set_gas_limit(max_gas);
context.insert_ref(self);
context.insert_mut(&mut chain_state);
context.insert(ContractProviderWrapper(provider));

// 6. Execute
let result = vm.run()?;

// 7. Get gas usage
let gas_usage = vm.context().current_gas_usage().min(max_gas);
```

---

## Refactoring Strategy: TBPF Implementation with Compatible Interface

### Phase 1: Create New Branch in tos-vm

**Repository**: `https://github.com/tos-network/tos-vm`
**New Branch**: `feat/tbpf-engine`

```bash
cd tos-vm
git checkout -b feat/tbpf-engine
```

### Phase 2: Replace Internal VM Implementation

**Goal**: Replace stack-based VM with TBPF (eBPF register-based VM) while keeping public API unchanged.

#### 2.1 Update Module Format

**Current `Module`**: Custom bytecode format (likely stack-based instructions)

**New `Module`**: ELF binary with eBPF instructions

**Implementation**:

```rust
// File: tos-vm/vm/src/module.rs

use std::sync::Arc;

/// Contract bytecode module
///
/// INTERNAL CHANGE: Now stores ELF binary instead of custom bytecode
pub struct Module {
    /// ELF binary with eBPF bytecode
    elf: Arc<Vec<u8>>,

    /// Entry points (ID → function name mapping)
    entry_points: HashMap<u16, String>,

    /// Hooks (ID → function name mapping)
    hooks: HashMap<u8, String>,

    /// Cached TBPF VM configuration
    config: TbpfConfig,
}

impl Module {
    /// Create module from ELF bytecode
    pub fn from_elf(elf: Vec<u8>) -> Result<Self, ModuleError> {
        // Verify ELF format
        let parsed_elf = parse_elf(&elf)?;

        // Extract entry points from ELF symbols
        let entry_points = extract_entry_points(&parsed_elf)?;

        // Extract hooks from ELF symbols
        let hooks = extract_hooks(&parsed_elf)?;

        Ok(Self {
            elf: Arc::new(elf),
            entry_points,
            hooks,
            config: TbpfConfig::default(),
        })
    }

    /// Legacy constructor for backward compatibility
    ///
    /// DEPRECATED: Use from_elf() for new code
    pub fn new(bytecode: Vec<u8>) -> Result<Self, ModuleError> {
        // Assume bytecode is ELF format
        Self::from_elf(bytecode)
    }

    /// Get ELF bytes (internal use)
    pub(crate) fn elf_bytes(&self) -> &[u8] {
        &self.elf
    }

    /// Get entry point name by ID
    pub(crate) fn entry_point(&self, id: u16) -> Option<&str> {
        self.entry_points.get(&id).map(|s| s.as_str())
    }

    /// Get hook name by ID
    pub(crate) fn hook(&self, id: u8) -> Option<&str> {
        self.hooks.get(&id).map(|s| s.as_str())
    }
}

// Keep Serializer implementation for compatibility
impl Serializer for Module {
    fn write(&self, writer: &mut Writer) {
        // Write ELF bytes
        writer.write_u32(self.elf.len() as u32);
        writer.write_bytes(&self.elf);

        // Write entry points
        writer.write_u16(self.entry_points.len() as u16);
        for (id, name) in &self.entry_points {
            writer.write_u16(*id);
            writer.write_string(name);
        }

        // Write hooks
        writer.write_u8(self.hooks.len() as u8);
        for (id, name) in &self.hooks {
            writer.write_u8(*id);
            writer.write_string(name);
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        // Read ELF bytes
        let elf_len = reader.read_u32()? as usize;
        let mut elf = vec![0u8; elf_len];
        reader.read_bytes_into(&mut elf)?;

        // Read entry points
        let entry_count = reader.read_u16()? as usize;
        let mut entry_points = HashMap::new();
        for _ in 0..entry_count {
            let id = reader.read_u16()?;
            let name = reader.read_string()?;
            entry_points.insert(id, name);
        }

        // Read hooks
        let hook_count = reader.read_u8()? as usize;
        let mut hooks = HashMap::new();
        for _ in 0..hook_count {
            let id = reader.read_u8()?;
            let name = reader.read_string()?;
            hooks.insert(id, name);
        }

        Ok(Self {
            elf: Arc::new(elf),
            entry_points,
            hooks,
            config: TbpfConfig::default(),
        })
    }

    fn size(&self) -> usize {
        4 + self.elf.len()
            + 2 + self.entry_points.iter().map(|(_, name)| 2 + 4 + name.len()).sum::<usize>()
            + 1 + self.hooks.iter().map(|(_, name)| 1 + 4 + name.len()).sum::<usize>()
    }
}
```

**Entry Point Naming Convention**:

```c
// Contract developers annotate functions with IDs

// Entry point 0
#[export_name = "entry_0"]
pub extern "C" fn constructor() -> u64 {
    // Constructor logic
    0
}

// Entry point 1
#[export_name = "entry_1"]
pub extern "C" fn transfer() -> u64 {
    // Transfer logic
    0
}

// Hook 0 (constructor hook)
#[export_name = "hook_0"]
pub extern "C" fn on_deploy() -> u64 {
    0
}
```

#### 2.2 Replace VM Internal Implementation

**Current `VM`**: Stack-based interpreter

**New `VM`**: TBPF (eBPF) interpreter/JIT wrapper

**Implementation**:

```rust
// File: tos-vm/vm/src/vm.rs

use solana_rbpf::{
    ebpf,
    vm::{Config, EbpfVm, SyscallRegistry},
    verifier,
};

/// Virtual Machine for executing smart contracts
///
/// INTERNAL CHANGE: Now uses TBPF (eBPF) instead of stack-based VM
pub struct VM<'a> {
    /// Reference to environment (for syscalls)
    environment: &'a Environment,

    /// Loaded module (ELF bytecode)
    module: Option<Arc<Module>>,

    /// TBPF VM instance (created lazily)
    tbpf_vm: Option<EbpfVm<'a, TbpfContext<'a>>>,

    /// Execution context (shared with syscalls)
    context: Context,

    /// Entry point to invoke
    entry_point: Option<String>,

    /// Parameters to pass to entry point
    parameters: Vec<ValueCell>,

    /// TBPF configuration
    config: Config,
}

impl<'a> VM<'a> {
    /// Create new VM instance with environment
    pub fn new(environment: &'a Environment) -> Self {
        Self {
            environment,
            module: None,
            tbpf_vm: None,
            context: Context::new(),
            entry_point: None,
            parameters: Vec::new(),
            config: Config {
                enable_instruction_meter: true,  // For gas metering
                enable_stack_frame_gaps: false,
                ..Config::default()
            },
        }
    }

    /// Load bytecode module
    pub fn append_module(&mut self, module: &Module) -> Result<(), VmError> {
        // Verify ELF bytecode
        verifier::check(module.elf_bytes())
            .map_err(|e| VmError::VerificationFailed(e.to_string()))?;

        self.module = Some(Arc::new(module.clone()));
        Ok(())
    }

    /// Invoke entry point by ID
    pub fn invoke_entry_chunk(&mut self, entry: u16) -> Result<(), VmError> {
        let module = self.module.as_ref()
            .ok_or(VmError::NoModuleLoaded)?;

        let entry_name = module.entry_point(entry)
            .ok_or(VmError::EntryPointNotFound(entry))?;

        self.entry_point = Some(entry_name.to_string());
        Ok(())
    }

    /// Invoke hook by ID
    pub fn invoke_hook_id(&mut self, hook: u8) -> Result<bool, VmError> {
        let module = self.module.as_ref()
            .ok_or(VmError::NoModuleLoaded)?;

        if let Some(hook_name) = module.hook(hook) {
            self.entry_point = Some(hook_name.to_string());
            Ok(true)
        } else {
            Ok(false)  // Hook not found (not an error)
        }
    }

    /// Push parameter to stack
    ///
    /// NOTE: For eBPF (register-based), we collect parameters instead of pushing to stack
    pub fn push_stack(&mut self, value: ValueCell) -> Result<(), VmError> {
        self.parameters.push(value);
        Ok(())
    }

    /// Get mutable context
    pub fn context_mut(&mut self) -> &mut Context {
        &mut self.context
    }

    /// Get immutable context
    pub fn context(&self) -> &Context {
        &self.context
    }

    /// Execute the VM
    pub fn run(&mut self) -> Result<ValueCell, VmError> {
        let module = self.module.as_ref()
            .ok_or(VmError::NoModuleLoaded)?;

        let entry_fn = self.entry_point.as_ref()
            .ok_or(VmError::NoEntryPointSet)?;

        // Create TBPF VM if not created yet
        if self.tbpf_vm.is_none() {
            // Build syscall registry from environment
            let syscalls = build_syscall_registry(self.environment)?;

            // Create TBPF VM
            let tbpf_vm = EbpfVm::new(
                module.elf_bytes(),
                &self.config,
                &syscalls,
            ).map_err(|e| VmError::VmCreationFailed(e.to_string()))?;

            self.tbpf_vm = Some(tbpf_vm);
        }

        // Serialize parameters to input buffer (borsh format)
        let input_data = serialize_parameters(&self.parameters)?;

        // Create TBPF context
        let mut tbpf_context = TbpfContext {
            gas_limit: self.context.gas_limit,
            gas_used: 0,
            context_data: &mut self.context,
        };

        // Execute the entry point
        let vm = self.tbpf_vm.as_mut().unwrap();
        let result = vm.execute_program_with_context(
            &input_data,
            &mut tbpf_context,
        ).map_err(|e| VmError::ExecutionFailed(e.to_string()))?;

        // Update gas usage
        self.context.gas_used = tbpf_context.gas_used;

        // Convert TBPF result (u64) to ValueCell
        Ok(ValueCell::U64(result))
    }
}

/// Internal TBPF context
struct TbpfContext<'a> {
    gas_limit: u64,
    gas_used: u64,
    context_data: &'a mut Context,
}

/// Build syscall registry from environment
fn build_syscall_registry(env: &Environment) -> Result<SyscallRegistry, VmError> {
    let mut registry = SyscallRegistry::default();

    // Register all syscalls from environment
    // Each syscall in EnvironmentBuilder becomes an eBPF syscall

    // Example: println becomes "tos_log"
    registry.register_syscall_by_name(
        b"tos_log",
        SyscallLog::init,
    )?;

    // ... register all other syscalls ...

    Ok(registry)
}

/// Serialize parameters for TBPF input
fn serialize_parameters(params: &[ValueCell]) -> Result<Vec<u8>, VmError> {
    // Convert ValueCell array to borsh-serialized bytes
    borsh::to_vec(params)
        .map_err(|e| VmError::SerializationFailed(e.to_string()))
}
```

#### 2.3 Syscall Bridge

**Challenge**: Environment syscalls expect stack-based `FnParams`, but TBPF uses C-style syscalls.

**Solution**: Create a bridge layer that converts TBPF syscalls to Environment function calls.

```rust
// File: tos-vm/vm/src/syscall_bridge.rs

use solana_rbpf::syscalls::{SyscallObject, Result as SyscallResult};
use tos_vm::{Context, FnParams, ValueCell};

/// Syscall: tos_log (debug logging)
pub struct SyscallLog;

impl SyscallLog {
    pub fn init<'a>() -> Box<dyn SyscallObject<TbpfContext<'a>> + 'a> {
        Box::new(Self)
    }
}

impl<'a> SyscallObject<TbpfContext<'a>> for SyscallLog {
    fn call(
        &mut self,
        msg_ptr: u64,
        msg_len: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        memory: &mut [u8],
        context: &mut TbpfContext<'a>,
    ) -> SyscallResult<u64> {
        // Read message from TBPF memory
        let msg_bytes = &memory[msg_ptr as usize..(msg_ptr + msg_len) as usize];
        let msg_str = std::str::from_utf8(msg_bytes)?;

        // Convert to ValueCell parameter
        let params = vec![ValueCell::String(msg_str.to_string())];

        // Call the original "println" function from environment
        let env_context: &mut Context = context.context_data;
        let println_fn = env_context.get_function("println")
            .ok_or("println function not found")?;

        println_fn.call(params, env_context)?;

        Ok(0)
    }
}

// Similar bridges for other syscalls:
// - SyscallGetBalance → calls get_balance_for_asset()
// - SyscallTransfer → calls transfer()
// - SyscallStorageLoad → calls storage_load()
// ... etc
```

### Phase 3: Update tos-builder

**Minimal changes needed** - `EnvironmentBuilder` API stays the same, but internal function registration needs to support TBPF syscalls.

```rust
// File: tos-builder/src/lib.rs

impl EnvironmentBuilder {
    /// Register native function
    ///
    /// INTERNAL CHANGE: Now generates TBPF syscall bindings
    pub fn register_native_function(
        &mut self,
        name: &str,
        on_type: Option<Type>,
        params: Vec<(&str, Type)>,
        handler: fn(FnInstance, FnParams, &mut Context) -> FnReturnType,
        gas_cost: u64,
        return_type: Option<Type>,
    ) {
        // Store function metadata (unchanged)
        let fn_meta = FunctionMetadata {
            name: name.to_string(),
            on_type,
            params,
            handler,
            gas_cost,
            return_type,
        };

        self.functions.insert(name.to_string(), fn_meta);

        // NEW: Generate TBPF syscall name
        let syscall_name = format!("tos_{}", name);
        self.syscall_names.insert(name.to_string(), syscall_name);
    }

    /// Build environment
    pub fn build(self) -> Environment {
        Environment {
            functions: self.functions,
            syscall_names: self.syscall_names,
            opaque_types: self.opaque_types,
        }
    }
}
```

---

## Migration Guide for TOS Blockchain

### Required Changes in TOS Repository

**Minimal changes** - only update dependency version and contract compilation:

#### 1. Update Cargo.toml

```toml
# File: common/Cargo.toml

[dependencies]
# Update to new TBPF-based version
tos-vm = { git = "https://github.com/tos-network/tos-vm", branch = "feat/tbpf-engine" }
tos-builder = { git = "https://github.com/tos-network/tos-vm", branch = "feat/tbpf-engine" }
tos-environment = { git = "https://github.com/tos-network/tos-vm", branch = "feat/tbpf-engine" }
```

#### 2. Update Contract Compilation

**Old**: Compile to custom bytecode format
**New**: Compile to eBPF/ELF format

**Compiler**: Use `cargo build-bpf` (from Solana toolchain)

```bash
# Install Solana toolchain (for BPF compiler)
sh -c "$(curl -sSfL https://release.solana.com/stable/install)"

# Compile contract to eBPF
cd my_contract/
cargo build-bpf

# Output: target/deploy/my_contract.so (ELF format)
```

#### 3. No Code Changes Needed!

**Contract execution code remains unchanged**:

```rust
// File: common/src/transaction/verify/contract.rs

// THIS CODE STAYS EXACTLY THE SAME
let mut vm = VM::new(contract_environment.environment);
vm.append_module(contract_environment.module)?;
vm.invoke_entry_chunk(entry)?;
for constant in parameters.rev() {
    vm.push_stack(constant)?;
}
let context = vm.context_mut();
context.set_gas_limit(max_gas);
let result = vm.run()?;
```

---

## Implementation Timeline

**Faster than full replacement** because TOS code doesn't need to change:

| Phase | Duration | Work | Repository |
|-------|----------|------|------------|
| Phase 1: TBPF VM Core | 3-4 weeks | Implement Module, VM, syscall bridge | tos-vm |
| Phase 2: Testing | 2 weeks | Unit tests, integration tests | tos-vm |
| Phase 3: TOS Integration | 1 week | Update dependencies, test contracts | tos |
| **Total** | **6-7 weeks** | Complete TBPF migration | Both |

**Time savings**: 4-7 weeks faster than full TOS refactoring.

---

## Testing Strategy

### Unit Tests in tos-vm

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tbpf_module_loading() {
        let elf = include_bytes!("../tests/fixtures/hello.so");
        let module = Module::from_elf(elf.to_vec()).unwrap();

        assert!(module.entry_point(0).is_some());
    }

    #[test]
    fn test_tbpf_execution() {
        let env = EnvironmentBuilder::default().build();
        let mut vm = VM::new(&env);

        let elf = include_bytes!("../tests/fixtures/counter.so");
        let module = Module::from_elf(elf.to_vec()).unwrap();

        vm.append_module(&module).unwrap();
        vm.invoke_entry_chunk(0).unwrap();

        let result = vm.run().unwrap();
        assert_eq!(result.as_u64(), Some(0));
    }
}
```

### Integration Tests in TOS

```rust
#[tokio::test]
async fn test_tbpf_contract_deployment() {
    // Compile TBPF contract
    let elf = compile_bpf_contract("tests/contracts/token.rs");

    // Deploy via transaction
    let deploy_tx = create_deploy_transaction(elf);
    blockchain.process_transaction(deploy_tx).await.unwrap();

    // Verify contract is deployed
    let contract_hash = deploy_tx.hash();
    assert!(blockchain.contract_exists(&contract_hash).await);
}
```

---

## Benefits of This Approach

### ✅ Advantages

1. **Minimal TOS Changes**
   - Only dependency version update
   - No code refactoring needed
   - Low risk for TOS blockchain

2. **Clean Separation**
   - VM implementation isolated in tos-vm repo
   - TOS blockchain doesn't need to know about TBPF internals
   - Easier to test and debug

3. **Backward Compatible API**
   - All existing TOS integration code works
   - Same `VM::new()`, `vm.run()` interface
   - Same `Context`, `ValueCell` types

4. **Faster Development**
   - Can work on tos-vm independently
   - Parallel development possible
   - Easy to rollback if needed

5. **Future Flexibility**
   - Can add JIT compilation later
   - Can optimize TBPF internals without breaking TOS
   - Can experiment with different eBPF implementations

### ⚠️ Considerations

1. **Module Format Change**
   - Existing contracts need recompilation to eBPF
   - Need migration tooling for contract developers

2. **Stack vs Register Model**
   - Old VM: stack-based (push parameters)
   - New VM: register-based (serialize parameters)
   - Bridge layer handles conversion

3. **Testing Complexity**
   - Need comprehensive tests for API compatibility
   - Need to verify gas metering consistency
   - Need performance benchmarks

---

## Next Steps

### Immediate Actions

1. ✅ **Create tos-vm branch**
   ```bash
   cd tos-vm
   git checkout -b feat/tbpf-engine
   ```

2. ⏳ **Set up TBPF dependencies**
   ```toml
   # tos-vm/Cargo.toml
   [dependencies]
   solana-rbpf = "0.8"
   goblin = "0.8"  # ELF parsing
   borsh = "1.0"   # Serialization
   ```

3. ⏳ **Implement Module refactoring**
   - Replace internal bytecode with ELF
   - Keep Serializer interface
   - Add entry point/hook mapping

4. ⏳ **Implement VM refactoring**
   - Wrap TBPF VM
   - Keep public API (new, append_module, run, etc.)
   - Implement syscall bridge

5. ⏳ **Add unit tests**
   - Test Module loading
   - Test VM execution
   - Test gas metering

6. ⏳ **Test with TOS**
   - Update TOS dependency to new branch
   - Compile test contract to eBPF
   - Run integration tests

---

## Questions to Resolve

1. **TBPF Implementation**:
   - Use Solana's `rbpf` crate directly?
   - Or fork and customize for TOS?

2. **Entry Point Convention**:
   - Use ELF symbol names (`entry_0`, `hook_0`)?
   - Or use custom ELF section markers?

3. **Parameter Serialization**:
   - Use borsh (Solana standard)?
   - Or use bincode?

4. **JIT Compilation**:
   - Enable immediately?
   - Or add later for performance?

5. **Gas Metering**:
   - Use TBPF instruction counting?
   - Or custom gas calculation?

---

## References

- [Solana rbpf](https://github.com/solana-labs/rbpf)
- [eBPF Specification](https://www.kernel.org/doc/html/latest/bpf/instruction-set.html)
- [ELF Format](https://en.wikipedia.org/wiki/Executable_and_Linkable_Format)
- [Borsh Serialization](https://borsh.io/)

---

**Document Version**: 1.0
**Last Updated**: 2025-10-29
**Maintainer**: TOS Development Team
