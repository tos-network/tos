# GHOSTDAG JSON Test Suite

This directory contains JSON-based test cases for GHOSTDAG consensus validation. These tests enable cross-implementation testing and provide a language-agnostic way to verify GHOSTDAG correctness.

## Overview

The JSON test format allows you to define:
- DAG structures with multiple blocks and parents
- Expected GHOSTDAG consensus outcomes (blue_score, blue_work, etc.)
- Complex scenarios including merges, k-cluster violations, and red blocks

## JSON Schema

Each test file should follow this structure:

```json
{
  "name": "test_name",
  "description": "What this test verifies",
  "config": {
    "k": 10,
    "genesis_hash": "0000000000000000000000000000000000000000000000000000000000000000",
    "version": "1.0"
  },
  "metadata": {
    "author": "Test Author",
    "created": "2025-10-13",
    "tags": ["category", "type"],
    "reference": "TIP-2 Section X"
  },
  "blocks": [
    {
      "id": "block_id",
      "hash": "64_character_hex_string",
      "parents": ["parent_block_id"],
      "difficulty": 1000,
      "timestamp": 1000000000000,
      "expected": {
        "blue_score": 1,
        "blue_work": "2000",
        "selected_parent": "parent_block_id",
        "mergeset_blues": ["parent_block_id"],
        "mergeset_reds": [],
        "blues_anticone_sizes": {},
        "mergeset_non_daa": []
      }
    }
  ]
}
```

## Field Descriptions

### Test Level

- **name**: Unique identifier for the test
- **description**: Human-readable explanation of what the test verifies
- **config**: Test configuration
  - **k**: K-cluster parameter (typically 10 for BlockDAG protocols)
  - **genesis_hash**: Genesis block hash (64 hex characters)
  - **version**: Test format version
- **metadata**: Optional metadata for organization
  - **author**: Test creator
  - **created**: Creation date (YYYY-MM-DD)
  - **tags**: Categories for filtering/grouping
  - **reference**: Link to spec/issue

### Block Level

- **id**: Human-readable block identifier (used for parent references)
- **hash**: Actual block hash (64 hex characters, must be unique)
- **parents**: Array of parent block IDs (empty for genesis)
- **difficulty**: Block difficulty (used to calculate work)
- **timestamp**: Optional timestamp in milliseconds
- **expected**: Expected GHOSTDAG consensus results
  - **blue_score**: Expected blue score (number of blue blocks in past)
  - **blue_work**: Expected cumulative blue work (as decimal string or hex)
  - **selected_parent**: Block ID of the selected parent (highest blue_work)
  - **mergeset_blues**: Array of blue block IDs in the mergeset
  - **mergeset_reds**: Array of red block IDs in the mergeset
  - **blues_anticone_sizes**: Map of block ID to anticone size
  - **mergeset_non_daa**: Blocks outside DAA window (optional)

## Existing Test Cases

### simple_chain.json

Tests a basic linear chain with no merges. All blocks should be blue, and blue_score should increase by 1 for each block.

**Purpose**: Verify basic GHOSTDAG functionality in the simplest case.

**Structure**:
```
genesis -> block_1 -> block_2 -> block_3
```

### simple_merge.json

Tests a simple DAG merge where two parallel branches merge into one block. Both branches should be blue.

**Purpose**: Verify GHOSTDAG can handle concurrent blocks without treating them as orphans.

**Structure**:
```
        genesis
        /     \
    block_a  block_b
        \     /
        block_c
```

## Running Tests

### From Rust

```rust
use tos_daemon::core::tests::ghostdag_json_loader::*;

#[tokio::test]
async fn test_ghostdag_suite() {
    let test_dir = "daemon/test_data/ghostdag";
    let tests = load_all_json_tests(test_dir).await.unwrap();

    for test_case in tests {
        let result = execute_json_test(&test_case).await.unwrap();
        assert!(result.passed, "Test failed: {}", test_case.name);
    }
}
```

### From Command Line

```bash
# Run all GHOSTDAG JSON tests
cargo test --package tos_daemon --lib ghostdag_json_loader::tests -- --nocapture

# Run specific test
cargo test --package tos_daemon --lib test_load_simple_chain
```

## Creating New Tests

1. Create a new JSON file in this directory
2. Follow the schema above
3. Start with genesis block (no parents)
4. Add blocks in topological order (parents before children)
5. Calculate expected GHOSTDAG values:
   - blue_score = max(parents.blue_score) + 1
   - blue_work = parent.blue_work + sum(blue_blocks.work)
   - selected_parent = parent with highest blue_work
   - mergeset_blues/reds based on k-cluster constraint

### Tips for Test Creation

- **Genesis block**: Always the first block with no parents
- **Block ordering**: Define blocks in topological order
- **Hash uniqueness**: Each block must have a unique 64-character hex hash
- **Parent references**: Use block IDs (not hashes) for parent field
- **Difficulty**: Use consistent difficulty (e.g., 1000) unless testing specific scenarios
- **Blue work**: Can be decimal string ("1000") or hex ("0x3e8")

### Example: Adding a K-cluster Violation Test

```json
{
  "name": "test_k_cluster_violation",
  "description": "Tests red block detection when k-cluster is violated",
  "config": {
    "k": 3,
    "genesis_hash": "0000000000000000000000000000000000000000000000000000000000000000"
  },
  "blocks": [
    // Genesis and blue blocks...
    {
      "id": "red_block",
      "hash": "rrrr...",
      "parents": [...],
      "difficulty": 1000,
      "expected": {
        "blue_score": 5,
        "blue_work": "6000",
        "selected_parent": "some_blue",
        "mergeset_blues": ["blue1", "blue2"],
        "mergeset_reds": ["red_block"],  // This block violates k-cluster
        "blues_anticone_sizes": {...}
      }
    }
  ]
}
```

## Validation Rules

The test loader validates:
- ✓ Unique block IDs and hashes
- ✓ Valid hash format (64 hex characters)
- ✓ Parent references exist
- ✓ Genesis has no parents
- ✓ Blocks defined in valid order
- ✓ JSON schema compliance

## Test Categories (Tags)

Recommended tags for organizing tests:

- **linear_chain**: Simple chain with no merges
- **merge**: DAG merges (multiple parents)
- **red_blocks**: Tests with red blocks
- **k_cluster**: K-cluster constraint tests
- **daa**: Difficulty adjustment tests
- **basic**: Simple, foundational tests
- **complex**: Multi-level DAG structures
- **edge_case**: Boundary conditions

## Cross-Implementation Testing

These JSON tests can be used by:
- TOS (Rust implementation)
- Other BlockDAG implementations
- Reference implementations
- Third-party validators

To use in another language:
1. Implement a JSON parser for the schema
2. Build the DAG from block definitions
3. Run GHOSTDAG consensus
4. Compare actual vs expected results

## Contributing Tests

When adding new tests:
1. Use descriptive names (e.g., `test_diamond_merge.json`)
2. Include detailed description field
3. Add appropriate tags
4. Reference specification sections if applicable
5. Test both success and failure cases
6. Document any special conditions

## References

- **TIP-2**: GHOSTDAG Consensus Specification
- **GHOSTDAG Paper**: https://eprint.iacr.org/2018/104.pdf
- **TOS Documentation**: https://docs.tos.network

## License

These test cases are part of the TOS Network project and are licensed under BSD-3-Clause.
