#!/usr/bin/env bash
set -euo pipefail

# Simple release builder for qqqa
# - Bumps Cargo.toml version
# - Builds binaries for common targets
# - Packages tar.gz artifacts under releases/v<version>/
# - Writes checksums and a minimal manifest.json
# - Keeps last 3 versions under releases/
# - Optionally tags the repo at a given SHA
#
# Usage:
#   scripts/release.sh v0.7.0 [<git_sha>] [TARGETS="..."]
#
# Defaults TARGETS:
#   x86_64-apple-darwin aarch64-apple-darwin x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu

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
targets_default=(
  x86_64-apple-darwin
  aarch64-apple-darwin
  x86_64-unknown-linux-gnu
  aarch64-unknown-linux-gnu
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

for t in "${targets[@]}"; do
  echo "--> Target: $t"
  if ! rustup target list | grep -q "^${t} (installed)"; then
    echo "    Installing target $t via rustup..."
    rustup target add "$t"
  fi
  RUSTFLAGS="${RUSTFLAGS:-}" cargo build --release --target "$t" || {
    echo "    WARN: build failed for $t, skipping packaging" >&2
    continue
  }

  outdir="releases/v${ver}/${t}"
  mkdir -p "$outdir"
  bin_dir="target/${t}/release"
  cp -f "${bin_dir}/qq" "${outdir}/qq" || true
  cp -f "${bin_dir}/qa" "${outdir}/qa" || true
  # Include basic docs
  cp -f README.md "$outdir/README.md" || true
  cp -f LICENSE "$outdir/" 2>/dev/null || true

  # Package tar.gz per target
  tarball="releases/qqqa-v${ver}-${t}.tar.gz"
  tar -C "$outdir" -czf "$tarball" . 2>/dev/null || {
    # Fallback: package what's available
    echo "    Packaging whatever binaries are present for $t"
    tar -C "$outdir" -czf "$tarball" .
  }
done

# Checksums and manifest
checksum_cmd=""
if command -v shasum >/dev/null 2>&1; then
  checksum_cmd="shasum -a 256"
elif command -v sha256sum >/dev/null 2>&1; then
  checksum_cmd="sha256sum"
fi

manifest_dir="releases/v${ver}"
mkdir -p "$manifest_dir"
manifest_file="${manifest_dir}/manifest.json"
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
  for f in releases/qqqa-v${ver}-*.tar.gz; do
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

# Update index.json: keep last 3 versions
echo "==> Updating releases/index.json (keep last 3)"
index_file="releases/index.json"
versions=( $(ls -1 releases | grep '^v' | sort -Vr | head -n 3) )
{
  echo "["
  first=1
  for v in "${versions[@]}"; do
    [[ -f "releases/${v}/manifest.json" ]] || continue
    if [[ $first -eq 0 ]]; then echo ","; fi
    first=0
    cat "releases/${v}/manifest.json"
  done
  echo
  echo "]"
} > "$index_file"

# Prune older release directories
all=( $(ls -1 releases | grep '^v' | sort -V) ) || true
to_keep_set=" ${versions[*]} "
for v in "${all[@]}"; do
  if [[ " $to_keep_set " != *" $v "* ]]; then
    echo "Pruning releases/$v"
    rm -rf "releases/$v"
  fi
done

# Prune orphaned tarballs not in kept versions
for tb in releases/qqqa-v*.tar.gz; do
  [[ -e "$tb" ]] || continue
  b=$(basename "$tb")
  vt=$(echo "$b" | sed -E 's/^qqqa-(v[0-9][^ -]*)-.*/\1/')
  if [[ " $to_keep_set " != *" $vt "* ]]; then
    echo "Pruning tarball $b"
    rm -f "$tb"
  fi
done

echo "==> Done. Artifacts in releases/ and index at releases/index.json"
echo "    Remember to commit changes and tag the release:"
echo "      git add Cargo.toml releases/ && git commit -m 'release: v${ver}'"
if [[ -n "$git_sha" ]]; then
  echo "      git tag -a v${ver} ${git_sha} -m 'qqqa v${ver}'"
else
  echo "      git tag -a v${ver} -m 'qqqa v${ver}'"
fi
echo "      git push && git push --tags"
