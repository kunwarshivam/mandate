# The live preflight's read budget (E7-26, DEC-959)

- **Spec:** `docs/project/decisions/DEC-959.md` (the R0 rehearsal's R11); connections spec §6.5
  (the conservative ordinary bucket and the reserved exit budget); DEC-945 item 2 (an age
  counted from the clock read before the call); DEC-947 items 1 and 2 (the lapse's snapshot).
- **Code:** `crates/mandate-mcp/src/budget.rs` — `CallClass::Read` and
  `RateBudget::read_token_in`; `crates/mandate-mcp/src/transport.rs` — `TransportConfig::read_wait`,
  `Monotonic::pause` and the transport's `admit` (only a read waits, within the bound);
  `crates/mandate-robinhood/src/scope.rs` and `preflight.rs` — every broker read a `Read`, and
  `preflight_facts_on`; `crates/mandate-shell/src/robinhood.rs` —
  `RobinhoodBridge::preflight_facts_on`; `crates/mandate-paper/src/snapshot.rs` —
  `LapseSnapshot::fresh`; `crates/mandate-paper/src/robinhood.rs` — `robinhood_reads`;
  `crates/mandate-paper/src/lib.rs` — `live_start` keeps the snapshot and `run` hands it to the
  preflight.
- **Tests:** `crates/mandate-mcp/src/tests/wait.rs` (the rehearsal's exact call sequence fits the
  conservative budget; an exhausted budget waits for a token due within the bound and refuses at
  once one due past it, sending nothing; openings and exits never wait);
  `crates/mandate-mcp/src/tests/budget.rs` (a read's next token, exact, and a property over random
  openings, reads and exits that asking changes no admission); `crates/mandate-robinhood/tests/preflight.rs`
  and `scope.rs` (every read a `Read`; the preflight on a snapshot reads no account figure
  again); `crates/mandate-paper/src/live_tests.rs` (the live start keeps the lapse's snapshot; a
  fresh one is reused and a stale one, one across a clock that ran back, or none is read again).
- **Run:** `cargo nextest run -p mandate-mcp -p mandate-robinhood -p mandate-paper -p mandate-rh-sim -p mandate-shell-rh-sim`.
