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

rejection_reason() {
  local address
  address="$(printf '%s' "$1" | tr 'A-F' 'a-f')"
  if [[ "$address" == *:* ]]; then
    case "$address" in
      ::) echo "the unspecified address" ;;
      ::1) echo "loopback" ;;
      ::ffff:*) echo "an IPv4-mapped address" ;;
      fe[89ab]*) echo "link-local (fe80::/10)" ;;
      f[cd]*) echo "unique local (fc00::/7)" ;;
      ff*) echo "multicast (ff00::/8)" ;;
    esac
    return 0
  fi
  local a b
  IFS=. read -r a b _ <<<"$address"
  a=$((10#$a))
  b=$((10#$b))
  if [ "$a" -eq 0 ]; then echo "this-network or unspecified (0.0.0.0/8)"
  elif [ "$a" -eq 10 ]; then echo "private (10.0.0.0/8)"
  elif [ "$a" -eq 127 ]; then echo "loopback (127.0.0.0/8)"
  elif [ "$a" -eq 100 ] && [ "$b" -ge 64 ] && [ "$b" -le 127 ]; then echo "shared address space (100.64.0.0/10)"
  elif [ "$a" -eq 169 ] && [ "$b" -eq 254 ]; then echo "link-local (169.254.0.0/16)"
  elif [ "$a" -eq 172 ] && [ "$b" -ge 16 ] && [ "$b" -le 31 ]; then echo "private (172.16.0.0/12)"
  elif [ "$a" -eq 192 ] && [ "$b" -eq 168 ]; then echo "private (192.168.0.0/16)"
  elif [ "$a" -ge 224 ] && [ "$a" -le 239 ]; then echo "multicast (224.0.0.0/4)"
  elif [ "$a" -ge 240 ]; then echo "reserved (240.0.0.0/4)"
  fi
}

allow=""
for host in "${args[@]:1}"; do
  if ! [[ "$host" =~ ^[A-Za-z0-9]([A-Za-z0-9.-]*[A-Za-z0-9])?$ ]]; then
    echo "not a host name: $host" >&2
    exit 2
  fi
  found="$(getent ahosts "$host" | awk '{ print $1 }' | sort -u)"
  usable=""
  while read -r address; do
    if [ -z "$address" ] || ! [[ "$address" =~ ^[0-9A-Fa-f:.]+$ ]]; then
      continue
    fi
    reason="$(rejection_reason "$address")"
    if [ -n "$reason" ]; then
      echo "$host resolves to $address, rejected: $reason" >&2
    else
      usable="$usable"$'\n'"$address"
    fi
  done <<<"$found"
  if [ -z "$usable" ]; then
    echo "$host resolves to no global address; nothing written" >&2
    exit 1
  fi
  allow="$allow$usable"
done

dropin="/etc/systemd/system/$unit.service.d"
run install -d -o root -g root -m 0755 "$dropin"
{
  printf '[Service]\n'
  printf '%s\n' "$allow" | sort -u | while read -r address; do
    if [ -n "$address" ]; then
      printf 'IPAddressAllow=%s\n' "$address"
    fi
  done
} | put_file "$dropin/10-egress.conf" 0644 root:root
run systemctl daemon-reload
if [ "$(systemctl is-active "$unit" 2>/dev/null || true)" = active ]; then
  run systemctl restart "$unit"
fi
echo "allowed: $unit may reach the addresses in $dropin/10-egress.conf, and loopback"
