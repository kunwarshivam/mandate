# RAID Log: Risks, Assumptions, Issues, Dependencies

| | |
|---|---|
| **Owner** | Project management |
| **Status** | Living document, reviewed weekly |

Scales: likelihood and impact are **H**igh, **M**edium, **L**ow.

## Risks

| ID | Risk | L | I | Mitigation | Owner |
|---|---|---|---|---|---|
| R-01 | Regulators treat agents trading user accounts on platform-originated ideas as investment advice or discretion | H | H | Working assumption that Mandate may be an investment adviser ([DEC-98](04-decision-log.md#decisions), [ADR-0002](../adr/0002-autonomous-ideation-and-retail.md)); counsel engaged during Phase 0 ([DEC-102](04-decision-log.md#decisions); compliance questions 31 to 35); no live trading until counsel signs off; no custody or per-trade pricing; every thesis journaled with evidence and authorship | Founder |
| R-02 | An agent causes significant user losses through a bug | M | H | Independent risk gate; drawdown ladder; fault-injection and fuzz testing; paper run required before live; broker-side OCO/bracket protective exits | Founder |
| R-03 | Duplicate or orphaned orders after crashes | M | H | Write-ahead intents with idempotency keys; reconciliation on restart; fault-injection gate | Founder |
| R-04 | Credential compromise | L | H | Trading-only OAuth scopes and keys (no fund movement); vault per workspace deployment; per-workspace encryption keys; penetration test | Founder |
| R-05 | Prompt injection through news, filings, or social inputs drives the research agent to admit an instrument or trade | H | H | LLMs produce outputs only; deterministic order builder and risk gate; envelope limits and `max_instruments`; admissions through the eligibility floor and autonomy rules (default `ask`); a vetted source allowlist and corroboration by an independent source or market data before any admission ([DEC-101](04-decision-log.md#decisions), E17-7); input-drift detector as a Phase 1 Must (E17-5); prompt-injection fixtures across every input source in simulation fuzzing | Founder |
| R-25 | Robinhood Agentic Trading has no paper or test environment, restricts platforms acting for many customers, or changes its terms; its MCP scope reads every account of the customer, including account numbers | M | M | Alpaca stays the first connector; the connector is an adapter behind the executor; account numbers and balances read over MCP are personal data held by reference (journal spec §6.4) and never logged; ask Robinhood support for a test path before E7-6 ([OD-12](04-decision-log.md#open-decisions)) | Founder |
| R-26 | One research agent drives many accounts into the same instrument at once, so correlated flow and herding across accounts concentrate market impact | H | H | Each account stays bounded by its own limits and participation cap; an aggregate-flow monitor per deployment alerts the operator when research-agent exposure in an instrument passes a threshold in dollars or as a share of average daily dollar volume; an operator per-thesis halt stops matching admissions and openings in every workspace of the deployment; staggered execution per workspace ([DEC-100](04-decision-log.md#decisions), E17-6). Residual: until the operator halts, aggregate flow is bounded only by the sum of the per-account caps, and the monitor does not sum across deployments or managed cells | Founder |
| R-27 | Historical backtests of LLM theses are contaminated by training data, so thesis quality looks better than it is | H | M | Backtests test mechanics only; thesis quality is judged only on forward paper against pre-registered baselines, net of modeled costs; scorecards in Phase 1 and a forward-paper quality gate at the Phase 1 exit ([DEC-99](04-decision-log.md#decisions), E15-3, E17-8) | Founder |
| R-28 | A thesis revision loop compounds overfitting: each rewrite fits the evaluator instead of the market, and the rewrite hides the failure that would have shown it | H | H | Revisions scored only on forward paper (E17-8), a fresh scorecard per version, journaled lineage with a per-lineage cap, and no loop before the DEC-99 evaluation has run once ([DEC-111](04-decision-log.md#decisions), E17-9) | Founder |
| R-29 | A steered or compromised AI agent session merges a change that weakens a safety control or the checks themselves: all agent sessions share one GitHub identity, which is also the approver login; the checks do not protect changes to their own files; and the public repository lets anyone put text in front of agents ([threat model](../security/threat-model.md) §7.5) | M | H | CI, independent review on a different model, mutants on safety-critical diffs, spec guard, linear history, revert; the untrusted-author rule ([DEC-439](decisions/DEC-439.md) items 2, 3, 7); proposed: a separate coordinator identity, founder review of self-protecting paths, merge restricted to the merge path (DEC-439 items 9, 10, 12; E21-13 to E21-16) | Founder |
| R-30 | The model aggregator in every prompt path (DEC-432 item 14) sees every managed tenant's research prompts, including which instruments agents hold and consider, and is the only witness to which model answered | M | M | Prompts hold no secrets, account IDs, personal data, or other tenants' data (INF-8, HI-11); outputs treated as a compromised model's (HI-10); terms excluding training (DEC-432 item 15); proposed: per-environment capped keys, zero retention, canary probes, revisit direct APIs at M8 (DEC-439 items 6, 14; E21-17) | Founder |
| R-31 | An owner account taken over raises its own envelope and trades the account for the attacker's benefit, for example into a thin stock the attacker sells (no withdrawal is possible) | L | H | Passkeys; step-up for raising risk; origin-bound step-up only and out-of-band notices of risk-increasing events (DEC-439 items 4, 5); eligibility floor and participation caps; proposed: a cooling-off delay (DEC-439 item 15) | Founder |
| R-32 | A collaborator's or the founder's GitHub account is compromised: write access can merge a green PR by hand, and the founder's account is the approver identity | L | H | `COLLABORATION.md` rules; required checks; proposed: hardware-key two-factor, collaborators at Triage, merge restricted (DEC-439 items 10, 11; E21-16) | Founder |
| R-33 | A compromised dependency (crate, Python or npm package, CI action) runs in a build, CI, or the web app | L | H | Lockfiles, cargo-deny, the dependency registry, `allowed_external`, pinned actions, checksum-verified tools; deferred: cargo-vet and SBOM at M6, signing at M11 (E21-8); proposed: npm checks (E21-19) | Founder |
| R-06 | Broker or exchange API changes, outages, or rate limits | H | M | Connector abstraction; health checks; data-staleness halt; Kraken Derivatives US as second venue in Phase 3 | Founder |
| R-07 | Jev access or pricing changes (early access, new vendor) | M | M | Model gateway abstraction; the fast tier is proposed out of v1 ([DEC-432](decisions/DEC-432.md) item 13); if a fast model is lost, its outputs count as missing and the owner may confirm a new mandate version pinning another model (Laya self-hosted, or an LLM wrapper). The gateway never falls back to a different model at runtime ([DEC-67](04-decision-log.md#decisions), [inference spec](../specs/inference.md)) | Founder |
| R-08 | Users expect profits and churn when agents lose money | H | M | Clear positioning (control, not returns); baselines in reports; paper first; disclosures | Founder |
| R-09 | Escalations are too frequent (noise) or too rare (missed risk) | M | M | User-set autonomy rules; escalation-precision metric; per-workspace thresholds; approver feedback loop | Founder |
| R-10 | Founder review bandwidth: one human reviews all agent output, so review becomes the bottleneck and quality slips under load | H | H | Small, single-story changes; merges go through CI and an independent review agent on a different model ([DEC-79](04-decision-log.md#decisions)), with the founder reviewing after the fact ([RACI](05-raci.md#human-only-responsibilities)); safety-critical changes also need zero missed mutants; narrow v1 scope | Founder |
| R-11 | NautilusTrader LGPL obligations if embedded in on-prem distribution | M | M | Own minimal core for Phase 0–1; legal review before embedding | Founder |
| R-12 | Market-data redistribution licensing (equities) | M | M | Each user's market data comes through their own Alpaca account and data plan; no redistribution by Mandate; review before any shared data offering | Founder |
| R-13 | A broker or consumer competitor adds equivalent guardrails | M | M | Multi-venue; hybrid and on-prem; audit depth ([competitive](../product/03-competitive-landscape.md)) | Founder |
| R-14 | Market manipulation emerges from agent behavior (for example, spoofing-like patterns) | M | H | Gate-enforced conduct controls and daily surveillance report ([spec §9.6](../specs/trading-domain.md#96-market-conduct-controls-dec-31)); eligibility floor; acceptable-use policy; ability to halt agents | Founder |
| R-15 | Agent-written code is subtly wrong in safety-critical logic (accounting, risk gate, idempotency) while passing its own tests | M | H | Founder writes or verifies reference test cases first; property-based and fault-injection suites as the contract; independent review agent; differential tests against hand-calculated cases | Founder |
| R-16 | Architectural drift: different agent runs make inconsistent design choices | M | M | [`AGENTS.md`](../../AGENTS.md) rules; decision log as the source of truth; stories reference the HLD section they implement; review agents check conformance | Founder |
| R-17 | Secrets or live credentials exposed to agents or committed to the repo | L | H | Agents use paper and demo credentials and fixtures only; secret scanning in CI; live credentials exist only in the founder-controlled vault | Founder |
| R-18 | Limited trading-industry experience on the team | M | M | Research agents for venue and market-structure questions; recruit a fractional advisor from trading risk or compliance before real capital | Founder |
| R-19 | Agents break US account rules or trigger broker restrictions (margin, settlement, self-crossing, equity/order-ratio checks) | M | H | 1× gross exposure, no shorts, limit-only openings; account ledger; restriction checks before every order; both day-trading regimes supported (Alpaca: intraday margin); reference cases RC-08 to RC-17 | Founder |
| R-23 | Positions unprotected during exit sequences, partial bracket fills, extended hours, gaps, or for fractional shares (stops are regular-session only; crypto stop-limits can miss; crypto has no OCO) | M | M | Bracket tranche model; OCO for partial fills; re-placement before expiry; crypto stop-limit sequences; unprotected-window alerts; disclosure to owners; drawdown ladder and kill switch | Founder |
| R-24 | Live equity agents require paid consolidated (SIP) data on the user's Alpaca account, adding cost and onboarding friction | M | M | Paper runs on the free IEX profile; disclose the requirement early; consider crypto-first live use for users without SIP | Founder |
| R-20 | CFTC treats automated trading on perpetuals as commodity trading advice | M | H | Counsel review before the Kraken connector ships, under the same adviser working assumption as for securities ([DEC-98](04-decision-log.md#decisions)) | Founder |
| R-21 | Dependence on a single broker (Alpaca) for v1: outages, terms changes, or Alpaca's own AI tooling competing | M | M | Connector abstraction; Kraken second; OAuth app compliance; differentiate on mandates, escalation, audit, and deployment | Founder |
| R-22 | Paper trading is more optimistic than live (Alpaca paper ignores regulatory fees, dividends, borrow fees, size limits; random partial fills) | H | M | Paper-mode shadow ledger ([spec §10](../specs/trading-domain.md#10-paper-mode)); conservative backtest fills on SIP data; readiness report compares backtest, paper, and estimated live costs; small first live allocations | Founder |

## Assumptions

| ID | Assumption | How to validate |
|---|---|---|
| A-01 | Professionals and small funds will run agents live if mandates are enforceable and escalation is selective | Design-partner live conversion |
| A-02 | Alpaca paper trading (US stocks, ETFs, crypto) is the fastest path to real usage in the US | Paper-to-live conversion versus effort |
| A-03 | Users can express intent in plain language that compiles reliably into mandates | Mandate compile accuracy metric |
| A-04 | Hybrid deployment is a buying criterion for funds | Design-partner requests; conversion of hybrid prospects |
| A-05 | Fast decision models add value over quant-only signal models for event-driven decisions | Signal-model scorecards versus quant-only baseline in shadow mode |
| A-06 | Web push, email, and chat are sufficient approval channels for v1 | Approval response time; timeout rate |

## Issues

| ID | Issue | Status | Action |
|---|---|---|---|
| I-01 | GitHub access secret was saved as `GITHUB_TOKWN` instead of `GITHUB_TOKEN` | Open | Rename the secret in the Cursor dashboard |
| I-02 | First venue not chosen | **Closed** | Resolved by [DEC-23](04-decision-log.md#decisions): Alpaca first, Kraken Derivatives US second |

## Dependencies

| ID | Dependency | Needed by | Risk if late |
|---|---|---|---|
| D-01 | Alpaca paper account for the founder; Alpaca OAuth app registration for the platform | M6 (paper account), M8 (OAuth app) | Blocks Phase 1 / Phase 2 |
| D-02 | Alpaca historical market data API (free account; stock data on the free feed is limited to IEX, full consolidated data needs a subscription) | M1 | Low; alternative data vendors exist |
| D-03 | Laya weights (open) and/or Jev early-access key | Phase 3 (P1 in v1) | Fast signal model delayed |
| D-04 | LLM provider account | Mandate compiler (M8) | Blocks plain-language authoring |
| D-05 | Notification providers: web push, email, chat, SMS | M10 | Limits channels |
| D-06 | Billing provider (Stripe Billing, Orb, or Metronome) | M12 | Blocks paid plans |
| D-07 | External counsel | M13 | Blocks launch with real capital |
| D-08 | Durable execution engine (Restate) and NATS JetStream maturity for our use | M5–M8 | Fallback to Temporal / alternative messaging |
| D-09 | Kraken Derivatives US demo access covering US perpetual contracts ([OD-07](04-decision-log.md#open-decisions)) | Phase 3 | Kraken connector testing delayed |
| D-10 | Robinhood Agentic Trading: a paper or test path, rate limits, and platform-scale terms ([OD-12](04-decision-log.md#open-decisions)); access itself is self-serve | M8 (E7-6) | Retail equities limited to Alpaca |
| D-11 | Securities counsel on the adviser question (compliance questions 31 to 35) | Phase 0, now ([DEC-102](04-decision-log.md#decisions)) | No live trading for any user |
