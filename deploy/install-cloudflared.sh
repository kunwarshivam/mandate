#!/usr/bin/env bash
# Installs cloudflared from Cloudflare's apt repository and runs the tunnel `owlhead-api` as a
# service. Run as root, only once the API answers on http://127.0.0.1:8080:
#
#   bash install-cloudflared.sh --dry-run
#   bash install-cloudflared.sh
#
# It asks for the tunnel token (from the Cloudflare dashboard) without echoing it, and stores it in
# /etc/owlhead/cloudflared.env (root:root 0600). If that file already exists, the token is kept;
# delete the file first to replace it.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR source=lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
parse_flags "$@"
require_host

KEYRING=/usr/share/keyrings/cloudflare-main.gpg
TOKEN_FILE=/etc/owlhead/cloudflared.env

say "cloudflared from pkg.cloudflare.com"
if [ ! -f "$KEYRING" ]; then
  run curl -fsSL --proto '=https' --tlsv1.2 -o "$KEYRING" https://pkg.cloudflare.com/cloudflare-main.gpg
fi
if [ "$DRY_RUN" = 0 ]; then
  echo "repository key fingerprint (compare with Cloudflare's documentation):"
  gpg --show-keys --with-colons "$KEYRING" | awk -F: '$1 == "fpr" { print "  " $10 }'
fi
echo "deb [signed-by=$KEYRING] https://pkg.cloudflare.com/cloudflared noble main" |
  put_file /etc/apt/sources.list.d/cloudflared.list 0644 root:root
run apt-get update
run env DEBIAN_FRONTEND=noninteractive apt-get install -y cloudflared

say "The tunnel token"
run install -d -o root -g root -m 0700 /etc/owlhead
if [ -f "$TOKEN_FILE" ]; then
  echo "keeping the existing $TOKEN_FILE"
elif [ "$DRY_RUN" = 1 ]; then
  echo "+ ask for the token and write $TOKEN_FILE (root:root 0600)"
else
  read -r -s -p "Paste the tunnel token, then press Enter: " token
  echo
  if [ -z "$token" ]; then
    echo "no token given; nothing written" >&2
    exit 1
  fi
  printf 'TUNNEL_TOKEN=%s\n' "$token" | put_file "$TOKEN_FILE" 0600 root:root
  unset token
fi

say "The cloudflared service"
put_file /etc/systemd/system/cloudflared.service 0644 root:root <"$DEPLOY_DIR/systemd/cloudflared.service"
run systemctl daemon-reload
run systemctl enable --now cloudflared.service
run systemctl restart cloudflared.service
echo "check it: systemctl status cloudflared; the dashboard shows the tunnel as HEALTHY"
