# Authentication: the reduction-session gate (E9-1)

- **Spec:** [identity spec](../../../../docs/specs/identity.md) §6.4 route 2; DEC-833, DEC-834,
  DEC-653.
- **Code:** `mandate-authn`, `crates/mandate-authn/src/reduction.rs` (layer 2, pure): `ReductionGate`
  and the one refusal `Unauthenticated`, stubbed (slice A3, tests first).
- **Tests:** `crates/mandate-authn/tests/reduction.rs`, pending but for the refusal's fixed text.
- **Run:** `cargo nextest run -p mandate-authn --run-ignored all`; `cargo xtask ci pending`.
