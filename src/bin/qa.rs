use anyhow::{anyhow, Result};
use clap::{ArgAction, Parser};
use qqqa::ai::{AssistantReply, ChatClient, Msg};
use qqqa::config::Config;
use qqqa::history::read_recent_history;
use qqqa::prompt::{build_qa_system_prompt, build_qa_user_message};
use qqqa::tools::{parse_tool_call, ToolCall};
use std::io::{Read, Stdin};

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

    /// Override the model ID from profile
    #[arg(short = 'm', long = "model")]
    model: Option<String>,

    /// Disable terminal history context
    #[arg(short = 'n', long = "no-history", action = ArgAction::SetTrue)]
    no_history: bool,

    /// Verbose internal logs
    #[arg(short = 'd', long = "debug", action = ArgAction::SetTrue)]
    debug: bool,

    /// Auto-approve command execution (use with caution)
    #[arg(short = 'y', long = "yes", action = ArgAction::SetTrue)]
    yes: bool,

    /// The task to perform. If omitted, stdin must be piped.
    #[arg(trailing_var_arg = true)]
    task: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.init {
        let path = Config::init_interactive(cli.debug)?;
        if cli.debug {
            eprintln!("[debug] Initialized config at {}", path.display());
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

    let task = cli.task.join(" ");
    if task.trim().is_empty() && stdin_block.as_deref().map_or(true, |s| s.trim().is_empty()) {
        return Err(anyhow!("No input provided. Pass a task or pipe stdin."));
    }

    let (cfg, _p) = Config::load_or_init(cli.debug)?;
    let eff = match cfg.resolve_profile(cli.profile.as_deref(), cli.model.as_deref()) {
        Ok(eff) => eff,
        Err(e) => {
            let msg = e.to_string();
            let mut out = msg.clone();
            if msg.contains("Missing API key") {
                out.push_str(
                    "\n\nFix it quickly:\n- Run `qa --init` and choose provider; optionally paste the API key.\n- Or export an env var, e.g.\n    export GROQ_API_KEY=...  # Groq\n    export OPENAI_API_KEY=... # OpenAI",
                );
            }
            return Err(anyhow!(out));
        }
    };
    if cli.debug {
        eprintln!("[debug] Using provider='{}' base_url='{}' model='{}'", eff.provider_key, eff.base_url, eff.model);
    }

    let history = if cli.no_history { Vec::new() } else { read_recent_history(20, cli.debug) };
    let system_prompt = build_qa_system_prompt();
    let user_msg = build_qa_user_message(Some(os_info::get().os_type()), &history, stdin_block.as_deref(), &task);

    let client = ChatClient::new(eff.base_url, eff.api_key)?;
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
            &[Msg { role: "system", content: &system_prompt }, Msg { role: "user", content: &user_msg }],
            tools_spec,
            cli.debug,
        )
        .await?;

    match assistant_reply {
        AssistantReply::ToolCall { name, arguments_json } => {
            let handled = match name.as_str() {
                "read_file" => {
                    let args: qqqa::tools::read_file::Args = serde_json::from_str(&arguments_json)
                        .map_err(|e| anyhow!("Failed to parse read_file args: {}", e))?;
                    match qqqa::tools::read_file::run(args) {
                        Ok(content) => {
                            print_tool_result("read_file", &content);
                            true
                        }
                        Err(e) => {
                            print_tool_error("read_file", &e.to_string());
                            true
                        }
                    }
                }
                "write_file" => {
                    let args: qqqa::tools::write_file::Args = serde_json::from_str(&arguments_json)
                        .map_err(|e| anyhow!("Failed to parse write_file args: {}", e))?;
                    match qqqa::tools::write_file::run(args) {
                        Ok(summary) => {
                            print_tool_result("write_file", &summary);
                            true
                        }
                        Err(e) => {
                            print_tool_error("write_file", &e.to_string());
                            true
                        }
                    }
                }
                "execute_command" => {
                    let args: qqqa::tools::execute_command::Args = serde_json::from_str(&arguments_json)
                        .map_err(|e| anyhow!("Failed to parse execute_command args: {}", e))?;
                    match qqqa::tools::execute_command::run(args, cli.yes, cli.debug).await {
                        Ok(summary) => {
                            print_tool_result("execute_command", &summary);
                            true
                        }
                        Err(e) => {
                            print_tool_error("execute_command", &e.to_string());
                            true
                        }
                    }
                }
                _ => false,
            };
            if !handled {
                println!("Unknown tool call: {}", name);
            }
        }
        AssistantReply::Content(assistant) => {
            // Try to parse as a tool call per our plain-JSON protocol; else print the answer.
            match parse_tool_call(assistant.trim()) {
                Ok(call) => {
                    match call {
                        ToolCall::ReadFile(args) => {
                            match qqqa::tools::read_file::run(args) {
                                Ok(content) => print_tool_result("read_file", &content),
                                Err(e) => print_tool_error("read_file", &e.to_string()),
                            }
                        }
                        ToolCall::WriteFile(args) => {
                            match qqqa::tools::write_file::run(args) {
                                Ok(summary) => print_tool_result("write_file", &summary),
                                Err(e) => print_tool_error("write_file", &e.to_string()),
                            }
                        }
                        ToolCall::ExecuteCommand(args) => {
                            match qqqa::tools::execute_command::run(args, cli.yes, cli.debug).await {
                                Ok(summary) => print_tool_result("execute_command", &summary),
                                Err(e) => print_tool_error("execute_command", &e.to_string()),
                            }
                        }
                    }
                }
                Err(_) => {
                    println!("{}", assistant.trim_end());
                }
            }
        }
    }

    Ok(())
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
