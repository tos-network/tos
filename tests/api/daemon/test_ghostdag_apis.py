"""
Test GHOSTDAG APIs (TIP-2)

Tests GHOSTDAG-specific APIs introduced or modified in TIP-2:
- get_block_at_topoheight: Get block using topoheight (sequential index)
- get_blocks_at_height: Get multiple blocks at same height (DAG)
- Block structure: parents_by_level, blue_score, blue_work
"""

import pytest
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from lib.rpc_client import TosRpcClient, RpcError
from config import TestConfig


@pytest.fixture
def client():
    """Create RPC client fixture"""
    client = TosRpcClient()
    if not client.ping():
        pytest.skip("Daemon not available")
    return client


@pytest.mark.tip2
def test_get_block_at_topoheight(client):
    """Test get_block_at_topoheight API"""
    # Get current topoheight
    info = client.get_info()
    current_topoheight = info["topoheight"]

    # Get block at current topoheight
    block = client.get_block_at_topoheight(current_topoheight)

    # Validate block structure (flat, not nested)
    assert "hash" in block
    assert "topoheight" in block
    # transactions field is optional (may be txs_hashes instead)
    assert "transactions" in block or "txs_hashes" in block


@pytest.mark.tip2
def test_block_header_ghostdag_fields(client):
    """Test block header contains GHOSTDAG fields"""
    info = client.get_info()
    current_topoheight = info["topoheight"]

    block = client.get_block_at_topoheight(current_topoheight)
    header = block  # Block structure is flat

    # GHOSTDAG fields (API uses 'height' for blue_score)
    assert "height" in header, "Missing height (blue_score) in header"
    assert "blue_work" in header, "Missing blue_work in header"

    # Validate types
    assert isinstance(header["height"], int)

    # Validate values
    assert header["height"] >= 0


@pytest.mark.tip2
def test_block_header_parents_by_level(client):
    """Test block header has tips structure (TIP-2)"""
    info = client.get_info()
    current_topoheight = info["topoheight"]

    # Skip genesis block
    if current_topoheight == 0:
        pytest.skip("Cannot test parents on genesis block")

    block = client.get_block_at_topoheight(current_topoheight)
    header = block  # Block structure is flat

    # TIP-2: API exposes this as 'tips' field (direct parent hashes)
    assert "tips" in header, "Missing tips (parent hashes) in header"
    assert isinstance(header["tips"], list)
    assert len(header["tips"]) > 0, "Block should have at least one parent"

    # Validate parent hashes
    for parent_hash in header["tips"]:
        assert isinstance(parent_hash, str)
        assert len(parent_hash) == 64, "Invalid hash length"


@pytest.mark.tip2
def test_blue_score_increases(client):
    """Test that height (blue_score) increases over time"""
    info = client.get_info()
    current_topoheight = info["topoheight"]

    if current_topoheight < 10:
        pytest.skip("Not enough blocks")

    # Get two blocks
    recent_block = client.get_block_at_topoheight(current_topoheight)
    older_block = client.get_block_at_topoheight(current_topoheight - 10)

    # API uses 'height' for blue_score
    recent_height = recent_block["height"]
    older_height = older_block["height"]

    # Height (blue score) should increase
    assert recent_height > older_height, (
        f"Height (blue score) should increase: "
        f"older={older_height}, recent={recent_height}"
    )


@pytest.mark.tip2
def test_topoheight_sequential(client):
    """Test topoheight provides sequential indexing"""
    info = client.get_info()
    current_topoheight = info["topoheight"]

    if current_topoheight < 5:
        pytest.skip("Not enough blocks")

    # Get several consecutive blocks by topoheight
    blocks = []
    for i in range(5):
        topoheight = current_topoheight - i
        block = client.get_block_at_topoheight(topoheight)
        blocks.append(block)

    # All blocks should be unique
    hashes = [b["hash"] for b in blocks]
    assert len(hashes) == len(set(hashes)), "Duplicate blocks found"


@pytest.mark.tip2
def test_genesis_block_special_case(client):
    """Test genesis block (topoheight 0) has special properties"""
    try:
        genesis = client.get_block_at_topoheight(0)
    except RpcError:
        pytest.skip("Genesis block not accessible")

    # Block structure is flat (no nested header)
    header = genesis

    # Genesis should have no parents (empty tips list)
    if "tips" in header:
        assert len(header["tips"]) == 0, "Genesis block should have no parents"

    # Genesis height (blue_score) should be 0
    assert header["height"] == 0, "Genesis height (blue_score) should be 0"


@pytest.mark.tip2
def test_block_timestamp_field(client):
    """Test block header has timestamp"""
    info = client.get_info()
    block = client.get_block_at_topoheight(info["topoheight"])
    header = block  # Block structure is flat

    assert "timestamp" in header
    assert isinstance(header["timestamp"], int)
    assert header["timestamp"] > 0


@pytest.mark.tip2
def test_block_difficulty_bits(client):
    """Test block header has difficulty"""
    info = client.get_info()
    block = client.get_block_at_topoheight(info["topoheight"])
    header = block  # Block structure is flat

    # API uses 'difficulty' field (string, not bits)
    assert "difficulty" in header
    assert isinstance(header["difficulty"], str)
    assert int(header["difficulty"]) > 0


@pytest.mark.tip2
def test_invalid_topoheight(client):
    """Test error handling for invalid topoheight"""
    info = client.get_info()
    invalid_topoheight = info["topoheight"] + 100000

    with pytest.raises(RpcError) as exc_info:
        client.get_block_at_topoheight(invalid_topoheight)

    # Should get an error
    assert exc_info.value.code != 0


@pytest.mark.tip2
def test_blue_work_accumulation(client):
    """Test that blue_work accumulates correctly"""
    info = client.get_info()
    current_topoheight = info["topoheight"]

    if current_topoheight < 10:
        pytest.skip("Not enough blocks")

    # Get two blocks
    recent_block = client.get_block_at_topoheight(current_topoheight)
    older_block = client.get_block_at_topoheight(current_topoheight - 10)

    recent_work = recent_block["blue_work"]
    older_work = older_block["blue_work"]

    # blue_work is stored as string (U256), convert to int for comparison
    recent_work_int = int(recent_work, 16) if isinstance(recent_work, str) else recent_work
    older_work_int = int(older_work, 16) if isinstance(older_work, str) else older_work

    # Blue work should accumulate (increase)
    assert recent_work_int > older_work_int, (
        f"Blue work should accumulate: "
        f"older={older_work_int}, recent={recent_work_int}"
    )


if __name__ == "__main__":
    pytest.main([__file__, "-v", "-s"])
