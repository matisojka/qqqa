# qqqa

Fast, stateless LLM-powered assistant for your shell: qq answers; qa runs commands

## What is qqqa

qqqa is a two-in-one small CLI tool written in Rust that brings LLM assistance to the command line without ceremony.

The two binaries are:

- `qq` - ask a single question, e.g. "qq how can I recursively list all files in this directory"
- `qa` - a single step agent that can optionally use tools to finish a task: read a file, write a file, or execute a command with confirmation.

By default the repo includes profiles for OpenAI and Groq.

## Names and typing speed

qq means quick question. qa means quick agent. Both are easy to type rapidly on QWERTY keyboards with minimal finger movement. That makes interacting with LLMs faster and more natural during real work.

## Philosophy

qqqa is deliberately stateless. There is no long running session and no hidden conversation memory stored by the tool. Every run is independent and reproducible.

Why stateless is great:

- Simple and focused - Unix philosophy applied to LLM tools.
- Shell friendly - compose with pipes and files instead of interactive chats.
- Safe by default - qq is read-only and has access to no tools. qa is built with security in mind and requires confirmation before running tools.

The tools may include transient context you choose to provide:

- `qq` can include the last few terminal commands as hints and piped stdin if present.
- `qa` can read files or run a specific command, but only once per invocation and with safety checks.

## Why we recommend using Groq by default

For fast feedback loops, speed and cost matter. The included `groq` profile targets Groq's OpenAI compatible API and the model `openai/gpt-oss-20b`. We recommend Groq for really fast inference speed at roughly 1000 tokens per minute and at a low price point compared to many alternatives. Set `GROQ_API_KEY` and you are ready to go.

You can still use OpenAI or any other OpenAI compatible provider by adding a provider entry and a profile in `~/.qq/config.json`.

## Features

- OpenAI compatible API client with streaming and non streaming calls.
- Stateless, single shot workflow that plays well with pipes and scripts.
- Rich but simple formatting using XML like tags rendered to ANSI colors.
- Config driven providers and profiles with per profile model overrides.
- Safety rails for file access and command execution.
- Optional no-emoji mode persisted via `--no-fun`.

## Install

Build from source with Rust 1.75+:

```sh
cargo build --release
```

The binaries will be in `target/release/qq` and `target/release/qa`. Add them to your `PATH` or use `cargo run` during development.

Optional local install while iterating:

```sh
cargo install --path .
```

## Configure

On first run qqqa creates `~/.qq/config.json` with safe permissions. For a smooth first interaction, run the init flow:

```sh
# Interactive setup (choose provider and set key)
qq --init
# or
qa --init
```

The initializer lets you choose the default provider:

- Groq + `openai/gpt-oss-20b` (faster, cheaper)
- OpenAI + `gpt-5-mini` (slower, a bit smarter)

It also offers to store an API key in the config (optional). If you prefer environment variables, leave it blank and set one of:

- `GROQ_API_KEY` for Groq
- `OPENAI_API_KEY` for OpenAI

Defaults written to `~/.qq/config.json`:

- Providers
  - `openai` → base `https://api.openai.com/v1`, env `OPENAI_API_KEY`
  - `groq` → base `https://api.groq.com/openai/v1`, env `GROQ_API_KEY`
- Profiles
  - `openai` → model `gpt-5-mini`
  - `groq` → model `openai/gpt-oss-20b` (default)

- Optional flag: `no_emoji` (unset by default). Set via `qq --no-fun` or `qa --no-fun`.

You can still override at runtime:

```sh
# choose profile
qq -p groq "what is ripgrep"

# override model for a single call
qq -m openai/gpt-oss-20b "explain this awk one-liner"
```

## Usage

### qq - ask a question

```sh
# simplest
qq "convert mp4 to mp3"

# stream tokens with formatted output
qq -s "how do I kill a process by name on macOS"

# include piped context
git status | qq "summarize what I should do next"

# raw text (no ANSI formatting)
qq -r "explain sed vs awk"

# skip reading terminal history
qq -n "find large files in the last day"

# disable emojis in responses (persists)
qq --no-fun "summarize this"
```

qq builds a user message that includes a timestamp, your OS, optional recent terminal history, optional piped stdin, and your question. The assistant is instructed to stay on technical topics and to format output using these tags:

- `<cmd>` for commands
- `<bold>` for headings
- `<info>` for tips
- `<file>` for paths
- `<warn>` for warnings
- `<br/>` for blank lines

The CLI renders these tags as ANSI colored output for readability.

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

# auto approve tool execution for non interactive scripts
qa -y "count lines across *.rs"

# disable emojis in responses (persists)
qa --no-fun "format and lint the repo"
```

`execute_command` prints the proposed command and asks for confirmation. It warns if the working directory is outside your home. Use `-y` to auto approve in trusted workflows.

## Safety model

- File tools require paths to be inside your home or the current directory. Reads are capped to 1 MiB.
- Command execution blocks obviously dangerous patterns like `rm -rf /`, `sudo`, and direct disk writes. Commands run with a timeout.
- The agent performs at most one tool step. There is no loop.
- Config files are created with safe permissions. API keys come from environment variables unless you explicitly add a key to the config.

## Environment variables

- `GROQ_API_KEY` for the Groq provider
- `OPENAI_API_KEY` for the OpenAI provider

## Development

Build and run:

```sh
cargo build
cargo run --bin qq -- --help
cargo run --bin qa -- --help
```

Run tests:

```sh
cargo test
```

Project layout:

- `src/bin/qq.rs` and `src/bin/qa.rs` entry points
- Core modules in `src/`: `ai.rs`, `config.rs`, `prompt.rs`, `history.rs`, `perms.rs`, `formatting.rs`
- Tools in `src/tools/`: `read_file.rs`, `write_file.rs`, `execute_command.rs`
- Integration tests in `tests/`

## Contributing

See CONTRIBUTING.md for guidelines on reporting issues and opening pull requests, and for the release process.

## Releases

The repo ships prebuilt binaries under `releases/`. We keep the latest ~3 versions with a simple, script-driven flow.

- Build and package a release:

```sh
# Build v0.7.0 for common targets and package tar.gz artifacts
scripts/release.sh v0.7.0

# Optionally specify a Git SHA to record in the manifest (and tag later)
scripts/release.sh v0.7.0 <git_sha>

# Override targets (space-separated)
TARGETS="x86_64-apple-darwin aarch64-apple-darwin" scripts/release.sh v0.7.0
```

What the script does:

- Bumps `Cargo.toml` version to the given one.
- Builds `qq` and `qa` for each target with `cargo build --release`.
- Packages `qqqa-v<version>-<target>.tar.gz` into `releases/` and writes checksums.
- Writes `releases/v<version>/manifest.json` and updates `releases/index.json`.
- Prunes older versions, keeping the last 3.

Tagging the release:

```sh
git add Cargo.toml releases/
git commit -m "release: v0.7.0"
git tag -a v0.7.0 -m "qqqa v0.7.0"   # or: git tag -a v0.7.0 <sha> -m "qqqa v0.7.0"
git push && git push --tags
```

Common targets (customizable via `TARGETS`):

- `x86_64-apple-darwin`
- `aarch64-apple-darwin`
- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`

Notes:

- Cross-compiling may require additional toolchains; `rustup target add <triple>` is attempted automatically.
- For fully-static Linux builds, you can adjust targets to `*-unknown-linux-musl` if your environment supports it.

## Troubleshooting

- API error about missing key: run `qq --init` to set things up, or export the relevant env var, e.g. `export GROQ_API_KEY=...`.
- No output when streaming: try `-d` to see debug logs.
- Piped input not detected: ensure you are piping into `qq` and not running it in a subshell that swallows stdin.

## License

Licensed under MIT.
