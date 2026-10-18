# The local vault (E10-13 V1)

- **Spec:** `docs/specs/connections.md` §5.2; `docs/design/infrastructure.md` §3.1, §5.2;
  DEC-692; DEC-822 item 4; DEC-976.
- **Code:** `mandate-vault-local`: `crates/mandate-vault-local/src/startup.rs` (the two keys as
  systemd credentials, the token key never in the API process, exact directory owners and modes),
  `src/store.rs` (ChaCha20-Poly1305 entries, the per-file inode, link, owner and mode checks,
  each role's refusals), `src/entry.rs` (the associated data and file names), `src/port.rs` (the
  connect core's `Vault` port), `src/import.rs` (the client-secret import and the service users'
  ids), `src/error.rs` (`VaultError`); `crates/mandate-cli/src/vault.rs` (`mandate vault import`).
- **Tests:** `crates/mandate-vault-local/src/tests/` (a fresh layout per test in a temporary
  directory; startup refusals, entries, tampering, file checks, the port, the import, and a canary
  secret through every path); `crates/mandate-cli/tests/vault_import.rs` (the binary prints no
  canary and takes no secret as an argument).
- **Run:** `cargo nextest run -p mandate-vault-local` and
  `cargo nextest run -p mandate-cli --test vault_import`.
