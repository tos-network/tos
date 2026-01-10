#!/bin/bash
set -e

echo "Building cpi-callee contract..."
cargo build --release --target=tbpf-tos-tos
echo "✓ cpi-callee built successfully"
ls -lh target/tbpf-tos-tos/release/cpi_callee.so
