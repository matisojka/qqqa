# Contributing & Release Guide

Thanks for your interest in qqqa. This doc is for contributors and for anyone cutting releases.

## How to Contribute

- Issues
  - Report bugs and feature requests via GitHub Issues. Include repro steps, logs, and environment info when possible.
- Pull Requests
  - Small, focused diffs are best. Describe the change, rationale, and how to verify.
  - Keep style consistent with the repo. Prefer `anyhow` in app code, `thiserror` for library errors.
  - Update docs/tests when behavior changes. Avoid committing secrets.
- Development basics
  - Build: `cargo build` (or `cargo build --release`)
  - Run: `cargo run --bin qq -- --help` and `cargo run --bin qa -- --help`
  - Test: `cargo test`

## Building Releases

We keep prebuilt binaries in `target/releases/` inside the repo and retain ~3 latest versions; this directory is already gitignored, so the tarballs stay local and never bloat the repo. The `scripts/release.sh` helper automates the heavy lifting and prints next steps.

Prerequisites
- Clean working tree and passing tests.
- Rust stable; `rustup` available to add targets.
- Optional extra linkers if you aim for musl or uncommon targets.

Quick start (tagging HEAD)
```
# build artifacts, update Cargo.toml + manifest
scripts/release.sh v0.9.1

# follow the printed checklist (summary below)
git add Cargo.toml
git commit -m "release: v0.9.1"
git tag -a v0.9.1 -m "qqqa v0.9.1"
git push && git push --tags

# publish the GitHub release with the generated tarballs/manifest
gh release create v0.9.1 target/releases/v0.9.1/qqqa-v0.9.1-*.tar.gz \
  target/releases/v0.9.1/qqqa-v0.9.1-src.tar.gz target/releases/v0.9.1/manifest.json \\
  --title "qqqa v0.9.1" --notes-file docs/RELEASE_NOTES_TEMPLATE.md
```

Tag a specific SHA by passing it as the second argument: `scripts/release.sh v0.9.1 <git_sha>` (the script still prints the same checklist, but the tag command includes your SHA).

Limit targets when you only want a subset (e.g., during local smoke tests):
```
TARGETS="x86_64-apple-darwin aarch64-apple-darwin" scripts/release.sh v0.9.1
```

What the script does
- Bumps `Cargo.toml` version to the provided one (normalizes to SemVer).
- Builds `qq` and `qa` for macOS (x86_64/arm64), Linux MUSL (x86_64/arm64), and Windows (x86_64 GNU + arm64 gnullvm) by default; override `TARGETS` if needed.
- Packages `qqqa-v<version>-<target>.tar.gz` under `target/releases/v<version>/` with README and LICENSE.
- Writes `target/releases/v<version>/manifest.json` for upload to GitHub Releases.
- Prints a human checklist covering commits, Git tags, and `gh release create` (artifacts stay under `target/releases/`, which is already gitignored; upload them when drafting the GitHub release).
- Provides `docs/RELEASE_NOTES_TEMPLATE.md` as a quick-start for the release description (`gh release create --notes-file docs/RELEASE_NOTES_TEMPLATE.md`).

Notes
- Cross-compiling across OSes may require appropriate toolchains. The script runs `rustup target add` for the listed targets.
- For static Linux builds, switch to `*-unknown-linux-musl` targets if your environment supports them.
- Windows artifacts use MSVC targets on Windows hosts. When cross-compiling from macOS/Linux, we ship the x86_64 build via MinGW-w64 (`x86_64-pc-windows-gnu`, requires `x86_64-w64-mingw32-gcc`) and the arm64 build via llvm-mingw (`aarch64-pc-windows-gnullvm`, requires `aarch64-w64-mingw32-clang`). Install the corresponding toolchains (Homebrew `mingw-w64` plus an `llvm-mingw` bundle) before running the release script.

## Code of Conduct

Be respectful, constructive, and considerate. We value practical collaboration and helpful reviews.
