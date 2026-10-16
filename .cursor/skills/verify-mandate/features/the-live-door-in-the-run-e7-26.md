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
  seams into `run()` ahead of every credential read (FT-3)"), implemented since
  [PR #1328](https://github.com/kunwarshivam/mandate/pull/1328) landed the frames, the call
  sites and the probes.
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
  the run as `PaperError::Lapse`) — re-exported by `crates/mandate-paper/src/lib.rs`, each
  frame the seam's own call since PR #1328 implemented them. The call sites inside `run()`,
  written by the same PR per DEC-918 items 3 and 4: one `the_live_door` frame called between
  the control-stream read and `confirmed_version` — ahead of the shell's phase-1 fold and so
  ahead of every credential read — in the order `chosen` (over the deployment's own document,
  read by the `deployed_document` pre-read that mirrors the fold's walk to the parsed mandate
  and nothing else: no V-rule, no live-environment stop, every failure falling through to the
  fold's own refusal, so the door never answers where the shell refuses and only ever adds a
  refusal in front of the shell's), then `account_stream` (ungated by the placing flag), then
  `unspent` before anything runs — carrying the empty rows and counts of a live arm no build
  runs (the feature stays compile-only; the default build's `connector_not_built` stands in
  front of it, DEC-910 item 3) until the live arm's row hands the journal's own rows and the
  snapshot's own counts. Those empty rows and zero counts pass a live connection whatever the
  account holds, so DEC-919 adds `unsupplied` (pending, a stub until its implementation PR):
  the door's lapse call while it holds no evidence, refusing a live connection
  `lapse_evidence_not_supplied` (`LapseRefusal::NotSupplied`) and making no check on a paper
  one; its implementation PR replaces the placeholder `unspent` call in `the_live_door` with it,
  so only the live arm's own `unspent` call with real reads clears the refusal. The four run-level probes beside them live in `crates/mandate-paper/src/tests.rs`
  (the DEC-77 amendment's in-module slot): the revoked record, the live deployment, the
  foreign-account record and the fail-closed naming, each refusing through the door ahead of
  the fold and of every credential read (`run::every_refusal_before_the_credentials_reads_none`'s
  live arm moved with them, per the coordinator's ruling carried in PR #1328's body). The G1b
  frames' call sites stay stream L / M7's remainder's, the shell watch's loop the shell row's,
  and the live arm's connector, preflight and account-stream journal read the live arm's under
  these signatures.
- **Tests:** `crates/mandate-paper/tests/door.rs`, every pin live since
  [PR #1328](https://github.com/kunwarshivam/mandate/pull/1328) un-ignored them (the pin
  bodies byte-identical to the tests PR's):
  `a_paper_run_through_the_door_keeps_todays_connection`,
  `the_door_names_its_own_refusal_where_the_shell_would_answer_live`,
  `the_lapse_reads_the_account_stream_derived_from_the_runs_own_names`,
  `a_stream_the_journal_cannot_form_refuses_the_run`,
  `a_fill_on_the_runs_own_account_stream_spends_the_record_through_the_door`,
  `the_flat_half_refuses_ahead_of_the_spend_with_the_snapshots_own_counts`, and
  `a_paper_connection_makes_no_lapse_check_through_the_door`, and DEC-919's two pending pins
  `a_live_connection_with_no_lapse_evidence_refuses_to_start` and
  `a_paper_connection_with_no_lapse_evidence_starts_as_today` (each failing at the
  `unsupplied` stub until its implementation PR) — pinning V-001's paper default
  and LT-14 (the run's own account stream carried, no flat or spent check on a paper
  connection, whatever it holds), the exact refusal the run's report holds where the shell
  would answer a V-rule or `live` (the revoked paper record, the record binding another
  account stream, and LT-1's runtime build test — the `connector_not_built` arm a locally
  built `--features live` turns by design, no CI job ever running it there), §2's account
  stream form and type (another run's names naming another run's stream; a form the journal
  cannot hold failing closed, never a fallback stream), the bound stream alone spending, and
  DEC-470 item 1's counts carried as data, re-derived by nothing. `crates/mandate-paper/src/tests.rs`'s
  four run-level probes pin the call-site order DEC-918 item 4 left to the implementation's
  review (the revoked record's exact message, the live deployment's `connection_unrecorded`
  ahead of the shell's `live`, the foreign-account record's `connection_account`, and the
  fail-closed naming ungated by the placing flag — each of the four wrong wirings the review's
  hunt names failing a probe).
- **Run:** `cargo nextest run -p mandate-paper` (the suite passing, DEC-919's two pins ignored
  as pending); `cargo nextest run -p mandate-paper --run-ignored only` (the two pending pins,
  each failing at the `unsupplied` stub's report);
  `cargo check -p mandate-paper --features live` (the frames compile under the founder's
  feature, which no CI or release build enables).
