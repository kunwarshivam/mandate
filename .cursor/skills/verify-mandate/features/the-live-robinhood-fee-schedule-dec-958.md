# The live Robinhood fee schedule (E7-26, DEC-958)

- **Spec:** `docs/project/decisions/DEC-958.md` (the R0 rehearsal's R10); trading-domain spec
  §6.2 (US equities fees, the day charge `round(total, 2, ceiling)`); DEC-957 item 3 (the fee
  schedule names the mandate's environment); DEC-526 item 4 (the paper-only schedule).
- **Code:** `crates/mandate-executor/src/fees.rs` — `ROBINHOOD_LIVE`, `ROBINHOOD_LIVE_SCHEDULE`
  and `robinhood_live_fee_config`; `crates/mandate-shell/src/paper/artifacts.rs` — `fee_artifact`
  and `fees` (each schedule's members and reader); `config/fees/robinhood-live-2026-10-16.toml`
  (the cited transcription) and `config/fees/robinhood-live-2026-10-16.json` (the registered
  object `scripts/r0-rehearsal.sh` registers).
- **Tests:** `crates/mandate-executor/tests/live_fees.rs` (the schedule is the transcription
  figure for figure and the published rates exactly; a live sell books exactly SEC, TAF and CAT
  on the reference amounts; a buy books only CAT; any environment but `live` and any broker but
  `robinhood` refuse; a property that the day charge is the exact cost rounded up to the cent,
  against an integer oracle, and that a sell's gap to Robinhood's per-fee rounding stays under two
  cents); `crates/mandate-shell/tests/live_artifacts.rs` (a live run priced by the live schedule
  builds its artifacts; a paper run refuses it either way; another broker or another member set
  refuses).
- **Run:** `cargo nextest run -p mandate-executor -p mandate-shell`.
