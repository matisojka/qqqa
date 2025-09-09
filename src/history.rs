use fs_err as fs;
use std::cmp::min;

/// Try to read the last `limit` commands from common shell history files.
/// This function is best-effort and will gracefully return an empty list if nothing is found.
pub fn read_recent_history(limit: usize, debug: bool) -> Vec<String> {
    let home = match dirs::home_dir() {
        Some(p) => p,
        None => return Vec::new(),
    };

    // Priority order per spec
    let candidates = [
        home.join(".zsh_history"),
        home.join(".bash_history"),
        home.join(".local/share/fish/fish_history"),
    ];

    for path in candidates.iter() {
        if !path.exists() {
            continue;
        }
        if debug {
            eprintln!("[debug] Reading history from {}", path.display());
        }
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let mut lines: Vec<String> = Vec::new();
        if path.ends_with("fish_history") {
            // fish format: lines like "- cmd: echo hi"
            for line in content.lines() {
                if let Some(rest) = line.strip_prefix("- cmd: ") {
                    lines.push(rest.trim().to_string());
                }
            }
        } else if path.ends_with(".zsh_history") {
            for line in content.lines() {
                // zsh extended history format: ": 1696971992:0;command here"
                if let Some(semi_idx) = line.rfind(';') {
                    let cmd = &line[semi_idx + 1..];
                    if !cmd.trim().is_empty() {
                        lines.push(cmd.trim().to_string());
                    }
                } else if !line.trim().is_empty() {
                    lines.push(line.trim().to_string());
                }
            }
        } else {
            // bash or plain text
            for line in content.lines() {
                if !line.trim().is_empty() {
                    lines.push(line.trim().to_string());
                }
            }
        }

        if !lines.is_empty() {
            let n = lines.len();
            let start = n.saturating_sub(limit);
            return lines[start..min(n, start + limit)].to_vec();
        }
    }
    Vec::new()
}

