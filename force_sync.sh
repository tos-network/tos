#!/bin/bash
#
# force_sync.sh - Manual sync trigger for TOS daemon (BUG-004 Phase 3)
#
# Usage:
#   ./force_sync.sh                    # Auto-select best peer, local daemon
#   ./force_sync.sh 192.168.1.100:2125 # Sync from specific peer
#   ./force_sync.sh - 157.7.65.157     # Auto-select peer, remote daemon
#   ./force_sync.sh 192.168.1.100:2125 157.7.65.157  # Specific peer, remote daemon
#

set -e

PEER_ADDRESS="${1:-}"
DAEMON_HOST="${2:-127.0.0.1}"
DAEMON_PORT="${3:-8080}"

# Handle "-" as placeholder for auto-select
if [ "$PEER_ADDRESS" = "-" ]; then
    PEER_ADDRESS=""
fi

RPC_URL="http://${DAEMON_HOST}:${DAEMON_PORT}/json_rpc"

echo "=== TOS Force Sync Tool ==="
echo "Daemon: ${RPC_URL}"

# Check current status first
echo ""
echo "Checking current status..."
STATUS=$(curl -s "${RPC_URL}" -d '{"jsonrpc":"2.0","method":"get_info","id":1}' -H 'Content-Type: application/json')

if [ $? -ne 0 ] || [ -z "$STATUS" ]; then
    echo "ERROR: Cannot connect to daemon at ${RPC_URL}"
    exit 1
fi

OUR_TOPO=$(echo "$STATUS" | jq -r '.result.topoheight // "N/A"')
VERSION=$(echo "$STATUS" | jq -r '.result.version // "N/A"')
echo "  Version: ${VERSION}"
echo "  Our topoheight: ${OUR_TOPO}"

# Check P2P status
P2P_STATUS=$(curl -s "${RPC_URL}" -d '{"jsonrpc":"2.0","method":"p2p_status","id":1}' -H 'Content-Type: application/json')
BEST_TOPO=$(echo "$P2P_STATUS" | jq -r '.result.best_topoheight // "N/A"')
PEER_COUNT=$(echo "$P2P_STATUS" | jq -r '.result.peer_count // 0')
echo "  Best peer topoheight: ${BEST_TOPO}"
echo "  Connected peers: ${PEER_COUNT}"

if [ "$BEST_TOPO" != "N/A" ] && [ "$OUR_TOPO" != "N/A" ]; then
    GAP=$((BEST_TOPO - OUR_TOPO))
    echo "  Gap: ${GAP} blocks"
fi

# Build request
if [ -n "$PEER_ADDRESS" ]; then
    echo ""
    echo "Requesting sync from peer: ${PEER_ADDRESS}"
    REQUEST='{"jsonrpc":"2.0","method":"force_sync","params":{"peer_address":"'"${PEER_ADDRESS}"'"},"id":1}'
else
    echo ""
    echo "Requesting sync from best available peer..."
    REQUEST='{"jsonrpc":"2.0","method":"force_sync","params":{},"id":1}'
fi

# Execute force_sync
RESPONSE=$(curl -s "${RPC_URL}" -d "${REQUEST}" -H 'Content-Type: application/json')

# Check for errors
ERROR=$(echo "$RESPONSE" | jq -r '.error.message // empty')
if [ -n "$ERROR" ]; then
    echo ""
    echo "ERROR: ${ERROR}"
    exit 1
fi

# Parse success response
SUCCESS=$(echo "$RESPONSE" | jq -r '.result.success // false')
if [ "$SUCCESS" = "true" ]; then
    SYNC_PEER=$(echo "$RESPONSE" | jq -r '.result.peer // "N/A"')
    BEFORE=$(echo "$RESPONSE" | jq -r '.result.our_topoheight_before // "N/A"')
    AFTER=$(echo "$RESPONSE" | jq -r '.result.our_topoheight_after // "N/A"')
    SYNCED=$(echo "$RESPONSE" | jq -r '.result.blocks_synced // 0')

    echo ""
    echo "=== Sync Completed ==="
    echo "  Peer: ${SYNC_PEER}"
    echo "  Topoheight before: ${BEFORE}"
    echo "  Topoheight after: ${AFTER}"
    echo "  Blocks synced: ${SYNCED}"
else
    echo ""
    echo "Sync response:"
    echo "$RESPONSE" | jq .
fi
