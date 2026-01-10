#!/bin/bash
set -e

echo "Building cpi-caller contract..."
cargo build --release --target=tbpf-tos-tos
echo "✓ cpi-caller built successfully"
ls -lh target/tbpf-tos-tos/release/cpi_caller.so
