# R0 rehearsal against `mandate-rh-sim`: what passes on `main`, and what refuses where

A status record for R0 of the [first live trade brief](first-live-trade.md) (steps 1 and 2: the
build, then the rehearsal against the simulator, DEC-124). An agent ran every founder command in
order against `mandate-rh-sim` on loopback and a private Postgres journal, never a broker
([DEC-922](../decisions/DEC-922.md): the `rh-sim` build is the only one that dials loopback, and
it refuses any endpoint that is not plain `http` on `127.x`, [DEC-937](../decisions/DEC-937.md)
item 2). It used no credential and placed no order (`AGENTS.md` rules 7 and 8). Claim
[#1371](https://github.com/kunwarshivam/mandate/issues/1371).

**Result with R1 fixed (`claude/r0-connection-checked`, [DEC-946](../decisions/DEC-946.md)):**
the builds, the simulator, the journal, the first configuration commands and now the connect
check pass: the runner's `--connect-check` (new step 2d2) logs in on the simulator's page, reads
the contract and the agentic account, and journals `StreamOpened` and a passing
`ConnectionChecked` on the account stream, and `connection record` confirms it (2e). The
rehearsal now stops at 2f: the journal refuses the live `ConnectionEstablished` on the control
stream with `environment_mismatch`, because `workspace open` opened that stream in `paper`
([R1b](#the-blocker-chain-beyond-step-2f)). Every later step refuses as a consequence, as before.
Behind R1b, the four refusals R2 to R5 still stand, and the grant (item 9) and the restart's
`Unknown` adoption (item 10) are being built in their lanes.

**Result on `main` at `bf150da`** (before R1): the rehearsal stopped at `connection record`,
which refused `check_missing` because no command wrote the `ConnectionChecked` it requires.

## How to run it

```bash
scripts/r0-rehearsal.sh                # builds, then rehearses; exits 0 only if every step passes
scripts/r0-rehearsal.sh --no-build     # reuse the binaries in CARGO_TARGET_DIR
```

The script builds into its own `CARGO_TARGET_DIR` with `CARGO_INCREMENTAL=0` unless one is set.
It uses `MANDATE_PG_URL` (a superuser DSN; a schema of its own is created and dropped) or else
starts a private PostgreSQL cluster on a random loopback port and stops it at exit, and migrates
`migrations/0001_journal.sql` as `mandate_journal_owner`, as the journal tests do. It writes each
command with its exit status, stdout and stderr to `$WORK/steps.md`. The simulator's OAuth login
page is answered with the same two requests a browser makes when the founder presses Approve.

**Why not a CI job.** The rehearsal runs a binary built with `live`, and DEC-529 item 3 (enforced
by `cargo xtask live-feature`, X1) allows exactly one CI job to enable that feature, and only to
compile it. So the script is the harness; CI only ShellChecks it (`cargo xtask ci fast`'s lint now
covers `scripts/`). The whole run takes under a minute after the builds.

## The rehearsal mandate and script

- **Mandate:** the CLI's SPY fixture made `live` on connection `conn_rh_rehearsal_01`, with
  DEC-529's limits: `max_order_usd` 100 (also the allocation, `max_position_usd` and
  `max_gross_exposure_usd`), one pinned US equity ETF, and `ask` for every opening (no `auto`
  rule). The ticker is the founder's (DEC-529 item 2); the rehearsal uses a made-up symbol,
  `RHETF`, which matters at [R5](#the-blocker-chain-beyond-step-2f).
- **Simulator script:** one agentic account with 100 USD cash and buying power, the regular
  session, `RHETF` quoted at 85, and one scripted buy fill at 85.

## The step table (`claude/r0-connection-checked`, `d2bf0db`)

`ws_r0`, `founder`, `agent_r0`, the account ref and the check id are the rehearsal's opaque ids.
A row may group several of the script's step ids: 2c is `2c`, `2c-fee`, `2c-cal` and `2c-rules`;
2q and 2r are `2q-ctl`, `2r-ctl`, `2q-agent`, `2r-agent`, `2q-acct` and `2r-acct`.
`$J` is `--journal $DSN --store $WORK/store`.

| Step | R0 | Command | Result | Output (last line) |
|---|---|---|---|---|
| 1a | 1 | `cargo build --bins -p mandate-cli -p mandate-rh-sim` | PASS | `Finished` |
| 1b | 1 | `cargo build --bins -p mandate-paper --features live,rh-sim,mandate-mcp/rh-sim` | PASS | `Finished` |
| 2a | 2 | `mandate-rh-sim --script sim.json`, then `GET /.well-known/oauth-protected-resource/mcp` | PASS | `{"authorization_servers":["http://127.0.0.1:<port>/oauth"],"resource":"http://127.0.0.1:<port>/mcp"}` |
| 2b | 2 | `mandate workspace open --workspace ws_r0 $J` | PASS | `opened ctl:ws_r0 as event … at seq 1` |
| 2c | 2, 3 | `mandate config register --kind instrument_snapshot instrument.json` (and `fee_config`, `trading_calendar`, `rule_set` from the tracer fixtures) | PASS | `registered as event … at seq 2` to `5` |
| 2d | 2, 3 | `mandate model register quant.ma_crossover 1.0.0` | PASS | `registered as event … at seq 6` |
| 2d2 | 2, 3 | `MANDATE_RH_SIM_ENDPOINT=<sim> MANDATE_RH_ACCOUNT_NUMBER=<number> mandate-paper --connect-check --workspace ws_r0 --account-ref <ref> --connection conn_rh_rehearsal_01 --journal $DSN --confirm-live`, the login answered on the simulator's page | PASS | `checked 10000100000000000000000001` (was missing on `main`: R1) |
| 2e | 2, 3 | `mandate connection record --environment live --connection conn_rh_rehearsal_01 --broker robinhood --account-ref <ref> --checked <2d2's id> --margin-attestation cash_account --scope …` (the nine allowlisted tools; show the code) | PASS | `code …` (was `refused (check_missing)`) |
| 2f | 2, 3 | the same with `--code` | **FAIL** | `the journal: refused: Invalid { reason: EnvironmentMismatch, path: "environment" }` (R1b) |
| 2g | 2, 3 | `mandate version create mandate.json --environment live` | **FAIL** (from 2f) | `refused (paper_only)` |
| 2h, 2i | 2, 3 | `mandate version confirm <version> --environment live` (show, then `--code`) | **FAIL** (from 2g) | `refused (version_unknown)` |
| 2j, 2k | 2, 3 | `mandate agent deploy agent_r0 <version> --environment live` (show, then `--code`) | **FAIL** (from 2g) | `refused (version_unknown)` |
| 2l | 2, 4 | `MANDATE_RH_SIM_ENDPOINT=<sim> mandate-paper --workspace ws_r0 --agent agent_r0 --account-ref <ref> $J --bars <dir> --confirm-live` | **FAIL** (from 2k) | `no confirmed deployment: no AgentDeployed names the agent` |
| 2m | 2, 5 | the same with `--place-one-order`, the login answered on the simulator's page | **FAIL** (from 2k) | the same; no login URL was printed |
| 2n to 2p | 2, 5 | `mandate approvals list`, `show`, `approve <id> --code <code>` (`cli_confirm`) | **FAIL** (from 2m) | `no approvals`; nothing to show or grant |
| 2q, 2r ctl | 2, 6 | `mandate journal export --journal $DSN ctl:ws_r0 ctl.jsonl`; `mandate journal verify ctl.jsonl --store …` | PASS | `verified, stream ctl:ws_r0, seq 1 to 6, …` |
| 2q, 2r acct | 2, 6 | the same for `acct:ws_r0:<ref>` | PASS | `verified, stream acct:ws_r0:<ref>, seq 1 to 2, …` (2d2's `StreamOpened` and `ConnectionChecked`) |
| 2q, 2r agent | 2, 6 | the same for `agent:ws_r0:agent_r0` | **FAIL** (from 2m) | `export_stream_empty` |
| 2s | 2, 7 | the placing run again: must refuse before any place call | **FAIL** (from 2k) | refused, but `no confirmed deployment`, not the lapse |

### The refusals observed, and the line responsible

| Step | Refusal | Where | Cause | Lane |
|---|---|---|---|---|
| 2f | `environment_mismatch` (the journal's `Invalid`) | `crates/mandate-journal/src/lib.rs:703` (`append`), reached from `crates/mandate-cli/src/connection.rs` (`commit_caused`) | A stream's environment is its first event's, and `mandate workspace open` opens `ctl:{workspace}` in `paper` (`crates/mandate-cli/src/workspace.rs:42`), so the `live` `ConnectionEstablished` cannot follow it. The CLI's own tests seed a journal that does not check this | **None.** See [R1b](#the-blocker-chain-beyond-step-2f) |
| 2g | `paper_only` | `crates/mandate-cli/src/version.rs:90` (`admitted`) | A `live` gesture needs the owner's recorded, unrevoked Robinhood `live` connection; 2f recorded none | Follows from 2f |
| 2h to 2k | `version_unknown` | `crates/mandate-cli/src/gestures.rs:134` (confirm) and `:165` (deploy) | No `MandateVersionCreated` names the version (the script passes `sha256:none`, having none) | Follows from 2g |
| 2l, 2m, 2s | `not_deployed` (`no AgentDeployed names the agent`) | `crates/mandate-shell/src/control.rs:242`, reached through `crates/mandate-paper/src/lib.rs:408` | No deployment; the door (`deployed_document`) holds no document, so the shell's fold answers | Follows from 2k |

## The blocker chain beyond step 2f

Not observed, because 2f stops the rehearsal first (R1 is fixed and observed passing); each is
read from the code, in the order a run reaches it once the one before is fixed.

| # | Refusal | Where | Why | Lane | Proposed fix |
|---|---|---|---|---|---|
| R1 | `check_missing` at `connection record` | `crates/mandate-cli/src/connection.rs:294` | No connect sequence journaled `ConnectionChecked` | **Fixed** in `claude/r0-connection-checked` ([DEC-946](../decisions/DEC-946.md)): steps 2d2 and 2e pass | The fix as built: a connect-check mode in the one binary allowed to dial the broker, the runner (for example `mandate-paper --connect-check --workspace … --account-ref … --connection … --confirm-live`), which logs in (O1b), lists the tools and hashes the contract, reads `get_accounts`, runs `mandate_connections::checks::run`, and appends `StreamOpened` and `ConnectionChecked` (occasion `connect`) on `acct:{workspace}:{account_ref}`, printing only the event id for `--checked`. Safety-critical (connectors, OAuth): a DEC-921 row with its tests first. The CLI cannot do it (`forbidden_internal` keeps it off the connectors) |
| R1b | `environment_mismatch` at `connection record --code` (2f) | `crates/mandate-journal/src/lib.rs:703` | `workspace open` opens the control stream in `paper` only, and the journal fixes a stream's environment at its `StreamOpened`, so no `live` control record (connection, version, deployment) can ever be appended to it | **None** | A decision first: either `workspace open --environment live` opens a live workspace's control stream in `live` (the founder's live workspace is then a separate workspace from any paper one), or the control stream's events carry the workspace's environment and the gesture's own environment moves into the payload. The first keeps the journal's one-environment-per-stream rule and needs only the CLI's `open` and the shell's control fold to accept a live `ctl` stream; the rehearsal would then open `ws_r0` with `--environment live` |
| R2 | `lapse_evidence_not_supplied` | `crates/mandate-paper/src/lib.rs:404` passes `None` as the snapshot, so `door.rs:122` calls `unsupplied` (`door.rs:176`) | The door's lapse needs the agentic account's `AccountSnapshot`, which only the Robinhood connector reads, and `run()` hands none until the runner's ports connect that connector (DEC-933 item 3, DEC-928 item 5) | None named (DEC-928 item 5's "later lanes") | Connect the Robinhood session before the lapse, after `acknowledge` and `harden`: door's `chosen` and `account_stream` → `acknowledge` (`--confirm-live`, core dumps off) → `connect` (the login) → `RobinhoodBridge::preflight_facts`' snapshot → `lapse_read(Some(snapshot))` → the shell's fold. This moves the credential read in front of the fold for a live run only, so it needs a decision amending FT-3's "every check that needs no credential first" for that case, or the fold split so its credential-free checks still run first |
| R3 | `DeploymentRefusal::Live` | `crates/mandate-paper/src/lib.rs:402` hands `LiveAdmission::refusing()`; refused at `crates/mandate-shell/src/control.rs:254` | DEC-934 item 4: every caller passes the refusing value until the runner holds DEC-924's four inputs | None named | In `run()`, build `LiveAdmission` from `Build::CURRENT`, `args.acknowledged`, `DoorChecks::Passed` only after R2's real `chosen` and `lapse_read` returned `Ok`, and `McpHost { dialed: the endpoint's host, pinned: PINNED_HOST }`. The rehearsal build has `live`, so it pins Robinhood's host, but it dials the loopback endpoint, so DEC-934 item 3's exact host equality still refuses there: the rehearsal needs the host condition to accept the loopback endpoint in an `rh-sim` build only (DEC-922 item 1's reading), a change to DEC-924 item 1's fourth condition that needs its own decision |
| R4 | `Cause::Absent` with `SESSION_UNBUILT` at `Stage::Reconcile` | `crates/mandate-paper/src/robinhood.rs:220` to `235` (`Linked::preflight`, `Linked::cycle`) | The login's token is held but no MCP session carries it (O2); a Robinhood run stops after the login, before any read | None named | Build the `RobinhoodBridge` (DEC-928 item 2) over an `McpClient` connected on `Linked`'s runtime with the login's token, and route `preflight` to `robinhood_facts` (DEC-933 item 2) and `cycle` to the shell's Robinhood cycle |
| R5 | `Stage::Reconcile` refusals of the market reads | `mandate_shell::paper::market_facts` (`crates/mandate-shell/src/paper/facts.rs:145`) and `robinhood_facts` (`:171`) | A Robinhood run's preflight reads the asset record, IEX quote and minute bars from **Alpaca's** data API (DEC-933 item 2), and `--bars` must come from `mandate download`, so the rehearsal needs the founder's Alpaca data keys, which no agent has (rule 8); and the simulator's quote must name the same instrument and agree with the real IEX quote within the collar (DEC-529 item 12), so the rehearsal must pin the founder's real ticker and script the simulator's quote near its price, not `RHETF` | Not a code defect | The founder runs the rehearsal on their own machine with their data keys, after `mandate download` and `inspect`, and sets the simulator's `quotes` from the latest IEX ask (the script's `sim.json`, one line) |
| R6 | the `ask` opening denied at its deadline | `crates/mandate-paper/src/wiring.rs:296` (`wait_for_the_grant`) | Until item 9 lands, the runtime is never handed the founder's grant, so the opening is skipped at `timeout_s` (`on_timeout: skip`, rule 3) even after `mandate approvals approve` commits it | **Item 9**, [#1367](https://github.com/kunwarshivam/mandate/pull/1367) (`claude/live-grant-cycle-answers`, DEC-940) | — |
| R7 | the restart (R0 step 7) | the startup reconciliation | A restart after a lost place answer must adopt exactly one matching order or keep it `Unknown` (DEC-529 item 4) | **Item 10** (`claude/unknown-adoption-run-path`) | — |

So the rehearsal can pass only after R1b to R4 land in addition to items 9 and 10, and only on the
founder's machine (R5). Each touches safety-critical code (the CLI's control stream, the runner,
the shell's live admission, the connector) and needs its own lane.

## Findings that need no code

- **The `rh-sim` feature does not imply `mandate-mcp/rh-sim`** (DEC-926 item 2, DEC-937 item 2):
  the runner must be built with `--features live,rh-sim,mandate-mcp/rh-sim`. The brief's R0 step 1
  says `--features live`; the rehearsal build needs all three.
- **`mandate-cli` has no features**: it is built without any, for the rehearsal and the live run.
- **A simulator quote is canonical decimal text**: `"85"` parses, `"85.00"` refuses the whole
  script (`quotes` is "missing, unreadable"), by `Script::parse`'s strict reading (DEC-926).
- **The private journal works on PostgreSQL 16** with the one migration applied by `psql` as the
  owner role; CI's floor is 17.

## Reproducing a single step

Each step's full output is in `$WORK/out/<step>.out` and `.err`, and `$WORK/steps.md` holds them
all with their commands. Event ids repeat across runs because the CLI derives them (DEC-290).
