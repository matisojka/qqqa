#![cfg(unix)]

use qqqa::ai::{CliCompletionRequest, run_cli_completion};
use qqqa::config::CliEngine;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use tempfile::tempdir;

#[tokio::test]
async fn run_cli_completion_returns_agent_message_from_script() {
    let dir = tempdir().unwrap();
    let script_path = dir.path().join("fake_codex");
    let script = r#"#!/bin/sh
printf '%s\n' '{"type":"item.completed","item":{"type":"reasoning","text":"thinking"}}'
printf '%s\n' '{"type":"item.completed","item":{"type":"agent_message","text":"hello"}}'
"#;
    fs::write(&script_path, script).unwrap();
    let mut perms = fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script_path, perms).unwrap();

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
printf '%s\n' "$@" > "{args}"
cat > "{prompt}"
printf '%s\n' '{{"type":"item.completed","item":{{"type":"agent_message","text":"ok"}}}}'
"#,
        args = args_dump.display(),
        prompt = prompt_dump.display()
    );
    fs::write(&script_path, script).unwrap();
    let mut perms = fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script_path, perms).unwrap();

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

    let args_contents = fs::read_to_string(&args_dump).expect("args file");
    let args: Vec<&str> = args_contents.lines().collect();
    assert_eq!(
        args,
        vec![
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
    );

    let prompt_contents = fs::read_to_string(&prompt_dump).expect("prompt file");
    let expected_prompt = "<system-prompt>\nSYSTEM\n</system-prompt>\n\n<user-prompt>\nUSER\n</user-prompt>\n";
    assert_eq!(prompt_contents, expected_prompt);
}
