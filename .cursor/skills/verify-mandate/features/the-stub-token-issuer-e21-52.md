# The stub token issuer for dev and CI cells (E21-52, lane SI)

- **Spec:** [workspace API spec](../../../../docs/specs/workspace-api.md) §3.11 items 2 and 5;
  [identity spec](../../../../docs/specs/identity.md) §6.1 (the stub issuer); DEC-2100.
- **Code:** `crates/mandate-stub-issuer`. `src/start.rs` (`check`: a build without debug
  assertions, then the tier, then a loopback IP literal), `src/issuer.rs` (`Issuer`: the P-256 key
  made at start, the key set, the ES256 access tokens), `src/serve.rs` (`serve`: `GET /jwks.json`,
  `POST /token`, the `Host` check, no CORS), and the `mandate-stub-issuer` binary (`--bind`,
  `--audience`; SIGTERM stops it).
- **Tests:** `crates/mandate-stub-issuer/tests/`: `start.rs` (the refusals and their order),
  `issuer.rs` (tokens checked with `mandate_authn::verify`, a new key each start, subject
  refusals), `http.rs` (421, 415, 400, 413, 404, no CORS), `process.rs` (each refusal alone on
  standard error with exit 2, the announced issuer, SIGTERM), and `sign_in.rs` (`POST /v1/sessions`
  on `mandate-api-server` with the stub's key set fetched over loopback).
- **Run:** `cargo nextest run -p mandate-stub-issuer`.
