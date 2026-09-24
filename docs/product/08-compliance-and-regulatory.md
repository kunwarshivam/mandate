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

- does **not** hold customer funds, or any permission that can move them (withdrawals or
  transfers);
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
| Trading-only OAuth scopes; reject API keys that can withdraw or transfer | Connections (PRD FR-2.2) |
| Enforce US market rules (day-trading regime, settlement, short sales, market hours) | Risk gate (PRD FR-5.10) |
| No per-trade or outcome-based pricing | [Pricing](07-pricing-and-packaging.md) |
| No marketing of expected returns; performance shown as the user's own historical results with disclosures | Product and marketing policy |
| Complete records of decisions, approvals, and configuration changes | Journal (PRD 6.7) |
| Risk disclosures accepted before going live | Go-live flow |

## Retail gating

Retail managed accounts are out of scope until counsel confirms the model. Expected
additional requirements for retail include suitability-style guardrails (conservative
presets, leverage caps), clearer disclosures, education, and jurisdiction checks.

## United States

Mandate serves the United States first ([DEC-22](../project/04-decision-log.md#decisions)).
Questions for counsel, by regulator:

| Area | Applies to | Question for counsel |
|---|---|---|
| SEC: Investment Advisers Act; state adviser rules | US stocks and ETFs (Alpaca) | Does a platform where users define mandates and agents trade in the user's own brokerage account constitute advice or discretion? Which product constraints keep it tooling? |
| SEC / FINRA: broker-dealer | Order flow | Confirm that flat subscription pricing and no per-transaction compensation keep Mandate outside broker-dealer activity |
| CFTC / NFA: commodity trading advisor | Crypto perpetuals (Kraken Derivatives US), later CME futures | Does automated trading software acting on user-defined mandates fall within the commodity trading advisor definition, and do any exemptions apply? |
| State money transmission | All | Confirm no-custody design avoids money-transmitter licensing |
| New York (BitLicense) | Crypto spot | Confirm software without custody is outside "virtual currency business activity" |
| Broker and venue terms | Alpaca, Kraken | Confirm third-party platform use; Alpaca OAuth app requirements; Kraken API terms for platforms acting for users |

### US market rules the product must enforce

These are rules on the **user's account** that an autonomous agent could otherwise break. The
risk gate enforces them (PRD FR-5.10), and the
[trading domain spec §9](../specs/trading-domain.md#9-account-rules-risk-gate) defines them
precisely:

- **Day trading:** FINRA replaced the pattern-day-trader rule with an intraday margin standard
  (effective June 4, 2026, with broker phase-in until October 20, 2027). Until each broker
  transitions, the legacy limits may still apply; the risk gate supports both regimes.
- **Settlement:** cash accounts cannot trade with unsettled proceeds.
- **Short sales:** locate and borrow requirements and short-sale price restrictions apply.
- **Market hours:** regular, extended, and overnight sessions differ by asset; crypto trades
  continuously.
- **Wash sales:** tax consequences for the user; surfaced as information, not advice.

## Venues and eligibility

- **Alpaca:** users open and verify accounts with Alpaca, which performs KYC; Mandate connects
  through OAuth and never performs KYC for trading accounts.
- **Kraken Derivatives US:** eligibility (identity verification, futures eligibility check, some
  state restrictions) is determined by Kraken; Mandate surfaces eligibility errors clearly.
- **Non-US venues** that exclude US persons (for example, Binance, Bybit, OKX, Hyperliquid) are
  not supported.
- Crypto rules are changing; review before each new asset class or venue.

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
