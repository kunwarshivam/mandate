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
- **Clock (DEC-939)**: an NTP-disciplined host (since DEC-936, a quote up to 250 ms ahead of the host clock is also accepted). Before the dry run, record `chronyc tracking`
  (`Leap status : Normal`, `System time` under 5 ms) or `timedatectl show -p NTPSynchronized`
  (`yes`). The judge refuses a quote stamped after the host clock, and an unsynced clock that trails
  the exchange by even a few ms refuses most SPY quotes (#1355). Cloud agent sessions without UDP 123
  egress do not qualify.
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
because the run refuses if the broker's id differs. `b0b6dd9d-...` is AAPL's id, which the tracer's and the
Alpaca recordings' fixtures use; nothing labelled SPY carries it any more (#2631).
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
sed -i -e 's/sha256:4f3559229f89b27c04b43ec773b0ff0b884895362622964b78dfd791d67e18fc/sha256:cf36ebf8746a95c79443a2a0b5b49762809e082bced8a0a5bfdda7a3dd6c469d/' \
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
(DEC-470 item 1, `judge.rs`; the full line, seen on 2026-10-16, is `the tracer stopped at Validate: a paper account with no position and no open order`), nothing on stdout,
and no POST. Prove it: re-export the agent stream to a new filename and confirm its `seq` did not grow. If a
second `submitted` ever appears, stop and report it.

### 8a. Book a fill the journal missed (`--reconcile-only`, DEC-955)

If a placing run stopped after its order filled (as on 2026-10-16, section 11), the account stream
can hold the entry `filled` with no `FillApplied` or `LateFillApplied`. Book it without placing:
```bash
$P $RUN --reconcile-only ; echo "exit=$?"
```
It reads the account stream, and only if the stream owns a submitted order whose fill it has not
booked does it read the broker, with GETs only: that order by its own client order id, and the
account's `FILL` activities. It appends the fill records the broker's word calls for (here one
`LateFillApplied` at the broker's price), checks that nothing more is owed, and prints
`booked <n> fill record(s); nothing sent`, exit 0. It reads no quote or bars, runs no model and
places nothing; it needs `--journal` and is refused beside `--place-one-order`. A second
invocation prints `booked 0 fill records; nothing sent`. A placing run does the same booking
before its preflight, so after this the second run (section 8) is still refused at Validate while
the position is open: DEC-470 item 1 is unchanged. Prove it with the section 7b export: the
account stream gains exactly the fill record(s) and still verifies.

If the broker and the journal disagree (for example the entry reads filled and no matching fill
activity is listed), the run stops before any append with `the tracer stopped at Reconcile: the
broker and the journal disagree on <client_order_id>'s fills (<reason>)` and exits non-zero.
Nothing is journaled or sent; report the line, and do not work around it.

## 9. Common refusals (stderr, exit non-zero) and what to do

| Message / code | Meaning | Do |
|---|---|---|
| `--confirm-paper is required`, `<flag> needs a value`, `an argument is unknown` | usage | only the six value flags, `--confirm-paper`, `--place-one-order` and `--reconcile-only` exist |
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
| `... Reconcile: the broker and the journal disagree on <id>'s fills (<reason>)` | the own-fill booking found the broker's fills and the journal disagreeing (DEC-955 item 4); nothing written or sent | stop and report the line |
| `the proposal classified ask, and the tracer acts only on auto` | autonomy does not make the opening `auto` | fix autonomy in a new version |
| `cancel_unconfirmed` (alone) | watch bound hit, cancel not confirmed | check the paper dashboard now, cancel by hand, report it |
| CLI codes: `owner_user_invalid`, `workspace_already_open`, `instrument_snapshot_invalid`, `instrument_unregistered`, `version_invalid`, `code_mismatch`, `version_unconfirmed`, `version_superseded`, `agent_active`, `agent_stopped` (DEC-1173), `mandate_not_paper`, `config_file_unreadable` | a step 4 gesture refused, nothing written | fix the input; `code_mismatch` = re-show the code; `version_superseded` = a later confirm exists |

## 10. Could not verify

- The local Postgres SQL in section 0 (no database here); confirm `e2_app` logs in and `workspace open` works.
- Whether the Nasdaq Trader lookup needs a particular file; only the `etp_source` literal is enforced.
- The builder's order sizing and the current SPY price against your `max_order_usd`.
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

### Run on an NTP-synced host: stopped at the clock check (16:17 to 16:19 UTC)

A fresh cloud session with network access set to Full, to run E2 under DEC-939. chrony 4.x was
installed from apt and started with `time.google.com`, `time.cloudflare.com`, `216.239.35.0`,
`162.159.200.1` and `pool.ntp.org` (`makestep 0.1 -1`). DNS now resolves (the pool names became
addresses), but no NTP server answered: every source stayed at `Reach 0`, and a plain
client query (48-byte mode-3 packet to UDP 123) timed out against all four targets. Output at
16:18:57 UTC:
```
^? 216.239.35.0      0   7     0     -     +0ns[   +0ns] +/-    0ns
^? 162.159.200.1     0   7     0     -     +0ns[   +0ns] +/-    0ns
(six more sources, all Reach 0)
Reference ID    : 00000000 ()
System time     : 0.000000000 seconds fast of NTP time
Leap status     : Not synchronised
216.239.35.0  Total TX : 8  Total RX : 0
162.159.200.1 Total TX : 8  Total RX : 0
```
The Full network level opens outbound HTTP(S) through the session's proxy only; UDP 123 is still
refused. Under DEC-939 item 2 this host does not run E2, so the run stopped here: nothing was
built into a journal, no Alpaca call was made, no dry run and no order. Setting the clock from an
HTTP `Date` header or from Alpaca's stamps is not an NTP source (DEC-939, alternatives) and was not
tried. E2 needs a host with UDP 123 egress (a VM or the founder's machine with chrony or
systemd-timesyncd), or a new founder decision.

### Run on the founder's macOS laptop, NTP-synced: stopped at the dry run, classification facts for one share (16:30 to 16:45 UTC)

**Clock (DEC-939).** `sntp` against three servers, before and after the founder stepped the clock
with `sudo sntp -sS time.apple.com`:
```
16:30:31 UTC, before (the host trailed NTP by about 154 ms: not run)
+0.154425 +/- 0.078209 time.apple.com
+0.153544 +/- 0.017777 time.google.com
+0.154229 +/- 0.027352 time.cloudflare.com
16:34:06 UTC, after the step
-0.000450 +/- 0.078950 time.apple.com
-0.001972 +/- 0.017728 time.google.com
-0.001768 +/- 0.027305 time.cloudflare.com
16:36:13 UTC, just before the dry run
-0.002212 +/- 0.078958 time.apple.com
```
The host clock was within 2.2 ms of NTP, slightly ahead, for the run.

**Preparation, all clean.** PostgreSQL 16.15 (Homebrew), §0's SQL and migration applied, `e2_app`
logs in with `search_path = journal`; release build from this branch. The paper account was ACTIVE,
not trading- or account-blocked, `accrued_fees` 0, equity above 10,000, 0 positions, 0 open
orders; `GET /v2/assets/SPY` id `b28f4066-5c6d-479b-a2af-85dc1a8f16fb` (ARCA, tradable, active).
Bars 2026-08-31 to 2026-10-15: 33 rows, `inspect` `problems: 0`, 0 true gaps, 0 splits. The Nasdaq
Trader directory was reachable this time (DEC-930 item 3's gap is closed for this run):
```
ACT Symbol|Security Name|Exchange|CQS Symbol|ETF|Round Lot Size|Test Issue|NASDAQ Symbol
SPY|State Street SPDR S&P 500 ETF Trust|P|SPY|Y|40|N|SPY
```
`spy.json` dated `2026-10-16T00:00:00Z`, `etp: plain`. The mandate grep printed `allocation_usd`,
`max_gross_exposure_usd` and `max_position_usd` 10000 and `max_order_usd` 5000. Control stream:
```
opened ctl:ws_paper01 as event 69772S86W1YMVYJV0SV0N719K7 at seq 1
registered as event 0KBXZP8123BJ0SMKMDJ099TBC8 at seq 2   (fee_config)
registered as event 4R4MRRQZG8SYH38TSPY3B11QWS at seq 3   (trading_calendar)
registered as event 1M9996A876WKMX32PEC9V5YKZQ at seq 4   (rule_set)
registered as event 2B203DGB7B1MTJ4HVJ0H… at seq 5 (instrument_snapshot; id cut, typos misreads its tail)
registered as event 7ETJKWB14PKV4D6P276P19R279 at seq 6   (model quant.ma_crossover 1.0.0)
created sha256:07d55fc806872d1c2568be9a9f49006e09a8855ec9a17a11ddd6c61a26780b21 as event 1SFXDPBPRQRWN9C3Y9FY4ZW9HS at seq 7
code 2a56a26c / warnings W-002
confirmed as event 40SEXPS7HK50CXBTMZXX3JTAWJ at seq 8
code 0c66e84e / warnings W-002
deployed as event 636N00RGGRNRK8QMDCE7AFV1SX at seq 9
```

**Dry run at 16:36:13 UTC**, inside the regular session:
```
the tracer stopped at Journal: the journal answered Invalid
exit=1
```
The clock fix worked: the run passed Reconcile, Validate, the data and the model (the output was
`Long`), and the gate's dry run allowed the proposal. It stopped later. A local build of this
branch with temporary `eprintln!` lines (never committed; the branch is unchanged) showed two
defects, one masking the other:

1. **The classification facts are stated for one share.** This blocks every run that sizes more
   than one increment, and it is deliberate (DEC-877 item 3; see the corrections below). `run_context` (`crates/mandate-shell/src/paper/context.rs`) builds the builder
   context's `action` with `buy_action(..., artifacts.instrument.increment_qty)`, which is one share.
   The builder then sized the proposal at the $5,000 cap: 6 SPY at limit 766.84. `BuilderPlan::classify`
   (`crates/mandate-shell/src/adapters.rs`) requires `action.order_usd == qty × limit`
   (`action_order_matches`): 766.84 against 4,601.04, so it refused `classification facts for the
   proposed action`. That fails closed, which is correct, but no opening above one share can ever
   be classified. The paper tests pass because their mandate (`max_order_usd` 300 against a 255.2
   ask) sizes exactly one share.
2. **The fail-safe `deny` is not a valid record.** On a classifier error, `Bridge::classify`
   (`crates/mandate-shell/src/tracer.rs`) returns `Autonomy::Deny` with `decided_by: None`. The
   runtime writes a `DecisionMade` with `dry_run: allow`, `autonomy: deny` and `decided_by: null`,
   and the journal refuses it (`Invalid { reason: Schema, path: "payload.decided_by" }`, journal
   spec §9.1 rule 5: `decided_by` is non-null exactly when `autonomy` is). So the run stops at
   Journal, and the report never names the Classify cause. Fail-closed still holds; only the
   reason is lost.

Both are safety-critical changes (the shell's classification bridge and the paper context): tests
first, mutants, and the different-model review. Per the brief, nothing was worked around: no
mandate change (a version with `max_order_usd` between one and two shares is the founder's
envelope decision, rule 11, and the brief said not to change the mandate), and no placing run, so
§6 and §8 did not run.

**After stopping.** `GET /v2/orders?status=open&limit=500&direction=asc&nested=true` returned `[]`
and `/v2/positions` 0 rows, so there is no filled entry and no legs to classify as nested or top
level. Exports: `ctl:ws_paper01` seq 1 to 9 `result: verified` (last hash
`5546fd5a4a8b34d00124531f4c52fd8f13b9af0136735fd4df10e39b59755f7d`). The agent and account streams
are `export_stream_empty` (the dry run journals nothing durable).

**Runbook corrections.**
- §0 on macOS: `brew install postgresql@16 && brew services start postgresql@16`, then
  `psql postgres` replaces `sudo -u postgres psql`. `sed -i` needs `sed -i ''` on macOS.
- §0 clock on macOS: `sntp time.apple.com` gives the offset. This laptop trailed NTP by 154 ms with
  "Set time automatically" on; `sudo sntp -sS time.apple.com` (in a real terminal, since it asks for
  a password) stepped it to under 3 ms.
- §3: a `max_order_usd` above one share (the runbook's $5,000) cannot pass the paper run's
  classification. This is by design, not a defect: DEC-877 item 3 sizes the paper run at one whole
  share, and item 4 keeps lifting that for the founder, because a partly filled multi-share entry
  would have its protective order refused by the one-order guard (rule 13). Set `max_order_usd`
  above one ask and below two.

### The one-share run, same host (17:00 to 17:20 UTC): the order filled with its bracket, then the run stopped at Journal

**Founder decisions (typed in the session).** (1) A new mandate version with `max_order_usd` 1000
(SPY ask about 766, so the builder sizes exactly one share), with allocation and the other caps
still 10000: an envelope decision (rule 11). (2) The version was refused in `ws_paper01`
(`refused (version_invalid)`; a diagnostic build showed `{V002, V006}`: `agent_spy` on the first
version already claims SPY, and the CLI has no `agent stop`). The founder chose a fresh workspace
`ws_paper02`. `ws_paper01`'s agent holds nothing and never runs again, and the run's preflight still
requires a flat account.

**Clock.** macOS `timed` pulled the clock back to 162 ms behind NTP within 25 minutes of the step,
then it drifted about 0.5 ms a minute. For the run the founder turned network time off and ran
`sntp -sS time.apple.com` every 20 s as root, so the clock was set from NTP throughout:
```
17:13:48 UTC, before the dry run: -0.000283 (apple)  +0.000389 (google)  +0.000788 (cloudflare)
17:13:59 UTC, before the placing run: +0.001948 (google)  +0.002343 (cloudflare)
```

**ws_paper02 control stream** (seq 1 to 9): workspace, the same four configuration objects and
model, version `sha256:90a0d73dbd83e46bea093fb70818943d964a618a068b25929efa392d07a06c41` created
at seq 7, `code 4385ce3b / warnings W-002`, confirmed at seq 8, `code 8775704d / warnings W-002`,
`agent_spy` deployed at seq 9.

**Dry run, 17:13:52 UTC**, exit 0:
```
would place md-00000100000000000005000001 (nothing sent; pass --place-one-order)
```

**The one placing run, 17:13:59 to 17:14:08 UTC**, exit 1, nothing on stdout:
```
the tracer stopped at Journal: the journal answered Invalid
```
The order had gone out and filled. The broker's record: `md-00000100000000000005000001`, broker id
`68099755-c7e6-40ea-8ba2-5dc644393428`, buy 1 SPY limit 763.76, `order_class: bracket`, created
17:14:02.217Z, **filled 1 at 763.75** at 17:14:02.621Z. Its two nested legs: a sell `limit` (`new`)
and a sell `stop` (`held`). The account stream holds the protection at stop 725.58 and take-profit
840.14. Position: 1 SPY at 763.75.

**The journal** (all three `result: verified`):
- `agent:ws_paper02:agent_spy` seq 1 to 7: StreamOpened, AgentModeChanged x2, ObservationRecorded,
  ModelOutputRecorded, DecisionMade (buy 1), IntentProposed. Last hash
  `5412689680d5c3999a0dd70176651f709f0dfe42f53baf164fc9745f88f64e7b`.
- `acct:ws_paper02:01J8Z2ACCT00000000000000A1` seq 1 to 13: StreamOpened, AccountStateObserved,
  AccountSnapshotRecorded, ReconciliationRun, IntentReceived, ProtectionChanged, GateDecided,
  OrderRequestRecorded, **OrderSubmitted** (`md-00000100000000000005000001`, qty 1, limit 763.76,
  tif gtc), OrderStateChanged `accepted`, OrderStateChanged `filled` (filled_qty 1),
  ProtectionChanged `placed` (qty 1, stop 725.58, take-profit 840.14), AccountStateObserved. Last
  hash `62c61a5a0829e1d97ddd66d6edd729f7032a30164cd942069941c9b86f44e5a5`.
- `ctl:ws_paper02` seq 1 to 9. Last hash
  `21b18e71aa017e71c8888afa515770ed5e596eb6e87cd6c71bc3eaffed34e12d`.

Exactly one `OrderSubmitted`. Neither stream holds a `FillApplied`, and the journal does not record
the fill price (`OrderStateChanged` version 1 has no price member; the broker gives 763.75).

**Why the run stopped: the executor's `FillApplied` does not meet the journal's own contract.**
`fill()` (`crates/mandate-executor/src/orders.rs`) writes `FillApplied` through `batch.journal`,
which attaches empty `config_refs`, with payload members `instrument` and `fees` as text. The journal
requires, for `FillApplied` and `LateFillApplied`, the config refs `fee_config`,
`trading_calendar`, `settlement_calendar` and `instrument_snapshot` (journal spec §9 catalogue,
`crates/mandate-journal/src/catalogue.rs`). Its version-1 schema has `instrument_id`, and `fees` is a
list of `{kind, amount, asset, status}` (`FILL_APPLIED_V1`, `crates/mandate-journal/src/schema.rs`).
A throwaway probe (never committed) ran `Draft::parse` on a draft shaped the way the executor
builds it, with this run's envelope:
```
as the executor writes it:              Invalid { reason: MissingConfigRef, path: "config_refs.fee_config" }
with the four config refs added:        Invalid { reason: Schema, path: "payload.instrument" }
refs + instrument_id + fees as a list:  accepted
```
So every fill the executor records is refused, on main too (`orders.rs` is unchanged there for
these members), and the paper path has no `settlement_calendar` to cite: nothing registers one. The
refusal fails closed after the protection was already recorded, so the position was protected, but
the platform's accounting never applied the fill. This is safety-critical (accounting, the executor,
the journal contract): tests first against the journal's own draft check, mutants, and the
different-model review.

**§7a, the open-orders read after the fill** (`status=open&limit=500&direction=asc&nested=true`,
saved unedited outside the repo). It returns one order: the take-profit leg, broker id
`58a1eef5-7397-48ea-b0c0-bb1154cb1bda`, sell limit qty 1, `new`, `order_class: bracket`, at **top
level** with `legs: []`. The filled entry is not listed (it is not open), and the `held` stop is not
listed at all. So for DEC-878 item 4: with `status=open`, a filled bracket's legs are not nested
under the entry. Only the working take-profit appears, at top level, and the held stop is absent.
(`status=all&nested=true` does show the filled entry with both legs nested.)

**§8, the second run, was not made.** The placing run exited non-zero, and the brief says to
record a refusal and stop, so no second `--place-one-order` was sent. The position and its bracket
stay at the broker. The second-run refusal is still to be shown, on the next run day, once the
founder decides.

### §8 run, same laptop, main at `12a693a4` (19:23 to 19:30 UTC): refused, nothing sent

**Build.** Main at `12a693a4` (it includes #1369, DEC-938) does not compile on macOS: the
hardening in `crates/mandate-paper/src/harden.rs` (#1354, DEC-937 item 4) imports
`rustix::process::{DumpableBehavior, set_dumpable_behavior}`, which exist on Linux only:
```
error[E0432]: unresolved imports `rustix::process::DumpableBehavior`, `rustix::process::set_dumpable_behavior`
```
With the founder's approval, the binaries were built with a local patch that was never committed
and was reverted straight after the build. On a target other than Linux, `disable_core_dumps`
returns `Err(PaperError::Harden)`, so a live run refuses rather than reading a credential with core
dumps on. The paper run never calls it (`Ports::harden` runs for a live connection only). The
proper fix needs its own safety-critical PR.

**Before the run (read-only).** Alpaca paper: the position is 1 SPY at 763.75. Entry
`68099755-c7e6-40ea-8ba2-5dc644393428` is `filled`, with its nested legs: take-profit
`58a1eef5-7397-48ea-b0c0-bb1154cb1bda` (sell limit 840.14, `new`) and stop
`a59d69f1-c452-49a3-95ea-32c96f5d990f` (sell stop 725.58, `held`). The market was open. Clock:
```
19:27:15 UTC: -0.003889 (time.apple.com)  -0.003597 (time.google.com)  -0.003653 (time.cloudflare.com)
```
The streams before the run: agent seq 1 to 7 (`5412689680…`), account seq 1 to 13
(`62c61a5a08…`), control seq 1 to 9 (`21b18e71aa…`).

**The one invocation**, `$P $RUN --place-one-order`, 19:27:30.888 to 19:27:32.741 UTC:
```
stdout: (empty, 0 bytes)
stderr: the tracer stopped at Validate: a paper account with no position and no open order
exit=1
```
The stage prefix that §8 marks UNVERIFIED is `Validate`.

**Proof that nothing was sent or journaled.**
- **Re-exports to new files, each `result: verified`, unchanged:** agent seq 1 to 7, last hash
  `5412689680d5c3999a0dd70176651f709f0dfe42f53baf164fc9745f88f64e7b`. Account seq 1 to 13, last
  hash `62c61a5a0829e1d97ddd66d6edd729f7032a30164cd942069941c9b86f44e5a5`. Control seq 1 to 9,
  last hash `21b18e71aa017e71c8888afa515770ed5e596eb6e87cd6c71bc3eaffed34e12d`.
- **One `OrderSubmitted`:** no stream grew. The agent stream has no `OrderSubmitted`; the account
  stream still has exactly one (seq 9, `md-00000100000000000005000001`).
- **Alpaca after the run:** `status=open&nested=true` lists only the take-profit leg `58a1eef5-…`
  (sell limit 1, `new`, top level). `status=all&nested=true` since 2026-10-16 lists only the
  filled entry `68099755-…`, with its two nested legs. So no second order exists. The position is
  still 1 SPY.

**The first fill is still not journaled.** The account stream has no `FillApplied` or
`LateFillApplied`. The refusal comes from the preflight judge (`Validate`), before the production
cycle and its startup reconciliation run, so #1369's fill path never ran. This is a gap, not a
failure of §8. While the position is open, every paper run is refused at the same check, so no
paper run can ever book this fill, and the fill price (763.75) lives only at the broker. Booking it
needs a path that reconciles without placing. That is a new story for the founder to schedule, and
it must not loosen the flat-account check.

**Booking path (DEC-955).** The gap above is closed by `--reconcile-only` (section 8a): run it once
on this account to book the 763.75 fill as a `LateFillApplied`, then re-export the account stream.
A placing run now books the same way before its preflight and is still refused at Validate while
the position is open.

