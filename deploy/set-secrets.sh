#!/usr/bin/env bash
# Sets the host's secrets, last, right before the first start (DEC-822 item 4). Run as root:
#
#   bash set-secrets.sh
#
# Exactly three kinds of secret live outside the vault, each a root:root 0600 file under
# /etc/owlhead/credentials/, which systemd hands to the units that need it with LoadCredential=:
#   - the vault's keys: vault-pending-key (the API and the executor) and vault-token-key (the
#     executor alone, DEC-692);
#   - session-signing-key (the API);
#   - tunnel-token (cloudflared), which install-cloudflared.sh writes.
# This script makes the first two kinds with openssl when they are missing, and leaves every one
# that exists alone. It asks for the Alpaca OAuth client id, which is public, and fills it into
# api.env and executor.env, which hold no secret. The Alpaca client secret goes into the vault
# through V1's import, read from stdin, and is written nowhere under /etc/owlhead; until V1 is
# installed, that step is skipped and said so. It never prints a value and never passes one on a
# command line.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR source=lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
parse_flags "$@"
require_host

API_ENV=/etc/owlhead/api.env
EXEC_ENV=/etc/owlhead/executor.env
CREDENTIALS=/etc/owlhead/credentials
KEYS=(vault-pending-key vault-token-key session-signing-key)
VAULT_IMPORT=(/usr/local/bin/mandate vault import alpaca-oauth-client-secret)
if [ "$DRY_RUN" = 1 ]; then
  echo "+ make each missing key in $CREDENTIALS (root:root 0600) with openssl; ask for Alpaca's client id and fill it into $API_ENV and $EXEC_ENV; import the client secret into the vault once V1 is installed"
  exit 0
fi
for file in "$API_ENV" "$EXEC_ENV" "$CREDENTIALS"; do
  if [ ! -e "$file" ]; then
    echo "$file does not exist; run bootstrap.sh first" >&2
    exit 1
  fi
done

# Reads one answer, or stops with a clear message when stdin is empty or closed.
ask() {
  local prompt="$1" silent="$2"
  if [ "$silent" = 1 ]; then
    IFS= read -r -s -p "$prompt" answer || { echo; echo "no input; nothing written" >&2; exit 1; }
    echo
  else
    IFS= read -r -p "$prompt" answer || { echo "no input; nothing written" >&2; exit 1; }
  fi
  answer="${answer//[[:space:]]/}"
  if [ -z "$answer" ]; then
    echo "an empty answer; nothing written" >&2
    exit 1
  fi
}

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
for key in "${KEYS[@]}"; do
  if [ ! -s "$CREDENTIALS/$key" ]; then
    # The value goes from openssl straight into the file, through no variable.
    openssl rand -base64 32 | put_file "$CREDENTIALS/$key" 0600 root:root
  else
    run chown root:root "$CREDENTIALS/$key"
    run chmod 0600 "$CREDENTIALS/$key"
  fi
done

client_id="$(current "$API_ENV" ALPACA_OAUTH_CLIENT_ID)"
client_id="${client_id:-$(current "$EXEC_ENV" ALPACA_OAUTH_CLIENT_ID)}"
if [ -z "$client_id" ]; then
  ask "Alpaca OAuth client id: " 0
  client_id="$answer"
fi
unset answer
fill "$API_ENV" ALPACA_OAUTH_CLIENT_ID "$client_id"
fill "$EXEC_ENV" ALPACA_OAUTH_CLIENT_ID "$client_id"
unset client_id

# The client secret: into the vault, as owlhead_exec, with the executor's credentials, through
# stdin only. Nothing of it is written under /etc/owlhead.
if [ -x "${VAULT_IMPORT[0]}" ] && "${VAULT_IMPORT[0]}" vault import --help >/dev/null 2>&1; then
  ask "Alpaca OAuth client secret (not echoed): " 1
  printf '%s' "$answer" | systemd-run --quiet --wait --pipe --collect \
    -p User=owlhead_exec -p Group=owlhead_exec -p UMask=0077 \
    -p LoadCredential=vault-pending-key:"$CREDENTIALS/vault-pending-key" \
    -p LoadCredential=vault-token-key:"$CREDENTIALS/vault-token-key" \
    -p EnvironmentFile="$EXEC_ENV" \
    "${VAULT_IMPORT[@]}"
  unset answer
else
  echo "the Alpaca client secret: skipped until V1 lands (\`mandate vault import\`); run this script again then"
fi

problems=""
for file in "$API_ENV" "$EXEC_ENV"; do
  while IFS= read -r line; do
    case "$line" in
      "" | "#"*) ;;
      *)
        # Only the name is ever reported; a value never leaves the file.
        if [ -z "${line#*=}" ]; then
          problems="$problems ${file##*/}:${line%%=*}(empty)"
        fi
        if [[ "${line%%=*}" =~ (KEY|SECRET|TOKEN|PASSWORD) ]]; then
          problems="$problems ${file##*/}:${line%%=*}(a secret in an environment file; DEC-822 item 4)"
        fi
        ;;
    esac
  done <"$file"
done
for found in "$CREDENTIALS"/*; do
  name="${found##*/}"
  case " ${KEYS[*]} tunnel-token " in
    *" $name "*) ;;
    *) problems="$problems credentials/$name(not one of the three kinds)" ;;
  esac
done
if [ -n "$problems" ]; then
  echo "fix by hand:$problems" >&2
  exit 1
fi
echo "every key is in $CREDENTIALS (root:root 0600) and the client id in both env files. Copy both vault keys to your password manager now: they are never in a backup."
