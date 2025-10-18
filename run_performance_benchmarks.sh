#!/bin/bash

# Performance Benchmarks Runner
# Runs the two newly implemented performance benchmarks

echo "=========================================="
echo "TOS Performance Benchmarks"
echo "=========================================="
echo ""

cd daemon

echo "1. Running Transaction Throughput Benchmark..."
echo "----------------------------------------------"
cargo test --lib test_transaction_throughput_with_security -- --ignored --nocapture --test-threads=1 2>&1 | grep -A 20 "=== Performance Benchmark Results ==="

echo ""
echo ""
echo "2. Running GHOSTDAG Performance Benchmark..."
echo "----------------------------------------------"
cargo test --lib test_ghostdag_performance_benchmark -- --ignored --nocapture --test-threads=1 2>&1 | grep -A 10 "=== GHOSTDAG Performance Benchmark Results ==="

echo ""
echo "=========================================="
echo "Benchmarks Completed"
echo "=========================================="
