#!/usr/bin/env python3
"""
Terminos Wallet Batch Mode Test Suite

This script tests the wallet's batch mode functionality by executing
various wallet commands with parameters to ensure they work without
requiring interactive input.

Usage:
    python3 wallet_batch_test.py

Requirements:
    - Python 3.6+
    - Cargo and Rust toolchain
    - terminos-wallet binary must be built
"""

import subprocess
import sys
import json
import time
import os
import tempfile
import shutil
from pathlib import Path
from typing import Dict, List, Tuple, Optional

class WalletBatchTester:
    """Test wallet batch mode functionality"""
    
    def __init__(self):
        self.test_results = []
        self.wallet_binary = "../target/debug/terminos_wallet"
        self.test_wallet_dir = None
        self.wallet_name = "test_wallet_batch"
        self.wallet_password = "test123"
        
    def run_wallet_command(self, cmd_with_args: str, timeout: int = 30) -> Dict:
        """Run wallet command and return result"""
        try:
            # Use batch mode with the wallet binary directly
            # The command and arguments should be passed as a single string to --cmd
            cmd = [self.wallet_binary, "--batch-mode", "--cmd", cmd_with_args, "--wallet-path", self.wallet_name, "--password", self.wallet_password]
            
            print(f"Running: {' '.join(cmd)}")
            
            result = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                timeout=timeout,
                cwd=os.getcwd()  # Run from current directory
            )
            
            return {
                "success": result.returncode == 0,
                "stdout": result.stdout,
                "stderr": result.stderr,
                "returncode": result.returncode,
                "command": ' '.join(cmd)
            }
        except subprocess.TimeoutExpired:
            return {
                "success": False,
                "stdout": "",
                "stderr": "Command timed out",
                "returncode": -1,
                "command": cmd_with_args
            }
        except FileNotFoundError:
            return {
                "success": False,
                "stdout": "",
                "stderr": "Wallet binary not found",
                "returncode": -1,
                "command": cmd_with_args
            }
    
    def test_no_parameter_commands(self) -> List[Tuple[str, Dict]]:
        """Test commands that don't require parameters (Command::new)"""
        print("\n=== Testing No-Parameter Commands ===")
        
        commands = [
            "display_address",
            "status", 
            "energy_info",
            "nonce",
            "tx_version",
            "logout",
            "clear_tx_cache",
            "offline_mode",
            "multisig_show"
        ]
        
        results = []
        for cmd in commands:
            print(f"\nTesting: {cmd}")
            result = self.run_wallet_command(cmd)
            results.append((cmd, result))
            
            if result["success"]:
                print(f"✅ {cmd}: PASSED")
            else:
                print(f"❌ {cmd}: FAILED")
                print(f"   Error: {result['stderr']}")
        
        return results
    
    def test_optional_parameter_commands(self) -> List[Tuple[str, Dict]]:
        """Test commands with optional parameters (Command::with_optional_arguments)"""
        print("\n=== Testing Optional Parameter Commands ===")
        
        # Test with correct parameter formats
        commands_with_params = [
            ("list_balances 1", "list_balances with page=1"),
            ("list_assets 1", "list_assets with page=1"),
            ("list_tracked_assets 1", "list_tracked_assets with page=1"),
            ("history 1", "history with page=1"),
            ("seed 0", "seed with language=0"),
            ("online_mode 127.0.0.1:8080", "online_mode with daemon_address"),
            ("rescan 1000", "rescan with topoheight=1000"),
        ]
        
        results = []
        for cmd_with_args, description in commands_with_params:
            print(f"\nTesting: {description}")
            result = self.run_wallet_command(cmd_with_args)
            results.append((cmd_with_args, result))
            
            if result["success"]:
                print(f"✅ {cmd_with_args}: PASSED")
            else:
                print(f"❌ {cmd_with_args}: FAILED")
                print(f"   Error: {result['stderr']}")
        
        return results
    
    def test_required_parameter_commands(self) -> List[Tuple[str, Dict]]:
        """Test commands with required parameters (Command::with_required_arguments)"""
        print("\n=== Testing Required Parameter Commands ===")
        
        # Test with correct parameter formats
        commands_with_params = [
            ("export_transactions test_export.csv", "export_transactions with filename"),
            ("freeze_tos 100000000 7 yes", "freeze_tos with amount, duration, confirm"),
            ("unfreeze_tos 50000000 yes", "unfreeze_tos with amount, confirm"),
            ("set_asset_name 0000000000000000000000000000000000000000000000000000000000000000", "set_asset_name with hash"),
        ]
        
        results = []
        for cmd_with_args, description in commands_with_params:
            print(f"\nTesting: {description}")
            result = self.run_wallet_command(cmd_with_args)
            results.append((cmd_with_args, result))
            
            if result["success"]:
                print(f"✅ {cmd_with_args}: PASSED")
            else:
                print(f"❌ {cmd_with_args}: FAILED")
                print(f"   Error: {result['stderr']}")
        
        return results
    
    def test_transaction_commands(self) -> List[Tuple[str, Dict]]:
        """Test transaction-related commands with correct parameters"""
        print("\n=== Testing Transaction Commands ===")
        
        # Test transfer command with various parameters
        # Note: These commands might fail due to insufficient balance or invalid addresses
        # but we're testing the parameter parsing, not the actual transaction execution
        transfer_tests = [
            ("transfer tos:jppcqn7cz48ccy2rd53wfnuedrtjl933vays6n45qju8tm5wupuqqqyjc78 100000000 tos yes", "transfer with address, amount, fee_type, confirm"),
            ("transfer_all tos:jppcqn7cz48ccy2rd53wfnuedrtjl933vays6n45qju8tm5wupuqqqyjc78 tos yes", "transfer_all with address, fee_type, confirm"),
            ("burn 10000000 yes", "burn with amount, confirm"),
        ]
        
        results = []
        for cmd_with_args, description in transfer_tests:
            print(f"\nTesting: {description}")
            result = self.run_wallet_command(cmd_with_args)
            results.append((cmd_with_args, result))
            
            if result["success"]:
                print(f"✅ {cmd_with_args}: PASSED")
            else:
                print(f"❌ {cmd_with_args}: FAILED")
                print(f"   Error: {result['stderr']}")
        
        return results
    
    def test_hash_parameter_commands(self) -> List[Tuple[str, Dict]]:
        """Test commands that require hash parameters"""
        print("\n=== Testing Hash Parameter Commands ===")
        
        # Use a valid hash format (64 hex characters)
        test_hash = "0000000000000000000000000000000000000000000000000000000000000000"
        
        hash_commands = [
            (f"balance {test_hash}", "balance with hash"),
            (f"track_asset {test_hash}", "track_asset with hash"),
            (f"untrack_asset {test_hash}", "untrack_asset with hash"),
        ]
        
        results = []
        for cmd_with_args, description in hash_commands:
            print(f"\nTesting: {description}")
            result = self.run_wallet_command(cmd_with_args)
            results.append((cmd_with_args, result))
            
            if result["success"]:
                print(f"✅ {cmd_with_args}: PASSED")
            else:
                print(f"❌ {cmd_with_args}: FAILED")
                print(f"   Error: {result['stderr']}")
        
        return results
    
    def test_interactive_commands(self) -> List[Tuple[str, Dict]]:
        """Test commands that normally require interactive input"""
        print("\n=== Testing Interactive Commands ===")
        
        # These commands normally require interactive input but might work with parameters
        interactive_commands = [
            ("balance", "balance without parameters (should fail)"),
            ("transfer", "transfer without parameters (should fail)"),
            ("freeze_tos", "freeze_tos without parameters (should fail)"),
        ]
        
        results = []
        for cmd_with_args, description in interactive_commands:
            print(f"\nTesting: {description}")
            result = self.run_wallet_command(cmd_with_args)
            results.append((cmd_with_args, result))
            
            # These should fail because they require parameters
            if not result["success"]:
                print(f"✅ {cmd_with_args}: CORRECTLY FAILED (expected)")
            else:
                print(f"❌ {cmd_with_args}: UNEXPECTEDLY PASSED")
        
        return results
    
    def test_command_parameter_formats(self) -> List[Tuple[str, Dict]]:
        """Test different parameter formats to understand how arguments are parsed"""
        print("\n=== Testing Command Parameter Formats ===")
        
        # Test different ways to pass parameters
        format_tests = [
            ("display_address", "display_address (no params)"),
            ("list_balances 1", "list_balances with page=1"),
            ("list_balances 0", "list_balances with page=0"),
            ("list_balances 10", "list_balances with page=10"),
            ("seed 1", "seed with language=1"),
            ("online_mode localhost:8080", "online_mode with daemon_address"),
            ("freeze_tos 100000000 7 yes", "freeze_tos with amount, duration, confirm"),
            ("unfreeze_tos 50000000 yes", "unfreeze_tos with amount, confirm"),
        ]
        
        results = []
        for cmd_with_args, description in format_tests:
            print(f"\nTesting: {description}")
            result = self.run_wallet_command(cmd_with_args)
            results.append((cmd_with_args, result))
            
            if result["success"]:
                print(f"✅ {cmd_with_args}: PASSED")
            else:
                print(f"❌ {cmd_with_args}: FAILED")
                print(f"   Error: {result['stderr']}")
        
        return results
    
    def run_all_tests(self) -> bool:
        """Run all batch mode tests"""
        print("🚀 Starting Terminos Wallet Batch Mode Tests")
        print("=" * 50)
        
        # Test different categories of commands
        all_results = []
        
        # Command parameter format tests
        all_results.extend(self.test_command_parameter_formats())
        
        # No parameter commands
        all_results.extend(self.test_no_parameter_commands())
        
        # Optional parameter commands  
        all_results.extend(self.test_optional_parameter_commands())
        
        # Required parameter commands
        all_results.extend(self.test_required_parameter_commands())
        
        # Hash parameter commands
        all_results.extend(self.test_hash_parameter_commands())
        
        # Transaction commands
        all_results.extend(self.test_transaction_commands())
        
        # Interactive commands (should fail)
        all_results.extend(self.test_interactive_commands())
        
        # Summary
        print("\n" + "=" * 50)
        print("📊 TEST SUMMARY")
        print("=" * 50)
        
        passed = 0
        total = len(all_results)
        
        for test_name, result in all_results:
            if result["success"]:
                print(f"✅ {test_name}: PASSED")
                passed += 1
            else:
                print(f"❌ {test_name}: FAILED")
                print(f"   Command: {result['command']}")
                print(f"   Error: {result['stderr']}")
        
        print(f"\nResults: {passed}/{total} tests passed")
        
        if passed == total:
            print("🎉 All batch mode tests passed!")
            return True
        else:
            print(f"⚠️  {total - passed} tests failed")
            return False
    
    def generate_test_report(self, results: List[Tuple[str, Dict]]) -> str:
        """Generate a detailed test report"""
        report = []
        report.append("# Terminos Wallet Batch Mode Test Report")
        report.append(f"Generated: {time.strftime('%Y-%m-%d %H:%M:%S')}")
        report.append("")
        
        for test_name, result in results:
            status = "PASSED" if result["success"] else "FAILED"
            report.append(f"## {test_name}: {status}")
            report.append(f"**Command:** `{result['command']}`")
            report.append(f"**Return Code:** {result['returncode']}")
            
            if result["stdout"]:
                report.append(f"**Output:**\n```\n{result['stdout']}\n```")
            
            if result["stderr"]:
                report.append(f"**Error:**\n```\n{result['stderr']}\n```")
            
            report.append("")
        
        return "\n".join(report)

def main():
    """Main test runner"""
    tester = WalletBatchTester()
    
    try:
        success = tester.run_all_tests()
        sys.exit(0 if success else 1)
    except KeyboardInterrupt:
        print("\n⚠️  Tests interrupted by user")
        sys.exit(1)
    except Exception as e:
        print(f"\n💥 Test runner error: {e}")
        sys.exit(1)

if __name__ == "__main__":
    main() 