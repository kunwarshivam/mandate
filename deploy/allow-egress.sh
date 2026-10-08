#!/usr/bin/env bash
# Sets the outbound allow list of one service (DEC-822 item 5). Every unit here denies all egress
# (IPAddressDeny=any) and allows loopback; this script adds the addresses of the hosts that one
# service must reach, as a drop-in under /etc/systemd/system/<unit>.service.d/. Run as root:
#
#   bash allow-egress.sh --dry-run owlhead-api <supabase-project-host>
#   bash allow-egress.sh owlhead-api <supabase-project-host>
#   bash allow-egress.sh cloudflared region1.v2.argotunnel.com region2.v2.argotunnel.com
#
# Pass every host the service needs each time: the drop-in is replaced, not extended. The units
# can filter by address only, not by name or port, so the list is the addresses the names resolve
# to now. A CDN-hosted name can move; when the service stops reaching its host, run this again.
# That is the residual of IP-based egress control (deploy/README.md), not a gap in this script.
set -euo pipefail

# shellcheck source-path=SCRIPTDIR source=lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

flags=()
args=()
for arg in "$@"; do
  case "$arg" in
    -*) flags+=("$arg") ;;
    *) args+=("$arg") ;;
  esac
done
parse_flags "${flags[@]}"
require_host
if [ "${#args[@]}" -lt 2 ]; then
  echo "usage: allow-egress.sh [--dry-run] <owlhead-api|owlhead-executor|cloudflared> <host>..." >&2
  exit 2
fi
unit="${args[0]}"
case "$unit" in
  owlhead-api | owlhead-executor | cloudflared) ;;
  *)
    echo "unknown unit: $unit" >&2
    exit 2
    ;;
esac

allow=""
for host in "${args[@]:1}"; do
  if ! [[ "$host" =~ ^[A-Za-z0-9]([A-Za-z0-9.-]*[A-Za-z0-9])?$ ]]; then
    echo "not a host name: $host" >&2
    exit 2
  fi
  found="$(getent ahosts "$host" | awk '{ print $1 }' | sort -u)"
  if [ -z "$found" ]; then
    echo "$host does not resolve; nothing written" >&2
    exit 1
  fi
  allow="$allow"$'\n'"$found"
done

dropin="/etc/systemd/system/$unit.service.d"
run install -d -o root -g root -m 0755 "$dropin"
{
  printf '[Service]\n'
  printf '%s\n' "$allow" | sort -u | while read -r address; do
    if [ -n "$address" ] && [[ "$address" =~ ^[0-9A-Fa-f:.]+$ ]] && [ "$address" != 0.0.0.0 ] && [ "$address" != :: ]; then
      printf 'IPAddressAllow=%s\n' "$address"
    fi
  done
} | put_file "$dropin/10-egress.conf" 0644 root:root
run systemctl daemon-reload
if [ "$(systemctl is-active "$unit" 2>/dev/null || true)" = active ]; then
  run systemctl restart "$unit"
fi
echo "allowed: $unit may reach the addresses in $dropin/10-egress.conf, and loopback"
