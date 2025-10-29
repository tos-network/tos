# TOS-VM TBPF Implementation Guide

**Date**: 2025-10-29
**Strategy**: Interface-Compatible TBPF Refactoring
**Status**: Implementation Ready

---

## Overview

This guide provides **step-by-step instructions** for implementing TBPF in the `tos-vm` repository while maintaining full API compatibility with TOS blockchain.

**Repository**: https://github.com/tos-network/tos-vm
**Target Branch**: `feat/tbpf-engine`

---

## Prerequisites

### Required Tools

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install Solana toolchain (for BPF compiler and rbpf crate reference)
sh -c "$(curl -sSfL https://release.solana.com/stable/install)"

# Verify installation
solana --version
cargo --version
```

### Required Dependencies

```toml
# Will be added to tos-vm/Cargo.toml

[dependencies]
# TBPF execution engine
solana-rbpf = "0.8"

# ELF parsing
goblin = "0.8"

# Serialization
borsh = "1.0"

# Error handling
anyhow = "1"
thiserror = "2"
```

---

## Phase 1: Repository Setup (Day 1)

### Step 1.1: Clone and Branch

```bash
# Clone tos-vm repository
git clone https://github.com/tos-network/tos-vm.git
cd tos-vm

# Create feature branch
git checkout -b feat/tbpf-engine

# Push branch to remote
git push -u origin feat/tbpf-engine
```

### Step 1.2: Update Cargo.toml

**File**: `tos-vm/Cargo.toml`

```toml
[workspace]
members = [
    "vm",
    "builder",
    "types",
    "environment",
]

[workspace.dependencies]
# New TBPF dependencies
solana-rbpf = "0.8"
goblin = "0.8"
borsh = "1.0"
anyhow = "1"
thiserror = "2"

# Existing dependencies
serde = { version = "1", features = ["derive"] }
serde_json = "1"
log = "0.4"
```

**File**: `tos-vm/vm/Cargo.toml`

```toml
[package]
name = "tos-vm"
version = "0.2.0"  # Bump version for TBPF
edition = "2021"

[dependencies]
solana-rbpf = { workspace = true }
goblin = { workspace = true }
borsh = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }
serde = { workspace = true }
log = { workspace = true }

# Reference other workspace crates
tos-types = { path = "../types" }
tos-environment = { path = "../environment" }

[dev-dependencies]
tokio = { version = "1", features = ["full"] }
```

### Step 1.3: Create New Files

```bash
cd tos-vm/vm/src

# Create new TBPF-specific modules
touch tbpf_vm.rs       # TBPF VM wrapper
touch syscall_bridge.rs # Syscall adapter
touch elf_loader.rs    # ELF parsing and validation
```

Update `tos-vm/vm/src/lib.rs`:

```rust
// File: tos-vm/vm/src/lib.rs

mod module;
mod vm;
mod context;
mod value;

// NEW: TBPF implementation modules
mod tbpf_vm;
mod syscall_bridge;
mod elf_loader;

// Re-export public API (unchanged)
pub use module::Module;
pub use vm::VM;
pub use context::Context;
pub use value::{ValueCell, Primitive, Type};
pub use errors::VmError;

// Internal re-exports for TBPF
pub(crate) use tbpf_vm::TbpfVm;
pub(crate) use syscall_bridge::SyscallBridge;
pub(crate) use elf_loader::ElfLoader;
```

---

## Phase 2: Module Refactoring (Week 1)

### Step 2.1: Update Module Structure

**File**: `tos-vm/vm/src/module.rs`

```rust
use std::sync::Arc;
use std::collections::HashMap;
use goblin::elf::Elf;
use anyhow::{Result, Context as _};

/// Contract bytecode module
///
/// BREAKING CHANGE (Internal): Now stores ELF binary with eBPF bytecode
/// PUBLIC API: Unchanged
#[derive(Clone, Debug)]
pub struct Module {
    /// ELF binary (TBPF bytecode)
    elf: Arc<Vec<u8>>,

    /// Parsed ELF metadata (cached)
    elf_metadata: Arc<ElfMetadata>,

    /// Entry points: ID → function symbol name
    /// Maps invoke_entry_chunk(ID) to ELF function
    entry_points: HashMap<u16, String>,

    /// Hooks: ID → function symbol name
    /// Maps invoke_hook_id(ID) to ELF function
    hooks: HashMap<u8, String>,
}

#[derive(Debug)]
struct ElfMetadata {
    /// ELF object parsed by goblin
    elf: Elf<'static>,

    /// Text section offset
    text_section_offset: usize,

    /// Text section size
    text_section_size: usize,
}

impl Module {
    /// Create module from ELF bytecode
    ///
    /// NEW: Primary constructor for TBPF modules
    pub fn from_elf(elf_bytes: Vec<u8>) -> Result<Self> {
        // Parse ELF
        let elf_metadata = parse_elf(&elf_bytes)?;

        // Extract entry points from ELF symbols
        let entry_points = extract_entry_points(&elf_metadata)?;

        // Extract hooks from ELF symbols
        let hooks = extract_hooks(&elf_metadata)?;

        Ok(Self {
            elf: Arc::new(elf_bytes),
            elf_metadata: Arc::new(elf_metadata),
            entry_points,
            hooks,
        })
    }

    /// Create module from bytecode (legacy API)
    ///
    /// PUBLIC API: Unchanged for compatibility
    pub fn new(bytecode: Vec<u8>) -> Result<Self> {
        // Assume bytecode is ELF format
        Self::from_elf(bytecode)
    }

    /// Get ELF bytes (internal use)
    pub(crate) fn elf_bytes(&self) -> &[u8] {
        &self.elf
    }

    /// Get entry point function name by ID
    pub(crate) fn entry_point(&self, id: u16) -> Option<&str> {
        self.entry_points.get(&id).map(|s| s.as_str())
    }

    /// Get hook function name by ID
    pub(crate) fn hook(&self, id: u8) -> Option<&str> {
        self.hooks.get(&id).map(|s| s.as_str())
    }

    /// Get all entry point IDs
    pub(crate) fn entry_point_ids(&self) -> Vec<u16> {
        self.entry_points.keys().copied().collect()
    }

    /// Get all hook IDs
    pub(crate) fn hook_ids(&self) -> Vec<u8> {
        self.hooks.keys().copied().collect()
    }
}

/// Parse ELF binary and extract metadata
fn parse_elf(elf_bytes: &[u8]) -> Result<ElfMetadata> {
    // Parse ELF using goblin
    let elf = Elf::parse(elf_bytes)
        .context("Failed to parse ELF binary")?;

    // Verify it's a valid eBPF ELF
    if elf.header.e_machine != goblin::elf::header::EM_BPF {
        anyhow::bail!("Not a valid eBPF binary (wrong e_machine)");
    }

    // Find .text section (contains eBPF bytecode)
    let text_section = elf.section_headers.iter()
        .find(|sh| {
            if let Some(name) = elf.shdr_strtab.get_at(sh.sh_name) {
                name == ".text"
            } else {
                false
            }
        })
        .context("ELF missing .text section")?;

    Ok(ElfMetadata {
        elf,
        text_section_offset: text_section.sh_offset as usize,
        text_section_size: text_section.sh_size as usize,
    })
}

/// Extract entry points from ELF symbols
///
/// Convention: Functions named "entry_N" map to ID N
/// Example: "entry_0" → ID 0, "entry_1" → ID 1
fn extract_entry_points(metadata: &ElfMetadata) -> Result<HashMap<u16, String>> {
    let mut entry_points = HashMap::new();

    for sym in &metadata.elf.syms {
        if let Some(name) = metadata.elf.strtab.get_at(sym.st_name) {
            // Check if symbol name starts with "entry_"
            if let Some(id_str) = name.strip_prefix("entry_") {
                // Parse ID from name
                if let Ok(id) = id_str.parse::<u16>() {
                    entry_points.insert(id, name.to_string());
                    log::debug!("Found entry point: {} → ID {}", name, id);
                }
            }
        }
    }

    if entry_points.is_empty() {
        log::warn!("No entry points found in ELF (no 'entry_N' symbols)");
    }

    Ok(entry_points)
}

/// Extract hooks from ELF symbols
///
/// Convention: Functions named "hook_N" map to ID N
/// Example: "hook_0" → ID 0 (constructor hook)
fn extract_hooks(metadata: &ElfMetadata) -> Result<HashMap<u8, String>> {
    let mut hooks = HashMap::new();

    for sym in &metadata.elf.syms {
        if let Some(name) = metadata.elf.strtab.get_at(sym.st_name) {
            // Check if symbol name starts with "hook_"
            if let Some(id_str) = name.strip_prefix("hook_") {
                // Parse ID from name
                if let Ok(id) = id_str.parse::<u8>() {
                    hooks.insert(id, name.to_string());
                    log::debug!("Found hook: {} → ID {}", name, id);
                }
            }
        }
    }

    Ok(hooks)
}

// Keep existing Serializer implementation for TOS compatibility
impl crate::Serializer for Module {
    fn write(&self, writer: &mut Writer) {
        // Serialize ELF bytes
        writer.write_u32(self.elf.len() as u32);
        writer.write_bytes(&self.elf);

        // Serialize entry points
        writer.write_u16(self.entry_points.len() as u16);
        for (id, name) in &self.entry_points {
            writer.write_u16(*id);
            writer.write_string(name);
        }

        // Serialize hooks
        writer.write_u8(self.hooks.len() as u8);
        for (id, name) in &self.hooks {
            writer.write_u8(*id);
            writer.write_string(name);
        }
    }

    fn read(reader: &mut Reader) -> Result<Self, ReaderError> {
        // Read ELF bytes
        let elf_len = reader.read_u32()? as usize;

        // Enforce maximum contract size (1 MB)
        const MAX_CONTRACT_SIZE: usize = 1024 * 1024;
        if elf_len > MAX_CONTRACT_SIZE {
            return Err(ReaderError::InvalidSize);
        }

        let mut elf_bytes = vec![0u8; elf_len];
        reader.read_bytes_into(&mut elf_bytes)?;

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

        // Parse ELF metadata
        let elf_metadata = parse_elf(&elf_bytes)
            .map_err(|_| ReaderError::InvalidValue)?;

        Ok(Self {
            elf: Arc::new(elf_bytes),
            elf_metadata: Arc::new(elf_metadata),
            entry_points,
            hooks,
        })
    }

    fn size(&self) -> usize {
        let mut size = 4 + self.elf.len(); // ELF bytes

        // Entry points
        size += 2; // count
        for (_, name) in &self.entry_points {
            size += 2 + 4 + name.len(); // id + string
        }

        // Hooks
        size += 1; // count
        for (_, name) in &self.hooks {
            size += 1 + 4 + name.len(); // id + string
        }

        size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_from_elf() {
        // Test with a minimal valid eBPF ELF
        let elf = include_bytes!("../tests/fixtures/minimal.so");
        let module = Module::from_elf(elf.to_vec()).unwrap();

        assert!(!module.elf_bytes().is_empty());
    }

    #[test]
    fn test_entry_point_extraction() {
        // Test entry point extraction
        let elf = include_bytes!("../tests/fixtures/with_entries.so");
        let module = Module::from_elf(elf.to_vec()).unwrap();

        assert!(module.entry_point(0).is_some());
        assert_eq!(module.entry_point(0), Some("entry_0"));
    }
}
```

---

## Phase 3: VM Wrapper Implementation (Week 1-2)

### Step 3.1: Create TBPF VM Wrapper

**File**: `tos-vm/vm/src/tbpf_vm.rs`

```rust
use solana_rbpf::{
    ebpf,
    vm::{Config, EbpfVm},
    verifier,
};
use anyhow::{Result, Context as _};
use crate::Module;

/// TBPF VM configuration
pub(crate) struct TbpfConfig {
    /// Enable instruction metering (for gas)
    pub enable_instruction_meter: bool,

    /// Max instruction count
    pub max_instruction_count: u64,

    /// Enable JIT compilation
    pub enable_jit: bool,
}

impl Default for TbpfConfig {
    fn default() -> Self {
        Self {
            enable_instruction_meter: true,
            max_instruction_count: 1_400_000, // Solana's max
            enable_jit: false, // Disable JIT initially for stability
        }
    }
}

/// TBPF VM instance (wrapper around solana-rbpf)
pub(crate) struct TbpfVm<'a> {
    /// Underlying eBPF VM
    vm: EbpfVm<'a>,

    /// Configuration
    config: TbpfConfig,

    /// Instruction count (for gas metering)
    instruction_count: u64,
}

impl<'a> TbpfVm<'a> {
    /// Create TBPF VM from module
    pub fn new(
        module: &Module,
        syscalls: &'a SyscallRegistry,
        config: TbpfConfig,
    ) -> Result<Self> {
        // Verify bytecode
        verifier::check(module.elf_bytes())
            .context("eBPF bytecode verification failed")?;

        // Create rbpf config
        let rbpf_config = Config {
            enable_instruction_meter: config.enable_instruction_meter,
            max_instruction_count: config.max_instruction_count,
            ..Config::default()
        };

        // Create eBPF VM
        let vm = EbpfVm::new(
            module.elf_bytes(),
            &rbpf_config,
            syscalls,
        ).context("Failed to create eBPF VM")?;

        Ok(Self {
            vm,
            config,
            instruction_count: 0,
        })
    }

    /// Execute function by name
    pub fn execute(
        &mut self,
        function_name: &str,
        input_data: &[u8],
        context: &mut impl std::any::Any,
    ) -> Result<u64> {
        // Execute the function
        let result = self.vm.execute_program_with_context(
            input_data,
            context,
        ).context("eBPF execution failed")?;

        // Get instruction count
        if self.config.enable_instruction_meter {
            self.instruction_count = self.vm.get_total_instruction_count();
        }

        Ok(result)
    }

    /// Get instruction count (for gas calculation)
    pub fn instruction_count(&self) -> u64 {
        self.instruction_count
    }
}
```

### Step 3.2: Update VM Public API

**File**: `tos-vm/vm/src/vm.rs`

```rust
use anyhow::{Result, Context as _};
use crate::{
    Module,
    Context,
    ValueCell,
    VmError,
    tbpf_vm::{TbpfVm, TbpfConfig},
    syscall_bridge::build_syscall_registry,
};

/// Virtual Machine for executing smart contracts
///
/// PUBLIC API: Unchanged
/// INTERNAL: Now uses TBPF (eBPF) execution engine
pub struct VM<'a> {
    /// Environment (for syscall registration)
    environment: &'a Environment,

    /// Loaded module
    module: Option<Module>,

    /// Execution context
    context: Context,

    /// Entry point to invoke
    entry_point: Option<String>,

    /// Parameters (collected via push_stack)
    parameters: Vec<ValueCell>,

    /// TBPF configuration
    tbpf_config: TbpfConfig,
}

impl<'a> VM<'a> {
    /// Create new VM instance
    ///
    /// PUBLIC API: Unchanged
    pub fn new(environment: &'a Environment) -> Self {
        Self {
            environment,
            module: None,
            context: Context::new(),
            entry_point: None,
            parameters: Vec::new(),
            tbpf_config: TbpfConfig::default(),
        }
    }

    /// Load bytecode module
    ///
    /// PUBLIC API: Unchanged
    pub fn append_module(&mut self, module: &Module) -> Result<(), VmError> {
        self.module = Some(module.clone());
        Ok(())
    }

    /// Invoke entry point by ID
    ///
    /// PUBLIC API: Unchanged
    pub fn invoke_entry_chunk(&mut self, entry: u16) -> Result<(), VmError> {
        let module = self.module.as_ref()
            .ok_or(VmError::NoModuleLoaded)?;

        let entry_name = module.entry_point(entry)
            .ok_or(VmError::EntryPointNotFound(entry))?;

        self.entry_point = Some(entry_name.to_string());
        Ok(())
    }

    /// Invoke hook by ID
    ///
    /// PUBLIC API: Unchanged
    pub fn invoke_hook_id(&mut self, hook: u8) -> Result<bool, VmError> {
        let module = self.module.as_ref()
            .ok_or(VmError::NoModuleLoaded)?;

        if let Some(hook_name) = module.hook(hook) {
            self.entry_point = Some(hook_name.to_string());
            Ok(true)
        } else {
            Ok(false) // Hook not found
        }
    }

    /// Push parameter to stack
    ///
    /// PUBLIC API: Unchanged
    /// INTERNAL: Collects parameters instead of pushing to stack
    pub fn push_stack(&mut self, value: ValueCell) -> Result<(), VmError> {
        self.parameters.push(value);
        Ok(())
    }

    /// Get mutable context
    ///
    /// PUBLIC API: Unchanged
    pub fn context_mut(&mut self) -> &mut Context {
        &mut self.context
    }

    /// Get immutable context
    ///
    /// PUBLIC API: Unchanged
    pub fn context(&self) -> &Context {
        &self.context
    }

    /// Execute the VM
    ///
    /// PUBLIC API: Unchanged
    /// INTERNAL: Uses TBPF instead of stack-based VM
    pub fn run(&mut self) -> Result<ValueCell, VmError> {
        let module = self.module.as_ref()
            .ok_or(VmError::NoModuleLoaded)?;

        let entry_fn = self.entry_point.as_ref()
            .ok_or(VmError::NoEntryPointSet)?;

        // Build syscall registry from environment
        let syscalls = build_syscall_registry(self.environment, &mut self.context)?;

        // Create TBPF VM
        let mut tbpf_vm = TbpfVm::new(
            module,
            &syscalls,
            self.tbpf_config.clone(),
        )?;

        // Serialize parameters to input buffer
        let input_data = serialize_parameters(&self.parameters)?;

        // Execute
        let result = tbpf_vm.execute(
            entry_fn,
            &input_data,
            &mut self.context,
        )?;

        // Update gas usage
        let instruction_count = tbpf_vm.instruction_count();
        self.context.set_gas_used(instruction_count);

        // Convert result to ValueCell
        Ok(ValueCell::U64(result))
    }
}

/// Serialize parameters for TBPF input
fn serialize_parameters(params: &[ValueCell]) -> Result<Vec<u8>, VmError> {
    borsh::to_vec(params)
        .map_err(|e| VmError::SerializationFailed(e.to_string()))
}
```

---

## Phase 4: Syscall Bridge (Week 2-3)

### Step 4.1: Create Syscall Bridge

**File**: `tos-vm/vm/src/syscall_bridge.rs`

```rust
use solana_rbpf::syscalls::{SyscallObject, SyscallRegistry, Result as SyscallResult};
use crate::{Context, Environment, ValueCell, FnParams};

/// Build syscall registry from environment
pub(crate) fn build_syscall_registry<'a>(
    env: &'a Environment,
    context: &'a mut Context,
) -> anyhow::Result<SyscallRegistry<'a>> {
    let mut registry = SyscallRegistry::default();

    // Register all syscalls from environment
    // Each function in EnvironmentBuilder becomes an eBPF syscall

    // Example: "println" → "tos_log"
    if env.has_function("println") {
        registry.register_syscall_by_name(
            b"tos_log",
            || Box::new(SyscallLog::new(env, context)),
        )?;
    }

    // Example: "get_balance_for_asset" → "tos_get_balance"
    if env.has_function("get_balance_for_asset") {
        registry.register_syscall_by_name(
            b"tos_get_balance",
            || Box::new(SyscallGetBalance::new(env, context)),
        )?;
    }

    // ... register all other syscalls ...

    Ok(registry)
}

/// Syscall: tos_log (debug logging)
struct SyscallLog<'a> {
    env: &'a Environment,
    context: &'a mut Context,
}

impl<'a> SyscallLog<'a> {
    fn new(env: &'a Environment, context: &'a mut Context) -> Self {
        Self { env, context }
    }
}

impl<'a> SyscallObject for SyscallLog<'a> {
    fn call(
        &mut self,
        msg_ptr: u64,
        msg_len: u64,
        _arg3: u64,
        _arg4: u64,
        _arg5: u64,
        memory: &mut [u8],
    ) -> SyscallResult<u64> {
        // Validate memory access
        validate_memory_access(msg_ptr, msg_len, memory)?;

        // Read message from eBPF memory
        let msg_bytes = &memory[msg_ptr as usize..(msg_ptr + msg_len) as usize];
        let msg_str = std::str::from_utf8(msg_bytes)
            .map_err(|_| "Invalid UTF-8")?;

        // Call the original "println" function from environment
        let params = vec![ValueCell::String(msg_str.to_string())];
        self.env.call_function("println", params, self.context)?;

        Ok(0)
    }
}

/// Helper: Validate memory access
fn validate_memory_access(ptr: u64, len: u64, memory: &[u8]) -> SyscallResult<()> {
    let end = ptr.checked_add(len)
        .ok_or("Pointer overflow")?;

    if end as usize > memory.len() {
        return Err("Memory access violation".into());
    }

    Ok(())
}
```

---

## Testing Strategy

### Unit Tests

**File**: `tos-vm/vm/tests/module_tests.rs`

```rust
use tos_vm::Module;

#[test]
fn test_module_from_elf() {
    let elf = include_bytes!("fixtures/minimal.so");
    let module = Module::from_elf(elf.to_vec()).unwrap();
    assert!(!module.elf_bytes().is_empty());
}

#[test]
fn test_entry_points() {
    let elf = include_bytes!("fixtures/counter.so");
    let module = Module::from_elf(elf.to_vec()).unwrap();

    assert!(module.entry_point(0).is_some());
    assert_eq!(module.entry_point(0), Some("entry_0"));
}
```

### Integration Tests

**File**: `tos-vm/vm/tests/vm_tests.rs`

```rust
use tos_vm::{VM, Module, Environment};

#[test]
fn test_vm_execution() {
    let env = Environment::default();
    let mut vm = VM::new(&env);

    let elf = include_bytes!("fixtures/hello.so");
    let module = Module::from_elf(elf.to_vec()).unwrap();

    vm.append_module(&module).unwrap();
    vm.invoke_entry_chunk(0).unwrap();

    let result = vm.run().unwrap();
    assert_eq!(result.as_u64(), Some(0));
}
```

---

## Integration with TOS Blockchain

### Update TOS Dependency

**File**: `tos/common/Cargo.toml`

```toml
[dependencies]
# Update to TBPF branch
tos-vm = { git = "https://github.com/tos-network/tos-vm", branch = "feat/tbpf-engine" }
tos-builder = { git = "https://github.com/tos-network/tos-vm", branch = "feat/tbpf-engine" }
tos-environment = { git = "https://github.com/tos-network/tos-vm", branch = "feat/tbpf-engine" }
```

### No Code Changes Needed!

Existing TOS code continues to work:

```rust
// File: common/src/transaction/verify/contract.rs

// THIS CODE REMAINS UNCHANGED
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

## Contract Compilation Guide

### Writing TBPF Contracts

**File**: `my_contract/src/lib.rs`

```rust
use borsh::{BorshSerialize, BorshDeserialize};

// Entry point 0: constructor
#[no_mangle]
#[export_name = "entry_0"]
pub extern "C" fn constructor() -> u64 {
    // Constructor logic
    tos_log("Contract deployed!");
    0 // Success
}

// Entry point 1: transfer
#[no_mangle]
#[export_name = "entry_1"]
pub extern "C" fn transfer() -> u64 {
    // Transfer logic
    0
}

// Hook 0: constructor hook
#[no_mangle]
#[export_name = "hook_0"]
pub extern "C" fn on_deploy() -> u64 {
    0
}

// Syscall bindings
extern "C" {
    fn tos_log(msg_ptr: *const u8, msg_len: u64);
}

fn tos_log(msg: &str) {
    unsafe {
        tos_log(msg.as_ptr(), msg.len() as u64);
    }
}
```

### Compiling to eBPF

```bash
# Install Solana toolchain
sh -c "$(curl -sSfL https://release.solana.com/stable/install)"

# Compile contract
cd my_contract
cargo build-bpf

# Output: target/deploy/my_contract.so (ELF format)
```

---

## Timeline

| Week | Tasks | Deliverables |
|------|-------|--------------|
| Week 1 | Setup + Module refactoring | Module with ELF support |
| Week 2 | VM wrapper implementation | VM using TBPF internally |
| Week 3 | Syscall bridge | All syscalls working |
| Week 4 | Testing | Unit + integration tests |
| Week 5 | TOS integration | Updated dependency |
| Week 6 | Final testing | Production ready |

**Total**: 6 weeks

---

## Checklist

### Phase 1: Setup
- [ ] Clone tos-vm repository
- [ ] Create `feat/tbpf-engine` branch
- [ ] Update Cargo.toml dependencies
- [ ] Create new module files

### Phase 2: Module
- [ ] Implement Module::from_elf()
- [ ] Implement ELF parsing
- [ ] Extract entry points and hooks
- [ ] Keep Serializer compatibility
- [ ] Add unit tests

### Phase 3: VM
- [ ] Create TbpfVm wrapper
- [ ] Update VM::run() to use TBPF
- [ ] Keep public API unchanged
- [ ] Add integration tests

### Phase 4: Syscalls
- [ ] Build syscall registry
- [ ] Implement syscall bridges
- [ ] Test all syscalls
- [ ] Verify gas metering

### Phase 5: Integration
- [ ] Update TOS dependency
- [ ] Compile test contract
- [ ] Run TOS integration tests
- [ ] Performance benchmarks

---

**Ready to start implementation!** 🚀

Next step: Set up the tos-vm repository and begin Phase 1.
