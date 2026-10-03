# Design gap register

| | |
|---|---|
| **Owner** | The coordinating agent session; the founder reviews |
| **Status** | Living document. Opened 2026-10-03 |

The trading core has exact, tested specs: accounting, the mandate, the risk gate, the executor, and
the journal. Much of the rest of the system is described only at the level of the
[HLD](../HLD.md), or not at all. This register lists every part that still needs a design document
before it can be built safely. Each row says:

- what exists today;
- what document is needed;
- which milestone waits on it;
- which choices in it are the founder's under DEC-79.

**Using the register.** A gap is claimed like a story: open a claim issue, write the document as a
PR, and get an independent review. Then move its row to *Written*. A design document follows the
house method ([AGENTS.md](../../AGENTS.md), "Getting it right the first time"):

- invariants first;
- a lifecycle walk;
- an adversary section;
- decisions kept apart from defects, with founder choices left Proposed and the work proceeding on
  the most conservative option.

## Wave 1: the data plane (critical path to an agent trading paper on its own theses)

| # | Gap | Exists today | Document | Waits on it | Founder choices inside | State |
|---|---|---|---|---|---|---|
| 1 | **Agent construction and harness:** how a confirmed mandate becomes a running agent, the research loop's tools, prompts, context, budgets, and evaluation, and how model output is journaled for replay | HLD §5; the research note [11-harness-engineering](../product/11-harness-engineering.md); `mandate-runtime` and `mandate-research` (pure code, no model calls); the E17-0 spike | `docs/specs/agent-harness.md` (DEC-431) | E15-1, E17-2's live half, the Phase 1 exit | Per-agent spend caps | Drafting |
| 2 | **Inference and model gateway:** providers, pinning, deadlines, fallbacks, caching, metering, spend caps; what Laya and Jev are | HLD §9 (about 15 lines); DEC-67 (no model substitution); mandate spec §8.1 (output contract) | `docs/specs/inference.md` (DEC-432) | E15-1, E15-2, E17 | Providers and hosting; whether to build Laya and Jev; inference budgets | Drafting |
| 3 | **Data plane:** live market data streams, news, filings, vetted sources, the point-in-time store, and fan-out | HLD §4 "Shared data plane" (one paragraph); `mandate-marketdata` (historical download and inspect); the Alpaca latest-quote read | `docs/specs/data-plane.md` (DEC-433) | Perception in the runtime; E17-5, E17-7; the Phase 1 soak | News and filings vendors; licensing for data redistribution (HLD §12 item 3) | Drafting |
| 4 | **Infrastructure and operations:** environments, the one-process-per-agent topology, stores, the secrets vault, backups and disaster recovery, deploy and upgrade, observability, on-call, cost drivers | HLD §4, §5, §11; ADR-0001; the journal's Postgres hot store and verification | `docs/design/infrastructure.md` (DEC-434) | The Phase 1 soak (M7); M8, M11, M13 | Hosting and cloud; vault product; recovery targets; budgets | Drafting |

## Wave 2: the platform (Phase 2)

| # | Gap | Exists today | Document | Waits on it | Founder choices inside | State |
|---|---|---|---|---|---|---|
| 5 | **Workspace services API:** the contract between the web app and the backend, covering the mandate registry and compiler, deployments, approvals, audit queries, and connections | None. The web app (DEC-200) renders recorded fixtures | `docs/specs/workspace-api.md` | M8, M9, E10, E11, E12 | None expected | Open |
| 6 | **Identity, tenancy, and authentication:** passkeys and OIDC, organizations, workspaces and roles, step-up, separation of duties, tenant isolation | E9 backlog rows; HLD §8 | `docs/specs/identity.md` (DEC-437) | M8, E9 (safety-critical: authentication, step-up, roles, tenant isolation) | Identity provider | Drafted (#556) |
| 7 | **Notifications and approval channels:** opaque payloads, the relay, push, email and chat, escalation chains, quiet hours | E8 rows; AGENTS.md rule 6; HLD §6 C | `docs/specs/notifications.md` (DEC-438) | M7 (email and one chat channel), M10 | Which channels v1 ships (HLD §12 item 6); providers | Drafted (#558) |
| 8 | **Global control plane:** directory, licensing, fleet, relay, and behaviour when it is unavailable | HLD §4 (bullet points) | `docs/design/control-plane.md` | M8, M11, M12 | None expected | Open |
| 9 | **Broker connections beyond Alpaca paper:** OAuth with Alpaca (M8); Robinhood Agentic Trading over MCP (E7-6, second connector, [DEC-98](04-decision-log.md#decisions)); per-user key and permission checks | E7-6 backlog row; ADR-0002; the Alpaca connector | `docs/specs/connections.md` | M8; E7-6 | Robinhood beta terms (ADR-0002, "Monitor") | Open |
| 10 | **Threat model:** assets, trust boundaries, attackers, and mitigations across every plane, covering prompt injection (RAID R-05), credential theft, tenant breakout, a malicious insider, and supply chain | Mentioned in [07-quality-and-release](07-quality-and-release.md); RAID R-05 | `docs/security/threat-model.md` (DEC-439) | M13; informs 1 to 9 | The `main` ruleset's bypass, merge-path identities, collaborator access, pen test, aggregator controls (DEC-439 items 9 to 18) | Drafted (#557) |
| 11 | **Billing:** metering, plans, licenses, and the usage feed | HLD §10; [07-pricing-and-packaging](../product/07-pricing-and-packaging.md) | `docs/design/billing.md` | M12 | Billing provider; prices | Open |
| 12 | **Cost model:** inference, data, and hosting per agent and per workspace, against pricing | None | `docs/product/12-cost-model.md` | Pricing, the Phase 2 exit | Every number in it (DEC-79: spending) | Drafted (#566) |

## Founder decisions still open in the HLD

These are listed so they are not lost. Each becomes a decision file when the founder takes it.

| Item | Where | Notes |
|---|---|---|
| NautilusTrader licensing (LGPL-3.0): use its connectors, or write our own | HLD §12 item 2 | No Nautilus dependency today; our connectors are our own |
| Market data redistribution | HLD §12 item 3 | v1 avoids it: each user's data comes through their own Alpaca account |
| Custom code in v1: declarative only, or WebAssembly plug-ins | HLD §12 item 4 | Nothing built; the declarative mandate is the only path today |
| Approval channels in v1 | HLD §12 item 6 | Decides the shape of gap 7 |
| Mobile access to on-site approval services | HLD §12 item 7 | Hybrid and on-prem only |
| Counsel engagement, disclosures, and terms | [DEC-98](04-decision-log.md#decisions), [DEC-102](04-decision-log.md#decisions); [compliance](../product/08-compliance-and-regulatory.md) | Legal text is the founder's; no live trading until counsel signs off |

## Product documents that lag decisions

Found by the 2026-10-03 docs audit and left for the founder:

- The roadmap's Phase 4 is labelled "Equities", though equities are in v1.
- The PRD and roadmap do not yet mention delegations ([DEC-181](04-decision-log.md#decisions)), owner-connected agents ([DEC-141](04-decision-log.md#decisions)), or the Owlhead plugin ([DEC-183](04-decision-log.md#decisions)).
- [09-product-experience](../product/09-product-experience.md) D7 says no acknowledgment record exists. The journal spec now has `OwnerAcknowledged`.
- The compliance document's "Posture safeguards" section still opens by citing DEC-33.
