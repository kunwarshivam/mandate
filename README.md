# Mandate

A platform for deploying autonomous trading agents on users' own brokerage and exchange
accounts. The owner sets the envelope: capital, goals, limits, allowed asset classes, how much
autonomy the agent has, and when it must ask. Inside that envelope the platform's research agent
brings the ideas: it reads market data, news, and filings, proposes theses, and admits instruments
into the working universe through the eligibility floor and the owner's autonomy rules
([ADR-0002](docs/adr/0002-autonomous-ideation-and-retail.md)). Deterministic code sizes positions
and places orders; an independent risk gate enforces the limits; every decision is journaled in a
hash-chained log. Language models produce opinions, never orders.

It is built for retail traders and small funds from the start: Alpaca paper trading first,
Robinhood Agentic Trading as the second connector. It runs fully managed, hybrid (a thin hosted
control plane with strategy, approvals, audit data, and credentials on the customer's side), or
fully on-prem. Nothing trades live with real money until securities counsel has signed off
([DEC-98](docs/project/04-decision-log.md), [DEC-102](docs/project/04-decision-log.md)).

## Status

Phase 0, the core engine, is in progress. The specs are approved and the crates below exist with
their tests; there is no agent runtime, broker connector, or user interface yet. The
[work tracker](docs/project/08-work-tracker.md) says where every story stands and what is next.

| Crate | What it does |
|---|---|
| `mandate-num` | Fixed-point money, price, and quantity types with exact-or-error arithmetic |
| `mandate-time` | UTC nanosecond timestamps, dates, the NYSE calendar and trading sessions |
| `mandate-canon` | Canonical JSON, the decimal grammar, and SHA-256 for the journal |
| `mandate-journal` | The hash-chained journal: drafts, the append protocol, verification, anchoring, and the content-addressed artifact core |
| `mandate-artifacts-fs` | Filesystem artifact store: write-once objects under their SHA-256, atomic publish, checked reads |
| `mandate-journal-pg` | The Postgres journal hot store (stubs and tests; implementation in review) |
| `mandate-accounting` | The account fold: positions, cost basis, cash and settlement, fees, marks, P&L |
| `mandate-marketdata` | Alpaca historical market data: exact vendor numbers, idempotent Parquet datasets, corporate actions, inspection |
| `mandate-sim` | The backtest fill model as a pure function (stubs and tests; implementation next) |
| `mandate-refcases` | One named test per reference case in `fixtures/refcases`, so the specs' worked examples are executable |
| `mandate-cli` | `mandate download` and `mandate inspect` |

`python/` holds the reference-case exporter (`mandate_tools`) and the research-agent spike
(`research_spike`: theses from news and prices through an LLM, sized under caps, paper orders,
a hash-chained journal, and a scorecard against SPY).

## Specs and reference cases

The exact rules live in three specs, each with machine-readable cases that the code must reproduce:
the [trading domain](docs/specs/trading-domain.md) (instruments, orders, fills, fees, accounting,
settlement, corporate actions, US account rules), the [mandate](docs/specs/mandate.md) (structure,
validation, limits, autonomy, the order builder), and the [journal](docs/specs/journal.md)
(canonical bytes, hash chain, storage, verification). Spec paths are protected: they change only
through a decision-log entry, in a PR that ships no code.

## Building and checking

The toolchain is pinned in `rust-toolchain.toml` (Rust 1.98.1). One command runs every CI job
locally:

```bash
cargo xtask check
```

It runs lint (fmt, clippy, layering, markers, typos, ruff), the tests, the reference cases, the
Python reference implementation, supply-chain checks (cargo-deny, gitleaks), the spec guard, the
pending-tests gate, and a mutation gate on changed safety-critical code. On Linux,
`.cursor/install.sh` installs the tools it needs (uv, Python 3.14, cargo-nextest, typos,
cargo-deny, cargo-mutants, gitleaks, and PostgreSQL where reachable). The Postgres journal tests
run when `MANDATE_PG_URL` points at a database and skip otherwise. `mandate download` needs Alpaca
market-data keys; nothing in this repository places orders.

## How the work runs

The founder builds with AI coding agents (Claude Code and Cursor cloud agents) under
[AGENTS.md](AGENTS.md). Safety-critical stories ship as a tests PR with pending markers, then an
implementation PR that may only delete those markers, then a status PR (DEC-77). Every PR merges
only after green CI and a pass from an independent review agent on a different model (DEC-79); the
founder reviews after the fact and can revert. Several agent sessions coordinate through claim
issues, a reserved-identifiers table, and one merge queue
([coordination playbook](.cursor/skills/mandate-mode/playbooks/coordination.md)).

## Documents

- [Documentation index](docs/README.md)
- [AGENTS.md](AGENTS.md): rules for AI coding agents working in this repository
- [High-Level Design](docs/HLD.md)
- Architecture decisions: [ADR-0001 engineering setup](docs/adr/0001-engineering-setup.md),
  [ADR-0002 autonomous ideation and retail](docs/adr/0002-autonomous-ideation-and-retail.md)
- Product: [vision](docs/product/01-vision-and-strategy.md), [PRD v1](docs/product/04-prd-v1.md),
  [roadmap](docs/product/05-roadmap.md),
  [compliance](docs/product/08-compliance-and-regulatory.md)
- Project: [charter](docs/project/01-project-charter.md),
  [milestones](docs/project/02-milestones-and-wbs.md), [RAID log](docs/project/03-raid-log.md),
  [decision log](docs/project/04-decision-log.md), [backlog](docs/project/06-backlog-v1.md),
  [work tracker](docs/project/08-work-tracker.md)

Mandate is software under development, not investment advice, and it holds no customer funds.
