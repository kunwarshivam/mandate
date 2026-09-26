#!/usr/bin/env bash
# Records Alpaca market-data responses used as test fixtures (E2-1; CI never calls Alpaca, ES-19).
# Each scenario directory holds `requests.txt` (the exact path and query of every page, in order)
# and `page-<n>.json` (the response body, byte for byte). Credentials are sent only as headers and
# are never written; the script fails if a recorded body contains either credential value.
#
# Usage: MANDATE_ALPACA_PAPER_KEY_ID=... MANDATE_ALPACA_PAPER_SECRET=... bash record.sh [prefix]
# With a prefix, only the scenarios whose names start with it are recorded again. A scenario given
# a page count stops after that many pages (a day too large to record whole; parsing tests only).
set -euo pipefail
cd "$(dirname "$0")"
: "${MANDATE_ALPACA_PAPER_KEY_ID:?set MANDATE_ALPACA_PAPER_KEY_ID}"
: "${MANDATE_ALPACA_PAPER_SECRET:?set MANDATE_ALPACA_PAPER_SECRET}"
host="https://data.alpaca.markets"
only="${1:-}"

record() {
  local scenario="$1" first="$2" max_pages="${3:-0}" path token n=1
  case "$scenario" in "$only"*) ;; *) return 0 ;; esac
  rm -rf "$scenario" && mkdir -p "$scenario"
  path="$first"
  while :; do
    printf '%s\n' "$path" >>"$scenario/requests.txt"
    curl -sSf -H "APCA-API-KEY-ID: ${MANDATE_ALPACA_PAPER_KEY_ID}" \
      -H "APCA-API-SECRET-KEY: ${MANDATE_ALPACA_PAPER_SECRET}" \
      -o "$scenario/page-$n.json" "$host$path"
    if grep -qF -e "$MANDATE_ALPACA_PAPER_KEY_ID" -e "$MANDATE_ALPACA_PAPER_SECRET" "$scenario/page-$n.json"; then
      echo "credential found in $scenario/page-$n.json" >&2
      rm -rf "$scenario"
      exit 1
    fi
    token="$(python3 -c 'import json,sys; t=json.load(open(sys.argv[1])).get("next_page_token"); print(t or "")' "$scenario/page-$n.json")"
    [ -z "$token" ] && break
    [ "$max_pages" -gt 0 ] && [ "$n" -ge "$max_pages" ] && break
    token="$(python3 -c 'import sys,urllib.parse; print(urllib.parse.quote(sys.argv[1], safe="-._~"))' "$token")"
    path="$first&page_token=$token"
    n=$((n + 1))
  done
}

day() { printf 'start=%sT00:00:00Z&end=%sT00:00:00Z' "$1" "$2"; }

record stock-bars-sip-spy-1hour-2026-09-24-paged \
  "/v2/stocks/bars?symbols=SPY&timeframe=1Hour&$(day 2026-09-24 2026-09-25)&limit=5&feed=sip&adjustment=raw&sort=asc"
record stock-bars-sip-spy-1hour-2026-09-19 \
  "/v2/stocks/bars?symbols=SPY&timeframe=1Hour&$(day 2026-09-19 2026-09-20)&limit=10000&feed=sip&adjustment=raw&sort=asc"
record stock-bars-sip-spy-1hour-2026-09-23 \
  "/v2/stocks/bars?symbols=SPY&timeframe=1Hour&$(day 2026-09-23 2026-09-24)&limit=10000&feed=sip&adjustment=raw&sort=asc"
record stock-bars-sip-spy-1hour-2026-09-24 \
  "/v2/stocks/bars?symbols=SPY&timeframe=1Hour&$(day 2026-09-24 2026-09-25)&limit=10000&feed=sip&adjustment=raw&sort=asc"
record stock-trades-iex-shy-2026-09-24-paged \
  "/v2/stocks/trades?symbols=SHY&$(day 2026-09-24 2026-09-25)&limit=300&feed=iex&sort=asc"
record crypto-bars-btcusd-1hour-2026-09-24 \
  "/v1beta3/crypto/us/bars?symbols=BTC%2FUSD&timeframe=1Hour&$(day 2026-09-24 2026-09-25)&limit=10000&sort=asc"
record crypto-trades-btcusd-2026-09-24-paged \
  "/v1beta3/crypto/us/trades?symbols=BTC%2FUSD&$(day 2026-09-24 2026-09-25)&limit=300&sort=asc"
record stock-quotes-sip-cphc-2026-09-24-paged \
  "/v2/stocks/quotes?symbols=CPHC&$(day 2026-09-24 2026-09-25)&limit=50&feed=sip&sort=asc"
record stock-quotes-iex-cphc-2026-09-24 \
  "/v2/stocks/quotes?symbols=CPHC&$(day 2026-09-24 2026-09-25)&limit=10000&feed=iex&sort=asc"
record stock-quotes-sip-cphc-2026-09-19 \
  "/v2/stocks/quotes?symbols=CPHC&$(day 2026-09-19 2026-09-20)&limit=10000&feed=sip&sort=asc"
record crypto-quotes-btcusd-2026-09-24-first-page \
  "/v1beta3/crypto/us/quotes?symbols=BTC%2FUSD&$(day 2026-09-24 2026-09-25)&limit=20&sort=asc" 1
record corporate-actions-aapl-2020-2021-paged \
  "/v1/corporate-actions?symbols=AAPL&start=2019-12-01&end=2023-01-01&limit=5&sort=asc"
record corporate-actions-ge-2018-2026 \
  "/v1/corporate-actions?symbols=GE&start=2017-12-01&end=2027-09-26&limit=1000&sort=asc"
record corporate-actions-nvda-2021-2024 \
  "/v1/corporate-actions?symbols=NVDA&start=2020-12-01&end=2026-01-01&limit=1000&sort=asc"
