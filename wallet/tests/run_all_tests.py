#!/usr/bin/env python3
"""
Terminos Wallet Batch Mode Test Runner

This script runs all the individual test files for wallet batch mode commands.
It provides a convenient way to run all tests or specific test categories.

Usage:
    python3 run_all_tests.py                    # Run all tests
    python3 run_all_tests.py --display         # Run only display_address tests
    python3 run_all_tests.py --list            # Run only list commands tests
    python3 run_all_tests.py --balance         # Run only balance commands tests
    python3 run_all_tests.py --energy          # Run only energy commands tests
    python3 run_all_tests.py --transaction     # Run only transaction commands tests
    python3 run_all_tests.py --utility         # Run only utility commands tests

Requirements:
    - Python 3.6+
    - Cargo and Rust toolchain
    - terminos-wallet binary must be built
"""

import subprocess
import sys
import os
import argparse
from typing import Dict, List, Tuple

class TestRunner:
    """Run all wallet batch mode tests"""
    
    def __init__(self):
        self.test_files = {
            "basic": "test_basic_commands.py",
            "display": "test_display_address.py",
            "list": "test_list_commands.py",
            "balance": "test_balance_commands.py",
            "energy": "test_energy_commands.py",
            "transaction": "test_transaction_commands.py",
            "utility": "test_utility_commands.py",
            "wallet_management": "test_wallet_management.py",
            "server": "test_server_commands.py",
            "multisig": "test_multisig_commands.py",
        }
        
    def run_test_file(self, test_file: str) -> Dict:
        """Run a single test file and return results"""
        try:
            print(f"\n{'='*60}")
            print(f"Running: {test_file}")
            print(f"{'='*60}")
            
            # Get the current working directory (wallet directory)
            current_dir = os.getcwd()
            print(f"Current directory: {current_dir}")
            
            # Construct the full path to the test file
            test_file_path = os.path.join("tests", test_file)
            print(f"Test file path: {test_file_path}")
            
            result = subprocess.run(
                [sys.executable, test_file_path],
                capture_output=True,
                text=True,
                timeout=300,  # 5 minutes timeout
                cwd=current_dir  # Use current directory instead of script directory
            )
            
            # Print stdout and stderr for debugging
            if result.stdout:
                print("STDOUT:")
                print(result.stdout)
            if result.stderr:
                print("STDERR:")
                print(result.stderr)
            
            print(f"Return code: {result.returncode}")
            
            return {
                "success": result.returncode == 0,
                "stdout": result.stdout,
                "stderr": result.stderr,
                "returncode": result.returncode,
                "file": test_file
            }
        except subprocess.TimeoutExpired:
            return {
                "success": False,
                "stdout": "",
                "stderr": "Test timed out after 5 minutes",
                "returncode": -1,
                "file": test_file
            }
        except FileNotFoundError:
            return {
                "success": False,
                "stdout": "",
                "stderr": f"Test file not found: {test_file}",
                "returncode": -1,
                "file": test_file
            }
    
    def run_all_tests(self) -> List[Tuple[str, Dict]]:
        """Run all test files"""
        print("🚀 Starting All Wallet Batch Mode Tests")
        print("=" * 60)
        
        results = []
        for category, test_file in self.test_files.items():
            result = self.run_test_file(test_file)
            results.append((category, result))
            
            if result["success"]:
                print(f"✅ {category}: PASSED")
            else:
                print(f"❌ {category}: FAILED")
                print(f"   Error: {result['stderr']}")
                print(f"\n🛑 Stopping tests due to failure in {category}")
                break  # Stop on first failure
        
        return results
    
    def run_specific_tests(self, categories: List[str]) -> List[Tuple[str, Dict]]:
        """Run specific test categories"""
        print(f"🚀 Starting Specific Wallet Batch Mode Tests: {', '.join(categories)}")
        print("=" * 60)
        
        results = []
        for category in categories:
            if category in self.test_files:
                test_file = self.test_files[category]
                result = self.run_test_file(test_file)
                results.append((category, result))
                
                if result["success"]:
                    print(f"✅ {category}: PASSED")
                else:
                    print(f"❌ {category}: FAILED")
                    print(f"   Error: {result['stderr']}")
                    print(f"\n🛑 Stopping tests due to failure in {category}")
                    break  # Stop on first failure
            else:
                print(f"⚠️  Unknown test category: {category}")
                print(f"\n🛑 Stopping tests due to unknown category: {category}")
                break  # Stop on unknown category
        
        return results
    
    def print_summary(self, results: List[Tuple[str, Dict]]):
        """Print test summary"""
        print("\n" + "=" * 60)
        print("📊 FINAL TEST SUMMARY")
        print("=" * 60)
        
        passed = 0
        total = len(results)
        
        for category, result in results:
            if result["success"]:
                print(f"✅ {category}: PASSED")
                passed += 1
            else:
                print(f"❌ {category}: FAILED")
                print(f"   File: {result['file']}")
                print(f"   Error: {result['stderr']}")
        
        print(f"\nResults: {passed}/{total} test categories passed")
        
        if passed == total:
            print("🎉 All test categories passed!")
            return True
        else:
            print(f"⚠️  {total - passed} test categories failed")
            return False

def main():
    """Main test runner"""
    parser = argparse.ArgumentParser(description="Run wallet batch mode tests")
    parser.add_argument("--basic", action="store_true", help="Run only basic commands tests")
    parser.add_argument("--display", action="store_true", help="Run only display_address tests")
    parser.add_argument("--list", action="store_true", help="Run only list commands tests")
    parser.add_argument("--balance", action="store_true", help="Run only balance commands tests")
    parser.add_argument("--energy", action="store_true", help="Run only energy commands tests")
    parser.add_argument("--transaction", action="store_true", help="Run only transaction commands tests")
    parser.add_argument("--utility", action="store_true", help="Run only utility commands tests")
    parser.add_argument("--wallet-management", action="store_true", help="Run only wallet management commands tests")
    parser.add_argument("--server", action="store_true", help="Run only server commands tests")
    parser.add_argument("--multisig", action="store_true", help="Run only multisig commands tests")
    
    args = parser.parse_args()
    
    runner = TestRunner()
    
    try:
        # Check if specific tests are requested
        specific_tests = []
        if args.basic:
            specific_tests.append("basic")
        if args.display:
            specific_tests.append("display")
        if args.list:
            specific_tests.append("list")
        if args.balance:
            specific_tests.append("balance")
        if args.energy:
            specific_tests.append("energy")
        if args.transaction:
            specific_tests.append("transaction")
        if args.utility:
            specific_tests.append("utility")
        if args.wallet_management:
            specific_tests.append("wallet_management")
        if args.server:
            specific_tests.append("server")
        if args.multisig:
            specific_tests.append("multisig")
        
        if specific_tests:
            results = runner.run_specific_tests(specific_tests)
        else:
            results = runner.run_all_tests()
        
        success = runner.print_summary(results)
        sys.exit(0 if success else 1)
        
    except KeyboardInterrupt:
        print("\n⚠️  Tests interrupted by user")
        sys.exit(1)
    except Exception as e:
        print(f"\n💥 Test runner error: {e}")
        sys.exit(1)

if __name__ == "__main__":
    main() 