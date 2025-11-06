use anyhow::{Result, anyhow};
use clap::{ArgAction, Parser};
use qqqa::ai::{ChatClient, Msg};
use qqqa::config::Config;
use qqqa::formatting::{print_assistant_text, print_stream_token, start_loading_animation};
use qqqa::history::read_recent_history;
use qqqa::prompt::{build_qq_system_prompt, build_qq_user_message};
use std::io::{Read, Stdin};

/// qq — ask an LLM assistant a question
///
/// - Reads optional terminal history and piped stdin as context.
/// - Sends a single user prompt to an OpenAI-compatible endpoint.
#[derive(Debug, Parser)]
#[command(name = "qq", disable_colored_help = false, version, about)]
struct Cli {
    /// Initialize or reinitialize configuration (~/.qq/config.json) and exit
    #[arg(long = "init", action = ArgAction::SetTrue)]
    init: bool,

    /// Disable emojis going forward (persists to config)
    #[arg(long = "no-fun", action = ArgAction::SetTrue)]
    no_fun: bool,
    /// Profile name from config to use
    #[arg(short = 'p', long = "profile")]
    profile: Option<String>,

    /// Override the model ID from profile
    #[arg(short = 'm', long = "model")]
    model: Option<String>,

    /// Disable terminal history context
    #[arg(short = 'n', long = "no-history", action = ArgAction::SetTrue)]
    no_history: bool,

    /// Stream response tokens as they arrive
    #[arg(short = 's', long = "stream", action = ArgAction::SetTrue)]
    stream: bool,

    /// Print raw text (no formatting)
    #[arg(short = 'r', long = "raw", action = ArgAction::SetTrue)]
    raw: bool,

    /// Verbose internal logs
    #[arg(short = 'd', long = "debug", action = ArgAction::SetTrue)]
    debug: bool,

    /// The question to ask (free text). If omitted, stdin must be piped.
    #[arg(trailing_var_arg = true)]
    question: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Run interactive init if requested.
    if cli.init {
        let path = Config::init_interactive(cli.debug)?;
        if cli.debug {
            eprintln!("[debug] Initialized config at {}", path.display());
        }
        return Ok(());
    }

    // Apply persistent emoji-disable if requested.
    if cli.no_fun {
        let (mut cfg, path) = Config::load_or_init(cli.debug)?;
        cfg.no_emoji = Some("true".to_string());
        cfg.save(&path, cli.debug)?;
        if cli.debug {
            eprintln!(
                "[debug] Disabled emojis in system prompt (persisted at {}).",
                path.display()
            );
        }
    }

    // Detect piped stdin and read it if present.
    let stdin_is_tty = atty::is(atty::Stream::Stdin);
    let stdin_block = if !stdin_is_tty {
        Some(read_all_stdin(std::io::stdin())?)
    } else {
        None
    };

    // Build question string from args.
    let question = cli.question.join(" ");
    if question.trim().is_empty() && stdin_block.as_deref().map_or(true, |s| s.trim().is_empty()) {
        return Err(anyhow!(
            "No input provided. Pass a question or pipe stdin (e.g., `ls -la | qq explain`)."
        ));
    }

    // Load config and resolve profile/model.
    let (cfg, _path) = Config::load_or_init(cli.debug)?;
    let eff = match cfg.resolve_profile(cli.profile.as_deref(), cli.model.as_deref()) {
        Ok(eff) => eff,
        Err(e) => {
            let msg = e.to_string();
            let mut out = msg.clone();
            if msg.contains("Missing API key") {
                out.push_str(
                    "\n\nFix it quickly:\n- Run `qq --init` and choose provider; optionally paste the API key.\n- Or export an env var, e.g.\n    export GROQ_API_KEY=...  # Groq\n    export OPENAI_API_KEY=... # OpenAI",
                );
            }
            return Err(anyhow!(out));
        }
    };
    if cli.debug {
        eprintln!(
            "[debug] Using provider='{}' base_url='{}' model='{}'",
            eff.provider_key, eff.base_url, eff.model
        );
    }

    // Read terminal history unless disabled.
    let history = if cli.no_history {
        Vec::new()
    } else {
        read_recent_history(20, cli.debug)
    };

    // Build system + user messages for formatting/topic control.
    let mut system = build_qq_system_prompt();
    if cfg.no_emoji_enabled() {
        system.push_str("\nHard rule: You MUST NOT use emojis anywhere in the response.\n");
    }
    let user = build_qq_user_message(
        Some(os_info::get().os_type()),
        &history,
        stdin_block.as_deref(),
        &question,
    );

    // Prepare HTTP client.
    let client = ChatClient::new(eff.base_url, eff.api_key)?;

    // Stream or buffered request per flag.
    if cli.stream {
        // If pretty output is desired, buffer tokens to render XML-ish after stream completes.
        if cli.raw {
            // One empty line before streamed raw output
            println!("");
            let msgs = [
                Msg {
                    role: "system",
                    content: &system,
                },
                Msg {
                    role: "user",
                    content: &user,
                },
            ];
            client
                .chat_stream_messages(&eff.model, &msgs, cli.debug, |tok| {
                    print_stream_token(tok);
                })
                .await?;
            println!();
        } else {
            use qqqa::formatting::render_xmlish_to_ansi;
            let msgs = [
                Msg {
                    role: "system",
                    content: &system,
                },
                Msg {
                    role: "user",
                    content: &user,
                },
            ];
            let mut buf = String::new();
            client
                .chat_stream_messages(&eff.model, &msgs, cli.debug, |tok| {
                    buf.push_str(tok);
                })
                .await?;
            use qqqa::formatting::compact_blank_lines;
            let rendered = render_xmlish_to_ansi(&buf);
            let compacted = compact_blank_lines(&rendered);
            // One empty line before the first line of the answer
            println!("");
            println!("{}", compacted.trim_end());
        }
    } else {
        let loading = start_loading_animation();
        let msgs = [
            Msg {
                role: "system",
                content: &system,
            },
            Msg {
                role: "user",
                content: &user,
            },
        ];
        let full = client
            .chat_once_messages(&eff.model, &msgs, cli.debug)
            .await?;
        // Ensure the animation is stopped and cleared before printing the answer
        drop(loading);
        // One empty line before the first line of the answer
        println!("");
        print_assistant_text(&full, cli.raw);
    }

    Ok(())
}

/// Read the entire stdin into a string. We do this synchronously before async work
/// begins to keep things simple and robust.
fn read_all_stdin(mut stdin: Stdin) -> Result<String> {
    let mut buf = String::new();
    stdin.read_to_string(&mut buf)?;
    Ok(buf)
}
