use crate::perms::ensure_safe_command;
use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::path::PathBuf;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

#[derive(Debug, Deserialize)]
pub struct Args {
    pub command: String,
    #[serde(default)]
    pub cwd: Option<String>,
}

pub async fn run(args: Args, auto_yes: bool, debug: bool) -> Result<String> {
    ensure_safe_command(&args.command)?;

    let cwd = args.cwd.as_deref().unwrap_or(".");
    eprintln!("Proposed command: {}", &args.command);
    eprintln!("Working directory: {}", cwd);
    // Additional confirmation if cwd is outside the user's home directory
    if let Some(home) = dirs::home_dir() {
        use std::path::Path;
        let cwd_path = Path::new(cwd);
        // Best effort: canonicalize if possible, else use provided path
        let resolved = std::fs::canonicalize(cwd_path).unwrap_or_else(|_| cwd_path.to_path_buf());
        if !resolved.starts_with(&home) {
            eprintln!(
                "Warning: working directory is outside your home: {}",
                resolved.display()
            );
            if !auto_yes {
                eprint!("Proceed anyway? [y/N]: ");
                use std::io::Write;
                std::io::stdout().flush().ok();
                let mut line = String::new();
                std::io::stdin().read_line(&mut line).ok();
                let l = line.trim().to_lowercase();
                if !(l == "y" || l == "yes") {
                    return Err(anyhow!("Execution canceled by user (cwd outside home)"));
                }
            } else if debug {
                eprintln!("[debug] Proceeding due to --yes flag");
            }
        }
    }
    if !auto_yes {
        eprint!("Execute? [y/N]: ");
        use std::io::Write;
        std::io::stdout().flush().ok();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).ok();
        let l = line.trim().to_lowercase();
        if !(l == "y" || l == "yes") {
            return Err(anyhow!("Execution canceled by user"));
        }
    }

    // Build shell execution: run via sh -lc to allow pipes and quotes.
    let mut cmd = Command::new("sh");
    cmd.arg("-lc").arg(&args.command).current_dir(PathBuf::from(cwd));
    cmd.kill_on_drop(true);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let child = cmd.spawn().with_context(|| "Failed to spawn command")?;
    let run = async {
        let out = child.wait_with_output().await?;
        Result::<std::process::Output>::Ok(out)
    };

    // Apply timeout for robustness.
    let out = match timeout(Duration::from_secs(120), run).await {
        Ok(res) => res?,
        Err(_) => return Err(anyhow!("Command timed out after 120s")),
    };

    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let code = out.status.code().unwrap_or(-1);
    if debug {
        eprintln!("[debug] exit code: {} (stdout {} bytes, stderr {} bytes)", code, stdout.len(), stderr.len());
    }

    // Return a structured plain-text summary including stdout/stderr.
    let mut summary = String::new();
    summary.push_str(&format!("Exit code: {}\n", code));
    summary.push_str("--- stdout ---\n");
    summary.push_str(&stdout);
    if !stdout.ends_with('\n') { summary.push('\n'); }
    summary.push_str("--- stderr ---\n");
    summary.push_str(&stderr);
    if !stderr.ends_with('\n') { summary.push('\n'); }
    Ok(summary)
}
