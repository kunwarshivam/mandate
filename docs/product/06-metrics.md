# Product Metrics

| | |
|---|---|
| **Owner** | Product |
| **Status** | Draft v0.1 |

## North star

**Live agent-days under mandate:** the number of days, summed across all agents, that an
agent ran live (real capital) with zero mandate violations.

Why: it captures the core promise. It only grows when users trust agents enough to run them
live, keep them running, and the platform keeps them inside their mandates.

## Input metrics

| Metric | Definition | Why it matters |
|---|---|---|
| Activation | Workspaces with a connected account and a mandate compiled | Setup friction |
| Paper conversion | Mandates that complete a paper run ÷ mandates compiled | Mandate authoring and backtest quality |
| Live conversion | Agents promoted to live ÷ agents that completed paper | Trust |
| Autonomy rate | Decisions executed without approval ÷ all decisions (live agents) | Whether agents actually work alone |
| Escalation precision | Escalations the approver marks as warranted ÷ all escalations | Whether asking is selective, not noisy |
| Approval response time | Median time from notification to response | Whether escalation fits users' lives |
| Timeout rate | Approvals resolved by safe default ÷ all approvals | Channel and threshold fit |
| Agent retention | Live agents still running after 30 and 90 days | Durable value |
| Mandate compile accuracy | Compiled fields accepted without edits ÷ all compiled fields | Authoring quality |

## Guardrail metrics (must stay at target)

| Metric | Target |
|---|---|
| Orders outside mandate | 0 |
| Duplicate orders | 0 |
| Unreconciled positions after recovery | 0 |
| Sensitive content in notification payloads | 0 |
| Kill-switch latency | Within one decision cycle |
| Journal chain verification failures | 0 |

## Outcome metrics (reported, not promised)

Reported to users per agent, never marketed as expected returns:

- Return, volatility, maximum drawdown, Sharpe, versus a buy-and-hold baseline.
- Per-signal-model hit rate and calibration measurement (reporting only).

## Business metrics

| Metric | Definition |
|---|---|
| Paying organizations | Organizations on a paid plan |
| Net revenue retention | Revenue from a cohort now ÷ its starting revenue |
| Hybrid / on-prem share | Organizations using hybrid or on-prem ÷ paying organizations |
| Gross margin | Including model and data pass-through costs |

## Instrumentation

Product analytics come from counts and IDs only; in hybrid mode no content leaves the
customer's site.

| Event | Properties |
|---|---|
| `workspace_created` | org_id, mode |
| `connection_added` / `connection_rejected` | venue, reason (for rejections) |
| `mandate_compiled` / `mandate_edited` | fields_inferred, fields_edited |
| `backtest_completed` / `paper_started` / `paper_completed` | agent_id, duration |
| `agent_live` / `agent_paused` / `agent_stopped` | agent_id, reason |
| `decision_made` | agent_id, classification (auto/ask/deny) |
| `approval_requested` / `approval_responded` / `approval_timed_out` | agent_id, channel, latency, warranted (optional feedback) |
| `risk_rung_reached` / `kill_switch_used` | agent_id, level |
| `reconciliation_mismatch` | agent_id |
