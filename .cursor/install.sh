#!/usr/bin/env bash
# Cloud Agent install step: the toolchain and tools pinned by ADR-0001, then a warm build.
# Idempotent: each tool is installed only if the pinned version is missing, and every download is
# checked against a pinned SHA-256. Versions must match .github/workflows/ci.yml.
# Some cloud sessions allow only the package hosts (github.com, static.rust-lang.org, crates.io,
# PyPI), so nothing here may be fetched from astral.sh.
set -euo pipefail

cd "$(dirname "$0")/.."

UV_VERSION="0.12.19"
UV_SHA256="23bf5552d220e0842b65c862097b2ebaeba0064b74eda5e565e77fd25969d8c8"
PYTHON_VERSION="3.14"
BIN="${CARGO_HOME:-$HOME/.cargo}/bin"
mkdir -p "$BIN" "$HOME/.local/bin"
export PATH="$BIN:$HOME/.local/bin:$PATH"
# uv defaults to releases.astral.sh for Python builds. It still checks each build against the
# SHA-256 compiled into the pinned uv binary.
export UV_PYTHON_INSTALL_MIRROR="https://github.com/astral-sh/python-build-standalone/releases/download"

# tool version url sha256 member
TOOLS=(
  "cargo-deny 0.20.2 https://github.com/EmbarkStudios/cargo-deny/releases/download/0.20.2/cargo-deny-0.20.2-x86_64-unknown-linux-musl.tar.gz 9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f cargo-deny-0.20.2-x86_64-unknown-linux-musl/cargo-deny"
  "cargo-nextest 0.9.146 https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-0.9.146/cargo-nextest-0.9.146-x86_64-unknown-linux-gnu.tar.gz 682c21b777c333e96fd532e114d3a5a894e0729ab88d94c0a9f20f8419695428 cargo-nextest"
  "typos 1.50.2 https://github.com/crate-ci/typos/releases/download/v1.50.2/typos-v1.50.2-x86_64-unknown-linux-musl.tar.gz abcb3e257c7c2abeff4d903f7fe68071357637605bdb283ce2251f44bc70dc09 ./typos"
  "gitleaks 8.30.1 https://github.com/gitleaks/gitleaks/releases/download/v8.30.1/gitleaks_8.30.1_linux_x64.tar.gz 551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb gitleaks"
  "cargo-mutants 27.1.0 https://github.com/sourcefrog/cargo-mutants/releases/download/v27.1.0/cargo-mutants-x86_64-unknown-linux-gnu.tar.gz dfe6dc37d0342c891d2829b5a695aa57c2d0edecef7e7d0399a30cc6e206411e cargo-mutants"
)

installed_version() {
  case "$1" in
    cargo-deny) cargo-deny --version 2>/dev/null | awk '{print $2}' ;;
    cargo-nextest) cargo-nextest nextest --version 2>/dev/null | awk 'NR==1 {print $2}' ;;
    typos) typos --version 2>/dev/null | awk '{print $2}' ;;
    gitleaks) gitleaks version 2>/dev/null ;;
    cargo-mutants) cargo-mutants mutants --version 2>/dev/null | awk '{print $2}' ;;
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

if [ "$(uv --version 2>/dev/null | awk '{print $2}')" != "$UV_VERSION" ] ||
  [ "$(uvx --version 2>/dev/null | awk '{print $2}')" != "$UV_VERSION" ]; then
  echo "installing uv $UV_VERSION"
  uv_member="uv-x86_64-unknown-linux-gnu"
  curl -sSfL -o "$tmp/uv.tgz" "https://github.com/astral-sh/uv/releases/download/$UV_VERSION/$uv_member.tar.gz"
  echo "$UV_SHA256  $tmp/uv.tgz" | sha256sum --check --quiet
  tar -xzf "$tmp/uv.tgz" -C "$tmp" "$uv_member/uv" "$uv_member/uvx"
  install -m 0755 "$tmp/$uv_member/uv" "$tmp/$uv_member/uvx" "$HOME/.local/bin/"
fi

# The export above reaches only this script, and .cursor/environment.json cannot set environment
# variables, so the mirror goes into uv's user config for every later uv call in the session. A
# top-level key is prepended because TOML allows none after the first table; a mirror the user
# already set is left alone.
uv_config="${XDG_CONFIG_HOME:-$HOME/.config}/uv/uv.toml"
if ! grep -qs '^[[:space:]]*python-install-mirror[[:space:]]*=' "$uv_config"; then
  mkdir -p "$(dirname "$uv_config")"
  { echo "python-install-mirror = \"$UV_PYTHON_INSTALL_MIRROR\""; cat "$uv_config" 2>/dev/null || true; } >"$tmp/uv.toml"
  mv "$tmp/uv.toml" "$uv_config"
fi
uv python install "$PYTHON_VERSION"

# PostgreSQL for the Postgres journal tests (ADR-0001 ES-08, crates/mandate-journal-pg/README.md).
# Best effort: apt.postgresql.org is not reachable from every session, and without a database those
# tests skip. CI runs them against its own service container either way.
PG_MAJOR=18
SUDO=""
[ "$(id -u)" -eq 0 ] || SUDO="sudo -n"
as_postgres() {
  if [ -z "$SUDO" ]; then runuser -u postgres -- "$@"; else $SUDO -u postgres "$@"; fi
}
install_postgres() {
  $SUDO apt-get install -y -qq postgresql-common &&
    $SUDO /usr/share/postgresql-common/pgdg/apt.postgresql.org.sh -y &&
    $SUDO apt-get install -y -qq "postgresql-$PG_MAJOR"
}
if [ ! -x "/usr/lib/postgresql/$PG_MAJOR/bin/postgres" ] && ! install_postgres; then
  echo "warning: PostgreSQL $PG_MAJOR not installed; the Postgres journal tests will skip" >&2
fi
if [ -x "/usr/lib/postgresql/$PG_MAJOR/bin/postgres" ]; then
  $SUDO pg_ctlcluster "$PG_MAJOR" main start 2>/dev/null || true
  as_postgres psql -qc "ALTER USER postgres PASSWORD 'postgres'" ||
    echo "warning: could not set the local postgres password (see the journal README)" >&2
  echo "Postgres journal tests: export MANDATE_PG_URL=postgres://postgres:postgres@localhost:5432/postgres"
fi

# Cursor's agent hooks add the invoking user's email as a Co-authored-by trailer on agent commits.
# The founder does not want that address in the history, and CI rejects the trailer (spec guard).
for hook in "$HOME"/.cursor/agent-hooks/*/commit-msg.cursor.co-author; do
  [ -e "$hook" ] && chmod -x "$hook"
done

# Warm state derived from the checkout: locked dependencies, the xtask binary, and the reference
# implementation's pinned environment.
cargo fetch --locked
cargo build --locked --package xtask
uv sync --locked --directory python
uv run --no-project --python "$PYTHON_VERSION" --with-requirements reference/mandate/requirements.txt python -c "import yaml, jsonschema"
