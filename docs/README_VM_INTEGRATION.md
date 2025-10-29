# TOS VM Integration: Complete Documentation

**Last Updated**: 2025-10-29
**Status**: Planning Complete, Ready for Implementation

---

## Overview

This directory contains comprehensive documentation for integrating TBPF (Solana-style eBPF) execution engine into TOS blockchain.

**Selected Strategy**: ✅ **Option 2 - tos-vm Refactoring** (Interface-Compatible)

---

## Documentation Index

### 1. **VM_ENGINE_INTEGRATION_PLAN.md** (1,000+ lines)
**Purpose**: Full TOS blockchain refactoring approach (Alternative)

**Contents**:
- Complete TBPF replacement strategy
- Full blockchain integration plan
- 10-14 week timeline
- Detailed syscall implementations

**Status**: ❌ **Not Selected** (kept for reference)

**Use Case**: Reference if you need to understand the full blockchain refactoring approach

---

### 2. **TOS_VM_TBPF_REFACTORING_PLAN.md** ⭐ (932 lines)
**Purpose**: Interface-compatible tos-vm refactoring (SELECTED)

**Contents**:
- Current tos-vm API analysis
- TBPF implementation strategy
- API compatibility approach
- 6-7 week timeline
- Benefits and considerations

**Status**: ✅ **SELECTED STRATEGY**

**Use Case**: Understand the overall strategy and architecture

---

### 3. **TOS_VM_IMPLEMENTATION_GUIDE.md** ⭐⭐ (1,020 lines)
**Purpose**: Step-by-step implementation guide (ACTION PLAN)

**Contents**:
- Prerequisites and setup
- Phase-by-phase implementation (6 weeks)
- Complete code templates
- Module, VM, and syscall implementations
- Testing strategy
- Integration guide

**Status**: ✅ **READY TO IMPLEMENT**

**Use Case**: Follow this guide to implement the TBPF refactoring

---

## Quick Start

### For Understanding the Strategy

1. Read: `TOS_VM_TBPF_REFACTORING_PLAN.md`
2. Understand: Why interface-compatible refactoring is better
3. Review: API compatibility requirements

### For Implementation

1. **Start Here**: `TOS_VM_IMPLEMENTATION_GUIDE.md`
2. **Follow**: Phase 1 → Phase 2 → Phase 3 → Phase 4
3. **Timeline**: 6 weeks total

---

## Strategy Summary

### Selected Approach: tos-vm Refactoring

**Repository**: `https://github.com/tos-network/tos-vm`
**Branch**: `feat/tbpf-engine`

**Key Principle**: Replace VM internals, keep API unchanged

### What Changes

| Component | Before | After |
|-----------|--------|-------|
| Bytecode Format | Custom format | ELF with eBPF |
| Execution Engine | Stack-based VM | TBPF (register-based) |
| Internal Implementation | Custom interpreter | solana-rbpf wrapper |

### What Stays the Same

| Component | Interface |
|-----------|-----------|
| `VM::new()` | ✅ Unchanged |
| `VM::append_module()` | ✅ Unchanged |
| `VM::run()` | ✅ Unchanged |
| `Module` structure | ✅ Unchanged (public API) |
| `Context` API | ✅ Unchanged |
| TOS blockchain code | ✅ **No changes needed!** |

---

## Implementation Phases

### Phase 1: Setup (Day 1)
- Clone tos-vm repository
- Create `feat/tbpf-engine` branch
- Update dependencies (solana-rbpf, goblin, borsh)

### Phase 2: Module Refactoring (Week 1)
- Implement `Module::from_elf()`
- Parse ELF symbols for entry points and hooks
- Keep Serializer interface compatible

### Phase 3: VM Wrapper (Week 1-2)
- Create TbpfVm wrapper around solana-rbpf
- Update `VM::run()` to use TBPF
- Keep public API unchanged

### Phase 4: Syscall Bridge (Week 2-3)
- Convert environment functions to eBPF syscalls
- Implement SyscallObject for each function
- Maintain gas metering

### Phase 5: Testing (Week 4)
- Unit tests for Module and VM
- Integration tests
- Gas metering verification

### Phase 6: TOS Integration (Week 5-6)
- Update TOS dependency to new branch
- Compile test contracts to eBPF
- Run full integration tests

---

## Key Benefits

### ✅ Minimal TOS Changes
- Only dependency version update in `Cargo.toml`
- No code refactoring needed
- Low risk for TOS blockchain

### ✅ Faster Development
- 6-7 weeks (vs 10-14 weeks for full refactoring)
- Can work on tos-vm independently
- Parallel development possible

### ✅ Easy Rollback
- Changes isolated to tos-vm repository
- Can revert by switching branch
- No breaking changes to TOS

### ✅ Clean Architecture
- VM logic isolated in tos-vm
- TOS doesn't know about TBPF internals
- Better separation of concerns

---

## Timeline

| Week | Phase | Work | Deliverable |
|------|-------|------|-------------|
| 1 | Setup + Module | ELF support | Module with ELF parsing |
| 2 | VM Wrapper | TBPF integration | VM using TBPF |
| 3 | Syscall Bridge | Syscall adapters | All syscalls working |
| 4 | Testing | Unit + integration | Test suite passing |
| 5 | TOS Integration | Update dependency | TOS using TBPF VM |
| 6 | Final Testing | End-to-end tests | Production ready |

**Total**: 6 weeks

---

## Contract Development

### Writing Contracts

**Old** (Custom bytecode):
```rust
// Custom contract format (deprecated)
```

**New** (eBPF):
```rust
// Entry point 0
#[no_mangle]
#[export_name = "entry_0"]
pub extern "C" fn constructor() -> u64 {
    tos_log("Contract deployed!");
    0
}

// Entry point 1
#[no_mangle]
#[export_name = "entry_1"]
pub extern "C" fn transfer() -> u64 {
    // Transfer logic
    0
}
```

### Compiling Contracts

```bash
# Install Solana toolchain (for BPF compiler)
sh -c "$(curl -sSfL https://release.solana.com/stable/install)"

# Compile contract to eBPF
cd my_contract
cargo build-bpf

# Output: target/deploy/my_contract.so (ELF format)
```

---

## Integration with TOS

### Updating Dependency

**File**: `tos/common/Cargo.toml`

```diff
[dependencies]
-tos-vm = { git = "https://github.com/tos-network/tos-vm", branch = "dev" }
+tos-vm = { git = "https://github.com/tos-network/tos-vm", branch = "feat/tbpf-engine" }
```

### No Code Changes!

Existing TOS code continues to work:

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

## Testing Strategy

### Unit Tests (tos-vm)
- Module ELF parsing
- Entry point extraction
- VM execution
- Gas metering

### Integration Tests (tos-vm)
- Full contract execution
- Syscall functionality
- Error handling

### TOS Integration Tests
- Contract deployment
- Contract invocation
- State management
- Gas refunds

---

## Success Criteria

### ✅ Phase 1 Complete
- [ ] tos-vm branch created
- [ ] Dependencies updated
- [ ] Build successful

### ✅ Phase 2 Complete
- [ ] Module supports ELF
- [ ] Entry points extracted
- [ ] Serializer works

### ✅ Phase 3 Complete
- [ ] VM uses TBPF
- [ ] Public API unchanged
- [ ] Basic execution works

### ✅ Phase 4 Complete
- [ ] All syscalls implemented
- [ ] Gas metering accurate
- [ ] Tests passing

### ✅ Phase 5 Complete
- [ ] TOS dependency updated
- [ ] Integration tests pass
- [ ] Contracts deploying

### ✅ Production Ready
- [ ] All tests passing
- [ ] Performance acceptable
- [ ] Documentation complete
- [ ] Ready for mainnet

---

## FAQ

### Q: Do we need to change TOS blockchain code?
**A**: No! Only update the dependency version in `Cargo.toml`.

### Q: Will existing contracts work?
**A**: They need to be recompiled to eBPF format, but the API is the same.

### Q: Can we rollback if issues arise?
**A**: Yes, easily. Just switch the tos-vm branch back to `dev`.

### Q: How long will this take?
**A**: 6 weeks with the phased approach in the implementation guide.

### Q: What about performance?
**A**: eBPF should be 2-5x faster (interpreter) or 10-50x faster (with JIT).

### Q: Is this compatible with Solana?
**A**: The bytecode format is compatible, but syscalls are TOS-specific.

---

## Next Steps

### Immediate Actions

1. ✅ **Review Documents**
   - Read `TOS_VM_TBPF_REFACTORING_PLAN.md`
   - Read `TOS_VM_IMPLEMENTATION_GUIDE.md`

2. ⏳ **Set Up Environment**
   - Clone tos-vm repository
   - Install Solana toolchain
   - Create feature branch

3. ⏳ **Start Implementation**
   - Follow Phase 1 in implementation guide
   - Update dependencies
   - Create new module files

4. ⏳ **Begin Coding**
   - Implement Module refactoring
   - Add ELF parsing
   - Write unit tests

---

## Support & References

### Documentation
- Solana sBPF: https://solana.com/docs/programs/faq#berkeley-packet-filter-bpf
- solana-rbpf: https://github.com/solana-labs/rbpf
- eBPF Spec: https://www.kernel.org/doc/html/latest/bpf/instruction-set.html

### Code References
- `TOS_VM_IMPLEMENTATION_GUIDE.md` - Complete code templates
- `common/src/contract/mod.rs` - Existing syscall implementations
- `common/src/transaction/verify/contract.rs` - VM usage example

---

## Conclusion

This documentation provides everything needed to successfully integrate TBPF into TOS blockchain with minimal risk and maximum efficiency.

**Strategy**: ✅ Interface-compatible tos-vm refactoring
**Timeline**: 6 weeks
**Risk Level**: Low
**TOS Changes**: Minimal (dependency update only)

**Ready to start implementation!** 🚀

Follow `TOS_VM_IMPLEMENTATION_GUIDE.md` for detailed step-by-step instructions.

---

**Last Updated**: 2025-10-29
**Maintainer**: TOS Development Team
