# The runtime's control-stream tail (E7-26)

- **Spec:** `docs/project/tasks/first-live-trade.md` (LT-4's "one door, one grant"; R0 step 5's
  "the founder reads it and grants it with `cli_confirm`"); `docs/project/tasks/M7-escalation-v0.md`
  ("Answering": "The owner's answer arrives only as a control-stream
  `ApprovalResponseSubmitted`, tailed and handed in as `Input::Journal`";
  "Decisions needed" 5, and the restart row: "A control-stream response committed before the crash
  is re-tailed and judged at `max(submitted_at, folded clock)`"); DEC-155 (before DEC-344, in
  `docs/project/04-decision-log.md`; items 2 and 5: "The notifier driver and the control-stream
  tail belong in the shell"); `docs/project/decisions/DEC-912.md` item 1 (journal order);
  `docs/project/decisions/DEC-914.md` items 2 (verbatim, never re-sorted, never filtered) and 7
  (the call sites are the implementation PR's and its review's); `docs/specs/mandate.md` §6.4;
  `docs/specs/journal.md` §2, §3 (§3 v0.22's `client`), §9.2, §9.7; the #1308/#1311 reviews'
  R0-gap row "the runtime grant→submit continuation (stream L / M7's remainder)".
- **Code:** `mandate-shell`, `crates/mandate-shell/` (layer 8, safety-critical): the tail is
  implemented since [PR #1330](https://github.com/kunwarshivam/mandate/pull/1330) deleted the
  stub body. `crates/mandate-shell/src/tail.rs` (`tailed` — the fold of one read of the workspace
  control stream into the `FoldedEvent`s the runtime's `Input::Journal` carries, one per row,
  verbatim, in the journal's own order, never filtered; the payload and `causation_id` read from
  the row's own canonical body bytes, the same reading `Session::fold` gives the agent and account
  streams; the actor mapped from the envelope's own `actor.kind`, `user` folding
  `ActorKind::User` and every other kind — the `client` of journal §3 v0.22 included — folding
  what admission never admits, a kind the tail cannot read giving `System`, never `User`, per
  the `FoldedEvent` contract's fail-closed sentence; a body the tail cannot read, and a body
  holding no `payload` member, refusing as a difference with nothing handed), exported by
  `crates/mandate-shell/src/lib.rs`. The install, written by the same PR inside
  `crates/mandate-shell/src/tracer.rs` (DEC-475 keeps the `Session` private to the crate), per
  DEC-914 item 7's reservation: `Session::tail_the_control_stream` takes one read of
  `ctl:{workspace}` at the open — the journal port's read only, no ownership taken, so the CLI's
  writer stays unfenced (journal §2, DEC-131 item 18) — after `Input::Started`'s recovery and
  the cycle's one-order guard and before the cycle proposes anything, and hands each folded
  event to the runtime as `Input::Journal`, every row, the whole read each time: the runtime's
  own copy fold is the idempotence (EI-3, DEC-155 item 2), so no cursor is kept. The control
  rows are never state-folded into the runtime's replay: `Input::Journal` is the answer's whole
  path, and the runtime's fold reserves what it does not interpret as `NotInterpreted` (DEC-85) —
  journal §9.2's deployment and registration records among them, which the shell reads through
  the validation context, never through the runtime — so a state fold of the read would refuse a
  workspace whose control stream the tail reads whole. The **per-wake re-read of the live grant
  wait** (DEC-914 item 2's "a read taken again on every wake") is the runner's call site
  (`mandate-paper`'s `wait_for_the_grant`), the G1b frames' live-arm row's, outside this crate's
  boundary; the shell watch's loop is the shell LT-10 row's.
- **Tests:** `crates/mandate-shell/tests/tail.rs`, every pin live since
  [PR #1330](https://github.com/kunwarshivam/mandate/pull/1330) un-ignored them (the pin bodies
  byte-identical to the tests PR's): the owner's answer reaching the runtime verbatim (the ctl
  stream the row names, its own seq, event id and event type, its body's own payload, the
  envelope's own `user` actor — never the tracer's blanket `System`); the journal's own order
  handed whole (every row of the read, an `OwnerCommandIssued` and an `AgentDeployed` row kept
  beside the answers, seqs 3, 9 and 31 staying the rows' own, never the read's indices); the
  fail-closed actor map (journal §3's `system`, `agent`, `broker` and `platform_operator`
  folding their own kinds, `client` and an actor holding no readable kind folding `System`,
  never `User`); and the unreadable row refusing as a difference with nothing handed
  (`Envelope`'s own `stored body` and `payload` refusals, the same as the agent and account
  folds'). The shell's living suite (188 tests at #1326's count,
  `tests/control.rs` and `tests/watch.rs` included) stays green through the implementation; the
  call-site/read discipline the pins cannot bind (the open-replay read, its whole-stream cursor,
  the per-wake remainder) is DEC-914 item 7's review vector over this PR's diff, named in its
  body's planted-wiring table.
- **Run:** `cargo nextest run -p mandate-shell` (the living suite, 192 passing and 9 skipped —
  the sibling LT-10 row's pending pins — at this PR's head);
  `cargo nextest run -p mandate-shell --run-ignored ignored-only --no-fail-fast` (the sibling
  LT-10 row's nine pending pins, each failing at that row's own untouched stub).
