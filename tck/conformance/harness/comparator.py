"""
Result comparison logic for conformance testing.
"""

from dataclasses import dataclass
from typing import Any, Dict, List, Optional


@dataclass
class Divergence:
    """Represents a divergence between client implementations."""
    field: str
    expected: Any
    actual: Any
    client: str
    reference_client: str
    vector_name: str
    details: Optional[str] = None


@dataclass
class ComparisonResult:
    """Result of comparing outputs from multiple clients."""
    success: bool
    divergences: List[Divergence]
    clients_compared: List[str]

    @property
    def has_divergences(self) -> bool:
        return len(self.divergences) > 0


class ResultComparator:
    """Compares results from multiple client implementations."""

    def __init__(self, reference_client: str = "tos-rust"):
        """
        Initialize comparator.

        Args:
            reference_client: The client to use as reference (default: tos-rust)
        """
        self.reference_client = reference_client

    def compare_results(
        self,
        results: Dict[str, Dict[str, Any]],
        vector_name: str,
    ) -> ComparisonResult:
        """
        Compare results from all clients.

        Args:
            results: Dict mapping client name to their result dict
            vector_name: Name of the test vector

        Returns:
            ComparisonResult with any divergences found
        """
        divergences = []
        clients = list(results.keys())

        if len(clients) < 2:
            return ComparisonResult(
                success=True,
                divergences=[],
                clients_compared=clients,
            )

        # Get reference result
        if self.reference_client not in results:
            raise ValueError(
                f"Reference client '{self.reference_client}' not in results"
            )
        reference = results[self.reference_client]

        # Compare each client against reference
        for client, result in results.items():
            if client == self.reference_client:
                continue

            client_divergences = self._compare_single(
                reference=reference,
                actual=result,
                client=client,
                vector_name=vector_name,
            )
            divergences.extend(client_divergences)

        return ComparisonResult(
            success=len(divergences) == 0,
            divergences=divergences,
            clients_compared=clients,
        )

    def _compare_single(
        self,
        reference: Dict[str, Any],
        actual: Dict[str, Any],
        client: str,
        vector_name: str,
    ) -> List[Divergence]:
        """Compare a single client result against reference."""
        divergences = []

        # Compare error codes
        ref_error = reference.get("error_code", 0)
        act_error = actual.get("error_code", 0)
        if ref_error != act_error:
            divergences.append(Divergence(
                field="error_code",
                expected=ref_error,
                actual=act_error,
                client=client,
                reference_client=self.reference_client,
                vector_name=vector_name,
                details=f"Error code mismatch: expected 0x{ref_error:04x}, got 0x{act_error:04x}",
            ))

        # Compare state digests
        ref_digest = reference.get("state_digest")
        act_digest = actual.get("state_digest")
        if ref_digest and act_digest and ref_digest != act_digest:
            divergences.append(Divergence(
                field="state_digest",
                expected=ref_digest,
                actual=act_digest,
                client=client,
                reference_client=self.reference_client,
                vector_name=vector_name,
                details="State digest mismatch after execution",
            ))

        # Compare success status
        ref_success = reference.get("success", True)
        act_success = actual.get("success", True)
        if ref_success != act_success:
            divergences.append(Divergence(
                field="success",
                expected=ref_success,
                actual=act_success,
                client=client,
                reference_client=self.reference_client,
                vector_name=vector_name,
            ))

        # Compare gas used (if present)
        ref_gas = reference.get("gas_used")
        act_gas = actual.get("gas_used")
        if ref_gas is not None and act_gas is not None and ref_gas != act_gas:
            divergences.append(Divergence(
                field="gas_used",
                expected=ref_gas,
                actual=act_gas,
                client=client,
                reference_client=self.reference_client,
                vector_name=vector_name,
            ))

        return divergences

    def compare_state_digests(
        self,
        digests: Dict[str, str],
        vector_name: str,
    ) -> ComparisonResult:
        """
        Compare state digests from all clients.

        Args:
            digests: Dict mapping client name to state digest hex string
            vector_name: Name of the test vector

        Returns:
            ComparisonResult with any divergences found
        """
        divergences = []
        clients = list(digests.keys())

        if len(clients) < 2:
            return ComparisonResult(
                success=True,
                divergences=[],
                clients_compared=clients,
            )

        reference_digest = digests.get(self.reference_client)
        if not reference_digest:
            raise ValueError(
                f"Reference client '{self.reference_client}' not in digests"
            )

        for client, digest in digests.items():
            if client == self.reference_client:
                continue

            if digest != reference_digest:
                divergences.append(Divergence(
                    field="state_digest",
                    expected=reference_digest,
                    actual=digest,
                    client=client,
                    reference_client=self.reference_client,
                    vector_name=vector_name,
                    details="State digest mismatch",
                ))

        return ComparisonResult(
            success=len(divergences) == 0,
            divergences=divergences,
            clients_compared=clients,
        )
