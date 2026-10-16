# E2 runbook: the first Alpaca PAPER trade (one SPY order)

Built from origin/main 39480761 (`docs/project/tasks/first-paper-trade.md`, "The manual paper run (E2)"),
every command and flag checked against the clap definitions in `crates/mandate-cli` and the argument
parser in `crates/mandate-paper/src/lib.rs`. Nothing below needs or contains a real credential. Anything
I could not confirm from source is marked UNVERIFIED. Paths are relative to a checkout of main.

Read this first, because the brief omits it: `mandate-paper` refuses to start without `--confirm-paper`
(`parse()` in `crates/mandate-paper/src/lib.rs`). Every `mandate-paper` command here carries it.

## 0. Prerequisites

- **Build** (default features only; never `--features live`):
  `cargo build --release -p mandate-cli -p mandate-paper`
  Binaries: `target/release/mandate` and `target/release/mandate-paper`.
- **Postgres (18 in CI; 16 worked on 2026-10-16), journal roles and schema.** There is no migrate command in the CLI; `deploy/bootstrap.sh`
  is the repo's own recipe (VM-oriented). A local equivalent of its SQL, UNVERIFIED end to end (derived
  from `deploy/bootstrap.sh`; I did not run it):
  ```bash
  sudo -u postgres psql -v ON_ERROR_STOP=1 <<'SQL'
  CREATE DATABASE mandate;
  \c mandate
  CREATE ROLE mandate_journal_owner NOLOGIN;
  CREATE ROLE mandate_journal_app NOLOGIN;
  CREATE ROLE e2_app LOGIN PASSWORD 'CHOOSE-A-LOCAL-PASSWORD' IN ROLE mandate_journal_app;
  CREATE SCHEMA journal AUTHORIZATION mandate_journal_owner;
  GRANT USAGE ON SCHEMA journal TO mandate_journal_app;
  ALTER ROLE e2_app IN DATABASE mandate SET search_path = journal;
  SQL
  { echo 'SET ROLE mandate_journal_owner; SET search_path = journal;'; cat migrations/0001_journal.sql; } \
    | sudo -u postgres psql -v ON_ERROR_STOP=1 mandate
  ```
  Run on 2026-10-16 against PostgreSQL 16: this SQL and the migration applied cleanly and `e2_app` logs
  in with `search_path = journal`. If the container restarts, `pg_ctlcluster 16 main start` again
  before any command (otherwise every command fails `the journal failed: unavailable`).
  The DSN the commands take must log in as a member of `mandate_journal_app` (`PgJournal::from_dsn` docs),
  not as the superuser. (The tests' superuser `MANDATE_PG_URL` is a different thing; do not use it here.)
- **Shell variables** (no password in the DSN: sqlx reads `PGPASSWORD`, `crates/mandate-cli/src/postgres.rs`):
  ```bash
  export MANDATE_PG_URL='postgres://e2_app@localhost:5432/mandate'   # app-role DSN; used below via --journal
  export PGPASSWORD='<the local password you chose above>'
  export W=$HOME/e2 ; mkdir -p $W/files        # then cd into the repo checkout
  M=target/release/mandate ; P=target/release/mandate-paper
  J="--journal $MANDATE_PG_URL --store $W/store"
  O="--workspace ws_paper01 --user founder_01"
  ```
  The binaries take the DSN through `--journal`, not from the environment; the variable is a convenience.
- **Alpaca PAPER credentials**, exactly these two env vars (`KEY_ID_VAR`/`SECRET_VAR` in
  `crates/mandate-alpaca/src/http.rs`; `download` reads the same two):
  ```bash
  export MANDATE_ALPACA_PAPER_KEY_ID='<paper key id>'
  export MANDATE_ALPACA_PAPER_SECRET='<paper secret>'
  ```
  Type them yourself at the terminal; never paste them into a file or chat. The host is fixed in code to
  `https://paper-api.alpaca.markets` (`PAPER_HOST`); there is no host flag, and the host MUST be the paper
  host. Any env var whose name contains `ALPACA` and whose value looks like a URL or host makes
  `mandate-paper` stop before reading a credential (`refuse_configured_host`). Use keys generated in the
  Alpaca PAPER dashboard only.
- **Flat paper account**: no position, no open order, zero accrued fees, status ACTIVE, not blocked
  (`crates/mandate-shell/src/paper/judge.rs`). Check and cancel in the Alpaca paper dashboard first.
- **Window**: run `mandate-paper` on a US trading day between 09:30 and 15:50 New York (the judge needs the
  regular session outside the 10-minute close window, at the preflight and again at submission). Start in
  the morning: after the order the process stays up watching it, bounded by the close window.
- **Fresh model output**: the output's `as_of` is the last completed session's 16:00 ET close and it expires
  `max_output_age_s` later (fixture 86400 s). A morning run the day after a close is inside that; a Monday
  run is not (about 65 h since Friday's close) unless your mandate pins a larger value, your envelope
  decision. Today (Fri 2026-10-16) the previous close is Thu 2026-10-15.

## 1. Daily bars (on the run day, before the window)

The run refuses unless the stored daily bars end exactly on the last completed regular session and are
clean (`trusted()` in `crates/mandate-shell/src/adapters.rs`). `--end` must be before today's UTC date.
```bash
$M download --symbols SPY --asset-class us-equity --kind bars --timeframe 1Day --feed iex \
    --start 2026-08-31 --end 2026-10-15 --out $W/data
$M inspect $W/data/alpaca/iex/bars-1Day/SPY
```
Success: one line per partition and a total; `inspect` exits 0 (the brief wants zero problems, zero true
gaps, no split inside the span; read the report). `--bars` for the run is that dataset directory itself:
`$W/data/alpaca/iex/bars-1Day/SPY`. Needs at least max(20, slow_periods) sessions; the dates above give ~30.

## 2. Configuration objects

Existing fixtures, copy as is:
```bash
cp crates/mandate-shell/tests/fixtures/tracer/config/fee-config.json        $W/files/fee.json
cp crates/mandate-shell/tests/fixtures/tracer/config/trading-calendar.json  $W/files/calendar.json
cp crates/mandate-shell/tests/fixtures/tracer/config/rule-set.json          $W/files/rules.json
```
These are exactly what the paper path accepts (`crates/mandate-shell/src/paper/artifacts.rs`): fee members
`effective_from, environment, schedule` with `schedule` = `conservative_v1`; calendar members `first,
holidays, last, special_sessions`, both lists empty (the fixture's `last` is 2026-12-31; after that a new
calendar object is needed); rule set members `executor, gate, gate_config, iex_quote_max_age_s,
ruleset_version, story`, with `story` literally `E7-7`. The fee `effective_from` (2026-01-01) must not be
after the run date.

**Instrument snapshot**: no file exists in the repo (only inline strings in tests); the shape is DEC-523,
enforced by `is_dec_523` in `crates/mandate-cli/src/config.rs` and again at run time: exactly these eight
string members. SPY's Alpaca asset id below is the one the shell tests use
(`crates/mandate-shell/tests/common/mod.rs` `SPY`); confirm it against the paper API (`GET /v2/assets/SPY`),
because the run refuses if the broker's id differs. Do NOT use `b0b6dd9d-...`: that is AAPL's id, which the CLI
test fixture `crates/mandate-cli/tests/fixtures/spy_mandate.json` mislabels "SPY".
```bash
cat > $W/files/spy.json <<EOF
{"asset_class":"us_equity","etp":"plain","etp_classified_at":"$(date -u +%Y-%m-%dT00:00:00Z)","etp_source":"nasdaq_trader_symbol_directory","exchange":"arca","increment":"whole","instrument_id":"b28f4066-5c6d-479b-a2af-85dc1a8f16fb","symbol":"SPY"}
EOF
```
Before registering, look SPY up in the Nasdaq Trader symbol directory (ETF column; SPY is a plain ETF, not
leveraged, inverse or volatility-linked) so the `etp` claim is true. The date must be within the last 7 days
and not in the future at run time (`etp_classification_max_age_s` 604,800 s; the gate denies every opening
otherwise). Re-register within 7 days of each run.

## 3. Mandate document (your envelope, AGENTS.md rule 11)

Base file: `crates/mandate-cli/tests/fixtures/spy_mandate.json` (the same as the shell's tracer mandate except
name and symbol). It is a TEST fixture holding values you must decide, with two traps for a real SPY run:
1. `asset_id` is AAPL's id and `content_hash` is a test placeholder. The real pinned hash of
   `quant.ma_crossover` 1.0.0 is `sha256:cf36ebf8746a95c79443a2a0b5b49762809e082bced8a0a5bfdda7a3dd6c469d`
   (literal pin test `PINNED_1_0_0`, `crates/mandate-modelhost/tests/host.rs`).
2. `max_order_usd` is "300", which does not cover one SPY share at current prices (the brief: it must cover
   at least one whole share or the builder proposes quantity zero and the run refuses). Check the ask first.
Keep `autonomy` as is: the paper run acts only on `auto`, and the fixture's `routine` rule makes openings
`auto`. Protection is on with `take_profit_distance` set (required by the brief).
```bash
cp crates/mandate-cli/tests/fixtures/spy_mandate.json $W/files/mandate.json
sed -i -e 's/b0b6dd9d-8b9b-48a9-ba46-b9d54906e415/b28f4066-5c6d-479b-a2af-85dc1a8f16fb/' \
       -e 's/sha256:4f3559229f89b27c04b43ec773b0ff0b884895362622964b78dfd791d67e18fc/sha256:cf36ebf8746a95c79443a2a0b5b49762809e082bced8a0a5bfdda7a3dd6c469d/' \
       $W/files/mandate.json
# founder's choice (2026-10-16): $5,000 order cap, $10,000 allocation (paper money)
sed -i -e 's/"allocation_usd":"1000"/"allocation_usd":"10000"/' \
       -e 's/"max_position_usd":"1000"/"max_position_usd":"10000"/' \
       -e 's/"max_gross_exposure_usd":"1000"/"max_gross_exposure_usd":"10000"/' \
       -e 's/"max_order_usd":"300"/"max_order_usd":"5000"/' \
       $W/files/mandate.json
grep -o '"\(allocation_usd\|max_position_usd\|max_gross_exposure_usd\|max_order_usd\)":"[0-9]*"' $W/files/mandate.json
# expect 10000, 10000, 10000, 5000; if any line is missing, the fixture's key or value differs: edit by hand
```
With a $5,000 cap the builder may size several SPY shares, not one. That is fine for E2. Your paper
account needs at least $10,000 equity (V-002).
Keep `max_order_usd` <= the other three, as in the fixture. To keep the order at one share, put
`max_order_usd` above one share but below two (UNVERIFIED: the builder's exact sizing). The mandate must also
pass V-002 at run time (allocation <= paper account equity).

## 4. Confirmed-mandate commands (P0, D1, D1c, D2), in this order

Each prints one line with an event id and `seq`; each refuses with a stable code (section 9).
```bash
$M workspace open --workspace ws_paper01 $J            # opened ctl:ws_paper01 as event <id> at seq 1
                                                       # (refused workspace_already_open if the stream has any event)
$M config register --kind fee_config           $W/files/fee.json      $O $J   # registered as event <id> at seq <n>
$M config register --kind trading_calendar     $W/files/calendar.json $O $J
$M config register --kind rule_set             $W/files/rules.json    $O $J
$M config register --kind instrument_snapshot  $W/files/spy.json      $O $J
$M model register quant.ma_crossover 1.0.0 $O $J       # content object computed by the host; no file
$M version create $W/files/mandate.json $O $J          # created sha256:<64 hex> as event <id> at seq <n>
V=sha256:<the hash printed above>
$M version confirm $V $O $J                            # prints: code <CODE> / warnings <list or none>; commits nothing
$M version confirm $V --code <CODE> $O $J              # confirmed as event <id> at seq <n>  (cli_confirm)
$M agent deploy agent_spy $V $O $J                     # prints: code <CODE2> / warnings ...; commits nothing
$M agent deploy agent_spy $V --code <CODE2> $O $J      # deployed as event <id> at seq <n>
```
Read the printed warnings before typing the code: that is the confirmation. With the section 3
sizing, both `version confirm` and `agent deploy` print `warnings W-002` (mandate spec: one full
position's loss at its stop exceeds the daily loss budget); it is a warning, not a violation. The brief also lists
`policy_set` and `model_registry` registrations (`--kind policy_set|model_registry`), but nothing on the
paper run path reads them on main (`governance()` has no caller in `mandate-paper` or the shell's run), so
they are skipped here. UNVERIFIED that no later check wants them; if a refusal names them, register them.
Re-running an identical command is idempotent (finds the earlier event, commits nothing).

## 5. Dry run (nothing sent, no cycle journaled)

```bash
RUN="--confirm-paper --workspace ws_paper01 --agent agent_spy --account-ref 01J8Z2ACCT00000000000000A1 \
     --journal $MANDATE_PG_URL --store $W/store --bars $W/data/alpaca/iex/bars-1Day/SPY"
$P $RUN
```
`--journal` is needed even for the dry run (it reads the control stream); only the placing run appends to
it. `--account-ref` is any `[A-Za-z0-9_-]{1,64}` id; a ULID is used so it also fits the journal spec's
`account_ref` type. On success it prints exactly one of:
- `would place <client_order_id> (nothing sent; pass --place-one-order)`: go on.
- `the model output Flat; nothing sent` (or `Undecided`): the model says no buy. Stop. Do not change the
  mandate or windows to get a `Long` (the brief calls that outcome-chasing). Try another session.
A refusal prints one line on stderr and exits non-zero (section 9).

## 6. The placing run (your explicit confirmation immediately before it, DEC-502 item 6)

Only after the dry run said `would place`, still inside 09:30 to 15:50 ET, and you have decided to send one
order to PAPER:
```bash
$P $RUN --place-one-order
```
One POST through the production cycle, then it watches the order (GETs and executor ticks) until it is
terminal and, if protection applies, its legs are in place, or until the close-window bound. **It prints
nothing until it finishes** (`process()` prints at the end); leave it running and do not interrupt it.
Success: `submitted <client_order_id>` on stdout, exit 0. If it exits non-zero, note the stderr line and
check the paper dashboard for an open order before doing anything else.

## 7. Evidence after the fill

**a. The open-orders read after the fill (DEC-878 item 4)**: settles whether Alpaca nests a filled
bracket's legs under the entry or lists them top level. A read-only GET to the paper host, the same path the
connector uses (`OPEN_ORDERS`, `crates/mandate-alpaca/src/client.rs`):
```bash
curl -sS -H "APCA-API-KEY-ID: $MANDATE_ALPACA_PAPER_KEY_ID" -H "APCA-API-SECRET-KEY: $MANDATE_ALPACA_PAPER_SECRET" \
  "https://paper-api.alpaca.markets/v2/orders?status=open&limit=500&direction=asc&nested=true" > $W/open-orders-after-fill.json
```
Save it unedited (order ids, no keys). Note whether the filled entry shows exactly two nested sell legs
(stop and take-profit) or the legs appear at top level. The docs PR that records E2 attaches it.

**b. Export and verify the three streams** (names per `crates/mandate-paper/src/evidence.rs`; `export` takes
`--journal` only and refuses an existing file; `verify` needs the store):
```bash
for S in agent:ws_paper01:agent_spy acct:ws_paper01:01J8Z2ACCT00000000000000A1 ctl:ws_paper01; do
  F=$W/export-$(echo $S | tr ':' '_').jsonl
  $M journal export $S $F --journal $MANDATE_PG_URL
  $M journal verify $F --store $W/store
done
```
`export` prints `exported: stream <id>, seq 1 to <N>, last hash <h>, to <file>`. `verify` prints a report
ending `result: verified, stream <id>, seq 1 to <N>, last hash <h>` and exits 0; a failure prints
`result: failed, seq <n>, <check code>` and exits non-zero. A control or account report may add a line that
the connection-cause check did not run; that is expected. `export_stream_empty` means that stream has no
event (for the agent or account stream: the placing run wrote nothing).

## 8. The second run (must be refused)

Same placing command again, same day, with the position still open:
```bash
$P $RUN --place-one-order ; echo "exit=$?"
```
Expected: non-zero exit, one stderr line ending `a paper account with no position and no open order`
(DEC-470 item 1, `judge.rs`; the "tracer stopped at <stage>:" prefix is UNVERIFIED), nothing on stdout,
and no POST. Prove it: re-export the agent stream to a new filename and confirm its `seq` did not grow. If a
second `submitted` ever appears, stop and report it.

## 9. Common refusals (stderr, exit non-zero) and what to do

| Message / code | Meaning | Do |
|---|---|---|
| `--confirm-paper is required`, `<flag> needs a value`, `an argument is unknown` | usage | only the six value flags, `--confirm-paper`, `--place-one-order` exist |
| `the control stream could not be read` | no or wrong `--journal`, DB down, wrong role or search_path | check DSN, `PGPASSWORD`, section 0 SQL |
| `<VAR> looks like a URL; ...` | an `*ALPACA*` env var holds a host | unset it |
| `the paper credentials or client are unavailable` | the two env vars unset or empty | export both |
| `no confirmed deployment: ...` | not deployed, stopped, document missing, V-rule failed | redo step 4; a V-007 violation = model hash or registration mismatch |
| `the registered configuration cannot be used: <code>` | `unregistered` (kind missing), `snapshot_value`, `snapshot_extra_member`, `fee_not_yet_effective`, `registry_mismatch` | re-register the named object |
| `the model host refused: ...` | `closes_end`, `too_few_closes`, `pin_hash_mismatch`, `param_keys`, ... | redo step 1 with the right `--end`; check the mandate's pinned hash |
| `the stored data cannot be trusted: ...` | daily bars stale, gapped or split | redo step 1 and `inspect` |
| `... a paper account with no position and no open order` / `an active, unblocked paper account` / `a paper account without accrued fees` | account not flat or blocked | clean the paper account (also the expected second-run refusal) |
| `... a current, uncrossed IEX quote` or a session refusal | stale quote or outside 09:30-15:50 | retry inside the window |
| `the gate did not allow the proposal (<reason>)` / `the order builder proposed a quantity of zero` | gate denial / `max_order_usd` below one share | never work around a denial; for zero qty make a new mandate version |
| `the proposal classified ask, and the tracer acts only on auto` | autonomy does not make the opening `auto` | fix autonomy in a new version |
| `cancel_unconfirmed` (alone) | watch bound hit, cancel not confirmed | check the paper dashboard now, cancel by hand, report it |
| CLI codes: `owner_user_invalid`, `workspace_already_open`, `instrument_snapshot_invalid`, `instrument_unregistered`, `version_invalid`, `code_mismatch`, `version_unconfirmed`, `version_superseded`, `agent_active`, `mandate_not_paper`, `config_file_unreadable` | a step 4 gesture refused, nothing written | fix the input; `code_mismatch` = re-show the code; `version_superseded` = a later confirm exists |

## 10. Could not verify

- The local Postgres SQL in section 0 (no database here); confirm `e2_app` logs in and `workspace open` works.
- Whether the Nasdaq Trader lookup needs a particular file; only the `etp_source` literal is enforced.
- The builder's order sizing and the current SPY price against your `max_order_usd`.
- The exact stderr stage prefix on the second-run refusal.
- That Alpaca's SPY asset id is still `b28f4066-...` (the run compares it with the broker's record).
- Whether the paper run ever waits on an `ask` grant (that wiring is described for the live arm); the
  fixture's autonomy makes openings `auto`, so it should not.

## 11. The run of 2026-10-16 (agent-run under DEC-930): stopped at the dry run

Preparation, all clean: PostgreSQL 16 schema; release build; SPY bars 2026-08-31 to 2026-10-15
(33 rows, `inspect` 0 problems, no split); the paper account ACTIVE, unblocked, flat, no accrued
fees, equity above 10,000; SPY's asset id `b28f4066-5c6d-479b-a2af-85dc1a8f16fb` matches
`GET /v2/assets/SPY`; control stream seq 1 to 9 (workspace, four registrations, model, version
`sha256:07d55fc806872d1c2568be9a9f49006e09a8855ec9a17a11ddd6c61a26780b21` created, confirmed,
`agent_spy` deployed). The Nasdaq Trader directory was unreachable from the session (DEC-930
item 3).

Dry runs at 14:59 to 15:02 UTC, inside the regular session, no order sent:
```
the tracer stopped at Reconcile: a current IEX quote
the tracer stopped at Reconcile: a current IEX quote
the tracer stopped at Validate: a current asset record
the tracer stopped at Validate: a current asset record
```
**Defect found (blocks every run).** `run()` reads `now` once, before any broker read
(`crates/mandate-paper/src/lib.rs`, `let now = clock.now()`). The asset record is stamped
`loaded_at` with the wall clock at its read (`crates/mandate-alpaca/src/client.rs`, `loaded_at:
self.pause.now()`), and the judge requires it not after `now`
(`AssetSnapshot::current` -> `judge_age`, `AheadOfClock`; `crates/mandate-shell/src/paper/judge.rs`,
`a current asset record`). A read always happens after `now`, so the asset check refuses every
real run; the quote check (`current_quote`: `quote_at > now`) refuses whenever the quote is newer
than the run's start, which for SPY's IEX quote is almost always. The Reconcile refusal is the same
kind: the container's clock trails Alpaca's by tens of milliseconds, so the freshest quote can be
stamped after the read's own `now`. Tests pass because their clocks do not advance between the
run's `now` and the reads. The fix (judge at a `now` taken after the reads, or one `now` per
check) is a safety-critical change to the shell's judge and the paper run: tests first, mutants,
and the different-model review. Per the brief the refusal was not worked around; nothing was
placed, so sections 6 to 8 did not run.

After stopping: `GET /v2/orders?status=open&nested=true` returned `[]` and `/v2/positions`
`[]`. Exports: `ctl:ws_paper01` seq 1 to 9 `verified` (last hash
`7f290a55ccfa4c7c640bff1cc7068bc797e113f6e2c07a2e19d635a6a109cc58`); the agent and account
streams are `export_stream_empty` (no cycle was journaled).

### Retry after DEC-935 (#1358), 16:01 to 16:05 UTC: stopped at Reconcile

The branch merged main at `13b57ce` (#1358, the run clock read after the preflight) and rebuilt.
Clock check before the run (`date -u` against the `Date` header of `curl -sI
https://paper-api.alpaca.markets/v2/clock`, three samples): within the header's 1 s resolution,
the server's second turned over 0.04 to 0.22 s before the local one. NTP (UDP 123) is not reachable
from the session, so the host clock could not be checked or corrected.

Dry run at 16:03:15 UTC:
```
the tracer stopped at Reconcile: a current IEX quote
exit=1
```
The cause, measured with the same read the preflight makes (`/v2/stocks/SPY/quotes/latest?feed=iex`,
20 samples, local clock read after the response): in 14 of 20 the quote's `t` was 2 to 24 ms
**after** the local clock at receipt, so `judge_age` refuses it `AheadOfClock`; none was older than
the 10 s bound. The judge is right to refuse a quote stamped after its clock; the host clock trails
the exchange's stamps by tens of milliseconds. Not worked around (no clock adjustment, no retry
loop); nothing placed. The next run needs an NTP-disciplined host (for example chrony, with its
offset recorded before the run) or a founder decision on a clock tolerance, which would loosen a
check (DEC-79).
