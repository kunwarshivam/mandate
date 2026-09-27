#!/usr/bin/env bash
# Records Alpaca **paper trading** responses used as test fixtures (E7-2, E7-3, E7-4; CI never
# calls Alpaca, ADR-0001 ES-19). Each scenario directory holds `requests.txt` — one line per
# request, `<METHOD> <path and query>[ <canonical request body>]`, because a trading call is
# identified by what it sends and not only by where it sends it — and `response-<n>.json`, the
# response body byte for byte, in the same order.
#
# Only the paper trading host is ever contacted (ES-23; `AGENTS.md` rule 8: never place real
# orders). Every order this script places is a limit order far from the market so that it rests,
# and every one of them is cancelled before the script exits, including on failure.
#
# Credentials travel only as headers and are never written. The broker's `account_number` and
# account `id` are personal data (journal spec §6.4), so they are replaced by opaque `pii_refs`
# entries **before** a body is saved; the script fails and deletes the scenario if either a
# credential value or an unredacted account field survives into a file.
#
# Usage: MANDATE_ALPACA_PAPER_KEY_ID=... MANDATE_ALPACA_PAPER_SECRET=... bash record.sh [prefix]
set -euo pipefail
cd "$(dirname "$0")"
: "${MANDATE_ALPACA_PAPER_KEY_ID:?set MANDATE_ALPACA_PAPER_KEY_ID}"
: "${MANDATE_ALPACA_PAPER_SECRET:?set MANDATE_ALPACA_PAPER_SECRET}"
host="https://paper-api.alpaca.markets"
root="alpaca-trading"
only="${1:-}"
symbol="${MANDATE_RECORD_SYMBOL:-AAPL}"
placed=()

cleanup() {
  local id
  for id in ${placed+"${placed[@]}"}; do
    curl -sS -o /dev/null -X DELETE \
      -H "APCA-API-KEY-ID: ${MANDATE_ALPACA_PAPER_KEY_ID}" \
      -H "APCA-API-SECRET-KEY: ${MANDATE_ALPACA_PAPER_SECRET}" \
      "$host/v2/orders/$id" || true
  done
}
trap cleanup EXIT

# Replaces every personal-data field a wire type names with an opaque reference, before the bytes
# are saved (journal spec §6.4, task brief interpretation 24).
redact() {
  python3 - "$1" <<'PY'
import json, sys
path = sys.argv[1]
with open(path) as handle:
    body = handle.read()
try:
    value = json.loads(body)
except json.JSONDecodeError:
    sys.exit(0)

PII = {"account_number": "pii:account_number:1", "id": "pii:account_id:1"}

def scrub(node, account_object):
    if isinstance(node, dict):
        out = {}
        for key, inner in node.items():
            if account_object and key in PII:
                out[key] = PII[key]
            else:
                out[key] = scrub(inner, False)
        return out
    if isinstance(node, list):
        return [scrub(item, account_object) for item in node]
    return node

# Only the account object carries `account_number` and the account `id`; an order's `id` is not
# personal data, so the substitution is scoped to the object that names both.
is_account = isinstance(value, dict) and "account_number" in value
with open(path, "w") as handle:
    json.dump(scrub(value, is_account), handle, separators=(",", ":"), sort_keys=True)
    handle.write("\n")
PY
}

check() {
  local file="$1" scenario="$2"
  if grep -qF -e "$MANDATE_ALPACA_PAPER_KEY_ID" -e "$MANDATE_ALPACA_PAPER_SECRET" "$file"; then
    echo "credential found in $file" >&2
    rm -rf "$root/$scenario"
    exit 1
  fi
  if grep -qE '"account_number":"[^p]' "$file"; then
    echo "unredacted account number in $file" >&2
    rm -rf "$root/$scenario"
    exit 1
  fi
  # The account object's own `id` is personal data too (journal spec §6.4). An order's `id` is not,
  # so the check reads the object that carries `account_number`, the same scope `redact` uses.
  if ! python3 - "$file" <<'PY'
import json, sys
try:
    value = json.load(open(sys.argv[1]))
except (OSError, json.JSONDecodeError):
    sys.exit(0)
def unredacted(node):
    if isinstance(node, dict):
        if "account_number" in node:
            for key in ("account_number", "id"):
                if key in node and not str(node[key]).startswith("pii:"):
                    return True
        return any(unredacted(inner) for inner in node.values())
    if isinstance(node, list):
        return any(unredacted(item) for item in node)
    return False
sys.exit(1 if unredacted(value) else 0)
PY
  then
    echo "unredacted account id in $file" >&2
    rm -rf "$root/$scenario"
    exit 1
  fi
}

# record <scenario> <method> <path and query> [body]
record() {
  local scenario="$1" method="$2" path="$3" body="${4:-}" n
  case "$scenario" in "$only"*) ;; *) return 0 ;; esac
  mkdir -p "$root/$scenario"
  n=$(( $(ls "$root/$scenario" 2>/dev/null | grep -c '^response-' || true) + 1 ))
  # shellcheck disable=SC2034
  local status
  if [ -n "$body" ]; then
    printf '%s %s %s\n' "$method" "$path" "$body" >>"$root/$scenario/requests.txt"
    status=$(curl -sS -X "$method" -w '%{http_code}' \
      -H "APCA-API-KEY-ID: ${MANDATE_ALPACA_PAPER_KEY_ID}" \
      -H "APCA-API-SECRET-KEY: ${MANDATE_ALPACA_PAPER_SECRET}" \
      -H "Content-Type: application/json" \
      -d "$body" -o "$root/$scenario/response-$n.json" "$host$path")
  else
    printf '%s %s\n' "$method" "$path" >>"$root/$scenario/requests.txt"
    status=$(curl -sS -X "$method" -w '%{http_code}' \
      -H "APCA-API-KEY-ID: ${MANDATE_ALPACA_PAPER_KEY_ID}" \
      -H "APCA-API-SECRET-KEY: ${MANDATE_ALPACA_PAPER_SECRET}" \
      -o "$root/$scenario/response-$n.json" "$host$path")
  fi
  printf '%s\n' "$status" >>"$root/$scenario/statuses.txt"
  [ -s "$root/$scenario/response-$n.json" ] || printf '{}\n' >"$root/$scenario/response-$n.json"
  redact "$root/$scenario/response-$n.json"
  check "$root/$scenario/response-$n.json" "$scenario"
  python3 -c 'import json,sys; json.load(open(sys.argv[1]))' \
    "$root/$scenario/response-$n.json" 2>/dev/null || true
  remember "$root/$scenario/response-$n.json"
}

# Keeps every order id this script created, so `cleanup` can cancel all of them.
remember() {
  local id
  id="$(python3 -c '
import json, sys
try:
    body = json.load(open(sys.argv[1]))
except Exception:
    sys.exit(0)
if isinstance(body, dict) and body.get("status") in {"new", "accepted", "pending_new", "held", "accepted_for_bidding", "partially_filled"}:
    print(body.get("id", ""))
' "$1")"
  [ -n "$id" ] && placed+=("$id")
  return 0
}

fresh() {
  case "$1" in "$only"*) rm -rf "$root/$1" ;; esac
}

client_id() { printf 'md-%s' "$(python3 -c 'import secrets;print(secrets.token_hex(13))')"; }

limit_body() {
  printf '{"client_order_id":"%s","extended_hours":false,"limit_price":"1.00","order_class":"simple","qty":"1","side":"buy","symbol":"%s","time_in_force":"gtc","type":"limit"}' "$1" "$symbol"
}

bracket_body() {
  printf '{"client_order_id":"%s","limit_price":"1.00","order_class":"bracket","qty":"1","side":"buy","stop_loss":{"stop_price":"0.80"},"symbol":"%s","take_profit":{"limit_price":"999.00"},"time_in_force":"gtc","type":"limit"}' "$1" "$symbol"
}

main() {
  local resting bracket duplicate

  fresh submit_limit_accepted
  resting="$(client_id)"
  record submit_limit_accepted POST /v2/orders "$(limit_body "$resting")"

  fresh order_by_client_id_found
  record order_by_client_id_found GET "/v2/orders:by_client_order_id?client_order_id=$resting"

  fresh order_by_client_id_absent
  record order_by_client_id_absent GET "/v2/orders:by_client_order_id?client_order_id=md-never-submitted"

  fresh submit_duplicate_client_order_id
  duplicate="$(limit_body "$resting")"
  record submit_duplicate_client_order_id POST /v2/orders "$duplicate"

  fresh submit_bracket_accepted
  bracket="$(client_id)"
  record submit_bracket_accepted POST /v2/orders "$(bracket_body "$bracket")"

  fresh submit_rejected
  record submit_rejected POST /v2/orders \
    "$(printf '{"client_order_id":"%s","qty":"0","side":"buy","symbol":"%s","time_in_force":"gtc","type":"market"}' "$(client_id)" "$symbol")"

  fresh open_orders_page
  record open_orders_page GET "/v2/orders?status=open&limit=50&direction=asc"

  fresh positions
  record positions GET /v2/positions

  fresh account_active
  record account_active GET /v2/account

  fresh activities_fills
  record activities_fills GET "/v2/account/activities?activity_types=FILL&page_size=50"

  fresh cancel_confirmed
  record cancel_confirmed DELETE "/v2/orders/$(order_id_of "$resting")"
  record cancel_confirmed GET "/v2/orders:by_client_order_id?client_order_id=$resting"
}

order_id_of() {
  curl -sS -H "APCA-API-KEY-ID: ${MANDATE_ALPACA_PAPER_KEY_ID}" \
    -H "APCA-API-SECRET-KEY: ${MANDATE_ALPACA_PAPER_SECRET}" \
    "$host/v2/orders:by_client_order_id?client_order_id=$1" |
    python3 -c 'import json,sys; print(json.load(sys.stdin).get("id",""))'
}

main
