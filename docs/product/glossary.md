# Glossary

| Term | Meaning |
|---|---|
| **Mandate** (product) | Working name of the platform |
| **mandate** (object) | An agent's binding specification: goal, done condition, instruments, connection, behavior, advisors, cadence, risk limits, autonomy rules, notifications. Called the "agent spec" in the HLD |
| **Agent** | A versioned mandate plus the logic that executes it |
| **Deployment** | A running instance of one agent version |
| **Organization** | Billing and SSO entity; sets org-wide limits |
| **Workspace** | The tenant: members, connections, agents, data, encryption keys |
| **Connection** | A broker or exchange account, its stored credential, and the scopes granted to agents |
| **Advisor** | A component that produces an opinion (signal, conviction, horizon, thesis): quant model, fast decision model, or LLM research |
| **Decider** | Combines advisor opinions, weighted by track record, into a proposed action with a confidence level |
| **Autonomy policy** | Rules that classify each proposed action as AUTO, ASK, or DENY |
| **Risk gate** | Independent code on the order path that enforces limits regardless of agent logic |
| **Risk envelope** | The set of limits an agent must stay within |
| **Drawdown ladder** | Automatic de-risking steps at increasing losses (for example, halve sizes, exits only, flatten) |
| **Kill switch** | Immediate stop at agent, connection, workspace, organization, or global level |
| **Escalation** | Asking a human to approve an action |
| **Approval request** | The escalation record: proposed action, alternatives, evidence, risk impact, deadline, default |
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
| **Fast decision model** | A model that returns typed answers with probabilities in tens to hundreds of milliseconds (for example, Laya, Jev) |
| **Calibration** | Adjusting a model's stated confidence so it matches how often it is actually right |
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
| **Perpetual future (perp)** | A futures contract with no expiry, kept near spot price through funding payments |
| **Funding** | Periodic payments between long and short perpetual holders |
| **Slippage** | Difference between the expected and actual fill price |
| **Point-in-time data** | Data as it was known at a given moment, preventing look-ahead bias |
