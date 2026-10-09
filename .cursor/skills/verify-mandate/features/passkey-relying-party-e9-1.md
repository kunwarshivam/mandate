# Passkey relying party (E9-1)

- **Spec:** identity spec §6.1, §6.3, §7.2 step 4; DEC-660 (dependencies, algorithms, and the
  strict readings).
- **Code:** `mandate-passkey`, `crates/mandate-passkey/` (layer 2, pure, safety-critical):
  `src/lib.rs` (`enrol`, `verify`, `RelyingParty`, `Challenge`, `Credential`, `PublicKey`,
  `Refusal`), `src/client_data.rs` (`clientDataJSON`), `src/auth_data.rs` (authenticator data
  and its flags), `src/cbor.rs` (the CBOR subset), and `src/cose.rs` (COSE keys and signature
  verification with `ring`).
- **Tests:** `crates/mandate-passkey/tests/api.rs` (refusal codes, the challenge length) and
  `tests/oracle.rs` (the software authenticator in `tests/common/mod.rs` against RFC 4648 and
  RFC 8949 vectors and `ring`'s verifier; its keys are generated in the test, never a real
  authenticator, identity spec §1.3); `tests/enrol.rs` (enrolment: `none` attestation, the
  client data, the RP ID hash, the flags, the key types) and `tests/structure.rs` (credential ID
  lengths, the CBOR subset, COSE key parameters), `tests/verify.rs` (assertions: user
  verification, the counter, the origin, the challenge, the RP ID hash, the signature) and
  `tests/properties.rs` (a signed response verifies; any change to it is refused; the counter
  rule; no input panics).
- **Run:** `cargo nextest run -p mandate-passkey`.
