#!/bin/bash

# TOS Testnet Daemon Setup Script
# This script configures and starts TOS daemon for testnet with optimized settings

set -e  # Exit on error

echo "========================================"
echo "TOS Testnet Daemon Setup"
echo "========================================"

# Check if we're in the correct directory
if [[ ! -f "Cargo.toml" ]] || [[ ! -d "daemon" ]]; then
    echo "❌ Error: Please run this script from TOS project root directory"
    echo "Current directory: $(pwd)"
    echo "Please run: cd ~/tos && ./daemon/setup_testnet_daemon.sh"
    exit 1
fi

echo "🔧 Building TOS daemon for testnet..."
cargo build --release --bin tos_daemon

if [[ $? -eq 0 ]]; then
    echo "✅ TOS daemon build successful!"

    # Create systemd service file for testnet
    echo "📋 Creating testnet systemd service..."
    sudo tee /etc/systemd/system/tos-testnet-daemon.service > /dev/null << EOF
[Unit]
Description=TOS Testnet Daemon
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=$(pwd)
ExecStart=$(pwd)/target/release/tos_daemon \\
    --network testnet \\
    --rpc-bind-address 0.0.0.0:8080 \\
    --p2p-bind-address 0.0.0.0:2125 \\
    --allow-boost-sync \\
    --auto-prune-keep-n-blocks 2000
Restart=always
RestartSec=5
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
EOF

    # Reload systemd and enable service
    sudo systemctl daemon-reload
    sudo systemctl enable tos-testnet-daemon

    echo "✅ Testnet systemd service created successfully"
    echo ""
    echo "========================================"
    echo "🎉 Testnet Setup Completed!"
    echo "========================================"
    echo ""
    echo "📋 Available commands:"
    echo "Start testnet: sudo systemctl start tos-testnet-daemon"
    echo "Stop testnet: sudo systemctl stop tos-testnet-daemon"
    echo "Check status: sudo systemctl status tos-testnet-daemon"
    echo "View logs: sudo journalctl -u tos-testnet-daemon -f"
    echo ""
    echo "🌐 Testnet Configuration:"
    echo "Network: Testnet"
    echo "P2P port: 2125"
    echo "RPC port: 8080"
    echo "Seed node: 157.7.65.157:2125 (this server)"
    echo ""
    echo "⚡ Optimization features:"
    echo "- Boost sync: Enabled for faster synchronization"
    echo "- Auto-pruning: Keep 2000 blocks (reduced storage)"
    echo "- Address prefix: 'tst' (testnet addresses)"
    echo ""
    echo "💾 Expected storage usage: ~50-100 MB"
    echo "⚠️  Note: As seed node, first startup will create testnet genesis block"
    echo ""
    echo "🚀 To start immediately:"
    echo "sudo systemctl start tos-testnet-daemon"

else
    echo "❌ Build failed, please check error messages"
    exit 1
fi