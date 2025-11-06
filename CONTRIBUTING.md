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

We keep prebuilt binaries in `releases/` inside the repo and retain ~3 latest versions. A helper script automates most of it.

Prerequisites
- Clean working tree and passing tests.
- Rust stable; `rustup` available to add targets.
- Optional extra linkers if you aim for musl or uncommon targets.

Quick start (tagging HEAD)
1) Build and package v0.8.4:
```
scripts/release.sh v0.8.4
```
2) Commit artifacts and version bump:
```
git add Cargo.toml
git commit -m "release: v0.8.4"
```
3) Tag and push:
```
git tag -a v0.8.4 -m "qqqa v0.8.4"
git push && git push --tags
```

Tag a specific SHA
```
scripts/release.sh v0.8.4 <git_sha>
git add Cargo.toml
git commit -m "release: v0.8.4"
git tag -a v0.8.4 <git_sha> -m "qqqa v0.8.4"
git push && git push --tags
```

Limit targets
```
TARGETS="x86_64-apple-darwin aarch64-apple-darwin" scripts/release.sh v0.8.4
```

What the script does
- Bumps `Cargo.toml` version to the provided one (normalizes to SemVer).
- Builds `qq` and `qa` for macOS (x86_64/arm64) and Linux MUSL (x86_64/arm64) by default; override `TARGETS` if needed.
- Packages `qqqa-v<version>-<target>.tar.gz` under `target/releases/v<version>/` with README and LICENSE.
- Writes `target/releases/v<version>/manifest.json` for upload to GitHub Releases.

Notes
- Cross-compiling across OSes may require appropriate toolchains. The script runs `rustup target add` for the listed targets.
- For static Linux builds, switch to `*-unknown-linux-musl` targets if your environment supports them.

## Code of Conduct

Be respectful, constructive, and considerate. We value practical collaboration and helpful reviews.
