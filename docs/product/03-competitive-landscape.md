# Competitive Landscape

| | |
|---|---|
| **Owner** | Product |
| **Status** | Draft v0.3, 2026-09-30. Adds the general-purpose agents section [ADR-0003](../adr/0003-earned-autonomy.md) sequenced. Replaces v0.2 of 2026-09-27, which replaced v0.1 after it made negative claims about competitors that no source supported ([issue #177](https://github.com/kunwarshivam/mandate/issues/177)) |
| **Method** | Public web pages only, read 2026-09-26 and 2026-09-27; the general-agents section's sources were read 2026-09-30. Every figure used here was re-checked against its source page. No accounts, sign-ups, connectors, broker tools, or orders |
| **Regulatory material** | Public commentary, not legal advice. Legal and compliance text is reserved for the founder and counsel ([DEC-79](../project/04-decision-log.md#decisions)) |
| **Related** | [Strategy options](10-strategy-options.md), [Vision](01-vision-and-strategy.md), [Compliance](08-compliance-and-regulatory.md), [ADR-0002](../adr/0002-autonomous-ideation-and-retail.md) |

## How to read this document

- **Documented** means the company's own page, help article, terms, filing, or press release says it.
  Documentation shows what a company says its product does. It does not show that the product works.
- **Vendor claim** marks a company's statement about its own traction or capability that no
  independent source confirms.
- **Reported** means press, a review site, or an aggregator says it.
- **Not documented** means we found no public statement either way. It is never evidence that a
  capability is missing. We say a company does not do something only when its own documents say so.
- Source keys in square brackets, such as [PU5], point to the [source list](#sources). Each entry
  there has its URL and its date.
- Mandate itself is pre-release. Its capabilities below are specified and partly built, and nothing
  has run outside paper trading. The matrix says which.

## Summary

1. **Every element of Mandate's pitch exists somewhere; we found no single product that documents
   all of them together.** Public documents plan approval and deterministic execution of an
   approved plan. Scalar Field documents isolated strategy books, idempotent execution, and
   reconciliation. TradeAgentic describes an owner envelope with software-originated ideas (vendor
   claim). Regent describes pre-execution mandates and verifiable audit (vendor claim, devnet).
   Brokers ship scoped accounts, previews, and caps. Positioning must be "the combination, shown
   working", not "the only one with controls".
2. **Brokers opened their accounts to agents in 2026 and state that they do not supervise them.**
   Robinhood reported "nearly 100 thousand customers" with Agentic Trading accounts and "over
   $100 million in AUC" as of 2026-07-29 [RH4]. Robinhood and Public both state that they do not
   "control, supervise, monitor, recommend, or audit" third-party agents [RH1] [PU11].
3. **Public is the most complete broker-native competitor.** Its Agents became generally available on
   2026-08-05 [PU7]. It documents plan approval, deterministic execution, buying-power checks, per-agent
   margin, pause and delete, notifications, and run logs. It also documents that two agents on the same
   asset run independently [PU5], that no confirmation is sought before each transaction once an agent
   is active [PU8], and that Agent backtesting is not yet available [PU6].
4. **The least-contested differentiators are account-wide coordination across agents, per-action
   approval with expiry and revalidation, and an exportable, verifiable decision record.** Each is
   contested by at least one vendor claim (Conviction, for example, claims human approval before live
   execution), and none is proven by Mandate yet.
5. **Idea origination inside an owner's envelope is a real difference and a regulatory cost.**
   Public, Robinhood, and Scalar Field document user-defined logic. QuantConnect's Mia and
   TradeAgentic describe originating ideas. Peers that run automated strategies in retail accounts
   either register as advisers (Composer, Quantbase for Surmount's internal accounts, Autopilot,
   Autonomous) or describe themselves as software or technology providers (TradeAgentic, Scalar
   Field, Surmount for self-directed connections). See [Strategy options](10-strategy-options.md).
6. **The general-purpose agents (Muse, Grok Bot, Dots) shipped scoped, owner-picked permissions
   and no money layer.** All three document approval machinery and none documents any brokerage
   or trading reach, while the brokers their plugins would reach state they do not supervise
   connected agents. That is the gap the money-layer plugin (DEC-183) bets on, and their
   "always allow" scopes are exactly the perpetual delegation ADR-0003 caps at 30 days.

## Segment map

| Segment | Members covered here | Relationship to Mandate |
|---|---|---|
| Agentic features inside brokers | Public Agents, Robinhood Agentic Trading, eToro Agent Portfolios, Composer by SoFi | Competitors for the retail user, and channels when they let outside agents in |
| Direct agent-trading products on the user's own account | Scalar Field, Conviction, TradeAgentic, NexusTrade, Coil, Regent (a control layer, not an end-user product) | Closest competitors |
| General-purpose agents with money ambitions | Meta Muse, SpaceXAI Grok Bot, OpenAI Dots | Not competitors today (no documented trading); the money-layer plugin (DEC-183) makes them a channel, and their permission scopes set the expectations ADR-0003's delegations answer |
| Strategy and automation platforms | QuantConnect (Mia, LEAN), Composer, Surmount, Autopilot, Option Alpha, TradersPost, Capitalise.ai, Autonomous | Substitutes for delegated execution; several are registered advisers |
| Broker channels and MCP servers | Alpaca, Interactive Brokers, Webull, Tradier, tastytrade, TradeStation, Kraken, Coinbase, Public MCP, Robinhood MCP | Channels and suppliers; each is also a substitute when a user connects their own agent directly |
| Infrastructure | Alpaca (API, paper, OAuth), NautilusTrader, LEAN, SnapTrade | Suppliers, or build-versus-buy alternatives |

## Broker-native agents

### Public Agents

**What it documents**

- A user describes an agent in plain language. The builder asks clarifying questions and shows the
  full plan before "Create & Activate" [PU2]. A catalog offers templates [PU7].
- "Once you approve a plan, the Agent runs that plan the same way every time. ... The conversation
  used to build that plan is not deterministic" [PU1].
- "An Agent does only what you told it to do" [PU1].
- Actions: place, edit, and cancel orders in stocks, options, and crypto; move money between Public
  accounts; manage watchlists; send alerts [PU2]. Multi-leg options and rolling since 2026-04-03
  [PU7]. Options orders above Public's maximum size are split, and "Each portion may be routed to a
  different execution venue" [PU9].
- Before an order, "the Agent checks your available buying power". If funds are short, it skips the
  run and notifies the user [PU3]. Agents respect option levels, crypto enablement, extended-hours
  approvals, and IRA restrictions [PU3].
- "If your account has margin investing enabled, Agents will default to using margin". Margin can be
  turned off per agent [PU3].
- Capital, per-asset stops, and frequency caps are written into each plan [PU10].
- Pause, edit, and delete take effect immediately [PU2]. Repeated system failures mark an agent
  "action required" instead of retrying (2026-07-06) [PU7].
- Run logs and an activity feed of "everything your Agent evaluates and acts on" [PU5] [PU10].
- "two Agents acting on the same asset will each run their own plan independently. If you want them
  to coordinate, build that logic into the plan" [PU5].
- Once an agent is active, Public "will not alert you in advance of any pending Transaction, nor will
  we await confirmation from you in advance of executing that Transaction", and "Transactions cannot
  be cancelled or reversed" [PU8].
- "Can I backtest my Agent? Not yet. Backtesting is on the roadmap but not currently available"
  (help article of 2026-06-09) [PU6]. None of the 21 changelog entries from 2026-03-30 to 2026-09-14
  adds it [PU7].
- Agents need no Premium plan and standard fees apply; there is no limit on the number of agents
  [PU6]. "Your personal account data is not used to train external models" [PU4].
- The terms bar "any automated or programmatic method to extract data or output from the Services"
  [PU8].
- Dates: preview on 2026-03-30; "out of preview and generally available" on 2026-08-05 (v1.1.0)
  [PU7]; App Store 5.5.1 on 2026-09-15: "AI Agents are now fully available to everyone" [PU13].
  Prediction markets through Kalshi on 2026-09-24: "Prediction Markets are now available to all
  Public members" [PU12].
- Public also runs a hosted MCP server for outside agents and states it "does not control,
  supervise, monitor, recommend, or audit any third-party AI agent" [PU11].
- The help center (2026-06-09) still describes a waitlist and web-only use. The changelog and App
  Store notes are newer; prefer them [PU7] [PU13].

**What it does not document**

- An account-level cap across all agents, or a "pause all" control.
- Export of run logs, plan-version history, or any integrity check on the logs.
- Handling of ambiguous submissions, idempotency, or crash recovery beyond "action required" [PU7].
- A paper or simulation mode for Agents.
- The model provider behind the builder and Research.
- Any count of agents or users.

**Overlap with Mandate.** Plain-language authoring, a plan preview, explicit activation,
deterministic execution of an approved plan, buying-power and eligibility checks, pause and
delete, notifications, and run logs are baseline expectations, not differentiators. Mandate's
candidate differences are specific: an owner envelope that binds every agent in the account,
coordination between agents through one account ledger, ASK approvals with expiry and
revalidation, an exportable hash-chained journal, forward-paper evidence, and ideas originated by
the platform. The last one is also a larger compliance surface.

### Robinhood Agentic Trading

**What it documents**

- Launched in beta on 2026-05-27, "with support for equities only" [RH1]. The support article now
  says: "You currently can use your agent to place long equities, options, and crypto orders";
  "Margin borrowing is not yet enabled for Agentic accounts" [RH3].
- A customer opens a dedicated agentic account and connects an MCP client they choose. The agent
  can read "All your Robinhood accounts, including your Robinhood account numbers", and trades only
  in the agentic account. "You can only open an agentic account and authenticate your agent on a
  desktop device" [RH2].
- The support article describes trading with or without the owner's approval of each trade [RH3].
- "Robinhood does not control, supervise, monitor, recommend, or audit these AI agents" [RH1].
- Scale: "To date, nearly 100 thousand customers have opened Agentic Trading accounts, with over
  $100 million in AUC" (Q2 2026 results, 2026-07-29) [RH4]. On the call that day, management said
  some users dislike "stitching together these 2 apps" and that models "sometimes ... will fight
  you" [RH5].
- Robinhood Strategies, its managed-portfolio product, had "over 300 thousand Funded Customers with
  nearly $2 billion" [RH4].

**What it does not document**

- A paper or sandbox environment for agentic accounts.
- Developer registration, terms for a hosted platform acting for many customers, rate limits, or
  token lifetime (OD-12 in the [decision log](../project/04-decision-log.md#open-decisions)).
- The date the rollout reached all eligible users. A report of 2026-09-07 attributes a completion
  date to Virtuals, not to Robinhood, and notes that Robinhood's materials do not establish it [RH6].

**Overlap with Mandate.** Robinhood is Mandate's second connector (DEC-98) and the largest pool of
people already running agents on a retail account. Its dedicated account is a natural capital cap.
Everything above that cap (limits, coordination, approvals, records) is left to the user's own
agent. Robinhood is also a substitute: a user's own agent connected directly, with no Mandate in
between.

### eToro Agent Portfolios

**What it documents.** Announced 2026-03-26 with a gradual rollout: a dedicated sub-portfolio with a
minimum of $200 and a scoped API key. Supported agents include Claude Code and Cursor; "Standard
chatbots like ChatGPT, Gemini, and Claude.ai" are not supported [ET1].

**Reported.** "eToro's Agent Portfolios are not offered to U.S. clients" [SB1].

**Overlap with Mandate.** Allocated capital per agent and bring-your-own-agent access are shared
ideas. Not a US channel today.

### Composer by SoFi

**What it documents.** Launched 2026-06-23 after SoFi's acquisition of Composer; users can search
over 2,000 community-built strategies and build strategies with AI [CO1]. Current pricing: Starter
$0, Advanced $10 a month, Pro $32 a month billed yearly ($384 a year) [CO2]. Composer's Form CRS
describes a registered investment adviser offering "limited, automated investment advisory
services" by subscription [CO3].

**What it does not document.** Account-level limits separate from a strategy's rules, per-action
approvals, or an exportable decision record.

**Overlap with Mandate.** Natural-language strategy building, backtests before activation, and
automated execution are established. Composer shows the registered-adviser, flat-subscription route
for software-delivered strategies.

## Direct agent-trading products

### Scalar Field (YC X25)

**What it documents**

- LLM research and backtesting, then an event-driven agent that trades "without another LLM call at
  every step" [SF7].
- Venues in its docs include Alpaca (paper and live through one OAuth flow, "strategy code validated
  on paper runs unchanged against live"), Robinhood, Public, Polymarket, Kalshi, Jupiter, and
  Hyperliquid [SF3] [SF4].
- For Robinhood: "US equities and ETFs (no options or crypto orders)", and "Avoid trading directly in
  the Robinhood app while strategies are active" [SF4].
- Isolated books: "A strategy cannot read sibling strategies' positions, cash, or trade history".
  Target-position execution is idempotent; "Only one pending order per symbol is allowed"; checks for
  insufficient cash and buying power [SF1].
- Reconciliation compares the aggregate of all strategies on a shared account with broker holdings,
  and freezes a strategy when holdings fall below a protected floor after a grace period [SF2].
- Loss limits, drawdown thresholds, approvals, a kill switch, and decision logs appear as
  recommendations in a blog guide, not as documented product features [SF5]. The guide tells the
  user to write the prompt "like an investment mandate" and says "A good agent is only as clear as
  the mandate behind it"; marketing says agents run "according to user-defined logic" [SF5] [SF9].
- Terms (updated 2026-08-17): "Scalar Field is a quantitative research platform that provides
  compute, data, and infrastructure for live trading"; it "does not offer financial advice"; trades
  are "at your direction"; "All fees are non-refundable" [SF8].
- AI disclosure (updated 2026-05-07): its AI "can create and manage automated trading agents that
  execute buy and sell orders on your behalf". The user can "Set allocation limits and configure
  risk parameters (e.g., maximum drawdown thresholds)", "Review and approve agent activation", and
  "pause, resume, or emergency-liquidate any active agent at any time" [SF16].
- The full docs describe a strategy lifecycle of Active, Frozen, and Paused, and
  `strategy.liquidate()` as a flatten of the strategy's positions. `venue.trade()` is "Direct
  execution — no approval workflow" [SF19].
- Scheduled runs: "Each run starts with a clean slate — no state is carried over from previous
  runs"; each run is capped at 3 minutes and 512 MB; a run triggered while the previous one is
  still active "is skipped" [SF17].
- Robinhood connection: "A desktop computer", "Google Chrome or Brave", and a bridge extension;
  margin: "Not supported — buying power reflects cash only" [SF18].
- Pricing is credit-metered: Free $0 with 10 credits, Pro $80 to $175 a month, Ultra $200 to $1,000
  a month, Enterprise custom. Strategy agents are charged "0.01 credits / second" of execution
  [SF14].
- Fees disagree between pages. The pricing page lists Polymarket and Jupiter DEX with a "1% fee on
  trades" [SF6]; the fees doc lists both at "0 bps (currently waived)" [SF15].
- Venues over time. A pricing snapshot of 2026-06-01 lists Alpaca, Polymarket, and Jupiter DEX
  under Trading, with "Public.com (coming soon)" and "Webull (coming soon)" [SF12]. The current
  pricing page lists Robinhood, Alpaca, Public.com, Polymarket, Jupiter DEX, and Hyperliquid, with
  "Webull (coming soon)" [SF6].
- Timeline. A Show HN of 2024-12-18 described "a GPT-based quantitative research tool" (2 points)
  [SF10]. First YC launch on 2025-05-08, "Reinventing the Trading Terminal, One Intelligent Agent
  at a Time" [SF6]. A homepage snapshot of 2026-03-05 still reads "Your ultimate AI research
  assistant designed to transform the way you explore trading ideas"; by 2026-06-25 the page
  title is "Scalar Field | AI Agentic Trading Desk" [SF11]. Second YC launch on 2026-07-16, "The
  agentic trading desk", with "approximately 300 ms event-to-trade latency" [SF7].
- YC company page: team size 3 [SF13].
- Vendor claim (first launch, 2025-05-08): "around 800 paying traders, over 34,000 signups, and
  $74,000 in monthly revenue" [SF6]. No later traction figure was found. That is an absence of
  evidence, not evidence of decline.

**Reported.** Trustpilot shows 3 reviews with a TrustScore of 3.4. A 1-star review of June 2026
says "lost a tonne of money on this janky platform"; a 5-star review of July 2026 says "From
robinhood to polymarket, can trade everything"; the third, from 2024, concerns hiring assessments
[SF20].

**What it does not document.** As of 2026-09-27, the full docs were not found to describe a drawdown
limit, a loss limit, per-trade approval, or an account-wide kill switch [SF19]. Drawdown thresholds,
a kill switch, and max-loss limits appear only as guidance in the AI disclosure and the blog guide
[SF16] [SF5]. Also not documented: order types and paper availability for its Robinhood path,
cross-strategy capital arbitration, and export or integrity of logs.

**Overlap with Mandate.** The closest shipped competitor. Multi-broker execution, isolation between
strategies, idempotent execution, reconciliation, activation approval, and pause and flatten are not
unique to Mandate. Its public positioning moved from a research assistant to an agentic trading desk
between March and June 2026 (inference, from the snapshots) [SF11]. Compare exact authority
boundaries and recovery behavior on identical scenarios, not feature lists.

### Conviction (YC S25)

**What it documents**

- "Describe a trading idea in natural language. Conviction tests it on historical data and deploys
  an AI agent that trades on your behalf", with paper trading before connecting a brokerage [CV1]
  [CV2]. Trading launch about 2026-09-16 [CV2].
- Brokers: "Robinhood, Alpaca, Public, E*Trade, and Trading 212 supported" [CV3].
- "every order must pass through deterministic guardrails before it reaches your broker. If a
  trade violates your rules, it is blocked" [CV3].
- Guardrails the site lists: "max position size, max portfolio exposure, max daily loss, allowed
  assets, stop-loss rules, take-profit rules, leverage restrictions, shorting restrictions,
  cooldowns, event blackouts, and human approval requirements" [CV3]. The user can "require human
  approval before live execution" [CV3].
- "Conviction is a research, simulation, and strategy-testing platform. It does not provide
  personalized financial advice or guarantee investment performance" [CV3].
- Origin. "Launch HN: Parachute (YC S25) – Guardrails for Clinical AI" was posted on 2025-08-19
  [CV4]. Conviction's YC page lists the Parachute launch, "Evaluate, Deploy, and Monitor Clinical
  AI", among its launches, and the Parachute company page now shows Conviction [CV1]. The trading
  product is a pivot by the same founders (inference) [CV1] [CV4].

All of the above are site claims. No public documentation, help center, or API reference was found.

**What it does not document.** How the guardrails are enforced, live-order mechanics, handling of
ambiguous submissions and restarts, pricing, records or their export, and its regulatory posture
beyond the disclaimer.

**Overlap with Mandate.** Research, backtest, paper, then deployment is already a competitor's
proposition. Deterministic guardrails outside the agent and human approval before live execution
are now claimed by a competitor too, so neither is by itself a differentiator. What remains
unverified publicly is enforcement, recovery, and evidence: whether the guardrails hold, what
happens after a failure, and whether a user can check the record. v0.1 of this document called
its risk layer "thin"; no source supports that.

### TradeAgentic (RLG, LLC)

**What it documents (all vendor claims).** A self-hosted desktop "autonomous trading desk" for US
equities, ETFs, options, and crypto. Several internal strategies share one capital pool, and
"Capital follows measured results". The owner sets scope, size, and stop conditions, with no
per-trade override by design. Protective stops rest at the broker; daily loss limits,
concentration caps, pre-flight refusal gates, and a kill switch are described as structural.
"Every refusal is recorded and scored later against what the market actually did". It describes
itself as a software licensor, not a registered adviser or broker-dealer [TA1] [TA2].

**What it does not document.** The team, supported brokers [TA1] ("additional broker adapters in
migration"), prices, customers, or any integrity check on its records.

**Overlap with Mandate.** The closest statement of Mandate's thesis (DEC-97: owner envelope,
software ideas, evidence-weighted allocation). It rejects per-trade approval outright, where
Mandate has AUTO, ASK, and DENY. No traction is published.

### Regent Protocol

**What it documents (vendor claims).** A "financial authorization and compliance layer for AI agents
that move money": mandate conditions "enforced before the action executes", allow, hold, or deny verdicts, a kill
switch, "Mandates fail closed" ("if the control layer is unreachable, the money action is
refused"), and audit batches anchored on
Solana: "Evidence verifies against published keys, so an auditor does not have to trust the
operator" [RG1]. Available on devnet only; "Independent audit and SOC 2 are on the roadmap" [RG1].
Terms are governed by the laws of Kazakhstan [RG3].

**What it does not document.** Broker connectors, customers, pricing, or team.

**Overlap with Mandate.** The only product found that describes a verifiable, operator-independent
audit trail. Its primitives are payment-shaped (amounts, destinations). A possible partner, format
peer, or competitor for the control-layer story.

### NexusTrade

**What it documents (vendor blog).** Automated agents launched 2025-10-11, with live trades
"manually confirmed" at the time [NX1]. A May 2026 post describes "Two spreads in the approval queue right now,
waiting for me to press execute", rejection of stale option chains, and per-position stops [NX2]. Brokers
named: Alpaca and TradeStation.

**What it does not document.** Whether fully autonomous live trading is now allowed; pricing; users.

### Coil

**What it documents (vendor page).** A rules engine run by the user's own agent through Robinhood's
MCP server. It ships disarmed, needs an account allowlist, uses a drawdown ladder against a
high-water mark and a kill switch, fails closed, and publishes its record with "losses
included" [CL1].

**Overlap with Mandate.** Small, but its safety vocabulary is close to Mandate's, and it packages
"your own agent plus Robinhood MCP plus limits", the substitute Mandate must beat.

### Others, not researched in depth

- **Autonomous (ATG):** an AI wealth manager run as a registered adviser with Apex custody,
  invite-only [AU1]; "emerged from stealth with $15 million in pre-seed funding" in January 2026
  [AU2]. Platform-originated ideas, but in its own accounts, not the user's existing broker.
- **Nof1:** ran Alpha Arena, where language models traded real money; raised $15 million in May 2026
  and plans a consumer platform [NF1]. In one US-equities season, "The portfolio as a
  whole lost about a third", and "a model finished in profit only six times" [NF2].
- **Instinct (YC W26)** and **Volaren (YC F26):** pre-launch; execution not documented [YC1] [YC2].
  An unrelated company also called Instinct raised a large round in 2026; do not conflate them.

## General-purpose agents with money ambitions

Not competitors today: none of the three documents a brokerage connection, order placement, or
any investing capability. They matter here for two reasons. Their permission models set the
expectations owners now bring to any autonomous agent, and
[ADR-0003](../adr/0003-earned-autonomy.md) packages Mandate's MCP server as the money layer for
exactly these agents (DEC-183), which makes them a channel if that bet is right — while the
client ceiling (DEC-185) keeps an order one of them asked for out of `auto`.

### Meta Muse

**What it documents**

- Launched 2026-09-08: "a secure, private personal AI agent that proactively helps with people's
  goals and suggests ideas", running on a dedicated Muse Secure VM with its own browser and
  working "on a person's behalf across the apps they use daily" [GA1].
- A separate Sentinel agent is the permission authority: "Nothing Muse does reaches the internet
  unless the Sentinel approves it, and it asks the person for permission when needed" [GA1].
- It "keeps working after people close the app, and comes back when something changes or when it
  needs approval, like before it sends an email or makes a purchase" [GA1].
- Approvals are scoped: "Allow once", "Allow for this task", "Allow for this site", "Always
  allow" (per Connector), or "Deny" [GA2]. The help centre tells owners they are "responsible for
  guiding it carefully and approving its actions" and to "review your permissions periodically"
  [GA2].
- Payments go through Link, whose agent wallet "generates a one-time-use card so your real card
  details stay hidden"; Muse is "the first AI agent covered by Link's purchase protections" [GA1].
- An activity log, an Upcoming list, and "a complete audit trail of everything it has done and
  plans to do" [GA1] [GA2].

**What it does not document**

- Any brokerage, trading, or investing capability. Link purchases are payments, not trades.
- Any export or integrity check on the audit trail.
- Any timeout default when the owner does not answer an approval.

**Overlap with Mandate.** The Muse/Sentinel split is the same shape as Mandate's gate-and-agent
split, and Muse's scoped approvals are the pattern ADR-0003's delegations answer, bounded the way
Mandate requires and Muse does not document: a delegation expires within 30 days, is suspended by
any sign of trouble, and is spent or expires, where "Always allow" [GA2] never re-asks.

### SpaceXAI Grok Bot

**What it documents**

- Launched in beta 2026-08-11: "your team of always-on agents. They have their own computer, work
  inside tools and apps like you do, and keep working 24/7" [GA3].
- Bots "finish jobs end to end, and only come back when something needs your approval" [GA3].
- The multi-bot pattern: "A chief of staff sits on top, with a specialist for each lane", and in
  group chats the bots "pass work, assign ownership, and only pull you in for judgment calls"
  [GA3].

**What it does not document**

- Any brokerage, trading, or payment capability.
- Any per-action approval or permission-scoping mechanism of its own. A third-party summary
  reports an "Auto Review" step that allows, asks about, or blocks each action; xAI's own launch
  page does not document one, and the summary was unreachable on re-read (2026-09-30), so we
  treat it as unverified.

**Overlap with Mandate.** The chief-of-staff-over-specialists shape is the desk metaphor
ADR-0003 part 4 adopts (the research analyst, the risk officer, the trader, the reviewer, the
chief of staff). Like Muse, Grok Bot documents no way to reach a brokerage account, so it would
arrive as a client of the money layer rather than a competitor to it.

### OpenAI Dots

**What it documents**

- Announced at DevDay 2026-09-29: agents that "run all the time on their own cloud computers and
  work on a user's behalf", on GPT-6 Astra, each with "its own cloud computer and browser",
  connecting "to more than 4,000 apps through plugins" [GA4].
- "Dots start with built-in rules on when to act alone and when to ask for approval. Users can
  set Custom Rules to allow, block or require approval for specific actions. An auto-review step
  checks actions that could affect accounts or share information" [GA4].
- Proactive background work is read-only: "a dot can only use connected apps in read-only mode,
  so it cannot send messages or change content" [GA4].
- "A monitoring system can pause or stop a dot if it detects a safety concern"; "some sensitive
  tasks, such as changing a password, always stay with the user" [GA4].
- The first dot is included in Pro and Business Premium plans; "Over time, we envision teams of
  dots working together on your behalf" [GA4].

**What it does not document**

- Any brokerage connection or trading capability.
- What the built-in rules are, or what the auto-review step checks.
- Any exportable or verifiable record of a dot's actions.

**Overlap with Mandate.** Dots' built-in rules plus owner-set Custom Rules (allow, require
approval, block) is the same three-valued shape as Mandate's AUTO, ASK, DENY with the owner
picking the rules. What no general agent documents is the part that matters for money: a limit
envelope that binds independently of the agent's own behaviour, a decision record that survives
the agent, and venue-side protection while the agent is down.

### Why this segment matters here

- All three permission models include an "always" scope with no expiry, and none documents a
  review date or a suspension on trouble. That is the perpetual delegation ADR-0003 holds back:
  a 30-day cap forces a second look with fresh evidence.
- The brokers these agents would reach do not supervise them: Robinhood states it does not
  "control, supervise, monitor, recommend, or audit these AI agents" [RH1]. A general agent
  holding a raw broker key trades with no gate at all.
- Model trading with no envelope and no gate has a measured downside. In Alpha Arena Season 1
  (2026-10-18 to 2026-11-03), six models each staked a real $10,000 on leveraged crypto
  perpetuals; two finished in profit and the other four finished down 42.01 to 58.74 percent
  [GA5]. The lesson ADR-0003 draws is the opposite of the format: there, sizing and limits were
  left to the model; here they never are.

## Strategy and automation platforms

### QuantConnect (Mia and LEAN)

**What it documents.** "Mia writes QuantConnect algorithm code, runs it, fixes what is broken, and runs it again", and
backtests it. "When the pipeline is empty, she reads recent financial news" to generate ideas. She
"deploys and monitors paper trading" and watches live performance against the backtest baseline; "The decision about live capital stays with you" [QC1]. LEAN's risk-management model
"seeks to manage risk on the PortfolioTarget collection it receives from the Portfolio Construction
model before the targets reach the Execution model" [QC2]. Live brokerages include Interactive
Brokers, Schwab, Alpaca, Tradier, Webull, TradeStation, tastytrade, Coinbase, and Kraken; Robinhood
is not listed [QC3]. "Since 2012, QuantConnect has deployed more than 375,000 live strategies"
(vendor claim) [QC4]. LEAN is open source [QC5].

**What it does not document.** Owner-level limits outside the algorithm, per-action approvals, or a
verifiable decision record.

**Overlap with Mandate.** v0.1 said LEAN had "no agent layer"; Mia is one. Mandate differs in owner
authority over runtime decisions and in the retail owner experience, not in "having an agent". It
may also be a source of strategies for the bring-your-own-strategy mode.

### Registered-adviser automation: Surmount, Autopilot

- **Surmount** connects to E*Trade, Alpaca, TradeStation, Coinbase, Kraken, and others; Surmount AI
  Inc. "is not a registered investment adviser" and "acts solely as a technology provider" for
  self-directed connections, while internal accounts run through Quantbase, LLC, a registered adviser
  [SU1]. Its own review page lists Core $50 a year, Plus $100 a year, and Pro $300 a year, with a
  "1% management fee" on internal (Quantbase-managed) accounts [SU2].
- **Autopilot** copies portfolios (politicians and investors) in the user's own brokerage account.
  InvestmentNews reports "about three million downloads, including 80,000 paid subscribers". Its
  founder's claim of $750 million in assets is a LinkedIn post; the latest Form ADV reported there
  (2025-04-29) lists "$461,989,873 in assets across 132,559 total accounts", and $400 million is tied
  to the Pelosi portfolio [AP1].

**Overlap with Mandate.** Delegated execution in the user's own account, sold by subscription, under
an adviser registration. The regulatory template closest to DEC-98's working assumption.

### Rule automation: Option Alpha, TradersPost, Capitalise.ai

- **Option Alpha:** options bots; "$99 /mo with annual billing or $149 monthly", with "50 bots" and a
  "$100k limit per bot" [OA1]. Per-bot capital caps are an established retail control.
- **TradersPost:** turns TradingView and TrendSpider alerts into broker orders, with paper accounts
  [TP1].
- **Capitalise.ai:** natural-language automation supplied through brokers [CA1].

## Broker channels and MCP servers

| Broker | What its public material documents for agents | Source |
|---|---|---|
| Alpaca | MCP server v2 "expands tool coverage from 43 to 61 endpoints"; a CLI "providing access to 108 trading functions"; free paper trading with random partial fills "10% of the time", which "does not account for" market impact, slippage, or dividends; third-party live trading needs Alpaca's approval, and commercial apps need written approval; API limit "200 requests per minute, per account" (page dated December 2022) | [AL1] [AL2] [AL4] [AL5] [AL6] |
| Interactive Brokers | Opened MCP access to any tool on 2026-07-28; the client "reviews each instruction and converts it to an order" before it is submitted | [IB1] |
| Webull | MCP, CLI, and connectors for ChatGPT, Claude, and Grok; "more than 28 million registered users"; paperTrade OpenAPI across "six asset classes", with "more than 204 million simulated orders". Reported: its local MCP server "ships with a $10,000 notional limit and a 1,000-share cap"; "Order preview is advised, not required" | [WB1] [WB2] [SB1] |
| Tradier | OAuth API, a paper sandbox, a hosted MCP server that places orders, and a partner program | [TR1] [TR2] |
| tastytrade | Reported: a dry-run token that "lasts 60 seconds" before confirmation | [SB1] |
| TradeStation | MCP since 2026-01-13. Reported: per-order confirmation, and "a $10,000 minimum balance" | [TS1] [SB1] |
| Kraken | Local MCP, paper by default; live orders need `acknowledged: true` unless `--allow-dangerous` | [KR1] |
| Coinbase | Agent trading through MCP and CLI from 2026-06-11; custom limits described as coming | [CB1] |
| Public, Robinhood | See above | [PU11] [RH1] |

**Overlap with Mandate.** Each broker is a channel and a supplier, and each lets a user connect an
agent directly without Mandate. Broker controls are account-level and generic; Mandate's gate is
per mandate. The two are complementary, but a broker could add owner-level limits and records
itself (RAID R-13).

## Infrastructure

- **Alpaca** raised $135 million led by Peak XV on 2026-07-16 ("The new financing totals $435
  million, inclusive of debt financing primarily from Payward ... and BMO") and reports monthly active
  API users "grew nearly 4x during the last six months" [AL3]. It raised a separate $150 million
  Series D, led by Drive Capital at a $1.15 billion valuation, in January 2026 [AL8]. Alpaca is
  Mandate's first connector and, through its agent tooling, a potential competitor.
- **NautilusTrader:** a Rust engine with the same code for backtest and live; the final 1.x release
  was on 2026-08-02 and v2 is at release candidate [NT1]. No agent layer was found in its materials.
- **SnapTrade:** multi-broker linking; its Robinhood page says Robinhood "does not offer the ability
  to place trades" through it [ST1].

## Capability matrix

Legend: **Yes** documented; **Partly** documented for part of the capability, scope in the cell;
**No** the company's own material says it does not, or its design excludes it;
**Not documented** no public statement found as of 2026-09-27. "(vc)" marks a vendor claim with no mechanism shown.

| Differentiator | Public Agents | Robinhood Agentic | Scalar Field | QuantConnect Mia + LEAN | Composer by SoFi | TradeAgentic | Regent | Mandate (status) |
|---|---|---|---|---|---|---|---|---|
| D1 Owner limits enforced outside strategy or agent logic | Partly: limits live in each plan; buying power, eligibility, per-agent margin outside it [PU3] [PU10] | Partly: the funded agentic account caps capital; Robinhood states it does not supervise agents [RH1] [RH2] | Partly: capital and buying power enforced per strategy; drawdown thresholds only in the AI disclosure and guidance [SF1] [SF5] [SF16] | Partly: LEAN risk model runs inside the algorithm [QC2] | Not documented [CO1] | Yes (vc) [TA1] | Yes (vc), payments, devnet [RG1] | Specified; gate code merged (#157, #160), flatten open (#176) [M1] |
| D2 Coordination across agents on one account | No: agents run independently [PU5] | Not documented [RH2] | Partly: isolated books, aggregate reconciliation [SF1] [SF2] | Not documented [QC1] | Not documented [CO1] | Yes (vc): one shared capital pool [TA2] | Not documented [RG1] | Specified (E7-5, RC-17); not built [M2] |
| D3 Per-action approval with expiry and revalidation | No: no confirmation before each transaction [PU8] | Partly: optional per-trade approval; expiry not documented [RH3] | Not documented: activation approval only; `venue.trade()` has no approval workflow [SF16] [SF19] | Not documented: live deployment decision stays with the user [QC1] | Not documented [CO1] | No: no per-trade override by design [TA2] | Partly (vc): hold verdict [RG1] | Classification tests merged (#175); approvals and drift revalidation are M7 (E8-1 to E8-3) [M3] |
| D4 Exportable, verifiable decision record | Partly: run logs and activity feed; export not documented; automated extraction barred [PU5] [PU8] | Not documented [RH1] | Not documented [SF1] | Not documented [QC1] | Not documented [CO1] | Partly (vc): refusals recorded and scored; integrity not documented [TA1] | Yes (vc): anchored, operator-independent [RG1] | Journal hash chain and verify CLI merged (E5-1, E5-4); decision view is M9 (E12) [M4] |
| D5 Documented handling of ambiguous submissions and restarts | Partly: repeated failures become "action required" [PU7] | Not documented [RH3] | Yes: idempotent execution, pending-order rule, reconcile and freeze [SF1] [SF2] | Not documented [QC1] | Not documented [CO1] | Partly (vc): resumes after reboot [TA1] | Not documented [RG1] | Specified (E7-2, E7-3); tests #152 and implementation #174 open [M5] |
| D6 Rehearsal before live (paper or forward evidence) | No backtest as of 2026-09-14; paper not documented [PU6] [PU7] | Not documented: no paper environment found [RH2] | Partly: paper on Alpaca; no evidence page [SF3] | Yes: backtest, paper, divergence monitoring [QC1] | Partly: backtest before activation [CO1] | Partly (vc): out-of-sample scoring [TA1] | Not applicable | Alpaca paper tracer (E7-7, #173) and forward-paper evaluation (DEC-99, E17-8) specified [M6] |
| D7 One mandate across brokers | No: Public accounts only [PU8] | No: Robinhood accounts only [RH2] | Partly: same code across venues and paper or live; no mandate object [SF3] | Partly: one algorithm across many brokerages [QC3] | Not documented [CO2] | Not documented: brokers undisclosed [TA1] | Not documented [RG1] | Alpaca first, Robinhood at M8 (E7-6); not built [M7] |
| D8 Ideas originated by the platform inside an owner envelope | No: "does only what you told it to do" [PU1] | No: the user's own agent; Robinhood does not recommend [RH1] | No: "user-defined logic" [SF9] | Yes: Mia generates ideas; the user decides live capital [QC1] | Partly: AI helps build rules the user activates [CO1] | Yes (vc) [TA2] | Not applicable | Accepted (DEC-97); users only after DEC-99 passes [M8] |
| D9 Runs on the customer's infrastructure | Not applicable (broker) | Not applicable (broker) | Partly: Enterprise private workspaces [SF6] | Yes: LEAN is open source [QC5] | Not documented [CO2] | Yes (vc): self-hosted desktop [TA1] | Not documented [RG1] | Planned (M11 hybrid) [M9] |

**Reading the matrix.** No column is all "Yes". Mandate's column is specification and partial code,
not shipped product, so no row supports a comparative claim until the demo in
[Strategy options](10-strategy-options.md#10-demo-plan) runs.

## Claims we must not make

1. "No existing product combines ..." without the date and the evidence. Every element exists in
   some product; say what we found, dated.
2. That Public has no risk controls, no approval, no logs, or non-deterministic execution. Its
   documents show all four [PU1] [PU2] [PU3] [PU5].
3. That our execution is "deterministic, unlike Public's".
4. That Public is waitlisted or web-only. It has been generally available since 2026-08-05 [PU7].
5. That Public cannot backtest, without a date. Say "Agent backtesting was documented as not yet
   available (help article of 2026-06-09), and no changelog entry through 2026-09-14 adds it".
6. That Public Agents trade bonds. The launch release of 2026-03-31 mentions bonds only in its
   company boilerplate ("from stocks and bonds to crypto and options") [PU14], and the help article of
   2026-06-09 lists corporate bond and treasury trading as not yet in the app [PU2].
7. That QuantConnect has "no agent layer", that Conviction has a "thin risk layer", that Scalar Field
   has no controls, or that any competitor has "shallow safety".
8. That Scalar Field's Robinhood path is live-only or market-orders-only. Its current venue page
   does not say either [SF4].
9. That TradeStation's MCP ships $10,000 notional and 1,000-share caps. Those defaults are
   reported for Webull's local server; TradeStation's $10,000 is a minimum balance [SB1].
10. That Composer by SoFi runs "over $215 million of automated trades a day". No SoFi or Composer
    page says so.
11. That Surmount charges no asset-based fee. Its internal accounts carry a 1% management fee [SU2].
12. That Autopilot manages $750 million, as a fact. It is the founder's claim; the Form ADV figure
    reported is $462 million [AP1].
13. That Robinhood had "over 100 thousand" agentic accounts, or had them "by late June". The filing
    says "nearly 100 thousand ... to date" on 2026-07-29 [RH4].
14. That Robinhood completed its rollout on a given date. That date is Virtuals' claim [RH6].
15. Anything about a competitor's model provider. Public's is undisclosed.
16. Any figure for Public Agents usage. None is published.
17. That Mandate has any capability in the matrix in production, or that Mandate is "safer" than a
    named product. Mandate runs on paper only and its differentiators are unproven.
18. That a broker endorses, partners with, or supervises Mandate.
19. That a mandate limit caps realized loss. Gaps, halts, and outages can exceed any threshold.

## Unknowns to verify by product trial

A trial needs an account with the vendor. Opening one, and anything touching an account that holds
real money, is reserved for the founder ([DEC-79](../project/04-decision-log.md#decisions)). No trial
may place an order (`AGENTS.md` rule 8). Where access is unavailable, record "not tested".

| Vendor | Question |
|---|---|
| Public | Is there an account-level cap across all agents on notional, loss, or order count? A "pause all" control? |
| Public | What does the run log show (inputs, data values, plan version, skipped runs)? Can it be exported, and how far back? |
| Public | Is a plan's prior version kept after an edit? |
| Public | What happens when two agents sell the same shares: rejection, overselling, or resizing? |
| Public | After a submission times out or its status is ambiguous, does the runtime duplicate, wait, or mark "action required"? |
| Public | Has Agent backtesting or any dry-run mode shipped since 2026-09-14? |
| Public | Does Research propose specific trades, and can one click turn an answer into an Agent? |
| Robinhood | Is there any paper or test path for agentic accounts? What order types and sessions can an agent use? |
| Robinhood | May a hosted platform act as the MCP client for many customers (OD-12)? Token lifetime and revocation? |
| Scalar Field | Does any limit span strategies (the sum of allocations against buying power)? Are the drawdown thresholds in its AI disclosure enforced, and where? Does emergency liquidation span all strategies on an account? |
| Scalar Field | Is an order intent persisted before submission, and what happens on a lost acknowledgment? |
| Scalar Field | Which order types and environments does its Robinhood path support today? |
| Conviction | Do its listed brokers take live orders today? Where are its guardrails enforced, and what happens on a rejected or ambiguous submission? What record can a user export? |
| TradeAgentic | Who is behind RLG, LLC; which brokers work; is its scoring inspectable before purchase? |
| NexusTrade | Is manual confirmation still required for live trades? |
| Webull | Can an agent submit without confirmation? |

## Sources

Accessed means the page carried no date and was read on 2026-09-26 or 2026-09-27.

| Key | Source | Date |
|---|---|---|
| PU1 | [Public: Agents: The Basics](https://help.public.com/en/articles/15435640-agents-the-basics) | updated 2026-06-09 |
| PU2 | [Public: Setting Up and Controlling Agents](https://help.public.com/en/articles/15435700-agents-setting-up-and-controlling-agents) | 2026-06-09 |
| PU3 | [Public: Money, Risk, and Rules](https://help.public.com/en/articles/15435718-agents-money-risk-and-rules) | 2026-06-09 |
| PU4 | [Public: Privacy and Security](https://help.public.com/en/articles/15435727-agents-privacy-and-security) | 2026-06-09 |
| PU5 | [Public: Errors, Edge Cases, and What-ifs](https://help.public.com/en/articles/15435733-agents-errors-edge-cases-and-what-ifs) | 2026-06-09 |
| PU6 | [Public: Sharing, Pricing, and Limits](https://help.public.com/en/articles/15435737-agents-sharing-pricing-and-limits) | 2026-06-09 |
| PU7 | [Public Agents changelog](https://public.com/agents/changelog) and its [data file](https://universal.hellopublic.com/managed-json/agents-version-log.json) | 21 entries, 2026-03-30 to 2026-09-14 |
| PU8 | [Public Agentic Brokerage Agreement (PDF)](https://public.com/disclosures/agenticterms) | created 2026-03-30 |
| PU9 | [Public Agentic Brokerage Disclosure (PDF)](https://public.com/disclosures/agenticdisclosures) | 2026-06-24 |
| PU10 | [Public AI Agents prompting guide](https://public.com/ai-agents/how-it-works) | accessed |
| PU11 | [Public MCP server](https://public.com/mcp-trading) | accessed |
| PU12 | [Public: AI Agents for Prediction Markets (press release)](https://www.prnewswire.com/news-releases/public-launches-ai-agents-for-prediction-markets-302888505.html) | 2026-09-24 |
| PU13 | [Public on the App Store (version history)](https://apps.apple.com/us/app/public-com-stocks-crypto/id1204112719) | 5.5.1, 2026-09-15 |
| PU14 | [Public: first brokerage to introduce AI Agents (press release)](https://www.prnewswire.com/news-releases/public-becomes-the-first-brokerage-to-introduce-ai-agents-for-your-portfolio-302729050.html) | 2026-03-31 |
| RH1 | [Robinhood newsroom: Robinhood is now open to agents](https://robinhood.com/us/en/newsroom/robinhood-is-now-open-to-agents/) | 2026-05-27 |
| RH2 | [Robinhood: Agentic Trading overview](https://robinhood.com/us/en/support/articles/agentic-trading-overview/) | accessed |
| RH3 | [Robinhood: Trading with your agent](https://robinhood.com/us/en/support/articles/trading-with-your-agent/) | accessed |
| RH4 | [Robinhood Q2 2026 results, SEC exhibit 99.1](https://www.sec.gov/Archives/edgar/data/0001783879/000178387926000113/q22026robinhoodexhibit991.htm) | 2026-07-29 |
| RH5 | [Robinhood Q2 2026 earnings call transcript](https://www.fool.com/earnings/call-transcripts/2026/08/07/robinhood-hood-q2-2026-earnings-call-transcript/) | call 2026-07-29; published 2026-08-07 |
| RH6 | [RuntimeWire: Virtuals says Robinhood completed agentic trading rollout](https://runtimewire.com/article/virtuals-says-robinhood-completed-agentic-trading-rollout) | 2026-09-07 |
| ET1 | [eToro: Agent Portfolios](https://www.etoro.com/news-and-analysis/etoro-updates/agent-portfolios-let-your-ai-agent-trade-for-you/) | 2026-03-26 |
| CO1 | [SoFi: Introducing Composer by SoFi](https://investors.sofi.com/news/news-details/2026/Introducing-Composer-by-SoFi-AI-Powered-Investing-From-Idea-to-Execution/default.aspx) | 2026-06-23 |
| CO2 | [Composer pricing](https://www.composer.trade/pricing) | accessed |
| CO3 | [Composer Form CRS (PDF)](https://reports.adviserinfo.sec.gov/crs/crs_311289.pdf) | March 2024 |
| SF1 | [Scalar Field docs: strategies](https://scalarfield.io/docs/trading/strategies.md) | accessed |
| SF2 | [Scalar Field docs: reconciliation](https://scalarfield.io/docs/trading/reconciliation.md) | accessed |
| SF3 | [Scalar Field docs: Alpaca venue](https://scalarfield.io/docs/trading/venues/alpaca.md) | accessed |
| SF4 | [Scalar Field docs: Robinhood venue](https://scalarfield.io/docs/trading/venues/robinhood.md) | accessed |
| SF5 | [Scalar Field blog: create your own AI trading agent](https://blogs.scalarfield.io/guides/create-your-own-ai-trading-agent) | undated |
| SF6 | [Scalar Field YC launch: reinventing the trading terminal](https://www.ycombinator.com/launches/NSw-scalar-field-reinventing-the-trading-terminal-one-intelligent-agent-at-a-time) and [pricing](https://scalarfield.io/pricing) | launch 2025-05-08; pricing accessed 2026-09-27 |
| SF7 | [Scalar Field YC launch: the agentic trading desk](https://www.ycombinator.com/launches/RZm-scalar-field-the-agentic-trading-desk) | 2026-07-16 |
| SF8 | [Scalar Field terms of use](https://scalarfield.io/terms-of-use) | updated 2026-08-17 |
| SF9 | [Scalar Field: what is agentic trading](https://scalarfield.io/ai-agentic-trading) | accessed |
| SF10 | [Show HN: Quant Trading Tool with LLM (code gen)](https://news.ycombinator.com/item?id=42446888) | 2024-12-18 |
| SF11 | Scalar Field home page in the Wayback Machine: [2026-03-05](https://web.archive.org/web/20260305145958/https://scalarfield.io/) and [2026-06-25](https://web.archive.org/web/20260625193549/https://scalarfield.io/) | snapshots 2026-03-05 and 2026-06-25 |
| SF12 | [Scalar Field pricing in the Wayback Machine](https://web.archive.org/web/20260601002037/https://scalarfield.io/pricing) | snapshot 2026-06-01 |
| SF13 | [Scalar Field, YC company page](https://www.ycombinator.com/companies/scalar-field) | accessed |
| SF14 | [Scalar Field docs: pricing](https://scalarfield.io/docs/usage/pricing) | accessed |
| SF15 | [Scalar Field docs: fees](https://scalarfield.io/docs/usage/fees) | accessed |
| SF16 | [Scalar Field AI disclosure](https://scalarfield.io/ai-disclosure) | updated 2026-05-07 |
| SF17 | [Scalar Field docs: execution limits](https://scalarfield.io/docs/usage/execution-limits) | accessed |
| SF18 | [Scalar Field blog: connect Robinhood](https://blogs.scalarfield.io/guides/connect-robinhood-on-scalar-field) | undated |
| SF19 | [Scalar Field full docs (llms-full.txt)](https://scalarfield.io/docs/llms-full.txt) | accessed 2026-09-27 |
| SF20 | [Trustpilot: Scalar Field reviews](https://www.trustpilot.com/review/scalarfield.io) (reported) | accessed 2026-09-27 |
| CV1 | [Conviction, YC company page](https://www.ycombinator.com/companies/conviction) | accessed |
| CV2 | [Conviction YC launch: trading desk in your pocket](https://www.ycombinator.com/launches/U6n-conviction-trading-desk-in-your-pocket) | about 2026-09-16 |
| CV3 | [Conviction home page](https://www.convictiontrade.ai/) (client-rendered; text read from its published script bundle) | accessed 2026-09-27 |
| CV4 | [Launch HN: Parachute (YC S25) – Guardrails for Clinical AI](https://news.ycombinator.com/item?id=44952246) | 2025-08-19 |
| TA1 | [TradeAgentic home page](https://tradeagentic.ai/) | accessed |
| TA2 | [TradeAgentic: agentic trading strategies](https://tradeagentic.ai/agentic-trading-strategies/) | updated 2026-09 |
| RG1 | [Regent Protocol](https://regentprotocol.org/) | accessed |
| RG2 | [Regent Solana programs (GitHub)](https://github.com/regent-protocol/regent-solana-programs) | accessed |
| RG3 | [Regent terms](https://regentprotocol.org/terms) | accessed |
| NX1 | [NexusTrade: automated agents, five days in](https://nexustrade.io/blog/i-launched-automated-ai-stock-trading-agents-5-days-ago-heres-what-i-learned-20251012) | 2025-10-16 |
| NX2 | [NexusTrade: agent-deployed options strategy](https://nexustrade.io/blog/agent-deployed-options-strategy-tuesday-20260505) | 2026-05-05 |
| CL1 | [Coil](https://coil.trade/) | accessed |
| AU1 | [Autonomous](https://becomeautonomous.com/) | accessed |
| AU2 | [TechStartups: ATG emerges from stealth with $15M](https://techstartups.com/2026/01/07/paperspace-founders-ai-startup-atg-emerges-from-stealth-with-15m-to-bring-autonomous-ai-financial-advisors-to-everyone/) | 2026-01-07 |
| NF1 | [Nof1 funding release](https://www.businesswire.com/news/home/20260515505589/en/SUI-Group-Co-Leads-$15-Million-Funding-Round-for-AI-Trading-Lab-Nof1-Makes-Strategic-Investment-in-Recursive-Superintelligence) (read through its Nasdaq syndication) | 2026-05-15 |
| NF2 | [Business Standard: AI bots auditioning for Wall Street are mostly losing money](https://www.business-standard.com/markets/news/ai-bots-auditioning-for-wall-street-trading-are-mostly-losing-money-126050701793_1.html) | 2026-05 |
| GA1 | [Meta newsroom: Introducing Muse](https://about.fb.com/news/2026/09/introducing-muse-personal-ai-agent/) | 2026-09-08, re-read 2026-09-30 |
| GA2 | [Meta Help Center: How Muse works with your guidance and approval](https://www.meta.com/help/artificial-intelligence/1385290430137537/) | accessed 2026-09-30 |
| GA3 | [xAI: Introducing Grok Bot](https://x.ai/news/introducing-grok-bot) | 2026-08-11, re-read 2026-09-30 |
| GA4 | [The Next Web: OpenAI launches dots, always-on AI agents with their own cloud computers](https://thenextweb.com/news/openai-dots-always-on-ai-agents-cloud-computers-devday) | 2026-09-29, re-read 2026-09-30 |
| GA5 | [TradeRank: Alpha Arena leaderboard, final results](https://www.traderank.ai/alpha-arena-leaderboard) (nof1's own figures, from an archived nof1.ai snapshot of 2026-08-06) | accessed 2026-09-30 |
| YC1 | [Instinct, YC company page](https://www.ycombinator.com/companies/instinct-xyz) | accessed |
| YC2 | [Volaren, YC company page](https://www.ycombinator.com/companies/volaren-inc) | accessed |
| QC1 | [QuantConnect: Mia](https://www.quantconnect.com/docs/v2/ai-assistance/predefined-agents/mia) | accessed |
| QC2 | [QuantConnect: risk management key concepts](https://www.quantconnect.com/docs/v2/writing-algorithms/algorithm-framework/risk-management/key-concepts) | accessed |
| QC3 | [QuantConnect: live brokerages](https://www.quantconnect.com/docs/v2/cloud-platform/live-trading/brokerages) | accessed |
| QC4 | [QuantConnect home page](https://www.quantconnect.com/) | accessed |
| QC5 | [LEAN on GitHub](https://github.com/QuantConnect/Lean) | accessed |
| SU1 | [Surmount: brokers](https://surmount.ai/brokers) | accessed |
| SU2 | [Surmount review (vendor-authored)](https://surmount.ai/surmount-review-2026-is-it-worth-it-for-automated-investing) | 2026-09-23 |
| AP1 | [InvestmentNews: Autopilot](https://www.investmentnews.com/fintech/autopilot-surges-to-750m-aum-touts-ria-growth-as-users-copy-pelosi-buffett-trades/260729) | 2025-05-30 |
| OA1 | [Option Alpha pricing](https://optionalpha.com/pricing) | accessed |
| TP1 | [TradersPost pricing](https://traderspost.io/pricing) | accessed |
| CA1 | [Capitalise.ai brokerage solutions](https://capitalise.ai/brokerage-solutions/) | accessed |
| AL1 | [Alpaca: MCP server v2](https://alpaca.markets/blog/alpaca-launches-mcp-server-v2/) | 2026-04-09 |
| AL2 | [Alpaca: CLI for the Trading API](https://alpaca.markets/blog/alpaca-introduces-cli-for-trading-api/) | 2026-04-23 |
| AL3 | [Alpaca raises $135 million](https://alpaca.markets/blog/alpaca-raises-135-million-to-scale-agent-first-brokerage-infrastructure-for-tokenized-markets-and-ai-native-financial-services/) | 2026-07-16 |
| AL4 | [Alpaca paper trading docs](https://docs.alpaca.markets/docs/paper-trading) | accessed |
| AL5 | [Alpaca Connect API](https://docs.alpaca.markets/us/docs/about-connect-api) | accessed |
| AL6 | [Alpaca: API usage limit](https://alpaca.markets/support/usage-limit-api-calls) | 2022-12 |
| AL7 | [Alpaca: sharp growth in API trading](https://alpaca.markets/blog/alpaca-reports-sharp-growth-in-api-trading-as-ai-reshapes-market-access/) | 2026-04-23 |
| AL8 | [SiliconANGLE: Alpaca raises $150M](https://siliconangle.com/2026/01/14/alpaca-raises-150m-grow-brokerage-account-management-platform/) | 2026-01-14 |
| IB1 | [Interactive Brokers opens AI connectivity to any MCP tool](https://www.nasdaq.com/press-release/interactive-brokers-opens-ai-connectivity-any-tool-built-mcp-standard-2026-07-28) | 2026-07-28 |
| WB1 | [Webull: connectors, CLI, and MCP](https://www.prnewswire.com/news-releases/webull-launches-chatgpt-claude--grok-connectors-cli-and-enhanced-mcp-to-expand-ai-powered-investing-tools-302840127.html) | 2026-08-04 |
| WB2 | [Webull: paperTrade OpenAPI](https://www.prnewswire.com/news-releases/webull-unveils-enhanced-paper-trading-experience-with-professional-grade-and-openapi-multi-asset-simulation-302830191.html) | 2026-07-21 |
| TR1 | [Tradier MCP server](https://tradier.com/individuals/mcp-server) | accessed |
| TR2 | [Tradier API: getting started](https://docs.tradier.com/docs/getting-started) | accessed |
| TS1 | [TradeStation MCP release](https://www.businesswire.com/news/home/20260112743966/en/TradeStation-Securities-Releases-MCP-Connection-Allowing-Users-to-Connect-Trading-Accounts-with-Third-Party-AI-Platforms) | 2026-01-13 |
| KR1 | [Kraken MCP docs](https://docs.kraken.com/home/mcp) | accessed |
| CB1 | [TechCrunch: Coinbase debuts MCP for agent trading](https://techcrunch.com/2026/06/11/coinbase-debuts-mcp-for-agent-trading/) | 2026-06-11 |
| SB1 | [StockBrokers.com: best brokers for AI trading agents](https://www.stockbrokers.com/guides/ai-agent-brokers) (reported) | 2026-09-16 |
| NT1 | [NautilusTrader releases](https://github.com/nautechsystems/nautilus_trader/releases) | accessed |
| ST1 | [SnapTrade: Robinhood integration](https://snaptrade.com/brokerage-integrations/robinhood-api) | accessed |
| M1 | Mandate: [E6-3](../project/06-backlog-v1.md), PRs [#157](https://github.com/kunwarshivam/mandate/pull/157), [#160](https://github.com/kunwarshivam/mandate/pull/160), [#176](https://github.com/kunwarshivam/mandate/pull/176) | 2026-09-27 |
| M2 | Mandate: E7-5 in the [backlog](../project/06-backlog-v1.md) | 2026-09-27 |
| M3 | Mandate: [#175](https://github.com/kunwarshivam/mandate/pull/175), E8-1 to E8-3 in the [backlog](../project/06-backlog-v1.md) | 2026-09-27 |
| M4 | Mandate: [journal spec](../specs/journal.md), E5-4 ([#108](https://github.com/kunwarshivam/mandate/pull/108)), E12 in the [backlog](../project/06-backlog-v1.md) | 2026-09-27 |
| M5 | Mandate: E7-2, E7-3, PRs [#152](https://github.com/kunwarshivam/mandate/pull/152), [#174](https://github.com/kunwarshivam/mandate/pull/174) | 2026-09-27 |
| M6 | Mandate: E7-7 ([#173](https://github.com/kunwarshivam/mandate/pull/173), [#171](https://github.com/kunwarshivam/mandate/issues/171)), [DEC-99](../project/04-decision-log.md#decisions), E17-8 | 2026-09-27 |
| M7 | Mandate: [DEC-98](../project/04-decision-log.md#decisions), E7-6 | 2026-09-27 |
| M8 | Mandate: [DEC-97](../project/04-decision-log.md#decisions), [DEC-103](../project/04-decision-log.md#decisions), [ADR-0002](../adr/0002-autonomous-ideation-and-retail.md) | 2026-09-27 |
| M9 | Mandate: [milestones](../project/02-milestones-and-wbs.md) (M11) | 2026-09-27 |

Not re-verified in this refresh, and so carrying no claims here: the open-source research frameworks
v0.1 listed (TradingAgents, AI Hedge Fund, RD-Agent with Qlib, FinRobot, FinRL), the research
copilots for funds, AI-native funds, and compliance vendors for trading firms. See this file's
history for v0.1.
