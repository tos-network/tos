"""
Configuration management for the conformance test harness.
"""

import os
from dataclasses import dataclass, field
from typing import Dict, Optional


@dataclass
class ClientConfig:
    """Configuration for a single client endpoint."""
    name: str
    endpoint: str
    enabled: bool = True
    timeout: float = 30.0


@dataclass
class HarnessConfig:
    """Main configuration for the test harness."""
    # Client endpoints
    clients: Dict[str, ClientConfig] = field(default_factory=dict)

    # Paths
    vector_dir: str = "/vectors"
    result_dir: str = "/results"

    # Execution settings
    parallel_tests: bool = False
    stop_on_first_failure: bool = False
    verbose: bool = False

    # Timeouts
    request_timeout: float = 30.0
    test_timeout: float = 300.0

    @classmethod
    def from_env(cls) -> "HarnessConfig":
        """Load configuration from environment variables."""
        config = cls()

        # Load client endpoints from environment
        rust_endpoint = os.environ.get("RUST_ENDPOINT", "http://localhost:8081")
        c_endpoint = os.environ.get("C_ENDPOINT", "http://localhost:8082")

        config.clients = {
            "tos-rust": ClientConfig(
                name="TOS Rust",
                endpoint=rust_endpoint,
            ),
            "avatar-c": ClientConfig(
                name="Avatar C",
                endpoint=c_endpoint,
            ),
        }

        # Load paths
        config.vector_dir = os.environ.get("VECTOR_DIR", "/vectors")
        config.result_dir = os.environ.get("RESULT_DIR", "/results")

        # Load settings
        config.verbose = os.environ.get("VERBOSE", "").lower() in ("true", "1", "yes")
        config.stop_on_first_failure = os.environ.get(
            "STOP_ON_FIRST_FAILURE", ""
        ).lower() in ("true", "1", "yes")

        return config

    def get_enabled_clients(self) -> Dict[str, ClientConfig]:
        """Get only enabled client configurations."""
        return {
            name: client
            for name, client in self.clients.items()
            if client.enabled
        }
