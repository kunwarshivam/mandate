# The workspace API server: process, probes, drain, and the single 401 (E21-31)

- **Spec:** [workspace API spec](../../../../docs/specs/workspace-api.md) §3.3 (CORS), §3.5 (the
  problem document), §3.11 (the server process: bind, environment, probes, drain), API-1;
  [infrastructure design](../../../../docs/design/infrastructure.md) §3.4, §7.3, §9; DEC-822
  item 4; DEC-978.
- **Code:** `crates/mandate-api-server`. `src/config.rs` (the start checks: a loopback or pod-local
  bind, no secret-named variable, no password in the database URL, the session signing key from
  `$CREDENTIALS_DIRECTORY` with owner-only permissions), `src/lifecycle.rs` (`Lifecycle`, the
  local readiness predicate until `mandate-process` lands, and `Drain`), `src/http.rs` (the router:
  `/livez`, `/readyz`, CORS for the one app origin, and the one 401 for every other path), and the
  `mandate-api-server` binary (`--bind`, `--pod-address`; SIGTERM drains).
- **Tests:** `tests/config.rs`: every start refusal, and that no refusal prints a secret's value.
  `tests/http.rs`: the probes' bodies and statuses, readiness during a drain, the identical 401
  for every method and path, and CORS for the app origin and for any other origin.
  `tests/process.rs`: the binary refuses a public bind, a secret in its environment, and a missing
  key, and on SIGTERM drains and exits 0.
- **Run:** `cargo nextest run -p mandate-api-server`.
