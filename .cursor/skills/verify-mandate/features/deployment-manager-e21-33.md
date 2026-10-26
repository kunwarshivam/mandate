# The deployment manager: desired processes from the control stream, journaled starts and stops, crash loops left down (E21-33, slices 1 and 2)

- **Spec:** [infrastructure design](../../../../docs/design/infrastructure.md) §3.4 (restart
  policy, crash-loop bound, at most one writer), §3.6 (the kill-switch path), §8.2 (the crash-loop
  alert); [journal spec](../../../../docs/specs/journal.md) §5.2 (write before acting), §9.21 (the
  manager's three records, rules 165 to 168), §9.24 (a deployment's account, from `AgentDeployed`
  version 2 and its connection's establishment); DEC-1170; DEC-1171; DEC-1172 (the process in the
  cell); DEC-1173 (a stop is final, `deployment_unread`).
- **Code:** `mandate-deploy-manager` (`crates/mandate-deploy-manager`). `src/control.rs` (`fact`: the control stream's
  deployments, the connections they run on, stops and kill switches, and the manager's own
  records, from the journal alone), `src/manager.rs` (`Manager`: the fold, `desired`, `covers`, and `decide`, one step per
  slot; `backoff`), `src/record.rs` (`Record`: the §9.21 drafts with deterministic event ids, and `drafts`, the batch
  in which an unapplied crash loop carries the owner's `OwnerAlertSent`, `kill_switch_unapplied`,
  E8-17),
  `src/driver.rs` (`Deployer::tick`: journal first, then the effect), `src/orchestrator.rs` (the
  port, `Phase`, the `crash_loop` and `deployment_unread` alerts), `src/fake.rs` (the fake orchestrator), and
  `src/kubernetes.rs` (bare pods with `restartPolicy: Never` over plain HTTPS, D-3). The records are
  registered in `crates/mandate-journal/src/deployment.rs`. The process is `mandate-deployd`
  (`crates/mandate-deployd`): `src/lib.rs` (`PgControl`, the control stream over the Postgres
  journal; `input`, a pass's outcome as a lifecycle input; `templates`, the chart's pod templates)
  and `src/main.rs` (the loop, probes, drain and the in-cluster adapter). Its chart wiring (the
  `deploy-manager` process, its ServiceAccount and Role, its `kube-api` egress and gate) is in
  `deploy/cell/charts/workspace`.
- **Tests:** `crates/mandate-journal/tests/deployment_manager_records.rs` (the vectors' section
  through `append` and `Draft::parse`); `tests/agent_deployed.rs` (the vectors' `agent_deployed`
  streams: the processes a first pass starts, and the unread-deployment alerts); `tests/manager.rs` (deploy, back-off, the crash-loop
  bound and its one alert, a kill switch's immediate start and its one start of a crash-looped
  process, stops, lost nodes, retries, replay, journal and orchestrator outages);
  `tests/retired.rs` (DEC-2344 items 1 and 3: an executor no agent wants is still reached by a
  covering switch; an unapplied loop's operator alert, and its owner's `OwnerAlertSent` once, in the
  loop's batch, never for an ordinary loop or again after a restart, in single cases and a property
  test with its own oracle); `tests/unapplied_vectors.rs` (the vectors' `kill_switch_unapplied`
  streams: the manager's own reading of each loop is rule 264's);
  `tests/properties.rs` (random deploys, stops, connection revocations, kill switches, crashes, readiness flaps, lost
  nodes and clock jumps: one live instance per slot, every effect after its record, nothing
  started that is not wanted, nothing wanted stopped, nothing a kill switch covers left down, and
  the crash-loop bound; each check shown to catch its seeded bug); `tests/kubernetes.rs` (the
  adapter against a loopback API server, with each pod's labels); `crates/mandate-deployd/tests/`
  (`process.rs`: the lifecycle input, templates and alerts; `pg.rs`: a pass on a real
  Postgres journal, skipped without `MANDATE_PG_URL`); `deploy/tests/test_cell.py`
  (`deploy_manager_contained` and its seeded bugs).
- **Run:** `cargo nextest run -p mandate-deploy-manager -p mandate-deployd`,
  `cargo nextest run -p mandate-journal --test deployment_manager_records`, and
  `python3 -m pytest deploy/tests -q`.
