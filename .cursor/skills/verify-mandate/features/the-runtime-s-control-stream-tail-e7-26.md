# The runtime's control-stream tail (E7-26)

- **Spec:** `docs/project/tasks/first-live-trade.md` (LT-4's "one door, one grant"; R0 step 5's
  "the founder reads it and grants it with `cli_confirm`"); `docs/project/tasks/M7-escalation-v0.md`
  ("Answering": "The owner's answer arrives only as a control-stream
  `ApprovalResponseSubmitted`, tailed and handed in as `Input::Journal`";
  "Decisions needed" 5); DEC-155 (before DEC-344, in `docs/project/04-decision-log.md`; items 2 and 5: "The notifier
  driver and the control-stream tail belong in the shell"); `docs/project/decisions/DEC-912.md`
  item 1 (journal order); `docs/project/decisions/DEC-914.md` items 2 (verbatim, never
  re-sorted, never filtered) and 7 (the call sites are the implementation PR's and its review's);
  `docs/specs/mandate.md` §6.4; `docs/specs/journal.md` §2, §3 (§3 v0.22's `client`),
  §9.2, §9.7; the #1308/#1311 reviews' R0-gap row "the runtime grant→submit continuation
  (stream L / M7's remainder)".
- **Code:** `mandate-shell`, `crates/mandate-shell/` (layer 8, safety-critical):
  `crates/mandate-shell/src/tail.rs` (`tailed` — the fold of one read of the workspace control
  stream into the `FoldedEvent`s the runtime's `Input::Journal` carries, one per row, verbatim,
  in the journal's own order, never filtered; the payload and `causation_id` read from the row's
  own canonical body bytes, the same reading `Session::fold` gives the agent and account
  streams; the actor mapped from the envelope's own `actor.kind`, `user` folding
  `ActorKind::User` and every other kind — the `client` of journal §3 v0.22 included — folding
  what admission never admits, a kind the tail cannot read giving `System`, never `User`, per
  the `FoldedEvent` contract's fail-closed sentence; a body the tail cannot read, and a body
  holding no `payload` member, refusing as a difference with nothing handed), exported by
  `crates/mandate-shell/src/lib.rs`. **Pending stub only**: the install — `Session`'s fold of
  the tailed events, at the stream open's replay and at the live grant wait's per-wake re-read
  (DEC-914 item 2's "a read taken again on every wake"), with the cursor each starts from — is
  the implementation PR's, written in `crates/mandate-shell/src/tracer.rs` (DEC-475 keeps the
  `Session` private to the crate) and judged by its review against DEC-914 item 7's reservation;
  nothing in this PR changes what any caller can do.
- **Tests:** `crates/mandate-shell/tests/tail.rs`, every pin `pending E7-26` and failing at the
  stub's own report (DEC-77, DEC-137) until that PR: the owner's answer reaching the runtime
  verbatim (the ctl stream the row names, its own seq, event id and event type, its body's own
  payload, the envelope's own `user` actor — never the tracer's blanket `System`); the journal's
  own order handed whole (every row of the read, an `OwnerCommandIssued` and an `AgentDeployed`
  row kept beside the answers, seqs 3, 9 and 31 staying the rows' own, never the read's
  indices); the fail-closed actor map (journal §3's `system`, `agent`, `broker` and
  `platform_operator` folding their own kinds, `client` and an actor holding no readable kind
  folding `System`, never `User`); and the unreadable row refusing as a difference with nothing
  handed (`Envelope`'s own `stored body` and `payload` refusals, the same as the agent and
  account folds'). The shell's living suite (188 tests at #1326's count,
  `tests/control.rs` and `tests/watch.rs` included) is untouched and stays green through the
  implementation.
- **Run:** `cargo nextest run -p mandate-shell` (the living suite, every green test unchanged);
  `cargo nextest run -p mandate-shell --run-ignored ignored-only --no-fail-fast` (the four
  pending pins, each failing at the stub's own report until the implementation PR).
