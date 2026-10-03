# Cost Model (v0.1, draft)

| | |
|---|---|
| **Owner** | Product; every number is the founder's (DEC-79: spending) |
| **Status** | Draft v0.1. A model with variables and labelled assumptions. It commits no spend and sets no price |
| **Decisions** | [DEC-443](../project/decisions/DEC-443.md) (all Proposed) |
| **Builds on** | [HLD §9](../HLD.md#9-intelligence-layer) (speed tiers), [§10](../HLD.md#10-billing) (billing components), [§11](../HLD.md#11-technology); [pricing and packaging](07-pricing-and-packaging.md); [inference spec §7](../specs/inference.md#7-cost-model-hooks) ([DEC-432](../project/decisions/DEC-432.md)); agent harness spec §7 (DEC-431, in review in #554); [data plane spec](../specs/data-plane.md) ([DEC-433](../project/decisions/DEC-433.md)); [infrastructure design §10](../design/infrastructure.md#10-cost-model) ([DEC-434](../project/decisions/DEC-434.md)); control plane design §3.5 (DEC-440, in review in #562) |
| **Closes** | Design gap 12 ([10-design-gaps](../project/10-design-gaps.md)) |

## Contents

1. [Purpose, scope, and how to use it](#1-purpose-scope-and-how-to-use-it)
2. [Cost drivers and formulas](#2-cost-drivers-and-formulas)
3. [Unit costs and three illustrative scenarios](#3-unit-costs-and-three-illustrative-scenarios)
4. [Sensitivity](#4-sensitivity)
5. [Against pricing](#5-against-pricing)
6. [Budget guardrails already decided](#6-budget-guardrails-already-decided)
7. [Open questions and Proposed decisions](#7-open-questions-and-proposed-decisions)

---

## 1. Purpose, scope, and how to use it

**Purpose.** Say what one agent and one workspace cost us to run, which few variables decide that
cost, and what that means for the plans in [07-pricing-and-packaging](07-pricing-and-packaging.md).
It is the input the founder needs to set prices, plan caps, and the monthly budgets of DEC-434
item 18.

**Scope.** Our cost of goods for the managed deployment: inference, data, compute, storage, egress,
observability, the global control plane, staging, and operations. Hybrid and on-prem appear only
where they change our cost (the customer runs the infrastructure; DEC-432 item 16 lets them bring
their own model keys). Out of scope: prices themselves (gap 11, `docs/design/billing.md`), the
customer's own costs (their broker account and any data subscription on it), and company overhead
outside running the product.

**What this document is not.**

- It is not a budget. Nothing here authorizes spend. Spend stays inside the caps already decided
  (§6) and the conservative path of DEC-434: no new paid service, paper only, the founder's host.
- It quotes no live prices. Every dollar figure is either stated in the repository (with its
  source) or a public list price written as **"assumption, verify before use"**. Those were not
  fetched for this draft and may be out of date.
- It is not a forecast of adoption. Scenario sizes are illustrations.

**How to use it.**

1. Read §2 for the formula of the line you care about.
2. Replace the assumptions in §3.1 with measured or quoted values; the scenario tables in §3 are
   simple products and sums of those values.
3. **Update this document whenever** a vendor or its price changes, a model is pinned or
   withdrawn, a cadence default changes (`behavior.research.interval_s`, `behavior.cadence`), a
   retrieval bound changes (agent harness §6.3), hosting is chosen (DEC-434 item 13, at M8), or the
   spike's paper runs report a measured cost per run (DEC-431 item 15's revisit). Record the
   change and its source in §3.1 and bump the version.
4. The first measured values replace the biggest assumptions first: tokens per research call
   (from the spike's `llm_call` records and, later, `ModelInvocationRecorded`), events and bytes
   per agent per day (from the journal), and process memory (from the Phase 1 host).

---

## 2. Cost drivers and formulas

Variable names follow [infrastructure §10.1](../design/infrastructure.md#101-per-agent-per-month)
where they overlap. Money is USD per month unless the name says otherwise. `D_m` is days in a
month (30.4); risk days are calendar days, since the risk day turns at midnight New York time
(inference spec §7.3), so this is the conservative count.

### 2.1 Inference

Every call goes through one aggregator (DEC-432 item 14) and is metered with its token counts and
cost (INF-6, [inference spec §7.1](../specs/inference.md#71-metering-record)).

**Research (slow tier).** One call per run (agent harness §6.2, DEC-431 item 6).

```
c_call      = (T_in · (1 − s_pre) · p_in + T_in · s_pre · p_cached + T_out · p_out) · (1 + f_agg)
c_call_max  = (T_in_max · p_in + T_out_max · p_out) · (1 + f_agg)          the reservation (§7.2)
R_day       = active_seconds_per_day / interval_s                           runs per agent per day
I_research  = min( R_day · c_call · (1 − h_cache),  cap_agent_day ) · D_m
```

| Variable | Meaning | Where it comes from |
|---|---|---|
| `T_in`, `T_out` | Input and output tokens of a typical research call | Measure from the spike (`llm_call`) |
| `T_in_max` | Largest prompt the retrieval plan allows | Harness bounds (DEC-431 item 7): 50 news items × 2,000 bytes, 20 filing excerpts × 4,000 bytes, 20 bars × up to 50 instruments, 50 theses of memory, the template |
| `T_out_max` | The entry's `max_output_tokens` | Pinned with the model (DEC-432 item 1) |
| `p_in`, `p_out`, `p_cached` | Price per token, from the versioned price table | Inference spec §7.2 |
| `s_pre` | Share of input tokens served from the provider's prompt-prefix cache (template and instructions only; INF-9 keeps workspace data out of shared prefixes) | Measure |
| `h_cache` | Exact-response cache hit rate (INF-11). Near zero for research, whose inputs change with every news item | Measure |
| `f_agg` | Aggregator fee on top of provider prices | Aggregator terms |
| `interval_s` | `behavior.research.interval_s`, an envelope field the owner confirms (rule 11), floored by policy `research_interval_s` | Mandate spec §8 |
| `cap_agent_day` | `behavior.research.cost_cap_usd_per_day` (DEC-120); $5 in the internal phase (DEC-431 item 15) | Envelope, tightened by policy |

The research cap binds when `R_day · c_call > cap_agent_day`. Because the gateway reserves
`c_call_max` before each call (DEC-432 item 4), the last call of a day may be refused while
actual spend is still below the cap by up to one `c_call_max`.

**Fast tier.** Hosted in v1 (DEC-432 item 13), called off the tick (DEC-431 item 18, Proposed and
in force). Excluded from the retail profile until counsel answers question 33.

```
c_fast  = (F_in · p_in_fast + F_out · p_out_fast) · (1 + f_agg)
I_fast  = E_day · k_fast · c_fast · (1 − h_fast) · D_m
E_day   = evaluations per day = active_seconds_per_day / cadence_interval_s (+ event-triggered)
```

`k_fast` is the number of fast models the mandate pins per evaluation; `h_fast` is the exact-cache
hit rate, which can be material here because typed inputs repeat when nothing has changed. Fast
and LLM signal models other than the research agent have no per-agent envelope cap yet; the
workspace quota bounds them (inference spec §7.3; backlog E15-9).

**Other calls.** The mandate compiler (goal-first drafting) and the drift and evaluation jobs
(E17-5, E17-8) are occasional; they sit in `I_other` per workspace until measured.

**Per-workspace bound.** `I_workspace ≤ cap_ws_day · D_m` (HLD §8 quota; $20 per internal
workspace per day, DEC-431 item 15).

### 2.2 Data

| Source | Our cost in v1 | Why |
|---|---|---|
| Market data (bars, quotes, trades) | 0 | Each workspace reads through its own Alpaca connection; no redistribution in v1 (DEC-433 items 3 and 15). A live account's SIP feed (DEC-35) is the user's subscription, not ours |
| News | 0 | Alpaca news per workspace (DEC-433 item 16) |
| Filings and XBRL facts | ≈ 0 | SEC EDGAR is free; cost is ingest compute and storage in the shared plane |
| Paid vendor | 0 until the DEC-99 evaluation passes | DEC-433 item 16 |

`C_data = C_shared_plane / W_total`, where `C_shared_plane` is the EDGAR ingest's compute and
storage. A future licensed feed adds `C_license` (fixed) plus any per-user display fee; both are
new decisions.

### 2.3 Compute

One runtime process per agent (DEC-08); per workspace an executor per broker account, a scheduler,
a replicated model gateway, a cold exporter, market data ingest, and control services
([infrastructure §3.1](../design/infrastructure.md#31-processes-in-a-workspace-deployment)).

```
C_agent_compute = (v_agent · C_cpu + m_runtime · C_mem) · h_run / u_pack
C_ws_compute    = Σ_processes (v_p · C_cpu + m_p · C_mem) · h_run / u_pack
```

`v` is the CPU request in vCPU, `m` the memory limit in GB, `h_run` about 730 hours, and `u_pack`
the packing density: the share of node capacity that requests actually fill (headroom, daemons,
the guaranteed CPU of executors and schedulers). Memory has no swap (infrastructure §3.3), so the
per-agent floor is its memory limit.

### 2.4 Storage

```
G_day            = (e_day · b_event + a_day · b_artifact) / 10^9          GB per agent per day
C_storage(month n) = G_day · D_m · n · r_regions · C_store_cold  +  G_hot · C_store_hot · r_standby
```

Records are write-once for six years after the later of creation and the close of what they
support ([trading spec §13](../specs/trading-domain.md#13-records-retention-dec-33), DEC-33), so
the cold term grows linearly with the months `n` an agent has run and never falls during
retention. `r_regions` is 2 before live capital (journal spec §6.2). The hot store holds only the
unverified tail (`G_hot`), doubled by the synchronous standby on live (`r_standby` = 2). Only
cited licensed text is kept (DEC-431 item 22), so prompt artifacts carry digests, not news bodies.
Backups add `C_backup · (WAL_day · 35 + fulls)` per workspace (infrastructure §10.2).

### 2.5 Egress, observability, control plane, staging, people

| Line | Formula | Note |
|---|---|---|
| Egress | `Σ GB_out · C_egress` | Cross-region replication of the cold store, usage reports, web traffic. Model and broker calls are mostly ingress |
| Observability | `C_obs_cell` fixed per cell, plus `n_series · C_series` if a hosted metrics service is chosen | Metrics 30 days full, 13 months downsampled; logs 14 days (Proposed, infrastructure §4.5) |
| Global control plane | `C_cp` fixed | Directory, licensing, metering ingest, relay, distribution (DEC-440, in review). Usage reports are hourly counts only |
| Managed cell fixed | `C_cell` = Kubernetes control plane + Postgres primary and synchronous standby + vault cluster (OpenBao, DEC-434 item 14) + load balancing | Shared by `W_cell` workspaces |
| Staging | `C_staging` fixed, from M8 (DEC-434 item 19) | A cell in miniature, paper only |
| Second region | Roughly doubles the database and storage lines for live (infrastructure §10.4) | Before live capital |
| People and operations | `C_ops` = on-call tooling + support time | The founder alone on call until design partners (DEC-434 item 20); founder time is not costed in v0.1 |

### 2.6 Totals

```
C_agent     = I_research + I_fast + C_agent_compute + C_storage
C_workspace = Σ_agents C_agent + C_ws_compute + I_other + C_data + C_backup
              + (C_cell + C_obs_cell) / W_cell + (C_cp + C_staging + C_ops) / W_total
```

---

## 3. Unit costs and three illustrative scenarios

### 3.1 Assumptions

Every value below is an **assumption** unless its source says otherwise. "Verify" means check the
vendor's current list price or measure before relying on it.

| Variable | Value used | Source |
|---|---|---|
| Research model | A mid-tier hosted model; the spike pins `anthropic/claude-sonnet-5` through OpenRouter | `python/research_spike/src/research_spike/config.py` |
| `p_in`, `p_out` | $3 and $15 per million tokens | Assumption, verify before use (a mid-tier list price; the spike reads the live price from the aggregator's catalogue) |
| `p_cached`, `s_pre` | Not used (`s_pre` = 0) | Assumption; measure |
| `f_agg` | 5.5% | Assumption, verify before use (aggregator credit fee) |
| `T_in`, `T_out` | 30,000 and 2,000 | Assumption; measure from the spike. 4 theses per output (DEC-431 item 7) |
| `T_in_max`, `T_out_max` | 68,000 and 4,000 | Derived: the harness bounds at about 4 bytes per token; `max_output_tokens` assumed |
| `c_call`, `c_call_max` | $0.127 and $0.279 | Computed from the rows above |
| Fast model `F_in`, `F_out`, prices | 1,500 and 100 tokens; $0.25 and $1.25 per million | Assumption, verify before use (a small hosted model). `c_fast` ≈ $0.00053 |
| `h_cache`, `h_fast` | 0 | Conservative |
| `C_cpu`, `C_mem` | $0.035 per vCPU-hour; $0.0045 per GB-hour | Assumption, verify before use (on-demand general-purpose cloud, no commitment discount) |
| `u_pack` | 0.6 | Assumption |
| Agent runtime | 0.1 vCPU, 0.25 GB | Assumption; measure on the Phase 1 host |
| Workspace processes, full | 0.95 vCPU, 1.7 GB in all (executor 0.25/0.25, scheduler 0.1/0.1, gateway 2 × 0.1/0.25, exporter 0.05/0.1, ingest 0.1/0.25, control 0.25/0.5) | Assumption |
| Workspace processes, lean | 0.3 vCPU, 0.6 GB (retail: shared control services, one gateway replica set per cell) | Assumption; needs a design decision (§7) |
| `e_day`, `b_event` | 1,500 events, 1.5 KB | Assumption; measure from the journal |
| `a_day`, `b_artifact` | 6 prompt and response pairs, 128 KB | Assumption |
| `G_day` | ≈ 3 MB per agent per day, ≈ 1.1 GB per year | Computed |
| `C_store_cold` | $0.023 per GB-month, two regions | Assumption, verify before use (standard object storage with object lock) |
| `C_cell` + `C_obs_cell` | $500 per month | Assumption: managed Kubernetes fee, two small Postgres nodes, three small vault nodes, load balancer |
| `C_staging` | $400 per month | Assumption |
| `C_cp` | $150 per month | Assumption |

### 3.2 Per-agent unit cost

| Line | Formula | At the assumptions, per agent-month |
|---|---|---|
| Research, daily (`interval_s` = 86,400) | `1 · c_call · D_m` | $3.85 |
| Research, every 4 hours (`interval_s` = 14,400) | `6 · c_call · D_m` | $23.09 |
| Research, hourly (`interval_s` = 3,600) | `24 · c_call · D_m` | $92.37 |
| Research at the internal cap | `cap_agent_day · D_m` = $5 × 30.4 | $152.00 (upper bound; about 38 typical calls a day are admissible under the reservation rule, so about $146 is reachable) |
| Fast tier, equities (`cadence_interval_s` = 300 over a 6.5-hour session, `E_day` = 78) | `78 · c_fast · D_m` | $1.25 |
| Fast tier, crypto (`E_day` = 288) | `288 · c_fast · D_m` | $4.62 |
| Compute | `(0.1 · C_cpu + 0.25 · C_mem) · 730 / 0.6` | $5.63 |
| Storage, end of year 1 | `1.1 · 1 · 2 · C_store_cold` | $0.05 |
| Storage, end of year 6 | `1.1 · 6 · 2 · C_store_cold` | $0.30 |

### 3.3 Per-workspace unit cost

| Line | Formula | Per workspace-month |
|---|---|---|
| Workspace processes, full | `C_ws_compute` with the full set | $49.76 |
| Workspace processes, lean | `C_ws_compute` with the lean set | $16.06 |
| Data | `C_data` | ≈ $0 (§2.2) |
| Fixed share | `(C_cell + C_obs_cell)/W_cell + (C_cp + C_staging)/W_total` | $1,050 / W |

### 3.4 Scenario A: internal paper phase (now to M8)

The founder's host (Phase 1, infrastructure §3.2), Alpaca paper, internal workspaces only, the
DEC-431 caps. Assume one internal workspace with 4 research agents at a 4-hour interval, plus the
spike's one call per UTC day.

| Line | Value | Basis |
|---|---|---|
| Inference, expected | $92 per month | 4 agents × 6 runs × $0.127 × 30.4 |
| Inference, spike | $4 per month | 1 call a day |
| Inference, hard ceiling | $608 per workspace-month | DEC-431: $20 per workspace per day × 30.4 (the four $5 agent caps sum to the same $20) |
| Compute, storage, data | ≈ $0 marginal | The founder's existing host; Alpaca paper and EDGAR are free (assumption, verify Alpaca's paper data terms) |
| Staging, control plane | $0 | Not before M8 (DEC-434 item 19) |
| **Total, expected** | **≈ $96 per month**, ceiling $608 per internal workspace | |

In this phase the only real cost is inference, and the daily caps already bound it.

### 3.5 Scenario B: one design-partner workspace (M8 to Phase 2, Team plan)

A managed cell shared by 5 design-partner workspaces, plus staging and the control plane. Each
workspace runs 5 paper agents with hourly research and one fast model on equities. Full process
set.

| Line | Per workspace-month | Share |
|---|---|---|
| Inference (5 × ($92.37 + $1.25)) | $468.09 | 62% |
| Agent compute (5 × $5.63) | $28.14 | 4% |
| Workspace processes, full | $49.76 | 7% |
| Storage and backups | ≈ $1.00 | 0% |
| Fixed share ($1,050 / 5) | $210.00 | 28% |
| **Total** | **$756.99** | |

Hourly research would need a design partner's `cost_cap_usd_per_day` of at least $3.19 per agent. The cap is sized by the gateway's admission test, not by the expected total: the last call of a day is admitted only if `spent + c_call_max ≤ cap` (inference spec §7.3), so `cap ≥ (R_day − 1) · c_call + c_call_max` = 23 × $0.1266 + $0.2785.

### 3.6 Scenario C: retail scale (Individual plan, after the Phase 2 gate)

1,000 Individual workspaces, 1.5 agents each, research every 4 hours, no fast tier (retail
profile), lean process set, fixed costs of $3,000 a month (two regions, staging, control plane;
assumption).

| Line | Per workspace-month | Share |
|---|---|---|
| Inference (1.5 × $23.09) | $34.64 | 56% |
| Agent compute (1.5 × $5.63) | $8.44 | 14% |
| Workspace processes, lean | $16.06 | 26% |
| Storage | $0.10 | 0% |
| Fixed share ($3,000 / 1,000) | $3.00 | 5% |
| **Total** | **$62.24** | |
| *Variant: daily research* | *$33.37 (inference $5.77)* | |

---

## 4. Sensitivity

Three variables dominate every scenario.

| Rank | Variable | What changing it does |
|---|---|---|
| 1 | **Research runs per day `R_day`** (set by `interval_s`, an owner-confirmed envelope field floored by policy) | Inference is linear in it. In scenario C, going from every 4 hours to daily cuts the workspace cost from $62.24 to $33.37 (−46%); hourly raises it to $166.16 (+167%). The research cap is the ceiling: no `interval_s` can spend more than `cap_agent_day` |
| 2 | **Cost per research call `c_call`** (model price × prompt size, plus the aggregator fee) | Also linear in inference. A model at one third of the assumed price takes scenario C to $39.15 (−37%); a prompt at the retrieval plan's maximum (`T_in_max`) nearly doubles inference, taking scenario C to $95.15 (+53%). Prompt size is the lever the platform controls, through the retrieval plan's bounds (DEC-431 item 7); the model is the owner's pin (rule 11) |
| 3 | **Workspace floor: always-on processes and fixed cost per workspace** (`C_ws_compute`, `W_cell`, `W_total`) | Decides the cost of a workspace with few or idle agents, and of the design-partner phase. Scenario B's fixed share is $210 at 5 workspaces and $35 at 30. In scenario C, the full process set instead of the lean one adds $33.70 (+54%) |

Lines that do not dominate at these assumptions: storage (cents per agent-month even after six
years, though it never falls), data (zero in v1), egress, and the fast tier on equities. Storage
becomes material only if `e_day` or prompt artifacts grow by two orders of magnitude, or if a
licensed news feed's text had to be retained, which DEC-431 item 22 avoids.

---

## 5. Against pricing

[07-pricing-and-packaging](07-pricing-and-packaging.md) sets plans and levers but no price points;
those wait on design-partner interviews and are the founder's. So this section gives the **price
floor** each scenario implies at a target gross margin, not a price.

```
GM          = (Revenue − C_workspace) / Revenue
P_floor(m)  = C_workspace / (1 − m)
```

| Scenario | Plan | Cost per workspace-month | Floor at GM 50% | GM 70% | GM 80% |
|---|---|---|---|---|---|
| A, internal | None (no revenue) | ≈ $96 | – | – | – |
| B, design partner | Team | $756.99 | $1,513.98 | $2,523.29 | $3,784.94 |
| C, retail, every 4 hours | Individual | $62.24 | $124.48 | $207.46 | $311.19 |
| C, retail, daily research | Individual | $33.37 | $66.75 | $111.25 | $166.87 |
| Hybrid or on-prem | Enterprise | Support and releases only; inference 0 with the customer's own keys (DEC-432 item 16) | Set by the license | | |

**With inference passed through** (07 principle 3, HLD §10: model tokens at cost plus margin `k`),
the platform fee only has to cover the rest:

```
Revenue = P_fee + (1 + k) · I
GM      = (P_fee − C_rest + k · I) / (P_fee + (1 + k) · I)        C_rest = C_workspace − I
```

In scenario C, `C_rest` is $27.60, so the platform fee's floor at 70% on the non-inference part is
$92, and inference adds `k / (1 + k)` margin on its own (17% at `k` = 20%). Pass-through makes the
owner's own `cost_cap_usd_per_day` the visible control on the variable half of the bill.

**Constraints that hold in every option.**

- **Never per trade, never a percentage of assets or profits** (07 principle 2, HLD §10). No price
  may scale with orders, fills, notional, equity, or P&L.
- HLD §10 lists "decisions" as a usage component. A per-decision charge is not a per-trade charge,
  but it tracks trading activity closely enough to invite that reading; §7 asks the founder to
  meter decisions without pricing them.
- Paper agents are "free or cheap" (07 price levers), yet a paper agent's research costs exactly
  what a live one's does. A free paper tier therefore needs its own research cap or interval floor
  (§7).
- Usage reports carry counts and costs only, never content (CP-1, inference spec §7.4).

---

## 6. Budget guardrails already decided

| Guardrail | Decided in | Where it acts | How it maps to this model |
|---|---|---|---|
| $5 per agent per risk day for internal research agents | DEC-431 item 15 (founder-accepted 2026-10-03; the figures are also recorded in DEC-432) | Gateway reservation (inference spec §7.3); mandate spec §8.5 check 7 | `cap_agent_day`; caps `I_research` at $152 per agent-month |
| $20 per internal workspace per day | DEC-431 item 15 (founder-accepted 2026-10-03; also in DEC-432) | Gateway quota (`budget_exhausted`) | Caps all inference at $608 per workspace-month; binds before the agent caps once a workspace has more than 4 research agents |
| Caps degrade to fewer ideas, never more risk | DEC-431 item 15, INF-7, HI-9 | Gateway and harness | A binding cap lowers `R_day` in effect; it never changes an envelope field |
| Cost reserved at maximum before each call; a crash's reservation counts as spent | DEC-432 item 4 | Gateway meter stream | Effective daily spend can stop up to one `c_call_max` ($0.28) short of the cap |
| Monthly caps for infrastructure and for inference, alerts at 50%, 80%, 100% | DEC-434 item 18 | Operations | The inference cap should sit between expected spend (§3) and the sum of daily caps; the infrastructure cap covers §2.3 to §2.5. Values not yet set (§7) |
| No new paid service until hosting is chosen at M8; Phase 1 on the founder's host | DEC-434 items 13, 19 | Operations | Scenario A has no hosting line |
| No paid data vendor before the DEC-99 evaluation passes; no redistribution in v1 | DEC-433 items 15, 16 | Data plane | `C_data` = 0 |
| Hybrid and on-prem may bring their own model keys | DEC-432 item 16 | Gateway, billing | Our `I` = 0 for those deployments; the meter still counts for quotas |

---

## 7. Open questions and Proposed decisions

The founder's decisions are in [DEC-443](../project/decisions/DEC-443.md), all **Proposed**. Until
decided, nothing changes: the DEC-431 and DEC-434 guardrails hold, and no spend is committed.

| DEC-443 item | Question | Recommendation |
|---|---|---|
| 1 | Target gross margin | 70% blended at scale; the design-partner phase is run at a planned loss |
| 2 | Inference: bundled or passed through | Include a monthly inference allowance in each plan; bill usage above it at cost plus a stated margin |
| 3 | Inference cap per plan | Each plan's policy sets `research_cost_cap_usd_per_day` maximum, a `research_interval_s` floor, and the workspace quota, sized so the allowance stays inside the plan's margin |
| 4 | Free paper tier | Paper agents free on compute, with a research interval floor of one day and a low research cap |
| 5 | Decisions as a usage component | Meter decisions; do not price them |
| 6 | DEC-434 item 18's monthly values for the internal phase | Inference cap at about 1.5 times the expected spend, under the DEC-431 ceiling; infrastructure cap at the founder's current host cost |
| 7 | Process floor for retail | Design a lean workspace (shared control services per cell) before retail pricing |

**Open questions** (not decisions yet):

1. Does research run on days when the agent's market is closed? The harness lets runs continue
   after the close; an equities-only agent idle on weekends would spend about 30% less.
2. What do real prompts measure? `T_in` is the largest single assumption; the spike's paper runs
   (DEC-431 item 15) answer it.
3. How much does the provider's prefix cache save (`s_pre`), given INF-9's rule that shared
   prefixes hold no workspace data?
4. Which cost per run does the spike's report show, and does it change the $5 and $20 caps?
5. Who pays for a live account's SIP feed, and is its cost a barrier the plans must offset?
