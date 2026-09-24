# Project Charter: Mandate v1

| | |
|---|---|
| **Sponsor** | Founder (@kunwarshivam) |
| **Owner** | Project management |
| **Status** | Draft v0.1 |

## Purpose

Deliver Mandate v1, a platform where users deploy autonomous trading agents bound by
enforceable mandates, with selective human escalation and complete audit, to a first cohort of
design partners in managed and hybrid modes.

## Objectives

| # | Objective | Measure |
|---|---|---|
| O1 | Prove agents can trade autonomously and safely | Zero orders outside mandate and zero duplicate orders across fault-injection tests and design-partner use |
| O2 | Prove selective escalation works | 90%+ autonomy rate; 70%+ escalations judged warranted |
| O3 | Prove real adoption | 5+ design partners on paper; 3+ live |
| O4 | Prove deploy-anywhere | 1+ hybrid deployment in use |
| O5 | Establish a trustworthy record | Causal trace and export used in at least one real review |

## Scope

**In scope:** everything marked P0 in the [PRD](../product/04-prd-v1.md), delivered through
roadmap Phases 0–2 ([roadmap](../product/05-roadmap.md)).

**Out of scope:** see [PRD §3 Non-goals](../product/04-prd-v1.md#3-non-goals-v1).

## Deliverables

1. Core engine: market data, accounting, simulated execution, backtesting, journal.
2. Agent runtime with risk gate, drawdown ladder, reconciliation, and recovery.
3. Exchange connector (testnet and live) for the first exchange.
4. Escalation and private approval flow across the v1 channels.
5. Thin global control plane and workspace control services.
6. Web app: workspaces, connections, mandate authoring, backtest and paper, dashboard,
   approvals, audit explorer.
7. Hybrid installer and signed releases.
8. Billing integration.
9. Documentation, runbooks, terms and risk disclosures.

## Stakeholders

| Stakeholder | Interest |
|---|---|
| Founder | Product direction, funding, first user (trades own capital) |
| Design partners | Early access; influence on product; need safety and support |
| External counsel | Regulatory posture, terms, disclosures |
| Future team members | See [RACI](05-raci.md) for roles to fill |
| Exchanges / brokers | API terms, rate limits, testnet access |
| Model providers | Jev (TypeSafe), Laya (open weights), LLM providers |

## Constraints

- Small team at the start: the founder plus hires listed in the [RACI](05-raci.md).
- Rust core with Python for research and model tooling (see [decision log](04-decision-log.md)).
- No custody, no advice, no per-trade pricing ([compliance](../product/08-compliance-and-regulatory.md)).
- Exchange availability depends on jurisdiction.

## Assumptions

Tracked in the [RAID log](03-raid-log.md#assumptions).

## Key risks

Tracked in the [RAID log](03-raid-log.md#risks). Highest: regulatory exposure, agent-caused
losses, credential compromise, prompt injection, team capacity.

## Governance

- **Decisions:** recorded in the [decision log](04-decision-log.md) with status and rationale.
- **Change control:** scope changes to P0 requirements require a PRD update and a decision-log
  entry.
- **Phase gates:** each roadmap phase ends with its exit criteria reviewed and signed off by the
  founder ([milestones](02-milestones-and-wbs.md)).
- **Cadence:** weekly written status (progress against milestones, RAID changes, decisions
  needed); design-partner check-ins once partners are onboarded.

## Success criteria

v1 is successful when objectives O1–O5 are met and the [release criteria](07-quality-and-release.md#release-gates)
for Phase 2 pass.
