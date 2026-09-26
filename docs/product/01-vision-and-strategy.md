# Product Vision and Strategy

| | |
|---|---|
| **Product** | Mandate (working name) |
| **Owner** | Product |
| **Status** | Draft v0.1 |

## Vision

Anyone who trades, from a single systematic trader to a fund, can hand a clear mandate to an
autonomous agent and trust it to execute: fast when it is confident, asking when it is not,
and accountable for every decision it makes.

## Mission

Make autonomous trading agents **safe enough to trust with real capital** by combining three
things no existing tool combines: a binding mandate the agent cannot exceed, rule-based
escalation to a human only when it matters, and a complete, tamper-evident record of every
decision.

## The problem

Autonomous trading is becoming possible, but not trustworthy:

1. **Agents cannot be bounded.** Most agent frameworks let a language model decide and act.
   There is no enforceable contract between what the owner intended and what the agent does.
2. **Autonomy is all-or-nothing.** Tools either require a human to approve everything (not
   autonomous) or nothing (not safe). Nothing escalates *selectively* based on how sure the
   agent actually is.
3. **Decisions are not accountable.** When an agent trades, owners cannot answer "why did it do
   that?", and compliance teams cannot audit it.
4. **Research never reaches execution.** AI research tools stop at a memo or a trade idea; a
   human still turns ideas into orders and watches positions.
5. **Deployment is a blocker for serious users.** Firms will not send strategy, credentials, or
   trading intent to a vendor's cloud.

## Who it is for

| Segment | Stage | Why they buy |
|---|---|---|
| Professional individual traders in the US (systematic; stocks and crypto) | v1 design partners | Want agents that trade around the clock with hard limits and a phone ping when it matters |
| Emerging managers and small funds | v1 design partners | Need autonomy plus approvals, audit, and SSO without building infrastructure |
| Prop firms, funds, and trading desks | Later | Many agents, strict controls, on-prem deployment, compliance evidence |
| Retail investors (managed; Alpaca and Robinhood Agentic Trading) | v1, from the start ([DEC-98](../project/04-decision-log.md#decisions)); live after counsel signs off | An agent that brings its own ideas, bounded by an envelope they set; the retail profile, disclosures, and education |

See [Personas and journeys](02-personas-and-journeys.md).

## Product principles

1. **The mandate is the contract.** An agent can never act outside the goal, instruments, risk
   limits, and autonomy rules its owner approved.
2. **Reducing risk never needs approval; increasing risk beyond agreed limits always does.**
3. **Ask only when it matters.** Escalation is driven by the user's autonomy rules and explicit
   rules, not by habit. Every unnecessary ping erodes trust.
4. **Safe by default.** Timeouts, outages, and ambiguity resolve to "don't add risk".
5. **Everything is recorded.** Every observation, analysis, decision, approval, order, and fill.
6. **Deploy anywhere.** Managed, hybrid, or fully on-prem, from one installer. Strategy and
   credentials stay where the customer wants them.
7. **Users own the envelope; the agent brings the ideas.** Users set capital, limits, autonomy
   rules, and allowed asset classes and confirm every one; the platform's research agent originates
   theses and the agent trades on them within that envelope ([DEC-97](../project/04-decision-log.md#decisions),
   [ADR-0002](../adr/0002-autonomous-ideation-and-retail.md)). The platform does not hold funds or charge on trading outcomes.

## Positioning

**For** traders and trading firms **who** want to delegate trading to autonomous agents
**but** cannot trust today's agents with real capital, **Mandate** is an agent platform
**that** binds each agent to an enforceable mandate, escalates to a human only when it is
unsure, and records every decision. **Unlike** agent frameworks (TradingAgents, AI Hedge Fund)
or research copilots (Multiplier, finbar), Mandate runs agents in production against real
accounts, on the customer's infrastructure if required.

See [Competitive landscape](03-competitive-landscape.md).

## Strategy

### Where we play

- **Geography:** the United States first ([DEC-22](../project/04-decision-log.md#decisions)).
- **Asset classes and venues:** US stocks, ETFs, and crypto spot through Alpaca first (free
  paper trading on the same API as live; OAuth connections); retail equities through Robinhood
  Agentic Trading second; CFTC-regulated crypto perpetuals through Kraken Derivatives US third;
  options, Interactive Brokers, and Coinbase US futures later
  ([DEC-23](../project/04-decision-log.md#decisions), [DEC-98](../project/04-decision-log.md#decisions)).
- **Customers:** retail and professional individuals from the start ([DEC-98](../project/04-decision-log.md#decisions)), small funds
  alongside, larger firms next.
- **Deployment:** managed and hybrid first; fully on-prem / air-gapped after.

### How we win

| Lever | What it means |
|---|---|
| Trust | Mandates, rule-based escalation, hard risk gates, full audit. The reason a user lets an agent run unattended |
| Speed | Rust core; fast decision models (30–300 ms) for real-time judgments; LLMs never block trading |
| Deployment | The only agent platform that runs fully on the customer's side with the same product |
| Accountability | Causal decision trace from any fill back to the observations behind it |

### What we will not do

- Sell signals separately from agents, or run a marketplace of strategies (at least initially).
- Hold customer funds, or hold any permission that can move them (withdrawal or transfer).
- Charge per trade or as a percentage of assets or profits.
- Promise returns. The product makes agents safe and accountable; outcomes depend on the
  user's mandate and markets.

## Business model

Software subscription billed per organization: plan (seats and agents) plus usage
(agent-hours, model usage, data), with licenses for hybrid and on-prem deployments. See
[Pricing and packaging](07-pricing-and-packaging.md).

## Strategic bets and how we will know

| Bet | Evidence that it is right |
|---|---|
| Users will let agents run unattended if the mandate is enforceable and escalation is selective | Design partners run agents live; most decisions are autonomous; escalations are judged warranted |
| Selective escalation is a feature users value, not friction | Approval response times are short; users do not disable escalation |
| Firms will pay for deploy-anywhere and audit | Hybrid deployments requested by design partners; audit export used |
| Alpaca paper trading is the fastest path to real usage in the US | Fast onboarding; paper-to-live conversion |

Related: [Metrics](06-metrics.md), [Roadmap](05-roadmap.md), [HLD](../HLD.md).
