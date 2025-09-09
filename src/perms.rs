use anyhow::{anyhow, Result};
use fs_err as fs;
use std::path::{Path, PathBuf};

/// Cap read_file size to 1 MiB
pub const READ_FILE_MAX_BYTES: usize = 1 * 1024 * 1024;

/// Ensure a path is within HOME or current working directory.
pub fn ensure_safe_path(p: &Path) -> Result<()> {
    let abs = canonicalize_best_effort(p)?;
    let home_ok = dirs::home_dir().map(|h| abs.starts_with(h)).unwrap_or(false);
    let cwd_ok = std::env::current_dir().map(|c| abs.starts_with(c)).unwrap_or(false);
    if home_ok || cwd_ok {
        Ok(())
    } else {
        Err(anyhow!("Refusing to access path outside home or cwd: {}", abs.display()))
    }
}

fn canonicalize_best_effort(p: &Path) -> Result<PathBuf> {
    // Try to canonicalize; if the file doesn't exist yet (write), fallback to joining with cwd.
    match fs::canonicalize(p) {
        Ok(c) => Ok(c),
        Err(_) => {
            let base = if p.is_absolute() {
                PathBuf::from("/")
            } else {
                std::env::current_dir()?
            };
            Ok(base.join(p))
        }
    }
}

/// Very conservative safety checks for shell commands.
pub fn ensure_safe_command(cmd: &str) -> Result<()> {
    let lower = cmd.to_lowercase();
    // Block obviously destructive global wipes
    if lower.contains("rm -rf /") || lower.contains("rm -rf -- /") {
        return Err(anyhow!("Blocked dangerous command pattern: rm -rf /"));
    }
    // Discourage sudo without user interaction context
    if lower.contains("sudo ") {
        return Err(anyhow!("Blocked command using sudo (not allowed by default)"));
    }
    // Block direct disk wiping
    if lower.contains("mkfs") || lower.contains("dd if=") {
        return Err(anyhow!("Blocked potentially destructive disk operation"));
    }
    Ok(())
}

