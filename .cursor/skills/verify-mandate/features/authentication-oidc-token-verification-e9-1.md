# Authentication: OIDC token verification (E9-1)

- **Spec:** [identity spec](../../../../docs/specs/identity.md) §6.1, ID-9; the
  [E9-1 brief](../../../../docs/project/tasks/E9-1-authn.md); DEC-211, DEC-650, DEC-820.
- **Code:** `mandate-authn`, `crates/mandate-authn/` (layer 1, safety-critical, pure): `src/lib.rs`
  (`Refusal` with its stable codes, `SetupError`, `TokenPart`), `src/oidc.rs` (`IssuerConfig`,
  `TokenKind`, `VerifiedSubject`, `verify`, `CLOCK_SKEW_S`, `MAX_TOKEN_BYTES`), and `src/jwks.rs`
  (`Jwks`, `Algorithm`), implemented (slice A1).
- **Tests:** `crates/mandate-authn/tests/oidc.rs` (each allowed algorithm verifies; `none`, HMAC
  and every other `alg` refused before a key is used, whatever the `kid`, the HMAC key-confusion
  attack with five encodings of the public key included; an ES256-only issuer; a header naming
  its own key or a critical extension; the `kid` and key-type checks, with a second ES256 key so
  a verifier ignoring `kid` is caught; another key's signature and the ES256 signature form; no
  claim read before the signature; the issuer byte for byte against another tenant signing with
  the same keys; each refusal's stable code and `TokenKind`'s nonce-free `Debug`), with the
  in-memory issuer in `tests/common/mod.rs` (keys made in the test, one fixed RS256 test key);
  `crates/mandate-authn/tests/claims.rs` (audience and `azp`, `exp` and `nbf` with the skew, the
  nonce, `email_verified` for invitations, and the validity window against its own oracle) and
  `crates/mandate-authn/tests/shape.rs` (malformed and oversized tokens, strict base64url from
  hand-written text, the key set's rules and weak or foreign keys, the issuer configuration, any
  single-byte change, and the ID-9 canary over every printed type), all live.
- **Run:** `cargo nextest run -p mandate-authn`.
