"""
Test helper functions for TOS API tests
"""

import time
from typing import Callable, Any


def wait_for_block(client, timeout: int = 60, interval: float = 1.0) -> bool:
    """
    Wait for a new block to be produced

    Args:
        client: RPC client instance
        timeout: Maximum wait time in seconds
        interval: Check interval in seconds

    Returns:
        True if new block detected, False on timeout
    """
    initial_info = client.get_info()
    initial_topoheight = initial_info["topoheight"]

    start = time.time()
    while time.time() - start < timeout:
        current_info = client.get_info()
        if current_info["topoheight"] > initial_topoheight:
            return True
        time.sleep(interval)

    return False


def wait_for_sync(client, timeout: int = 300, interval: float = 5.0) -> bool:
    """
    Wait for node to sync to network

    Args:
        client: RPC client instance
        timeout: Maximum wait time in seconds
        interval: Check interval in seconds

    Returns:
        True if synced, False on timeout
    """
    start = time.time()
    last_topoheight = 0
    stable_count = 0

    while time.time() - start < timeout:
        info = client.get_info()
        current_topoheight = info["topoheight"]

        # If topoheight hasn't changed for 3 checks, consider synced
        if current_topoheight == last_topoheight:
            stable_count += 1
            if stable_count >= 3:
                return True
        else:
            stable_count = 0
            last_topoheight = current_topoheight

        time.sleep(interval)

    return False


def retry_on_error(func: Callable, max_retries: int = 3, delay: float = 1.0) -> Any:
    """
    Retry function on error

    Args:
        func: Function to retry
        max_retries: Maximum number of retries
        delay: Delay between retries in seconds

    Returns:
        Function result

    Raises:
        Last exception if all retries fail
    """
    last_exception = None

    for attempt in range(max_retries):
        try:
            return func()
        except Exception as e:
            last_exception = e
            if attempt < max_retries - 1:
                time.sleep(delay)

    raise last_exception
