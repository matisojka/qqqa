# Repository Guidelines

## Project Structure & Module Organization
- Source lives in `src/` with two binaries: `src/bin/qq.rs` and `src/bin/qa.rs`.
- Core modules: `ai.rs`, `config.rs`, `prompt.rs`, `history.rs`, `perms.rs`, `formatting.rs`.
- Tools for the agent live in `src/tools/` (`read_file.rs`, `write_file.rs`, `execute_command.rs`).
- Integration tests live in `tests/`. Build artifacts are in `target/`.
- Architecture notes: `src/lib.rs` re-exports the runtime modules and `src/tools/mod.rs` centralizes tool parsing/dispatch; refer to `README.md` for the high-level architecture and behavior overview.

## Build, Test, and Development Commands
- Build: `cargo build` (debug) or `cargo build --release`.
- Run qq: `cargo run --bin qq -- --help` or `cargo run --bin qq -- "your question"`.
- Run qa (agent): `cargo run --bin qa -- "your task"` (adds tool execution).
- Tests: `cargo test` (runs all in `tests/` and unit tests).
- Debug logs: `RUST_LOG=debug cargo run --bin qq -- -d "..."`.

## Coding Style & Naming Conventions
- Rust 2024 edition; format with `rustfmt` (4‑space indents). Prefer `clippy` clean builds.
- Naming: `snake_case` for functions/modules, `PascalCase` for types, `SCREAMING_SNAKE_CASE` for consts.
- Errors: favor `anyhow` for app layers and `thiserror` for library error types; use `?` over `unwrap()` in binaries.
- Keep modules focused and functions small; prefer pure helpers in `src/` over ad‑hoc logic in bins.

## Testing Guidelines
- Integration tests in `tests/*.rs`; unit tests colocated with modules.
- Name tests descriptively: `test_<behavior>()`; table‑driven where useful.
- Mock HTTP with `httpmock`; use `tempfile` for fs tests; avoid global state.
- Run: `cargo test`; add edge‑case coverage for config permissions and tool safety.

## Commit & Pull Request Guidelines
- Commits: imperative, scoped messages (e.g., `feat: add qa execute_command safety checks`).
- PRs: include a clear description, reproduction/verification steps, relevant logs, and screenshots when output formatting changes.
- Link issues, keep diffs minimal, and update docs/tests when behavior changes.

## Security & Configuration Tips
- Config path: `~/.qq/config.json`; enforce permissions (dir `700`, file `600`).
- API keys via env: `OPENROUTER_API_KEY` (default), `OPENAI_API_KEY`, `GROQ_API_KEY`, `ANTHROPIC_API_KEY`; never commit secrets.
- Agent (`qa`): print proposed commands, require confirmation unless `--yes`; avoid destructive patterns and paths outside the workspace/home.
- Command allowlist: default safe commands are bundled. Approving a blocked binary persists it under `command_allowlist` in `~/.qq/config.json` for future runs.
