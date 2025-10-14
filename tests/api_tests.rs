/// TOS API Integration Tests (Python)
///
/// This test suite runs Python-based integration tests for TOS APIs.
/// The tests are organized in tests/api/ directory.
///
/// To run these tests:
/// ```bash
/// cargo test --test api_tests
/// ```
///
/// To run specific test categories:
/// ```bash
/// cargo test --test api_tests test_daemon_get_info
/// cargo test --test api_tests test_tip2_apis
/// ```

use std::process::Command;
use std::path::PathBuf;

/// Get the root directory of the workspace
fn get_workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Get the Python test directory
fn get_test_dir() -> PathBuf {
    get_workspace_root().join("tests").join("api")
}

/// Check if Python 3 is available
fn check_python() -> bool {
    Command::new("python3")
        .arg("--version")
        .output()
        .is_ok()
}

/// Run Python tests with given arguments
fn run_python_tests(args: &[&str]) -> Result<(), String> {
    if !check_python() {
        return Err("Python 3 not found. Install Python 3.9+ to run API tests.".to_string());
    }

    let test_dir = get_test_dir();

    // Check if requirements are installed
    let check_deps = Command::new("python3")
        .current_dir(&test_dir)
        .args(&["-c", "import pytest"])
        .output();

    if check_deps.is_err() || !check_deps.unwrap().status.success() {
        eprintln!("\n⚠️  Python dependencies not installed!");
        eprintln!("Install with: pip install -r tests/api/requirements.txt\n");
        return Err("Python dependencies missing".to_string());
    }

    // Run tests
    let mut cmd = Command::new("python3");
    cmd.current_dir(&test_dir)
        .arg("run_tests.py")
        .args(args);

    println!("\n🧪 Running TOS API tests...");
    println!("Test directory: {:?}", test_dir);
    println!("Command: {:?}\n", cmd);

    let status = cmd.status()
        .map_err(|e| format!("Failed to execute Python tests: {}", e))?;

    if !status.success() {
        return Err(format!("Tests failed with exit code: {:?}", status.code()));
    }

    Ok(())
}

#[test]
fn test_python_environment() {
    /// Verify Python environment is set up correctly
    assert!(
        check_python(),
        "Python 3 is required for API tests. Install Python 3.9+"
    );

    let test_dir = get_test_dir();
    assert!(
        test_dir.exists(),
        "Test directory does not exist: {:?}",
        test_dir
    );

    let requirements = test_dir.join("requirements.txt");
    assert!(
        requirements.exists(),
        "requirements.txt not found: {:?}",
        requirements
    );
}

#[test]
#[ignore] // Run explicitly with: cargo test --test api_tests -- --ignored
fn test_all_api_tests() {
    /// Run all Python API tests
    run_python_tests(&["-v"]).expect("API tests failed");
}

#[test]
#[ignore]
fn test_daemon_apis() {
    /// Test daemon RPC APIs
    run_python_tests(&["--daemon", "-v"]).expect("Daemon API tests failed");
}

#[test]
#[ignore]
fn test_daemon_get_info() {
    /// Test get_info API specifically (includes TIP-2 bps fields)
    run_python_tests(&["daemon/test_get_info.py", "-v"])
        .expect("get_info API tests failed");
}

#[test]
#[ignore]
fn test_tip2_apis() {
    /// Test TIP-2 related APIs
    run_python_tests(&["--tip2", "-v"]).expect("TIP-2 API tests failed");
}

#[test]
#[ignore]
fn test_ai_mining_apis() {
    /// Test AI mining APIs
    run_python_tests(&["--ai-mining", "-v"]).expect("AI mining API tests failed");
}

#[test]
#[ignore]
fn test_integration() {
    /// Test integration scenarios
    run_python_tests(&["--integration", "-v"]).expect("Integration tests failed");
}

#[test]
#[ignore]
fn test_performance() {
    /// Run performance tests
    run_python_tests(&["--performance", "-v"]).expect("Performance tests failed");
}

// Convenience test for quick validation during development
#[test]
#[ignore]
fn test_quick_smoke_test() {
    /// Quick smoke test - just test get_info
    println!("\n🚀 Running quick smoke test (get_info API)...\n");
    run_python_tests(&["daemon/test_get_info.py::test_get_info_basic", "-v"])
        .expect("Smoke test failed");
}

#[cfg(test)]
mod helpers {
    use super::*;

    /// Print helpful information about running Python tests
    pub fn print_test_info() {
        println!("\n📚 TOS API Test Information");
        println!("=" .repeat(70));
        println!("Test directory: {:?}", get_test_dir());
        println!("\nTo run Python tests directly:");
        println!("  cd tests/api");
        println!("  python3 run_tests.py --help");
        println!("\nTo run via cargo:");
        println!("  cargo test --test api_tests -- --ignored");
        println!("  cargo test --test api_tests test_daemon_get_info -- --ignored");
        println!("=" .repeat(70));
        println!();
    }

    #[test]
    fn show_test_info() {
        print_test_info();
    }
}
