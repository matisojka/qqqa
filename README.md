# qqqa

Fast, stateless LLM-powered assistant for your shell: qq answers; qa runs commands

## What is qqqa

qqqa is a two-in-one, stateless CLI tool that brings LLM assistance to the command line without ceremony.

The two binaries are:

- `qq` - ask a single question, e.g. "qq how can I recursively list all files in this directory" (qq stands for "quick question")
- `qa` - a single step agent that can optionally use tools to finish a task: read a file, write a file, or execute a command with confirmation (qa stands for "quick agent")

By default the repo includes profiles for OpenRouter (default), OpenAI, Groq, and a local Ollama runtime. An Anthropic profile stub exists in the config for future work but is not wired up yet.



https://github.com/user-attachments/assets/91e888ad-0279-4d84-924b-ba96c0fe43a0



## Names and typing speed

qq means quick question. qa means quick agent. Both are easy to type rapidly on QWERTY keyboards with minimal finger movement. That makes interacting with LLMs faster and more natural during real work.

## Philosophy

qqqa is deliberately stateless. There is no long running session and no hidden conversation memory stored by the tool. Every run is mostly independent and reproducible. For maintaining a lowkey continuity you can use `"include_history": true` in the `config.json` (or choose to use history during the `qq --init` process).

Why stateless is great:

- Simple and focused - Unix philosophy applied to LLM tools.
- Shell friendly - compose with pipes and files instead of interactive chats.
- Safe by default - qq is read-only and has access to no tools. qa is built with security in mind and requires confirmation before running tools.

The tools may include transient context you choose to provide:

- `qq` can include the last few terminal commands as hints and piped stdin if present.
- `qa` can read files or run a specific command, but only once per invocation and with safety checks.

## Why I recommend using OpenRouter by default

OpenRouter mirrors the OpenAI Chat Completions API, adds generous community-hosted models, and keeps `openai/gpt-4.1-nano` fast and inexpensive. qqqa talks to `https://openrouter.ai/api/v1` out of the box and reads the API key from `OPENROUTER_API_KEY`, so your first run works as soon as you drop in a key.

If you need even more throughput, the bundled `groq` profile that targets `openai/gpt-oss-20b` and `openai/gpt-oss-120b` remains available, and you can still add any OpenAI-compatible provider by editing `~/.qq/config.json` or creating a new profile.
## Features

- OpenAI compatible API client with streaming and non streaming calls.
- Stateless, single shot workflow that plays well with pipes and scripts.
- Rich but simple formatting using XML like tags rendered to ANSI colors.
- Config driven providers and profiles with per profile model overrides.
- Safety rails for file access and command execution.
- Old-school and SERIOUS? Optional no-emoji mode persisted via `--no-fun` 🥸

## Install

### macOS

Use the Homebrew tap:

```sh
brew tap iagooar/qqqa
brew install qqqa
```

### Linux

Download a prebuilt archive from the [GitHub Releases](https://github.com/iagooar/qqqa/releases) page, extract it, and place `qq`/`qa` somewhere on your `PATH` (e.g., `/usr/local/bin`).

## Configure

On first run qqqa creates `~/.qq/config.json` with safe permissions. For a smooth first interaction, run the init flow:

```sh
# Interactive setup (choose provider and set key)
qq --init
# or
qa --init
```

If `~/.qq/config.json` already exists, the init command keeps it untouched and explains how to rerun after moving or deleting the file.

The initializer lets you choose the default provider:

- OpenRouter + `openai/gpt-4.1-nano` (default, fast and inexpensive)
- Groq + `openai/gpt-oss-20b` (faster, cheap paid tier)
- OpenAI + `gpt-5-mini` (slower, a bit smarter)
- Anthropic + `claude-3-5-sonnet-20241022` (placeholder until their Messages API finalizes)
- Ollama (runs locally, adjust port if needed)

It also offers to store an API key in the config (optional). If you prefer environment variables, leave it blank and set one of:

- `OPENROUTER_API_KEY` for OpenRouter (default)
- `GROQ_API_KEY` for Groq
- `OPENAI_API_KEY` for OpenAI
- `OLLAMA_API_KEY` (optional; any non-empty string works—even `local`—because the Authorization header cannot be blank)

Defaults written to `~/.qq/config.json`:

- Providers
  - `openrouter` → base `https://openrouter.ai/api/v1`, env `OPENROUTER_API_KEY`, default headers `HTTP-Referer=https://github.com/iagooar/qqqa` and `X-Title=qqqa`
  - `openai` → base `https://api.openai.com/v1`, env `OPENAI_API_KEY`
  - `groq` → base `https://api.groq.com/openai/v1`, env `GROQ_API_KEY`
  - `ollama` → base `http://127.0.0.1:11434/v1`, env `OLLAMA_API_KEY` (qqqa auto-injects a non-empty placeholder if you leave it unset)
  - `anthropic` → base `https://api.anthropic.com/v1`, env `ANTHROPIC_API_KEY` (present in the config schema for future support; not usable yet)
- Profiles
  - `openrouter` → model `openai/gpt-4.1-nano` (default)
  - `openai` → model `gpt-5-mini`
  - `groq` → model `openai/gpt-oss-20b`
  - `ollama` → model `llama3.1`
  - `anthropic` → model `claude-3-5-sonnet-20241022` (inactive placeholder until Anthropic integration lands)
- Optional per-profile `reasoning_effort` for GPT-5 family models. If you leave it unset, qqqa sends `"reasoning_effort": "minimal"` for any `gpt-5*` model to keep responses fast. Set it to `"low"`, `"medium"`, or `"high"` when you want deeper reasoning.
- (discouraged): you can change the timeout, e.g. `"timeout": "240"` under a model profile in `~/.qq/config.json` to raise the per-request limit (`qq` + `qa` default to 180 s - this is SLOW; faster models are a better fix).

Example override in `~/.qq/config.json`:

```json
{
  "profiles": {
    "openai": {
      "model_provider": "openai",
      "model": "gpt-5-mini",
      "reasoning_effort": "medium"
    }
  }
}
```

- Optional flag: `no_emoji` (unset by default). Set via `qq --no-fun` or `qa --no-fun`.
- Optional auto-copy: `copy_first_command` (unset/false by default). Enable during `qq --init`, by running `qq --enable-auto-copy`, or by editing `~/.qq/config.json` so qq copies the first `<cmd>` block to your clipboard. Turn it off with `qq --disable-auto-copy`. Override per run with `--copy-command`/`--cc` or `--no-copy-command`/`--ncc` (also available as `-ncc`).

### Terminal history

Terminal history is **off by default**. During `qq --init` / `qa --init` you can opt in to sending the last 10 `qq`/`qa` commands along with each request. You can still override per run with `--history` (force on) or `-n/--no-history` (force off). Only commands whose first token is `qq` or `qa` are ever shared.


## Usage

### qq - ask a question

```sh
# simplest
qq "convert mp4 to mp3"

# stream tokens with formatted output
qq -s "how do I kill a process by name on macOS"

# include piped context
git status | qq "summarize what I should do next"

# pipe extra context and keep CLI question
printf '%s\n' "This is a sample context. My code is 4242" | qq "What is my code"

# pipe the question itself
printf '%s\n' "Show me the full contents of this directory" | qq

# raw text (no ANSI formatting)
qq -r "explain sed vs awk"

# include terminal history for this run
qq --history "find large files in the last day"

# disable emojis in responses (persists)
qq --no-fun "summarize this"

# auto-copy the first <cmd> block for fast pasting (alias: --cc)
qq --copy-command "list docker images"

# temporarily disable auto-copy even if enabled in config (alias: --ncc / -ncc)
qq --no-copy-command "print working directory"

# enable auto-copy for all future qq runs
qq --enable-auto-copy

# disable auto-copy persistently
qq --disable-auto-copy
```

Note: it is possible to run qq without quotes, which works most of the time the same way as with quotes.


```sh
# simplest
qq convert mp4 to mp3
```


#### Example: forgot the ffmpeg incantation

You want to extract audio from a YouTube video but you do not remember the exact flags.

Ask with qq:

```sh
qq "how do I use ffmpeg to extract audio from a YouTube video into mp3"
```

A typical answer will suggest installing the tools and then using `yt-dlp` to fetch audio and `ffmpeg` to convert it:

```sh
# macOS
brew install yt-dlp ffmpeg

# Debian or Ubuntu
sudo apt-get update && sudo apt-get install -y yt-dlp ffmpeg

# Download and extract audio to MP3 using ffmpeg under the hood
yt-dlp -x --audio-format mp3 "https://www.youtube.com/watch?v=VIDEO_ID"
```

Do it for me with qa:

```sh
qa "download audio as mp3 from https://www.youtube.com/watch?v=VIDEO_ID"
```

The agent will propose a safe command like `yt-dlp -x --audio-format mp3 URL`, show it for confirmation, then run it. You can pass `-y` to auto approve.

### qa - do a single step with tools

`qa` can either answer in plain text or request one tool call in JSON. Supported tools:

- `read_file` with `{ "path": string }`
- `write_file` with `{ "path": string, "content": string }`
- `execute_command` with `{ "command": string, "cwd?": string }`

Examples:

```sh
# read a file the safe way
qa "read src/bin/qq.rs and tell me what main does"

# write a file
qa "create a README snippet at notes/intro.md with a short summary"

# run a command with confirmation
qa "list Rust files under src sorted by size"

# pipe the task itself
printf '%s\n' "Show me the full contents of this directory" | qa

# auto approve tool execution for non interactive scripts
qa -y "count lines across *.rs"

# include recent qq/qa commands just for this run
qa --history "trace which git commands I ran recently"

# disable emojis in responses (persists)
qa --no-fun "format and lint the repo"
```

When qa runs a command while stdout is a terminal, output now streams live; the structured `[tool:execute_command]` summary still prints afterward for easy copying.

`execute_command` prints the proposed command and asks for confirmation. It warns if the working directory is outside your home. Use `-y` to auto approve in trusted workflows.

The runner enforces a default allowlist (think `ls`, `grep`, `find`, `rg`, `awk`, etc.) and rejects pipelines, redirection, and other high-risk constructs. When a command is blocked, `qa` prompts you to add it to `command_allowlist` inside `~/.qq/config.json`; approving once persists the choice and updates future runs.

## Advanced features and configurations

### Custom TLS certificates (self-signed proxies)

Some OpenAI-compatible gateways (LiteLLM, local corporate proxies, etc.) terminate TLS with a self-signed CA. Add a per-provider `tls` block so qqqa trusts that CA in addition to the default Rustls bundle:

```json
{
  "model_providers": {
    "litellm": {
      "name": "LiteLLM",
      "base_url": "https://proxy.local/v1",
      "env_key": "LITELLM_API_KEY",
      "tls": {
        "ca_bundle_path": "certs/litellm-ca.pem",
        "ca_bundle_env": "SSL_CERTFILE_PATH"
      }
    }
  }
}
```

- `ca_bundle_path` accepts a PEM or DER file. Relative paths are resolved against `~/.qq/` so you can keep certificates next to the config.
- `ca_bundle_env` is optional; if set, qqqa reads that environment variable for the bundle path and falls back to `ca_bundle_path` when it is unset. This mirrors proxies that expose `SSL_CERTFILE_PATH` or similar knobs.
- Multiple certificates can live in the same file (concatenate PEM entries). qqqa appends them to the existing Rustls trust store, so standard public CAs continue to work.

With this configuration any provider—LiteLLM, Ollama over HTTPS, your company gateway, or another proxy—can authenticate with its custom CA without disabling TLS verification.



### Local models & custom ports

Pick the built-in `ollama` profile (or create your own) to talk to a local runtime. Override the API base when you expose the service on a different host/port:

```sh
qq --profile ollama --api-base http://127.0.0.1:11435/v1 "summarize build failures"
qa --profile ollama --api-base http://192.168.1.50:9000/v1 "apply the diff" -y
```

`qa --init` offers Ollama as an option and skips the API key warning; qqqa still sends a placeholder bearer token so OpenAI-compatible middleware keeps working. If you bypass the init flow and edit `config.json` manually, set either `"api_key": "local"` under the `ollama` provider or export `OLLAMA_API_KEY=local` so the Authorization header remains non-empty.

> Example local setup: LM Studio on macOS driving `ollama run meta-llama-3.1-8b-instruct-hf` (Q4_K_M) on a MacBook Air M4/32 GB works fine, just slower than the hosted OpenRouter/Groq profiles. Adjust the model tag in your `ollama` profile accordingly.

You can still override at runtime:

```sh
# choose profile
qq -p groq "what is ripgrep"

# override model for a single call
qq -m openai/gpt-oss-20b "explain this awk one-liner"
```


## Safety model

- File tools require paths to be inside your home or the current directory. Reads are capped to 1 MiB, and traversal/symlink escapes are blocked.
- Command execution uses a default allowlist (e.g. `ls`, `grep`, `rg`, `find`) plus your custom `command_allowlist` entries. Destructive patterns (`rm -rf /`, `sudo`, `mkfs`, etc.) are always blocked, and pipelines/redirection/newlines prompt for confirmation even with `--yes`.
- Commands run with a 120 s timeout and the agent performs at most one tool step—there is no loop.
- Config files are created with safe permissions. API keys come from environment variables unless you explicitly add a key to the config.

## Environment variables

- `OPENROUTER_API_KEY` for the OpenRouter provider (default)
- `GROQ_API_KEY` for the Groq provider
- `OPENAI_API_KEY` for the OpenAI provider

## Development

Project layout:

- `src/bin/qq.rs` and `src/bin/qa.rs` entry points
- Core modules in `src/`: `ai.rs`, `config.rs`, `prompt.rs`, `history.rs`, `perms.rs`, `formatting.rs`
- Tools in `src/tools/`: `read_file.rs`, `write_file.rs`, `execute_command.rs`
- Integration tests in `tests/`

## Contributing

See CONTRIBUTING.md for guidelines on reporting issues and opening pull requests, building from source, and the release process.

## Troubleshooting

- API error about missing key: run `qq --init` to set things up, or export the relevant env var, e.g. `export OPENROUTER_API_KEY=...`.
- No output when streaming: try `-d` to see debug logs.
- Piped input not detected: ensure you are piping into `qq` and not running it in a subshell that swallows stdin.

## License

Licensed under MIT.
