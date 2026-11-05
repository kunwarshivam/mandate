# Your first live order, in one page

Everything here is yours to run, on your Mac, with your own login. No agent runs any step, sees
your credentials, or reads the run's output. Nothing below is a credential: your Robinhood login,
your account number, and your Alpaca data keys are typed by you and never appear in a file, a
command history you share, or a message to an agent.

The full engineering detail behind this page is the [first live trade brief](first-live-trade.md)
(DEC-529); the rehearsal's history is [r0-rehearsal.md](r0-rehearsal.md).

## 1. Install, once (10 minutes)

1. A checkout of `main`, and Rust from rustup (it reads `rust-toolchain.toml`).
2. PostgreSQL 18: `brew install postgresql@18 && brew services start postgresql@18`, then
   `export PATH="$(brew --prefix postgresql@18)/bin:$PATH"`.
3. Skip `.cursor/install.sh` on a Mac — its tools are x86_64 Linux builds and R0 needs none.
4. Keep the Robinhood app signed in on your phone: the login may ask for approval there.

## 2. Rehearse first (DEC-124; about 15 minutes, mostly builds)

The rehearsal runs the same live build against the simulated Robinhood on your machine. Nothing
real is touched and no login of yours is used — its simulated login page answers itself.

```bash
scripts/r0-rehearsal.sh --sim-clock 2026-10-30T15:00:00Z
```

Pick a `--sim-clock` that is a **Tuesday to Friday, 11:00 New York** (the model's output goes
stale over a weekend, DEC-969 item 7; a Monday fails by design). Or run it without `--sim-clock`
between 09:30 and 15:50 New York on a trading day. **You should see:** 33 rows, every one `PASS`,
ending with step `2u -> the connection record is spent`. Exit 0. The full record is printed at
the end; keep it.

## 3. Prepare the live account (in the Robinhood app)

- The **agentic account** is flat: no position, no open order.
- It is a **cash account**, or margin is disabled.
- It holds about **100 USD of settled cash** — the most this order can spend.

## 4. Build and set up (once, outside market hours is fine)

```bash
cargo build --release --bins -p mandate-cli
cargo build --release --bins -p mandate-paper --features live
export PATH="$PWD/target/release:$PATH"

createdb mandate_r0
mandate db migrate --journal "postgres://$USER@127.0.0.1:5432/mandate_r0"
export DSN="postgres://$USER@127.0.0.1:5432/mandate_r0?options=-c%20search_path%3Djournal%20-c%20role%3Dmandate_journal_app"
export STORE="$HOME/mandate-r0/store" MARKET="$HOME/mandate-r0/market"
mkdir -p "$STORE" "$MARKET"
J=(--journal "$DSN" --store "$STORE")
O=(--workspace ws_live --user founder --environment live)
```

Then follow **"The R0 runbook: every command, in order"** in the [first live trade
brief](first-live-trade.md), parts B and C exactly: open the live workspace, download and
`inspect` the bars for your ETF (under ~90 USD a share), register the instrument snapshot, the
live fee schedule (`config/fees/robinhood-live-2026-10-16.json`), calendar, rule set and model,
then the connect check — typing the account number into `MANDATE_RH_ACCOUNT_NUMBER` **in that
command, by hand** — and `connection record` with `--margin-attestation cash_account`. The login
opens in your browser; approve on the phone if Robinhood asks. **You should see:** `checked
<EVENT>` from the connect check, `recorded conn_rh_live_01` from the record, and `registered as
event …` from every register.

Your Alpaca paper data keys (`MANDATE_ALPACA_PAPER_KEY_ID`, `MANDATE_ALPACA_PAPER_SECRET`) are
exported in your own shell only, for the bars and the quote cross-check. They are never written
to a file here.

## 5. The mandate (once)

Write `$HOME/mandate-r0/mandate.json` as the brief's runbook part D shows (its python snippet
builds it from the CLI's SPY fixture): `ask` for every opening, allocation and `max_order_usd` /
`max_position_usd` / `max_gross_exposure_usd` all 100, your one ETF pinned, and
`crypto_stop_limit_offset` `0.01` (without it, `version confirm` refuses). Create, confirm and
deploy — read the warnings; W-002 is expected. **You should see:** `confirmed …` then
`deployed …`, each after you re-run with `--code`.

## 6. The day (between 09:30 and 15:50 New York, Tuesday to Friday)

In one terminal, the run; in a second, your approvals. Every run prints a Robinhood login address
— open it, log in, approve on the phone if asked. No token survives between runs.

1. **Dry run, nothing sent** — the same command as below without `--place-one-order`. You should
   see: `the tracer stopped at Classify: the proposal classified ask`, and nothing sent. A `Flat`
   model or any refusal ends the day; do not work around it.
2. **The placing run** — add `--place-one-order` and leave it running. The order appears in your
   approvals inbox:

   ```bash
   mandate approvals list --agent agent_r0 "${O[@]}" "${J[@]}"
   mandate approvals show <APPROVAL> --agent agent_r0 "${O[@]}" "${J[@]}"   # read it, note the code
   mandate approvals approve <APPROVAL> --agent agent_r0 --code <CODE> "${O[@]}" "${J[@]}"
   ```

   Read the request before approving: **one share of your ETF, a limit at most 100 USD.** Doing
   nothing skips it at its deadline, safely. **You should see:** `Admitted and sent to the
   executor; the gate still decides.`, then `submitted <ENTRY>` and `submitted <ENTRY>-p…` (the
   resting GTC stop-limit sell), and the run exits 0.

## 7. Check

- **Robinhood app, agentic account:** history shows one filled buy of one share; orders shows one
  working GTC stop-limit sell; positions shows one share.
- **Journal:** export and verify each stream; each must answer `verified`:

  ```bash
  for s in ctl:ws_live agent:ws_live:agent_r0 "acct:ws_live:$REF"; do
    mandate journal export --journal "$DSN" "$s" "$HOME/mandate-r0/${s%%:*}.jsonl"
    mandate journal verify "$HOME/mandate-r0/${s%%:*}.jsonl" --store "$STORE"
  done
  ```

- **The restart sends nothing:** the same placing command is refused before any place call. While
  the share is held: `the account the connection binds is not flat`. After you sell:
  `the connection record is spent`. Either refusal is the pass.

## 8. While you hold the share, and how to stop at any point

- The stop rests at Robinhood with nothing running. Its GTC expiry is not published: **check it in
  the app every few days** and re-place it by hand if it lapsed (DEC-529 item 7).
- **Stop the run:** Ctrl-C in the run terminal. Nothing is sent after it stops.
- **Stop the agent:** the CLI commits your command to the journal for a running stream owner.
  `pause` and `kill` take the agent as `--agent` and need no code; `stop` and `exit` take it as a
  positional id and print a code you re-run with `--code`:

  ```bash
  mandate agent pause --agent agent_r0 "${O[@]}" "${J[@]}"
  mandate agent kill  --agent agent_r0 "${O[@]}" "${J[@]}"     # never refused
  mandate agent stop  agent_r0 "${O[@]}" "${J[@]}"             # then again with --code <CODE>
  ```

  `exit` also names the instrument: `mandate agent exit agent_r0 <TICKER> "${O[@]}" "${J[@]}"`.
  After R0's run exits, nothing runs to apply them, so the sure stop is the broker's:
  1. cancel the open order in the Robinhood app;
  2. sell the share to flatten;
  3. remove the MCP connection's access in Robinhood's settings.
- A second order on this record is refused by design; a fresh one needs a new connect check,
  record, version and deployment.

## 9. If a step fails

Send the last lines of the step's output — the rehearsal's `$WORK/steps.md`, or the run terminal's
last lines — with **no secrets** (no account number, no keys, no tokens; the logs never print
them, but check). A refusal is a finding, never something to edit or work around.
