#!/bin/bash

# TOS Node Quick Fix - Install Rust and Cargo
echo "🔧 Quick Fix: Installing Rust and Cargo"

# Update package manager
apt update

# Method 1: Recommended - Install latest version using official rustup
echo "📥 Installing latest Rust using rustup..."
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable

# Reload environment variables
source $HOME/.cargo/env
export PATH="$HOME/.cargo/bin:$PATH"

# Verify installation
echo "✅ Verifying installation:"
rustc --version
cargo --version

# If the above fails, use backup method
if ! command -v cargo &> /dev/null; then
    echo "⚠️  rustup failed, using apt install..."
    apt install -y cargo rustc build-essential
fi

echo "🏗️  Now you can compile TOS:"
echo "cargo build --release --bin tos_daemon"