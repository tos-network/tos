#!/bin/bash

# TOS Testnet Miner Setup Script
# Sets up persistent background mining with systemd service

set -e  # Exit on error

echo "========================================"
echo "TOS Testnet Miner Setup"
echo "========================================"

# Miner configuration
MINER_ADDRESS="tst12zacnuun3lkv5kxzn2jy8l28d0zft7rqhyxlz2v6h6u23xmruy7sqm0d38u"
DAEMON_ADDRESS="127.0.0.1:8080"
NUM_THREADS=1

# Check if we're in the correct directory
if [[ ! -f "Cargo.toml" ]] || [[ ! -d "daemon" ]]; then
    echo "❌ Error: Please run this script from TOS project root directory"
    echo "Current directory: $(pwd)"
    echo "Please run: cd ~/tos && ./miner/setup_miner.sh"
    exit 1
fi

# Check if testnet daemon is running
echo "🔍 Checking testnet daemon status..."
if ! systemctl is-active --quiet tos-daemon 2>/dev/null; then
    echo "⚠️  Warning: Testnet daemon is not running"
    echo "Please start daemon first:"
    echo "  sudo systemctl start tos-daemon"
    read -p "Do you want to start daemon now? (y/N): " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        echo "🚀 Starting daemon..."
        sudo systemctl start tos-daemon
        echo "⏱️  Waiting for daemon to initialize..."
        sleep 10
    else
        echo "❌ Cannot setup miner without daemon running"
        exit 1
    fi
fi

# Verify daemon connectivity
echo "🔍 Verifying daemon connectivity..."
if ! curl -s -f http://127.0.0.1:8080/json_rpc -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","method":"get_info","id":1}' >/dev/null; then
    echo "❌ Error: Cannot connect to daemon at 127.0.0.1:8080"
    echo "Please ensure daemon is running and accessible"
    exit 1
fi
echo "✅ Daemon connectivity verified"

echo "🔧 Building TOS miner..."
cargo build --release --bin tos_miner

if [[ $? -eq 0 ]]; then
    echo "✅ TOS miner build successful!"

    # Create systemd service file for miner
    echo "📋 Creating miner systemd service..."
    sudo tee /etc/systemd/system/tos-miner.service > /dev/null << EOF
[Unit]
Description=TOS Testnet Miner
After=network.target tos-daemon.service
Requires=tos-daemon.service

[Service]
Type=simple
User=root
WorkingDirectory=$(pwd)
ExecStart=$(pwd)/target/release/tos_miner \\
    --miner-address ${MINER_ADDRESS} \\
    --daemon-address ${DAEMON_ADDRESS} \\
    --num-threads ${NUM_THREADS} \\
    --log-level info \\
    --disable-log-color \\
    --disable-ascii-art \\
    --disable-interactive-mode
Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal

# Resource limits
LimitNOFILE=65536
MemoryHigh=512M
MemoryMax=1G

[Install]
WantedBy=multi-user.target
EOF

    # Reload systemd and enable service
    sudo systemctl daemon-reload
    sudo systemctl enable tos-miner

    echo "✅ Miner systemd service created successfully"
    echo ""
    echo "========================================"
    echo "🎉 Miner Setup Completed!"
    echo "========================================"
    echo ""
    echo "📋 Mining Configuration:"
    echo "Miner Address: ${MINER_ADDRESS}"
    echo "Daemon Address: ${DAEMON_ADDRESS}"
    echo "Threads: ${NUM_THREADS}"
    echo "Network: Testnet"
    echo ""
    echo "📋 Available commands:"
    echo "Start miner: sudo systemctl start tos-miner"
    echo "Stop miner: sudo systemctl stop tos-miner"
    echo "Check status: sudo systemctl status tos-miner"
    echo "View logs: sudo journalctl -u tos-miner -f"
    echo "Restart miner: sudo systemctl restart tos-miner"
    echo ""
    echo "🚀 To start mining immediately:"
    echo "sudo systemctl start tos-miner"
    echo ""
    echo "💡 Monitor mining:"
    echo "sudo journalctl -u tos-miner -f --no-pager"

else
    echo "❌ Build failed, please check error messages"
    exit 1
fi