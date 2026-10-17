# The founding grant: `mandate workspace found` (lane ID1)

- **Spec:** [identity spec](../../../../docs/specs/identity.md) §3.2, §4.3 (the owner bundle),
  ID-13; [journal spec](../../../../docs/specs/journal.md) §9.12 (`MemberActivated`, rules 96 to
  106, the fold, and the writer's **Order**); DEC-646, DEC-659, DEC-991.
- **Code:** `mandate-journal`, `crates/mandate-journal/src/membership.rs`: `MemberActivated`
  closed at schema version 1 with rules 96, 98 to 100, 103, 105, and 106; the other six
  `Member*` types stay unregistered. `mandate-identity`, `crates/mandate-identity/src/record.rs`
  (`control_entry`, the one mapping of a stored control-stream envelope to a `ControlEntry`, and
  `MEMBERSHIP_TYPES`), `src/founding.rs` (`OWNER_BUNDLE`, `founding_grant`, `founding_decision`,
  `found_workspace`), and `src/writer.rs` (`write_decided`, the one writer's attempt loop that
  `write_membership` and `found_workspace` share, so the founding decision is made on the view
  the append is made at). `mandate-cli`, `crates/mandate-cli/src/workspace.rs` (`found`,
  `found_in`, and the journal-backed `ControlStream`), with `founding_draft` in
  `src/control.rs`.
- **Tests:** `crates/mandate-journal/tests/member_activated.rs`: every `MemberActivated` case of
  the `membership` section of `fixtures/refcases/journal.json`, the founding grant's writer and
  stream, rule 106, and a property over random activations against an oracle of rules 98 to 105.
  `crates/mandate-identity/tests/founding.rs`: the grant, the decision, races, lost answers, the
  envelope mapping of every `membership` base draft, and a property over random bootstrap runs,
  restarts, races, and replays against a model of its own, shown to fail on a writer that skips
  the decision and on a wider grant. `crates/mandate-cli/tests/workspace_found.rs`: the command's
  arguments and refusals, the exact committed envelope (admitted by `Draft::parse`), fences,
  races, lost answers, and a property against an oracle reading the stored JSON.
  `crates/mandate-cli/tests/workspace_found_pg.rs`: the binary over Postgres (skipped without
  `MANDATE_PG_URL`).
- **Run:** `cargo nextest run -p mandate-journal -p mandate-identity -p mandate-cli`.
