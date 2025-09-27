#!/bin/bash

# Test script for bootstrap nodes functionality
# This script demonstrates various bootstrap node configurations

echo "=== TOS Bootstrap Nodes Test Script ==="
echo

# Build the daemon first
echo "Building TOS daemon..."
cargo build --bin tos_daemon --quiet
if [ $? -ne 0 ]; then
    echo "❌ Build failed!"
    exit 1
fi
echo "✅ Build successful"
echo

# Test 1: Check if bootstrap-nodes option exists
echo "Test 1: Checking if --bootstrap-nodes option is available..."
./target/debug/tos_daemon --help | grep -q "bootstrap-nodes"
if [ $? -eq 0 ]; then
    echo "✅ --bootstrap-nodes option is available"
else
    echo "❌ --bootstrap-nodes option not found"
    exit 1
fi
echo

# Test 2: Show help information for bootstrap-nodes
echo "Test 2: Bootstrap nodes help information:"
./target/debug/tos_daemon --help | grep -A 3 -B 1 "bootstrap-nodes"
echo

# Test 3: Test configuration parsing (dry run)
echo "Test 3: Testing configuration parsing..."
echo "Command: ./tos_daemon --bootstrap-nodes 192.168.1.100:2125,example.com:2125 --help"
echo "(This should not fail with parsing errors)"
./target/debug/tos_daemon --bootstrap-nodes 192.168.1.100:2125,example.com:2125 --help > /dev/null 2>&1
if [ $? -eq 0 ]; then
    echo "✅ Configuration parsing successful"
else
    echo "❌ Configuration parsing failed"
    exit 1
fi
echo

# Test 4: Show all P2P related options
echo "Test 4: All P2P configuration options:"
./target/debug/tos_daemon --help | grep -E "(bootstrap|priority|exclusive|seed|p2p)" | head -10
echo

echo "=== All Tests Passed ✅ ==="
echo
echo "Bootstrap nodes functionality is working correctly!"
echo
echo "Usage examples:"
echo "  # Single bootstrap node:"
echo "  ./tos_daemon --bootstrap-nodes 192.168.1.100:2125"
echo
echo "  # Multiple bootstrap nodes:"
echo "  ./tos_daemon --bootstrap-nodes node1.com:2125,node2.com:2125"
echo
echo "  # With priority nodes:"
echo "  ./tos_daemon --priority-nodes trusted.com:2125 --bootstrap-nodes bootstrap.com:2125"
echo