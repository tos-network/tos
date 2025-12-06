#!/bin/bash
# ============================================================================
# TOS Fork Detection Script
#
# Compares block hashes across multiple nodes to detect chain forks.
# Run this periodically (e.g., every 5 minutes via cron) for monitoring.
#
# Usage: ./check_fork.sh [CHECK_HEIGHT]
# ============================================================================

# Configuration - Edit these for your environment
NODES="127.0.0.1:12126 127.0.0.1:22126 127.0.0.1:32126"

# Height to check (default: 1000, or use first argument)
CHECK_HEIGHT="${1:-1000}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "=============================================="
echo "TOS Fork Detection - $(date)"
echo "Checking height: $CHECK_HEIGHT"
echo "=============================================="

FIRST_HASH=""
FORK_DETECTED=false
NODE_INFO=""

for node in $NODES; do
    # Get current height
    HEIGHT=$(curl -s --connect-timeout 5 "http://$node/json_rpc" \
        -d '{"jsonrpc":"2.0","method":"get_info","id":1}' 2>/dev/null \
        | jq -r '.result.topoheight // "N/A"')

    # Skip if node not reachable
    if [ "$HEIGHT" == "N/A" ] || [ "$HEIGHT" == "null" ] || [ -z "$HEIGHT" ]; then
        echo -e "${YELLOW}WARN${NC}: Node $node not reachable"
        continue
    fi

    # Skip if height too low
    if [ "$HEIGHT" -lt "$CHECK_HEIGHT" ] 2>/dev/null; then
        echo -e "${YELLOW}WARN${NC}: Node $node height ($HEIGHT) below check height ($CHECK_HEIGHT)"
        continue
    fi

    # Get block hash at check height
    HASH=$(curl -s --connect-timeout 5 "http://$node/json_rpc" \
        -d "{\"jsonrpc\":\"2.0\",\"method\":\"get_block_at_topoheight\",\"params\":{\"topoheight\":$CHECK_HEIGHT},\"id\":1}" 2>/dev/null \
        | jq -r '.result.hash // "N/A"')

    SHORT_HASH="${HASH:0:16}..."

    # Compare with first hash
    if [ -z "$FIRST_HASH" ]; then
        FIRST_HASH=$HASH
        NODE_INFO="$NODE_INFO\n  $node: Height=$HEIGHT, Hash=${GREEN}$SHORT_HASH${NC}"
    elif [ "$HASH" != "$FIRST_HASH" ] && [ "$HASH" != "N/A" ]; then
        FORK_DETECTED=true
        NODE_INFO="$NODE_INFO\n  $node: Height=$HEIGHT, Hash=${RED}$SHORT_HASH${NC} (DIFFERENT!)"
    else
        NODE_INFO="$NODE_INFO\n  $node: Height=$HEIGHT, Hash=${GREEN}$SHORT_HASH${NC}"
    fi
done

echo ""
echo "Node Status:"
echo "----------------------------------------------"
echo -e "$NODE_INFO"

echo ""
echo "----------------------------------------------"

if [ "$FORK_DETECTED" = true ]; then
    echo -e "${RED}ALERT: FORK DETECTED!${NC}"
    echo ""
    echo "Different block hashes found at height $CHECK_HEIGHT"
    echo "Reference hash: $FIRST_HASH"
    echo ""
    echo "ACTION REQUIRED: Investigate fork cause and resolve"
    echo "Use ./find_fork_point.sh to locate exact fork height"
    exit 1
else
    echo -e "${GREEN}OK: All nodes on same chain${NC}"
    echo "Block hash at height $CHECK_HEIGHT: ${FIRST_HASH:0:32}..."
    exit 0
fi
