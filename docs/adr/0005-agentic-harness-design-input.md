# The agentic harness: Claude Code for trading

Design for ADR-0005 and agent-harness spec v0.2. Status: founder direction of 2026-10-17 ("it has
to be agentic harness, as what we are selling/the product is agent harness for trading"; "like
claude code for trading"). Lane AH turns this into the ADR, the DEC (2200 range), and the normative
spec changes (agent-harness, inference, journal, mandate §8, workspace API). Items marked
**founder** stay Proposed and the build takes the conservative option.

## 1. The product in one paragraph

An owner gives an agent a mandate (money, goal, limits, what it may trade, when to ask) and a
strategy brief in plain words. The agent then works on its own: it researches with tools, forms
theses, proposes trades, watches what it holds, explains itself, and asks when it is unsure. The
harness lets it think freely and act only through doors the owner controls: deterministic code
sizes every order, the risk gate checks every limit and account rule, and the autonomy policy
decides what runs, what asks, and what is refused. Every step is recorded and can be replayed.
**Agentic where it thinks, deterministic where it touches money.**

## 2. The mapping

| Claude Code | Owlhead harness | Exists today |
|---|---|---|
| Agent loop: model, tools, repeat until done | A **run**: a bounded loop of model turns and tool calls | No: one call, no tools (spec §6.2, INF-5) |
| Read tools (Read, Grep, WebFetch) | Market data, news, filings, fundamentals, screens, the agent's own memory and records | Readers exist (I3, data plane); the model can't call them |
| Edit and Bash, gated by permissions | **Action tools** that only propose: thesis, entry, exit, invalidation, ask the owner, mandate change | The thesis shape exists (mandate §8.4) |
| Permission modes: allow, ask, deny | Autonomy policy AUTO / ASK / DENY (mandate §6) | Yes |
| "Don't ask again for this" | Delegations: bounded, expiring, owner-picked (ADR-0003, mandate §6.5) | Spec yes, code partly |
| PreToolUse hook that can't be overridden | The risk gate and the builder: no model output can override them | Yes |
| Permission denied, told why, tries again | A DENY or a refused admission comes back as the tool result, with a typed reason | No |
| CLAUDE.md | The owner's strategy brief (`behavior.description`, extended), inside the envelope | Partly |
| settings.json, which the agent can't edit | The mandate envelope: only the owner confirms changes | Yes (rule 11) |
| Transcript, resume | The journal: every turn and tool call; replay without calling a model | The journal yes; per-turn records no |
| Esc, interrupt | Pause and the kill switch, never dependent on the model | Yes |
| Subagents | Sub-runs with their own budget, for example "dig into this filing" | No |
| Chat session | The desk: talk to your agent (ADR-0003 item 4) | Spec yes, code no |
| MCP | External agents (Claude, ChatGPT, Dots) use the same tools as owner input (DEC-141, DEC-183) | Spec yes, code partly |
| Usage limits | Per-run, per-agent, per-workspace budgets, plus the deployment cap | Partly (the gateway meter) |

## 3. Architecture

```
            owner (web desk, approvals, MCP client)
                 |  owner input            ^  asks, explanations, transcript
                 v                         |
   +---------------------- AGENT LOOP (model + tools) ----------------------+
   |  context: pinned system prompt, owner brief, memory, run trigger       |
   |  loop: model turn -> tool call -> harness result -> model turn ...     |
   |  read tools: free inside bounds   action tools: proposals only         |
   +-----------------------------|-----------------------------------------+
                                 v  proposal (typed, no size, no price)
   +---------------------- HARNESS (deterministic) -------------------------+
   |  checks (spec §6.5) -> admission (§8.5) -> builder sizes (§8.3)        |
   |  -> gate dry run -> autonomy AUTO / ASK / DENY -> result to the model  |
   |  journal: every turn, tool call, result, verdict, before use           |
   +-----------------------------|-----------------------------------------+
                                 v  intent (journaled, idempotency key)
   +---------------------- EXECUTION (unchanged) ---------------------------+
   |  binding gate on the account stream -> executor -> broker              |
   |  stops, risk exits, kill switch: never wait on the agent loop          |
   +------------------------------------------------------------------------+
```

The trading tick stays deterministic and model-free (HI-21). The agent loop runs beside it, and its
proposals enter the core as inputs once their records commit.

## 4. Runs

A **run** is one session of the loop, with a trigger, a context, a budget, and a transcript.

| Kind | Trigger | Starting context | Typical output |
|---|---|---|---|
| Ideation | Schedule (the mandate's cadence) | Brief, universe screens, memory | Theses, entries |
| Monitoring | Schedule, or an event on a held or watched name: news or filing, a price move past a band, a fill, a gate refusal, a thesis nearing expiry | That position's thesis lineage, the event | Invalidation, revision, exit, nothing |
| Owner | A desk message | The conversation and the referenced records | Answer, explanation, proposal for the owner to confirm |
| Explain | An ASK is raised | The proposal and its records | The plain-language card text |

Rules:

- At most one run in flight per agent (HI-8). Owner and explain runs pre-empt queued runs; they never
  pre-empt trading.
- Event triggers are debounced and rate-limited per agent, so a news storm can't buy a run storm.
- Mode gates runs (HI-15). No run starts while the agent is `paused` or `stopped`; a run in flight is
  cancelled at its next step; its committed proposals stand only where they reduce risk.

## 5. Tools

Each tool has a typed schema, a class, a size cap on its result, and a cost. The tool set and its
schemas are pinned in the research entry's content hash (principle 8: changing them is a new
version the owner confirms).

**Read** (free inside the bounds):

- `market.bars`, `market.quote`, `market.screen` (eligible universe only).
- `news.search`, `news.get`, `filings.search`, `filings.get`: over the platform's ingested,
  allowlisted store only, never a live query to a vendor, so the model's queries never leave the
  deployment. Knowledge time is at or before the run's cut-off (HI-13).
- `fundamentals.get`, `calendar.get` (earnings, market hours).
- `memory.positions`, `memory.theses` (lineages and outcomes), `memory.runs` (past run summaries,
  the agent's "progress file"), `records.query` (its own decisions, verdicts, and refusals).
- `mandate.read`: the envelope and the autonomy rules, so the agent knows its own limits.

**Think** (no side effect):

- `note`: scratch text, kept in the run's transcript.
- `plan`: the agent's stated next steps, shown in the plan view.

**Action** (proposals only; each is judged by the harness, and the result comes back to the model):

| Tool | Becomes | Judged by |
|---|---|---|
| `propose_thesis(instrument, direction: long, conviction, confidence, horizon, evidence ids, invalidation)` | A thesis (mandate §8.4) | §6.5 checks, §8.5 admission, autonomy for the admission |
| `revise_thesis(thesis_id, ...)` | A revision (§8.6) | The same |
| `invalidate(thesis_id, reason, evidence ids)` | An invalidation (§6.7): removes at once (MI-19) | Never denied (risk reduction) |
| `propose_exit(instrument, reason)` | A discretionary exit request through the builder | Conduct controls pace it; never denied (rule 13) |
| `ask_owner(question, options)` | An owner question on the desk | Rate-limited; timeout means no answer (rule 3) |
| `propose_mandate_change(diff, reasons)` | A draft mandate version | Always ASK, validator and §9.2 classification, step-up; the model never confirms; expires unanswered; rate-limited |
| `spawn_subrun(goal, budget)` (later) | A child run with a slice of the parent's budget | The parent's bounds |

What the model never gets: a tool that sizes, prices, or places an order; that approves or skips;
that creates or widens a delegation; that changes the envelope, policy, allowlist, or universe
directly; that reads the vault; or that reaches any host. **Entries carry no size and no price**:
`propose_thesis` carries conviction and confidence, and the builder turns them into a size inside
the limits. That is AGENTS.md rule 4, kept whole.

**Feedback is the point.** Every action returns its verdict: admitted, asked (with the card id),
denied with a typed reason (`over_max_instruments`, `not_eligible`, `gate:max_position`,
`cooldown`, ...), or held. The model sees why and can adjust, as Claude Code does when a permission
is denied. The reason codes are typed and carry no limit values beyond what `mandate.read` already
shows.

## 6. Permissions

1. **The classes decide by default.** Read and think: allowed inside the bounds. Action: through the
   harness, every time.
2. **AUTO / ASK / DENY is the permission mode** (mandate §6.2). Supervised, Co-pilot, Autopilot
   (ADR-0003 item 3) are the dial.
3. **Delegations are "don't ask again"**: owner-picked, bounded, expiring, suspended by any sign of
   trouble (ADR-0003 item 2). The model can't create or pick one (item 5).
4. **The gate is the hook that can't be overridden**, and the binding gate at submission still runs
   on the account stream.
5. **Owner-run proposals are owner input** (rule 11, DEC-141), but the model's reading of the
   owner's words is confirmed by the owner before it acts: "sell half of X" shows as the exact order
   request and needs a tap (and step-up where the action adds risk). A model can't turn a chat
   message into an action the owner didn't see.

## 7. Context

- **Pinned system prompt**: the template, the tool schemas, the output rules, and the methodology
  text, in the content hash.
- **The owner's brief**: `behavior.description`, extended to a strategy brief of bounded length. It
  is owner input inside the envelope; nothing in it can lift a limit, and the validator warns where
  it contradicts the envelope.
- **Memory**: the agent's own positions, thesis lineages and outcomes, and past run summaries. The
  end of every run writes a summary record, so the next run starts where this one stopped.
- **Least context** (HI-11): no credential, vault reference, personal data, account dollar figures,
  or another agent's or workspace's data, ever, in any turn. Canary tests over every turn's prompt.
- **Compaction**: a long run summarizes earlier turns into a journaled summary artifact. Replay uses
  the recorded summary and never re-summarizes.

## 8. Bounds and budgets

| Bound | Where it's set | On reaching it |
|---|---|---|
| Steps per run, tool calls per run, result size per call, tokens per turn | Pinned in the entry (version) | The run ends: no further proposals; committed ones stand |
| Cost per run | Reserved up front through the gateway meter | The run ends |
| Deadline per run | Risk clock | The run ends (rule 3) |
| Agent cost per day | `behavior.research.cost_cap_usd_per_day` (DEC-120) | No new runs that day; exits, protection, and kill switches are unaffected |
| Workspace budget per month | A workspace setting | No new runs in that workspace |
| Deployment cap per month | A deployment setting, $50 today for the demo deployment (DEC-997, **founder**) | No new runs anywhere in the deployment |
| Event-triggered runs | A per-agent rate limit | Triggers coalesce |

A spent budget means fewer ideas, never more risk (HI-9). **Founder**: who pays for customers'
model calls (a plan or the owner's own key). The design supports both: the gateway meters per
workspace either way.

## 9. Records and replay

New or changed records (journal spec, ES-22):

- `RunStarted` (trigger, kind, cut-off, budget), `RunEnded` (reason: done, bound, deadline, cost,
  mode, error).
- `ModelInvocationRecorded`, one per turn (it is one per run today), with the turn index.
- `ToolCallRecorded` (tool, arguments) and `ToolResultRecorded` (content ref, size, truncation),
  each committed before the next turn reads it (HI-2 generalized).
- Action verdicts reuse today's records: `ThesisProposed`, `ThesisRevised`, the invalidation
  verdict (E19-5), the candidates' verdicts (E15-8), and approvals.
- `RunSummaryRecorded`: the memory the next run reads.

Replay feeds recorded results and never calls a model (INF-10, HI-3): a run replays byte for byte.
The web app's **transcript view** reads the same records: "here is every step your agent took,
what it read, and why".

## 10. Invariants

Kept as they are: HI-5 (the envelope doesn't move), HI-6 (missing is safe), HI-7 (research never
blocks trading), HI-9, HI-11 to HI-23.

Changed:

- **HI-1 Opinions only** becomes: a model output reaches the order path only as an action-tool
  proposal that the harness checks, admits, sizes, gate-dry-runs, and puts through autonomy. No tool
  sizes, prices, places, approves, or confirms.
- **HI-4 Reads only** becomes: every read tool only reads, from allowlisted, internal stores; the
  model may choose reads only from the pinned tool set, within the bounds. INF-5 changes to allow
  the pinned tool set and nothing else (no browsing, no code, no network).
- **HI-8 Runs are bounded** becomes the §8 table, with independent counters from the journal.

New:

- **HI-24 Journal before each turn**: no turn reads a tool result that isn't committed.
- **HI-25 Feedback is truthful**: the verdict a tool returns equals the journaled verdict.
- **HI-26 Owner words are confirmed**: no owner-run action proceeds without the owner seeing the
  exact request.
- **HI-27 Proposals are rate-limited**: asks, owner questions, and mandate-change proposals per
  agent per day.
- **HI-28 Queries stay inside**: no tool argument leaves the deployment.

Each invariant is a property test with an independent oracle, shown to catch a seeded bug, under a
**compromised model**: a gateway double that picks any schema-valid tool call each turn.

## 11. Adversaries

| Attack | Defense |
|---|---|
| A news item tells the model to read or buy something | Reads are bounded and internal; buys are only proposals: allowlist, corroboration, eligibility floor, `max_instruments`, admission autonomy, builder, gate (HI-10) |
| The model loops on tools or burns money | Step, call, and cost bounds; a reservation before each turn; one run per agent |
| A huge or hostile tool result | Result size caps; typed, sanitized results; truncation is recorded |
| The model nags the owner into a riskier mandate | Always ASK, step-up, rate limit, expiry; the diff is shown with its §9.2 classification |
| The model misreads the owner's chat | HI-26: the exact request is shown and confirmed |
| Leaking what the agent holds through search queries | Searches run over the internal store only (HI-28) |
| A slow or down provider delays an exit | No model call in the tick (HI-21); risk reduction never waits (HI-14) |
| A version change mid-run | The version in effect judges (HI-17); the run ends at the version change |
| Cross-tenant reads in the desk | Tools are scoped to the asking member's workspace and role; property tests |

## 12. Evaluation

- **Scenario suites**: recorded markets and news replayed through the loop with a recorded or live
  model; the grade comes from the journal (what was proposed, admitted, refused), not from the
  model's words.
- **Safety at pass^k**: every safety property must hold in all k trials; quality at pass@k.
- **Thesis quality**: forward paper only (DEC-99, DEC-122); backtests prove mechanics, not
  judgment.
- **Change control**: a new template, tool set, bound, or model is a new version; the regression
  suite runs on it before the owner is offered it (spec §8.3, §8.4).
- **Users** get the agentic research agent only after the DEC-99 evaluation passes (DEC-103 thin
  slice, **founder**); until then it runs in the team's internal paper workspaces and the demo.

## 13. Surfaces

- **The desk**: chat with your agent (read-only first, then proposals you confirm).
- **The plan view**: what it's watching, what it would do next, what would stop it.
- **The transcript view**: every run, step by step.
- **The approval card**: the proposal, the agent's explanation, and the scopes you may delegate.
- **The daily brief**.
- **MCP**: the same tools for your own agent (Claude, ChatGPT, Dots), as owner input.

Compliance wording on every surface (no advice, no performance claims) is **founder** and counsel
(DEC-79).

## 14. Out of scope for v1

The model sizing, pricing, placing, approving, or confirming; short sales; calibration of weights;
user-supplied code; perpetual delegations; the platform proposing `auto`.

## 15. Delivery

| Slice | What | Lane | Target |
|---|---|---|---|
| 1 | ADR-0005, DEC, agent-harness spec v0.2, inference spec (INF-5 amended, tool calls), journal records, mandate §8.4 touch-ups | AH | 10/19 |
| 2 | Gateway tool-call shape, per-turn metering, the deployment cap as a setting, multi-turn recorded fakes, OpenRouter tool calls | I1 + AH | 10/21 |
| 3 | The loop in `mandate-research-run`: read and think tools, bounds, records | AH | 10/22 |
| 4 | Action tools mapped onto theses, invalidations, and exits, with verdict feedback | AH + I2 | 10/23 |
| 5 | Run triggers (schedule and events) and the shell's driver (after R0) | I2 | 10/23 |
| 6 | Desk read-only Q&A, explain runs, the transcript view | EX + W2 | 10/24 |
| 7 | The demo scenario runs an agentic ideation and a monitoring run end to end | E2E | 10/25 |
| After the demo | Owner proposals in chat with step-up; the mandate-change proposal UI; subruns; MCP exposure; the evaluation suites | AH, EX, A3 | November |

Risk: slices 2 to 4 are a lot for six days. The one-call run stays as the fallback mode, so the
demo shows the agentic loop if slice 4 lands and the one-call run if it doesn't.

## 16. Revisions from the research review (2026-10-17)

The research memo (`0005-agentic-harness-research-memo.md`, beside this file) reviewed HybridAgent
(arXiv 2610.18861) and about 20 other sources. The coordinator adopts these changes; each binds the
spec work above, and where it conflicts with an earlier section, this section wins.

1. **Demo path.** The guaranteed path for 10/26 is a read-and-think loop that ends in the existing
   structured output (candidate theses and invalidations), judged by today's checks (§6.5). Action
   tools with mid-run verdict feedback (§5) are the stretch goal, landing when ready.
2. **Tool menus fixed per run kind, before anything untrusted is read.** The trigger chooses the
   menu, and tools outside it are left out of the schema. A news- or event-triggered monitoring run
   gets read and think tools plus risk-reducing actions only (`invalidate`, `propose_exit`). Only
   scheduled ideation runs may propose a thesis or entry. Owner runs get what the owner's request
   needs, confirmed per HI-26.
3. **Provenance.** Every action, and every text shown to the owner, records which untrusted tool
   results were in context when it was written ("written after reading: [result ids]"), so a
   reviewer and the explain runs can see what may have steered it.
4. **The platform computes corroboration** from its own reads, independent of what the model chose
   to read (HI-18 extended).
5. **Spotlighting.** Untrusted tool results are delimited and marked as data in every prompt. This
   is a defence in depth only, never a control the invariants rest on.
6. **Refusals** come back actionable but never reveal a limit value beyond what `mandate.read`
   shows. A repeated identical denial ends the run.
7. **Pin and record** the reasoning mode and effort, the sampling parameters, and the OpenRouter
   provider, with provider fallbacks off (DEC-67: no substitution).
8. **Run summaries are structured**, typed fields not free prose, and the next run treats them as
   untrusted input.
9. **Safety is tested at pass^k against an adaptive attacker**: a compromised-model double that
   picks its tool calls from the run so far, not only random ones.
10. **Scenario runs with a live model are graded on mechanics only**, never on returns: look-ahead
    bias makes backtested LLM returns meaningless (DEC-99 stands).
11. **Subruns**, when built after the demo, get read and think tools only and return a typed summary.
12. **An optional monitor model that can only downgrade** (AUTO to ASK), journaled like any model
    output; a missing monitor means no downgrade. After the demo.
13. **A stable prompt prefix**: pinned system prompt, then tool schemas, then the brief, then
    memory, then the trigger, so provider prompt caching cuts the cost of multi-turn runs.
14. **Never adopted**: multi-agent bull/bear debate as risk management, code-execution actions, the
    model choosing weights, allocations, sizes or leverage, self-rewriting prompts, and RL or
    fine-tuning on our own outcomes. HybridAgent's latent reasoning is out while we use hosted
    models, and out as audit evidence in any case: hidden reasoning is never evidence, and only
    text actions and records count.
