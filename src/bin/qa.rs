use anyhow::{Context, Result, anyhow};
use clap::{ArgAction, Parser};
use qqqa::ai::{AssistantReply, ChatClient, Msg};
use qqqa::config::{Config, InitExistsError};
use qqqa::history::read_recent_history;
use qqqa::perms;
use qqqa::prompt::{build_qa_system_prompt, build_qa_user_message, coalesce_prompt_inputs};
use qqqa::tools::{ToolCall, parse_tool_call};
use std::io::{Read, Stdin, Write};
use std::path::{Path, PathBuf};

/// qa — single-step agent that may use one tool
#[derive(Debug, Parser)]
#[command(name = "qa", disable_colored_help = false, version, about)]
struct Cli {
    /// Initialize or reinitialize configuration (~/.qq/config.json) and exit
    #[arg(long = "init", action = ArgAction::SetTrue)]
    init: bool,
    /// Profile name from config to use
    #[arg(short = 'p', long = "profile")]
    profile: Option<String>,

    /// Override the resolved provider base URL (e.g., http://localhost:11434/v1)
    #[arg(long = "api-base")]
    api_base: Option<String>,

    /// Override the model ID from profile
    #[arg(short = 'm', long = "model")]
    model: Option<String>,

    /// Disable terminal history context
    #[arg(short = 'n', long = "no-history", action = ArgAction::SetTrue)]
    no_history: bool,
    /// Include terminal history context even if disabled in config
    #[arg(long = "history", action = ArgAction::SetTrue, conflicts_with = "no_history")]
    history: bool,

    /// Verbose internal logs
    #[arg(short = 'd', long = "debug", action = ArgAction::SetTrue)]
    debug: bool,

    /// Auto-approve command execution (use with caution)
    #[arg(short = 'y', long = "yes", action = ArgAction::SetTrue)]
    yes: bool,

    /// Disable emojis going forward (persists to config)
    #[arg(long = "no-fun", action = ArgAction::SetTrue)]
    no_fun: bool,

    /// The task to perform. If omitted, stdin must be piped.
    #[arg(trailing_var_arg = true)]
    task: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.init {
        match Config::init_interactive(cli.debug) {
            Ok(path) => {
                if cli.debug {
                    eprintln!("[debug] Initialized config at {}", path.display());
                }
            }
            Err(e) => match e.downcast::<InitExistsError>() {
                Ok(init_err) => {
                    println!(
                        "Keeping your existing config at {}.\nMove or remove that file if you want to rerun --init.",
                        init_err.path.display()
                    );
                }
                Err(e) => return Err(e),
            },
        }
        return Ok(());
    }

    // Handle stdin piping for extra context.
    let stdin_is_tty = atty::is(atty::Stream::Stdin);
    let stdin_block = if !stdin_is_tty {
        Some(read_all_stdin(std::io::stdin())?)
    } else {
        None
    };

    let prepared = coalesce_prompt_inputs(cli.task.join(" "), stdin_block);
    if prepared.question.trim().is_empty() {
        return Err(anyhow!("No input provided. Pass a task or pipe stdin."));
    }
    let qqqa::prompt::PromptInputs {
        question: task,
        stdin_block,
    } = prepared;

    let (mut cfg, path) = Config::load_or_init(cli.debug)?;
    perms::set_custom_allowlist(cfg.command_allowlist());
    if cli.no_fun {
        cfg.no_emoji = Some("true".to_string());
        cfg.save(&path, cli.debug)?;
        if cli.debug {
            eprintln!(
                "[debug] Disabled emojis in system prompt (persisted at {}).",
                path.display()
            );
        }
    }
    let mut eff = match cfg.resolve_profile(cli.profile.as_deref(), cli.model.as_deref()) {
        Ok(eff) => eff,
        Err(e) => {
            let msg = e.to_string();
            let mut out = msg.clone();
            if msg.contains("Missing API key") {
                out.push_str(
                    "\n\nFix it quickly:\n- Run `qa --init` and choose provider; optionally paste the API key.\n- Or export an env var, e.g.\n    export OPENROUTER_API_KEY=... # OpenRouter (default)\n    export GROQ_API_KEY=...       # Groq\n    export OPENAI_API_KEY=...     # OpenAI\n    export ANTHROPIC_API_KEY=...  # Anthropic (Claude)",
                );
            }
            return Err(anyhow!(out));
        }
    };
    if let Some(base) = cli.api_base.as_deref() {
        eff.base_url = base.to_string();
    }
    if cli.debug {
        eprintln!(
            "[debug] Using provider='{}' base_url='{}' model='{}'",
            eff.provider_key, eff.base_url, eff.model
        );
    }

    let include_history = if cli.no_history {
        false
    } else if cli.history {
        true
    } else {
        cfg.history_enabled()
    };
    let history = if include_history {
        read_recent_history(10, cli.debug)
    } else {
        Vec::new()
    };
    let mut system_prompt = build_qa_system_prompt();
    if cfg.no_emoji_enabled() {
        system_prompt.push_str("\nHard rule: You MUST NOT use emojis anywhere in the response.\n");
    }
    let user_msg = build_qa_user_message(
        Some(os_info::get().os_type()),
        &history,
        stdin_block.as_deref(),
        &task,
    );

    let client = ChatClient::new(
        eff.base_url.clone(),
        eff.api_key.clone(),
        eff.headers.clone(),
        eff.request_timeout_secs,
    )?
    .with_reasoning_effort(eff.reasoning_effort.clone());
    // Provide tool specs so the API can emit structured tool_calls instead of erroring.
    let tools_spec = serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read a UTF-8 text file up to a safe size.",
                "parameters": {
                    "type": "object",
                    "properties": {"path": {"type": "string"}},
                    "required": ["path"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "json",
                "description": r#"Wrap another tool call as { "tool": string, "arguments": object }."#,
                "parameters": {
                    "type": "object",
                    "properties": {
                        "tool": {"type": "string"},
                        "arguments": {"type": "object"}
                    },
                    "required": ["tool", "arguments"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "write_file",
                "description": "Create or overwrite a UTF-8 text file.",
                "parameters": {
                    "type": "object",
                    "properties": {"path": {"type": "string"}, "content": {"type": "string"}},
                    "required": ["path", "content"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "execute_command",
                "description": "Run a safe shell command in the workspace/home.",
                "parameters": {
                    "type": "object",
                    "properties": {"command": {"type": "string"}, "cwd": {"type": "string"}},
                    "required": ["command"]
                }
            }
        }
    ]);

    let assistant_reply = client
        .chat_once_messages_with_tools(
            &eff.model,
            &[
                Msg {
                    role: "system",
                    content: &system_prompt,
                },
                Msg {
                    role: "user",
                    content: &user_msg,
                },
            ],
            tools_spec,
            cli.debug,
        )
        .await?;

    match assistant_reply {
        AssistantReply::ToolCall {
            name,
            arguments_json,
        } => {
            let handled = execute_tool_call(
                name.as_str(),
                &arguments_json,
                cli.yes,
                cli.debug,
                &mut cfg,
                &path,
            )
            .await?;
            if !handled {
                println!("Unknown tool call: {}", name);
            }
        }
        AssistantReply::Content(assistant) => {
            // Try to parse as a tool call per our plain-JSON protocol; else print the answer.
            match parse_tool_call(assistant.trim()) {
                Ok(call) => match call {
                    ToolCall::ReadFile(args) => match qqqa::tools::read_file::run(args) {
                        Ok(content) => print_tool_result("read_file", &content),
                        Err(e) => print_tool_error("read_file", &e.to_string()),
                    },
                    ToolCall::WriteFile(args) => match qqqa::tools::write_file::run(args) {
                        Ok(summary) => print_tool_result("write_file", &summary),
                        Err(e) => print_tool_error("write_file", &e.to_string()),
                    },
                    ToolCall::ExecuteCommand(args) => match run_execute_command_with_allowlist(
                        args, cli.yes, cli.debug, &mut cfg, &path,
                    )
                    .await
                    {
                        Ok(summary) => print_tool_result("execute_command", &summary),
                        Err(e) => print_tool_error("execute_command", &e.to_string()),
                    },
                },
                Err(_) => {
                    println!("{}", assistant.trim_end());
                }
            }
        }
    }

    Ok(())
}

async fn run_execute_command_with_allowlist(
    args: qqqa::tools::execute_command::Args,
    auto_yes: bool,
    debug: bool,
    cfg: &mut Config,
    cfg_path: &Path,
) -> Result<String> {
    let mut base_dir = std::env::current_dir().context("Failed to read current directory")?;
    if let Ok(canon) = base_dir.canonicalize() {
        base_dir = canon;
    }

    let original_args = args;
    loop {
        let mut stream_printer = |chunk: qqqa::tools::execute_command::StreamChunk| match chunk.kind
        {
            qqqa::tools::execute_command::StreamKind::Stdout => {
                let mut handle = std::io::stdout();
                let _ = handle.write_all(chunk.data);
                let _ = handle.flush();
            }
            qqqa::tools::execute_command::StreamKind::Stderr => {
                let mut handle = std::io::stderr();
                let _ = handle.write_all(chunk.data);
                let _ = handle.flush();
            }
        };
        let exec_args = sanitize_execute_args(original_args.clone(), &base_dir, debug);
        match qqqa::tools::execute_command::run(
            exec_args,
            auto_yes,
            debug,
            Some(&mut stream_printer),
        )
        .await
        {
            Ok(summary) => return Ok(summary),
            Err(err) => {
                if let Some(program) = err
                    .downcast_ref::<perms::CommandNotAllowedError>()
                    .map(|e| e.program.clone())
                {
                    if !atty::is(atty::Stream::Stdin) {
                        return Err(err);
                    }
                    if prompt_add_command_to_allowlist(&program)? {
                        let inserted = cfg.add_command_to_allowlist(&program);
                        if inserted {
                            cfg.save(cfg_path, debug)?;
                        }
                        perms::set_custom_allowlist(cfg.command_allowlist());
                        if inserted {
                            if debug {
                                eprintln!("[debug] Added '{}' to qa command allowlist", program);
                            } else {
                                eprintln!("Added '{}' to qa command allowlist.", program);
                            }
                        }
                        continue;
                    } else {
                        return Err(err);
                    }
                } else {
                    return Err(err);
                }
            }
        }
    }
}

fn prompt_add_command_to_allowlist(program: &str) -> Result<bool> {
    use std::io::{self, Write};

    eprint!(
        "Command '{}' is not in the qa allowlist. Add it now and retry? [y/N]: ",
        program
    );
    io::stderr().flush().context("Failed to flush prompt")?;
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .context("Failed to read response")?;
    let choice = line.trim().to_ascii_lowercase();
    Ok(choice == "y" || choice == "yes")
}

fn sanitize_execute_args(
    args: qqqa::tools::execute_command::Args,
    base_dir: &Path,
    debug: bool,
) -> qqqa::tools::execute_command::Args {
    let (sanitized_path, fell_back) = sanitize_cwd_path(args.cwd.as_deref(), base_dir);
    let sanitized_str = sanitized_path.to_string_lossy().to_string();

    if fell_back && debug {
        if let Some(original) = args.cwd.as_deref() {
            eprintln!(
                "[debug] Ignoring requested working directory '{}' and using {} instead",
                original, sanitized_str
            );
        } else {
            eprintln!(
                "[debug] Using current working directory {} for execute_command",
                sanitized_str
            );
        }
    }

    qqqa::tools::execute_command::Args {
        command: args.command,
        cwd: Some(sanitized_str),
    }
}

fn sanitize_cwd_path(requested: Option<&str>, base_dir: &Path) -> (PathBuf, bool) {
    let base = base_dir.to_path_buf();
    let Some(raw) = requested else {
        return (base, false);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return (base, false);
    }
    let path = Path::new(trimmed);
    if path.is_absolute() {
        if let Ok(canon) = std::fs::canonicalize(path) {
            if canon.starts_with(base_dir) {
                return (canon, false);
            }
        }
        return (base, true);
    }

    let mut relative = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(seg) => relative.push(seg),
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                return (base, true);
            }
        }
    }
    if relative.as_os_str().is_empty() {
        return (base, false);
    }
    (base_dir.join(relative), false)
}

async fn execute_tool_call(
    name: &str,
    arguments_json: &str,
    auto_yes: bool,
    debug: bool,
    cfg: &mut Config,
    cfg_path: &Path,
) -> Result<bool> {
    let mut current_name = name.to_string();
    let mut current_args = arguments_json.to_string();

    loop {
        match current_name.as_str() {
            "read_file" => {
                let args: qqqa::tools::read_file::Args = serde_json::from_str(&current_args)
                    .map_err(|e| anyhow!("Failed to parse read_file args: {}", e))?;
                match qqqa::tools::read_file::run(args) {
                    Ok(content) => print_tool_result("read_file", &content),
                    Err(e) => print_tool_error("read_file", &e.to_string()),
                }
                return Ok(true);
            }
            "write_file" => {
                let args: qqqa::tools::write_file::Args = serde_json::from_str(&current_args)
                    .map_err(|e| anyhow!("Failed to parse write_file args: {}", e))?;
                match qqqa::tools::write_file::run(args) {
                    Ok(summary) => print_tool_result("write_file", &summary),
                    Err(e) => print_tool_error("write_file", &e.to_string()),
                }
                return Ok(true);
            }
            "execute_command" => {
                let args: qqqa::tools::execute_command::Args = serde_json::from_str(&current_args)
                    .map_err(|e| anyhow!("Failed to parse execute_command args: {}", e))?;
                match run_execute_command_with_allowlist(args, auto_yes, debug, cfg, cfg_path).await
                {
                    Ok(summary) => print_tool_result("execute_command", &summary),
                    Err(e) => print_tool_error("execute_command", &e.to_string()),
                }
                return Ok(true);
            }
            "json" => {
                let inner: serde_json::Value = serde_json::from_str(&current_args)
                    .map_err(|e| anyhow!("Failed to parse json wrapper: {}", e))?;
                let tool_name = inner
                    .get("tool")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Wrapped json tool missing 'tool' field"))?;
                let arguments = inner
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));
                current_name = tool_name.to_string();
                current_args = serde_json::to_string(&arguments)?;
                continue;
            }
            _ => return Ok(false),
        }
    }
}

fn print_tool_result(tool: &str, result: &str) {
    println!("[tool:{}]", tool);
    println!("{}", result.trim_end());
}

fn print_tool_error(tool: &str, err: &str) {
    println!("[tool:{}:error] {}", tool, err);
}

fn read_all_stdin(mut stdin: Stdin) -> Result<String> {
    let mut buf = String::new();
    stdin.read_to_string(&mut buf)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::tempdir;

    #[tokio::test]
    async fn execute_tool_call_handles_json_wrapper() {
        let dir = tempdir().unwrap();
        unsafe {
            std::env::set_var("HOME", dir.path());
        }
        std::env::set_current_dir(dir.path()).unwrap();

        let file_path = dir.path().join("hello.txt");
        std::fs::write(&file_path, "hi there").unwrap();

        let mut cfg = Config::default();
        let cfg_path = dir.path().join("config.json");

        let payload = serde_json::json!({
            "tool": "read_file",
            "arguments": { "path": "hello.txt" }
        });

        let result = execute_tool_call(
            "json",
            &payload.to_string(),
            false,
            false,
            &mut cfg,
            &cfg_path,
        )
        .await
        .expect("json wrapper should succeed");
        assert!(result, "json wrapper should dispatch an inner tool");
    }

    #[tokio::test]
    async fn execute_tool_call_handles_nested_json_wrapper() {
        let dir = tempdir().unwrap();
        unsafe {
            std::env::set_var("HOME", dir.path());
        }
        std::env::set_current_dir(dir.path()).unwrap();

        let file_path = dir.path().join("data.txt");
        std::fs::write(&file_path, "content").unwrap();

        let mut cfg = Config::default();
        let cfg_path = dir.path().join("config.json");

        let inner = serde_json::json!({
            "tool": "read_file",
            "arguments": { "path": "data.txt" }
        });
        let payload = serde_json::json!({
            "tool": "json",
            "arguments": inner
        });

        let result = execute_tool_call(
            "json",
            &payload.to_string(),
            false,
            false,
            &mut cfg,
            &cfg_path,
        )
        .await
        .expect("nested json wrapper should succeed");
        assert!(result, "nested json wrapper should dispatch an inner tool");
    }

    #[test]
    fn sanitize_cwd_allows_relative_subdir() {
        let dir = tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(base.join("sub/child")).unwrap();

        let args = qqqa::tools::execute_command::Args {
            command: "pwd".into(),
            cwd: Some("sub/child".into()),
        };

        let sanitized = sanitize_execute_args(args, &base, false);
        let cwd_path = Path::new(sanitized.cwd.as_ref().unwrap());
        assert_eq!(cwd_path, base.join("sub/child"));
    }

    #[test]
    fn sanitize_cwd_rejects_parent_dirs() {
        let dir = tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();

        let (result, fell_back) = sanitize_cwd_path(Some("../outside"), &base);
        assert_eq!(result, base);
        assert!(fell_back);
    }

    #[test]
    fn sanitize_cwd_rejects_absolute_outside() {
        let dir = tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_path = outside.path().canonicalize().unwrap();

        let (result, fell_back) =
            sanitize_cwd_path(Some(outside_path.to_string_lossy().as_ref()), &base);
        assert_eq!(result, base);
        assert!(fell_back);
    }

    #[test]
    fn sanitize_cwd_accepts_absolute_inside_base() {
        let dir = tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let nested = base.join("nested");
        std::fs::create_dir_all(&nested).unwrap();

        let (result, fell_back) = sanitize_cwd_path(Some(nested.to_string_lossy().as_ref()), &base);
        assert_eq!(result, nested);
        assert!(!fell_back);
    }
}
