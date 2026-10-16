#!/usr/bin/env bash
# R0's rehearsal (the first live trade brief, R0 steps 1 and 2; DEC-124, DEC-922): every step the
# founder runs, in order, against the simulated Robinhood server on loopback and a private
# Postgres journal. It never dials a broker: the runner is the `rh-sim` build, which refuses any
# endpoint that is not http on 127.x (DEC-937 item 2), and the simulator listens on 127.0.0.1
# only. No credential is read or written; the simulated OAuth login is driven by the same two
# requests a browser makes on the simulator's page.
#
# Usage: scripts/r0-rehearsal.sh [--no-build] [--work DIR]
#
#   --no-build   reuse the binaries already in CARGO_TARGET_DIR
#   --work DIR   the scratch directory (default: a new one under ${TMPDIR:-/tmp})
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
while [ $# -gt 0 ]; do
  case "$1" in
    --no-build) BUILD=0 ;;
    --work) WORK=${2:?--work needs a directory}; shift ;;
    *) echo "usage: $0 [--no-build] [--work DIR]" >&2; exit 2 ;;
  esac
  shift
done
WORK=${WORK:-$(mktemp -d "${TMPDIR:-/tmp}/r0-rehearsal.XXXXXX")}
mkdir -p "$WORK"
WORK=$(cd "$WORK" && pwd)
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$WORK/target}
export CARGO_INCREMENTAL=0
export RUST_BACKTRACE=0
BIN=$CARGO_TARGET_DIR/debug
RECORD=$WORK/steps.md
STORE=$WORK/store
BARS=$WORK/bars
mkdir -p "$STORE" "$BARS"

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

printf "# R0 rehearsal record\n\nWork directory: \`%s\`; commit \`%s\`.\n\n" \
  "$WORK" "$(git -C "$ROOT" rev-parse --short HEAD)" >"$RECORD"

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
  step 1a 'Finished' cargo build --manifest-path "$ROOT/Cargo.toml" --bins -p mandate-cli -p mandate-rh-sim || true
  step 1b 'Finished' cargo build --manifest-path "$ROOT/Cargo.toml" --bins -p mandate-paper \
    --features live,rh-sim,mandate-mcp/rh-sim || true
fi
for binary in mandate mandate-rh-sim mandate-paper; do
  [ -x "$BIN/$binary" ] || { echo "missing $BIN/$binary: build first" >&2; exit 1; }
done
cli() { "$BIN/mandate" "$@"; }

# --- The journal: a private Postgres schema, migrated as the owner role ----------------------
start_postgres() {
  if [ -n "${MANDATE_PG_URL:-}" ]; then
    ADMIN_URL=$MANDATE_PG_URL
  else
    local installed=(/usr/lib/postgresql/*/bin/initdb)
    PG_BIN=$(dirname "$(command -v initdb 2>/dev/null || echo "${installed[-1]}")")
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
cat >"$WORK/sim.json" <<JSON
{"accounts":[{"number":"$RH_ACCOUNT","agentic_allowed":true,"cash":"100","buying_power":"100","pattern_day_trader":false}],
 "session":"regular","quotes":{"$SYMBOL":"85"},"fills":[{"symbol":"$SYMBOL","side":"buy","price":"85"}]}
JSON
"$BIN/mandate-rh-sim" --script "$WORK/sim.json" >"$WORK/sim.out" 2>"$WORK/sim.err" &
SIM_PID=$!
for _ in $(seq 50); do grep -q 'serving' "$WORK/sim.out" 2>/dev/null && break; sleep 0.1; done
ENDPOINT=$(sed -n 's/^mandate-rh-sim: serving //p' "$WORK/sim.out")
step 2a 'resource' curl -sf "${ENDPOINT%/mcp}/.well-known/oauth-protected-resource/mcp" || true

step 2b 'opened ctl:' cli workspace open --workspace "$WORKSPACE" --environment live "${JOURNAL[@]}" || true

cat >"$WORK/instrument.json" <<JSON
{"asset_class":"us_equity","etp":"plain","etp_classified_at":"$(date -u +%Y-%m-%dT00:00:00Z)","etp_source":"nasdaq_trader_symbol_directory","exchange":"arca","increment":"whole","instrument_id":"$ASSET_ID","symbol":"$SYMBOL"}
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
  env "MANDATE_RH_SIM_ENDPOINT=$ENDPOINT" "MANDATE_RH_ACCOUNT_NUMBER=$RH_ACCOUNT" \
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

RUN=(env "MANDATE_RH_SIM_ENDPOINT=$ENDPOINT" "MANDATE_RH_ACCOUNT_NUMBER=$RH_ACCOUNT" "$BIN/mandate-paper" --workspace "$WORKSPACE" --agent "$AGENT"
  --account-ref "$ACCOUNT_REF" --journal "$DSN" --store "$STORE" --bars "$BARS" --confirm-live)

# run ID EXIT EXPECT ARGS...: the runner in the background, logged in through the simulator's
# page. EXIT is `ok` for a run that must exit 0, or `refused` for one that must exit non-zero
# having submitted nothing (R0 step 7).
run() {
  local id=$1 exit=$2 expect=$3 pid status=0 result=FAIL output
  shift 3
  mkdir -p "$WORK/out"
  "${RUN[@]}" "$@" >"$WORK/out/$id.out" 2>"$WORK/out/$id.err" &
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
  report "$id" "$result" "$status" "mandate-paper ${RUN[*]:3} $*"
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

run 2l ok '^would place '
run 2m ok '^submitted ' --place-one-order
for stream in "ctl:$WORKSPACE" "agent:$WORKSPACE:$AGENT" "acct:$WORKSPACE:$ACCOUNT_REF"; do
  name=${stream%%:*}
  step "2q-$name" '^exported: ' cli journal export --journal "$DSN" "$stream" "$WORK/$name.jsonl" || true
  step "2r-$name" 'verified, stream ' cli journal verify "$WORK/$name.jsonl" --store "$STORE" || true
done
run 2s refused 'connection_spent|not flat' --place-one-order

echo
echo "record: $RECORD"
exit "$FAILED"
