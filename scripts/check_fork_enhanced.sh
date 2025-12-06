#!/bin/bash
# ============================================================================
# TOS Fork Detection Script (Enhanced)
#
# Compares block hashes, blue_work, and blue_score across nodes.
# More reliable fork detection than height/hash alone.
#
# Usage: ./check_fork_enhanced.sh [CHECK_HEIGHT]
#
# Reference: TOS_FORK_PREVENTION_IMPLEMENTATION_V2.md
# ============================================================================

NODES="127.0.0.1:12126 127.0.0.1:22126 127.0.0.1:32126"
CHECK_HEIGHT="${1:-1000}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

echo "=============================================="
echo "TOS Fork Detection (Enhanced) - $(date)"
echo "=============================================="

# Arrays to store values for comparison
declare -a HEIGHTS
declare -a HASHES
declare -a BLUE_WORKS
declare -a BLUE_SCORES

idx=0
for node in $NODES; do
    echo ""
    echo -e "${CYAN}Node: $node${NC}"

    # Get node info
    INFO=$(curl -s --connect-timeout 5 "http://$node/json_rpc" \
        -d '{"jsonrpc":"2.0","method":"get_info","id":1}' 2>/dev/null)

    if [ -z "$INFO" ] || [ "$INFO" == "null" ]; then
        echo -e "  ${YELLOW}Status: NOT REACHABLE${NC}"
        continue
    fi

    HEIGHT=$(echo $INFO | jq -r '.result.topoheight // "N/A"')
    TOP_HASH=$(echo $INFO | jq -r '.result.top_block_hash // "N/A"')
    VERSION=$(echo $INFO | jq -r '.result.version // "N/A"')

    echo "  Version:     $VERSION"
    echo "  Topoheight:  $HEIGHT"
    echo "  Top Hash:    $TOP_HASH"

    # Get blue_work and blue_score from the top block
    BLOCK_INFO=$(curl -s --connect-timeout 5 "http://$node/json_rpc" \
        -d "{\"jsonrpc\":\"2.0\",\"method\":\"get_block_by_hash\",\"params\":{\"hash\":\"$TOP_HASH\"},\"id\":1}" 2>/dev/null)

    if [ ! -z "$BLOCK_INFO" ] && [ "$BLOCK_INFO" != "null" ]; then
        BLUE_WORK=$(echo $BLOCK_INFO | jq -r '.result.blue_work // "N/A"')
        BLUE_SCORE=$(echo $BLOCK_INFO | jq -r '.result.blue_score // "N/A"')
        echo "  Blue Work:   $BLUE_WORK"
        echo "  Blue Score:  $BLUE_SCORE"

        # Store for comparison
        HEIGHTS[$idx]=$HEIGHT
        HASHES[$idx]=$TOP_HASH
        BLUE_WORKS[$idx]=$BLUE_WORK
        BLUE_SCORES[$idx]=$BLUE_SCORE
    else
        echo -e "  ${YELLOW}Could not fetch block details${NC}"
    fi

    ((idx++))
done

echo ""
echo "=============================================="
echo "Fork Analysis"
echo "=============================================="

# Compare all nodes
FORK_DETECTED=false
for ((i=0; i<idx; i++)); do
    for ((j=i+1; j<idx; j++)); do
        if [ "${HASHES[$i]}" != "${HASHES[$j]}" ]; then
            FORK_DETECTED=true
            echo -e "${RED}FORK DETECTED between nodes!${NC}"
            echo "  Node $i: ${HASHES[$i]} (bw=${BLUE_WORKS[$i]})"
            echo "  Node $j: ${HASHES[$j]} (bw=${BLUE_WORKS[$j]})"

            # Compare blue_work to determine heavier chain
            BW_I=$(echo "${BLUE_WORKS[$i]}" | tr -d '"')
            BW_J=$(echo "${BLUE_WORKS[$j]}" | tr -d '"')
            if [[ "$BW_I" > "$BW_J" ]]; then
                echo -e "  ${GREEN}Node $i has heavier chain (higher blue_work)${NC}"
            elif [[ "$BW_J" > "$BW_I" ]]; then
                echo -e "  ${GREEN}Node $j has heavier chain (higher blue_work)${NC}"
            else
                echo -e "  ${YELLOW}Same blue_work - tie breaker needed${NC}"
            fi
        fi
    done
done

if [ "$FORK_DETECTED" = false ]; then
    echo -e "${GREEN}No fork detected - all nodes have same top hash${NC}"
fi

# Check for height discrepancies
HEIGHT_DIFF=0
MAX_HEIGHT=0
MIN_HEIGHT=999999999
for ((i=0; i<idx; i++)); do
    if [ ${HEIGHTS[$i]} -gt $MAX_HEIGHT ]; then
        MAX_HEIGHT=${HEIGHTS[$i]}
    fi
    if [ ${HEIGHTS[$i]} -lt $MIN_HEIGHT ]; then
        MIN_HEIGHT=${HEIGHTS[$i]}
    fi
done

if [ $idx -gt 0 ]; then
    HEIGHT_DIFF=$((MAX_HEIGHT - MIN_HEIGHT))
    if [ $HEIGHT_DIFF -gt 0 ]; then
        echo ""
        echo "Height difference: $HEIGHT_DIFF blocks"
        if [ $HEIGHT_DIFF -gt 10 ]; then
            echo -e "${YELLOW}WARNING: Large height difference (>10 blocks)${NC}"
        fi
    fi
fi

echo ""
echo "=============================================="
echo "Check completed at $(date)"
echo "=============================================="
