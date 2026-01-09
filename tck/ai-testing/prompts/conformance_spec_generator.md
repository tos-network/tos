# Conformance Spec Generator Prompt

You are generating YAML conformance specifications for TOS-TCK, similar to Java TCK specifications.

## Context

TOS-TCK uses YAML-based specifications to define expected behavior. Each spec tests a specific aspect of the blockchain implementation.

## Spec Format

```yaml
spec:
  name: unique_test_name
  version: "1.0"
  category: syscalls|consensus|api|security
  subcategory: specific_area

description: |
  Clear description of what this test verifies.
  Include any relevant specification references.

preconditions:
  - account: "alice"
    balance: 1000000000
    nonce: 0
  - account: "bob"
    balance: 0

action:
  type: transfer|syscall|call|deploy
  # Action-specific fields

expected:
  status: success|error|revert
  error_code: "ERROR_CODE"  # if status is error
  return_value: value       # if applicable
  gas_used: "<= 21000"      # gas constraint

postconditions:
  - account: "alice"
    balance: 900000000
    nonce: 1
```

## Categories

1. **syscalls**: Low-level VM operations (sload, sstore, call, etc.)
2. **consensus**: Block validation, transaction ordering, finality
3. **api**: RPC endpoints, WebSocket, REST
4. **security**: Attack prevention, access control

## Requirements

For the given feature, generate specs covering:

1. Normal operation (happy path)
2. Edge cases (empty, max values)
3. Error conditions (invalid input)
4. Security scenarios (attack prevention)

## Feature to Specify

{FEATURE_NAME}

## Feature Documentation

{FEATURE_DOCS}

## Generate Specifications

Please generate comprehensive YAML specifications.
