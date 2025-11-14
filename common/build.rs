// This file is executed before the build and fetch the commit hash from git
// we create the build version and set it as an environment variable for the build.

use std::process::Command;

fn main() {
    let commit_hash = if let Some(hash) = option_env!("TOS_COMMIT_HASH") {
        hash[0..7].to_string()
    } else {
        // Run git command to get the commit hash
        // SAFETY: Build script - failure defaults to "unknown" instead of panicking
        let output = match Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .output()
        {
            Ok(output) if output.status.success() => {
                String::from_utf8_lossy(&output.stdout).trim().to_string()
            }
            _ => {
                // Git command failed or not available - use fallback
                "unknown".to_string()
            }
        };

        output
    };

    // Set the result as an environment variable for the build
    let build_version = format!("{}-{}", env!("CARGO_PKG_VERSION"), commit_hash);
    println!("cargo:rerun-if-env-changed=BUILD_VERSION");
    println!("cargo:BUILD_VERSION={build_version}");
    println!("cargo:rustc-env=BUILD_VERSION={build_version}");
}
