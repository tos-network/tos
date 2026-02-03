"""
Report generation for conformance test results.
"""

import json
import os
from dataclasses import asdict, dataclass
from datetime import datetime
from typing import Any, Dict, List, Optional

from comparator import ComparisonResult, Divergence


@dataclass
class TestResult:
    """Result of a single test vector."""
    vector_name: str
    suite_name: str
    passed: bool
    execution_time_ms: float
    comparison: Optional[ComparisonResult] = None
    error: Optional[str] = None


@dataclass
class SuiteResult:
    """Result of a test suite (collection of vectors)."""
    suite_name: str
    total_tests: int
    passed_tests: int
    failed_tests: int
    skipped_tests: int
    execution_time_ms: float
    test_results: List[TestResult]

    @property
    def pass_rate(self) -> float:
        if self.total_tests == 0:
            return 0.0
        return self.passed_tests / self.total_tests * 100


@dataclass
class ConformanceReport:
    """Complete conformance test report."""
    timestamp: str
    clients: List[str]
    reference_client: str
    total_suites: int
    total_tests: int
    total_passed: int
    total_failed: int
    total_divergences: int
    execution_time_ms: float
    suite_results: List[SuiteResult]
    divergences: List[Divergence]


class ReportGenerator:
    """Generates conformance test reports."""

    def __init__(self, result_dir: str):
        """
        Initialize report generator.

        Args:
            result_dir: Directory to write reports to
        """
        self.result_dir = result_dir
        os.makedirs(result_dir, exist_ok=True)

    def generate_report(
        self,
        suite_results: List[SuiteResult],
        clients: List[str],
        reference_client: str,
        execution_time_ms: float,
    ) -> ConformanceReport:
        """
        Generate a complete conformance report.

        Args:
            suite_results: Results from all test suites
            clients: List of client names tested
            reference_client: Name of reference client
            execution_time_ms: Total execution time

        Returns:
            ConformanceReport object
        """
        # Aggregate results
        total_tests = sum(s.total_tests for s in suite_results)
        total_passed = sum(s.passed_tests for s in suite_results)
        total_failed = sum(s.failed_tests for s in suite_results)

        # Collect all divergences
        divergences = []
        for suite in suite_results:
            for test in suite.test_results:
                if test.comparison and test.comparison.divergences:
                    divergences.extend(test.comparison.divergences)

        return ConformanceReport(
            timestamp=datetime.utcnow().isoformat() + "Z",
            clients=clients,
            reference_client=reference_client,
            total_suites=len(suite_results),
            total_tests=total_tests,
            total_passed=total_passed,
            total_failed=total_failed,
            total_divergences=len(divergences),
            execution_time_ms=execution_time_ms,
            suite_results=suite_results,
            divergences=divergences,
        )

    def write_json_report(
        self,
        report: ConformanceReport,
        filename: str = "conformance-report.json",
    ) -> str:
        """
        Write report as JSON file.

        Args:
            report: ConformanceReport to write
            filename: Output filename

        Returns:
            Path to written file
        """
        path = os.path.join(self.result_dir, filename)

        # Convert to dict for JSON serialization
        report_dict = self._report_to_dict(report)

        with open(path, "w") as f:
            json.dump(report_dict, f, indent=2)

        return path

    def write_summary(
        self,
        report: ConformanceReport,
        filename: str = "conformance-summary.txt",
    ) -> str:
        """
        Write human-readable summary.

        Args:
            report: ConformanceReport to summarize
            filename: Output filename

        Returns:
            Path to written file
        """
        path = os.path.join(self.result_dir, filename)

        lines = [
            "=" * 60,
            "TOS Conformance Test Report",
            "=" * 60,
            f"Timestamp: {report.timestamp}",
            f"Clients: {', '.join(report.clients)}",
            f"Reference: {report.reference_client}",
            "",
            "Results:",
            f"  Total Tests:  {report.total_tests}",
            f"  Passed:       {report.total_passed}",
            f"  Failed:       {report.total_failed}",
            f"  Divergences:  {report.total_divergences}",
            f"  Pass Rate:    {report.total_passed / max(report.total_tests, 1) * 100:.1f}%",
            f"  Duration:     {report.execution_time_ms:.2f}ms",
            "",
        ]

        # Add suite summaries
        lines.append("Suite Results:")
        for suite in report.suite_results:
            status = "PASS" if suite.failed_tests == 0 else "FAIL"
            lines.append(
                f"  [{status}] {suite.suite_name}: "
                f"{suite.passed_tests}/{suite.total_tests} "
                f"({suite.pass_rate:.1f}%)"
            )

        # Add divergence details if any
        if report.divergences:
            lines.append("")
            lines.append("Divergences:")
            for div in report.divergences:
                lines.append(f"  - {div.vector_name} ({div.field}):")
                lines.append(f"      {div.reference_client}: {div.expected}")
                lines.append(f"      {div.client}: {div.actual}")
                if div.details:
                    lines.append(f"      Details: {div.details}")

        lines.append("")
        lines.append("=" * 60)

        with open(path, "w") as f:
            f.write("\n".join(lines))

        return path

    def print_summary(self, report: ConformanceReport) -> None:
        """Print summary to console."""
        print("\n" + "=" * 60)
        print("TOS Conformance Test Results")
        print("=" * 60)
        print(f"Clients: {', '.join(report.clients)}")
        print(f"Reference: {report.reference_client}")
        print()
        print(f"Total:      {report.total_tests}")
        print(f"Passed:     {report.total_passed}")
        print(f"Failed:     {report.total_failed}")
        print(f"Divergences: {report.total_divergences}")
        print(f"Pass Rate:  {report.total_passed / max(report.total_tests, 1) * 100:.1f}%")
        print()

        if report.divergences:
            print("DIVERGENCES FOUND:")
            for div in report.divergences[:10]:  # Show first 10
                print(f"  - {div.vector_name}: {div.field}")
            if len(report.divergences) > 10:
                print(f"  ... and {len(report.divergences) - 10} more")

        status = "PASSED" if report.total_failed == 0 else "FAILED"
        print()
        print(f"Overall: {status}")
        print("=" * 60)

    def _report_to_dict(self, report: ConformanceReport) -> Dict[str, Any]:
        """Convert report to dictionary for JSON serialization."""
        return {
            "timestamp": report.timestamp,
            "clients": report.clients,
            "reference_client": report.reference_client,
            "total_suites": report.total_suites,
            "total_tests": report.total_tests,
            "total_passed": report.total_passed,
            "total_failed": report.total_failed,
            "total_divergences": report.total_divergences,
            "execution_time_ms": report.execution_time_ms,
            "suite_results": [
                {
                    "suite_name": s.suite_name,
                    "total_tests": s.total_tests,
                    "passed_tests": s.passed_tests,
                    "failed_tests": s.failed_tests,
                    "skipped_tests": s.skipped_tests,
                    "execution_time_ms": s.execution_time_ms,
                    "pass_rate": s.pass_rate,
                }
                for s in report.suite_results
            ],
            "divergences": [
                {
                    "field": d.field,
                    "expected": str(d.expected),
                    "actual": str(d.actual),
                    "client": d.client,
                    "reference_client": d.reference_client,
                    "vector_name": d.vector_name,
                    "details": d.details,
                }
                for d in report.divergences
            ],
        }
