#[cfg(test)]
mod tests {
    use anyhow::Result;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Helper function to create unique wallet name
    fn create_unique_wallet_name(test_name: &str) -> String {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let pid = std::process::id();
        format!("test_wallet_{}_{}_{}", test_name, pid, timestamp)
    }

    /// Get the wallet binary path (release or debug)
    fn get_wallet_binary_path() -> &'static str {
        use std::path::Path;
        // Check for release binary first (CI runs with --release), then debug
        if Path::new("../target/release/tos_wallet").exists() {
            "../target/release/tos_wallet"
        } else {
            "../target/debug/tos_wallet"
        }
    }

    /// Helper function to run wallet command
    fn run_wallet_command(cmd: &str, wallet_name: &str) -> Result<std::process::Output> {
        let output = Command::new(get_wallet_binary_path())
            .args(&[
                "--precomputed-tables-l1",
                "13",
                "--exec",
                cmd,
                "--wallet-path",
                wallet_name,
                "--password",
                "test123",
            ])
            .output()?;

        Ok(output)
    }

    /// Test basic wallet commands that should work in batch mode
    #[test]
    fn test_basic_wallet_commands() -> Result<()> {
        let wallet_name = create_unique_wallet_name("basic");

        // Test display_address command
        let output = run_wallet_command("display_address", &wallet_name)?;

        println!(
            "display_address output: {}",
            String::from_utf8_lossy(&output.stdout)
        );

        if !output.status.success() {
            println!(
                "display_address stderr: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        // The command should succeed (create wallet and show address)
        assert!(output.status.success(), "display_address command failed");

        // Output should contain a wallet address
        let stdout_str = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout_str.contains("tos1"),
            "Output should contain wallet address"
        );

        Ok(())
    }

    /// Test help command
    #[test]
    fn test_help_command() -> Result<()> {
        let output = Command::new(get_wallet_binary_path())
            .args(&["--help"])
            .output()?;

        assert!(output.status.success(), "help command failed");

        let stdout_str = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout_str.contains("Usage"),
            "Help output should contain usage information"
        );

        Ok(())
    }

    /// Test version command
    #[test]
    fn test_version_command() -> Result<()> {
        let output = Command::new(get_wallet_binary_path())
            .args(&["--version"])
            .output()?;

        assert!(output.status.success(), "version command failed");

        let stdout_str = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout_str.contains("tos_wallet"),
            "Version output should contain program name"
        );

        Ok(())
    }

    /// Test that binary exists and is executable
    #[test]
    fn test_binary_exists() {
        use std::path::Path;

        // Check for release binary first (CI runs with --release), then debug
        let binary_path = if Path::new("../target/release/tos_wallet").exists() {
            Path::new("../target/release/tos_wallet")
        } else {
            Path::new("../target/debug/tos_wallet")
        };

        assert!(
            binary_path.exists(),
            "Wallet binary should exist at {:?}",
            binary_path
        );

        // Try to get help output to verify it's executable
        let output = Command::new(binary_path).args(&["--help"]).output();

        assert!(output.is_ok(), "Wallet binary should be executable");
    }
}
