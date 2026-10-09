# Authentication: the reduction-session gate (E9-1)

- **Spec:** [identity spec](../../../../docs/specs/identity.md) §6.4 route 2; DEC-833, DEC-834,
  DEC-653.
- **Code:** `mandate-authn`, `crates/mandate-authn/src/reduction.rs` (layer 2, pure): `ReductionGate`
  (outstanding challenges and recent failures per address and per device, pruned as time advances)
  and the one refusal `Unauthenticated`, implemented (slice A3).
- **Tests:** `crates/mandate-authn/tests/reduction.rs` (a verified assertion never refused for a
  limit; verification only against an outstanding challenge, consumed on first use by whichever
  client presents it; the 300 s lifetime to the nanosecond; the challenge and failure limits per
  address and per device; every refusal indistinguishable; and random histories against a model
  folded from the history alone), all live.
- **Run:** `cargo nextest run -p mandate-authn`.
