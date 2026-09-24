# Compliance and Regulatory Considerations

| | |
|---|---|
| **Owner** | Product, with external counsel |
| **Status** | Draft v0.1 |

> This document records product positions and open questions. It is not legal advice.
> Every position below must be confirmed by securities and data-protection counsel before
> launch, and again before any retail offering.

## Regulatory posture

Mandate is a **software platform**. Users define their agents' mandates and connect their own
accounts. The platform:

- does **not** hold customer funds or accept withdrawal-enabled credentials;
- does **not** recommend trades, provide personalized advice, or sell signals;
- does **not** charge per trade or on assets or profits;
- does **not** route orders for compensation per transaction.

### Why this matters

In the US, a business that, for compensation, advises others on buying or selling securities
may need to register as an investment adviser; auto-trading in a client's account is commonly
treated as advice or discretion. Businesses that effect securities transactions for others,
especially for per-transaction compensation, may need broker-dealer registration. The product
positions above are designed to keep Mandate on the tooling side of those lines, but the
"agent acts in the user's account" pattern is a gray area that counsel must review, especially
for retail users.

## Product requirements derived from this posture

| Requirement | Where enforced |
|---|---|
| Users author and approve every mandate; the compiler's output is always shown for confirmation | Mandate authoring (PRD 6.3) |
| Templates are starting points users must review, not recommendations | Mandate authoring |
| No platform-generated "you should trade X" suggestions | Product policy |
| Reject withdrawal-enabled keys | Connections (PRD FR-2.2) |
| No per-trade or outcome-based pricing | [Pricing](07-pricing-and-packaging.md) |
| No marketing of expected returns; performance shown as the user's own historical results with disclosures | Product and marketing policy |
| Complete records of decisions, approvals, and configuration changes | Journal (PRD 6.7) |
| Risk disclosures accepted before going live | Go-live flow |

## Retail gating

Retail managed accounts are out of scope until counsel confirms the model. Expected
additional requirements for retail include suitability-style guardrails (conservative
presets, leverage caps), clearer disclosures, education, and jurisdiction checks.

## Jurisdictions and venues

- Several crypto exchanges restrict users by country (for example, Binance and Bybit restrict
  US persons). Connectors must respect venue terms; the platform should warn when a
  connection's venue is not available in the user's declared jurisdiction.
- Equities connectors depend on the broker's own onboarding and KYC; Mandate does not perform
  KYC for trading accounts.
- Crypto rules vary by jurisdiction and are changing; review per launch market.

## Data protection

- Personal data minimized in the global control plane (IDs and roles only in hybrid mode).
- GDPR / CCPA: data subject requests supported; crypto-shredding for personal data inside the
  immutable journal ([HLD §7](../HLD.md#7-logging-and-audit)).
- Data processing agreements with sub-processors (hosting, notifications, billing, model
  providers). Hybrid and on-prem customers can restrict model providers to local models.

## Security and assurance

- SOC 2 Type I, then Type II, targeted before Enterprise sales (roadmap Phase 4).
- Penetration test before design partners go live with real capital.
- Documented incident response and customer notification process.

## AI-specific risks

| Risk | Control |
|---|---|
| Prompt injection through news or social content causing trades | LLMs produce opinions only; the decider and risk gate are deterministic; mandate limits cap impact; unusual inputs trigger escalation |
| Model errors or hallucinated instruments | Typed outputs from decision models; instrument validation against the mandate universe |
| Overconfident models | Calibration against realized outcomes; confidence thresholds for escalation |
| Model provider changes | Model versions pinned per mandate version; changes create a new version |

## Terms and disclosures (to draft with counsel)

- Terms of service: software license; user responsibility for mandates and trading decisions;
  no advice; limitation of liability.
- Risk disclosure: leverage, automated trading, model error, venue and connectivity risk.
- Acceptable use: no market manipulation; the platform may stop agents that exhibit
  manipulative patterns.
