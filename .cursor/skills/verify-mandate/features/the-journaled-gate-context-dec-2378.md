# The journaled gate context (DEC-2378)

- **Spec:** DEC-2378 items 2 to 7 (the facts an opening reads, folded from journal rows alone with
  the risk clock as the only time, failing closed); journal spec §9.2 (`AgentDeployed`,
  `AgentStopped`, `AccountSnapshotRecorded`, `MandateVersionApplied`) and §9.30 (`MarkUpdated`
  version 2); mandate spec §5.2.
- **Code:** `mandate-gate-context` (`crates/mandate-gate-context`): `src/lib.rs` (`Journaled`,
  its `fold` over a `Row`, its `facts` for an opening, answering `Facts` or `Held`, and `held`, the
  agent's sub-ledger a flatten sells, and `working`, its orders no terminal state has ended).
  The cell's runtime reads it through `mandate-daemon`'s `src/production.rs`: `Facts`, the one
  fold the process feeds every control, account and clock row it reads, and `Flatten`, the
  flatten planner over it, and `src/production/plan.rs`: `Plan`, the order plan over the fold
  (DEC-2378 item 10), with the bracket from `mandate-num`'s `Price::equity_bracket`, and
  `src/production/gate.rs`: `Gate`, the gate's dry run over the fold with DEC-2378 item 11's four
  checks deferred and every opening still denied while any is (DEC-2388), configured by
  `src/deployment.rs`'s `rule_set`. `src/rule_set.rs`'s `gate_rule_set` is the one reading of the
  rule set's gate configuration, shared with `mandate-gate-inputs`.
- **Tests:** `crates/mandate-gate-context/tests/facts.rs`: one test a fail-closed rule (no clock,
  no mandate, another agent's deployment, a rejected or another stream's version, a stopped agent,
  no or stale snapshot, no, stale or insane mark, scheduler ticks ageing the facts while the
  account stream is quiet, a version-1 mark, a mark outside the counted
  sessions, a clock that goes back, and every unreadable row holding from then on);
  `crates/mandate-gate-context/tests/replay.rs`: random row sequences against an oracle that
  searches the rows from the newest back; `crates/mandate-gate-context/tests/held.rs`: the agent's
  own fills, each once, another agent's or another stream's not, a sold-out position dropped, and
  the working orders a terminal state or an abandonment ends; `crates/mandate-daemon/tests/production.rs`:
  with no opening facts a kill switch sells the whole sub-ledger and cancels only the agent's
  working orders, an owner exit only its instrument's, rows fed twice count once, an unreadable
  row holds openings and never the flatten; `crates/mandate-daemon/tests/runtime.rs`'s
  `the_production_fold_is_fed_the_named_account_and_the_clock_once_each`.
- `crates/mandate-daemon/tests/plan.rs`: on the demo mandate, a flat agent with every fact
  journaled is proposed an opening never above the bid and asked about it; no opening outside the
  regular session, in its close window from 15:50, while held or working, or with any fact
  missing or a mark not sane.
- `crates/mandate-daemon/tests/gate.rs`: the plan's own opening passes the gate's checks, is
  reported deferred on each of the four checks and the ten unseen parts (DEC-2388 item 8), with no check reported passed, and is
  denied `deferred_to_binding`; each journaled unseen fact that should refuse is named deferred; the gate's own
  reason first (order and position caps, the collar, a blocked account, the 1x buying power); no
  rule set, a missing fact, or a working order denies; an exit is never denied; a property test
  against the checks computed independently. `crates/mandate-daemon/tests/deployment.rs`: the
  latest registered rule set of the environment, never an older one past an unreadable latest.
  `crates/mandate-gate-context/tests/rule_set.rs`: the rule set's identity and exact members.
- **Run:** `cargo nextest run -p mandate-gate-context -p mandate-daemon`.
