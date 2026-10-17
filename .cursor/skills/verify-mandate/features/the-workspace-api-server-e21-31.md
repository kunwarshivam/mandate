# The workspace API server: process, probes, drain, sessions, the first reads, and the single 401 (E21-31)

- **Spec:** [workspace API spec](../../../../docs/specs/workspace-api.md) §3.3 (CORS), §3.5 (the
  problem document), §3.11 (the server process: bind, environment, probes, drain), API-1;
  [infrastructure design](../../../../docs/design/infrastructure.md) §3.4, §7.3, §9; DEC-822
  item 4; DEC-978.
- **Code:** `crates/mandate-api-server`. `src/config.rs` (the start checks: a loopback or pod-local
  bind, no secret-named variable, no password in the database URL, the session signing key from
  `$CREDENTIALS_DIRECTORY` with owner-only permissions), `src/lifecycle.rs` (`Lifecycle`, the
  local readiness predicate until `mandate-process` lands, and `Drain`), `src/http.rs` (the router:
  `/livez`, `/readyz`, CORS for the one app origin, the session routes with their CSRF check
  first, and the one 401 for every other path), `src/session.rs` (`Sessions`: the HMAC'd
  `__Host-` cookie over `mandate-authn`'s `SessionRecord`, refresh onto route 1's outage, lapsed
  access keeping pause and the kill switch, caps), `src/subject.rs` (the subject-link file of
  digests), `src/token.rs` and `src/keys.rs` (the issuer's access token against its fetched key
  set), `src/clock.rs`, and the `mandate-api-server` binary (`--bind`, `--probe-bind`,
  `--pod-address`, `--drain-bound-s`; SIGTERM drains).
- **Tests:** `tests/config.rs`: every start refusal, and that no refusal prints a secret's value.
  `tests/http.rs`: the probes' bodies and statuses, readiness during a drain, the identical 401
  for every method and path, and CORS for the app origin and for any other origin.
  `tests/process.rs`: the binary refuses a public bind, a secret in its environment, and a missing
  key, and on SIGTERM drains and exits 0.
  `tests/session.rs`: the cookie's format against an independent HMAC, forged and foreign
  cookies, lapsed access, rotation and reuse, outage and failed refreshes, sign-out, the caps, the
  idle and absolute lifetimes. `tests/subject.rs`: the digest against a hand-built canonical
  object, ownership and mode, malformed and duplicate lines. `tests/keys.rs`: URL rules, a fetched
  key set checking tokens, a failed fetch keeping the last set. `tests/sessions_http.rs`: the
  session routes, CSRF first with 403, every other refusal the one 401, the cookie's attributes.
  `tests/reads.rs`: the folds against hand-built rows and hand-written views. `tests/reads_http.rs`:
  the read routes' 401, 404 (absent, foreign, unjoined), 403, 503s, `reduction_only`, and bodies
  and watermarks.
- **Run:** `cargo nextest run -p mandate-api-server`.
