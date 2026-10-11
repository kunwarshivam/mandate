# Simulated Robinhood broker (E7-25)

- **Spec:** the Robinhood tool contract (E7-15: "The equity order tools", "Order states") and
  slice S1 of the first live trade brief, both from PR #706; connections spec §6; DEC-124;
  DEC-441 items 11 and 16.
- **Code:** `mandate-rh-sim`, `crates/mandate-rh-sim/` (layer 10, above every product crate, so
  a dev-dependency only; safety-critical, pure):
  `src/lib.rs` (`Sim`, its scripted `Event`s, among them `RefuseChangedResend` and `EchoRefId`, and `Fault`s,
  the order and request types, `SimError`, the sell reservation: working sells hold shares,
  a partly filled one only its unfilled remainder, and C4's reads over the core (DEC-875 item
  6, DEC-902): the accounts as configured, the portfolio's scripted cash and buying
  power, the positions with the fills' average — weighted on a second buy, kept by a sell —
  the halt-driven tradability, and the locked quotes at the scripted prices), and
  `src/server.rs` (S2: `SimServer`, the core over loopback MCP, honest or as a hostile `Variant`,
  serving the pinned contract `contract/tools.json`, the four order tools and the five account
  reads; DEC-849).
- **Tests:** `crates/mandate-rh-sim/tests/rules.rs` (quantity forms, sessions and text against
  the contract; only an agentic account reviews or places; each pre-trade alert refuses) and
  `crates/mandate-rh-sim/tests/lifecycle.rs` (fills and positions, `ref_id` after a lost answer
  and its echo and changed-resend switches, `gfd` and `gtc`, sessions, scripted answers, and the
  refusals of cancel, fill and a sell that working sells already hold, a working sell holding
  only its unfilled remainder), and
  `crates/mandate-rh-sim/tests/properties.rs` (over random scripts: a `ref_id` never yields a
  second order; a terminal order never changes and is refused; a fill never exceeds the quantity;
  every state change is a legal transition, and every legal one is accepted; each against the
  test's own oracle), and `crates/mandate-rh-sim/tests/server.rs` (S2: loopback only on a port the system chooses,
  the session and revision every later request needs, the listing as the pinned contract and its
  hash, a tool call driving the core as the core alone would, review, cancel and reads, unlisted
  tools, the extra-tool and injection variants, and the lost-answer and garbled-answer
  faults with the records list-and-match reads; pending), and
  `crates/mandate-rh-sim/tests/preflight.rs` (C4, DEC-875 item 6 and DEC-902: the connector's
  whole preflight through the loopback wire behind the fixture's one agentic account, the
  halted symbol's `NotTradable` before the quote, the served reads refusing what the core does
  not hold, and the core's own cash, buying power, position averages — weighted, kept by a
  sell, and rounded half-to-even onto the price's grid — tradability, and the served quotes),
  with fixtures in
  `tests/common/mod.rs`, the wire client in `tests/common/wire.rs`, and its live guard against
  `mandate-mcp`'s handshake in `crates/mandate-rh-sim/tests/wire.rs`.
- **Run:** `cargo nextest run -p mandate-rh-sim`.
