# The runner's Robinhood connect check, which `connection record` binds (DEC-946)

- **Spec:** `docs/specs/connections.md` §5.2 (the executor runs the broker-facing checks) and
  §8.1 checks 1, 2, 3 and 7; `docs/specs/journal.md` §9.8 (`ConnectionChecked`, rules 57, 58,
  62 and 68); `docs/project/tasks/r0-rehearsal.md` blocker R1; `docs/project/decisions/earlier-600-899.md#dec-875`
  item 1 (the account check); `docs/project/decisions/DEC-946.md` (this row's choices).
- **Code:** `mandate-paper`, `crates/mandate-paper/` (layer 9, safety-critical):
  `src/connect_check.rs` — `parse_check`, `connect_check` (the refusals before any login, core
  dumps off, `CheckPorts::observe`, `mandate_connections::checks::run`, and `StreamOpened` and
  `ConnectionChecked` in one batch on the account stream), `observe` (the account read through
  the Robinhood connector's own account check), and the drafts; `src/robinhood.rs` —
  `Linked::observe` (the MCP session over the login's token); `src/lib.rs` — `process`'s
  `--connect-check` and `ProductionCheck`. `mandate-mcp`, `crates/mandate-mcp/src/transport.rs`
  — `McpTransport::with_bearer`; `src/client.rs` — `McpClient::connect_to_check` and `Listed`.
  `mandate-connections`' `checks` decides every result.
- **Tests:** `crates/mandate-paper/tests/connect_check.rs` (the passing batch read back field by
  field and verified, a second check on an opened stream, every failed read journaled with its
  first failed check, the refusals before the login, a failed hardening or login journaling
  nothing, an append that did not commit, the arguments, the default binary's refusal, and the
  account read against `mandate-rh-sim`); `crates/mandate-mcp/src/tests/check.rs` (the bearer on
  every request and never printed, and the first connect's report of a clean, fund-moving or
  short contract).
- **Run:** `cargo nextest run -p mandate-paper -p mandate-mcp`; end to end against the simulator,
  `scripts/r0-rehearsal.sh` (steps 2d2, 2e and 2f).
