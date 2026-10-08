#!/usr/bin/env bash
# Fills /etc/owlhead/api.env and /etc/owlhead/executor.env in place, last, right before the first
# start. Run as root:
#
#   bash set-secrets.sh
#
# It makes each random key on the host with openssl, asks for the Alpaca OAuth client id and secret
# (the secret without echo), and leaves every value that is already set alone. The session key goes
# to api.env only, the client secret to executor.env only (DEC-692), the vault key and the client id
# to both. It never prints a value, never passes one on a command line, and keeps both files
# root:root 0600.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR source=lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
parse_flags "$@"
require_host

API_ENV=/etc/owlhead/api.env
EXEC_ENV=/etc/owlhead/executor.env
for file in "$API_ENV" "$EXEC_ENV"; do
  if [ ! -f "$file" ]; then
    echo "$file does not exist; run bootstrap.sh first" >&2
    exit 1
  fi
done
if [ "$DRY_RUN" = 1 ]; then
  echo "+ fill the empty values of $API_ENV and $EXEC_ENV: random keys from openssl, Alpaca's id and secret asked for"
  exit 0
fi

current() { sed -n "s/^$2=//p" "$1"; }

# Rewrites the named values of one file, each only where it is empty, through a 0600 temporary file.
fill() {
  local file="$1" rebuilt line name
  shift
  declare -A values=()
  while [ "$#" -gt 0 ]; do
    values[$1]="$2"
    shift 2
  done
  rebuilt="$(mktemp)"
  while IFS= read -r line || [ -n "$line" ]; do
    name="${line%%=*}"
    if [ "$line" = "$name=" ] && [ -n "${values[$name]:-}" ]; then
      printf '%s=%s\n' "$name" "${values[$name]}" >>"$rebuilt"
    else
      printf '%s\n' "$line" >>"$rebuilt"
    fi
  done <"$file"
  install -o root -g root -m 0600 "$rebuilt" "$file"
  rm -f "$rebuilt"
}

umask 077
vault_key="$(current "$API_ENV" MANDATE_VAULT_KEY)"
exec_key="$(current "$EXEC_ENV" MANDATE_VAULT_KEY)"
if [ -n "$vault_key" ] && [ -n "$exec_key" ] && [ "$vault_key" != "$exec_key" ]; then
  unset vault_key exec_key
  echo "the two files hold different vault keys; fix one by hand before going on" >&2
  exit 1
fi
vault_key="${vault_key:-$exec_key}"
vault_key="${vault_key:-$(openssl rand -base64 32)}"
session_key="$(openssl rand -base64 32)"

client_id="$(current "$API_ENV" ALPACA_OAUTH_CLIENT_ID)"
client_id="${client_id:-$(current "$EXEC_ENV" ALPACA_OAUTH_CLIENT_ID)}"
if [ -z "$client_id" ]; then
  read -r -p "Alpaca OAuth client id: " client_id
  client_id="${client_id//[[:space:]]/}"
fi
client_secret=""
if [ -z "$(current "$EXEC_ENV" ALPACA_OAUTH_CLIENT_SECRET)" ]; then
  read -r -s -p "Alpaca OAuth client secret (not echoed): " client_secret
  echo
  client_secret="${client_secret//[[:space:]]/}"
fi

fill "$API_ENV" MANDATE_API_SESSION_SIGNING_KEY "$session_key" MANDATE_VAULT_KEY "$vault_key" \
  ALPACA_OAUTH_CLIENT_ID "$client_id"
fill "$EXEC_ENV" MANDATE_VAULT_KEY "$vault_key" ALPACA_OAUTH_CLIENT_ID "$client_id" \
  ALPACA_OAUTH_CLIENT_SECRET "$client_secret"
unset vault_key exec_key session_key client_id client_secret

missing=""
for file in "$API_ENV" "$EXEC_ENV"; do
  while IFS= read -r line; do
    case "$line" in
      "" | "#"*) ;;
      *)
        # Only the name is ever reported; a value never leaves the file.
        if [ -z "${line#*=}" ]; then
          missing="$missing ${file##*/}:${line%%=*}"
        fi
        ;;
    esac
  done <"$file"
done
if grep -q '^ALPACA_OAUTH_CLIENT_SECRET=' "$API_ENV"; then
  echo "$API_ENV must not hold the client secret (DEC-692); remove that line" >&2
  exit 1
fi
if [ -n "$missing" ]; then
  echo "still empty:$missing" >&2
  exit 1
fi
echo "every value is set in $API_ENV and $EXEC_ENV (root:root 0600). Copy MANDATE_VAULT_KEY to your password manager now."
