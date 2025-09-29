#!/bin/bash

# TOS Miner Control Script
# Quick commands to manage TOS miner service

MINER_ADDRESS="tst12zacnuun3lkv5kxzn2jy8l28d0zft7rqhyxlz2v6h6u23xmruy7sqm0d38u"
DAEMON_ADDRESS="127.0.0.1:8080"
NUM_THREADS=1

print_usage() {
    echo "TOS Miner Control Script"
    echo ""
    echo "Usage: $0 {start|stop|status|logs|restart|install|manual}"
    echo ""
    echo "Commands:"
    echo "  start    - Start miner service"
    echo "  stop     - Stop miner service"
    echo "  status   - Show miner service status"
    echo "  logs     - Show real-time miner logs"
    echo "  restart  - Restart miner service"
    echo "  install  - Install and setup miner service"
    echo "  manual   - Run miner manually (foreground)"
    echo ""
    echo "Mining Configuration:"
    echo "  Address: ${MINER_ADDRESS}"
    echo "  Daemon:  ${DAEMON_ADDRESS}"
    echo "  Threads: ${NUM_THREADS}"
}

check_daemon() {
    if ! curl -s -f http://127.0.0.1:8080/json_rpc -X POST -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","method":"get_info","id":1}' >/dev/null 2>&1; then
        echo "❌ Error: Cannot connect to daemon at 127.0.0.1:8080"
        echo "Please ensure testnet daemon is running:"
        echo "  sudo systemctl start tos-testnet-daemon"
        return 1
    fi
    return 0
}

case "$1" in
    start)
        echo "🚀 Starting TOS miner service..."
        if check_daemon; then
            sudo systemctl start tos-miner
            echo "✅ Miner service started"
            echo "💡 Monitor with: $0 logs"
        fi
        ;;
    stop)
        echo "🛑 Stopping TOS miner service..."
        sudo systemctl stop tos-miner
        echo "✅ Miner service stopped"
        ;;
    status)
        echo "📊 Miner service status:"
        sudo systemctl status tos-miner --no-pager
        ;;
    logs)
        echo "📜 Real-time miner logs (Press Ctrl+C to exit):"
        sudo journalctl -u tos-miner -f --no-pager
        ;;
    restart)
        echo "🔄 Restarting TOS miner service..."
        sudo systemctl restart tos-miner
        echo "✅ Miner service restarted"
        echo "💡 Monitor with: $0 logs"
        ;;
    install)
        echo "📦 Installing TOS miner service..."
        if [[ -f "./miner/setup_miner.sh" ]]; then
            ./miner/setup_miner.sh
        else
            echo "❌ Error: setup_miner.sh not found"
            echo "Please run this script from TOS project root directory"
            exit 1
        fi
        ;;
    manual)
        echo "🔧 Starting miner manually..."
        echo "Press Ctrl+C to stop mining"
        if check_daemon; then
            ./target/release/tos_miner \
                --miner-address "${MINER_ADDRESS}" \
                --daemon-address "${DAEMON_ADDRESS}" \
                --num-threads ${NUM_THREADS} \
                --log-level info \
                --disable-log-color
        fi
        ;;
    *)
        print_usage
        exit 1
        ;;
esac