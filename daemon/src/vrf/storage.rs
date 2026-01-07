use crate::vrf::WrappedVrfSecret;
use anyhow::{anyhow, Context, Result};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

const VRF_SECRET_FILE_PERMS: u32 = 0o600;

pub fn read_vrf_secret_from_file(path: &Path) -> Result<WrappedVrfSecret> {
    let metadata = fs::metadata(path)
        .with_context(|| format!("Failed to read VRF key file metadata: {}", path.display()))?;

    #[cfg(unix)]
    {
        let mode = metadata.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(anyhow!(
                "VRF key file permissions too open (expected 0600, got {:o})",
                mode
            ));
        }
    }

    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read VRF key file: {}", path.display()))?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("VRF key file is empty: {}", path.display()));
    }

    let secret = trimmed
        .parse::<WrappedVrfSecret>()
        .map_err(|e| anyhow!("Invalid VRF secret key in {}: {}", path.display(), e))?;
    Ok(secret)
}

pub fn write_vrf_secret_to_file(path: &Path, secret: &WrappedVrfSecret) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create VRF key file parent directory: {}",
                parent.display()
            )
        })?;
    }

    let mut options = OpenOptions::new();
    options.create(true).write(true).truncate(true);

    #[cfg(unix)]
    {
        options.mode(VRF_SECRET_FILE_PERMS);
    }

    let mut file = options
        .open(path)
        .with_context(|| format!("Failed to open VRF key file: {}", path.display()))?;

    #[cfg(unix)]
    {
        fs::set_permissions(path, fs::Permissions::from_mode(VRF_SECRET_FILE_PERMS)).with_context(
            || format!("Failed to set VRF key file permissions: {}", path.display()),
        )?;
    }

    let hex = secret.to_hex();
    file.write_all(hex.as_bytes())
        .with_context(|| format!("Failed to write VRF key file: {}", path.display()))?;
    file.write_all(b"\n")
        .with_context(|| format!("Failed to finalize VRF key file: {}", path.display()))?;
    Ok(())
}
