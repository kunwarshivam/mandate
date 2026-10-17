# The long-running agent process: lease, schedule, drain, and health (E21-2, slice 1)

- **Spec:** [infrastructure design](../../../../docs/design/infrastructure.md) §3.4 (liveness,
  readiness, at most one writer by the fence), §7.3 (drain and hand over), §11 (process crash,
  node loss); [journal spec](../../../../docs/specs/journal.md) §5.1 (writer epochs), §5.2 (write
  before acting, recovery); DEC-968.
- **Code:** `crates/mandate-agentd`. `src/lease.rs` (`Lease`: the writer epoch of each of the
  deployment's streams, taken once, handed back to every cycle while still current, `FENCED`
  once a sibling took it), `src/supervisor.rs` (`Supervisor`: paper only, lease, reconciliation
  before the first cycle, one cycle per period, drain; `Host`, `IdleHost`, `Status`, `Stop`),
  `src/health.rs` (loopback `/livez` and `/readyz` with fixed words), `src/memory.rs`
  (`MemoryStore` over `MemoryJournal`), and the `mandate-agentd` binary (`--fixture` only; SIGTERM
  drains).
- **Tests:** `tests/lease.rs`: a property over the real `MemoryJournal` that at most one of three
  instances holds the lease and it is the last to acquire; a property over random starts, ticks,
  crashes (after the journal record, after the send), drains, and deaths that nothing is sent
  twice, nothing is sent unjournaled, only the newest process cycles, and a final restart leaves
  every submission answered. Each oracle is shown to fail on a seeded bug (a lease that takes a
  newer epoch for each cycle; a tick that skips the lease check). `tests/supervisor.rs`: refusal
  of live and backtest, reconciliation first, schedule, fencing before and during a cycle, drain.
  `tests/health.rs`: every phase's answers and the wire bytes. `tests/process.rs`: the binary
  refuses without a wired host, and drains and exits 0 on SIGTERM.
- **Run:** `cargo nextest run -p mandate-agentd`.
