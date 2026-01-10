#!/bin/bash
set -e

echo "Building cpi-test-callee contract..."
cargo build --release --target=bpfel-unknown-none
echo "✓ cpi-test-callee built successfully"
ls -lh ../target/bpfel-unknown-none/release/libcpi_test_callee.so
