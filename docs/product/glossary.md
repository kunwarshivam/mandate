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
| **Paper trading** | Trading against a live market with simulated or testnet funds |
| **Testnet** | An exchange's test environment with fake funds |
| **Shadow mode** | Running a new mandate version alongside the live one without real orders, to compare decisions |
| **Perpetual future (perp)** | A futures contract with no expiry, kept near spot price through funding payments |
| **Funding** | Periodic payments between long and short perpetual holders |
| **Slippage** | Difference between the expected and actual fill price |
| **Point-in-time data** | Data as it was known at a given moment, preventing look-ahead bias |
