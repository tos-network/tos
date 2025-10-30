#!/bin/bash
# TOS RocksDB Migration Progress Tracking Script
# This script analyzes ignored tests and tracks migration progress from SledStorage to RocksDB

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Project paths
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
TESTS_DIR="$PROJECT_ROOT/daemon/tests"

# Print header
print_header() {
    echo -e "${BLUE}╔════════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BLUE}║${NC}  ${CYAN}TOS RocksDB Migration Progress Tracker${NC}                          ${BLUE}║${NC}"
    echo -e "${BLUE}╔════════════════════════════════════════════════════════════════════╗${NC}"
    echo ""
}

# Count total ignored tests
count_ignored_tests() {
    grep -r "#\[ignore\]" "$TESTS_DIR" --include="*.rs" 2>/dev/null | wc -l | xargs
}

# Count migrated tests (RocksDB-based tests without ignore)
count_migrated_tests() {
    # Count tests in rocksdb files that are NOT ignored
    local rocksdb_files=$(find "$TESTS_DIR" -name "*rocksdb*.rs" -type f)
    local total=0

    for file in $rocksdb_files; do
        # Count test functions that are NOT ignored
        local tests=$(grep -E "^\s*#\[tokio::test\]|^\s*#\[test\]" "$file" | wc -l | xargs)
        local ignored=$(grep -B1 "#\[tokio::test\]\|#\[test\]" "$file" | grep -c "#\[ignore\]" || true)
        total=$((total + tests - ignored))
    done

    echo "$total"
}

# List ignored tests by file
list_ignored_by_file() {
    echo -e "${YELLOW}Ignored Tests by File:${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    grep -r "#\[ignore\]" "$TESTS_DIR" --include="*.rs" | cut -d: -f1 | sort | uniq -c | sort -rn | while read count file; do
        local filename=$(basename "$file")
        local dirname=$(basename "$(dirname "$file")")
        printf "  ${GREEN}%3d${NC} tests  │  ${CYAN}%-20s${NC}  │  %s\n" "$count" "$dirname/$filename" "$file"
    done
    echo ""
}

# Categorize tests
categorize_tests() {
    echo -e "${YELLOW}Test Categories:${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    # Storage tests (simple, priority for migration)
    local storage_tests=$(grep -r "#\[ignore\]" "$TESTS_DIR/security/storage_security_tests.rs" "$TESTS_DIR/stress/storage_stress.rs" 2>/dev/null | wc -l | xargs || echo "0")
    echo -e "  ${GREEN}Storage Tests${NC} (priority):          $storage_tests tests"
    echo "    → Simple CRUD operations, easy to migrate"
    echo ""

    # GHOSTDAG tests (complex, needs full implementation)
    local ghostdag_tests=$(grep -r "#\[ignore\]" "$TESTS_DIR/integration/ghostdag_tests.rs" "$TESTS_DIR/security/ghostdag_security_tests.rs" 2>/dev/null | wc -l | xargs || echo "0")
    echo -e "  ${YELLOW}GHOSTDAG Tests${NC} (complex):          $ghostdag_tests tests"
    echo "    → Requires full blockchain implementation"
    echo ""

    # Block submission tests (moderate complexity)
    local block_tests=$(grep -r "#\[ignore\]" "$TESTS_DIR/security/block_submission_tests.rs" 2>/dev/null | wc -l | xargs || echo "0")
    echo -e "  ${YELLOW}Block Submission Tests${NC} (moderate): $block_tests tests"
    echo "    → Needs genesis block setup"
    echo ""

    # DAG tests
    local dag_tests=$(grep -r "#\[ignore\]" "$TESTS_DIR/integration/dag_tests.rs" 2>/dev/null | wc -l | xargs || echo "0")
    echo -e "  ${YELLOW}DAG Tests${NC} (complex):              $dag_tests tests"
    echo "    → Complex DAG scenarios"
    echo ""

    # Stress tests
    local stress_tests=$(grep -r "#\[ignore\]" "$TESTS_DIR/stress/" 2>/dev/null | wc -l | xargs || echo "0")
    echo -e "  ${CYAN}Stress Tests${NC} (defer):             $stress_tests tests"
    echo "    → Performance and load testing"
    echo ""

    # Parallel execution tests
    local parallel_tests=$(grep -r "#\[ignore\]" "$TESTS_DIR/parallel_execution_*.rs" "$TESTS_DIR/integration/parallel_execution_*.rs" 2>/dev/null | wc -l | xargs || echo "0")
    echo -e "  ${GREEN}Parallel Execution Tests${NC}:         $parallel_tests tests"
    echo "    → Some already migrated to RocksDB"
    echo ""
}

# Show migrated tests
show_migrated_tests() {
    echo -e "${GREEN}Migrated Tests (RocksDB-based):${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    local rocksdb_files=$(find "$TESTS_DIR" -name "*rocksdb*.rs" -type f)

    for file in $rocksdb_files; do
        local filename=$(basename "$file")
        local tests=$(grep -E "^\s*#\[tokio::test\]|^\s*#\[test\]" "$file" | wc -l | xargs)
        local ignored=$(grep -B1 "#\[tokio::test\]\|#\[test\]" "$file" | grep -c "#\[ignore\]" || true)
        local active=$((tests - ignored))

        if [ "$active" -gt 0 ]; then
            echo -e "  ${GREEN}✓${NC} ${CYAN}$filename${NC}"
            echo "    Active: $active tests, Ignored: $ignored tests"

            # List test function names
            grep -E "^async fn test_|^fn test_" "$file" | sed 's/async fn //g' | sed 's/fn //g' | sed 's/() {//g' | sed 's/{//g' | while read test_name; do
                echo "      • $test_name"
            done
            echo ""
        fi
    done
}

# Calculate progress
calculate_progress() {
    local total_ignored=$1
    local migrated=$2
    local remaining=$((total_ignored))
    local progress=0

    if [ "$total_ignored" -gt 0 ]; then
        progress=$((migrated * 100 / total_ignored))
    fi

    echo -e "${YELLOW}Progress Summary:${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo -e "  Total ignored tests:      ${RED}$total_ignored${NC}"
    echo -e "  Migrated tests (RocksDB): ${GREEN}$migrated${NC}"
    echo -e "  Remaining to migrate:     ${YELLOW}$remaining${NC}"
    echo -e "  Progress:                 ${CYAN}$progress%${NC}"
    echo ""
}

# Estimate time savings
estimate_time_savings() {
    local migrated=$1

    echo -e "${YELLOW}Time Savings Analysis:${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    # Assumptions:
    # - Each migrated test saves ~30-45s per test run (no mining wait)
    # - Tests run multiple times per day during development
    local avg_time_per_test=37  # seconds
    local runs_per_day=10

    local time_saved_per_run=$((migrated * avg_time_per_test))
    local time_saved_per_day=$((time_saved_per_run * runs_per_day))
    local minutes_per_run=$((time_saved_per_run / 60))
    local minutes_per_day=$((time_saved_per_day / 60))

    echo "  Time saved per test run:  ${GREEN}~${minutes_per_run} minutes${NC} ($time_saved_per_run seconds)"
    echo "  Time saved per day:       ${GREEN}~${minutes_per_day} minutes${NC} (assuming $runs_per_day runs)"
    echo "  Speedup factor:           ${GREEN}~30x faster${NC} (no 30s mining delay)"
    echo ""

    echo "  ${CYAN}Benefit:${NC} Developers can run tests instantly, improving iteration speed"
    echo ""
}

# Show priority list
show_priorities() {
    echo -e "${YELLOW}Priority Migration List (Recommended Order):${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo -e "  ${GREEN}HIGH PRIORITY${NC} (Easy wins - simple storage operations):"
    echo "    1. storage_security_tests.rs - Basic CRUD operations"
    echo "    2. parallel_execution_parity_tests.rs - Already partially migrated"
    echo "    3. parallel_execution_security_tests.rs - Security tests"
    echo ""
    echo -e "  ${YELLOW}MEDIUM PRIORITY${NC} (Moderate complexity - needs genesis setup):"
    echo "    4. block_submission_tests.rs - Block validation tests"
    echo "    5. daa_tests.rs - Difficulty adjustment tests"
    echo "    6. concurrent_lock_tests.rs - Lock behavior tests"
    echo ""
    echo -e "  ${CYAN}LOW PRIORITY${NC} (Complex - defer until basic infrastructure is stable):"
    echo "    7. ghostdag_tests.rs - Requires full GHOSTDAG implementation"
    echo "    8. dag_tests.rs - Complex DAG scenarios"
    echo "    9. ghostdag_security_tests.rs - Advanced security tests"
    echo ""
    echo -e "  ${RED}DEFER${NC} (Not urgent - stress/performance tests):"
    echo "    10. stress/ - All stress tests (memory, storage, network, etc.)"
    echo ""
}

# Show next steps
show_next_steps() {
    echo -e "${YELLOW}Next Steps:${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "  1. Pick a high-priority test file from the list above"
    echo "  2. Read MIGRATION_QUICK_REFERENCE.md for step-by-step guide"
    echo "  3. Create new *_rocksdb.rs file or update existing one"
    echo "  4. Follow the migration pattern from parallel_execution_parity_tests_rocksdb.rs"
    echo "  5. Remove #[ignore] from successfully migrated tests"
    echo "  6. Run: cargo test <test_name> to verify"
    echo "  7. Update MIGRATION_PROGRESS.md with completed tests"
    echo ""
    echo "  ${CYAN}Documentation:${NC}"
    echo "    • MIGRATION_PROGRESS.md - Track completed migrations"
    echo "    • MIGRATION_QUICK_REFERENCE.md - Migration guide"
    echo "    • ROCKSDB_MIGRATION_SUMMARY.md - Overview and architecture"
    echo ""
}

# Main execution
main() {
    print_header

    # Count tests
    local total_ignored=$(count_ignored_tests)
    local migrated=$(count_migrated_tests)

    # Show statistics
    calculate_progress "$total_ignored" "$migrated"
    list_ignored_by_file
    categorize_tests
    show_migrated_tests
    estimate_time_savings "$migrated"
    show_priorities
    show_next_steps

    echo -e "${BLUE}╚════════════════════════════════════════════════════════════════════╝${NC}"
    echo ""
    echo -e "${GREEN}Run this script anytime to check migration progress!${NC}"
    echo ""
}

# Run main function
main
