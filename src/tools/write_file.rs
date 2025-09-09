use crate::perms::ensure_safe_path;
use anyhow::{Context, Result};
use fs_err as fs;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct Args {
    pub path: String,
    pub content: String,
}

pub fn run(args: Args) -> Result<String> {
    let path = PathBuf::from(&args.path);
    ensure_safe_path(&path)?;
    if let Some(parent) = path.parent() { fs::create_dir_all(parent).ok(); }
    fs::write(&path, args.content.as_bytes())
        .with_context(|| format!("Writing file: {}", path.display()))?;
    Ok(format!("Wrote {} ({} bytes)", path.display(), args.content.len()))
}

