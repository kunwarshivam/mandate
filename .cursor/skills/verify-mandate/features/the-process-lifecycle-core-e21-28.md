# The process lifecycle core: states, readiness, the drain and crash-loop bounds, and probes (E21-28)

- **Spec:** [infrastructure design](../../../../docs/design/infrastructure.md) §3.4 (liveness,
  readiness, restart, the crash-loop bound), §7.3 (drain and its bound); [journal
  spec](../../../../docs/specs/journal.md) §5.1 (writer epochs, `Fenced`); DEC-994.
- **Code:** `mandate-process` (`crates/mandate-process`). `src/lifecycle.rs` (`Lifecycle`: the fold of `Input`s at
  injected times into `State`; `Kind`, `Action`, `Exit`, `Snapshot`, `DRAIN_BOUND`, `crash_loop`)
  and `src/probe.rs` (`answer`, `Status`, and the Unix socket's `serve` and `query` for `exec`
  probes). `mandate-agentd` takes its `Stop` and its health words from it.
- **Tests:** `tests/oracle.rs`: a property over random input sequences, clocks that step back and
  drains that cross their bound to the nanosecond, comparing after every input the state, every
  action's permission (the kill switch in every state the process is up) and liveness with an
  oracle that recomputes them from the whole history; a fixed sweep that reaches every transition
  and none backwards; and seven seeded bugs the oracle catches (an opening while not ready, the
  kill switch after exit, the kill switch or risk exits gated on readiness, a fenced append
  ignored while draining, a drain never abandoned, a reconciliation from before the epoch). `tests/lifecycle.rs`: each transition, gate, permission, the drain
  bound, liveness, exit codes, and the crash-loop bound's edges. `tests/probe.rs`: the answers,
  the socket's replies, a stale socket, a silent, long or trickling client, the connection
  bound, and the socket's mode.
- **Run:** `cargo nextest run -p mandate-process -p mandate-agentd`.
