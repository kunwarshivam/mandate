# The vault behind `SecretSource` (E21-32)

- **Spec:** `docs/design/infrastructure.md` §5.2, §5.4, §5.6; DEC-434 item 14; DEC-977; E21-9's
  renewal rule; DEC-2019 (the cell's OpenBao).
- **Code:** `mandate-vault`: `crates/mandate-vault/src/source.rs` (`SecretSource`, `Grant`),
  `src/policy.rs` (the identity-by-path table and the OpenBao policies it generates),
  `src/lease.rs` (`Lease`, `LeaseKeeper`), `src/local.rs` (over `mandate-vault-local`),
  `src/openbao.rs` (the OpenBao client and `provision`), `src/error.rs` (`SourceError`).
- **Tests:** `crates/mandate-vault/src/tests/` (the policy matrix against a hand-written oracle,
  the generated policies, the lease rule as a property test, the seal check), and there
  `conformance.rs` (one suite), run by `local_vault.rs` and `openbao_server.rs` (skipped unless an
  OpenBao server is configured; unit tests, since only `cfg(test)` may reach a dev server).
- **The cell's vault:** `deploy/cell/charts/vault/` (OpenBao in server mode on raft, TLS 1.3,
  ingress only from `mandate.dev/egress-vault` pods), its invariants in `deploy/tests/test_vault.py`,
  and `vault()` in `xtask/src/cell.rs` (init once, unseal, refuse any storage but raft).
- **Run:** `cargo nextest run -p mandate-vault`; with OpenBao,
  `.github/scripts/start-openbao.sh` then `cargo xtask ci openbao` with the variables it prints.; the chart's invariants with `cargo xtask ci cell`'s chart checks, or
  `uv run --locked --directory python pytest -q ../deploy/tests/test_vault.py`.
