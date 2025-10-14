"""
Test data fixtures and generators
"""

import random
import string


def generate_test_address(network: str = "devnet") -> str:
    """
    Generate a random test address

    Args:
        network: Network type (devnet, testnet, mainnet)

    Returns:
        Random address string
    """
    prefix = {
        "devnet": "tst",
        "testnet": "tst",
        "mainnet": "tos"
    }.get(network, "tst")

    # Generate random address (not valid, just for testing)
    chars = string.ascii_lowercase + string.digits
    random_part = ''.join(random.choices(chars, k=60))

    return f"{prefix}1{random_part}"


def generate_test_transaction() -> dict:
    """
    Generate test transaction data

    Returns:
        Transaction dict
    """
    return {
        "source": generate_test_address(),
        "destination": generate_test_address(),
        "amount": random.randint(1000000, 10000000),
        "fee": random.randint(100, 1000),
        "nonce": random.randint(0, 100)
    }


def generate_test_hash() -> str:
    """
    Generate random hash string

    Returns:
        64-character hex hash
    """
    return ''.join(random.choices(string.hexdigits.lower(), k=64))
