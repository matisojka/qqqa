use anyhow::{Context, Result, anyhow};
use std::io::Write;
use std::process::{Command, Stdio};

type CommandSpec = &'static [&'static str];

#[cfg(target_os = "macos")]
const CLIPBOARD_COMMANDS: &[CommandSpec] = &[&["pbcopy"]];

#[cfg(target_os = "linux")]
const CLIPBOARD_COMMANDS: &[CommandSpec] = &[&["wl-copy"], &["xclip", "-selection", "clipboard"]];

#[cfg(target_os = "windows")]
const CLIPBOARD_COMMANDS: &[CommandSpec] = &[&["clip"]];

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
const CLIPBOARD_COMMANDS: &[CommandSpec] = &[
    &["pbcopy"],
    &["wl-copy"],
    &["xclip", "-selection", "clipboard"],
    &["clip"],
];

/// Copy text into the user's clipboard using best-effort native utilities.
pub fn copy_to_clipboard(text: &str) -> Result<()> {
    let mut last_error = None;
    for spec in CLIPBOARD_COMMANDS {
        match try_copy_with(spec, text) {
            Ok(()) => return Ok(()),
            Err(err) => last_error = Some(err),
        }
    }

    Err(last_error.unwrap_or_else(|| {
        anyhow!("No supported clipboard utility found (looked for pbcopy, wl-copy, xclip, clip)")
    }))
}

fn try_copy_with(spec: CommandSpec, text: &str) -> Result<()> {
    let (program, args) = spec
        .split_first()
        .ok_or_else(|| anyhow!("Empty command spec"))?;
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().with_context(|| {
        format!(
            "Failed to spawn clipboard helper '{}'. Make sure it is installed and on PATH.",
            program
        )
    })?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .with_context(|| format!("Failed to write to '{}' stdin", program))?;
    }
    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!(
            "Clipboard helper '{}' exited with code {:?}",
            program,
            status.code()
        ))
    }
}
