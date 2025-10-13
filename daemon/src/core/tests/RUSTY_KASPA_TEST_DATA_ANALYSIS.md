# Rusty-Kaspa JSON Test Data Analysis

## Executive Summary

This document provides a comprehensive analysis of the six GHOSTDAG test files from rusty-kaspa's integration test suite. These JSON files define DAG topologies with expected GHOSTDAG algorithm outputs for validation purposes.

**Test File Sources:** `/Users/tomisetsu/tos-network/rusty-kaspa/testing/integration/testdata/dags/`

---

## Test File Comparison Table

| File | K Value | Genesis ID | Total Blocks | Blocks with Reds | Max DAG Width | Primary Test Scenario |
|------|---------|------------|--------------|------------------|---------------|----------------------|
| dag0.json | 4 | A | 19 | 3 (L, T, +global H,I,Q) | 5 children of M | Complex merge with K-violations |
| dag1.json | 4 | 0 | 30 | 11 blocks | High complexity | Large-scale conflict resolution |
| dag2.json | 18 | 786 | 9 | 0 | Linear/simple | High-K linear chain (no conflicts) |
| dag3.json | 3 | 0 | 10 | 1 (block 10) | 3 children of 1 | K-constraint violation test |
| dag4.json | 2 | 0 | 9 | 1 (block 9) | Multiple merges | Low-K stress test |
| dag5.json | 3 | 0 | 7 | 1 (block 7) | 4 children of 0 | Multi-parent merge conflict |

---

## Detailed Analysis by Test File

### dag0.json - Complex Merge with K-Violations

**Configuration:**
- K = 4
- Genesis ID: "A"
- Total Blocks: 19
- Global Expected Reds: ["Q", "H", "I"]

**DAG Topology:**
```
                    A (genesis)
         /     /    |    \    \
        B     D     F     H    I
        |           |
        C           G
         \         /
           \     /
             E
             |
             J
             |
             K
            / \
           L   (I merges here)
           |
           M
        / /|\ \
       N O 2 Q R
       |     |
       S-----+
       |
       T (merges N,O,2,Q,S)
```

**Key Blocks:**

| Block | Parents | Selected Parent | Score | ExpectedBlues | ExpectedReds | Notes |
|-------|---------|-----------------|-------|---------------|--------------|-------|
| B | [A] | A | 1 | [A] | [] | Linear extension |
| C | [B] | B | 2 | [B] | [] | Linear extension |
| E | [C,D] | C | 4 | [C,D] | [] | First merge |
| J | [E,G] | E | 7 | [E,F,G] | [] | Major merge |
| L | [I,K] | K | 9 | [K] | [I] | **First red block violation** |
| T | [N,O,2,Q,S] | S | 16 | [S,2,N,O] | [Q] | **Complex 5-parent merge** |

**Test Patterns:**
1. **Linear Chain Extension**: Blocks B→C demonstrate simple chain growth
2. **Binary Merges**: E merges two chains (C,D), J merges two chains (E,G)
3. **Late Merge Conflicts**: Block L creates first conflict by merging old block I with K
4. **Wide Fan-out**: Block M has 5 children (N,O,2,Q,R) - tests anticone set management
5. **Multi-parent Convergence**: Block T merges 5 parents simultaneously

**Global Red Set Explanation:**
- **H, I**: Blocks that diverged early from A and never rejoined the main chain until very late
- **Q**: One of M's children that got excluded during T's merge (anticone violation)

**Key Assertions:**
- GHOSTDAG correctly handles delayed merges (block I finally merged at L)
- Wide fan-out followed by convergence (M's 5 children converging at T)
- Anticone set size management with K=4 constraint

---

### dag1.json - Large-Scale Conflict Resolution

**Configuration:**
- K = 4
- Genesis ID: "0"
- Total Blocks: 30
- No global ExpectedReds (handled per-block)

**DAG Topology:**
Very complex with multiple interleaving chains and frequent merges.

**High-Conflict Blocks:**

| Block | Parents | Selected Parent | Score | ExpectedBlues | ExpectedReds | Conflict Count |
|-------|---------|-----------------|-------|---------------|--------------|----------------|
| 12 | [a10,9] | a10 | 8 | [a10,5,9] | [3,6] | 2 reds |
| 14 | [13,a10] | 13 | 8 | [13,a10] | [4] | 1 red |
| 15 | [11,13] | 11 | 9 | [11,13] | [a1,8] | 2 reds |
| 20 | [16,17] | 17 | 10 | [17] | [6,7,9,11,16] | **5 reds - highest** |
| 22 | [19,21] | 21 | 13 | [21] | [15,19] | 2 reds |
| 26 | [22,24,25555] | 25555 | 15 | [25555,22] | [12,15,19,23,24] | **5 reds - highest** |

**Test Patterns:**
1. **Sustained Conflicts**: Multiple blocks have red sets throughout the DAG
2. **Chain Selection Dynamics**: Frequent switching of selected parents indicates competing chains
3. **Multi-Chain Convergence**: Block 26 merges 3 parents with 5 red exclusions
4. **Anticone Accumulation**: Block 20 and 26 both exclude 5 blocks (K=4 limit exceeded)
5. **Score Progression**: Scores grow to 17 (block 29), demonstrating deep DAG structure

**Key Observations:**
- Block "25555" (unusual ID) serves as a convergence point (score 13, selected by both 26 and 28)
- Block 20 creates massive anticone (5 reds) when merging two competing chains
- Final block 30 extends from block 27, showing a late-diverging chain

**Key Assertions:**
- GHOSTDAG handles sustained parallel chain development
- Correct anticone calculation with multiple competing chains
- Proper score accumulation in complex merge scenarios

---

### dag2.json - High-K Linear Chain (No Conflicts)

**Configuration:**
- K = 18 (very high)
- Genesis ID: "786"
- Total Blocks: 9
- Global Expected Reds: [] (empty)

**DAG Topology:**
```
            786 (genesis)
        /    |    \
      21d   d1c  f154
       |          /|\
      6ef      d1c 21d f154
       |          |
      c98        6c7
        \        /
          ec9--+
            \  |
             015
              |
            crash (merges 6ef,6c7)
```

**All Blocks:**

| Block | Parents | Selected Parent | Score | ExpectedBlues | ExpectedReds | Pattern |
|-------|---------|-----------------|-------|---------------|--------------|---------|
| 21d | [786] | 786 | 1 | [786] | [] | Linear |
| 6ef | [21d] | 21d | 2 | [21d] | [] | Linear |
| c98 | [6ef] | 6ef | 3 | [6ef] | [] | Linear |
| d1c | [786] | 786 | 1 | [786] | [] | Branch |
| ec9 | [d1c,c98] | c98 | 5 | [c98,d1c] | [] | Merge |
| f154 | [786] | 786 | 1 | [786] | [] | Branch |
| 6c7 | [d1c,21d,f154] | f154 | 4 | [f154,21d,d1c] | [] | 3-parent merge |
| 015 | [ec9,6c7] | ec9 | 8 | [ec9,f154,6c7] | [] | Major merge |
| crash | [6ef,6c7] | 6c7 | 6 | [6c7,6ef] | [] | Cross-chain |

**Test Patterns:**
1. **High K Value**: K=18 means virtually no anticone restrictions
2. **Zero Conflicts**: No red blocks anywhere - all blocks fit in anticone
3. **Multi-Parent Merges**: Block 6c7 has 3 parents, all accepted as blue
4. **Clean Convergence**: All merges are conflict-free
5. **Hexadecimal IDs**: Uses hex-style IDs (21d, 6ef, c98, etc.) - tests ID parsing

**Key Assertions:**
- With high K, complex DAG structures have no conflicts
- Multi-parent merges work cleanly when K is sufficient
- Score calculation correct in conflict-free scenarios
- System handles non-standard block ID formats

---

### dag3.json - K-Constraint Violation Test

**Configuration:**
- K = 3
- Genesis ID: "0"
- Total Blocks: 10

**DAG Topology:**
```
            0 (genesis)
            |
            1
        / / | \
       2  3  4  6
       |  |  |  |
          (merge) 7
            5    |
              \  8
               \ |
                 9
```

**Detailed Block Analysis:**

| Block | Parents | Selected Parent | Score | ExpectedBlues | ExpectedReds | K-Constraint Analysis |
|-------|---------|-----------------|-------|---------------|--------------|----------------------|
| 1 | [0] | 0 | 1 | [0] | [] | Clean |
| 2,3,4 | [1] | 1 | 2 | [1] | [] | 3 parallel chains from 1 |
| 5 | [4,2,3] | 4 | 5 | [4,2,3] | [] | Merges all 3 chains |
| 6 | [0] | 0 | 1 | [0] | [] | Late-diverging chain |
| 7,8,9 | Sequential | - | 2,3,4 | - | [] | Chain 6→7→8→9 |
| 10 | [5,9] | 5 | 6 | [5] | **[6,7,8,9]** | **Violates K=3 constraint** |

**Critical Test Case - Block 10:**
- **Parents**: [5, 9]
- **Selected Parent**: 5 (score 5 > 9's chain score 4)
- **Expected Reds**: [6, 7, 8, 9] - entire chain excluded
- **Reason**: The 6→7→8→9 chain (4 blocks) exceeds K=3 anticone limit

**Test Patterns:**
1. **Fan-out Test**: Block 1 creates 3 parallel children (tests max anticone size)
2. **Clean Convergence**: Block 5 successfully merges 3 chains within K=3
3. **Competing Chain**: Chain 6→7→8→9 builds independently
4. **K-Violation**: Block 10 merges two chains where one chain's length (4) > K (3)
5. **Whole-Chain Exclusion**: All blocks in the losing chain become red

**Key Assertions:**
- GHOSTDAG correctly identifies when a chain exceeds K limit
- All blocks in anticone set beyond K are marked red
- Selected parent is chain with higher score
- System handles "pruning" of entire subchains

---

### dag4.json - Low-K Stress Test

**Configuration:**
- K = 2 (very low)
- Genesis ID: "0"
- Total Blocks: 9

**DAG Topology:**
```
        0 (genesis)
       / \
      1   2
      |   |\
      5   3 4
       \ /|
        6 |
         \|
          7
           \
            8
            |
            9 (merges 6,7,8) -> 3 reds: [4,8,6]
```

**Detailed Block Analysis:**

| Block | Parents | Selected Parent | Score | ExpectedBlues | ExpectedReds | K=2 Analysis |
|-------|---------|-----------------|-------|---------------|--------------|--------------|
| 1,2 | [0] | 0 | 1 | [0] | [] | Parallel genesis children |
| 3,4 | [2] | 2 | 2 | [2] | [] | Chain 2→3, 2→4 |
| 5 | [1] | 1 | 2 | [1] | [] | Chain 1→5 |
| 6 | [4,5] | 5 | 5 | [5,2,4] | [] | Merges chains 1→5 and 2→4 |
| 7 | [3,5] | 5 | 5 | [5,2,3] | [] | Merges chains 1→5 and 2→3 |
| 8 | [3] | 3 | 3 | [3] | [] | Extends chain 2→3 |
| 9 | [6,7,8] | 7 | 6 | [7] | **[4,8,6]** | **3-parent merge violates K=2** |

**Critical Test Case - Block 9:**
- **Parents**: [6, 7, 8] (3 parents)
- **Selected Parent**: 7 (score 5)
- **Expected Reds**: [4, 8, 6] - 3 blocks excluded
- **Reason**: With K=2, anticone can contain at most 2 blocks. Block 9's 3 parents create conflicts
- **Blue Set**: Only [7] - just the selected parent
- **Red Explanation**:
  - Block 4: In anticone of selected parent 7
  - Block 8: In anticone of selected parent 7
  - Block 6: In anticone of selected parent 7 (competing merge)

**Test Patterns:**
1. **Minimal K**: K=2 is near-minimum for DAG consensus (K=1 would be a chain)
2. **Frequent Conflicts**: Low K means more blocks fall outside anticone
3. **Merge Competition**: Blocks 6 and 7 are competing merges from same base
4. **Multi-Parent Stress**: Block 9 has 3 parents with K=2, forcing exclusions
5. **Anticone Overflow**: Tests what happens when merge parents exceed K

**Key Assertions:**
- GHOSTDAG handles extreme low-K scenarios
- Multi-parent merges correctly exclude excess anticone blocks
- Competing merge blocks properly identified as red
- System maintains consistency even with tight K constraint

---

### dag5.json - Multi-Parent Merge Conflict

**Configuration:**
- K = 3
- Genesis ID: "0"
- Total Blocks: 7

**DAG Topology:**
```
        0 (genesis)
      / | | \
     1  2 3  4
      \ | | /
        5   6
         \ /
          7 -> red: [5]
```

**Detailed Block Analysis:**

| Block | Parents | Selected Parent | Score | ExpectedBlues | ExpectedReds | Notes |
|-------|---------|-----------------|-------|---------------|--------------|-------|
| 1,2,3,4 | [0] | 0 | 1 | [0] | [] | **4-way fan-out from genesis** |
| 5 | [1,2,3] | 3 | 4 | [3,1,2] | [] | Merges 3 of 4 chains |
| 6 | [1,2,3,4] | 4 | 5 | [4,1,2,3] | [] | **Merges all 4 chains** |
| 7 | [5,6] | 6 | 6 | [6] | **[5]** | **Conflict between competing merges** |

**Critical Test Case - Block 7:**
- **Parents**: [5, 6]
- **Selected Parent**: 6 (score 5 > 5's score 4)
- **Expected Reds**: [5]
- **Reason**: Blocks 5 and 6 are competing merge strategies:
  - Block 5: Merges only 3 chains [1,2,3]
  - Block 6: Merges all 4 chains [1,2,3,4] - more complete
- Block 6 is chosen because it has higher score (merged block 4 as well)
- Block 5 becomes red because it's in the anticone of the selected parent

**Test Patterns:**
1. **Maximum Fan-out**: Genesis creates 4 parallel children (tests wide DAG)
2. **Partial vs Complete Merge**: Compares 3-parent merge vs 4-parent merge
3. **Merge Competition**: Two different merge strategies compete
4. **Score-Based Selection**: Higher-scoring merge (more complete) wins
5. **Minimal Conflict**: Only 1 red block in entire DAG

**Key Assertions:**
- GHOSTDAG prefers more complete merges (higher blue score)
- Competing merge blocks correctly handled
- Multi-parent merges work correctly within K constraint
- System correctly calculates blue set when multiple merge strategies exist

---

## Common JSON Structure

All test files follow this schema:

```json
{
  "K": <integer>,                    // GHOSTDAG K parameter
  "GenesisID": "<string>",           // Genesis block identifier
  "ExpectedReds": [<optional list>], // Global red blocks (dag0, dag2 only)
  "Blocks": [
    {
      "ID": "<string>",                  // Unique block identifier
      "ExpectedScore": <integer>,        // Expected blue score
      "ExpectedSelectedParent": "<string>", // Expected chain block
      "ExpectedReds": [<list>],          // Red blocks in this block's anticone
      "ExpectedBlues": [<list>],         // Blue blocks in this block's past
      "Parents": [<list>]                // Direct parent blocks
    }
  ]
}
```

---

## Key Test Patterns Summary

### 1. Linear Chain Extension
- **Files**: All files
- **Pattern**: Single parent blocks extending chain
- **Assertion**: Score increases by 1, parent becomes blue, no reds

### 2. Binary Merges
- **Files**: dag0, dag2, dag3, dag4, dag5
- **Pattern**: Block with exactly 2 parents
- **Assertion**: Both parents in blue set if within K constraint

### 3. Multi-Parent Merges
- **Files**: dag3 (3 parents), dag5 (4 parents), dag1 (3 parents)
- **Pattern**: Block with 3+ parents
- **Assertion**: All parents blue if total anticone ≤ K

### 4. K-Constraint Violations
- **Files**: dag0, dag1, dag3, dag4, dag5
- **Pattern**: Merge where anticone size > K
- **Assertion**: Excess blocks marked red, selected parent chain preferred

### 5. Competing Chains
- **Files**: All files
- **Pattern**: Multiple chains from same ancestor
- **Assertion**: Higher-scoring chain selected, others may become red

### 6. Wide Fan-out
- **Files**: dag0 (5 children), dag5 (4 children), dag3 (3 children)
- **Pattern**: Block with many children
- **Assertion**: Tests anticone set management

### 7. Late Merges
- **Files**: dag0 (block L merges old block I), dag3 (block 10 merges old chain)
- **Pattern**: Merging old divergent blocks
- **Assertion**: Old blocks may become red if anticone too large

---

## Score Calculation Insights

**Score Formula**: `Score = |Blue(B)| = number of blocks in blue set`

**Observations from test data:**

1. **Linear Extension**: Score increases by 1
   - Example: dag0: A(0)→B(1)→C(2)

2. **Binary Merge**: Score = selected_parent_score + blue_count_from_other_parent
   - Example: dag0 block E: score(C)=2 + blue_from_D=2 → score(E)=4

3. **Multi-Parent Merge**: Score = selected_parent_score + sum(blue_blocks_from_other_parents)
   - Example: dag5 block 6: score(4)=1 + blues_from[1,2,3]=4 → score(6)=5

4. **Conflict Scenario**: Score only counts blue blocks, reds excluded
   - Example: dag3 block 10: Merges [5,9] but 9's chain red → only blues from 5 counted

---

## Red Block Classification

**Red blocks occur when:**

1. **Anticone Size Exceeds K**: Block's anticone relative to selected parent > K
   - dag3 block 10: Entire 6→7→8→9 chain (4 blocks) > K(3)

2. **Competing Merge Losers**: When multiple merge strategies compete
   - dag5 block 7: Block 5 loses to block 6 (less complete merge)

3. **Chain Selection**: Losing side of chain competition
   - dag4 block 9: Multiple competing chains, lowest scoring marked red

4. **Late Merge Conflicts**: Old divergent blocks merged late
   - dag0 block L: Block I diverged early, merged late, marked red

**Red Block Properties:**
- Not included in score calculation
- Not in blue anticone set
- Still part of DAG history (not deleted)
- Children can still reference them

---

## Edge Cases Covered

### 1. Block ID Formats
- **Alphanumeric**: A, B, C (dag0)
- **Numeric**: 1, 2, 3 (dag1, dag3, dag4, dag5)
- **Hexadecimal**: 21d, 6ef, c98 (dag2)
- **Mixed**: a1, a10, 25555 (dag1)
- **Special**: "crash" (dag2)

### 2. K Parameter Variations
- **K=2**: Minimal DAG (dag4)
- **K=3**: Low consensus (dag3, dag5)
- **K=4**: Standard consensus (dag0, dag1)
- **K=18**: High throughput, no conflicts (dag2)

### 3. DAG Structures
- **Linear chains**: All files have some linear segments
- **Binary trees**: Simple branching and merging
- **Wide fan-out**: Up to 5 children from single parent
- **Multi-parent convergence**: Up to 5 parents merging into single block
- **Deep DAGs**: Up to 30 blocks (dag1), scores up to 17

### 4. Conflict Scenarios
- **Zero conflicts**: dag2 (K=18, all blues)
- **Single conflict**: dag3, dag5 (1 red block each)
- **Multiple conflicts**: dag0 (3 global reds), dag1 (11 blocks with reds)
- **Whole-chain exclusion**: dag3 (4 blocks in one red set)

---

## Test Coverage Analysis

### Comprehensive Coverage

| Test Aspect | Files Testing | Coverage Level |
|-------------|---------------|----------------|
| Linear chains | All 6 files | Excellent |
| Binary merges | All 6 files | Excellent |
| Multi-parent merges (3+) | dag1, dag3, dag4, dag5 | Good |
| K-constraint violations | dag0, dag1, dag3, dag4 | Excellent |
| Zero-conflict scenarios | dag2 | Limited (1 file) |
| Low-K stress (K≤3) | dag3, dag4, dag5 | Good |
| High-K scenarios (K>10) | dag2 | Limited (1 file) |
| Wide fan-out (4+ children) | dag0, dag5 | Good |
| Deep DAGs (20+ blocks) | dag0, dag1 | Good |
| Complex topologies | dag1 | Excellent |
| ID format variations | All 6 files | Excellent |

### Potential Gaps

1. **Very High Block Counts**: Largest test is 30 blocks (dag1)
   - Real networks have millions of blocks
   - Recommendation: Add tests with 100+ blocks

2. **K > 20**: Only dag2 tests high K (K=18)
   - Higher K values used in production
   - Recommendation: Add K=50, K=100 tests

3. **Circular Merge Patterns**: No tests for complex circular anticone relationships
   - Recommendation: Add test with circular merge dependencies

4. **Genesis Variations**: All tests use simple single genesis
   - Recommendation: Test multiple genesis blocks (for reorgs)

5. **Score Edge Cases**: Highest score tested is 17
   - Recommendation: Test scores in hundreds/thousands

---

## Implementation Recommendations for TOS

### 1. Test Harness Design

```rust
// Recommended test structure
struct DagTestCase {
    k: u32,
    genesis_id: BlockId,
    blocks: Vec<TestBlock>,
    expected_reds_global: Vec<BlockId>,
}

struct TestBlock {
    id: BlockId,
    parents: Vec<BlockId>,
    expected_score: u64,
    expected_selected_parent: BlockId,
    expected_blues: Vec<BlockId>,
    expected_reds: Vec<BlockId>,
}
```

### 2. Test Execution Strategy

1. **Sequential Block Addition**: Add blocks in order from JSON
2. **Incremental Validation**: Validate GHOSTDAG output after each block
3. **Assertion Points**:
   - Blue score matches ExpectedScore
   - Selected parent matches ExpectedSelectedParent
   - Blue set matches ExpectedBlues
   - Red set matches ExpectedReds

### 3. Critical Assertions

```rust
// After adding each block
assert_eq!(block.blue_score(), test_case.expected_score);
assert_eq!(block.selected_parent(), test_case.expected_selected_parent);
assert_eq!(block.blue_set(), test_case.expected_blues);
assert_eq!(block.red_set(), test_case.expected_reds);
```

### 4. Test Priority Order

**Priority 1 (Must Pass):**
- dag2.json: Basic functionality, zero conflicts
- dag3.json: K-constraint violation
- dag0.json: Complex realistic scenario

**Priority 2 (Should Pass):**
- dag4.json: Low-K stress test
- dag5.json: Multi-parent merge conflicts

**Priority 3 (Advanced):**
- dag1.json: Large-scale complex topology

### 5. Debugging Strategy

When tests fail:
1. **Check block ordering**: Ensure blocks added in dependency order
2. **Verify K parameter**: Confirm K value matches test file
3. **Trace selected parent**: Follow chain selection logic
4. **Anticone calculation**: Verify anticone set computation
5. **Score accumulation**: Check blue set scoring

### 6. ID Handling

**Important**: Test files use diverse ID formats
- Implement flexible BlockId parsing (numeric, alphanumeric, hex)
- Don't assume numeric IDs
- Handle arbitrary string IDs

### 7. Performance Considerations

- **dag1.json** (30 blocks): Should complete in < 100ms
- **All tests**: Should complete in < 1 second total
- If slower, optimize:
  - Anticone set computation
  - Blue/red classification
  - Parent traversal

---

## Specific Test Insights

### dag0.json Key Learning Points
1. Global red set represents blocks that never merge into main chain
2. Wide fan-out (M with 5 children) tests anticone management
3. Multi-parent convergence (T with 5 parents) tests merge logic

### dag1.json Key Learning Points
1. Largest and most complex test (30 blocks)
2. Tests sustained parallel chain development
3. Multiple competing chain merge strategies
4. Frequent red block classification (11 blocks involved in conflicts)

### dag2.json Key Learning Points
1. High K eliminates conflicts entirely
2. Tests multi-parent merges without conflicts
3. Validates scoring in clean scenarios
4. Hexadecimal ID format validation

### dag3.json Key Learning Points
1. Clean demonstration of K-constraint violation
2. Entire losing chain (4 blocks) marked red at once
3. Tests competing chain selection logic
4. Shows how anticone limit enforces chain selection

### dag4.json Key Learning Points
1. Extreme low-K scenario (K=2)
2. Shows how tight K creates frequent conflicts
3. Competing merge blocks both marked red
4. Tests multi-parent merge with insufficient K

### dag5.json Key Learning Points
1. Maximum fan-out test (4 children from genesis)
2. Competing merge strategies (partial vs complete)
3. Score-based merge selection (complete merge wins)
4. Minimal conflict scenario (only 1 red)

---

## GHOSTDAG Algorithm Validation Points

Based on these test files, a correct GHOSTDAG implementation must:

1. **Blue Set Calculation**:
   - Include selected parent chain
   - Include blocks in anticone up to K limit
   - Exclude blocks beyond K limit (mark as red)

2. **Selected Parent Selection**:
   - Choose parent with highest blue score
   - Break ties consistently (not tested here, but important)
   - Follow selected parent chain for ordering

3. **Score Calculation**:
   - Score = |Blue(B)| = count of blue blocks
   - Include all blocks in blue anticone
   - Exclude red blocks from score

4. **Red Set Calculation**:
   - Blocks in past but not in blue set
   - Blocks beyond K limit in anticone
   - Losing chains in merge conflicts

5. **Anticone Management**:
   - Correctly identify blocks in anticone
   - Enforce K-size limit
   - Handle multi-parent anticone unions

6. **Merge Handling**:
   - Support multiple parents (tested up to 5)
   - Correctly merge anticone sets
   - Select highest-scoring merge strategy

---

## Conclusion

These six test files provide comprehensive coverage of GHOSTDAG algorithm behavior across various DAG topologies and K parameters. They test:

- Basic chain extension
- Binary and multi-parent merges
- K-constraint violations
- Competing chains and merge strategies
- Score calculation and propagation
- Red/blue classification
- Various K parameters (2, 3, 4, 18)
- Different DAG structures (linear, branching, converging)
- Edge cases (wide fan-out, deep DAGs, late merges)

**For TOS Implementation:**
1. Start with dag2.json (simplest, no conflicts)
2. Progress to dag3.json and dag5.json (clear K violations)
3. Advance to dag0.json (realistic complexity)
4. Complete with dag4.json and dag1.json (stress tests)

**Success Criteria:**
- All 6 test files pass completely
- All blocks match expected scores, parents, blues, and reds
- Tests execute quickly (< 1 second total)
- Implementation handles all ID formats and K parameters

This test suite provides gold-standard validation for GHOSTDAG implementations.
