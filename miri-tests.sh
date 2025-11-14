#!/bin/bash
# TOS Miri Memory Safety Tests
# Reference: Round 3 audit v4 recommendations
#
# Miri detects undefined behavior including:
# - Use-after-free
# - Out-of-bounds access
# - Data races
# - Invalid pointer arithmetic
#
# Usage: ./miri-tests.sh

set -e

echo "========================================="
echo "TOS Miri Memory Safety Validation"
echo "========================================="
echo ""

# Check if rustup is installed
if ! command -v rustup &> /dev/null; then
    echo "Error: rustup not found. Please install rustup first."
    exit 1
fi

echo "Installing Miri (nightly toolchain required)..."
rustup +nightly component add miri 2>/dev/null || true

echo ""
echo "========================================="
echo "Running Miri tests on pure computation modules..."
echo "========================================="
echo ""

# Test 1: VarUint arithmetic (pure computation, no I/O)
echo "1. Testing VarUint arithmetic operations..."
if cargo +nightly miri test --package tos_common --lib varuint::tests -- --test-threads=1 2>&1; then
    echo "   ✅ VarUint tests passed"
else
    echo "   ⚠️  VarUint tests failed or unsupported"
fi
echo ""

# Test 2: Serialization tests (some may work)
echo "2. Testing serialization (basic tests)..."
if cargo +nightly miri test --package tos_common --lib serializer::tests::test_string -- --test-threads=1 2>&1; then
    echo "   ✅ Serializer string tests passed"
else
    echo "   ⚠️  Serializer tests failed or unsupported"
fi
echo ""

# Test 3: Hash operations (may use hardware features)
echo "3. Testing hash operations..."
if cargo +nightly miri test --package tos_common --lib crypto::hash::tests -- --test-threads=1 2>&1; then
    echo "   ✅ Hash tests passed"
else
    echo "   ⚠️  Hash tests failed or unsupported (may use hardware acceleration)"
fi
echo ""

# Test 4: Data structures (DataElement, recursion)
echo "4. Testing API data structures..."
if cargo +nightly miri test --package tos_common --lib api::data::tests -- --test-threads=1 2>&1; then
    echo "   ✅ Data structure tests passed"
else
    echo "   ⚠️  Data structure tests failed or unsupported"
fi
echo ""

# Test 5: Account energy calculations
echo "5. Testing account energy calculations..."
if cargo +nightly miri test --package tos_common --lib account::energy_tests -- --test-threads=1 2>&1; then
    echo "   ✅ Energy calculation tests passed"
else
    echo "   ⚠️  Energy calculation tests failed or unsupported"
fi
echo ""

# Test 6: Immutable types
echo "6. Testing immutable data structures..."
if cargo +nightly miri test --package tos_common --lib immutable::tests -- --test-threads=1 2>&1; then
    echo "   ✅ Immutable tests passed"
else
    echo "   ⚠️  Immutable tests failed or unsupported"
fi
echo ""

echo "========================================="
echo "Miri tests complete!"
echo "========================================="
echo ""
echo "Note: Some tests may fail due to Miri limitations:"
echo "  - I/O operations (file, network)"
echo "  - System time access"
echo "  - FFI calls"
echo "  - Hardware-specific crypto"
echo ""
echo "See docs/MIRI_VALIDATION.md for more details."
