#!/bin/bash
# ============================================================================
# TOS Fork Point Finder
#
# Uses binary search to find the exact height where two nodes diverged.
#
# Usage: ./find_fork_point.sh <node1_url> <node2_url>
# Example: ./find_fork_point.sh 127.0.0.1:12126 127.0.0.1:22126
# ============================================================================

set -e

NODE1="${1:-127.0.0.1:12126}"
NODE2="${2:-127.0.0.1:22126}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

echo "=============================================="
echo "TOS Fork Point Finder"
echo "Node 1: $NODE1"
echo "Node 2: $NODE2"
echo "=============================================="

# Function to get block hash at height
get_hash() {
    local node=$1
    local height=$2
    curl -s --connect-timeout 10 "http://$node/json_rpc" \
        -d "{\"jsonrpc\":\"2.0\",\"method\":\"get_block_at_topoheight\",\"params\":{\"topoheight\":$height},\"id\":1}" 2>/dev/null \
        | jq -r '.result.hash // "NOT_FOUND"'
}

# Get current heights
echo ""
echo "Fetching node status..."
HEIGHT1=$(curl -s --connect-timeout 10 "http://$NODE1/json_rpc" \
    -d '{"jsonrpc":"2.0","method":"get_info","id":1}' 2>/dev/null \
    | jq -r '.result.topoheight // "N/A"')
HEIGHT2=$(curl -s --connect-timeout 10 "http://$NODE2/json_rpc" \
    -d '{"jsonrpc":"2.0","method":"get_info","id":1}' 2>/dev/null \
    | jq -r '.result.topoheight // "N/A"')

if [ "$HEIGHT1" == "N/A" ]; then
    echo -e "${RED}ERROR: Cannot connect to Node 1 ($NODE1)${NC}"
    exit 1
fi

if [ "$HEIGHT2" == "N/A" ]; then
    echo -e "${RED}ERROR: Cannot connect to Node 2 ($NODE2)${NC}"
    exit 1
fi

echo "Node 1 height: $HEIGHT1"
echo "Node 2 height: $HEIGHT2"

# Use minimum height as upper bound
if [ "$HEIGHT1" -lt "$HEIGHT2" ]; then
    MAX_HEIGHT=$HEIGHT1
else
    MAX_HEIGHT=$HEIGHT2
fi

echo "Search range: 0 to $MAX_HEIGHT"
echo ""
echo "Starting binary search..."
echo "----------------------------------------------"

# Binary search for fork point
LOW=0
HIGH=$MAX_HEIGHT
LAST_COMMON=0
ITERATIONS=0

while [ $LOW -le $HIGH ]; do
    MID=$(( (LOW + HIGH) / 2 ))
    ITERATIONS=$((ITERATIONS + 1))

    HASH1=$(get_hash $NODE1 $MID)
    HASH2=$(get_hash $NODE2 $MID)

    if [ "$HASH1" == "$HASH2" ] && [ "$HASH1" != "NOT_FOUND" ]; then
        echo -e "Height $MID: ${GREEN}MATCH${NC} (${HASH1:0:16}...)"
        LAST_COMMON=$MID
        LOW=$((MID + 1))
    else
        echo -e "Height $MID: ${RED}DIFFERENT${NC}"
        echo "  Node1: ${HASH1:0:32}..."
        echo "  Node2: ${HASH2:0:32}..."
        HIGH=$((MID - 1))
    fi
done

echo "----------------------------------------------"
echo "Binary search completed in $ITERATIONS iterations"
echo ""

# Results
FORK_HEIGHT=$((LAST_COMMON + 1))

echo -e "${CYAN}=============================================="
echo "RESULT:"
echo "==============================================${NC}"
echo ""
echo -e "  Last common block:  Height ${GREEN}$LAST_COMMON${NC}"
echo -e "  Fork started at:    Height ${RED}$FORK_HEIGHT${NC}"
echo ""

# Get details of fork point
echo "Fork Point Details:"
echo "----------------------------------------------"

COMMON_HASH=$(get_hash $NODE1 $LAST_COMMON)
echo "  Common ancestor (height $LAST_COMMON):"
echo "    Hash: $COMMON_HASH"
echo ""

if [ $FORK_HEIGHT -le $MAX_HEIGHT ]; then
    FORK_HASH1=$(get_hash $NODE1 $FORK_HEIGHT)
    FORK_HASH2=$(get_hash $NODE2 $FORK_HEIGHT)

    echo "  First divergent block (height $FORK_HEIGHT):"
    echo "    Node1: $FORK_HASH1"
    echo "    Node2: $FORK_HASH2"
fi

echo ""
echo "----------------------------------------------"
echo ""

# Calculate orphan chain sizes
ORPHAN1=$((HEIGHT1 - LAST_COMMON))
ORPHAN2=$((HEIGHT2 - LAST_COMMON))

echo "Chain Statistics:"
echo "  Node 1 blocks since fork: $ORPHAN1"
echo "  Node 2 blocks since fork: $ORPHAN2"

if [ $ORPHAN1 -gt $ORPHAN2 ]; then
    echo -e "  ${GREEN}Node 1 has longer chain${NC}"
elif [ $ORPHAN2 -gt $ORPHAN1 ]; then
    echo -e "  ${GREEN}Node 2 has longer chain${NC}"
else
    echo -e "  ${YELLOW}Both chains are same length${NC}"
fi

echo ""
echo "=============================================="
echo "To resolve: Delete chain data on shorter chain node and resync"
echo "=============================================="
