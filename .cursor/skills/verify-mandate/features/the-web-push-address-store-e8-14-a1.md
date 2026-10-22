# The web-push address store (E8-14 A1)

- **Spec:** `docs/specs/workspace-api.md` §4.11, §5.7 ("The vault's part", "The sweep");
  `docs/specs/journal.md` §9.15 ("The active set"); DEC-2120; DEC-2123; DEC-692 (the vault's
  users, keys, and refusals it follows).
- **Code:** `mandate-vault-addresses`: `crates/mandate-vault-addresses/src/startup.rs` (the
  address keys as systemd credentials, the token and pending keys refused, the vault root and
  `addresses/` opened directory-relative with exact owners and modes), `src/store.rs` (the
  writer's operations, the read-only reader, the ids and values, each secret a `secrecy` type),
  `src/maintenance.rs` (the re-seal tool's handle: counts by key and re-seals, never erases),
  `src/error.rs` (`AddressError`, opaque ids only).
- **Tests:** `crates/mandate-vault-addresses/src/tests/` (a fresh layout per test in a temporary
  directory; the error vocabulary, id parsing, startup refusals, the writer's and reader's
  operations, file checks, the sweep's listing, key rotation, the re-seal tool, and a canary through every error and
  `Debug` output); the `compile_fail` doctests on `AddressReader` (it has no write method).
- **Run:** `cargo nextest run -p mandate-vault-addresses --run-ignored all` and
  `cargo test -p mandate-vault-addresses --doc`.
