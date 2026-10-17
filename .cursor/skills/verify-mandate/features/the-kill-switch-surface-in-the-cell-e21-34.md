# The kill-switch surface in the cell (E21-34, lane O4)

- **Spec:** `docs/design/infrastructure.md` §3.6 and OPS-4; trading spec §5.5; journal spec §9.2
  (`OwnerCommandIssued`); mandate spec §6.1 and MI-23; DEC-2000; the runbook
  `docs/runbooks/RB-03-kill-switches.md`.
- **Code:** `crates/mandate-cli/src/brake.rs` (`mandate agent pause` and `mandate agent kill`,
  each with exactly one of `--agent`, `--connection` and `--workspace-wide`; `reached`, the agents
  a pause reaches; `pause`, one agent-scoped pause each; `kill`, one owner command of its scope over
  `agent::kill`); `xtask/src/cell.rs` (the `kill-switch` drill: `kill_switch`, `brake_args`,
  `brakes_expected`, `brakes_missing`; the `compose` drill's brakes).
- **Tests:** `crates/mandate-cli/tests/brake.rs` (the scope flags, the agents each scope reaches,
  one pause per agent, unreadable mandates paused, a failing pause not stopping the others, each
  scope's kill switch with no step-up, a repeated kill switch committed again, and a property over
  random deployments and stops against an independent oracle);
  `crates/mandate-cli/tests/brake_pg.rs` (the binary against Postgres); `xtask/src/cell.rs`
  (`the_brake_check_names_each_missing_brake`, `the_brakes_reach_each_scope_once`).
- **Run:** `cargo nextest run -p mandate-cli --test brake`; `MANDATE_PG_URL=postgres://… cargo
  nextest run -p mandate-cli --test brake_pg`; `cargo xtask cell drill kill-switch compose` (and
  with `--seeded`) in the local cell.
