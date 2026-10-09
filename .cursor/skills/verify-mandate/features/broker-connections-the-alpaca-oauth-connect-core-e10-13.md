# Broker connections: the Alpaca OAuth connect core (E10-13)

- **Spec:** `docs/specs/connections.md` §5.2 to §5.5, §8.1; `docs/specs/workspace-api.md` §1.4,
  §4.5, §5.6; `docs/specs/journal.md` §9.8 rule 131; DEC-690, DEC-691, DEC-693, DEC-694,
  DEC-697, DEC-699, DEC-821.
- **Code:** `mandate-connections` (pure; stubs until E10-13 lands):
  `crates/mandate-connections/src/start.rs` (API process: the single-use `state` and the
  authorization URL), `crates/mandate-connections/src/exchange.rs` (token-exchange process: the
  code exchange behind `TokenEndpoint`), `crates/mandate-connections/src/vault.rs` (the `Vault`
  trait and the secret wrappers), `crates/mandate-connections/src/hosts.rs` (`LiveTokenRequest`,
  the only live-host request; `PaperRequest`; `admit`), `crates/mandate-connections/src/grant.rs`
  (exact scopes), `crates/mandate-connections/src/revoke.rs` (the ordinary and compromised
  revoke as typed plans, and their step-up digests), `crates/mandate-connections/src/manager.rs`
  (the connection manager's start of a connect, `ConnectionRequested`, as a typed plan, and the
  connect's step-up digest), `crates/mandate-connections/src/error.rs`
  (`ConnectError`).
- **Tests:** `crates/mandate-connections/src/tests/` (fixture vault and token endpoint; DEC-821
  items 2 to 4 and the `state` rules; `manager.rs` pending E10-13), and the `compile_fail` doctest in
  `hosts.rs`.
- **Run:** `cargo nextest run -p mandate-connections --run-ignored all`.
