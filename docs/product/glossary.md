# Glossary

| Term | Meaning |
|---|---|
| **Owlhead** | The product's public name, at owlhead.ai. Anything public-facing uses it ([DEC-171](../project/04-decision-log.md#decisions)) |
| **Mandate** (product) | The internal name of the platform. The repository, the crates (`mandate-*`), and the working documents keep it; nothing is renamed ([DEC-171](../project/04-decision-log.md#decisions)) |
| **mandate** (object) | An agent's binding specification: goal, allowed asset classes and `max_instruments`, capital, connection, signal models, sizing, cadence, protection, risk limits, autonomy rules, notifications ([mandate spec](../specs/mandate.md)). Called the "agent spec" in the HLD |
| **Harness** | Mandate's enterprise layer: the gate, autonomy rules, journal, executor, connectors, conformance suite, and MCP channel, sold to brokers, fintechs, and teams building agents ([DEC-149](../project/04-decision-log.md#decisions)). Not the same as a test harness |
| **Platform (Mandate)** | Mandate's retail layer for owners; it runs through the harness and has no private path around it ([DEC-149](../project/04-decision-log.md#decisions)) |
| **Inner harness / outer harness** | The inner harness is the code around a model: loop, tools, context, sandbox, permissions, and hooks. The outer harness is what a team builds so agents do reliable work: the repository as system of record, mechanical invariants, evals, and feedback ([harness engineering §1](11-harness-engineering.md#1-what-harness-engineering-means)) |
| **Agent** | A versioned mandate plus the logic that executes it |
| **Deployment** | A running instance of one agent version |
| **Organization** | Billing and SSO entity; sets org-wide limits |
| **Workspace** | The tenant: members, connections, agents, data, encryption keys |
| **Connection** | A broker or exchange account, its stored credential, and the scopes granted to agents |
| **Signal model** | A registered component the user selects that produces an output (conviction, confidence, horizon, thesis): quant model, fast decision model, or LLM research. Never places orders. Formerly "advisor" |
| **Order builder** | Combines signal-model outputs with the user's fixed weights, sizes with the user-selected method, clips to limits, and proposes an action with a combined score. Formerly "decider" |
| **Combined score** | The order builder's weighted average of model confidences; an input to autonomy rules, not a probability of profit |
| **Envelope field** | A mandate field the user confirms (capital, goal, limits, autonomy rules, signal models and weights, sizing, protection, cadence, allowed asset classes, `max_instruments`); the compiler may propose a value, shown as proposed, but nothing activates unconfirmed ([DEC-97](../project/04-decision-log.md#decisions)). Formerly "judgment field" |
| **Strategy field** | The working universe and the theses behind it, produced by the research agent at runtime within the envelope, journaled, and outside the hashed mandate document ([mandate spec §3.2](../specs/mandate.md#32-strategy-fields-what-is-not-in-the-document-dec-97)) |
| **Working universe** | The instruments an agent may open or increase now: the pinned universe in bring-your-own-strategy mode, otherwise the instruments admitted by the research agent and not removed. Runtime state folded from `UniverseChanged`, bounded by `universe.max_instruments` ([mandate spec §2.3](../specs/mandate.md#23-the-working-universe-at-runtime-dec-97)) |
| **Monitor agent** | An agent that watches the instruments and conditions its confirmed mandate names and alerts the owner, and never opens a position: its autonomy denies every opening. Planned after v1 ([DEC-528](../project/decisions/DEC-528.md), E10-19) |
| **Watchlist** | A list of instruments the owner curates, stored in the workspace. It is not an envelope field, steers no agent unless the owner pins it into a confirmed mandate version, and is never read from or written to a broker. The platform authors, ranks and suggests no list ([DEC-528](../project/decisions/DEC-528.md)) |
| **Adoption** | Putting holdings the owner already has under an agent. Deferred: an agent's sub-ledger holds only what it bought ([DEC-46](../project/04-decision-log.md#decisions), [DEC-528](../project/decisions/DEC-528.md)) |
| **Bring-your-own-strategy** | The mode in which the owner pins the universe (`universe.pinned`), which turns the research agent off and gives the v0.5 behavior. Funds that supply their own strategies keep it |
| **Admission** | The ordered checks a thesis passes before its instrument enters the working universe: asset class, leveraged-ETP opt-in, the eligibility floor, the instrument-group claim, the source allowlist, corroboration, the lineage cap, and `max_instruments` ([mandate spec §8.5](../specs/mandate.md#85-admission-and-removal-dec-97-dec-101-dec-103)); the first order in an admitted instrument is then decided by the autonomy rules and `autonomy.admission` |
| **Thesis lineage** | A first thesis and its revisions, identified by `lineage_id`. A lineage is capped by `max_revisions_per_lineage` and is retired past the cap ([DEC-111](../project/04-decision-log.md#decisions)) |
| **Research agent** | The LLM-driven runtime component that turns allowlisted market data, news, filings, and the agent's own memory into theses and admits instruments into the working universe through the eligibility floor and the autonomy rules. One signal model with a user-confirmed weight ([ADR-0002](../adr/0002-autonomous-ideation-and-retail.md), [mandate spec §8.4](../specs/mandate.md#84-the-research-agent-dec-97-adr-0002)) |
| **Thesis** | A research-agent output naming one instrument, direction, horizon, evidence, corroboration, and invalidation conditions, with conviction and self-reported confidence; journaled as `ThesisProposed`. It expires at its horizon, when the instrument becomes exits-only and the thesis is scored ([DEC-118](../project/04-decision-log.md#decisions)) |
| **Thesis revision** | A thesis the research agent re-proposes after its predecessor failed on forward paper, naming the failure it addresses; journaled as `ThesisRevised` with its lineage, scored from zero, capped per lineage by `max_revisions_per_lineage` ([DEC-111](../project/04-decision-log.md#decisions)) |
| **Risk exit / owner exit / discretionary exit** | An exit from the risk engine (limits, automated flatten, trim, stop watchdog), exempt from all controls; an owner's close or kill switch, paced only by participation caps; an exit from the order builder or goal, paced by conduct controls but never denied |
| **Lifetime loss floor** | Equity level (contributed capital × (1 − `max_loss_from_allocation`)) at which an agent flattens and pauses permanently unless the owner loosens the mandate |
| **Autonomy policy** | Rules that classify each proposed action as AUTO, ASK, or DENY |
| **Risk gate** | Independent code on the order path that enforces limits regardless of agent logic |
| **Risk envelope** | The set of limits an agent must stay within |
| **Drawdown ladder** | Automatic de-risking steps at increasing losses (for example, halve sizes, exits only, flatten) |
| **Kill switch** | Immediate stop at agent, connection, workspace, organization, or global level |
| **Escalation** | Asking a human to approve an action |
| **Approval request** | The escalation record: proposed action, the rule that triggered it, evidence, risk impact, deadline, default. It never shows a platform-authored alternative trade ([mandate spec §6.4](../specs/mandate.md#64-approvals)) |
| **Delegation** | An owner-picked, bounded, expiring permission that turns an `ask` into `auto` inside the envelope. It never lifts a `deny` or a limit, and lasts at most 30 days (`autonomy.delegations`; [DEC-181](../project/04-decision-log.md#decisions), [ADR-0003](../adr/0003-earned-autonomy.md), [mandate spec §6.5](../specs/mandate.md#65-delegations-dec-181-adr-0003)) |
| **Safe default** | The action applied when an approval times out; never adds risk |
| **Drift re-validation** | Re-checking price and risk before executing an approved action |
| **Step-up authentication** | Extra authentication (passkey or biometrics) for sensitive actions |
| **Journal** | Append-only, hash-chained record of every step an agent takes |
| **Causal trace** | The chain from a fill back through order, approval, decision, opinions, and observations |
| **Global control plane** | Thin hosted service holding only non-sensitive metadata (directory, billing, fleet, notification relay, catalog) |
| **Workspace control services** | Mandate registry and compiler, policy, deployment, approvals, audit backend, connections; run next to the agents |
| **Data plane** | Agent runtimes, risk engine, execution gateway, journal, vault, model gateway |
| **Workspace deployment** | Workspace control services plus data plane, running in a managed cell or on the customer's site |
| **Cell** | A managed hosting unit containing many workspace deployments |
| **Managed / hybrid / on-prem** | Deployment modes: all ours; thin control plane ours and the rest the customer's; all the customer's |
| **Notification relay** | Delivers push notifications carrying only an opaque ID and generic text |
| **Fast decision model** | A model that returns typed answers with scores in tens to hundreds of milliseconds (for example, Laya, Jev) |
| **Calibration** | Adjusting a model's stated confidence so it matches how often it is actually right. Measured for reporting only in v1; never changes behavior ([DEC-47](../project/04-decision-log.md#decisions)) |
| **Paper trading** | Trading against a live market with simulated funds; for Alpaca, a separate paper environment with the same API as live |
| **Testnet / demo environment** | A venue's test environment with fake funds (for example, Kraken's derivatives demo) |
| **OAuth connection** | Connecting a user's brokerage account by authorization through the broker, granting scoped access without sharing API keys |
| **Cash account / settlement** | An account without margin; sale proceeds cannot be reused until the trade settles |
| **Pattern day trader (PDT) rule** | Former FINRA rule limiting day trades in margin accounts below $25,000 equity. Replaced by the intraday margin standard (SEC approval April 14, 2026; effective June 4, 2026; broker phase-in until October 20, 2027) |
| **Intraday margin standard** | FINRA's replacement for the PDT rule: accounts must hold margin equity matching their intraday exposure, regardless of day-trade count |
| **Day-trading regime** | Which of the two rule sets a broker applies to an account: `legacy_pdt` or `intraday_margin` |
| **Wash sale** | Selling at a loss and rebuying a substantially identical security within a window, which defers the tax loss |
| **Account ledger** | The single, serialized record of an account's buying power, reservations, orders, and positions that every agent on that broker account goes through |
| **Agent modes** | `normal`; `exits_only` (risk-reducing and protective orders only); `paused` (no new orders, protection stays); `stopped` (terminal, after the kill switch) |
| **Exits-only** | Agent mode in which only risk-reducing and protective orders are allowed |
| **Data profile** | Market-data rules per environment: `sip` (backtests, live equities), `iex` (paper), `crypto` |
| **Auction window** | Opening (09:28–09:30 ET) and closing (last minutes of the session) periods with no opening orders and no market orders |
| **Related-accounts group** | Owner-declared set of accounts across which self-trade prevention applies |
| **Abandoned** | Terminal state of an order intent that was never confirmed at the broker and failed its gate re-check or aged out |
| **OCO order** | One-cancels-other: a take-profit and a stop resting together; when one fills, the other is canceled |
| **Bracket order** | An entry order with attached take-profit and stop legs that activate when the entry fills |
| **Tranche model** | Each protected entry is its own bracket order; adding to a position is a new bracket |
| **Price collar** | Maximum distance of a limit price from the reference price (NBBO midpoint) |
| **Settlement calendar** | Days that are both NYSE trading days and Federal Reserve business days; used for T+1 settlement |
| **Cost basis** | Signed total amount paid (long) or received (short) for a position; average cost is derived from it |
| **Shadow ledger** | Model-side record of regulatory fees and dividends that Alpaca paper trading does not simulate |
| **Corporate actions** | Splits, dividends, and similar events that change share counts or cash and must be applied to positions and price history |
| **CFTC-regulated perpetual** | A perpetual future listed on a US exchange regulated by the Commodity Futures Trading Commission (for example, via Kraken Derivatives US) |
| **Shadow mode** | Running a new mandate version alongside the live one without real orders, to compare decisions |
| **Mandate experiment** | Up to three variants of a mandate run at once, one live and the others in shadow mode, each with its hypothesis journaled before it runs; only the owner promotes a variant, as a new confirmed mandate version |
| **Perpetual future (perp)** | A futures contract with no expiry, kept near spot price through funding payments |
| **Funding** | Periodic payments between long and short perpetual holders |
| **Slippage** | Difference between the expected and actual fill price |
| **Point-in-time data** | Data as it was known at a given moment, preventing look-ahead bias |
