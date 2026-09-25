# AGENTS.md

Instructions for AI coding agents working in this repository. Humans: see
[docs/README.md](docs/README.md).

## What this project is

Mandate is a platform for deploying autonomous trading agents that trade on users' own
exchange and brokerage accounts, bound by an enforceable mandate, escalating to a human when
unsure, and recording every decision. It is built by the founder working with AI coding
agents. The founder approves every merge.

## Sources of truth

Read the relevant document before changing anything it covers. If code and docs disagree,
stop and ask; do not silently pick one.

| Question | Document |
|---|---|
| How is the system structured? | [docs/HLD.md](docs/HLD.md) |
| What must v1 do? | [docs/product/04-prd-v1.md](docs/product/04-prd-v1.md) |
| What should I work on? | [docs/project/06-backlog-v1.md](docs/project/06-backlog-v1.md), in milestone order from [docs/project/02-milestones-and-wbs.md](docs/project/02-milestones-and-wbs.md) |
| What has already been decided? | [docs/project/04-decision-log.md](docs/project/04-decision-log.md) |
| What are the exact trading rules (accounting, orders, fees, settlement, account rules)? | [docs/specs/trading-domain.md](docs/specs/trading-domain.md) and its [reference cases](docs/specs/reference-cases/trading-domain.yaml), which tests must reproduce exactly |
| What is a mandate, which invariants must hold, and how are limits, autonomy, and the order builder defined? | [docs/specs/mandate.md](docs/specs/mandate.md), the [mandate](schemas/mandate.schema.json) and [policy](schemas/policy.schema.json) schemas, and its [reference cases](docs/specs/reference-cases/mandate.yaml) |
| How are events journaled, hashed, stored, and replayed? | [docs/specs/journal.md](docs/specs/journal.md) and its [test vectors](docs/specs/reference-cases/journal.yaml) |
| What do terms mean? | [docs/product/glossary.md](docs/product/glossary.md) |
| How is work reviewed and released? | [docs/project/07-quality-and-release.md](docs/project/07-quality-and-release.md) |

## Non-negotiable rules

1. **The mandate is the contract.** No code path may let an agent act outside its mandate.
   Limits are enforced by the risk gate, independent of agent logic.
2. **Reducing risk never needs approval; increasing risk beyond limits always does.**
3. **Timeouts and ambiguity resolve to a safe default that never adds risk.**
4. **LLMs produce opinions, never orders.** Deterministic code sizes positions and places orders.
5. **Journal before acting.** Every order intent is written to the journal, with an idempotency
   key, before it is sent.
6. **No sensitive content in notifications.** Notification payloads carry only opaque IDs and
   generic text.
7. **Credentials never leave the vault**, never appear in logs, and are never committed.
8. **Never place real orders.** Use broker paper environments (Alpaca paper), venue demo
   environments, and local fixtures only. Never ask for, read, or use live credentials or
   production secrets.
9. **Accepted decisions are binding.** To deviate, stop and propose a new decision-log entry
   for the founder instead of implementing the deviation.
10. **Enforce US account rules.** Day-trading regime, settlement, market-hours, eligibility,
    market-conduct, and account-restriction rules are part of the risk gate, not optional checks
    ([trading domain spec §9](docs/specs/trading-domain.md#9-risk-gate)).
11. **Mandate does not choose instruments, strategy, sizing, or limits.** The user enters and
    confirms every judgment field; the compiler only extracts values the user stated and never
    proposes instruments, signal models, numbers, or `auto`; platform defaults exist only for the
    non-judgment fields listed in the mandate spec §7; templates set structure, never values; no
    calibration in v1.
12. **All account-level actions go through the account ledger.** Agents never call the broker
    directly; opening orders are limit orders in the regular session; no short sales in v1.
13. **Risk reduction is never denied by conduct controls, eligibility, day-trade budgets, buying
    power, or opening-session rules.** Risk exits, protective orders, and automated kill switches
    are exempt from all of them. Owner exits are paced only by participation caps. Discretionary
    exits (signal or goal driven) are paced by conduct controls and, for equities, wait for the
    regular session, but are never denied. Exits and protective orders
    may be held only by agent mode `paused` or `stopped`, by an `Unknown` order in the same
    instrument, or by the broker. Owner exits may sell equities outside the regular session once the
    owner confirms the displayed bid. The kill switch is always available, touches only its scope
    (an agent-scoped kill switch never uses cancel-all or close-position), and does not depend on
    model state.

## Safety-critical paths

Changes here require tests written or verified against founder-approved reference cases
before implementation, and always receive line-by-line founder review:

- Accounting (positions, cash, fees, corporate actions, settlement, funding, P&L)
- Risk gate, US account rules, eligibility, market-conduct controls, drawdown ladder, kill switches
- Account ledger, reservations, and protective-exit sequencing
- Autonomy policy (AUTO / ASK / DENY), mandate validation, and mandate change classification
- Executor, idempotency, reconciliation, crash recovery
- Broker and exchange connectors, OAuth scopes, and key-permission checks
- Credential handling and the vault
- Authentication, step-up authentication, roles, tenant isolation
- Notification payloads and the approval flow

## How to work

1. Pick one story from the backlog. One change implements one story.
2. In the change description, cite the story ID, the PRD requirement, and the HLD section.
3. Write tests first for safety-critical paths; include property-based tests for invariants.
4. Keep changes small enough to review in one sitting.
5. Update docs in the same change when behavior, interfaces, or decisions change.
6. Emit journal events for every new state change.
7. Run the full local check before proposing the change (see below).

## Getting it right the first time (specs and safety-critical design)

The mandate spec needed a rewrite after every review round (v0.1 to v0.3). The findings had seven
causes; each rule below closes one of them. Follow these before sending any spec, schema, or
safety-critical design for review.

| Cause, with an example from the mandate spec | Rule |
|---|---|
| **Local fixes, no global re-check.** v0.2 scaled the high-water mark on allocation changes but left contributed capital additive, so a withdrawal could trip the lifetime floor, contradicting the spec's own sentence two lines earlier | **Invariants first.** Write the properties the design must always hold (for example, "an allocation change never triggers or lifts a limit") before writing rules. After every change, re-check every invariant, not only the finding being fixed |
| **Claims stated, never tested.** "Can neither lift nor trigger a limit" was asserted, not checked | **Every claim is a test.** Each invariant and each "never" or "always" in the text has a property-based test in the reference implementation. Fuzz random sequences of events (marks, fills, allocation changes, acknowledgments, version changes, clock ticks) and assert the invariants |
| **Examples that break their own rules.** A base mandate's `target_qty` cost more than its allocation; a case titled "fraction cap binds" did not bind; v0.1's V-021 rejected the spec's own example | **Validate fixtures against the rules.** Every example and base fixture passes every rule it is not meant to fail; every case asserts the condition its title claims, and is recomputed from the rules, not typed |
| **Cross-spec contracts not traced.** The agent flatten reused the account-wide kill switch (cancel-all, close-position); units differed (basis points vs fraction); two specs gave different outcomes for the same limit | **Trace every reference.** For each rule that relies on another spec, read that section and confirm scope, units, and outcome match; update both sides in the same change |
| **Lifecycles not finished.** Nothing said what happens after a flatten, after a goal completes, after an acknowledgment, or at a time boundary | **Walk every state to its exit.** For each state and limit: how it is entered, what it blocks, how it ends, who can end it, and what happens at session close, midnight, restart, and version change |
| **No adversary.** Order splitting, redeploying to reset limits, deposits to lift rungs, one bad tick, and a model outage enlarging orders were all found by reviewers | **Attack it yourself.** Before review, list how a careless user, a bad model, a malicious insider, and a bad market tick could exceed intended risk or imply platform advice; each must be blocked or disclosed |
| **Decisions hidden inside drafts, and scope creep.** About a third of each round were design choices only the founder can make, and each round added features that became new surface | **Separate decisions from defects.** List open design choices for the founder before drafting. While fixing findings, add no features; propose them separately |

**Before external review:**

1. Run the invariant fuzz, schema and fixture validation, and a link check. For the mandate spec:
   `python3 reference/mandate/generate.py`, `python3 reference/mandate/check_cases.py`, and
   `python3 reference/mandate/fuzz.py <seed>` for several seeds (requirements in
   `reference/mandate/requirements.txt`).
2. Self-review against the three role checklists: engineer (determinism, ordering, time base,
   replay, every input listed), risk (loopholes, gap risk, lifecycle), and compliance (who chooses
   what, records, wording).

**Freeze rule:** after an external review, fix only blockers and majors. Minors go to the backlog.

## Conventions

These apply once code exists; the first stories (E1) set them up.

- **Rust** for the core: runtime, risk, execution, connectors, market data, journal.
  `cargo fmt`, `cargo clippy -- -D warnings`, and `cargo test` must pass.
- **Python** for research, model tooling, and the SDK. `ruff` and `pytest` must pass.
- No `unwrap()` or `expect()` in non-test Rust code on trading paths; return typed errors.
- Money and quantities use fixed-point decimal types, never floating point, in accounting and
  order paths.
- Timestamps are UTC with nanosecond precision.
- Imports at the top of files; no inline imports.
- Comments explain constraints the code cannot show, not what the next line does.
- Commits: imperative, descriptive subject lines; one logical change per commit.

## Do not

- Add dependencies without stating why in the change description.
- Disable, skip, or weaken tests to make a change pass.
- Introduce floating-point arithmetic into money or quantity calculations.
- Log order details, positions, or mandate content in anything that leaves the workspace
  deployment.
- Add platform-generated trade recommendations, per-trade pricing, or custody of funds (see
  [compliance](docs/product/08-compliance-and-regulatory.md)).
