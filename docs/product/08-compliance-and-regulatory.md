# Compliance and Regulatory Considerations

| | |
|---|---|
| **Owner** | Product, with external counsel |
| **Status** | Draft v0.5 (aligned with trading domain spec v0.6 and mandate spec v0.3) |

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
| Templates are starting points users must review, not recommendations; they never ship with platform-chosen instruments or values ([DEC-38](../project/04-decision-log.md#decisions)) | Mandate authoring |
| No platform-generated "you should trade X" suggestions | Product policy |
| Trading-only OAuth scopes; reject API keys that can withdraw or transfer | Connections (PRD FR-2.2) |
| Enforce US market rules (day-trading regime, buying power and settlement, sessions, halts; no short sales) | Risk gate (PRD FR-5.10) |
| No per-trade or outcome-based pricing | [Pricing](07-pricing-and-packaging.md) |
| No marketing of expected returns; performance shown as the user's own historical results with disclosures | Product and marketing policy |
| Complete records of decisions, approvals, and configuration changes | Journal (PRD 6.7) |
| Risk disclosures accepted before going live | Go-live flow |

## Retail gating

Retail managed accounts are out of scope until counsel confirms the model. Expected
additional requirements for retail include platform policy ceilings (the retail profile:
`auto_allowed: false`, quant models only, no leveraged ETPs, protection required, a lower lifetime
loss limit; [DEC-61](../project/04-decision-log.md#decisions), pending counsel), clearer
disclosures, education, and jurisdiction checks. Ceilings are shown as limits, never pre-filled.

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
[trading domain spec §9](../specs/trading-domain.md#9-risk-gate) defines them
precisely:

- **Day trading:** FINRA replaced the pattern-day-trader rule with an intraday margin standard
  (effective June 4, 2026, with broker phase-in until October 20, 2027). Alpaca applies the new
  standard. Under Alpaca's policy, a call must be met within 2 business days and an account unmet
  by the 5th business day is frozen for 90 days; the rule itself (FINRA Rule 4210(d)(2)) requires
  satisfaction as promptly as possible, with the freeze applying to a practice of failing to meet
  deficits. The risk gate also supports the legacy rules for brokers that have not transitioned.
- **Buying power and settlement:** agents trade at 1× gross exposure with no debit balance. In
  margin accounts (all Alpaca accounts) unsettled proceeds may be reused; in cash accounts
  (other brokers) only settled cash is used, preventing good-faith and free-riding violations.
- **No short sales in v1** ([DEC-32](../project/04-decision-log.md#decisions)).
- **Sessions:** openings in the regular session only; exits may use extended hours; no overnight
  trading; no market orders in auction windows or halts.
- **Wash sales:** tax consequences for the user; surfaced as information, not advice.

## Venues and eligibility

- **Alpaca:** users open and verify accounts with Alpaca, which performs KYC; Mandate connects
  through OAuth and never performs KYC for trading accounts.
- **Kraken Derivatives US:** eligibility (identity verification, futures eligibility check, some
  state restrictions) is determined by Kraken; Mandate surfaces eligibility errors clearly.
- **Non-US venues** that exclude US persons (for example, Binance, Bybit, OKX, Hyperliquid) are
  not supported.
- Crypto rules are changing; review before each new asset class or venue.

## Posture safeguards in the product

From the risk and compliance review of the trading domain spec
([DEC-33](../project/04-decision-log.md#decisions)):

- **Mandate does not choose instruments, strategy, sizing, or limits**
  ([DEC-38](../project/04-decision-log.md#decisions)). Every order traces to a user-confirmed
  mandate version.
- **The user sets every judgment field** ([DEC-45](../project/04-decision-log.md#decisions)):
  instruments, goal, allocation, signal models and their parameters and weights, the sizing
  method, protection, all risk limits, and every `auto`. The compiler only extracts values the
  user stated, with the quoted text; unstated judgment fields stay blank until the user enters
  them. Platform defaults exist only for non-judgment fields (for example, `ask` as the autonomy
  default). Templates set structure, never values.
- **Sizing is a method the user selects** and confirms in plain language; model weights are fixed
  by the user; **there is no calibration in v1**
  ([DEC-47](../project/04-decision-log.md#decisions)). Any future calibration will be a
  user-selected method whose every change is shown and treated as a risk-increasing mandate change.
- **Signal models** ([DEC-52](../project/04-decision-log.md#decisions)) have no parameter defaults,
  document methodology only (no performance claims, rankings, or "recommended" labels), carry an
  authorship label, and are pinned by content hash. LLM models receive only the user's description,
  universe, and market inputs.
- Approval requests show the agent's proposal, the rule that triggered it, and model outputs with
  authorship; the combined score is labeled "not a probability of profit"; **never
  platform-authored alternatives, persuasive language, or profit estimates**. Live approvals
  require step-up; skip is the default.
- Goal wording avoids expectations: `profit_stop` is a level at which the agent stops, not a
  target ([DEC-46](../project/04-decision-log.md#decisions)).
- LLM model output is limited to observations, evidence, and invalidation conditions (no
  imperatives, price targets, or profit claims); it is collapsed behind "View model output" on
  approval screens and never appears in notifications. Agent memory does not feed models in v1, and
  the shared data plane carries no directional views
  ([DEC-62](../project/04-decision-log.md#decisions)).
- Acceptable use (to draft): users may not supply data feeds containing material nonpublic
  information.

## Market conduct

Autonomous agents can produce wash-trade, layering, or marking-the-close patterns without intent.
The risk gate enforces conduct controls on **opening and increasing orders** (one side at a time,
minimum resting time, a price collar on aggressiveness, participation caps, order-to-fill limits,
a close window with no market-on-close orders, and self-trade prevention across an owner-declared
group of related accounts). **Discretionary exits** (signal or goal driven) are paced by the same
controls, go out in the close window only as marketable limit orders (never MOC/LOC), and for
equities run in the regular session only; outside it they are deferred, never denied. Owner exits are paced by participation caps. Risk exits, protective orders, and
automated kill switches are exempt
([spec §9.6](../specs/trading-domain.md#96-market-conduct-controls-dec-31);
[DEC-48](../project/04-decision-log.md#decisions)). A daily surveillance
report is generated; threshold breaches are routed to the owner, whose acknowledgment is
journaled. **The platform does not supervise users' trading.** Instrument eligibility excludes
OTC, IPO-day, low-priced, and illiquid names; complex, leveraged, inverse, and volatility ETPs and
ETNs require explicit opt-in ([spec §3.2](../specs/trading-domain.md#32-eligibility-floor-dec-31)).

## Records retention

Trading records (intents; every risk-gate decision including allows, with the quotes and marks
used; mandate and model versions; LLM prompts and outputs that informed decisions; raw broker
requests and responses; fills; account snapshots; reconciliations; approvals with authentication
method; owner acknowledgments; surveillance reports) are retained **6 years after the later of
their creation and the closing of the position, lot, or account they support**, in write-once
storage; organizations may extend but not shorten; legal holds override deletion; in hybrid mode
the customer attests to the floor
([spec §13](../specs/trading-domain.md#13-records-retention-dec-33)). Counsel to confirm the period
against adviser customers' obligations (Advisers Act Rule 204-2) and the platform's own needs.

## Questions for counsel

1. Does an approval request that presents an agent-generated proposed trade, with evidence,
   constitute a recommendation or advice under the Advisers Act or state law, given that a
   user-authored mandate triggered it?
2. Do the platform-supplied signal-model library (momentum, mean reversion, trend), the LLM
   research model, mandate templates, and the compiler make Mandate the source of advice? Is the posture statement above accurate, and are the safeguards sufficient?
3. Is autonomous (AUTO) execution under a user mandate discretion by Mandate or by the user? Is
   per-field confirmation plus versioned mandates enough?
4. Should design partners be limited to entities, qualified clients, or accredited investors,
   given that individual users trading their own money are retail in substance?
5. Could order-handling logic (order types, sequencing, stop placement) or usage metering by
   agent-hours and model usage be characterized as effecting transactions or transaction-based
   compensation?
6. What is Mandate's exposure if an agent produces wash, spoofing-like, or closing-price patterns
   in securities, or in crypto spot under CEA §6(c)(1) and CFTC Rule 180.1, including wash or
   self-trades across a declared related-accounts group? Does the right to halt agents, or the
   owner's declaration of related accounts, create knowledge that implies a duty to surveil?
7. For adviser customers, is Mandate's journal a required record under Rule 204-2? What retention
   floor, integrity standard, and access undertakings should we commit to in managed and hybrid
   modes?
8. Can GDPR or CCPA deletion requests be declined for trading records and the identity records
   they depend on (who placed, approved, or acknowledged an action) for the retention period, under
   the legal-obligation and legal-claims exemptions, with personal data stored by reference and
   erased only after retention ends?
9. Do Alpaca's OAuth and third-party app terms permit autonomous order entry, reading or setting
   account configuration (margin multiplier), and multiple agents on one account? Does Alpaca view
   Mandate as a vendor within its market-access controls?
10. What does the SEC Marketing Rule require when adviser users show Mandate backtests or live
    results to investors, and can Mandate's own marketing use aggregated user results?
11. Does pending crypto wash-sale legislation (retroactive if enacted) change what we must record
    or disclose now? Does acting on users' Alpaca Crypto accounts raise New York BitLicense or
    other state issues?
12. How enforceable is the liability limit if a risk-gate defect gets a user's account restricted
    or causes losses, and what errors-and-omissions insurance is needed before live capital?
13. Can adviser users run several client accounts in one workspace, given cross-trade
    (Advisers Act §206(3)), allocation, and aggregation obligations?
14. Do suitability-like gates (the eligibility floor, the leveraged-ETP acknowledgment) create an
    implied duty of care, or liability if they fail?
15. Who owns review of the surveillance report, and does retaining it without platform review
    create exposure, given that the platform states it does not supervise users' trading?
16. Retention: is "6 years after the later of creation and closing of the supported position, lot,
    or account" sufficient given Rule 204-2 and tax periods, and can hybrid mode satisfy it through
    customer attestation?
17. Does the platform take on any duty when the broker reports intraday margin deficits or
    restrictions caused by the user's trading outside Mandate?
18. In managed mode, the platform's runtime consumes consolidated (SIP) market data licensed to the
    user's Alpaca account. Is that vendor processing, non-display use, or redistribution under
    exchange data agreements, and what licensing does Mandate need?
19. When a customer revokes a bring-your-own key, or runs in hybrid mode, what defense copy of
    records (if any) may the platform retain, and under what contract terms?
20. Can personalized LLM signal-model output (instrument-specific theses generated in real time for
    each user's universe) qualify for the publisher exclusion under *Lowe v. SEC*? If not, what
    changes would make it impersonal?
21. Is a platform-authored sizing function, applied to user-set caps, the platform determining the
    "amount" of securities for discretion purposes? Does having the user select and confirm the
    sizing method cure that?
22. Do platform-model scores used in user-written ASK/AUTO rules, or any future calibration, amount
    to platform discretion?
23. Do required judgment fields left blank for the user (rather than filled with platform values)
    adequately avoid the platform "choosing limits"? Is quoted-span extraction by the compiler
    acceptable?
24. What evidence (rendered text, version, authentication, timestamp) makes mandate confirmations
    and disclosure acceptances enforceable under E-SIGN and UETA, and adequate in a dispute?
25. Should leveraged and inverse ETPs be prohibited, or limited by holding period, for retail users
    rather than only gated by an acknowledgment (FINRA Regulatory Notice 09-31)? What should the
    retail profile's values be?
26. Could approval prompts (push timing, scores, theses, short timeouts) be treated as digital
    engagement practices or behavioral nudges that amount to recommendations under existing
    anti-fraud law, state unfair-practices law, or FTC Act §5? What should approval requests
    exclude?
27. If adviser customers rely on Mandate's two-approver and independent-approval controls as part
    of their Rule 206(4)-7 compliance program, does Mandate take on vendor or oversight obligations?
28. Do product terms such as "signal model", "combined score", and "profit stop" still create
    holding-out risk under the Advisers Act or state law?
29. Is treating goal-driven and signal-driven sales as discretionary exits (AUTO, paced by conduct
    controls, never denied) defensible under Exchange Act §9(a)(2) and CFTC Rule 180.1?
30. Would an optional, separately priced shared feed of platform-computed classifications of public
    events be a signal service, or qualify as impersonal publishing? What must it exclude to stay
    data?

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
| Prompt injection through news or social content causing trades | LLMs produce outputs only; the order builder and risk gate are deterministic; mandate limits cap impact; unusual inputs trigger escalation |
| Model errors or hallucinated instruments | Typed outputs from decision models; instrument validation against the mandate universe |
| Overconfident models | Self-reported confidence is labeled uncalibrated; combined-score thresholds in the user's autonomy rules; a missing model counts as fully bearish for buys and as zero for exits; scorecards for the user's review |
| Model provider changes | The content hash pins the underlying model identity; the gateway never substitutes a model; withdrawals are journaled `PlatformOperatorAction` events and outputs then count as missing ([DEC-67](../project/04-decision-log.md#decisions)) |

## Terms and disclosures (to draft with counsel)

- Terms of service: software license; user responsibility for mandates and trading decisions;
  no advice; limitation of liability.
- Risk disclosure: leverage, automated trading, model error, venue and connectivity risk.
- Acceptable use: no market manipulation; the platform may stop agents that exhibit
  manipulative patterns.
