# RAID Log: Risks, Assumptions, Issues, Dependencies

| | |
|---|---|
| **Owner** | Project management |
| **Status** | Living document, reviewed weekly |

Scales: likelihood and impact are **H**igh, **M**edium, **L**ow.

## Risks

| ID | Risk | L | I | Mitigation | Owner |
|---|---|---|---|---|---|
| R-01 | Regulators treat agents trading user accounts as investment advice or discretion | M | H | Software-only posture ([compliance](../product/08-compliance-and-regulatory.md)); no recommendations, custody, or per-trade pricing; start with professionals and businesses; counsel review before retail | Founder |
| R-02 | An agent causes significant user losses through a bug | M | H | Independent risk gate; drawdown ladder; fault-injection and fuzz testing; paper run required before live; venue-side protective stops | Founder |
| R-03 | Duplicate or orphaned orders after crashes | M | H | Write-ahead intents with idempotency keys; reconciliation on restart; fault-injection gate | Founder |
| R-04 | Credential compromise | L | H | Trade-only keys enforced; vault per workspace deployment; per-workspace encryption keys; penetration test | Founder |
| R-05 | Prompt injection through news or social inputs drives trades | M | H | LLMs produce opinions only; deterministic decider and risk gate; mandate limits; unusual-input escalation | Founder |
| R-06 | Exchange API changes, outages, or rate limits | H | M | Connector abstraction; health checks; data-staleness halt; second exchange in Phase 3 | Founder |
| R-07 | Jev access or pricing changes (early access, new vendor) | M | M | Model gateway abstraction; Laya self-hosted as fallback; LLM wrapper fallback | Founder |
| R-08 | Users expect profits and churn when agents lose money | H | M | Clear positioning (control, not returns); baselines in reports; paper first; disclosures | Founder |
| R-09 | Escalations are too frequent (noise) or too rare (missed risk) | M | M | Calibration; escalation-precision metric; per-workspace thresholds; approver feedback loop | Founder |
| R-10 | Founder review bandwidth: one human reviews all agent output, so review becomes the bottleneck and quality slips under load | H | H | Small, single-story changes; review agents pre-screen every change; CI gates block merges; founder review concentrated on safety-critical paths ([RACI](05-raci.md#human-only-responsibilities)); narrow v1 scope | Founder |
| R-11 | NautilusTrader LGPL obligations if embedded in on-prem distribution | M | M | Own minimal core for Phase 0–1; legal review before embedding | Founder |
| R-12 | Market-data redistribution licensing (equities) | M | M | Crypto first; customers bring their own equities data licenses; review before Phase 4 | Founder |
| R-13 | A broker or consumer competitor adds equivalent guardrails | M | M | Multi-venue; hybrid and on-prem; audit depth ([competitive](../product/03-competitive-landscape.md)) | Founder |
| R-14 | Market manipulation emerges from agent behavior (for example, spoofing-like patterns) | L | H | Order-pattern monitoring; acceptable-use policy; ability to halt agents | Founder |
| R-15 | Agent-written code is subtly wrong in safety-critical logic (accounting, risk gate, idempotency) while passing its own tests | M | H | Founder writes or verifies reference test cases first; property-based and fault-injection suites as the contract; independent review agent; differential tests against hand-calculated cases | Founder |
| R-16 | Architectural drift: different agent runs make inconsistent design choices | M | M | [`AGENTS.md`](../../AGENTS.md) rules; decision log as the source of truth; stories reference the HLD section they implement; review agents check conformance | Founder |
| R-17 | Secrets or live credentials exposed to agents or committed to the repo | L | H | Agents use testnet keys and fixtures only; secret scanning in CI; live keys exist only in the founder-controlled vault | Founder |
| R-18 | Limited trading-industry experience on the team | M | M | Research agents for venue and market-structure questions; recruit a fractional advisor from trading risk or compliance before real capital | Founder |

## Assumptions

| ID | Assumption | How to validate |
|---|---|---|
| A-01 | Professionals and small funds will run agents live if mandates are enforceable and escalation is selective | Design-partner live conversion |
| A-02 | Crypto perpetuals are the fastest path to real usage | Paper-to-live conversion versus effort |
| A-03 | Users can express intent in plain language that compiles reliably into mandates | Mandate compile accuracy metric |
| A-04 | Hybrid deployment is a buying criterion for funds | Design-partner requests; conversion of hybrid prospects |
| A-05 | Fast decision models add value over quant-only advisors for event-driven decisions | Advisor scorecards versus quant-only baseline in shadow mode |
| A-06 | Web push, email, and chat are sufficient approval channels for v1 | Approval response time; timeout rate |

## Issues

| ID | Issue | Status | Action |
|---|---|---|---|
| I-01 | GitHub access secret was saved as `GITHUB_TOKWN` instead of `GITHUB_TOKEN` | Open | Rename the secret in the Cursor dashboard |
| I-02 | First exchange not chosen; depends on founder and partner jurisdictions | Open | Decide before M6 (see [decision log](04-decision-log.md) OD-01) |

## Dependencies

| ID | Dependency | Needed by | Risk if late |
|---|---|---|---|
| D-01 | Exchange testnet access and API keys for the first exchange | M6 | Blocks Phase 1 |
| D-02 | Binance public historical data (no account needed) | M1 | Low; alternative sources exist |
| D-03 | Laya weights (open) and/or Jev early-access key | Phase 3 (P1 in v1) | Fast-model advisor delayed |
| D-04 | LLM provider account | Mandate compiler (M8) | Blocks plain-language authoring |
| D-05 | Notification providers: web push, email, chat, SMS | M10 | Limits channels |
| D-06 | Billing provider (Stripe Billing, Orb, or Metronome) | M12 | Blocks paid plans |
| D-07 | External counsel | M13 | Blocks launch with real capital |
| D-08 | Durable execution engine (Restate) and NATS JetStream maturity for our use | M5–M8 | Fallback to Temporal / alternative messaging |
