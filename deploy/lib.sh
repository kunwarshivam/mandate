#!/usr/bin/env bash
# Shared helpers for the deploy scripts. Sourced, never run. Every command that changes the host
# goes through `run`, so `--dry-run` prints the whole plan and changes nothing.
set -euo pipefail

DRY_RUN=0
# Set to 1 by put_file when it wrote a file whose content changed (or would, in a dry run).
CHANGED=0
DEPLOY_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export DEPLOY_DIR

parse_flags() {
  for arg in "$@"; do
    case "$arg" in
      --dry-run) DRY_RUN=1 ;;
      -h | --help)
        sed -n '2,/^set -euo/p' "$0" | sed '$d; s/^# \{0,1\}//'
        exit 0
        ;;
      *)
        echo "unknown argument: $arg" >&2
        exit 2
        ;;
    esac
  done
}

say() { printf '\n==> %s\n' "$*"; }

run() {
  if [ "$DRY_RUN" = 1 ]; then
    printf '+ %s\n' "$*"
  else
    "$@"
  fi
}

# Writes stdin to a file with an owner and mode, replacing it only when the content differs, and
# sets CHANGED=1 when it did. A dry run prints the content instead; never pass it a secret then.
put_file() {
  local dest="$1" mode="$2" owner="$3" tmp
  CHANGED=0
  tmp="$(mktemp)"
  cat >"$tmp"
  if ! cmp -s "$tmp" "$dest" 2>/dev/null; then
    CHANGED=1
  fi
  if [ "$DRY_RUN" = 1 ]; then
    printf '+ write %s (%s %s):\n' "$dest" "$owner" "$mode"
    sed 's/^/    /' "$tmp"
  elif [ "$CHANGED" = 1 ]; then
    install -o "${owner%%:*}" -g "${owner##*:}" -m "$mode" "$tmp" "$dest"
  else
    chown "$owner" "$dest"
    chmod "$mode" "$dest"
  fi
  rm -f "$tmp"
}

# Runs stdin as SQL in a database, as the postgres superuser, stopping at the first error.
sql() {
  local db="$1"
  if [ "$DRY_RUN" = 1 ]; then
    printf '+ psql -d %s <<SQL\n' "$db"
    sed 's/^/    /'
    printf '  SQL\n'
  else
    PGOPTIONS="-c client_min_messages=warning" runuser -u postgres -- psql --quiet -v ON_ERROR_STOP=1 -d "$db"
  fi
}

# One value from a query, as the postgres superuser; empty in a dry run.
query() {
  local db="$1" text="$2"
  if [ "$DRY_RUN" = 1 ]; then
    return 0
  fi
  runuser -u postgres -- psql -XtA -v ON_ERROR_STOP=1 -d "$db" -c "$text"
}

require_host() {
  # shellcheck source=/dev/null
  . /etc/os-release
  if [ "${ID:-}" != ubuntu ] || [ "${VERSION_ID:-}" != 24.04 ]; then
    echo "this runbook is for Ubuntu 24.04; found ${PRETTY_NAME:-unknown}" >&2
    exit 1
  fi
  if [ "$(uname -m)" != x86_64 ]; then
    echo "this runbook is for x86_64; found $(uname -m)" >&2
    exit 1
  fi
  if [ "$DRY_RUN" = 0 ] && [ "$(id -u)" != 0 ]; then
    echo "run as root (or with sudo); --dry-run needs no root" >&2
    exit 1
  fi
}

# Fails unless `mandate journal verify`'s result line verified the stream from seq 1 to the head that
# stream_heads records, ending at the recorded hash. A dump that lost a stream's last events still
# verifies as a shorter chain, so the head is what catches it.
check_head() {
  local verified="$1" stream="$2" seq="$3" hash="$4"
  local want="result: verified, stream $stream, seq 1 to $seq, last hash $hash"
  if [ "$verified" != "$want" ]; then
    printf 'stream %s does not end at its recorded head (seq %s, hash %s): %s\n' \
      "$stream" "$seq" "$hash" "${verified:-no result}" >&2
    return 1
  fi
}
