"""
Test Block Query RPC APIs

Tests various block query methods:
- get_block_at_topoheight, get_block_by_hash, get_top_block
- get_blocks_at_blue_score
- get_blocks_range_by_topoheight, get_blocks_range_by_blue_score
- get_dag_order
"""

import pytest
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from lib.rpc_client import TosRpcClient, RpcError


@pytest.fixture
def client():
    client = TosRpcClient()
    if not client.ping():
        pytest.skip("Daemon not available")
    return client


@pytest.mark.tip2
def test_get_block_by_hash(client):
    """Test get_block_by_hash API"""
    # Get a known block hash from top block
    top_block = client.call("get_top_block", [])
    block_hash = top_block["hash"]

    # Query by hash
    result = client.call("get_block_by_hash", [block_hash])

    assert result["hash"] == block_hash
    # Block structure is flat (no nested header)
    assert "topoheight" in result
    assert "timestamp" in result
    # transactions field is optional
    assert "txs_hashes" in result


def test_get_top_block(client):
    """Test get_top_block API"""
    result = client.call("get_top_block", [])

    # Block structure is flat
    assert "hash" in result
    assert "topoheight" in result
    assert "timestamp" in result
    assert isinstance(result["topoheight"], int)


@pytest.mark.tip2
def test_get_blocks_at_blue_score(client):
    """Test get_blocks_at_blue_score API (TIP-2)"""
    info = client.get_info()
    current_blue_score = info["blue_score"]

    # Get blocks at current blue score
    result = client.call("get_blocks_at_blue_score", [current_blue_score])

    assert isinstance(result, list)
    assert len(result) >= 1  # At least one block

    # All blocks should have same height (which is blue_score)
    for block in result:
        assert "height" in block
        assert block["height"] == current_blue_score


@pytest.mark.tip2
def test_get_blocks_range_by_topoheight(client):
    """Test get_blocks_range_by_topoheight API"""
    info = client.get_info()
    current_topo = info["topoheight"]

    if current_topo < 10:
        pytest.skip("Not enough blocks")

    # Get range of 5 blocks
    start = current_topo - 4
    end = current_topo
    result = client.call("get_blocks_range_by_topoheight", [start, end])

    assert isinstance(result, list)
    assert len(result) == 5  # Should return exactly 5 blocks

    # Verify sequential topoheights
    for i, block in enumerate(result):
        expected_topo = start + i
        assert block["topoheight"] == expected_topo


@pytest.mark.tip2
def test_get_blocks_range_by_blue_score(client):
    """Test get_blocks_range_by_blue_score API"""
    info = client.get_info()
    current_blue = info["blue_score"]

    if current_blue < 10:
        pytest.skip("Not enough blocks")

    # Get range
    start = current_blue - 4
    end = current_blue
    result = client.call("get_blocks_range_by_blue_score", [start, end])

    assert isinstance(result, list)
    assert len(result) >= 5  # At least 5 (could be more in DAG)


@pytest.mark.tip2
def test_get_dag_order(client):
    """Test get_dag_order API"""
    # Get top block topoheight
    top = client.call("get_top_block", [])
    topoheight = top["topoheight"]

    # API expects GetTopoHeightRangeParams with start and end
    result = client.call("get_dag_order", {"start": topoheight, "end": topoheight})

    # Result should be a list of blocks or hashes in DAG order
    assert isinstance(result, (list, dict))


# Block Structure Tests

@pytest.mark.tip2
def test_block_header_structure(client):
    """Test block header has all required fields"""
    info = client.get_info()
    block = client.get_block_at_topoheight(info["topoheight"])
    header = block  # Block structure is flat

    # Basic fields
    assert "version" in header
    assert "timestamp" in header
    assert "nonce" in header
    assert "difficulty" in header  # API uses 'difficulty' not 'bits'
    assert "miner" in header

    # GHOSTDAG fields (TIP-2)
    assert "height" in header  # API uses 'height' which is blue_score
    assert "blue_work" in header
    assert "tips" in header  # API uses 'tips' instead of 'parents_by_level'

    # Block identification
    assert "hash" in header
    assert "topoheight" in header


@pytest.mark.tip2
def test_block_transactions_structure(client):
    """Test block transactions structure"""
    info = client.get_info()
    block = client.get_block_at_topoheight(info["topoheight"])

    # API returns txs_hashes, not full transactions
    assert "txs_hashes" in block
    assert isinstance(block["txs_hashes"], list)


# Range Query Edge Cases

def test_get_blocks_range_single_block(client):
    """Test range query for single block"""
    info = client.get_info()
    topo = info["topoheight"]

    result = client.call("get_blocks_range_by_topoheight", [topo, topo])

    assert isinstance(result, list)
    assert len(result) == 1
    assert result[0]["topoheight"] == topo


def test_get_blocks_range_invalid_range(client):
    """Test range query with end < start"""
    info = client.get_info()
    topo = info["topoheight"]

    # Invalid range (end before start)
    with pytest.raises(RpcError):
        client.call("get_blocks_range_by_topoheight", [topo, topo - 10])


def test_get_blocks_range_too_large(client):
    """Test range query with very large range"""
    # Try to query 10000 blocks (should be rejected or limited)
    start = 0
    end = 10000

    try:
        result = client.call("get_blocks_range_by_topoheight", [start, end])
        # If it succeeds, verify reasonable limit
        assert len(result) <= 1000  # Reasonable limit
    except RpcError as e:
        # Expected - range too large
        assert e.code != 0


# Performance Tests

@pytest.mark.performance
def test_get_block_performance(client):
    """Test block query performance"""
    import time

    info = client.get_info()
    topo = info["topoheight"]

    iterations = 10
    times = []

    for _ in range(iterations):
        start = time.time()
        client.get_block_at_topoheight(topo)
        elapsed = (time.time() - start) * 1000
        times.append(elapsed)

    avg_time = sum(times) / len(times)
    print(f"\nget_block_at_topoheight: {avg_time:.2f}ms average")

    # Should be fast
    assert avg_time < 200  # 200ms threshold


if __name__ == "__main__":
    pytest.main([__file__, "-v", "-s"])
