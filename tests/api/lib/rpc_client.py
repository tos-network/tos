"""
TOS JSON-RPC Client for Testing

Provides a simple interface for making JSON-RPC calls to TOS daemon.
"""

import json
import time
from typing import Any, Dict, List, Optional, Union
import requests
from config import TestConfig


class RpcError(Exception):
    """RPC call error"""

    def __init__(self, code: int, message: str, data: Any = None):
        self.code = code
        self.message = message
        self.data = data
        super().__init__(f"RPC Error {code}: {message}")


class TosRpcClient:
    """TOS JSON-RPC Client"""

    def __init__(
        self,
        url: Optional[str] = None,
        timeout: Optional[int] = None,
        debug: bool = False,
    ):
        """
        Initialize RPC client

        Args:
            url: RPC endpoint URL (default: from config)
            timeout: Request timeout in seconds (default: from config)
            debug: Enable debug logging
        """
        self.url = url or TestConfig.DAEMON_RPC_URL
        self.timeout = (timeout or TestConfig.RPC_TIMEOUT) / 1000  # Convert ms to seconds
        self.debug = debug or TestConfig.DEBUG
        self.request_id = 0

    def call(
        self,
        method: str,
        params: Union[List, Dict, None] = None,
        timeout: Optional[int] = None,
    ) -> Any:
        """
        Make JSON-RPC call

        Args:
            method: RPC method name
            params: Method parameters (list or dict)
            timeout: Override timeout for this call

        Returns:
            Result from RPC call

        Raises:
            RpcError: If RPC returns error
            requests.RequestException: If network error occurs
        """
        self.request_id += 1

        # Build JSON-RPC request
        payload = {
            "jsonrpc": "2.0",
            "method": method,
            "params": params or [],
            "id": self.request_id,
        }

        if self.debug:
            print(f"[RPC Request] {method}")
            print(f"  Params: {json.dumps(params, indent=2)}")

        start_time = time.time()

        try:
            response = requests.post(
                self.url,
                json=payload,
                timeout=timeout or self.timeout,
                headers={"Content-Type": "application/json"},
            )
            response.raise_for_status()

        except requests.RequestException as e:
            if self.debug:
                print(f"[RPC Error] Network error: {e}")
            raise

        elapsed_ms = (time.time() - start_time) * 1000

        if self.debug:
            print(f"[RPC Response] {method} took {elapsed_ms:.2f}ms")

        # Parse JSON-RPC response
        try:
            result = response.json()
        except json.JSONDecodeError as e:
            raise RpcError(-32700, f"Invalid JSON: {e}")

        # Check for JSON-RPC error
        if "error" in result:
            error = result["error"]
            if self.debug:
                print(f"[RPC Error] {error}")
            raise RpcError(
                error.get("code", -1),
                error.get("message", "Unknown error"),
                error.get("data"),
            )

        # Return result
        if "result" not in result:
            raise RpcError(-32600, "Invalid response: missing 'result' field")

        if self.debug:
            print(f"  Result: {json.dumps(result['result'], indent=2)[:200]}...")

        return result["result"]

    def get_info(self) -> Dict[str, Any]:
        """Get network info (convenience method)"""
        return self.call("get_info", [])

    def get_block_at_topoheight(self, topoheight: int) -> Dict[str, Any]:
        """Get block at specific topoheight"""
        return self.call("get_block_at_topoheight", [topoheight])

    def get_balance(self, address: str, asset: Optional[str] = None) -> Dict[str, Any]:
        """Get balance for address"""
        params = {
            "address": address,
            "asset": asset or TestConfig.TOS_ASSET
        }
        return self.call("get_balance", params)

    def get_balance_at_topoheight(
        self, address: str, topoheight: int, asset: Optional[str] = None
    ) -> Dict[str, Any]:
        """Get balance at specific topoheight"""
        params = {
            "address": address,
            "asset": asset or TestConfig.TOS_ASSET,
            "topoheight": topoheight
        }
        return self.call("get_balance_at_topoheight", params)

    def submit_transaction(self, tx_hex: str) -> Dict[str, Any]:
        """Submit transaction"""
        return self.call("submit_transaction", [tx_hex])

    def get_miner_work(self, address: str) -> Dict[str, Any]:
        """Get miner work"""
        return self.call("get_miner_work", [address])

    def get_nonce(self, address: str) -> Dict[str, Any]:
        """Get nonce for address"""
        return self.call("get_nonce", {"address": address})

    def get_nonce_at_topoheight(self, address: str, topoheight: int) -> Dict[str, Any]:
        """Get nonce at specific topoheight"""
        return self.call("get_nonce_at_topoheight", {
            "address": address,
            "topoheight": topoheight
        })

    def has_balance(self, address: str, asset: Optional[str] = None, topoheight: Optional[int] = None) -> Dict[str, Any]:
        """Check if address has balance (returns object with 'exist' field)"""
        params = {
            "address": address,
            "asset": asset or TestConfig.TOS_ASSET
        }
        if topoheight is not None:
            params["topoheight"] = topoheight
        return self.call("has_balance", params)

    def has_nonce(self, address: str, topoheight: Optional[int] = None) -> Dict[str, Any]:
        """Check if address has nonce (returns object with 'exist' field)"""
        params = {"address": address}
        if topoheight is not None:
            params["topoheight"] = topoheight
        return self.call("has_nonce", params)

    def get_stable_balance(self, address: str, asset: Optional[str] = None) -> Dict[str, Any]:
        """Get stable balance for address"""
        return self.call("get_stable_balance", {
            "address": address,
            "asset": asset or TestConfig.TOS_ASSET
        })

    def get_account_history(
        self,
        address: str,
        asset: Optional[str] = None,
        minimum_topoheight: Optional[int] = None,
        maximum_topoheight: Optional[int] = None
    ) -> Dict[str, Any]:
        """Get account history"""
        params = {"address": address}
        if asset:
            params["asset"] = asset
        if minimum_topoheight is not None:
            params["minimum_topoheight"] = minimum_topoheight
        if maximum_topoheight is not None:
            params["maximum_topoheight"] = maximum_topoheight
        return self.call("get_account_history", params)

    def get_account_assets(self, address: str) -> Dict[str, Any]:
        """Get assets held by account"""
        return self.call("get_account_assets", {"address": address})

    def is_account_registered(self, address: str, in_stable_height: bool = True) -> bool:
        """Check if account is registered (returns direct boolean)"""
        return self.call("is_account_registered", {
            "address": address,
            "in_stable_height": in_stable_height
        })

    def get_account_registration_topoheight(self, address: str) -> int:
        """Get topoheight when account was registered (returns direct integer)"""
        return self.call("get_account_registration_topoheight", {"address": address})

    def validate_address(self, address: str, allow_integrated: bool = True) -> Dict[str, Any]:
        """Validate address"""
        return self.call("validate_address", {
            "address": address,
            "allow_integrated": allow_integrated
        })

    def ping(self) -> bool:
        """
        Check if daemon is reachable

        Returns:
            True if daemon responds
        """
        try:
            result = self.call("get_info", [])
            return "blue_score" in result
        except Exception:
            return False

    def wait_for_daemon(self, timeout: int = 60, interval: float = 1.0) -> bool:
        """
        Wait for daemon to become available

        Args:
            timeout: Maximum wait time in seconds
            interval: Check interval in seconds

        Returns:
            True if daemon becomes available, False on timeout
        """
        start = time.time()
        while time.time() - start < timeout:
            if self.ping():
                return True
            time.sleep(interval)
        return False


if __name__ == "__main__":
    # Test RPC client
    client = TosRpcClient(debug=True)

    print("Testing RPC connection...")
    if client.ping():
        print("✓ Daemon is reachable")

        # Test get_info
        info = client.get_info()
        print("\nNetwork Info:")
        print(f"  Blue Score:    {info.get('blue_score')}")
        print(f"  Topoheight:    {info.get('topoheight')}")
        print(f"  BPS:           {info.get('bps')}")
        print(f"  Actual BPS:    {info.get('actual_bps')}")
    else:
        print("✗ Daemon is not reachable")
        print(f"  URL: {client.url}")
