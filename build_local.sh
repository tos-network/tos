#!/bin/bash

# TOS Network - Local build script
# Builds only for the current platform

set -e

# Source cargo environment (needed for remote SSH sessions)
if [ -f "$HOME/.cargo/env" ]; then
    source "$HOME/.cargo/env"
fi

binaries=("tos_daemon" "tos_miner" "tos_wallet" "tos_genesis" "tos_ai_miner")

echo "🔨 Building TOS Network for local platform..."

# Detect current platform
if [[ "$OSTYPE" == "linux-gnu"* ]]; then
    target="x86_64-unknown-linux-gnu"
elif [[ "$OSTYPE" == "darwin"* ]]; then
    if [[ $(uname -m) == "arm64" ]]; then
        target="aarch64-apple-darwin"
    else
        target="x86_64-apple-darwin"
    fi
elif [[ "$OSTYPE" == "msys" ]] || [[ "$OSTYPE" == "win32" ]]; then
    target="x86_64-pc-windows-gnu"
else
    echo "❌ Unsupported platform: $OSTYPE"
    exit 1
fi

echo "📋 Target platform: $target"
echo "📦 Building binaries: ${binaries[*]}"
echo ""

# Build all binaries
echo "🚀 Starting build..."
cargo build --release

echo ""
echo "📁 Creating local build directory..."
rm -rf build/local
mkdir -p build/local

# Copy binaries
echo "📋 Copying binaries..."
for binary in "${binaries[@]}"; do
    binary_name="$binary"
    if [[ "$OSTYPE" == "msys" ]] || [[ "$OSTYPE" == "win32" ]]; then
        binary_name="$binary.exe"
    fi

    if [[ -f "target/release/$binary_name" ]]; then
        cp "target/release/$binary_name" "build/local/$binary_name"
        echo "  ✓ $binary_name"
    else
        echo "  ⚠ Warning: $binary_name not found"
    fi
done

# Copy documentation
echo "📄 Copying documentation..."
docs=("README.md" "BOOTSTRAP_NODE_DEPLOYMENT.md")
for doc in "${docs[@]}"; do
    if [[ -f "$doc" ]]; then
        cp "$doc" "build/local/$doc"
        echo "  ✓ $doc"
    fi
done

# Generate checksums
echo "🔐 Generating checksums..."
cd build/local
> checksums.txt
for binary in "${binaries[@]}"; do
    binary_name="$binary"
    if [[ "$OSTYPE" == "msys" ]] || [[ "$OSTYPE" == "win32" ]]; then
        binary_name="$binary.exe"
    fi

    if [[ -f "$binary_name" ]]; then
        sha256sum "$binary_name" >> checksums.txt
    fi
done
cd ../..

echo ""
echo "🎉 Local build completed successfully!"
echo "📁 Binaries available in: build/local/"
echo "🔐 Checksums: build/local/checksums.txt"
echo ""
echo "✨ Ready to run TOS Network! ✨"