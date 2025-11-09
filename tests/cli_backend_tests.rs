#![cfg(unix)]

use qqqa::ai::{CliCompletionRequest, run_cli_completion};
use qqqa::config::CliEngine;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use tempfile::tempdir;

fn read_args(path: &Path) -> Vec<String> {
    let data = fs::read(path).expect("args file");
    data.split(|b| *b == 0)
        .filter(|chunk| !chunk.is_empty())
        .map(|chunk| String::from_utf8_lossy(chunk).to_string())
        .collect()
}

fn write_executable_script(path: &Path, contents: &str) {
    use std::io::Write;

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .expect("create script");
    file.write_all(contents.as_bytes()).expect("write script");
    file.sync_all().ok();
    drop(file);

    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

#[tokio::test]
async fn run_cli_completion_returns_agent_message_from_script() {
    let dir = tempdir().unwrap();
    let script_path = dir.path().join("fake_codex");
    let script = r#"#!/bin/sh
printf '%s\n' '{"type":"item.completed","item":{"type":"reasoning","text":"thinking"}}'
printf '%s\n' '{"type":"item.completed","item":{"type":"agent_message","text":"hello"}}'
"#;
    write_executable_script(&script_path, script);

    let text = run_cli_completion(CliCompletionRequest {
        engine: CliEngine::Codex,
        binary: script_path.to_str().unwrap(),
        base_args: &[],
        system_prompt: "SYSTEM",
        user_prompt: "USER",
        model: "gpt-5",
        reasoning_effort: Some("minimal"),
        debug: true,
    })
    .await
    .expect("cli run succeeds");

    assert_eq!(text, "hello");
}

#[tokio::test]
async fn run_cli_completion_writes_tagged_prompts_to_stdin() {
    let dir = tempdir().unwrap();
    let script_path = dir.path().join("fake_codex");
    let args_dump = dir.path().join("args.txt");
    let prompt_dump = dir.path().join("prompt.txt");
    let script = format!(
        r#"#!/bin/sh
set -eu
DIR="$(dirname "$0")"
printf '%s\0' "$@" > "{args}"
cat > "{prompt}"
printf '%s\n' '{{"type":"item.completed","item":{{"type":"agent_message","text":"ok"}}}}'
"#,
        args = args_dump.display(),
        prompt = prompt_dump.display()
    );
    write_executable_script(&script_path, &script);

    let base_args = vec!["exec".to_string()];
    let text = run_cli_completion(CliCompletionRequest {
        engine: CliEngine::Codex,
        binary: script_path.to_str().unwrap(),
        base_args: &base_args,
        system_prompt: "SYSTEM",
        user_prompt: "USER",
        model: "gpt-5",
        reasoning_effort: Some("minimal"),
        debug: false,
    })
    .await
    .expect("cli run succeeds");

    assert_eq!(text.trim(), "ok");

    let args = read_args(&args_dump);
    let expected: Vec<String> = vec![
        "exec",
        "--json",
        "-c",
        "model_reasoning_effort=minimal",
        "-c",
        "sandbox_mode=read-only",
        "-c",
        "tools.web_search=false",
        "-",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    assert_eq!(args, expected);

    let prompt_contents = fs::read_to_string(&prompt_dump).expect("prompt file");
    let expected_prompt =
        "<system-prompt>\nSYSTEM\n</system-prompt>\n\n<user-prompt>\nUSER\n</user-prompt>\n";
    assert_eq!(prompt_contents, expected_prompt);
}

#[tokio::test]
async fn run_cli_completion_invokes_claude_with_expected_args() {
    let dir = tempdir().unwrap();
    let script_path = dir.path().join("fake_claude");
    let args_dump = dir.path().join("claude_args.txt");
    let script = format!(
        r#"#!/bin/sh
set -eu
printf '%s\0' "$@" > "{args}"
printf '%s' '{{"type":"result","subtype":"success","result":"<cmd>echo hi</cmd>"}}'
"#,
        args = args_dump.display()
    );
    write_executable_script(&script_path, &script);

    let text = run_cli_completion(CliCompletionRequest {
        engine: CliEngine::Claude,
        binary: script_path.to_str().unwrap(),
        base_args: &[],
        system_prompt: "SYSTEM",
        user_prompt: "USER",
        model: "sonnet",
        reasoning_effort: None,
        debug: false,
    })
    .await
    .expect("cli run succeeds");

    assert_eq!(text, "<cmd>echo hi</cmd>");

    let args = read_args(&args_dump);
    let expected: Vec<String> = vec![
        "-p",
        "--output-format",
        "json",
        "--model",
        "sonnet",
        "--append-system-prompt",
        "<system-prompt>\nSYSTEM\n</system-prompt>",
        "--disallowed-tools",
        "Bash(*) Edit",
        "--",
        "<user-prompt>\nUSER\n</user-prompt>",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    assert_eq!(args, expected);
}
