# Passkey relying party (E9-1)

- **Spec:** identity spec §6.1, §6.3, §7.2 step 4; DEC-660 (dependencies, algorithms, and the
  strict readings).
- **Code:** `mandate-passkey`, `crates/mandate-passkey/` (layer 2, pure, safety-critical):
  `src/lib.rs` (`enrol`, `verify`, `RelyingParty`, `Challenge`, `Credential`, `PublicKey`,
  `Refusal`).
- **Tests:** `crates/mandate-passkey/tests/api.rs` (refusal codes, the challenge length) and
  `tests/oracle.rs` (the software authenticator in `tests/common/mod.rs` against RFC 4648 and
  RFC 8949 vectors and `ring`'s verifier; its keys are generated in the test, never a real
  authenticator, identity spec §1.3).
- **Run:** `cargo nextest run -p mandate-passkey`.
