# Mandate documentation

Mandate is built by the founder working with AI coding agents. Agents follow
[AGENTS.md](../AGENTS.md).

## Architecture

- [High-Level Design](HLD.md): architecture, agent runtime, deployment modes, key flows,
  audit, multi-tenancy, intelligence layer, billing, technology.

## Architecture decisions

| Document | Purpose |
|---|---|
| [ADR-0001: Engineering setup](adr/0001-engineering-setup.md) | Repository, crates, toolchain, numeric and time types, tests, CI, merge policy, supply chain |
| [ADR-0002: Autonomous ideation and retail](adr/0002-autonomous-ideation-and-retail.md) | The mandate as a risk envelope, the research agent and dynamic universe, retail from the start, Robinhood Agentic Trading, the compliance working assumption |

## Specs

| Document | Purpose |
|---|---|
| [Trading domain spec](specs/trading-domain.md) | Exact rules for instruments, orders, fills, fees, accounting, settlement, corporate actions, and US account rules |
| [Trading domain reference cases](specs/reference-cases/trading-domain.yaml) | Machine-readable worked examples that implementations must reproduce |
| [Mandate spec](specs/mandate.md) | Mandate structure, validation, policy hierarchy, risk state and limits, autonomy rules, signal models and the order builder, versioning, records |
| [Mandate JSON Schema](../schemas/mandate.schema.json) | Structural rules for mandate documents (JSON Schema 2020-12) |
| [Mandate reference implementation](../reference/mandate/ref.py) | Generator, invariant fuzz, and case checks for the mandate spec |
| [Mandate reference cases](specs/reference-cases/mandate.yaml) | Computed cases for validation, policy, risk state, gate limits, the order builder, autonomy, flatten, goals, and change classification |
| [Journal spec](specs/journal.md) | Streams, event envelope, canonical serialization, hash chain, storage, replay, verification, export |
| [Journal test vectors](specs/reference-cases/journal.yaml) | Exact canonical bytes and hashes, plus tamper cases |

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
| [Strategy options](product/10-strategy-options.md) | Market and regulatory evidence, strategy options ranked, recommended wedge, broker, demo, and discovery plans |
| [Glossary](product/glossary.md) | Shared vocabulary |

## Project

| Document | Purpose |
|---|---|
| [Project charter](project/01-project-charter.md) | Objectives, scope, deliverables, stakeholders, governance |
| [Milestones and WBS](project/02-milestones-and-wbs.md) | Milestone map, work packages, critical path, phase gates |
| [RAID log](project/03-raid-log.md) | Risks, assumptions, issues, dependencies |
| [Decision log](project/04-decision-log.md) | Accepted, proposed, and open decisions |
| [Roles and RACI](project/05-raci.md) | Roles to fill and responsibilities |
| [Backlog: v1](project/06-backlog-v1.md) | Epics and user stories with acceptance criteria |
| [Quality and release plan](project/07-quality-and-release.md) | Definitions of ready and done, test strategy, release gates, incidents |
| [Design questions before the mandate spec rewrite](project/09-mandate-rewrite-questions.md) | Choices the founder makes before the rewrite for DEC-97 to DEC-103, with options and recommendations |
