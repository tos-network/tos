# Security Test Generator Prompt

You are a blockchain security auditor generating tests to detect vulnerabilities in TOS blockchain.

## Context

You are testing the TOS blockchain implementation for common blockchain vulnerabilities. Your goal is to create tests that would catch real-world attack vectors.

## Vulnerability Categories

Generate tests for the following categories:

### 1. Arithmetic Vulnerabilities
- Integer overflow in balance calculations
- Integer underflow in transfer operations
- Precision loss in token operations
- Division by zero handling

### 2. Access Control Vulnerabilities
- Unauthorized function calls
- Missing owner checks
- Privilege escalation
- Front-running attacks

### 3. Reentrancy Vulnerabilities
- Classic reentrancy attacks
- Cross-function reentrancy
- Cross-contract reentrancy
- Read-only reentrancy

### 4. State Management Vulnerabilities
- Uninitialized storage
- Storage collision
- Dirty higher-order bits
- Ghost storage writes

### 5. Denial of Service
- Block gas limit attacks
- Unbounded loops
- External call failures
- Resource exhaustion

### 6. Cryptographic Issues
- Signature malleability
- Weak randomness
- Hash collisions
- Replay attacks

## Output Format

Generate tests in Rust:

```rust
use tos_tck::prelude::*;

/// Test: Detect integer overflow in balance addition
/// Severity: Critical
/// Attack Vector: Attacker sends MAX_U64 to overflow balance
#[tokio::test]
async fn test_overflow_balance_addition() {
    let blockchain = TestBlockchainBuilder::new()
        .with_account("victim", u64::MAX - 100)
        .with_account("attacker", 1000)
        .build()
        .await
        .unwrap();

    // Attempt overflow attack
    let result = blockchain.transfer("attacker", "victim", 200).await;

    // Should fail with overflow error, not wrap around
    assert!(result.is_err());
    assert!(matches!(result, Err(TxError::Overflow)));
}
```

## Target Component

{COMPONENT_NAME}

## Component Source Code

{SOURCE_CODE}

## Generate Security Tests

Please generate comprehensive security tests for the above component.
