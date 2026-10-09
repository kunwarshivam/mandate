# Mandate documentation

Mandate is built by the founder working with AI coding agents. Agents follow
[AGENTS.md](../AGENTS.md).
People with write access start with [CONTRIBUTING.md](../CONTRIBUTING.md).

## Architecture

- [High-Level Design](HLD.md): architecture, agent runtime, deployment modes, key flows,
  audit, multi-tenancy, intelligence layer, billing, technology.

## Architecture decisions

| Document | Purpose |
|---|---|
| [ADR-0001: Engineering setup](adr/0001-engineering-setup.md) | Repository, crates, toolchain, numeric and time types, tests, CI, merge policy, supply chain |
| [ADR-0002: Autonomous ideation and retail](adr/0002-autonomous-ideation-and-retail.md) | The mandate as a risk envelope, the research agent and dynamic universe, retail from the start, Robinhood Agentic Trading, the compliance working assumption |
| [ADR-0003: Earned autonomy](adr/0003-earned-autonomy.md) | Goal-first drafting, delegations (bounded, expiring, owner-picked autonomy), the autonomy dial, the desk surfaces, and Mandate as the money layer for Dots, Muse, and Grok Bot |

## Specs

| Document | Purpose |
|---|---|
| [Trading domain spec](specs/trading-domain.md) | Exact rules for instruments, orders, fills, fees, accounting, settlement, corporate actions, and US account rules |
| [Trading domain reference cases](specs/reference-cases/trading-domain.yaml) | Machine-readable worked examples that implementations must reproduce |
| [Mandate spec](specs/mandate.md) | Mandate structure, validation, policy hierarchy, risk state and limits, autonomy rules, signal models and the order builder, versioning, records |
| [Mandate JSON Schema](../schemas/mandate.schema.json) | Structural rules for mandate documents (JSON Schema 2020-12) |
| [Policy JSON Schema](../schemas/policy.schema.json) | Structural rules for platform, organization, and workspace policy documents (JSON Schema 2020-12) |
| [Mandate reference implementation](../reference/mandate/ref.py) | Generator, invariant fuzz, and case checks for the mandate spec |
| [Mandate reference cases](specs/reference-cases/mandate.yaml) | Computed cases for validation, policy, risk state, gate limits, the order builder, autonomy, flatten, goals, change classification, admission, approvals, delegations, the review date, and tripwires |
| [Journal spec](specs/journal.md) | Streams, event envelope, canonical serialization, hash chain, storage, replay, verification, export |
| [Journal test vectors](specs/reference-cases/journal.yaml) | Exact canonical bytes and hashes, plus tamper cases |
| [Data plane spec](specs/data-plane.md) (draft v0.1) | Live market data, reference data, news, filings, fundamentals, the point-in-time store, fan-out to workspaces, failure walk, and adversaries |
| [Inference and model gateway spec](specs/inference.md) | Draft v0.1: every model call, the model gateway, pinning and no substitution, deadlines, caching, the registry, metering and spend caps, security, failure walk, and the fast-tier and provider decisions ([DEC-432](project/decisions/DEC-432.md)) |
| [Notifications and approval channels spec](specs/notifications.md) | Draft v0.3: every outbound notice and channel, the opaque payload, the dispatcher and relay, acting from a notification inside the workspace, failure walk, and adversaries ([DEC-438](project/decisions/DEC-438.md)) |
| [Identity, tenancy, and authentication spec](specs/identity.md) | Draft v0.1: principals, organizations, workspaces, memberships and the V-047 user count, roles and the permission matrix, passkeys and OIDC, sessions, action-bound step-up, separation of duties, tenant isolation at every layer, recovery and break-glass, hybrid identity providers, and adversaries ([DEC-437](project/decisions/DEC-437.md)) |
| [Agent harness spec](specs/agent-harness.md) | Draft v0.1: how a confirmed mandate version becomes a running agent process; construction and lifecycle, the trading and research loops, retrieval, model inputs and output checks, budgets, evaluation and change control, adversaries, and the founder's budget, evaluation, and licence decisions ([DEC-431](project/decisions/DEC-431.md)) |
| [Broker connections spec](specs/connections.md) | Draft v0.1: the connection object (references, never secrets), connector capabilities, Alpaca keys and OAuth, Robinhood Agentic Trading over MCP, permission and health checks, lifecycle walk, and adversaries ([DEC-441](project/decisions/DEC-441.md)) |
| [Workspace services API spec](specs/workspace-api.md) | Draft v0.1: the contract between the backend and the web app, the CLI, and owner-connected agents: owner input journaled on the control stream, roles and client scopes, idempotency, errors, read models, exports, failure walk, and adversaries ([DEC-436](project/decisions/DEC-436.md)) |

## Design

| Document | Purpose |
|---|---|
| [Infrastructure and operations](design/infrastructure.md) | Draft v0.2: environments, invariants, runtime topology, data stores, secrets and the vault, backups and disaster recovery, deploy and upgrade, observability and on-call, security baseline, cost model, failure walk (DEC-434) |
| [Billing](design/billing.md) | Draft v0.1: invariants, plans and entitlements, quota enforcement before spend, the metering pipeline from journaled counts to invoices, the provider interface, the non-payment ladder that never touches exits, adversaries (DEC-442) |
| [Global control plane](design/control-plane.md) | Draft v0.1: what the thin global plane holds and never holds, invariants CP-1 to CP-12, directory, licenses, fleet, relay, metering, distribution, anchor witness, the closed message set, the unavailability walk, adversaries (DEC-440) |

## Security

| Document | Purpose |
|---|---|
| [Threat model](security/threat-model.md) | Draft v0.1: assets, trust boundaries and data flows per deployment mode, attackers, threats and controls per boundary (prompt injection, credentials, tenant isolation, journal tampering, the agent-driven development process, supply chain), and residual risks ranked ([DEC-439](project/decisions/DEC-439.md)) |

## Product

| Document | Purpose |
|---|---|
| [Vision and strategy](product/01-vision-and-strategy.md) | Why Mandate exists, who it is for, principles, positioning, strategy |
| [Personas and journeys](product/02-personas-and-journeys.md) | Target users, roles, key journeys |
| [Competitive landscape](product/03-competitive-landscape.md) | Sourced, dated landscape: broker agents, direct competitors, platforms, channels, capability matrix, claims not to make |
| [PRD: v1](product/04-prd-v1.md) | Requirements for the design-partner release |
| [Roadmap](product/05-roadmap.md) | Phases, exit criteria, what is not planned |
| [Metrics](product/06-metrics.md) | North star, input and guardrail metrics, instrumentation |
| [Pricing and packaging](product/07-pricing-and-packaging.md) | Plans, price levers, questions to validate |
| [Compliance and regulatory](product/08-compliance-and-regulatory.md) | Regulatory posture and derived requirements (not legal advice) |
| [Product experience](product/09-product-experience.md) | Web v1 experience brief: principles, journeys, screen inventory and states, UX rules from the safety rules, open product decisions |
| [Strategy options](product/10-strategy-options.md) | Market and regulatory evidence, strategy options ranked, the recommendation as decided in DEC-141 (the complete product leads; the owner's own agent is an optional channel), broker, demo, and discovery plans |
| [Harness engineering](product/11-harness-engineering.md) | Research note: what harness engineering means, the open-source landscape, what trading agents and MCP gateways do and do not do, what enterprises appear to require, and the recommendations behind DEC-149's enterprise harness |
| [Cost model](product/12-cost-model.md) (draft v0.1) | Inference, data, compute, storage, and fixed costs per agent and per workspace, as formulas with labelled assumptions; three illustrative scenarios, sensitivity, price floors per plan, and the budget guardrails (DEC-443, Proposed) |
| [Glossary](product/glossary.md) | Shared vocabulary |

## Project

| Document | Purpose |
|---|---|
| [Project charter](project/01-project-charter.md) | Objectives, scope, deliverables, stakeholders, governance |
| [Milestones and WBS](project/02-milestones-and-wbs.md) | Milestone map, work packages, critical path, phase gates |
| [RAID log](project/03-raid-log.md) | Risks, assumptions, issues, dependencies |
| [Decision log](project/04-decision-log.md) | Decisions DEC-01 to DEC-302, and the registry of other reserved identifiers; it takes no new decision rows (DEC-344) |
| [Decisions](project/decisions/README.md) | One file per decision, DEC-303 onward (DEC-430 is the highest on `main` today), and how to write one |
| [Roles and RACI](project/05-raci.md) | Roles to fill and responsibilities |
| [Backlog: v1](project/06-backlog-v1.md) | Epics and user stories with acceptance criteria, and the known follow-ups |
| [Quality and release plan](project/07-quality-and-release.md) | Definitions of ready and done, test strategy, release gates, incidents |
| [Work tracker](project/08-work-tracker.md) | Where each milestone and story stands, reference-case counts, what waits on the founder, and what comes next |
| [Design questions before the mandate spec rewrite](project/09-mandate-rewrite-questions.md) | Choices put to the founder before the rewrite for DEC-97 to DEC-103, with options and recommendations; historical, since the rewrite merged as mandate spec v0.6 |
| [Task briefs](project/tasks/) | Agent task briefs for stories and streams, each written before its code |
| [Task template](project/templates/task.md) | The template every task brief starts from |

## Engineering

| Document | Purpose |
|---|---|
| [Dependency registry](dependencies.md) | Every direct Rust, Python and npm dependency, with why it is allowed |
| [ADR template](adr/template.md) | The template for an architecture decision record |
| [Web app](../web/README.md) | The `web/` app (DEC-200): how to run it, with its product, design and colour notes beside it |
