#!/usr/bin/env bash
# Installs cloudflared from Cloudflare's apt repository and runs the tunnel `owlhead-api` as a
# service. Run as root, only once the API answers on http://127.0.0.1:8080:
#
#   bash install-cloudflared.sh --dry-run
#   bash install-cloudflared.sh
#
# It asks for the tunnel token (from the Cloudflare dashboard) without echoing it, and stores it as
# a systemd credential, /etc/owlhead/credentials/tunnel-token (root:root 0600; DEC-822 item 4),
# which the unit loads with LoadCredential= and cloudflared reads with --token-file. If that file
# already exists, the token is kept; delete the file first to replace it.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR source=lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
parse_flags "$@"
require_host

KEYRING=/usr/share/keyrings/cloudflare-main.gpg
TOKEN_FILE=/etc/owlhead/credentials/tunnel-token
# The primary key of https://pkg.cloudflare.com/cloudflare-main.gpg ("CloudFlare Software Packaging
# 2025 <help@cloudflare.com>"), read on 2026-10-08. Cloudflare's install instructions
# (pkg.cloudflare.com) fetch this file over HTTPS and publish no separate fingerprint; pinning it
# here means a swapped key stops the install. If Cloudflare rotates the key, this line changes in a
# reviewed PR, never on the host.
CLOUDFLARE_FINGERPRINT=CC94B39C77AE7342A68B89628A682D308D4E5E73

key_fingerprint() {
  gpg --show-keys --with-colons "$1" 2>/dev/null | awk -F: '$1 == "fpr" { print $10; exit }'
}

say "cloudflared from pkg.cloudflare.com"
if [ "$DRY_RUN" = 1 ]; then
  echo "+ fetch https://pkg.cloudflare.com/cloudflare-main.gpg, check its fingerprint is $CLOUDFLARE_FINGERPRINT, install it as $KEYRING"
elif [ ! -f "$KEYRING" ] || [ "$(key_fingerprint "$KEYRING")" != "$CLOUDFLARE_FINGERPRINT" ]; then
  fetched="$(mktemp)"
  curl -fsSL --proto '=https' --tlsv1.2 -o "$fetched" https://pkg.cloudflare.com/cloudflare-main.gpg
  found="$(key_fingerprint "$fetched")"
  if [ "$found" != "$CLOUDFLARE_FINGERPRINT" ]; then
    rm -f "$fetched"
    echo "Cloudflare's repository key is ${found:-unreadable}, not $CLOUDFLARE_FINGERPRINT; stopping" >&2
    exit 1
  fi
  install -o root -g root -m 0644 "$fetched" "$KEYRING"
  rm -f "$fetched"
fi
echo "deb [signed-by=$KEYRING] https://pkg.cloudflare.com/cloudflared noble main" |
  put_file /etc/apt/sources.list.d/cloudflared.list 0644 root:root
run apt-get update
run env DEBIAN_FRONTEND=noninteractive apt-get install -y cloudflared
# The unit passes the token by file, never in the environment or on a command line.
if [ "$DRY_RUN" = 0 ] && ! cloudflared tunnel run --help 2>/dev/null | grep -q -- '--token-file'; then
  echo "this cloudflared has no --token-file; update it from Cloudflare's repository" >&2
  exit 1
fi

say "The tunnel token"
run install -d -o root -g root -m 0700 /etc/owlhead
run install -d -o root -g root -m 0700 /etc/owlhead/credentials
if [ -f "$TOKEN_FILE" ]; then
  echo "keeping the existing $TOKEN_FILE"
elif [ "$DRY_RUN" = 1 ]; then
  echo "+ ask for the token and write $TOKEN_FILE (root:root 0600)"
else
  read -r -s -p "Paste the tunnel token, then press Enter: " token
  echo
  token="${token//[[:space:]]/}"
  # A tunnel token is one long base64 string; anything else is a paste of the wrong thing (for
  # example the whole `cloudflared service install ...` command).
  if ! [[ "$token" =~ ^[A-Za-z0-9+/_=-]{100,}$ ]]; then
    unset token
    echo "that does not look like a tunnel token (one base64 string); nothing written" >&2
    exit 1
  fi
  printf '%s' "$token" | put_file "$TOKEN_FILE" 0600 root:root
  unset token
fi

say "The cloudflared service"
put_file /etc/systemd/system/cloudflared.service 0644 root:root <"$DEPLOY_DIR/systemd/cloudflared.service"
run systemctl daemon-reload
# The unit denies all egress but loopback (DEC-822 item 5); the tunnel's edge is the one thing it
# may reach. Cloudflare publishes these two names for the tunnel's connections (port 7844).
if [ "$DRY_RUN" = 1 ]; then
  bash "$DEPLOY_DIR/allow-egress.sh" --dry-run cloudflared region1.v2.argotunnel.com region2.v2.argotunnel.com
else
  bash "$DEPLOY_DIR/allow-egress.sh" cloudflared region1.v2.argotunnel.com region2.v2.argotunnel.com
fi
run systemctl enable --now cloudflared.service
run systemctl restart cloudflared.service
echo "check it: systemctl status cloudflared; the dashboard shows the tunnel as HEALTHY"
