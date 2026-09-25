# Product Roadmap

| | |
|---|---|
| **Owner** | Product |
| **Status** | Draft v0.1 |

Phases are ordered by dependency and gated by exit criteria, not dates. A phase starts when
the previous phase's exit criteria are met.

```mermaid
flowchart LR
    P0["Phase 0<br/>Core engine"] --> P1["Phase 1<br/>One autonomous agent<br/>on Alpaca paper"]
    P1 --> P2["Phase 2<br/>Platform v1<br/>design partners"]
    P2 --> P3["Phase 3<br/>Hybrid, fast models,<br/>learning loop"]
    P3 --> P4["Phase 4<br/>Equities, mobile,<br/>enterprise identity, on-prem"]
    P4 --> P5["Phase 5<br/>Retail, research lab,<br/>shared intelligence"]
```

## Now

### Phase 0: Core engine

Build the trading core from first principles, piece by piece.

- Market data: types, historical download from Alpaca (US stocks, ETFs, crypto), storage,
  quality checks, corporate actions (splits, dividends).
- Accounting for spot instruments: positions, cash, fees, corporate actions, settlement,
  realized and unrealized P&L. Perpetuals accounting (funding, margin) arrives with Kraken in
  Phase 3.
- Simulated execution: fills, slippage, fees.
- Backtest loop with a deliberately simple baseline strategy and evaluation metrics.
- Journal: append-only, hash-chained event log.

**Exit criteria:** a baseline strategy backtests reproducibly on a basket of US stocks and ETFs
and on BTC/USD spot, with
correct accounting (verified against hand-calculated cases) and a complete journal.

### Phase 1: One autonomous agent on Alpaca paper

- Agent runtime: perception, memory, quant signal models, order builder, autonomy policy.
- Risk gate, drawdown ladder, kill switch.
- Alpaca connector (paper), reconciliation, idempotent order intents, crash recovery.
- US market rules in the risk gate: day-trading regime (legacy or intraday margin), settlement, short-sale rules,
  market hours.
- Escalation v0: email and one chat channel, deadlines, safe defaults.
- Command-line control.

**Exit criteria:** an agent trades an Alpaca paper account unattended through a continuous soak,
survives forced restarts with no duplicate orders, and escalates and applies defaults
correctly.

## Next

### Phase 2: Platform v1 (design partners)

Everything in the [PRD](04-prd-v1.md) P0 list:

- Global control plane (thin) and workspace control services; managed mode.
- Web app: workspaces, connections, mandate authoring (plain language → mandate), backtest and
  paper, dashboard, approvals, audit explorer.
- OIDC SSO, roles, step-up auth, policy hierarchy.
- Private approval flow (opaque notifications).
- Billing and hybrid license keys.
- Hybrid installer (Helm / Docker Compose).

**Exit criteria:** PRD release criteria met; 5+ design partners on paper, 3+ live.

### Phase 3: Hybrid at scale, fast models, learning loop

- Hybrid deployments hardened (upgrades, health, fallback approval channels).
- Fast decision models (Laya in-process, Jev optional) with deadlines.
- Signal-model scorecards (reporting); user-selectable sizing methods; calibration only as a user-selected, versioned method (DEC-47).
- Shadow mode for new mandate versions.
- Kraken Derivatives US connector: CFTC-regulated crypto perpetuals, with perpetuals accounting
  (funding, margin, liquidation thresholds) and a funding/carry signal model.
- SMS and phone escalation; two-approver rule.

**Exit criteria:** escalation precision and autonomy-rate targets met across design partners;
at least one hybrid customer in production.

## Later

### Phase 4: Equities, mobile, enterprise

- Options on Alpaca.
- Interactive Brokers connector (CME futures, professional accounts); Coinbase US futures.
- Native mobile app for approvals.
- SAML and SCIM; separation of duties; fully on-prem / air-gapped packaging.
- SOC 2 readiness.

### Phase 5: Retail, research lab, shared intelligence

- Retail managed offering with presets and education (only after legal review).
- Research lab: agents propose and test new mandate variants; promotion requires approval.
- Shared intelligence plane: shared market data and public-event judgments for managed
  workspaces.
- WebAssembly plug-ins for custom logic.

## Explicitly not planned

- Trade recommendations or signals sold by the platform.
- Strategy marketplace or copy trading.
- Custody of funds.
- Pricing tied to trades, assets, or profits.
