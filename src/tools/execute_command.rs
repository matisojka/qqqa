use crate::perms::{CommandDisposition, ensure_safe_command};
use anyhow::{Context, Result, anyhow};
use atty::Stream;
use serde::Deserialize;
use std::io::{self, Write};
use std::path::PathBuf;
use tokio::process::Command;
use tokio::time::{Duration, timeout};

#[derive(Debug, Deserialize, Clone)]
pub struct Args {
    pub command: String,
    #[serde(default)]
    pub cwd: Option<String>,
}

pub async fn run(args: Args, auto_yes: bool, debug: bool) -> Result<String> {
    let cwd = args.cwd.as_deref().unwrap_or(".");
    eprintln!("Proposed command: {}", &args.command);
    eprintln!("Working directory: {}", cwd);

    let disposition = match ensure_safe_command(&args.command) {
        Ok(d) => d,
        Err(err) => {
            eprintln!("[blocked] {}", err);
            return Err(err);
        }
    };
    let mut requires_manual_confirmation = false;
    if let CommandDisposition::NeedsConfirmation { reason } = &disposition {
        requires_manual_confirmation = true;
        eprintln!("[warn] {}", reason);
    }
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
                if !prompt_yes_no("Proceed anyway? [y/N]: ")? {
                    return Err(anyhow!("Execution canceled by user (cwd outside home)"));
                }
            } else if debug {
                eprintln!("[debug] Proceeding due to --yes flag");
            }
        }
    }
    if requires_manual_confirmation || !auto_yes {
        if !prompt_yes_no("Execute? [y/N]: ")? {
            return Err(anyhow!("Execution canceled by user"));
        }
    }

    let child = spawn_child(&args.command, cwd, debug)?;
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
        eprintln!(
            "[debug] exit code: {} (stdout {} bytes, stderr {} bytes)",
            code,
            stdout.len(),
            stderr.len()
        );
    }

    // Return a structured plain-text summary including stdout/stderr.
    let mut summary = String::new();
    summary.push_str(&format!("Exit code: {}\n", code));
    summary.push_str("--- stdout ---\n");
    summary.push_str(&stdout);
    if !stdout.ends_with('\n') {
        summary.push('\n');
    }
    summary.push_str("--- stderr ---\n");
    summary.push_str(&stderr);
    if !stderr.ends_with('\n') {
        summary.push('\n');
    }
    Ok(summary)
}

fn spawn_child(command: &str, cwd: &str, debug: bool) -> Result<tokio::process::Child> {
    #[cfg(unix)]
    {
        let force_script = matches!(
            std::env::var("QQQA_FORCE_PTY")
                .ok()
                .map(|v| v.to_lowercase()),
            Some(ref v) if v == "1" || v == "true" || v == "yes"
        );
        let disable_script = matches!(
            std::env::var("QQQA_DISABLE_PTY")
                .ok()
                .map(|v| v.to_lowercase()),
            Some(ref v) if v == "1" || v == "true" || v == "yes"
        );
        let use_script = force_script || (!disable_script && atty::is(Stream::Stdout));
        if use_script {
            match spawn_script_child(command, cwd) {
                Ok(child) => return Ok(child),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    if debug {
                        eprintln!("[debug] 'script' not found; falling back to plain sh");
                    }
                }
                Err(err) => {
                    return Err(anyhow!(err).context("Failed to spawn command via script"));
                }
            }
        }
    }

    spawn_shell_child(command, cwd).with_context(|| "Failed to spawn command")
}

fn configure_child(cmd: &mut Command, cwd: &str) {
    cmd.current_dir(PathBuf::from(cwd));
    cmd.kill_on_drop(true);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
}

fn spawn_shell_child(command: &str, cwd: &str) -> std::io::Result<tokio::process::Child> {
    let mut cmd = Command::new("sh");
    cmd.arg("-lc").arg(command);
    configure_child(&mut cmd, cwd);
    cmd.spawn()
}

#[cfg(unix)]
fn spawn_script_child(command: &str, cwd: &str) -> std::io::Result<tokio::process::Child> {
    let mut cmd = Command::new("script");
    cmd.arg("-q")
        .arg("/dev/null")
        .arg("sh")
        .arg("-lc")
        .arg(command);
    configure_child(&mut cmd, cwd);
    cmd.spawn()
}

fn prompt_yes_no(prompt: &str) -> Result<bool> {
    eprint!("{}", prompt);
    io::stderr().flush().ok();

    if atty::is(Stream::Stdin) {
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        return Ok(is_yes(&line));
    }

    #[cfg(unix)]
    {
        use std::fs::OpenOptions;
        use std::io::{BufRead, BufReader};

        let file = OpenOptions::new()
            .read(true)
            .open("/dev/tty")
            .context("Failed to open /dev/tty for confirmation; re-run with --yes")?;
        let mut reader = BufReader::new(file);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .context("Failed to read confirmation from /dev/tty")?;
        return Ok(is_yes(&line));
    }

    #[cfg(windows)]
    {
        use std::fs::OpenOptions;
        use std::io::{BufRead, BufReader};

        let file = OpenOptions::new()
            .read(true)
            .open("CONIN$")
            .context("Failed to open CONIN$ for confirmation; re-run with --yes")?;
        let mut reader = BufReader::new(file);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .context("Failed to read confirmation from CONIN$")?;
        return Ok(is_yes(&line));
    }

    #[allow(unreachable_code)]
    Err(anyhow!(
        "Unable to read confirmation from TTY; pass --yes to skip prompts"
    ))
}

fn is_yes(input: &str) -> bool {
    matches!(input.trim().to_lowercase().as_str(), "y" | "yes")
}
