# Syscall Test Generator Prompt

You are a blockchain security expert generating comprehensive test cases for TOS blockchain syscalls.

## Context

TOS is a blockchain with smart contract support using a custom VM. The syscalls are similar to Ethereum's EVM opcodes but with some differences.

## Task

Generate test cases for the following syscall: `{SYSCALL_NAME}`

## Requirements

For each syscall, generate tests covering:

1. **Happy Path Tests**
   - Normal successful operation
   - Boundary values (min, max)
   - Typical use cases

2. **Error Cases**
   - Invalid input types
   - Out of bounds values
   - Insufficient permissions
   - Resource exhaustion

3. **Security Tests**
   - Overflow/underflow attempts
   - Reentrancy scenarios (if applicable)
   - Access control violations
   - Gas exhaustion attacks

4. **Edge Cases**
   - Empty inputs
   - Maximum length inputs
   - Zero values
   - Self-referential operations

## Output Format

Generate tests in Rust using the TOS-TCK framework:

```rust
use tos_tck::prelude::*;

#[tokio::test]
async fn test_{syscall_name}_happy_path() {
    let blockchain = TestBlockchainBuilder::new()
        .with_account("alice", 1000000000)
        .build()
        .await
        .unwrap();

    // Test implementation
}

#[tokio::test]
async fn test_{syscall_name}_insufficient_balance() {
    // Error case test
}

#[tokio::test]
async fn test_{syscall_name}_overflow_protection() {
    // Security test
}
```

## Syscall Reference

{SYSCALL_DOCUMENTATION}

## Generate Tests

Please generate comprehensive test cases following the above requirements.
