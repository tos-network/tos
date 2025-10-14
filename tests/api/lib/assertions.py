"""
Custom assertion helpers for TOS API tests
"""


def assert_valid_hash(value: str, message: str = "Invalid hash"):
    """Assert value is a valid hash (64 hex characters)"""
    assert isinstance(value, str), f"{message}: not a string"
    assert len(value) == 64, f"{message}: length {len(value)} != 64"
    assert all(c in "0123456789abcdef" for c in value.lower()), f"{message}: invalid characters"


def assert_valid_address(value: str, message: str = "Invalid address"):
    """Assert value is a valid address"""
    assert isinstance(value, str), f"{message}: not a string"
    assert value.startswith("tos") or value.startswith("tst"), f"{message}: invalid prefix"
    assert len(value) > 10, f"{message}: too short"


def assert_positive_integer(value: int, message: str = "Expected positive integer"):
    """Assert value is a positive integer"""
    assert isinstance(value, int), f"{message}: not an integer"
    assert value >= 0, f"{message}: negative value {value}"


def assert_bps_calculation(bps: float, block_time_target: int, tolerance: float = 0.001):
    """Assert BPS is correctly calculated from block_time_target"""
    expected_bps = 1000.0 / block_time_target
    diff = abs(bps - expected_bps)
    assert diff < tolerance, (
        f"BPS calculation incorrect: {bps} != {expected_bps} "
        f"(block_time_target={block_time_target})"
    )
