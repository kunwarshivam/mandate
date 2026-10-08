# The local vault (E10-13 V1)

- **Spec:** `docs/specs/connections.md` §5.2; `docs/design/infrastructure.md` §3.1, §5.2;
  DEC-692; DEC-822 item 4.
- **Code:** `mandate-vault-local` (stubs until E10-13 lands): `crates/mandate-vault-local/src/startup.rs`
  (the two keys as systemd credentials, the token key never in the API process, exact directory
  owners and modes), `crates/mandate-vault-local/src/error.rs` (`VaultError`).
- **Tests:** `crates/mandate-vault-local/src/tests/` (a fresh layout per test in a temporary
  directory; startup refusals, pending E10-13).
- **Run:** `cargo nextest run -p mandate-vault-local --run-ignored all`.
