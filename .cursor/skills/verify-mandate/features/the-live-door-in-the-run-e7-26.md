# The live door in the run: run()'s call of the connection and lapse seams (E7-26)

- **Spec:** `docs/project/tasks/first-live-trade.md` (row G1a's run-level half — "the G1a live
  door compile-in" of the #1308 review's R0-gap list and the M6 work-tracker row; "The
  connector chosen from the connection record and the environment from the confirmed mandate;
  the `live` feature; LT-1's build test; the lapse check: refuse unless the account is flat and
  the connection record is unspent (DEC-529 item 3)"); `docs/project/decisions/DEC-910.md`
  items 1 to 3; `docs/project/decisions/DEC-529.md` item 3;
  `docs/project/decisions/DEC-918.md` (this row's readings); LT-1, LT-14, FT-3; the founder's
  run's step 4 ("the exact refusal"); #1308's "Not done" line ("The G1a door in `run()`. Per
  this dispatch, `connection()`/`lapse()` are unchanged and unwired") and #1297's ("wiring the
  seams into `run()` ahead of every credential read (FT-3)").
- **Code:** `mandate-paper`, `crates/mandate-paper/` (layer 9, safety-critical):
  `crates/mandate-paper/src/door.rs` — `chosen` (the connection the run uses: the records the
  run's own single control-stream read hands verbatim and in journal order, the deployed
  mandate, and the run's own `--account-ref`; every refusal of the door reaching the run as
  `PaperError::Connection`, so its report names the door's refusal ahead of the shell's own
  `DeploymentRefusal::Live` and the V-rules that answer three of its cases with less exact
  names), `account_stream` (journal spec §2's `acct:{workspace_id}:{account_ref}` derived from
  the run's own names — the stream whose rows the lapse reads, through the same journal the
  control stream came from, ungated by `--place-one-order`; a form §2 cannot hold fails closed
  `Cause::Absent` at the reconcile stage, never a fallback onto another stream), and
  `unspent` (DEC-529 item 3's lapse wrapped for the run: the journal's own account-stream rows
  and the connector snapshot's own `Held` counts, re-derived by nothing; every refusal reaching
  the run as `PaperError::Lapse`) — re-exported by `crates/mandate-paper/src/lib.rs`. Every
  stub reports `Unimplemented { story: "E7-26" }` per DEC-137 until this row's implementation
  lands; the call sites inside `run()` (`chosen` before every credential read, `unspent` before
  anything runs, both ahead of the shell's own live refusals) are that implementation's to
  write and its review's to judge, per DEC-914 item 7's precedent (no pin of this PR edits
  `run()`, `connection.rs` or any shell file; the G1b frames' call sites stay stream L / M7's
  remainder's, the shell watch's loop the shell row's, and the live arm's connector, preflight
  and account-stream journal read the door implementation's under these signatures).
- **Tests:** `crates/mandate-paper/tests/door.rs`, every pin pending on E7-26 until the row's
  implementation lands:
  `a_paper_run_through_the_door_keeps_todays_connection`,
  `the_door_names_its_own_refusal_where_the_shell_would_answer_live`,
  `the_lapse_reads_the_account_stream_derived_from_the_runs_own_names`,
  `a_stream_the_journal_cannot_form_refuses_the_run`,
  `a_fill_on_the_runs_own_account_stream_spends_the_record_through_the_door`,
  `the_flat_half_refuses_ahead_of_the_spend_with_the_snapshots_own_counts`, and
  `a_paper_connection_makes_no_lapse_check_through_the_door` — pinning V-001's paper default
  and LT-14 (the run's own account stream carried, no flat or spent check on a paper
  connection, whatever it holds), the exact refusal the run's report holds where the shell
  would answer a V-rule or `live` (the revoked paper record, the record binding another
  account stream, and LT-1's runtime build test — the `connector_not_built` arm a locally
  built `--features live` turns by design, no CI job ever running it there), §2's account
  stream form and type (another run's names naming another run's stream; a form the journal
  cannot hold failing closed, never a fallback stream), the bound stream alone spending, and
  DEC-470 item 1's counts carried as data, re-derived by nothing.
- **Run:** `cargo nextest run -p mandate-paper` (the living suite green, the row's pins
  ignored); `cargo nextest run -p mandate-paper --run-ignored ignored-only` (every pin failing
  at its stub's own report until the implementation lands);
  `cargo check -p mandate-paper --features live` (the frames compile under the founder's
  feature, which no CI or release build enables).
