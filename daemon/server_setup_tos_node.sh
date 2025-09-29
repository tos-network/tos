#!/bin/bash

# TOS Node Server Installation Script
# For Ubuntu/Debian systems

set -e  # Exit on error

echo "========================================"
echo "TOS Node Server Installation Script"
echo "========================================"

# Update system packages
echo "📦 Updating system packages..."
apt update && apt upgrade -y

# Install required system dependencies
echo "🔧 Installing system dependencies..."
apt install -y \
    curl \
    wget \
    build-essential \
    pkg-config \
    libssl-dev \
    git \
    cmake \
    clang \
    llvm \
    libclang-dev \
    protobuf-compiler \
    htop \
    screen \
    ufw

# Install Rust (recommended way, newer than apt version)
echo "🦀 Installing Rust..."
if ! command -v rustc &> /dev/null; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
    source $HOME/.cargo/env
    echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.bashrc

    # Verify installation
    rustc --version
    cargo --version
    echo "✅ Rust installed successfully"
else
    echo "✅ Rust already installed"
fi

# Reload environment variables
source $HOME/.cargo/env

# Check if we're in the daemon directory or TOS root
if [[ -f "Cargo.toml" && -f "../Cargo.toml" ]]; then
    # We're in daemon directory, go to parent
    cd ..
elif [[ ! -f "Cargo.toml" ]] || [[ ! -d "daemon" ]]; then
    echo "❌ Error: Please run this script from TOS project root directory or daemon directory"
    echo "Current directory: $(pwd)"
    echo "Please run: cd ~/tos && ./daemon/server_setup_tos_node.sh"
    echo "Or: cd ~/tos/daemon && ./server_setup_tos_node.sh"
    exit 1
fi

# Setup firewall rules
echo "🔥 Configuring firewall..."
ufw --force enable
ufw allow ssh
ufw allow 2125/tcp  # P2P port
ufw allow 8080/tcp  # RPC port
echo "✅ Firewall configuration completed"

# Optimize build environment
echo "⚡ Optimizing build settings..."
export RUSTFLAGS="-C target-cpu=native"
export CARGO_INCREMENTAL=1

# Start building TOS daemon
echo "🏗️  Starting TOS daemon build (this may take 15-30 minutes)..."
echo "Build time: $(date)"

# Ensure sufficient memory, use swap if memory is insufficient
if [[ $(free -m | awk 'NR==2{printf "%.0f", $2}') -lt 2048 ]]; then
    echo "⚠️  Less than 2GB memory, recommending swap space"
    if [[ ! -f "/swapfile" ]]; then
        echo "Creating 2GB swap file..."
        fallocate -l 2G /swapfile
        chmod 600 /swapfile
        mkswap /swapfile
        swapon /swapfile
        echo '/swapfile none swap sw 0 0' | tee -a /etc/fstab
    fi
fi

# Build daemon
cargo build --release --bin tos_daemon

if [[ $? -eq 0 ]]; then
    echo "✅ TOS daemon build successful!"
    echo "📍 Binary location: $(pwd)/target/release/tos_daemon"

    # Display file information
    ls -lh target/release/tos_daemon

    # Create systemd service file
    echo "📋 Creating systemd service..."
    cat > /etc/systemd/system/tos-daemon.service << EOF
[Unit]
Description=TOS Daemon
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=$(pwd)
ExecStart=$(pwd)/target/release/tos_daemon --rpc-bind-address 0.0.0.0:8080 --p2p-bind-address 0.0.0.0:2125
Restart=always
RestartSec=5
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
EOF

    # Reload systemd and enable service
    systemctl daemon-reload
    systemctl enable tos-daemon

    echo "✅ systemd service created successfully"
    echo ""
    echo "========================================"
    echo "🎉 Installation completed!"
    echo "========================================"
    echo ""
    echo "📋 Available commands:"
    echo "Start service: systemctl start tos-daemon"
    echo "Stop service: systemctl stop tos-daemon"
    echo "Check status: systemctl status tos-daemon"
    echo "View logs: journalctl -u tos-daemon -f"
    echo ""
    echo "🌐 Network ports:"
    echo "P2P port: 2125"
    echo "RPC port: 8080"
    echo ""
    echo "⚠️  Note: First startup may require blockchain data synchronization"

else
    echo "❌ Build failed, please check error messages"
    exit 1
fi