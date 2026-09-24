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
    ([trading domain spec §9](docs/specs/trading-domain.md#9-risk-gate-account-rules)).
11. **Mandate never originates a trade idea.** Every order traces to a user-confirmed mandate
    version; platform defaults may only restrict trading; inferred mandate fields stay inactive
    until confirmed.
12. **All account-level actions go through the account ledger.** Agents never call the broker
    directly; opening orders are limit orders; no short sales in v1.

## Safety-critical paths

Changes here require tests written or verified against founder-approved reference cases
before implementation, and always receive line-by-line founder review:

- Accounting (positions, cash, fees, corporate actions, settlement, funding, P&L)
- Risk gate, US account rules, eligibility, market-conduct controls, drawdown ladder, kill switches
- Account ledger, reservations, and protective-exit sequencing
- Autonomy policy (AUTO / ASK / DENY)
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
