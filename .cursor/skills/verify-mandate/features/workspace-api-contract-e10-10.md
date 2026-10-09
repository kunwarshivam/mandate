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
  member, the two policy levels). Pending on behaviour (`xtask/behaviour-only/` rows), #993's two
  decode minors: a custom refusal located at an object's last member, at an array's item, and at
  an internally tagged object, and bytes that are not UTF-8 inside a string `malformed` at `""`.
- **Schemas:** `crates/mandate-api/tests/schemas.rs`: live enum and member drift against
  `schemas/workspace-api/` (DEC-683), each member `null`-able and optional exactly as its schema
  says, a kill switch scope's `null` id exactly for `workspace`, and no `serde(flatten)`; pending,
  every example round-trips, every `.invalid` case is refused, and the integer bounds hold at their
  edges.
- **Run:** `cargo nextest run -p mandate-api --run-ignored all` (fails at the stubs until E10-10).
