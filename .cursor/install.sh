#!/usr/bin/env bash
# Cloud Agent install step: the toolchain and tools pinned by ADR-0001, then a warm build.
# Idempotent: each tool is installed only if the pinned version is missing, and every download is
# checked against a pinned SHA-256. Versions must match .github/workflows/ci.yml.
set -euo pipefail

cd "$(dirname "$0")/.."

UV_VERSION="0.12.19"
PYTHON_VERSION="3.14"
BIN="${CARGO_HOME:-$HOME/.cargo}/bin"
mkdir -p "$BIN" "$HOME/.local/bin"
export PATH="$BIN:$HOME/.local/bin:$PATH"

# tool version url sha256 member
TOOLS=(
  "cargo-deny 0.20.2 https://github.com/EmbarkStudios/cargo-deny/releases/download/0.20.2/cargo-deny-0.20.2-x86_64-unknown-linux-musl.tar.gz 9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f cargo-deny-0.20.2-x86_64-unknown-linux-musl/cargo-deny"
  "cargo-nextest 0.9.146 https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-0.9.146/cargo-nextest-0.9.146-x86_64-unknown-linux-gnu.tar.gz 682c21b777c333e96fd532e114d3a5a894e0729ab88d94c0a9f20f8419695428 cargo-nextest"
  "typos 1.50.2 https://github.com/crate-ci/typos/releases/download/v1.50.2/typos-v1.50.2-x86_64-unknown-linux-musl.tar.gz abcb3e257c7c2abeff4d903f7fe68071357637605bdb283ce2251f44bc70dc09 ./typos"
  "gitleaks 8.30.1 https://github.com/gitleaks/gitleaks/releases/download/v8.30.1/gitleaks_8.30.1_linux_x64.tar.gz 551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb gitleaks"
)

installed_version() {
  case "$1" in
    cargo-deny) cargo-deny --version 2>/dev/null | awk '{print $2}' ;;
    cargo-nextest) cargo-nextest nextest --version 2>/dev/null | awk 'NR==1 {print $2}' ;;
    typos) typos --version 2>/dev/null | awk '{print $2}' ;;
    gitleaks) gitleaks version 2>/dev/null ;;
  esac
}

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# Rust: rustup reads rust-toolchain.toml (1.98.1 with rustfmt, clippy, llvm-tools).
if ! command -v rustup >/dev/null; then
  echo "rustup is required (the Cursor default image provides it)" >&2
  exit 1
fi
rustup toolchain install

for entry in "${TOOLS[@]}"; do
  read -r name version url sha member <<<"$entry"
  if [ "$(installed_version "$name")" = "$version" ]; then
    continue
  fi
  echo "installing $name $version"
  curl -sSfL -o "$tmp/$name.tgz" "$url"
  echo "$sha  $tmp/$name.tgz" | sha256sum --check --quiet
  tar -xzf "$tmp/$name.tgz" -C "$tmp" "$member"
  install -m 0755 "$tmp/$member" "$BIN/$name"
done

if [ "$(uv --version 2>/dev/null | awk '{print $2}')" != "$UV_VERSION" ]; then
  echo "installing uv $UV_VERSION"
  curl -LsSf "https://astral.sh/uv/$UV_VERSION/install.sh" | env UV_NO_MODIFY_PATH=1 sh
fi
uv python install "$PYTHON_VERSION"

# Warm state derived from the checkout: locked dependencies, the xtask binary, and the reference
# implementation's pinned environment.
cargo fetch --locked
cargo build --locked --package xtask
uv sync --locked --directory python
uv run --no-project --python "$PYTHON_VERSION" --with-requirements reference/mandate/requirements.txt python -c "import yaml, jsonschema"
