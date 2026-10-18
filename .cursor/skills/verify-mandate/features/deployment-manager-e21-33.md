# The deployment manager: desired processes from the control stream, journaled starts and stops, crash loops left down (E21-33, slice 1)

- **Spec:** [infrastructure design](../../../../docs/design/infrastructure.md) §3.4 (restart
  policy, crash-loop bound, at most one writer), §3.6 (the kill-switch path), §8.2 (the crash-loop
  alert); [journal spec](../../../../docs/specs/journal.md) §5.2 (write before acting), §9.21 (the
  manager's three records, rules 165 to 168); DEC-1170.
- **Code:** `mandate-deploy-manager` (`crates/mandate-deploy-manager`). `src/control.rs` (`fact`: the control stream's
  deployments, stops and kill switches, and the manager's own records, through a `Resolver` for
  accounts), `src/manager.rs` (`Manager`: the fold, `desired`, `covers`, and `decide`, one step per
  slot; `backoff`), `src/record.rs` (`Record`: the §9.21 drafts with deterministic event ids),
  `src/driver.rs` (`Deployer::tick`: journal first, then the effect), `src/orchestrator.rs` (the
  port, `Phase`, the `crash_loop` alert), `src/fake.rs` (the fake orchestrator), and
  `src/kubernetes.rs` (bare pods with `restartPolicy: Never` over plain HTTPS, D-3). The records are
  registered in `crates/mandate-journal/src/deployment.rs`.
- **Tests:** `crates/mandate-journal/tests/deployment_manager_records.rs` (the vectors' section
  through `append` and `Draft::parse`); `tests/manager.rs` (deploy, back-off, the crash-loop
  bound and its one alert, a kill switch's immediate start and its one start of a crash-looped
  process, stops, lost nodes, retries, replay, journal and orchestrator outages);
  `tests/properties.rs` (random deploys, stops, kill switches, crashes, readiness flaps, lost
  nodes and clock jumps: one live instance per slot, every effect after its record, nothing
  started that is not wanted, nothing wanted stopped, nothing a kill switch covers left down, and
  the crash-loop bound; each check shown to catch its seeded bug); `tests/kubernetes.rs` (the
  adapter against a loopback API server).
- **Run:** `cargo nextest run -p mandate-deploy-manager`, and
  `cargo nextest run -p mandate-journal --test deployment_manager_records`.
