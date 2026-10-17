# ADR-0005: The agentic harness: Claude Code for trading

| | |
|---|---|
| **Status** | Accepted ([DEC-2203](../project/decisions/DEC-2203.md)): the founder's direction of 2026-10-17, with the items DEC-2203 marks Proposed left to the founder and the conservative option in force |
| **Date** | 2026-10-17 |
| **Deciders** | The founder (the direction, and the deployment cap's clarification); an agent (Claude Code, lane AH) for the readings, under DEC-79 and DEC-176, from the coordinator's design input and research memo |

Every ADR has a decision behind it; this one's is [DEC-2203](../project/decisions/DEC-2203.md). The
normative text is the [agent harness spec](../specs/agent-harness.md) v0.2 §6.9 to §6.13 and §10.4,
the [inference spec](../specs/inference.md) v0.4, [journal spec](../specs/journal.md) v0.49 §9.22,
[mandate spec](../specs/mandate.md) §8.4, the [data-plane spec](../specs/data-plane.md) v0.3 §4.6,
and the [workspace API spec](../specs/workspace-api.md) §4.7. The research behind it is in
[the memo beside this file](0005-agentic-harness-research-memo.md).

## Context

The founder decided on 2026-10-17 that the research harness must be agentic, because an agent
harness for trading is what the platform sells, and described it as "like Claude Code for trading".
Until now a research run was one model call with no tools, over a context deterministic code
assembled (agent harness spec v0.1 §6.2, HI-4, inference spec INF-5). The safety model must not
move: the model writes opinions, never orders (`AGENTS.md` rule 4); the envelope changes only by a
confirmed version (rule 11); timeouts and ambiguity add no risk (rule 3); everything is journaled
before it is used (rule 5).

The product in one paragraph: an owner gives an agent a mandate and a strategy brief in plain words.
The agent then works on its own: it researches with tools, forms theses, watches what it holds,
explains itself, and asks when it is unsure. The harness lets it think freely and act only through
doors the owner controls: deterministic code sizes every order, the risk gate checks every limit and
account rule, and the autonomy policy decides what runs, what asks, and what is refused. Every step
is recorded and can be replayed. **Agentic where it thinks, deterministic where it touches money.**

## Decision

1. **The mapping.**

   | Claude Code | The harness |
   |---|---|
   | The agent loop | A **run**: a bounded loop of model turns and tool calls (spec §6.9) |
   | Read tools | Market data, news, filings, and the agent's own memory and records, over internal stores only (§6.10) |
   | Edit and Bash, gated by permissions | **Action tools** that only propose: a thesis, an invalidation (§6.10) |
   | Permission modes allow, ask, deny | The autonomy policy AUTO, ASK, DENY (mandate spec §6.2; §6.13) |
   | "Don't ask again" | Delegations: owner-picked, bounded, expiring (ADR-0003) |
   | A hook the model can't override | The order builder and the risk gate |
   | Denied, told why, tries again | A verdict returned as the tool result, as a typed code (HI-25) |
   | CLAUDE.md | The owner's brief, `behavior.description` (§6.12) |
   | settings.json, which the agent can't edit | The envelope (rule 11) |
   | Transcript | The journal: every turn, tool call, and result (journal spec §9.22) |
   | Esc | Pause, Stop, and the kill switch, never dependent on the model |
   | Chat | The desk, `owner` and `explain` runs (spec §15) |
   | MCP | Owner-connected agents use the same reads as owner input (DEC-141) |
   | Usage limits | Per-run, per-agent, per-workspace budgets and the deployment cap, all configuration (§6.11) |

2. **Runs.** Kinds `ideate` and `monitor` run on the mandate's schedule; `owner` and `explain` start
   from a desk message or an ASK (spec §15). One run in flight per agent. The kind fixes the run's
   tool menu before anything untrusted is read.
3. **Two ways to give an opinion.** The guaranteed path ends the run in today's structured output
   (`theses`, `invalidations`), judged once by today's checks. Action tools with mid-run verdict
   feedback are the next step, declared for an entry once they pass their evaluations.
4. **Tools.** Read tools read only the data service's point-in-time store and the agent's own folds,
   never a live vendor query, so no argument leaves the deployment. Think tools (`note`, `plan`) have
   no effect outside the transcript. Action tools map onto paths that already exist. The model never
   gets a tool that sizes, prices, places, approves, skips, delegates, changes the envelope, policy,
   allowlist, or universe, reads the vault, runs code, or reaches a host.
5. **Permissions.** The tool's class decides by default; actions always go through the harness;
   AUTO, ASK, and DENY are the permission mode; the builder and the gate are the hook the model
   cannot override; feedback is truthful and never reveals a limit value; corroboration is computed
   by the platform from its own reads; owner words become an action only once the owner confirms the
   exact request.
6. **Context and memory.** A stable prompt order (pinned system prompt, tool schemas, brief, memory,
   trigger, transcript) so prompt caching serves the prefix; typed run summaries, read back as
   untrusted; least context in every turn (HI-11); provenance on every action; hidden reasoning is
   never evidence.
7. **Bounds and budgets.** Pinned shape bounds (turns, tool calls, result bytes, repeated denials, a
   deadline on the risk clock); money only as configuration (a per-run ceiling and a monthly budget
   per workspace, the agent's daily cap in the envelope, a deployment cap that is $50 a month today
   for the demo and dev deployment). Every bound ends the run, except result size.
8. **Records and replay.** `RunStarted`, `ToolCallRecorded`, `ToolResultRecorded`,
   `RunSummaryRecorded`, and `RunEnded`, beside one `ModelInvocationRecorded` per turn. Replay never
   calls a model, and a run re-runs byte for byte from its recorded responses.
9. **Invariants.** HI-1, HI-3, HI-4, and HI-8 change; HI-24 (journal before each turn), HI-25
   (feedback is truthful), HI-26 (owner words are confirmed), HI-27 (proposals that reach a person are
   rate-limited), and HI-28 (queries stay inside) are new. Each is a property test against an adaptive
   compromised model, at pass^k.
10. **Evaluation.** Safety at pass^k against an adaptive attacker; scenario runs with a live model
    graded on mechanics only; thesis quality on forward paper only (DEC-99). Users get the agentic
    research agent only after its DEC-99 evaluation passes (DEC-103).
11. **Never adopted:** multi-agent bull and bear debate as risk management; code-execution actions;
    the model choosing weights, allocations, sizes, or leverage; self-rewriting prompts; RL or
    fine-tuning on our own outcomes; latent reasoning as audit evidence.

### Where this ADR departs from the design input

The coordinator's design input asked for these; each departure keeps a rule whole, and DEC-2203
records it.

| Design input | This ADR | Why |
|---|---|---|
| Reserve each run's cost up front | Each turn reserves its own maximum; the run has a ceiling checked on every turn | A whole-run reservation holds budget other agents need and counts the whole run at its maximum after a crash |
| Monitoring triggered by events (news, a filing, a price move, a fill, a refusal) | Scheduled only, until a mandate spec change adds triggers | Evaluation triggers and cadence are envelope fields (rule 11), as E19-12 found |
| `ModelInvocationRecorded` gains a turn index | The turn's place is its order among the run's records | The record is closed and being registered; the index is derivable |
| `mandate.read` shows the agent's limits | It shows the envelope's facts that carry no money and no limit value | HI-11 keeps limits and dollar figures out of every prompt; loosening it is the founder's (DEC-2203 item 19) |
| `propose_exit` through the builder | Specified, not declared | A new origin of a discretionary exit is a mandate spec change, which the founder approves (item 14) |
| `propose_mandate_change` from the agent's runs | Owner runs only, specified and not declared | The memo's narrowing; whether the agent may propose envelope changes at all is compliance text (item 15) |
| A run in flight resumes after a crash | A crash ends the run; replay is exact | Without a journaled turn start, a resumed run cannot count the lost turn's reservation (E19-14) |
| Compaction of long runs | Not needed under v0.2's bounds | The bounds keep every prompt small; a journaled compaction is a later change |
| Action tools with mid-run verdict feedback as the stretch goal | Specified, and undeclared until the founder decides | A cap that ends such a run leaves earlier admissions standing, so HI-9 holds only in a weaker form, which is the founder's (DEC-2203 item 25) |
| `ask_owner` in slice 1 | Specified, not declared | It needs its own records, an opaque notification (rule 6), and founder-approved wording (item 20) |

## Consequences

- The model chooses what it reads, inside the data universe, the allowlist, the cut-off, and its
  budget. A fooled model can read more allowlisted data and spend up to its run's ceiling; it still
  produces at most a thesis or an invalidation, which the same deterministic rules judge (HI-10).
- Every turn, tool call, and result is a journal record, so the transcript view, audits, and replay
  read the same facts, and a run's cost and bounds fold from the journal.
- The gateway now carries tool declarations and returns tool calls, so its INF-5 test changes from
  "no tools" to "exactly the pinned menu", and OpenRouter's routing lock admits `tools` for pinned
  declarations only (DEC-997 item 2, with the founder's acceptance of DEC-997).
- Delivery, from the design input: slice 1 the specs (lane AH, 10/19); slice 2 the gateway's tool
  calls and per-turn metering (lanes I1 and AH, 10/21); slice 3 the loop with read and think tools
  (AH), **with E19-6's adversarial bench and a §8.3 regression evaluation inside it** (recorded-fake
  scenarios, an adaptive compromised model, safety at pass^k), so they can pass by about 10/24 and
  the demo agents may pin the agentic entry (DEC-2203 item 13); a live-model pass^k is not planned
  under the $50 cap;
  slice 4 action tools with verdict feedback (AH and I2); slice 5 the shell's driver (I2, after R0);
  slice 6 the desk and transcript view (EX and W2); slice 7 the demo scenario end to end. If the
  agentic slices are not ready, the demo agents pin a `single_call` entry instead: a choice of pin
  made before the demo, never a runtime fallback (DEC-2203 item 13).
- To monitor: verdict feedback may teach a model, within a run, to propose only what passes
  admission (Goodhart pressure; memo question 2); the regression evaluation reports it.

## Alternatives rejected

| Alternative | Why |
|---|---|
| Keep one call per run and widen the retrieval plan | Not an agent; the founder ruled it out |
| Tools as a tagged union in the output schema | Weaker models fill it poorly, and it is tool use under another name; native tool calls validated by the gateway keep the same boundary |
| Run tools inside the gateway | The gateway would need the data plane and the builder, and would stop being the one place that only brokers calls |
| Let the model fetch URLs or search the web | Breaks DP-9 and DEC-101, and opens exfiltration through query strings |
| Retry a failed agentic run as a single call | A second sample after a failure selects outputs (DEC-432 item 3) |
| Bull and bear debate, an "LLM risk team", code actions, model-chosen sizes | Each breaks an invariant or adds no guarantee the gate does not already give (memo §3) |
