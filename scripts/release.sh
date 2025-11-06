#!/usr/bin/env bash
set -euo pipefail

# Simple release builder for qqqa
# - Bumps Cargo.toml version
# - Builds binaries for common targets
# - Packages tar.gz artifacts under target/releases/v<version>/
# - Writes checksums and a minimal manifest.json (also under target/releases)
# - Optionally tags the repo at a given SHA
#
# Usage:
#   scripts/release.sh v0.7.0 [<git_sha>] [TARGETS="..."]
#
# Defaults TARGETS depend on host OS and prefer MUSL for Linux cross-builds:
#   - macOS: x86_64-apple-darwin aarch64-apple-darwin x86_64-unknown-linux-musl aarch64-unknown-linux-musl
#   - Linux: host triple from `rustc -vV` (GNU), plus darwin only if osxcross toolchains are present
#
# You can override with TARGETS to include cross targets, but ensure
# cross C toolchains exist (e.g., aarch64-linux-musl-gcc or aarch64-linux-gnu-gcc).

root_dir=$(cd "$(dirname "$0")/.." && pwd)
cd "$root_dir"

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 v<version> [git_sha]" >&2
  exit 1
fi

ver_in=$1
ver_raw=${ver_in#v}
# Normalize to semver (major.minor.patch); if only major.minor provided, append .0
if [[ "$ver_raw" =~ ^[0-9]+\.[0-9]+$ ]]; then
  ver="${ver_raw}.0"
else
  ver="$ver_raw"
fi
git_sha=${2:-}

# Compute sensible OS-aware defaults to avoid accidental cross builds
host_os=$(uname -s)
targets_default=(
  x86_64-apple-darwin
  aarch64-apple-darwin
  x86_64-unknown-linux-musl
  aarch64-unknown-linux-musl
)

IFS=' ' read -r -a targets <<< "${TARGETS:-${targets_default[*]}}"

echo "==> Releasing version ${ver}"

# Bump Cargo.toml version (in-place) in a portable way
if command -v gsed >/dev/null 2>&1; then
  gsed -i -E "s/^version = \"[0-9].*\"/version = \"${ver}\"/" Cargo.toml
else
  if [[ "$(uname -s)" == "Darwin" ]]; then
    sed -i '' -E "s/^version = \"[0-9].*\"/version = \"${ver}\"/" Cargo.toml
  else
    sed -i -E "s/^version = \"[0-9].*\"/version = \"${ver}\"/" Cargo.toml
  fi
fi

echo "==> Building targets: ${targets[*]}"

# Per-version directory for artifacts and manifest
artifact_root="target/releases/v${ver}"
mkdir -p "$artifact_root"
manifest_dir="$artifact_root"

for t in "${targets[@]}"; do
  echo "--> Target: $t"
  export PKG_CONFIG_ALLOW_CROSS=1
  cc_tool=""
  linker_tool=""
  arch_prefix=""
  if [[ "$t" == *"-unknown-linux-gnu"* ]]; then
    # GNU libc toolchain
    arch_prefix=$(echo "$t" | sed -E 's/-unknown-linux-gnu$//')
    cc_tool="${arch_prefix}-linux-gnu-gcc"
    linker_tool="$cc_tool"
    if ! command -v "$cc_tool" >/dev/null 2>&1; then
      echo "    WARN: Missing cross C toolchain: $cc_tool (required by ring)." >&2
      if [[ "$host_os" == "Darwin" ]]; then
        echo "          Tip: prefer MUSL targets on macOS (install: brew install FiloSottile/musl-cross/musl-cross)" >&2
      fi
      echo "          Skipping $t." >&2
      continue
    fi
  elif [[ "$t" == *"-unknown-linux-musl"* ]]; then
    # MUSL toolchain is best for portable Linux binaries
    arch_prefix=$(echo "$t" | sed -E 's/-unknown-linux-musl$//')
    cc_tool="${arch_prefix}-linux-musl-gcc"
    linker_tool="$cc_tool"
    if ! command -v "$cc_tool" >/dev/null 2>&1; then
      echo "    WARN: Missing MUSL cross toolchain: $cc_tool (required by ring)." >&2
      if [[ "$host_os" == "Darwin" ]]; then
        echo "          Install via Homebrew: brew install FiloSottile/musl-cross/musl-cross" >&2
      fi
      echo "          Skipping $t." >&2
      continue
    fi
  elif [[ "$t" == *"-apple-darwin"* && "$host_os" != "Darwin" ]]; then
    # Cross-compiling to macOS from Linux typically requires osxcross
    # Try to find an osxcross clang for the architecture
    arch_prefix=$(echo "$t" | sed -E 's/-apple-darwin$//')
    # Probe PATH for any matching clang
    found=""
    IFS=: read -ra pths <<< "$PATH"
    for d in "${pths[@]}"; do
      for f in "$d/${arch_prefix}-apple-darwin"*-clang; do
        [[ -x "$f" ]] || continue
        found="$f"; break
      done
      [[ -n "$found" ]] && break
    done
    if [[ -z "$found" ]]; then
      echo "    WARN: No osxcross clang found for $t. Install osxcross and ensure <arch>-apple-darwin*-clang is on PATH. Skipping $t." >&2
      continue
    fi
    cc_tool="$found"
    linker_tool="$found"
  fi

  # If we have a cc/linker tool for this target, set env vars cargo/cc understands
  if [[ -n "$cc_tool" ]]; then
    cc_var="CC_${t//-/_}"
    linker_var="CARGO_TARGET_${t//-/_}_LINKER"
    # shellcheck disable=SC2163
    export "$cc_var"="$cc_tool"
    # shellcheck disable=SC2163
    export "$linker_var"="$linker_tool"
    echo "    Using CC via $cc_var=$cc_tool and $linker_var=$linker_tool"
  fi
  if ! rustup target list | grep -q "^${t} (installed)"; then
    echo "    Installing target $t via rustup..."
    rustup target add "$t"
  fi
  RUSTFLAGS="${RUSTFLAGS:-}" cargo build --release --target "$t" || {
    echo "    WARN: build failed for $t, skipping packaging" >&2
    continue
  }

  outdir="${manifest_dir}/${t}"
  mkdir -p "$outdir"
  bin_dir="target/${t}/release"
  cp -f "${bin_dir}/qq" "${outdir}/qq" || true
  cp -f "${bin_dir}/qa" "${outdir}/qa" || true
  # Include basic docs
  cp -f README.md "$outdir/README.md" || true
  cp -f LICENSE "$outdir/" 2>/dev/null || true

  # Package tar.gz per target
  tarball="${artifact_root}/qqqa-v${ver}-${t}.tar.gz"
  tar -C "$outdir" -czf "$tarball" . 2>/dev/null || {
    # Fallback: package what's available
    echo "    Packaging whatever binaries are present for $t"
    tar -C "$outdir" -czf "$tarball" .
  }

  # Cleanup per-target staging directory after packaging
  rm -rf "$outdir"
done

# Checksums and manifest
checksum_cmd=""
if command -v shasum >/dev/null 2>&1; then
  checksum_cmd="shasum -a 256"
elif command -v sha256sum >/dev/null 2>&1; then
  checksum_cmd="sha256sum"
fi

manifest_file="${artifact_root}/manifest.json"
date_iso=$(date -u +%Y-%m-%dT%H:%M:%SZ)

echo "==> Writing manifest ${manifest_file}"
{
  echo "{"
  echo "  \"version\": \"${ver}\"," 
  echo "  \"date\": \"${date_iso}\"," 
  if [[ -n "$git_sha" ]]; then
    echo "  \"git_sha\": \"${git_sha}\"," 
  fi
  echo "  \"artifacts\": ["
  first=1
  for f in "${manifest_dir}"/qqqa-v${ver}-*.tar.gz; do
    [[ -e "$f" ]] || continue
    name=$(basename "$f")
    csum=""
    if [[ -n "$checksum_cmd" ]]; then
      csum=$($checksum_cmd "$f" | awk '{print $1}')
    fi
    if [[ $first -eq 0 ]]; then echo ","; fi
    first=0
    echo -n "    { \"file\": \"${name}\""
    if [[ -n "$csum" ]]; then
      echo -n ", \"sha256\": \"${csum}\""
    fi
    echo -n " }"
  done
  echo
  echo "  ]"
  echo "}"
} > "$manifest_file"

echo "==> Done. Artifacts staged under ${artifact_root}/"
echo "    Remember to commit the version bump and tag the release:"
echo "      git add Cargo.toml && git commit -m 'release: v${ver}'"
if [[ -n "$git_sha" ]]; then
  echo "      git tag -a v${ver} ${git_sha} -m 'qqqa v${ver}'"
else
  echo "      git tag -a v${ver} -m 'qqqa v${ver}'"
fi
echo "      git push && git push --tags"
