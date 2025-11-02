#[cfg(test)]
mod tests {
    use anyhow::Result;
    use std::path::Path;
    use std::process::Command;

    /// Test wallet batch mode functionality using Python script
    #[test]
    fn test_wallet_batch_mode_with_python() -> Result<()> {
        let script_path = Path::new("tests/run_all_tests.py");
        if !script_path.exists() {
            println!("Python test runner not found at {:?}", script_path);
            println!("Skipping Python integration test");
            return Ok(());
        }

        let output = Command::new("python3")
            .arg(script_path)
            .current_dir(".")
            .output()?;

        println!(
            "Python test runner stdout: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        println!(
            "Python test runner stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        assert!(output.status.success(), "Python test runner failed");

        Ok(())
    }

    /// Test individual wallet commands
    #[test]
    fn test_display_address_command() -> Result<()> {
        let script_path = Path::new("tests/test_display_address.py");
        if !script_path.exists() {
            println!("Display address test script not found at {:?}", script_path);
            println!("Skipping display address test");
            return Ok(());
        }

        let output = Command::new("python3")
            .arg(script_path)
            .current_dir(".")
            .output()?;

        println!(
            "Display address test stdout: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        println!(
            "Display address test stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        assert!(output.status.success(), "Display address test failed");

        Ok(())
    }

    #[test]
    fn test_list_commands() -> Result<()> {
        let script_path = Path::new("tests/test_list_commands.py");
        if !script_path.exists() {
            println!("List commands test script not found at {:?}", script_path);
            println!("Skipping list commands test");
            return Ok(());
        }

        let output = Command::new("python3")
            .arg(script_path)
            .current_dir(".")
            .output()?;

        println!(
            "List commands test stdout: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        println!(
            "List commands test stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        assert!(output.status.success(), "List commands test failed");

        Ok(())
    }

    #[test]
    fn test_balance_commands() -> Result<()> {
        let script_path = Path::new("tests/test_balance_commands.py");
        if !script_path.exists() {
            println!(
                "Balance commands test script not found at {:?}",
                script_path
            );
            println!("Skipping balance commands test");
            return Ok(());
        }

        let output = Command::new("python3")
            .arg(script_path)
            .current_dir(".")
            .output()?;

        println!(
            "Balance commands test stdout: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        println!(
            "Balance commands test stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        assert!(output.status.success(), "Balance commands test failed");

        Ok(())
    }

    #[test]
    fn test_energy_commands() -> Result<()> {
        let script_path = Path::new("tests/test_energy_commands.py");
        if !script_path.exists() {
            println!("Energy commands test script not found at {:?}", script_path);
            println!("Skipping energy commands test");
            return Ok(());
        }

        let output = Command::new("python3")
            .arg(script_path)
            .current_dir(".")
            .output()?;

        println!(
            "Energy commands test stdout: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        println!(
            "Energy commands test stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        assert!(output.status.success(), "Energy commands test failed");

        Ok(())
    }

    #[test]
    fn test_transaction_commands() -> Result<()> {
        let script_path = Path::new("tests/test_transaction_commands.py");
        if !script_path.exists() {
            println!(
                "Transaction commands test script not found at {:?}",
                script_path
            );
            println!("Skipping transaction commands test");
            return Ok(());
        }

        let output = Command::new("python3")
            .arg(script_path)
            .current_dir(".")
            .output()?;

        println!(
            "Transaction commands test stdout: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        println!(
            "Transaction commands test stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        assert!(output.status.success(), "Transaction commands test failed");

        Ok(())
    }

    #[test]
    fn test_utility_commands() -> Result<()> {
        let script_path = Path::new("tests/test_utility_commands.py");
        if !script_path.exists() {
            println!(
                "Utility commands test script not found at {:?}",
                script_path
            );
            println!("Skipping utility commands test");
            return Ok(());
        }

        let output = Command::new("python3")
            .arg(script_path)
            .current_dir(".")
            .output()?;

        println!(
            "Utility commands test stdout: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        println!(
            "Utility commands test stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        assert!(output.status.success(), "Utility commands test failed");

        Ok(())
    }
}
