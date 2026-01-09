//! Fuzz target for contract bytecode parsing
//!
//! Tests that arbitrary byte sequences do not cause panics
//! when parsed as contract bytecode.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

/// Contract bytecode input
#[derive(Debug, Arbitrary)]
struct ContractInput {
    /// Bytecode
    bytecode: Vec<u8>,
    /// Constructor arguments
    constructor_args: Vec<u8>,
    /// Call data
    call_data: Vec<u8>,
}

/// Simplified opcode definitions for parsing
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
enum Opcode {
    Stop = 0x00,
    Add = 0x01,
    Mul = 0x02,
    Sub = 0x03,
    Div = 0x04,
    Mod = 0x06,
    Exp = 0x0A,
    Lt = 0x10,
    Gt = 0x11,
    Eq = 0x14,
    IsZero = 0x15,
    And = 0x16,
    Or = 0x17,
    Xor = 0x18,
    Not = 0x19,
    Sha3 = 0x20,
    Address = 0x30,
    Balance = 0x31,
    Caller = 0x33,
    CallValue = 0x34,
    CallDataLoad = 0x35,
    CallDataSize = 0x36,
    CallDataCopy = 0x37,
    CodeSize = 0x38,
    CodeCopy = 0x39,
    GasPrice = 0x3A,
    ExtCodeSize = 0x3B,
    ExtCodeCopy = 0x3C,
    ReturnDataSize = 0x3D,
    ReturnDataCopy = 0x3E,
    ExtCodeHash = 0x3F,
    BlockHash = 0x40,
    Coinbase = 0x41,
    Timestamp = 0x42,
    Number = 0x43,
    Difficulty = 0x44,
    GasLimit = 0x45,
    ChainId = 0x46,
    SelfBalance = 0x47,
    Pop = 0x50,
    MLoad = 0x51,
    MStore = 0x52,
    MStore8 = 0x53,
    SLoad = 0x54,
    SStore = 0x55,
    Jump = 0x56,
    JumpI = 0x57,
    Pc = 0x58,
    MSize = 0x59,
    Gas = 0x5A,
    JumpDest = 0x5B,
    Push1 = 0x60,
    Push32 = 0x7F,
    Dup1 = 0x80,
    Swap1 = 0x90,
    Log0 = 0xA0,
    Create = 0xF0,
    Call = 0xF1,
    CallCode = 0xF2,
    Return = 0xF3,
    DelegateCall = 0xF4,
    Create2 = 0xF5,
    StaticCall = 0xFA,
    Revert = 0xFD,
    Invalid = 0xFE,
    SelfDestruct = 0xFF,
}

fuzz_target!(|input: ContractInput| {
    // Limit bytecode size to prevent OOM
    if input.bytecode.len() > 24576 || input.bytecode.is_empty() {
        return;
    }

    // Parse bytecode to extract opcodes
    let _ = parse_bytecode(&input.bytecode);

    // Validate bytecode structure
    let _ = validate_bytecode(&input.bytecode);

    // Check for valid JUMPDEST targets
    let _ = find_jumpdests(&input.bytecode);

    // Parse call data as function selector + args
    if input.call_data.len() >= 4 {
        let _selector = &input.call_data[..4];
        let _args = &input.call_data[4..];
    }
});

/// Parse bytecode into instruction list
fn parse_bytecode(bytecode: &[u8]) -> Vec<(usize, u8, Vec<u8>)> {
    let mut instructions = Vec::new();
    let mut pc = 0;

    while pc < bytecode.len() {
        let opcode = bytecode[pc];
        let mut immediate = Vec::new();

        // Handle PUSH instructions (0x60-0x7F)
        if (0x60..=0x7F).contains(&opcode) {
            let push_size = (opcode - 0x5F) as usize;
            for i in 1..=push_size {
                if pc + i < bytecode.len() {
                    immediate.push(bytecode[pc + i]);
                }
            }
            pc += push_size;
        }

        instructions.push((pc, opcode, immediate));
        pc += 1;
    }

    instructions
}

/// Validate bytecode structure
fn validate_bytecode(bytecode: &[u8]) -> Result<(), &'static str> {
    let mut pc = 0;

    while pc < bytecode.len() {
        let opcode = bytecode[pc];

        // Check for invalid opcodes
        if opcode == 0xFE {
            // INVALID opcode found - not necessarily an error
        }

        // Handle PUSH instructions
        if (0x60..=0x7F).contains(&opcode) {
            let push_size = (opcode - 0x5F) as usize;
            if pc + push_size >= bytecode.len() {
                return Err("PUSH instruction truncated");
            }
            pc += push_size;
        }

        pc += 1;
    }

    Ok(())
}

/// Find all JUMPDEST positions
fn find_jumpdests(bytecode: &[u8]) -> Vec<usize> {
    let mut jumpdests = Vec::new();
    let mut pc = 0;

    while pc < bytecode.len() {
        let opcode = bytecode[pc];

        if opcode == 0x5B {
            // JUMPDEST
            jumpdests.push(pc);
        }

        // Skip PUSH immediate data
        if (0x60..=0x7F).contains(&opcode) {
            let push_size = (opcode - 0x5F) as usize;
            pc += push_size;
        }

        pc += 1;
    }

    jumpdests
}
