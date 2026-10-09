# Workspace API contract (E10-10)

- **Spec:** `docs/specs/workspace-api.md` §3.1 to §3.5, §5; DEC-436, DEC-680 to DEC-683, DEC-686, DEC-689.
- **Code:** `mandate-api`, pure: `crates/mandate-api/src/problem.rs` (RFC 9457 problem document,
  closed `code` and `effect`), `crates/mandate-api/src/wire.rs` (decimal, ref, timestamp, asset,
  id, and event-id scalars as canonical strings; `decode` and `encode`),
  `crates/mandate-api/src/idempotency.rs` (the key and the derived event id),
  `crates/mandate-api/src/envelope.rs`, `requests.rs`, and `responses.rs` (the envelope's shapes,
  the closed requests, plain and conditional, and the closed responses, `Validate` stubbed; the
  kill switch's scope a tagged enum). Stubbed.
- **Tests:** `crates/mandate-api/tests/contract.rs`, pending E10-10: §3.5's statuses parsed from the
  spec; §3.4's ids against a `sha2` and Crockford oracle; scalars as canonical strings, a JSON
  number in a decimal refused; the problem's closed enums spelled as the spec spells them, its
  per-code titles pinned, `current_base` exactly for `stale_base`, `outcome_unknown` carrying only
  `unknown`. Live: the problem's and the policy violation's serde shapes (`event_id` a required
  member, the two policy levels). Live, #993's two decode minors (#1104): a custom refusal
  located at an object's last member, at an array's item, at a map's last entry, and at an
  internally tagged object, and bytes that are not UTF-8 inside a string `malformed` at `""`; and
  a member's name escaped in its pointer as RFC 6901 says (#1112). Live, through
  `wire::object_only!`, each derived shape read only from a JSON object: DEC-881, an internally
  tagged object (`Actor`, the kill switch's `scope`, a problem's violation) written as a JSON
  array, `[]` included, refused as `type` at its member or array item; DEC-882, a derived struct
  (a whole request, response, problem, or envelope shape; a nested record, step-up, delegation,
  bid, watermark, or step) written as a JSON array refused as `type` at `""`, its member, or its
  array item.
- **No body names a workspace:** `crates/mandate-api/tests/body_workspace.rs`, live (#1130, #560
  minor 6): every request schema under `schemas/workspace-api/commands/` and every request type in
  `requests.rs` has a decoder row; each example, and each `.valid.json` case applied to it, reaches
  every object its schema describes (nested ones through `$ref`), and `workspace`, `workspace_id`,
  or `ws` added to any of its objects is `unknown_member` at that member, or at the internally
  tagged object holding it; no request schema names such a member; a kill switch's workspace scope
  naming any workspace is refused, `null` accepted.
- **Lenient decoder:** `crates/mandate-api/src/lenient.rs`, `decode_lenient` for the six API-7
  bodies (DEC-682 item 27, DEC-689 item 2, DEC-886): members read in body order by its own scan,
  each through the strict decoder under its pointer. `crates/mandate-api/tests/lenient.rs`, live:
  every case of
  `schemas/workspace-api/examples/lenient/api7.json`, its `dropped` exactly and its kept value
  against the body with those pointers removed; a body that is not a JSON object read as `{}` with
  `[""]` for pause and hold and refused for the other four; API-4's comparison over the members
  kept; each member of each operation's full example corrupted (wrong type, garbage, an unknown
  nested member), dropped and listed when not hard and refused when hard, against a hard-member
  list typed from §5; a `workspace`, `workspace_id` or `ws` member at the root, in a record, or in
  the hard `scope`, dropped and listed and never moving the stop (#1155); duplicates; and DEC-886's
  items 2, 4 and 9.
- **Schemas:** `crates/mandate-api/tests/schemas.rs`: live enum and member drift against
  `schemas/workspace-api/` (DEC-683), each member `null`-able and optional exactly as its schema
  says, a kill switch scope's `null` id exactly for `workspace`, and no `serde(flatten)`; pending,
  every example round-trips, every `.invalid` case is refused, and the integer bounds hold at their
  edges.
- **Run:** `cargo nextest run -p mandate-api --run-ignored all` (fails at the stubs until E10-10).
