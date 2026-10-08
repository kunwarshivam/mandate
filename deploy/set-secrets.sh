#!/usr/bin/env bash
# Fills /etc/owlhead/api.env in place, last, right before the API's first start. Run as root:
#
#   bash set-secrets.sh
#
# It makes each random key on the host with openssl, asks for the Alpaca OAuth client id and secret
# (the secret without echo), and leaves every value that is already set alone. It never prints a
# value, never passes one on a command line, and keeps the file root:root 0600.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR source=lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
parse_flags "$@"
require_host

ENV_FILE=/etc/owlhead/api.env
if [ ! -f "$ENV_FILE" ]; then
  echo "$ENV_FILE does not exist; run bootstrap.sh first" >&2
  exit 1
fi
if [ "$DRY_RUN" = 1 ]; then
  echo "+ fill the empty values of $ENV_FILE: random keys from openssl, Alpaca's id and secret asked for"
  exit 0
fi

declare -A fill=()
current() { sed -n "s/^$1=//p" "$ENV_FILE"; }

for name in MANDATE_API_SESSION_SIGNING_KEY MANDATE_VAULT_KEY; do
  if [ -z "$(current "$name")" ]; then
    fill[$name]="$(openssl rand -base64 32)"
  fi
done
if [ -z "$(current ALPACA_OAUTH_CLIENT_ID)" ]; then
  read -r -p "Alpaca OAuth client id: " value
  fill[ALPACA_OAUTH_CLIENT_ID]="${value//[[:space:]]/}"
fi
if [ -z "$(current ALPACA_OAUTH_CLIENT_SECRET)" ]; then
  read -r -s -p "Alpaca OAuth client secret (not echoed): " value
  echo
  fill[ALPACA_OAUTH_CLIENT_SECRET]="${value//[[:space:]]/}"
fi
unset value

umask 077
rebuilt="$(mktemp)"
while IFS= read -r line || [ -n "$line" ]; do
  name="${line%%=*}"
  if [[ "$line" == *=* ]] && [ -n "${fill[$name]:-}" ]; then
    printf '%s=%s\n' "$name" "${fill[$name]}" >>"$rebuilt"
  else
    printf '%s\n' "$line" >>"$rebuilt"
  fi
done <"$ENV_FILE"
install -o root -g root -m 0600 "$rebuilt" "$ENV_FILE"
rm -f "$rebuilt"

missing=""
for name in MANDATE_API_DATABASE_URL MANDATE_API_SESSION_SIGNING_KEY MANDATE_VAULT_KEY ALPACA_OAUTH_CLIENT_ID ALPACA_OAUTH_CLIENT_SECRET; do
  if [ -z "$(current "$name")" ]; then
    missing="$missing $name"
  fi
done
if [ -n "$missing" ]; then
  echo "still empty:$missing" >&2
  exit 1
fi
echo "every value in $ENV_FILE is set (root:root 0600). Copy MANDATE_VAULT_KEY to your password manager now."
