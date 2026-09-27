# Strategy Options

| | |
|---|---|
| **Owner** | Product |
| **Status** | Draft v0.3, 2026-09-27. Research input for the founder, updated for the founder's decisions on DEC-141, DEC-145, DEC-148, and DEC-149 and for option 16. Not a PRD change, and not legal advice |
| **Inputs** | [Issue #177](https://github.com/kunwarshivam/mandate/issues/177), the [competitive landscape](03-competitive-landscape.md) v0.2, [vision](01-vision-and-strategy.md), [PRD](04-prd-v1.md), [pricing](07-pricing-and-packaging.md), [compliance](08-compliance-and-regulatory.md), [ADR-0002](../adr/0002-autonomous-ideation-and-retail.md), and the [decision log](../project/04-decision-log.md) |
| **Method** | Public web sources, read 2026-09-26 and 2026-09-27, then fact-checked against each source page. No accounts, outreach, broker tools, or orders |
| **Decision** | [DEC-141](../project/04-decision-log.md#decisions), Accepted (founder, 2026-09-27): Mandate stays the complete product; an owner's own agent may connect as an optional channel through a Mandate MCP server over the owner-input API. [DEC-148](../project/04-decision-log.md#decisions), Accepted (founder, 2026-09-27): that channel (E10-6) is pulled forward to M8 and stays optional, never the main path. [DEC-145](../project/04-decision-log.md#decisions), Accepted (founder, 2026-09-27): no platform-paid loss or breach guarantee. [DEC-149](../project/04-decision-log.md#decisions), Accepted (founder, 2026-09-27): the harness is an enterprise product, and the platform runs through it ([positioning](#positioning-harness-and-platform-dec-149)) |

> **Regulatory material in this document is a summary of public guidance and commentary. It is not
> legal advice and draws no legal conclusion.** Legal and compliance text is reserved for the founder
> and counsel ([DEC-79](../project/04-decision-log.md#decisions),
> [DEC-102](../project/04-decision-log.md#decisions)). Each regulatory point is a flag for counsel,
> cross-referenced to the [questions for counsel](08-compliance-and-regulatory.md#questions-for-counsel).

Source keys in square brackets refer to the [competitive landscape's source list](03-competitive-landscape.md#sources)
or, for sources used only here, to [this document's source list](#sources).

## Summary

1. **People already let general-purpose agents trade retail accounts, and brokers say they do not
   supervise those agents.** Robinhood reported "nearly 100 thousand customers" with agentic accounts
   and "over $100 million in AUC" as of 2026-07-29 [RH4]. Its management named the friction:
   "stitching together these 2 apps", and models that "sometimes ... will fight you" [RH5]. Alpaca
   reported API usage growth of "nearly 4x quarter-over-quarter" in Q1 2026 [AL7].
2. **Delegation is still rare.** 62% of surveyed US retail investors use AI to inform decisions, but
   4.5% use automated trading algorithms and 6.4% use AI portfolio-management tools [IN1] [IN2]. 26%
   would let AI manage their investments [BE1].
3. **Idea origination is the least proven and most regulated part of the plan.** Language models
   lost money in Alpha Arena's US-equities season [NF2]. Platform-originated, personalized ideas are
   the shape of investment advice, which is why DEC-98 assumes Mandate may need to register.
4. **Recommendation, as decided in DEC-141.** The complete product leads: the owner sets the
   envelope and the platform's research agent (DEC-97) brings the ideas. As an optional channel, an
   owner who already runs their own agent can connect it through a Mandate MCP server that exposes
   the same API Mandate uses to take the owner's input, so every request passes the same builder,
   autonomy rules, gate, ledger, and journal. That channel is an adoption on-ramp, not a separate
   product, and never the only or the main path; DEC-148 pulls its story (E10-6) forward to M8.
   Hedge with compliance evidence for small advisers and an agent-safety conformance suite.

## 1. Market evidence

| Finding | Source |
|---|---|
| Robinhood: "To date, nearly 100 thousand customers have opened Agentic Trading accounts, with over $100 million in AUC" (2026-07-29). Strategies: "over 300 thousand Funded Customers with nearly $2 billion" | [RH4] |
| Robinhood, Q2 2026 call (2026-07-29): users dislike "going to a codex or a Claude code [sic] and kind of stitching together these 2 apps"; models "sometimes ... will fight you" | [RH5] |
| Robinhood and Public: neither will "control, supervise, monitor, recommend, or audit" third-party agents | [RH1] [PU11] |
| Public Agents generally available on 2026-08-05; available to everyone by 2026-09-15; no usage figures published | [PU7] [PU13] |
| Alpaca: API usage growth "accelerated nearly 4x quarter-over-quarter" in Q1 2026, with monthly growth near 30%. Separately, monthly active API users "grew nearly 4x during the last six months" (July 2026). $150 million Series D in January 2026; $135 million round in July 2026 ($435 million with debt) | [AL7] [AL3] [AL8] |
| Webull: "more than 28 million registered users"; "more than 204 million simulated orders" on paperTrade | [WB1] [WB2] |
| Interactive Brokers: "approximately 4.4 million cleared customer accounts" at the end of 2025 | [IK1] |
| QuantConnect: "more than 375,000 live strategies" since 2012 (vendor claim) | [QC4] |
| Scalar Field: "around 800 paying traders, over 34,000 signups, and $74,000 in monthly revenue" (vendor claim, more than a year old) | [SF6] |
| Composer by SoFi: launched 2026-06-23 with over 2,000 community-built strategies | [CO1] |
| Autopilot: "about three million downloads, including 80,000 paid subscribers"; Form ADV of 2025-04-29 lists $462 million across 132,559 accounts; the $750 million figure is the founder's LinkedIn claim | [AP1] |
| Investing.com survey (n=938): 62% use AI tools to inform decisions; trust in AI: 54% somewhat, 20% mostly, 4% completely | [IN1] |
| Investing.com: 6.4% use AI portfolio-management tools and 4.5% use automated trading algorithms | [IN2] |
| Betterment 2025: "just 30% said they trust AI to give them financial advice, and just 26% would let AI manage their investments" | [BE1] |
| FINRA Foundation: 34% of US adults hold investments outside retirement accounts | [FF1] |
| FINRA Foundation: among investors under 35, 43% trade options, 22% buy on margin, and 61% use influencer recommendations | [FF2] |
| Investment Adviser Association: 16,544 SEC-registered advisers serving 73.7 million clients; "92.8% had 100 or fewer non-clerical employees, while 67.4% managed less than $1 billion" | [IA1] |

**Reading.** The pool of people running agents on retail accounts is real and recent. The friction
and the unsupervised gap are documented by the brokers themselves. Delegation is a trust problem
before it is a capability problem.

## 2. Regulatory evidence (public commentary, not legal advice)

| Topic | What the public source says | Flag for counsel |
|---|---|---|
| FINRA 2026 oversight report | Names AI-agent risks: "Autonomy: AI agents acting autonomously without human validation and approval", and "Scope and Authority: Agents may act beyond the user's actual or intended scope and authority" [FR1] | Maps onto the gate, autonomy policy, and journal. Useful for broker vendor reviews (question 7) |
| House Financial Services letter | Eight members (Foster, Sherman, Lynch, Himes, Casten, Tlaib, Garcia, Pettersen) sent the SEC 13 questions on agentic trading on 2026-06-23, with answers requested "by July 31, 2026". Question 7 asks when "an AI agent or its developer be required to register" [HL1] | Directly on questions 31 and 32. No SEC answer found |
| SEC 2026 examination priorities | Covers "automated investment tools, AI technologies, and trading algorithms", and whether advice is consistent with investors' profiles or stated strategies [SE1] | If Mandate registers, the journal becomes exam evidence (questions 27, 31) |
| SEC Chair remarks, 2026-09-10 | AI "should serve as a complement to—not a substitute for—human judgment"; "Widespread reliance on similar tools can allow errors to cascade" [SE2] | Echoes DEC-100 (correlated flow) |
| Predictive data analytics proposal | Withdrawn; notice dated 2025-06-12, effective 2025-06-17 [SE3] | No agent-specific conflicts rule to build to; general duties remain |
| AI-washing enforcement | Delphia paid $225,000 and Global Predictions $175,000 over AI claims (2024) [SE4] | Marketing copy must be literally true (question 35) |
| Market access rule | Knight Capital paid $12 million under the market access rule (2013) [SE5] | Broker controls are generic; Mandate's are per mandate |
| Internet adviser exemption | Amended rule, compliance date 2025-03-31 [SE6] | A possible registration route to compare (question 31) |
| CFTC exemption 4.14(a)(9) | Unavailable where advice is tailored to particular clients' positions or circumstances [CF1] | Weighs against personalized crypto perpetuals first (Kraken, DEC-98 connector 3) |
| Broker terms | Robinhood and Public disclaim supervision of connected agents [RH1] [PU11] | Question 32: does that shift any obligation to Mandate? |

Postures seen among peers (documented by each company): broker-native (Public, Robinhood, eToro,
Composer by SoFi); software or technology provider for self-directed accounts (Scalar Field,
TradeAgentic, Surmount); registered adviser (Composer, Quantbase for Surmount's internal accounts,
Autopilot, Autonomous). Whether a mode where the owner's own agent proposes carries a different
posture from platform ideation is for counsel (question 34 today covers bring-your-own-strategy only).

## 3. Willingness to pay

| Product | Price (documented unless marked) | Model | Source |
|---|---|---|---|
| Public Agents | No separate charge; standard fees | Bundled with brokerage | [PU6] |
| Composer | $0; $10 a month; Pro $32 a month billed yearly ($384). Form CRS (March 2024): "$30 per month or $288 annually" | Flat subscription, registered adviser | [CO2] [CO3] |
| Surmount | Core $50, Plus $100, Pro $300 a year; "1% management fee" on internal (Quantbase-managed) accounts | Subscription, plus an asset fee on managed accounts | [SU2] |
| Autopilot | $29 a quarter or $100 a year (reported) | Flat subscription, registered adviser | [AP1] |
| Option Alpha | "$99 /mo with annual billing or $149 monthly" | Subscription; free through some brokers | [OA1] |
| TradersPost | Tiers from about $40 to about $300 a month, depending on billing period | Subscription | [TP1] |
| Scalar Field | Free, then Pro and Ultra tiers by credits (vendor docs) | Subscription plus credits | [SF6] |

**Reading.** Retail delegation sells at $0 to about $32 a month, or through an asset fee on managed
accounts; Public's Agents set a floor of zero. Active-trader automation sells at $40 to $300 a month.
Mandate's own documents rule out per-trade and asset- or profit-based pricing
([pricing](07-pricing-and-packaging.md)), so every option below is priced per organization, per live
agent, or by usage. The research agent's model cost per agent-day is unmeasured; a flat retail plan
may not cover it, so usage passthrough matters for the ideation tier and much less for a tier where
the owner's own agent pays its own model costs.

## 4. Customer segments

| Segment | Evidence of the need | Where they are | Fit with accepted plans |
|---|---|---|---|
| **A. Retail owners already running an agent** on Robinhood or Alpaca | Robinhood's figures and friction [RH4] [RH5]; brokers disclaim supervision | Claude, Cursor, and ChatGPT communities; Alpaca MCP users | Retail from the start (DEC-98); Alpaca first, Robinhood second |
| **B. Retail owners who want the platform to bring ideas** | 26% would let AI manage [BE1]; Composer and Autopilot sell delegated strategies | Mass retail | DEC-97; gated by DEC-99 and counsel |
| **C. Professional systematic individuals** (vision's Alex) | Option Alpha, TradersPost, QuantConnect pricing | r/algotrading, QuantConnect, TradingView | v1 design partners |
| **D. Small advisers and emerging managers** (vision's Priya) | 67.4% of SEC advisers manage under $1 billion [IA1]; exam priorities on automated tools [SE1] | Custodians Schwab, Fidelity, Interactive Brokers | v1 design partners; custodian reach is the gap |
| **E. Small systematic trading teams** (#177) | No direct evidence gathered | Unknown | Later segment in the vision |
| **F. Brokers and fintechs adding agent access** | Brokers disclaim supervision; FINRA names the risks [FR1] | Alpaca Broker API partners, Tradier, Webull | Not in the plan |

## 5. Strategy options

Options 0 to 12 come from the strategy research; options 13 to 15 are those in issue #177; option 16
is the founder's, from 2026-09-27. Every
experiment is paper only, spends nothing, and contacts no one without the founder's authorization.
"Kill" means stop the option, not the product.

### Overview

| # | Option | Target | Revenue model | Regulatory exposure (flag) |
|---|---|---|---|---|
| 0 | Baseline: platform-originated theses, retail first (#177's recommended variant) | A, B | Per-organization plan plus usage | High: adviser (DEC-98; questions 31, 33) |
| 1 | The owner's own agent as an optional channel (MCP over the owner-input API; DEC-141) | A, C | Part of the plan; per live agent | Unknown; counsel (questions 31, 32, 34) |
| 2 | Read-only monitor for agents already running at Public or Robinhood | A | Low monthly subscription | Low; data-access terms |
| 3 | Compliance evidence for small advisers and emerging managers | D | Per organization a month; hybrid licence | Vendor to regulated firms (questions 7, 27) |
| 4 | Supervision layer licensed to brokers | F | Platform licence plus per-account fee | Brokers' own supervision duties; heavy vendor review |
| 5 | Open-core: open spec, journal format, and verifier; sell hosting | C, F | Hosted plans, support | Low for the open part |
| 6 | Agent-safety conformance suite | F, builders | Certification or licence | Low |
| 7 | Insurance and audit partnership | Insurers and their insureds | Audit fees; any revenue share needs counsel | Insurance distribution if Mandate sells cover |
| 8 | Public forward-paper research league | Builders, press | Indirect | Marketing Rule, hypothetical performance (questions 10, 35) |
| 9 | Crypto first | Crypto traders | Subscription | CTA exposure, leverage |
| 10 | Register first as an adviser | B | Flat subscription | This is the regulatory path |
| 11 | Embedded agents for Alpaca Broker API fintechs | F | Per end-user licence | Partner is the regulated front; cross-border |
| 12 | Prediction-market agents | Active retail | Subscription | CFTC event contracts; state disputes |
| 13 | Bring-your-own-strategy as the lead and as a diagnostic (#177) | C, D | Per live agent | Question 34 |
| 14 | Small systematic trading teams (#177) | E | Per organization | Depends on who owns the ideas |
| 15 | Customer-side deployment and private models (#177) | D, E | Hybrid and on-prem licence | As 0 or 13 |
| 16 | Mandate experiments: multi-variant shadow mode (founder, 2026-09-27) | A, C, D | Part of the plan; model cost per variant | Hypothetical performance (questions 10, 35) |

### Option 0: Baseline, platform-originated theses (DEC-97)

- **Value.** "The agent brings its own ideas, inside an envelope you set", with the thesis promise
  made concrete through controls, approvals, and evidence (#177).
- **Why now.** Brokers opened accounts to agents this year; the accepted direction (DEC-97, DEC-98).
- **For.** It is the product the founder chose; the admission machinery (ADR-0002) is specified; it
  is the only option where the platform's research is the product.
- **Against.** Thesis quality is unproven (Alpha Arena [NF2]); users see no research until DEC-99
  passes (DEC-103); adviser exposure and counsel cost come first; model cost per agent is unmeasured.
- **Plan impact.** None: M5 to M7, E17 thin slice, DEC-99 evaluation, counsel (DEC-102).
- **Experiment.** Run the DEC-103 thin slice for four weeks of forward paper on the research basket
  with pre-registered baselines (E17-8), and publish the result internally.
- **Kill.** None of the tested configurations beats buy-and-hold of the basket net of modeled costs
  over the window. That argues against leading with DEC-97, not against keeping it.

### Option 1: The owner's own agent as an optional channel (accepted as DEC-141, as an on-ramp)

The research first framed this as a lead "gateway" product. The founder's decision keeps the
evidence and changes the role: the complete product leads, and this is an optional channel over the
owner-input API, not the main path. DEC-148 pulls its story forward without changing that role. The
evidence below is unchanged.

- **Target.** Segment A, then C: owners already running Claude, ChatGPT, Codex, or a custom agent on
  Alpaca or Robinhood.
- **Value.** "Bring the agent you already use. It works through the same controls as you do: it
  cannot change your envelope, it cannot approve its own requests, every action has a receipt, and
  your kill switch stops it."
- **Why now.** Robinhood, eToro, and Alpaca's MCP v2 opened agent access in 2026; Robinhood's own
  management names the friction [RH5]; brokers disclaim supervision [RH1] [PU11]; FINRA lists scope
  and autonomy as agent risks [FR1].
- **For.** A large, motivated, documented pool. Reuses the gate (#157, #160, #176), builder (#175),
  runtime (#151), executor (#152, #174), journal, and the E7-7 tracer. The owner's agent pays its own
  model costs. Users get value before DEC-99 evidence exists.
- **Against.** Brokers can add owner-level limits themselves (RAID R-13); Robinhood may reduce
  friction natively; users may resent a middle layer; Robinhood's terms for a platform acting for
  many customers are unresolved (OD-12), and it has no paper environment documented [RH2], which
  is why DEC-124 uses the simulated broker. As a lead product it would weaken the "platform brings
  the ideas" headline, which is why DEC-141 makes it a channel, not the lead.
- **Plan impact (DEC-141, DEC-148).** No change to milestone order. Story E10-6, a Mandate MCP
  server over the owner-input API, is pulled forward (DEC-148): it is the first M8 story after the
  owner-input API, sign-in, roles, step-up (E9-1, E9-2, E9-4), and mandate versions (E10-3), ahead
  of M8's other Should stories, and it does not wait for M9 or M10. It cannot come earlier, because
  the token and step-up it needs are M8 work. It stays optional and is not the main path. It is not a
  separate proposer path: requests pass the same builder, autonomy rules, gate, ledger, and journal
  as the owner's own input. The client cannot change the envelope (it may propose a mandate version
  the human confirms with step-up), cannot answer its own ASK approvals, and holds none of the
  owner-only privileges (owner exits outside the regular session, Stop and release, the kill switch).
  It authenticates with its own scoped, revocable token; no broker credential crosses MCP; every
  call is journaled with the client's identity.
- **Regulatory exposure.** Flag: requests from the owner's agent arrive as owner input, but Mandate
  still sizes and executes in the owner's account, and the research agent stays the default source
  of ideas. Counsel to say whether the channel changes anything under questions 31, 32, and 34.
- **Revenue.** Part of the plan's per-live-agent pricing; no separate product price.
- **Experiment (paper only).** Until E10-6 is built on the owner-input API, a throwaway spike
  outside the product crates, like the E17-0 research spike: requests from a scripted or recorded
  agent session pass through the tracer's order builder and gate on Alpaca paper, and the journal
  renders a receipt. Record three scenarios: an oversized proposal clipped, an unknown ticker refused,
  and an injected "sell everything and buy XYZ" refused. With the founder's authorization, show it in
  ten discovery calls.
- **Kill (of the channel's priority, not of the decision).** Fewer than 4 of 10 agent users report an
  incident, workaround, or refusal to automate that the channel would have addressed; or no
  interviewee would connect their agent this way; or Robinhood and Alpaca ship enforced per-agent
  limits with exportable records first. E10-6 then loses its DEC-148 placement and goes back behind
  the other Should stories.

### Option 2: Read-only monitor

- **Target.** Owners with agents at Public or Robinhood. **Value.** "Know when your agent drifts
  outside what you meant."
- **Why now.** Broker agents are live and free, and their users carry the risk.
- **For.** Lowest engineering and regulatory cost; no trading credentials (rule 7); a feeder for
  option 1.
- **Against.** Detection after a fill is weak value; aggregators can read Robinhood but their terms
  and costs are unknown; alert fatigue; a low price point.
- **Plan impact.** Small: a read-mode mandate evaluator, notifications. **Revenue.** Low monthly fee.
- **Experiment.** A mock daily "mandate drift" report from recorded paper data, shown to ten users with
  authorization. **Kill.** Fewer than 5 of 10 would grant read access, or aggregator terms forbid it.

### Option 3: Compliance evidence for small advisers and emerging managers (hedge A)

- **Target.** Segment D. **Value.** "The investment policy, enforced before each order and recorded
  for the exam."
- **Why now.** Exam priorities cover automated tools and controls matching disclosures [SE1]; most
  advisers are small [IA1].
- **For.** Larger contracts; records are already designed with retention in mind; two-approver rules
  and SSO are already planned; the adviser owns the ideas, which may change Mandate's own posture
  (questions 7, 27); matches the vision's second v1 segment.
- **Against.** Advisers custody at Schwab, Fidelity, or Interactive Brokers, not Alpaca; Fidelity has
  no public trading API (ADR-0002); long sales cycles and SOC 2 expectations; many advisers rebalance
  models rather than trade; order-management incumbents were not researched.
- **Plan impact.** Pulls M8 roles and SSO, M11 hybrid, and audit export earlier; needs a custodian
  connector beyond Alpaca.
- **Revenue.** Per organization a month; hybrid licence.
- **Experiment.** Build a sample exam packet from a paper run (mandate versions, gate decisions with
  inputs, approvals, verified export) and, with authorization, test it in five interviews with chief
  compliance officers or operating officers. **Kill.** Fewer than 2 of 5 name a budget and a custodian
  Mandate can reach within 12 months.

### Option 4: Supervision layer licensed to brokers

- **Target.** Segment F. **Value.** A drop-in control layer for the supervision brokers disclaim.
- **Why now.** Brokers opened agent access while disclaiming supervision; FINRA names the risks [FR1].
- **For.** Very large distribution per deal.
- **Against.** Public built its controls in-house; brokers prefer to own controls and customers;
  enterprise sales and security reviews are heavy for this team; channel conflict with option 1.
- **Plan impact.** Major: multi-tenant embedding and SOC 2 early; the owner experience drops back.
- **Revenue.** Platform licence plus per-account fee. **Regulatory.** Brokers' vendor-supervision
  duties apply to Mandate as a vendor.
- **Experiment.** A two-page controls brief mapped to FINRA's report, with no outreach; with
  authorization, one validation call with a mid-size API broker. **Kill.** No broker takes a second
  meeting within four weeks.
- **Positioning ([DEC-149](../project/04-decision-log.md#decisions)).** Brokers are one of
  the enterprise buyers of the harness ([positioning](#positioning-harness-and-platform-dec-149));
  this option's score is unchanged.

### Option 5: Open-core

- **Target.** Builders and brokers evaluating standards. **Value.** "A standard, inspectable contract
  for delegated trading; verify any agent's journal yourself."
- **Why now.** The control-layer story is contested (Regent, TradeAgentic); signed mandates are
  becoming an industry idiom in agent payments.
- **For.** LEAN shows open engine to paid cloud works [QC5]; an inspectable gate earns trust faster
  than claims; contributors add broker failure cases.
- **Against.** Gives competitors a free control layer; a licence choice is hard to reverse; support
  load; the advantage moves to operations.
- **Plan impact.** A licensing ADR; spec and verifier polish. The repository is already public, so the
  experiment is about packaging and a licence, not visibility.
- **Experiment.** Package the journal spec and `mandate-cli journal verify` as a standalone verifier
  with examples; publishing it is the founder's decision. **Kill.** No external integration request
  within four weeks of publication.

### Option 6: Agent-safety conformance suite (hedge B)

- **Target.** Brokers adding MCP access, agent builders, and insurers. **Value.** "Prove your agent
  stack is safe to connect."
- **Why now.** Brokers opened MCP access this year; FINRA's agent risks [FR1]; FINRA's 2015 notice on
  algorithmic trading supervision lists testing and kill-switch practices [FR2].
- **For.** Mostly exists: reference cases, fault injection (M6 exit), prompt-injection fixtures
  (DEC-101). #177 names "accumulated broker failure cases and conformance tests" as a durable
  advantage. Strengthens option 1's trust claim and opens option 4 later.
- **Against.** Certifying needs neutrality and brand, and Mandate competes with those it would
  certify; small direct revenue.
- **Plan impact.** Packages E4 simulation, E6 gate tests, and E7 fault injection as a runnable suite.
- **Experiment.** Fifteen scenarios runnable against the simulator and Alpaca paper, with Mandate's
  own results; with authorization, ask two builders to run it. **Kill.** No outside party runs it
  within four weeks of availability.
- **Positioning ([DEC-149](../project/04-decision-log.md#decisions)).** The conformance suite
  is part of the harness sold to enterprises
  ([positioning](#positioning-harness-and-platform-dec-149)); this option's score is unchanged.

### Option 7: Insurance and audit partnership

- **Target.** Insurers covering AI agents, and their insureds. **Value.** Records that make agent
  trading underwritable.
- **Why now.** Insurers began offering AI-agent cover in 2025 and 2026 (secondary sources, not
  verified here).
- **For.** Addresses the liability every broker disclaimer leaves; little engineering.
- **Against.** Early market focused on enterprises; market losses are not "errors"; any revenue share
  raises compensation and licensing questions for counsel.
- **Experiment.** A desk comparison of one published underwriting standard's controls with Mandate's.
  No contact. **Kill.** Investment-decision losses are excluded, or no partner is reachable within six
  months. A later option. Mandate paying losses itself, insured or self-funded, is ruled out by
  [DEC-145](../project/04-decision-log.md#decisions); this option is about records, not cover.

### Option 8: Public forward-paper research league

- **Target.** Builders, model labs, press. **Value.** Pre-registered, net-of-cost, baseline-relative
  evidence of whether AI trade ideas work.
- **Why now.** Alpha Arena drew attention with contests that were not pre-registered [NF2].
- **For.** Reuses the DEC-99 evaluator (E17-8); tells Mandate whether DEC-97 is worth its cost.
- **Against.** Marketing Rule and hypothetical-performance risk if tied to the product (questions 10,
  35); model spending needs the founder.
- **Experiment.** The option 0 experiment with three model configurations, internal only. **Kill.**
  Publishing is refused by counsel or the founder; the internal run still informs option 0.

### Option 9: Crypto first

- **Target.** Crypto-native traders. **Value.** Agents matter most when people sleep.
- **Why now.** US crypto perpetuals onshore: Coinbase from 2025-07-21, up to 10x leverage [CB2].
- **For.** Clear around-the-clock value.
- **Against.** CTA exposure for tailored advice [CF1]; leverage harm to retail; conduct questions.
- **Plan impact.** Reorders DEC-23 and DEC-98 connectors; perpetuals need margin and liquidation
  accounting.
- **Experiment.** Desk only: which gate rules change for 24/7 spot, using Alpaca crypto paper.
  **Kill.** Counsel indicates CTA registration is likely for personalized perpetuals; keep spot crypto
  on the existing plan.

### Option 10: Register first as an adviser

- **Target.** Segment B. **Value.** "The registered AI trading agent whose limits you set and whose
  every decision you can audit."
- **Why now.** Composer and Autopilot show registered, software-delivered strategies sold by flat
  subscription [CO3] [AP1]; the internet adviser rule was amended [SE6].
- **For.** Turns DEC-98's liability into a trust signal; it is probably required for option 0 at scale.
- **Against.** Cost and time (compliance officer, manual, Form ADV, exams); fiduciary duty over the
  research agent's ideas; Marketing Rule limits on showing performance.
- **Plan impact.** Counsel and a compliance build move onto the critical path (DEC-102).
- **Experiment.** A counsel-led route comparison with cost and time estimates; no filing. **Kill.** The
  estimate exceeds the founder's budget or time.

### Option 11: Embedded agents for Alpaca Broker API fintechs

- **Target.** Fintechs built on Alpaca. **Value.** "Add safe autonomous agents to your app."
- **Why now.** Alpaca's API growth [AL7].
- **For.** Alpaca's distribution; the partner is the regulated front.
- **Against.** Many partners are outside the US (DEC-22); partners can build on Alpaca's MCP server
  themselves; multi-tenant hosting early.
- **Experiment.** A desk scan of Alpaca's partner list for firms advertising AI features; with
  authorization, three conversations. **Kill.** No partner with US retail users and an AI roadmap
  engages.

### Option 12: Prediction-market agents

- **Target.** Active retail. **Why now.** Public added Kalshi on 2026-09-24 [PU12].
- **Against.** Outside DEC-23's asset scope; state disputes; new accounting; #177 warns against new
  surface. **Experiment.** None. **Kill.** Default: not in v1.

### Option 13: Bring-your-own-strategy as the lead and as a diagnostic (#177)

- **Target.** Segments C and D. **Value.** The owner's pinned universe and fixed signal models, with
  Mandate's enforcement, approvals, and records.
- **Why now.** It is the only mode users can have before DEC-99 passes (DEC-103; product experience
  brief §2.2).
- **For.** Already accepted (E17-4); separates the value of control and evidence from the value of
  research, which #177 asks to measure.
- **Against.** Users must bring a strategy; weaker "autonomous" story; overlaps QuantConnect and rule
  automation.
- **Plan impact.** None; it is planned.
- **Experiment.** In discovery calls, show the same paper run in both modes and ask which one the
  person would use and pay for. **Kill.** No interviewee values the controls without the research.

### Option 14: Small systematic trading teams (#177)

- **Target.** Segment E. **Value.** Team approvals, separation of duties, and audit without building
  infrastructure.
- **For.** Matches planned team features (two approvers, SSO, roles).
- **Against.** No evidence gathered on this segment; different approval and buying processes (#177);
  QuantConnect and in-house tools compete.
- **Experiment.** Three to five of the ten discovery interviews with teams, with authorization.
  **Kill.** No team names a concrete gap Mandate fills that its current tools do not.

### Option 15: Customer-side deployment and private models (#177)

- **Target.** Segments D and E with data-control needs. **Value.** Strategy, credentials, and records
  stay on the customer's side.
- **For.** A vision principle; TradeAgentic's self-hosted design shows the pattern [TA1].
- **Against.** Installation and support cost; no design partner has asked for it yet.
- **Plan impact.** M11 hybrid as planned; no change until a paying partner asks (#177).
- **Experiment.** None beyond asking in discovery. **Kill.** No paying design partner requests it.

### Option 16: Mandate experiments, multi-variant shadow mode (founder, 2026-09-27)

- **Target.** Segments A, C, and D: owners who want to test strategy changes before risking money.
  **Value.** "Test up to three versions of your strategy against each other on the same market, with
  the hypothesis written down before the run, and promote the one you choose."
- **Shape.** A mandate (the owner's envelope) holds at most three variants: one live and the
  others in shadow. Each variant is its own owner-confirmed mandate version that differs only in
  strategy fields (signal models, weights, thresholds, cadence), so rule 11 holds. Shadow variants see
  the same market data and pass the same gate, evaluated against their own simulated account state;
  each keeps a simulated book, never consumes or holds the live account's buying power, day-trade
  budget, or reservations, and nothing is sent to the broker. Each variant's hypothesis and success criterion are journaled before it runs
  (pre-registration), so neither the user nor the platform can cherry-pick results afterwards.
- **Promotion.** Always an owner action that creates a new confirmed mandate version. No automatic
  winner-picking: v1 has no calibration, and the platform does not recommend trades.
- **Why now.** It extends the roadmap's Phase 3 shadow mode for new mandate versions (E15-4) from one
  candidate to a small set, on the journal, gate, and simulated execution that are already built or
  in review.
- **For.** It turns the pre-registered track record into a per-user habit. The hash chain makes the
  journal tamper-evident, and its stream heads are anchored externally with an RFC 3161 timestamp
  every 5 minutes and at each end of day ([journal spec §10](../specs/journal.md#10-anchoring)), so
  "we recorded it before it happened" is provable to within the anchor interval, and best-effort
  while timestamping is unavailable (E5-1 to E5-4).
  Stateless competitors would have to rebuild their core to match it, and the accumulated experiment
  history is a switching cost.
- **Against.** Model cost grows with the number of variants, hence the cap of three. Shadow fills are
  simulated and must be labelled as simulated, like paper, and any comparison between variants is
  labelled as hypothetical performance. It is scope growth.
- **Plan impact.** The v1 build is unchanged. Story E15-5 is scheduled right after the Phase 1 exit
  (M5 to M7, one autonomous agent on Alpaca paper), pulled forward from roadmap Phase 3.
- **Regulatory exposure.** Flag: comparisons of simulated results are hypothetical performance
  (questions 10 and 35); owner-chosen promotion keeps the platform from recommending a variant.
- **Revenue.** Part of the plan; model cost scales with variants.
- **Experiment (paper only, no build).** Run two or three paper mandates side by side on Alpaca paper
  and, with the founder's authorization, show the comparison in the ten discovery calls. Ask: "would
  you pay to test variants against each other before risking money?"
- **Kill.** Fewer than 4 of 10 discovery interviewees say they would pay to test variants before
  risking money. E15-5 then returns to Phase 3 with E15-4.

## 6. Ranking

Each criterion is scored 1 to 5, where 5 is best; for regulatory exposure, 5 means lowest exposure.
Scores are judgement from the evidence above, unweighted, and meant to order the discussion, not to
decide it.

| Rank | # | Option | Demand evidence | Reuse of built work | Time to first value on paper | Regulatory exposure | Defensibility | Revenue potential | Total |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1 | Owner's own agent, optional channel | 5 | 5 | 5 | 3 | 3 | 3 | **24** |
| 2 | 3 | Compliance evidence for small advisers | 3 | 4 | 3 | 4 | 4 | 4 | **22** |
| 2 | 6 | Conformance suite | 3 | 5 | 5 | 5 | 3 | 1 | **22** |
| 4 | 13 | Bring-your-own-strategy lead | 3 | 5 | 4 | 4 | 2 | 2 | **20** |
| 4 | 14 | Small systematic teams | 2 | 4 | 3 | 4 | 3 | 4 | **20** |
| 4 | 16 | Mandate experiments | 1 | 4 | 4 | 3 | 5 | 3 | **20** |
| 7 | 4 | Broker supervision layer | 3 | 3 | 1 | 3 | 4 | 5 | **19** |
| 7 | 5 | Open-core | 2 | 4 | 4 | 5 | 2 | 2 | **19** |
| 9 | 10 | Register first | 3 | 3 | 2 | 2 | 4 | 4 | **18** |
| 10 | 2 | Read-only monitor | 3 | 3 | 4 | 5 | 1 | 1 | **17** |
| 11 | 0 | Baseline, platform ideas first | 2 | 4 | 2 | 1 | 3 | 4 | **16** |
| 11 | 7 | Insurance and audit | 1 | 4 | 2 | 4 | 3 | 2 | **16** |
| 11 | 8 | Research league | 2 | 4 | 3 | 3 | 3 | 1 | **16** |
| 11 | 11 | Embedded for Alpaca fintechs | 2 | 3 | 2 | 3 | 3 | 3 | **16** |
| 15 | 9 | Crypto first | 3 | 3 | 3 | 1 | 2 | 3 | **15** |
| 15 | 15 | Customer-side deployment | 1 | 2 | 1 | 4 | 4 | 3 | **15** |
| 17 | 12 | Prediction markets | 2 | 1 | 1 | 2 | 1 | 2 | **9** |

The baseline scores low on time to first value and regulatory exposure because users see no research
until DEC-99 passes and counsel answers. That is a statement about go-to-market order, not about
whether DEC-97 is the right destination.

**After DEC-141.** The scores are unchanged, but the table no longer sets the order. The founder
decided that the complete product (option 0, with option 13 as its pre-DEC-99 mode) leads, and that
option 1 is an optional channel inside it rather than a competing lead. Option 1's high score now
reads as "a cheap, well-evidenced on-ramp", not "the lead product". The rest of the ranking stands.
Option 16 was scored when the founder added it; its demand score is 1 until discovery tests it.

## 7. Recommendation

The founder decided this on 2026-09-27 ([DEC-141](../project/04-decision-log.md#decisions)):

> we should support it optionally, but ours should be complete product, it is just that they can
> connect it to MCP to say claude and then use same API that we would have used for taking input
> from user

**The complete product leads (option 0, with option 13 as its mode before DEC-99).** The owner sets
the envelope; the platform's research agent (DEC-97) brings the ideas and reaches users on the
timetable DEC-99 and DEC-103 already set. Until then, users run bring-your-own-strategy.

**The owner's own agent is an optional channel and an adoption on-ramp (option 1, DEC-141).** It is
never the only or the main path: an owner who never connects an agent gets the complete product. An
owner who already runs an agent (for example Claude) can connect it through a Mandate MCP server that
exposes the owner-input API. It is not a separate proposer path, and it follows the rules below.

- Every request goes through the same order builder, autonomy rules, risk gate, account ledger, and
  journal as the owner's own input.
- The client cannot change the envelope. It may propose a mandate version, which only the human
  confirms, with step-up.
- ASK approvals go to the human, and a client cannot approve its own proposal.
- Owner-only privileges and the kill switch stay with the human.
- No broker credential crosses MCP. The client has its own scoped, revocable token.
- Every call is journaled with the client's identity.
- The work is story E10-6, pulled forward by the founder on 2026-09-27
  ([DEC-148](../project/04-decision-log.md#decisions)): "we can make it forward as long as we are
  saying it is not the only or main path". It is the first M8 story after the owner-input API, E9-1,
  E9-2, E9-4 (step-up), and E10-3 (mandate versions), ahead of M8's other Should stories, and it
  does not wait for M9's web app. Its earlier dependencies (the journal, the order builder, the gate,
  the executor and account ledger, and the tracer bullet E7-7) land before the Phase 1 gate.
  Milestone order does not change.

The evidence for option 1 (the Robinhood and Alpaca figures, the friction brokers name, and brokers
disclaiming supervision) is why the channel is worth having. It argues for an on-ramp that brings
people with their own agents into the complete product, not for replacing it. Research on
2026-09-27 is why it moves forward: brokers now open order entry to third-party agents while
disclaiming supervision. Per the broker's announcement, Robinhood's agentic trading (beta from
2026-05-27) "does not control, supervise, monitor, recommend, or audit these AI agents" [RH1]. Per
the broker's announcement as reported, Coinbase for Agents (2026-06-11) will add custom limits such
as maximum trade size and spend later [CB1]. Hobby projects add their own safety layers.

**Hedge A: option 3.** Run discovery in parallel; change no engineering until 2 of 5 name a budget
and a reachable custodian.

**Hedge B: option 6.** Nearly free given existing tests; it strengthens the trust claim of both the
product and the channel, and is the only credible opening to option 4 later.

**Keep as planned:** option 15 (M11), and option 0's forward-paper experiment, run now.

**Scheduled after the Phase 1 exit: option 16** (mandate experiments, story E15-5), with its paper
experiment run now ([DEC-145](../project/04-decision-log.md#decisions) rules out a loss guarantee).

**Deprioritize:** option 9 (CTA exposure, leverage), option 12 (out of scope), option 4 as a lead
motion (revisit after option 6 has outside users).

**Rules 4 and 11.** DEC-141 interprets these rules without changing them. A request from the owner's
agent is owner input: deterministic code still sizes and places every order, and the working universe
changes only as rule 11 allows.

### Positioning: harness and platform (DEC-149)

The founder decided on 2026-09-27 ([DEC-149](../project/04-decision-log.md#decisions)):

> I want it both to evolve along side each other, harness is for enterprise and platform runs
> through the harness. Retail would bring some money, real money would be in the enterprise, retail
> is hard to crack.

- **Two layers, two buyers.** The **harness** (the gate, the autonomy rules, the journal, the
  executor, the connectors, the conformance suite, and the MCP channel) is sold to enterprises:
  brokers, fintechs, and teams building agents. The retail **platform** is sold to owners.
- **The platform runs through the harness.** It has no private path around it: every platform order
  passes the same builder, autonomy rules, gate, account ledger, and journal an enterprise customer's
  would.
- **Enterprise is the expected main revenue; retail is the proving ground.** Retail brings some
  revenue and proves the harness on real owners' accounts. The founder expects retail to be hard to
  win.
- **The harness moat.** Generic harnesses and MCP gateways enforce stateless per-call rules; Mandate's
  harness is stateful and domain-aware, journals before acting, and reduces risk without approval
  ([harness engineering §6](11-harness-engineering.md#6-what-mandate-already-does-that-most-harnesses-dont)).
- **Open for discovery: the first enterprise customer.** Which of brokers, fintechs, or agent
  builders comes first is not decided. Customer outreach needs the founder's authorization
  ([DEC-79](../project/04-decision-log.md#decisions)).

DEC-149 amends DEC-141's positioning without reversing it: the complete product, the research agent
inside the envelope, and the optional MCP channel still hold. Milestone order does not change; the
enterprise harness stories are proposals in the
[backlog](../project/06-backlog-v1.md#enterprise-harness-proposed-dec-149).

## 8. Differentiators mapped to existing work

The five P0 differentiators from #177, each mapped to stories and PRs before any new story is
proposed.

| Differentiator | Existing stories and PRs | State on 2026-09-27 | Gap before it can be shown |
|---|---|---|---|
| Owner mandate independent of strategy logic | E6-3 risk gate: [#157](https://github.com/kunwarshivam/mandate/pull/157), [#160](https://github.com/kunwarshivam/mandate/pull/160) merged, [#176](https://github.com/kunwarshivam/mandate/pull/176) open; E6-2 builder tests [#175](https://github.com/kunwarshivam/mandate/pull/175) merged; E10-3 change classification (stream F) | Gate spine and limits merged; builder implementation next | Builder implementation; a revision that tries to raise size or broaden assets refused by the gate |
| Account-wide coordination | E7-5 account ledger and one agent per instrument (RC-17); reservations (E6-6) | Specified, not built | E7-5 implementation; a two-agent scenario |
| Selective approvals with expiry and revalidation | E6-2 classification ([#175](https://github.com/kunwarshivam/mandate/pull/175)); E8-1 to E8-3 at M7 | Classification tests merged; approvals not started | M7 |
| Decision receipt | E5-1 to E5-4 journal and verifier (merged); the tracer's journal record (E7-7); E12-1 causal trace at M9 | Journal and `journal verify` merged | A narrow receipt over the journal: candidate to pull forward as part of E12-1, not a new story |
| Demonstrated recovery | E7-2, E7-3: tests [#152](https://github.com/kunwarshivam/mandate/pull/152), implementation [#174](https://github.com/kunwarshivam/mandate/pull/174), both open; runtime and kill switches [#151](https://github.com/kunwarshivam/mandate/pull/151) merged; tracer E7-7 ([#171](https://github.com/kunwarshivam/mandate/issues/171), brief [#173](https://github.com/kunwarshivam/mandate/pull/173) merged) | Executor in review; tracer brief merged | #152 and #174 merged, then the tracer's restart reconciliation |

P1 items from #177 map to accepted work too: the forward-paper evidence page to E17-8 (DEC-99,
DEC-111); portable mandates with a broker capability check to E7-6 (DEC-98); the daily owner briefing
to E12 and E8. None needs a new story yet.

### Defensible differentiators

The P0 differentiators above are what a user sees. What a competitor would find hardest to copy, in
the founder's order (2026-09-27):

1. **The provable, pre-registered track record.** Every decision is journaled before acting, and its
   existence at a time is provable to within the anchor interval (journal spec §10). A monthly breach
   record for each owner is proposed (story E12-4, not yet planned); publishing it beyond the owner
   needs counsel's answer and the founder (DEC-79).
2. **Mandate experiments** (option 16), which make the first one per-user.
3. **Distribution through brokers** as a supervision layer (option 4).
4. **Owning the conformance standard:** the journal spec and verifier (options 5 and 6).
5. **Regulatory position** (option 10 and counsel's answers).

A guarantee against losses or breaches, paid by Mandate and insured or self-funded, is ruled out
([DEC-145](../project/04-decision-log.md#decisions)).

## 9. Broker plan

| Order | Broker | Environment | Why | Constraints |
|---|---|---|---|---|
| 1 | **Alpaca** | Paper (E7-7 tracer, M6); OAuth at M8 (E7-1) | Free paper on the same API as live; registered OAuth apps; DEC-23 | Paper does not model market impact, slippage, or dividends [AL4]; disclose on any evidence page. Live third-party trading needs Alpaca's approval [AL5]. Limit of 200 requests a minute per account [AL6] |
| 2 (paper demo) | **Tradier or Webull** | Tradier sandbox with delayed data [TR2]; Webull paperTrade OpenAPI [WB2] | The only way to show one mandate on two real broker APIs without live money | Proposed here, not decided. It adds a connector DEC-98 does not list, so it needs a backlog story and a decision row before code. Choose by a desk check of each API's terms and order semantics; Tradier has a partner program [TR1], Webull a larger user base [WB1] |
| 2 (per DEC-98) | **Robinhood** Agentic Trading | Fixtures and the simulated broker with Robinhood's rules (DEC-124) | Largest pool of agent users; DEC-98's second connector (E7-6, M8) | No paper environment documented [RH2]; any real connection needs the founder under DEC-79 and nothing live before counsel signs off (DEC-98, DEC-102, rule 8). The agent's token can read every Robinhood account [RH2]; E7-6 already requires using only the agentic account's data |

Label every demo artifact as a recorded fixture, a dry run, or paper execution. Nothing in this plan
authorizes live trading or changes DEC-98's connector order.

## 10. Demo plan

Tied to current work, in order. Each step proves engineering, not product value or investment skill.

1. **One order on Alpaca paper** through the real crates (E7-7, #171, brief #173), with one allowed
   order and one genuine gate refusal.
2. **The envelope holds:** a proposal above the mandate's size is clipped by the builder; a proposal
   that would breach a limit is denied by the gate with its reason code (#157, #160, #175).
3. **Two agents, one account:** concurrent proposals whose combined exposure exceeds the cap; the
   second is refused through reservations; an agent-scoped kill switch leaves the other agent's
   holdings untouched (E7-5, E6-5, #151, #176).
4. **Approval with expiry:** an ASK request expires to the safe default; an approved action whose
   price moved is revalidated and refused (E8-1 to E8-3, M7).
5. **Recovery:** a lost acknowledgment, a restart, and a partial fill end in reconciliation with no
   duplicate order (#152, #174, E7-7).
6. **Receipt:** from the fill back to its inputs, mandate version, gate result, intent, and broker
   response; export and `mandate-cli journal verify` (E5-4, E12-1).
7. **The optional channel (once E10-6 exists; a paper spike before then):** steps 2 to 6 repeated
   with requests from the owner's own agent, plus a request to widen the envelope that waits for the
   human's step-up, and an ASK the client cannot approve.
8. **Second broker:** the same mandate on a paper second broker, with capability differences
   explained, not silently translated.
9. **Forward-paper evidence:** research-agent results against pre-registered baselines, failures and
   revisions included (E17-8, DEC-99, DEC-111).

## 11. Discovery plan

Outreach of any kind needs the founder's authorization; this document authorizes none.

- **Ten interviews:** four owners already running an agent on Robinhood or Alpaca; two Public Agents
  users; two builders of trading agents; two small systematic teams. Add five adviser or
  emerging-manager interviews for hedge A.
- **Ask for the last concrete incident:** a workaround, a manual intervention, or a reason they
  refused to automate. Ask what they use today and what it costs.
- **Show** the same ordinary workflow and one failure scenario from the demo plan, in
  bring-your-own-strategy mode and through their own agent over the optional channel. Ask which evidence
  changes their willingness to run it unattended.
- **Recruit three design partners** for repeated paper use; record whether they return and whether
  requests converge.
- **Test willingness to pay** for a concrete package, after measuring model, data, and support cost.
- **Record disconfirming evidence** with the same care: Public or the broker already solves it; no
  coordination problem exists; no one pays; demand exists only for promised returns.

These are discovery targets, not proof of product-market fit.

## 12. What would change our mind

| If we observe | Then |
|---|---|
| Robinhood or Alpaca ship enforced per-agent limits, approvals, and exportable records | The channel's value shrinks; the complete product competes on ideas, portability, and verification; lean on option 3 |
| Fewer than 4 of 10 agent users report a real incident or fear, and none would connect their agent | Move E10-6 to the back of the backlog |
| The DEC-103 thin slice beats its pre-registered baselines net of costs | Platform ideas are a real differentiator, as the lead assumes; consider option 10 |
| The thin slice fails its baselines | Keep ideation gated; the product rests on control and evidence (options 13, 3, 6), with the channel as an on-ramp; the founder may revisit what leads |
| Counsel says the optional channel carries different exposure from owner input | Change the channel's terms or scope with counsel before E10-6 ships |
| Counsel says a registration route is cheap and fast | Option 10 rises |
| Adviser interviews show budget and reachable custodians | Promote option 3 to co-primary; pull M8 roles and M11 hybrid forward |
| A widely reported loss caused by a retail trading agent | Demand for options 1, 4, and 6 rises; accelerate the conformance suite |
| The SEC or FINRA proposes rules on agents in customer accounts | Re-map controls to the rule text; option 4 likely rises |
| Robinhood forbids one platform acting for many customers (OD-12) | The product and the channel run on Alpaca and paper brokers only; retail reach shrinks |

## 13. Corrections the fact-check forced

The research behind this document was fact-checked claim by claim (67 claims: 49 verified, 13 partly
verified, 2 not found at source, 3 contradicted). The corrections applied here and in the landscape:

- Robinhood's figure is "nearly 100 thousand ... to date" from the filing of 2026-07-29, not "over
  100 thousand" or "by late June"; the call was on 2026-07-29, not 2026-08-07, the transcript's date.
- Public's changelog has 21 entries, not 22. Its 2026-03-31 release mentions bonds only in company boilerplate, not for Agents.
  The prediction-market release says "available to all Public members".
- The House letter had eight signatories, not seven.
- Alpaca's API limit is per account, not per key, on a page dated December 2022. Its July round is
  $135 million equity within $435 million of financing including debt; the January Series D of
  $150 million is separate.
- Composer's current prices are $0, $10, and $32 a month billed yearly. "$40 as Composer by SoFi"
  came from an expired promotion. "Over $215 million of automated trades a day" is not on any SoFi
  or Composer page and is dropped.
- Surmount's Pro plan is $300 a year, not $360, and internal accounts carry a 1% management fee.
  The claim that flat subscriptions are the norm among idea-originating automated advisers is
  withdrawn.
- Autopilot's $750 million is its founder's claim; the Form ADV figure is $462 million across 132,559
  accounts.
- Alpha Arena's "most flagships lost 40–60% in Season 1" is not in the cited source and is dropped.
- The Investing.com 4.5% and 6.4% figures are cited to its blog, not the news article. The FINRA
  Foundation's 34% adult-ownership figure is cited to the white paper.
- The $10,000 notional and 1,000-share defaults belong to Webull's local MCP server, not
  TradeStation, whose $10,000 is a minimum balance.
- Scalar Field's Robinhood page does not say live-only or market-orders-only.
- The date Robinhood's rollout completed is Virtuals' claim, not Robinhood's.

## Sources

Keys not listed here are in the [competitive landscape's source list](03-competitive-landscape.md#sources).

| Key | Source | Date |
|---|---|---|
| IN1 | [Investing.com: nearly two-thirds of retail investors use AI](https://www.investing.com/news/stock-market-news/survey-nearly-twothirds-of-retail-investors-use-ai-to-inform-market-decisions-4598846) | 2026-04 |
| IN2 | [Investing.com blog: how retail investors are using AI in 2026](https://www.investing.com/blog/how-retail-investors-are-using-ai-in-2026-339) | 2026 |
| BE1 | [Betterment Retail Investor Survey 2025 (PDF)](https://www.betterment.com/hubfs/PDFs/b4c/Betterment%20Retail%20Survey%202025.pdf) | 2025 |
| FF1 | [FINRA Foundation investor survey white paper (PDF)](https://www.finrafoundation.org/sites/finrafoundation/files/2025-11/NFCS_Investor_Survey_Report_White_Paper.pdf) | 2025-11 |
| FF2 | [FINRA: new FINRA Foundation research on shifting investor behaviors](https://www.finra.org/media-center/newsreleases/2025/new-finra-foundation-research-examines-shifting-investor-behaviors) | 2025-12-04 |
| IA1 | [Investment Adviser Association: 2026 industry snapshot (PDF)](https://www.investmentadviser.org/wp-content/uploads/2026/06/Snapshot-2026.pdf) | 2026-06 |
| IK1 | [Interactive Brokers 10-K for 2025](https://www.sec.gov/Archives/edgar/data/1381197/000138119726000062/ibkr-20251231.htm) | FY2025 |
| FR1 | [FINRA 2026 Annual Regulatory Oversight Report: GenAI](https://www.finra.org/rules-guidance/guidance/reports/2026-finra-annual-regulatory-oversight-report/gen-ai) | 2025-12-09 |
| FR2 | [FINRA Regulatory Notice 15-09](https://www.finra.org/rules-guidance/notices/15-09) | 2015-03-26 |
| HL1 | [Letter from House Financial Services members to the SEC on agentic trading (PDF)](https://foster.house.gov/sites/evo-subsites/foster-evo.house.gov/files/evo-media-document/foster_sherman-request-for-information-re-agentic-trading-6.23.2026.pdf) | 2026-06-23 |
| SE1 | [SEC Division of Examinations 2026 priorities (PDF)](https://www.sec.gov/files/2026-exam-priorities.pdf) | fiscal 2026 |
| SE2 | [SEC Chair remarks to the Investor Advisory Committee](https://www.sec.gov/newsroom/speeches-statements/atkins-remarks-iac-091026) | 2026-09-10 |
| SE3 | [SEC withdrawal notice (PDF)](https://www.sec.gov/files/rules/final/2025/33-11377.pdf) | 2025-06-12 |
| SE4 | [SEC press release 2024-36](https://www.sec.gov/newsroom/press-releases/2024-36) | 2024-03-18 |
| SE5 | [SEC press release 2013-222](https://www.sec.gov/newsroom/press-releases/2013-222) | 2013-10-16 |
| SE6 | [SEC press release 2024-42](https://www.sec.gov/newsroom/press-releases/2024-42) | 2024-03-27 |
| CF1 | [17 CFR 4.14 (eCFR)](https://www.ecfr.gov/current/title-17/chapter-I/part-4/subpart-A/section-4.14) | accessed |
| CB2 | [Coinbase: perpetual futures have arrived in the US](https://www.coinbase.com/blog/perpetual-futures-have-arrived-in-the-us) | 2025 |
