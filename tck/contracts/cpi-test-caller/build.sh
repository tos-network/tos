#!/bin/bash
set -e

echo "Building cpi-test-caller contract..."
cargo build --release --target=bpfel-unknown-none
echo "✓ cpi-test-caller built successfully"
ls -lh ../target/bpfel-unknown-none/release/libcpi_test_caller.so
