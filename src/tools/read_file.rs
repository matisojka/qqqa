use crate::perms::{ensure_safe_path, READ_FILE_MAX_BYTES};
use anyhow::{anyhow, Context, Result};
use fs_err as fs;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct Args {
    pub path: String,
}

pub fn run(args: Args) -> Result<String> {
    let path = PathBuf::from(&args.path);
    ensure_safe_path(&path)?;
    let meta = fs::metadata(&path).with_context(|| format!("Reading metadata: {}", path.display()))?;
    if meta.len() as usize > READ_FILE_MAX_BYTES {
        return Err(anyhow!("File too large (>{} bytes): {}", READ_FILE_MAX_BYTES, path.display()));
    }
    let content = fs::read_to_string(&path)
        .with_context(|| format!("Reading file: {}", path.display()))?;
    Ok(content)
}

