# Workspace API contract (E10-10)

- **Spec:** `docs/specs/workspace-api.md` §3.1, §3.4, §3.5; DEC-436, DEC-680, DEC-681.
- **Code:** `mandate-api`, pure: `crates/mandate-api/src/problem.rs` (RFC 9457 problem document,
  closed `code` and `effect`), `crates/mandate-api/src/wire.rs` (decimal, ref, timestamp, asset,
  id, and event-id scalars as canonical strings; `decode` and `encode`),
  `crates/mandate-api/src/idempotency.rs` (the key and the derived event id). Stubbed.
- **Tests:** `crates/mandate-api/tests/contract.rs`, pending E10-10: §3.5's statuses parsed from the
  spec; §3.4's ids against a `sha2` and Crockford oracle; scalars as canonical strings, a JSON
  number in a decimal refused; the problem's closed enums spelled as the spec spells them, its
  per-code titles pinned, `current_base` exactly for `stale_base`, `outcome_unknown` carrying only
  `unknown`. Live: the problem's and the policy violation's serde shapes (`event_id` a required
  member, the two policy levels).
- **Run:** `cargo nextest run -p mandate-api --run-ignored all` (fails at the stubs until E10-10).
