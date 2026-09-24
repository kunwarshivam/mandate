# Decision Log

| | |
|---|---|
| **Owner** | Project management |
| **Status** | Living document |

Status values: **Accepted**, **Proposed** (recommended, awaiting confirmation), **Open**
(not yet decided). Changing an accepted decision requires a new entry that supersedes it.

## Decisions

| ID | Decision | Status | Rationale | Alternatives considered |
|---|---|---|---|---|
| DEC-01 | Build a production platform where users deploy autonomous agents on their own accounts | Accepted | Founder direction; software business, no custody | AI-native fund trading own capital; research copilot |
| DEC-02 | Rust for the core (runtime, risk, execution, connectors, journal); Python for research, model tooling, SDK | Accepted | Speed and safety on the hot path; Python ecosystem for research | Python-only; C++ |
| DEC-03 | The mandate is the contract: agents cannot act outside the approved spec | Accepted | Core trust promise | Prompt-level instructions only |
| DEC-04 | LLMs produce opinions; deterministic code decides sizing and places orders | Accepted | Auditability; resistance to prompt injection; speed | LLM makes the final call (TradingAgents style) |
| DEC-05 | Reducing risk never needs approval; increasing risk beyond limits always does | Accepted | Asymmetric safety; enables autonomy | Approve everything; approve nothing |
| DEC-06 | Timeouts and ambiguity resolve to a safe default that never adds risk | Accepted | Fail-safe behavior | Timeout executes the proposed action |
| DEC-07 | Journal intent before sending any order; idempotency keys; event-sourced recovery | Accepted | No duplicate orders; full audit | Best-effort logging |
| DEC-08 | One process per agent deployment | Accepted | Isolation, independent lifecycle | Many agents per process |
| DEC-09 | Organization → Workspace hierarchy; workspace is the tenant; billing per organization | Accepted | Founder requirement; clean isolation boundary | Flat accounts |
| DEC-10 | Thin global control plane; sensitive services run with the data plane; three deployment modes from one installer | Accepted | Strategy and trading intent stay on the customer's site; on-prem as packaging | Full hosted control plane with edge data plane only |
| DEC-11 | Notifications carry only opaque IDs; approval details load from the workspace deployment | Accepted | Privacy of trading intent | Full details in notifications |
| DEC-12 | Never price per trade or on assets or profits | Accepted | Regulatory posture; neutral incentives | Per-trade fees; performance fees |
| DEC-13 | Exchange testnet first, crypto perpetuals first | Accepted | Free, 24/7, safe onboarding | US equities first via Interactive Brokers |
| DEC-14 | Build Phase 0 core from first principles, piece by piece | Accepted | Founder direction; deep understanding of trading mechanics | Start directly on NautilusTrader |
| DEC-15 | Working name "Mandate" | Accepted | Matches the core concept; finance-native | Delegate, Autopilot, Principal |
| DEC-21 | Build with the founder plus AI coding agents; no hires planned for v1. The founder approves all merges and holds all production secrets | Accepted | Founder direction; speed and cost | Hire a core engineering team |
| DEC-16 | Restate for durable execution | Proposed | Single Rust binary; runs on the edge | Temporal |
| DEC-17 | NATS JetStream for messaging | Proposed | Account model maps to workspaces; single binary on the edge | Kafka / Redpanda, Redis Streams |
| DEC-18 | OIDC SSO in v1; SAML and SCIM later | Proposed | Covers Google, Okta, Entra for design partners | SAML from day one |
| DEC-19 | v1 approval channels: web push, email, one chat; SMS and phone next; native mobile later | Proposed | Fastest path; avoids app-store dependency | Native mobile app in v1 |
| DEC-20 | Declarative mandates only in v1; WebAssembly plug-ins later | Proposed | Safety and scope | Plug-ins in v1 |

## Open decisions

| ID | Question | Needed by | Inputs |
|---|---|---|---|
| OD-01 | First exchange (Bybit, Binance, OKX, Hyperliquid, Kraken, Coinbase) | M6 | Founder and design-partner jurisdictions; testnet quality; API terms |
| OD-02 | First fast decision model: Laya (self-hosted) or Jev (hosted, early access) | Phase 3 (P1 in v1) | Bake-off on labeled financial decisions: accuracy, calibration, latency, cost |
| OD-03 | Adopt NautilusTrader connectors later, or keep our own | Phase 3 | LGPL review; connector coverage |
| OD-04 | Minimum paper-trading duration before live | M9 | Design-partner feedback; risk appetite |
| OD-05 | Mobile access to on-site approval services: customer VPN, end-to-end encrypted relay, or both | M10 | Design-partner security requirements |
| OD-06 | Minimum identity data held by the global control plane in hybrid mode | M8 | Privacy review; billing needs |
