#!/usr/bin/env bash
# R0's rehearsal (the first live trade brief, R0 steps 1 and 2; DEC-124, DEC-922): every step the
# founder runs, in order, against the simulated Robinhood server on loopback and a private
# Postgres journal. It never dials a broker: the runner is the `rh-sim` build, which refuses any
# endpoint that is not http on 127.x (DEC-937 item 2), and the simulator listens on 127.0.0.1
# only. No credential is read or written; the simulated OAuth login is driven by the same two
# requests a browser makes on the simulator's page.
#
# Usage: scripts/r0-rehearsal.sh [--no-build] [--work DIR] [--sim-clock INSTANT]
#
#   --no-build           reuse the binaries already in CARGO_TARGET_DIR
#   --work DIR           the scratch directory (default: a new one under ${TMPDIR:-/tmp})
#   --sim-clock INSTANT  run the whole rehearsal on a clock that reads INSTANT, an RFC 3339 UTC
#                        instant inside a regular session, when the script starts and moves with
#                        the system clock from there (DEC-962): every runner and CLI process gets
#                        MANDATE_RH_SIM_CLOCK at its own start on that one timeline, the simulator
#                        its `clock`, and the fixture's daily bars and the instrument snapshot its
#                        date. The CLI is then built with its `rh-sim` feature, which only this
#                        rehearsal turns on; the runner is the `rh-sim` build already.
#
# The journal: MANDATE_PG_URL, a superuser DSN, if set (a schema of its own is created and
# dropped); otherwise a private PostgreSQL cluster is started from the `initdb` and `pg_ctl` on
# PATH or under /usr/lib/postgresql/*/bin, and stopped at exit.
#
# Prints one row per step (id, result, command, last output line) and writes the whole record,
# every command with its exit status, stdout and stderr, to $WORK/steps.md. Exits 0 only when
# every step passes, that is when R0's rehearsal would succeed.
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
BUILD=1
WORK=""
SIM_CLOCK=""
while [ $# -gt 0 ]; do
  case "$1" in
    --no-build) BUILD=0 ;;
    --work) WORK=${2:?--work needs a directory}; shift ;;
    --sim-clock) SIM_CLOCK=${2:?--sim-clock needs an RFC 3339 instant}; shift ;;
    *) echo "usage: $0 [--no-build] [--work DIR] [--sim-clock INSTANT]" >&2; exit 2 ;;
  esac
  shift
done

# The rehearsal's one timeline (DEC-962): SIM_CLOCK when the script starts, moving with the
# system clock. sim_now prints its reading as canonical RFC 3339, read at each process's start.
# The clock math runs in python3, which the mandate fixture below already needs: macOS's BSD date
# has neither -d nor %N, and the founder rehearses on a Mac (DEC-954's portability floor).
epoch_ns_of() {
  python3 -I -c 'import re, sys
from datetime import datetime, timezone
text = re.sub(r"\\.(\d{1,9})", lambda m: "." + (m.group(1) + "000000")[:6], sys.argv[1], count=1)
text = text[:-1] + "+00:00" if text.endswith("Z") else text
moment = datetime.fromisoformat(text)
if moment.tzinfo is None:
    moment = moment.replace(tzinfo=timezone.utc)
span = moment.astimezone(timezone.utc) - datetime(1970, 1, 1, tzinfo=timezone.utc)
print((span.days * 86400 + span.seconds) * 1000000000 + span.microseconds * 1000)' "$1"
}
now_ns() {
  python3 -I -c 'import time; print(time.time_ns())'
}
CLI_FEATURES=()
if [ -n "$SIM_CLOCK" ]; then
  [[ $SIM_CLOCK =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(\.[0-9]{1,9})?(Z|[+-][0-9]{2}:[0-9]{2})$ ]] ||
    { echo "--sim-clock: not an RFC 3339 instant" >&2; exit 2; }
  SIM_EPOCH_NS=$(epoch_ns_of "$SIM_CLOCK") || { echo "--sim-clock: not an instant" >&2; exit 2; }
  REAL_EPOCH_NS=$(now_ns)
  CLI_FEATURES=(--features mandate-cli/rh-sim)
fi
sim_now() {
  python3 -I -c 'import sys, time
from datetime import datetime, timedelta, timezone
ns = int(sys.argv[1]) + time.time_ns() - int(sys.argv[2])
moment = datetime(1970, 1, 1, tzinfo=timezone.utc) + timedelta(microseconds=ns // 1000)
print(moment.strftime("%Y-%m-%dT%H:%M:%S") + ".%09dZ" % (ns % 1000000000))' "$SIM_EPOCH_NS" "$REAL_EPOCH_NS"
}
# clocked COMMAND...: COMMAND on the rehearsal's clock, when one is set.
clocked() {
  if [ -n "$SIM_CLOCK" ]; then env "MANDATE_RH_SIM_CLOCK=$(sim_now)" "$@"; else "$@"; fi
}
WORK=${WORK:-$(mktemp -d "${TMPDIR:-/tmp}/r0-rehearsal.XXXXXX")}
mkdir -p "$WORK"
WORK=$(cd "$WORK" && pwd)
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$WORK/target}
export CARGO_INCREMENTAL=0
export RUST_BACKTRACE=0
BIN=$CARGO_TARGET_DIR/debug
RECORD=$WORK/steps.md
STORE=$WORK/store
MARKET=$WORK/market
mkdir -p "$STORE" "$MARKET"

WORKSPACE=ws_r0
OWNER=founder
AGENT=agent_r0
CONNECTION=conn_rh_rehearsal_01
ACCOUNT_REF=01J0R0REHEARSA1ACC0VNT0000
RH_ACCOUNT=RHSIM00000001
SYMBOL=RHETF
ASSET_ID=5f0c3c1e-7d1a-4c8e-9b0a-2e6f1d3a9c41
SCHEMA=r0_rehearsal_$$

SIM_PID=""
PG_DATA=""
PG_BIN=""
ADMIN_URL=""
DSN="unset"
FAILED=0

# shellcheck disable=SC2329 # invoked by the EXIT trap below
cleanup() {
  if [ -n "$SIM_PID" ]; then kill "$SIM_PID" 2>/dev/null || true; fi
  if [ -n "$PG_DATA" ]; then
    as_pg "$PG_BIN/pg_ctl" -D "$PG_DATA" -m fast stop >/dev/null 2>&1 || true
    rm -rf "$(dirname "$PG_DATA")"
  fi
  if [ -n "$ADMIN_URL" ] && [ -n "${MANDATE_PG_URL:-}" ]; then
    psql "$ADMIN_URL" -qc "DROP SCHEMA IF EXISTS $SCHEMA CASCADE" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

as_pg() {
  if [ "$(id -u)" = 0 ]; then runuser -u postgres -- "$@"; else "$@"; fi
}

printf "# R0 rehearsal record\n\nWork directory: \`%s\`; commit \`%s\`; run clock \`%s\`.\n\n" \
  "$WORK" "$(git -C "$ROOT" rev-parse --short HEAD)" "${SIM_CLOCK:-the system clock}" >"$RECORD"

# step ID EXPECT COMMAND...: runs COMMAND, saves its output under $WORK/out/ID.*, and passes when
# it exits 0 and its stdout and stderr together contain EXPECT (an extended regex).
step() {
  local id=$1 expect=$2 status=0 result
  shift 2
  mkdir -p "$WORK/out"
  "$@" >"$WORK/out/$id.out" 2>"$WORK/out/$id.err" || status=$?
  if [ "$status" = 0 ] && cat "$WORK/out/$id.out" "$WORK/out/$id.err" | grep -Eq -- "$expect"; then
    result=PASS
  else
    result=FAIL
    FAILED=1
  fi
  report "$id" "$result" "$status" "$*"
  [ "$result" = PASS ]
}

report() {
  local id=$1 result=$2 status=$3 command=${4//"$DSN"/\$DSN} last
  last=$(cat "$WORK/out/$id.out" "$WORK/out/$id.err" | grep -v '^$' | tail -n 1 | cut -c1-160)
  printf '%-4s %-4s exit %-3s %s\n     -> %s\n' "$id" "$result" "$status" "${command//$WORK/\$WORK}" "$last"
  {
    printf '## %s: %s (exit %s)\n\n```text\n$ %s\n' "$id" "$result" "$status" "${command//$WORK/\$WORK}"
    cat "$WORK/out/$id.out" "$WORK/out/$id.err"
    printf '\n```\n\n'
  } | sed "s|$WORK|\$WORK|g" >>"$RECORD"
}

# --- R0 step 1: the builds ------------------------------------------------------------------
if [ "$BUILD" = 1 ]; then
  step 1a 'Finished' cargo build --manifest-path "$ROOT/Cargo.toml" --bins -p mandate-cli -p mandate-rh-sim \
    ${CLI_FEATURES[@]+"${CLI_FEATURES[@]}"} || true
  step 1b 'Finished' cargo build --manifest-path "$ROOT/Cargo.toml" --bins -p mandate-paper \
    --features live,rh-sim,mandate-mcp/rh-sim || true
  # The stored daily bars' writer (DEC-961 item 3): an example, built from dev-dependencies only.
  step 1c 'Finished' cargo build --manifest-path "$ROOT/Cargo.toml" -p mandate-paper --example rehearsal_bars || true
fi
for binary in mandate mandate-rh-sim mandate-paper examples/rehearsal_bars; do
  [ -x "$BIN/$binary" ] || { echo "missing $BIN/$binary: build first" >&2; exit 1; }
done
cli() { clocked "$BIN/mandate" "$@"; }

# --- The journal: a private Postgres schema, migrated as the owner role ----------------------
start_postgres() {
  if [ -n "${MANDATE_PG_URL:-}" ]; then
    ADMIN_URL=$MANDATE_PG_URL
  else
    local installed=(/usr/lib/postgresql/*/bin/initdb)
    PG_BIN=$(dirname "$(command -v initdb 2>/dev/null || echo "${installed[${#installed[@]}-1]}")")
    local dir port
    dir=$(mktemp -d "${TMPDIR:-/tmp}/r0-pg.XXXXXX")
    if [ "$(id -u)" = 0 ]; then chown postgres "$dir"; fi
    PG_DATA=$dir/data
    port=$((20000 + RANDOM % 20000))
    as_pg "$PG_BIN/initdb" -D "$PG_DATA" -A trust -U postgres >/dev/null
    as_pg "$PG_BIN/pg_ctl" -D "$PG_DATA" -l "$dir/log" -w \
      -o "-p $port -k $dir -c listen_addresses=127.0.0.1" start >/dev/null
    ADMIN_URL="postgres://postgres@127.0.0.1:$port/postgres"
  fi
  psql "$ADMIN_URL" -q -v ON_ERROR_STOP=1 <<SQL
DO \$\$ BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mandate_journal_owner') THEN
    CREATE ROLE mandate_journal_owner NOLOGIN; END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'mandate_journal_app') THEN
    CREATE ROLE mandate_journal_app NOLOGIN; END IF;
END \$\$;
CREATE SCHEMA $SCHEMA AUTHORIZATION mandate_journal_owner;
GRANT USAGE ON SCHEMA $SCHEMA TO mandate_journal_app;
SET ROLE mandate_journal_owner;
SET search_path = $SCHEMA;
\i $ROOT/migrations/0001_journal.sql
SQL
  local joiner='?'
  case "$ADMIN_URL" in *\?*) joiner='&' ;; esac
  DSN="${ADMIN_URL}${joiner}options=-c%20search_path%3D${SCHEMA}%20-c%20role%3Dmandate_journal_app"
}
start_postgres
JOURNAL=(--journal "$DSN" --store "$STORE")
# The rehearsal's workspace is live from its first event, and every owner command states it
# (DEC-952: paper and live never share a stream).
AS_OWNER=(--workspace "$WORKSPACE" --user "$OWNER" --environment live)
LIVE_OWNER=("${AS_OWNER[@]}")

# --- R0 step 2: the rehearsal ----------------------------------------------------------------
# The simulator's one agentic account; the runner scopes every request to it (CN-8, DEC-947).
RH_ACCOUNT=RHSIM00000001
SIM_CLOCK_MEMBER=""
if [ -n "$SIM_CLOCK" ]; then SIM_CLOCK_MEMBER=",\"clock\":\"$(sim_now)\""; fi
cat >"$WORK/sim.json" <<JSON
{"accounts":[{"number":"$RH_ACCOUNT","agentic_allowed":true,"cash":"100","buying_power":"100","pattern_day_trader":false}],
 "session":"regular","quotes":{"$SYMBOL":"85"},"fills":[{"symbol":"$SYMBOL","side":"buy","price":"85"},{"symbol":"$SYMBOL","side":"sell","price":"80.5"},{"symbol":"$SYMBOL","side":"buy","price":"85"}]$SIM_CLOCK_MEMBER}
JSON
"$BIN/mandate-rh-sim" --script "$WORK/sim.json" >"$WORK/sim.out" 2>"$WORK/sim.err" &
SIM_PID=$!
for _ in $(seq 50); do grep -q 'serving' "$WORK/sim.out" 2>/dev/null && break; sleep 0.1; done
ENDPOINT=$(sed -n 's/^mandate-rh-sim: serving //p' "$WORK/sim.out")
# The live run's market reads (DEC-933 item 2) come from a fixture in the rh-sim build (DEC-961):
# the IEX quote within a cent of the simulator's ask of 85, inside DEC-529's collar, stamped as it
# is read; the minute bars; and the stored daily bars `--bars` names, rising to 85, through the
# last completed session. No market-data key is read and no data host dialed.
printf '{"ask":"85.01","ask_size":"100","asset_id":"%s","bid":"84.99","bid_size":"100","exchange":"ARCA","minute_volume":"20000","symbol":"%s"}\n' \
  "$ASSET_ID" "$SYMBOL" >"$MARKET/market.json"
BARS_AT=()
if [ -n "$SIM_CLOCK" ]; then BARS_AT=("$(sim_now)"); fi
step 2a0 'bars-1Day' "$BIN/examples/rehearsal_bars" "$MARKET/daily" "$SYMBOL" 85 ${BARS_AT[@]+"${BARS_AT[@]}"} || true
BARS=$(tail -n 1 "$WORK/out/2a0.out")
BARS=${BARS:-$MARKET/daily}
step 2a 'resource' curl -sf "${ENDPOINT%/mcp}/.well-known/oauth-protected-resource/mcp" || true

step 2b 'opened ctl:' cli workspace open --workspace "$WORKSPACE" --environment live "${JOURNAL[@]}" || true

cat >"$WORK/instrument.json" <<JSON
{"asset_class":"us_equity","etp":"plain","etp_classified_at":"$(if [ -n "$SIM_CLOCK" ]; then sim_now | cut -c1-10; else date -u +%Y-%m-%d; fi)T00:00:00Z","etp_source":"nasdaq_trader_symbol_directory","exchange":"arca","increment":"whole","instrument_id":"$ASSET_ID","symbol":"$SYMBOL"}
JSON
step 2c 'registered' cli config register --kind instrument_snapshot "$WORK/instrument.json" "${AS_OWNER[@]}" "${JOURNAL[@]}" || true
TRACER=$ROOT/crates/mandate-shell/tests/fixtures/tracer/config
# The live Robinhood fee schedule (DEC-958): a live run refuses the tracer's paper-only one.
step 2c-fee 'registered' cli config register --kind fee_config "$ROOT/config/fees/robinhood-live-2026-10-16.json" "${AS_OWNER[@]}" "${JOURNAL[@]}" || true
step 2c-cal 'registered' cli config register --kind trading_calendar "$TRACER/trading-calendar.json" "${AS_OWNER[@]}" "${JOURNAL[@]}" || true
step 2c-rules 'registered' cli config register --kind rule_set "$TRACER/rule-set.json" "${AS_OWNER[@]}" "${JOURNAL[@]}" || true
step 2d 'registered' cli model register quant.ma_crossover 1.0.0 "${AS_OWNER[@]}" "${JOURNAL[@]}" || true
MODEL_HASH=$(grep -rl '"kind":"quant_model_content"' "$STORE" | head -n 1 | xargs -r basename || true)

# login ERR: the founder's two clicks on the simulator's login page, once the run prints its URL.
login() {
  local err=$1 url request
  for _ in $(seq 300); do
    url=$(sed -n 's/^open this address in a browser to log in: //p' "$err" | head -n 1)
    [ -n "$url" ] && break
    kill -0 "$2" 2>/dev/null || return 0
    sleep 0.1
  done
  [ -n "$url" ] || return 0
  request=$(curl -sf "$url" | grep -Eo 'name="request" value="[^"]+"' | sed 's/.*value="//; s/"$//')
  curl -sf -L -o /dev/null -d "request=$request&decision=approve" "${ENDPOINT%/mcp}/oauth/approve"
}

# 2d2: the connect check (R1, DEC-946): the runner logs in on the simulator's page, reads the
# contract and the agentic account, and journals ConnectionChecked; 2e and 2f bind its event id.
connect_check() {
  local id=2d2 pid status=0 result=FAIL
  mkdir -p "$WORK/out"
  clocked env "MANDATE_RH_SIM_ENDPOINT=$ENDPOINT" "MANDATE_RH_ACCOUNT_NUMBER=$RH_ACCOUNT" \
    "$BIN/mandate-paper" --connect-check --workspace "$WORKSPACE" --account-ref "$ACCOUNT_REF" \
    --connection "$CONNECTION" --journal "$DSN" --confirm-live >"$WORK/out/$id.out" 2>"$WORK/out/$id.err" &
  pid=$!
  login "$WORK/out/$id.err" "$pid" || true
  wait "$pid" || status=$?
  if [ "$status" = 0 ] && grep -q '^checked ' "$WORK/out/$id.out"; then result=PASS; fi
  [ "$result" = PASS ] || FAILED=1
  report "$id" "$result" "$status" "mandate-paper --connect-check --workspace $WORKSPACE --account-ref $ACCOUNT_REF --connection $CONNECTION --journal $DSN --confirm-live"
}
connect_check
CHECKED=$(sed -n 's/^checked //p' "$WORK/out/2d2.out" | head -n 1)
CHECKED=${CHECKED:-none}

RECORD_ARGS=(connection record "${LIVE_OWNER[@]:0:4}" --environment live --connection "$CONNECTION"
  --broker robinhood --account-ref "$ACCOUNT_REF" --checked "$CHECKED" --margin-attestation cash_account
  --scope get_accounts --scope get_equity_orders --scope get_equity_positions --scope get_portfolio
  --scope get_equity_quotes --scope get_equity_tradability --scope review_equity_order
  --scope place_equity_order --scope cancel_equity_order
  "${JOURNAL[@]}")
step 2e '^code ' cli "${RECORD_ARGS[@]}" || true
CODE=$(sed -n 's/^code //p' "$WORK/out/2e.out" | head -n 1)
step 2f '^recorded ' cli "${RECORD_ARGS[@]}" --code "${CODE:-none}" || true

python3 -I - "$ROOT/crates/mandate-cli/tests/fixtures/spy_mandate.json" "$WORK/mandate.json" \
  "${MODEL_HASH:-0}" "$CONNECTION" "$SYMBOL" "$ASSET_ID" <<'PY'
import json, sys
source, target, model_hash, connection, symbol, asset_id = sys.argv[1:]
m = json.load(open(source))
m["name"] = "r0-rehearsal"
m["environment"] = "live"
m["connection_id"] = connection
m["behavior"]["description"] = "R0 rehearsal against mandate-rh-sim: one ETF, ask for openings."
m["behavior"]["signal_models"][0]["content_hash"] = "sha256:" + model_hash
m["autonomy"]["rules"] = []
m["autonomy"]["default"] = "ask"
m["capital"]["allocation_usd"] = "100"
for limit in ("max_order_usd", "max_position_usd", "max_gross_exposure_usd"):
    m["risk"][limit] = "100"
# Robinhood protects US equities with a stop-limit, so V-008 needs its offset (DEC-539).
m["protection"]["crypto_stop_limit_offset"] = "0.01"
m["universe"]["pinned_instruments"] = [
    {"asset_class": "us_equity", "asset_id": asset_id, "symbol": symbol}]
open(target, "w").write(json.dumps(m, sort_keys=True, separators=(",", ":")))
PY
step 2g 'sha256:' cli version create "$WORK/mandate.json" "${LIVE_OWNER[@]}" "${JOURNAL[@]}" || true
VERSION=$(grep -Eo 'sha256:[0-9a-f]{64}' "$WORK/out/2g.out" | head -n 1 || true)
VERSION=${VERSION:-sha256:none}
step 2h '^code ' cli version confirm "$VERSION" "${LIVE_OWNER[@]}" "${JOURNAL[@]}" || true
CODE=$(sed -n 's/^code //p' "$WORK/out/2h.out" | head -n 1)
step 2i '^confirmed' cli version confirm "$VERSION" --code "${CODE:-none}" "${LIVE_OWNER[@]}" "${JOURNAL[@]}" || true
step 2j '^code ' cli agent deploy "$AGENT" "$VERSION" "${LIVE_OWNER[@]}" "${JOURNAL[@]}" || true
CODE=$(sed -n 's/^code //p' "$WORK/out/2j.out" | head -n 1)
step 2k '^deployed' cli agent deploy "$AGENT" "$VERSION" --code "${CODE:-none}" "${LIVE_OWNER[@]}" "${JOURNAL[@]}" || true

RUN=(env "MANDATE_RH_SIM_ENDPOINT=$ENDPOINT" "MANDATE_RH_ACCOUNT_NUMBER=$RH_ACCOUNT" "MANDATE_RH_SIM_MARKET=$MARKET" "$BIN/mandate-paper" --workspace "$WORKSPACE" --agent "$AGENT"
  --account-ref "$ACCOUNT_REF" --journal "$DSN" --store "$STORE" --bars "$BARS" --confirm-live)

# run ID EXIT EXPECT ARGS...: the runner in the background, logged in through the simulator's
# page. EXIT is `ok` for a run that must exit 0, or `refused` for one that must exit non-zero
# having submitted nothing (R0 step 7).
run() {
  local id=$1 exit=$2 expect=$3 pid status=0 result=FAIL output
  shift 3
  mkdir -p "$WORK/out"
  clocked "${RUN[@]}" "$@" >"$WORK/out/$id.out" 2>"$WORK/out/$id.err" &
  pid=$!
  login "$WORK/out/$id.err" "$pid" || true
  if [ "$id" = 2m ]; then grant "$pid"; fi
  wait "$pid" || status=$?
  output=$(cat "$WORK/out/$id.out" "$WORK/out/$id.err")
  if grep -Eq -- "$expect" <<<"$output"; then
    case "$exit:$status" in
      ok:0) result=PASS ;;
      refused:0) ;;
      refused:*) grep -q '^submitted ' "$WORK/out/$id.out" || result=PASS ;;
    esac
  fi
  [ "$result" = PASS ] || FAILED=1
  report "$id" "$result" "$status" "mandate-paper ${RUN[*]:5} $*"
}

# grant PID: the founder's inbox while the placing run waits (`mandate approvals`, cli_confirm).
grant() {
  local approval=""
  for _ in $(seq 120); do
    cli approvals list --agent "$AGENT" "${AS_OWNER[@]}" "${JOURNAL[@]}" >"$WORK/inbox.txt" 2>&1 || true
    approval=$(grep -Eo '[0-9A-HJKMNP-TV-Z]{26}' "$WORK/inbox.txt" | head -n 1 || true)
    [ -n "$approval" ] && break
    kill -0 "$1" 2>/dev/null || break
    sleep 1
  done
  step 2n 'deadline' cli approvals list --agent "$AGENT" "${AS_OWNER[@]}" "${JOURNAL[@]}" || true
  [ -n "$approval" ] || { echo "no approval was requested" >>"$WORK/out/2n.err"; return 0; }
  step 2o '^code ' cli approvals show "$approval" --agent "$AGENT" "${AS_OWNER[@]}" "${JOURNAL[@]}" || true
  local code
  code=$(sed -n 's/^code //p' "$WORK/out/2o.out" | head -n 1)
  step 2p '^granted ' cli approvals approve "$approval" --agent "$AGENT" --code "${code:-none}" \
    "${AS_OWNER[@]}" "${JOURNAL[@]}" || true
}

# The R0 mandate asks for every opening (DEC-529), so the dry run's answer to a fresh output is
# the ask, which a run without --place-one-order never makes: it stops at Classify, non-zero, and
# sends nothing (DEC-969 item 7). On a clock whose last completed session closed more than the
# model's max_output_age_s earlier (a Monday, or the day after a holiday) the output is not fresh
# and the dry run decides nothing, so this step fails there by design (DEC-969 item 6).
run 2l refused 'Classify: the proposal classified ask'
run 2m ok '^submitted ' --place-one-order
for stream in "ctl:$WORKSPACE" "agent:$WORKSPACE:$AGENT" "acct:$WORKSPACE:$ACCOUNT_REF"; do
  name=${stream%%:*}
  step "2q-$name" '^exported: ' cli journal export --journal "$DSN" "$stream" "$WORK/$name.jsonl" || true
  step "2r-$name" 'verified, stream ' cli journal verify "$WORK/$name.jsonl" --store "$STORE" || true
done
run 2s refused 'connection_spent|not flat' --place-one-order

# --- The stop's trigger (DEC-1152 item 3): the market falls through B3's stop at 80.76, the
# script's sell fill takes it at 80.5, inside its limit of 79.9524, and the account is flat. The
# simulator answers every order's record; the stop's reads sold and filled.
step 2t '"side":"sell","state":"filled","stop_price"' curl -sf -X POST \
  -H 'content-type: application/json' --data '{"quotes":{"'"$SYMBOL"'":"80.5"}}' \
  "${ENDPOINT%/mcp}/rehearsal/market" || true
# The same command on the account the stop left flat: the record is spent by the entry's fill
# (DEC-910 item 3), so it is refused before any place call, as R0 step 7 is (DEC-529 item 13: one
# order per live record). A second entry needs a fresh connection record. R23 is decided: a
# sold-out position's row of quantity zero is flat (DEC-2390 item 1, the founder), so the lapse
# finds the account flat and refuses the spent record; it sends nothing.
run 2u refused 'the connection record is spent' --place-one-order

echo
echo "record: $RECORD"
exit "$FAILED"
