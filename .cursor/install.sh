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
  "shellcheck 0.11.0 https://github.com/koalaman/shellcheck/releases/download/v0.11.0/shellcheck-v0.11.0.linux.x86_64.tar.gz b7af85e41cc99489dcc21d66c6d5f3685138f06d34651e6d34b42ec6d54fe6f6 shellcheck-v0.11.0/shellcheck"
  "actionlint 1.7.12 https://github.com/rhysd/actionlint/releases/download/v1.7.12/actionlint_1.7.12_linux_amd64.tar.gz 8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8 actionlint"
)

# The local cell's tools (D-4, DEC-969; `cargo xtask cell`), with the versions in ci.yml, and flux
# for the founder's bootstrap of a hosted cluster (`cargo xtask gitops`, E21-58, DEC-2018). k3d is a
# bare binary; helm publishes its binaries only on get.helm.sh (DEC-993 item 3), so it is fetched
# best effort, like PostgreSQL below: without it the cell's checks skip.
K3D_VERSION="5.9.0"
K3D_SHA256="06d8f25bc3a971c4eb29e0ff08429b180402db0f4dec838c9eac427e296800a0"
HELM_VERSION="4.3.0"
HELM_SHA256="86584a54def73570558f66f5111cc53dfed56689637ae32c1201205d494f54fb"
TOOLS+=(
  "kubeconform 0.8.0 https://github.com/yannh/kubeconform/releases/download/v0.8.0/kubeconform-linux-amd64.tar.gz 9bc2bffbf71f261128533edaf912153948b7ff238f9a531ae6d34466ec287883 kubeconform"
  "flux 2.9.6 https://github.com/fluxcd/flux2/releases/download/v2.9.6/flux_2.9.6_linux_amd64.tar.gz b4d22673e9246cbd628881f1a9ef3b090085dced291e42d804555cee8e8d42c5 flux"
)

# The hosted environments' OpenTofu and its linter (E21-56, DEC-2012 item 2), with the versions in
# ci.yml. tflint publishes only zip archives, so it is unpacked with Python's zipfile.
TOOLS+=(
  "tofu 1.13.1 https://github.com/opentofu/opentofu/releases/download/v1.13.1/tofu_1.13.1_linux_amd64.tar.gz 378ada19d4bc70c43732004e8159be771b23b9a5afdf059e5f8a2b3fa2c70a69 tofu"
)
TFLINT_VERSION="0.64.0"
TFLINT_SHA256="cca9d13e2e1d7a2c627af60ff899a3c9b74212899416aeb96ec764d2ef954537"

installed_version() {
  case "$1" in
    cargo-deny) cargo-deny --version 2>/dev/null | awk '{print $2}' ;;
    cargo-nextest) cargo-nextest nextest --version 2>/dev/null | awk 'NR==1 {print $2}' ;;
    typos) typos --version 2>/dev/null | awk '{print $2}' ;;
    gitleaks) gitleaks version 2>/dev/null ;;
    shellcheck) shellcheck --version 2>/dev/null | awk '/version:/ {print $2}' ;;
    actionlint) actionlint -version 2>/dev/null | awk 'NR==1 {print $1}' ;;
    flux) flux version --client 2>/dev/null | awk '{sub(/^v/, "", $2); print $2}' ;;
    cargo-mutants) cargo-mutants mutants --version 2>/dev/null | awk '{print $2}' ;;
    kubeconform) kubeconform -v 2>/dev/null | sed 's/^v//' ;;
    tofu) tofu version 2>/dev/null | awk 'NR==1 {print $2}' | sed 's/^v//' ;;
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

if [ "$(tflint --version 2>/dev/null | awk 'NR==1 {print $3}')" != "$TFLINT_VERSION" ]; then
  echo "installing tflint $TFLINT_VERSION"
  curl -sSfL -o "$tmp/tflint.zip" "https://github.com/terraform-linters/tflint/releases/download/v$TFLINT_VERSION/tflint_linux_amd64.zip"
  echo "$TFLINT_SHA256  $tmp/tflint.zip" | sha256sum --check --quiet
  python3 -I -c 'import sys, zipfile; zipfile.ZipFile(sys.argv[1]).extract("tflint", sys.argv[2])' "$tmp/tflint.zip" "$tmp"
  install -m 0755 "$tmp/tflint" "$BIN/tflint"
fi

if [ "$(k3d version 2>/dev/null | awk 'NR==1 {print $3}')" != "v$K3D_VERSION" ]; then
  echo "installing k3d $K3D_VERSION"
  curl -sSfL -o "$tmp/k3d" "https://github.com/k3d-io/k3d/releases/download/v$K3D_VERSION/k3d-linux-amd64"
  echo "$K3D_SHA256  $tmp/k3d" | sha256sum --check --quiet
  install -m 0755 "$tmp/k3d" "$BIN/k3d"
fi
if [ "$(helm version --template '{{.Version}}' 2>/dev/null)" != "v$HELM_VERSION" ]; then
  echo "installing helm $HELM_VERSION"
  if curl -sSfL -o "$tmp/helm.tgz" "https://get.helm.sh/helm-v$HELM_VERSION-linux-amd64.tar.gz"; then
    echo "$HELM_SHA256  $tmp/helm.tgz" | sha256sum --check --quiet
    tar -xzf "$tmp/helm.tgz" -C "$tmp" linux-amd64/helm
    install -m 0755 "$tmp/linux-amd64/helm" "$BIN/helm"
  else
    echo "warning: get.helm.sh is not reachable; helm not installed, so the cell's checks will skip" >&2
  fi
fi

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
  $SUDO apt-get update -qq &&
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
# Disabling it keeps that address off agent commits; CI no longer checks for it (DEC-2601).
for hook in "$HOME"/.cursor/agent-hooks/*/commit-msg.cursor.co-author; do
  [ -e "$hook" ] && chmod -x "$hook"
done

# Warm state derived from the checkout: locked dependencies, the xtask binary, and the reference
# implementation's pinned environment.
cargo fetch --locked
cargo build --locked --package xtask
uv sync --locked --directory python
uv run --no-project --python "$PYTHON_VERSION" --with-requirements reference/mandate/requirements.txt python -c "import yaml, jsonschema"
