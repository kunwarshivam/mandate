# The research, unpacked: what 20 papers taught us about building "Claude Code for trading"

A plain-language companion to the full memo (`docs/adr/0005-agentic-harness-research-memo.md`).
Each source gets what it is, what it found, and what we do about it.

## The five takeaways

1. **Our basic design is what the field converged on.** A small set of typed tools, hard rules the
   model can't override, a recorded transcript, and telling the agent why it was refused. Anthropic,
   Claude Code and the security research all point the same way.
2. **The new risk is poisoned text steering the agent.** Once the agent picks what to read, a news
   article can try to steer what it does next. Fix: decide which tools a run gets *before* it reads
   anything, and track what it had read when it acted.
3. **Backtested LLM trading returns are mostly an illusion.** The models have already seen what
   happened. Only forward paper trading proves an agent's judgment, which is already our rule.
4. **Letting the model size trades loses money.** In the public live contest, four of six models lost
   money. Code sizes; the model only gives opinions.
5. **Skip the trendy stuff:** debating "bull and bear" agents, letting the model run code,
   self-rewriting prompts, and training on our own profits.

## 1. How to build the agent itself

| Source | What it says | What we do |
|---|---|---|
| Anthropic, *Building effective agents* (Dec 2024) | Start with the simplest thing that works, and give the agent clear stopping points | The demo's guaranteed path is the simple loop: read, think, one final answer. Mid-run proposals are the stretch goal |
| Anthropic, *Writing effective tools for agents* (Sep 2025) | Few, well-named tools; trim big results; error messages that say what to do next | Every refusal returns a reason and what the agent can do instead, never your limit values. Results are size-capped |
| Anthropic, *Effective context engineering* (Sep 2025) | Too much context makes models worse; summarize; use sub-agents that return short reports | A stable prompt order (which also makes it cheaper through caching); run summaries; sub-agents later, read-only |
| Anthropic, *Harnesses for long-running agents* (Nov 2025) | Keep a structured progress file the agent can only update in narrow ways | Each run ends with a **structured** summary the next run reads, treated as untrusted |
| Claude Code docs: permissions, hooks, subagents | Deny before ask before allow; a denied tool is *removed* from what the model sees; a blocking hook can't be overridden; sub-agents get their own tool lists | Tools a run may not use aren't shown to it at all. The risk gate is our "hook that can't be overridden" |
| OpenAI, *Harness engineering* (Feb 2026; couldn't open it, read secondhand) | Enforce rules with code checks, not prompts | Already how this repo works (AGENTS.md, xtask checks) |

## 2. How agents use tools (academic)

| Source | What it says | What we do |
|---|---|---|
| **ReAct** (2022) | Reason, act, observe, repeat | This *is* our loop |
| **Reflexion** (2023) | The agent writes notes on its mistakes and reads them next time | A safe form of learning: post-mortem notes in memory that can never change your mandate or its pinned prompt |
| **Toolformer, CodeAct, Search-R1, ReTool** (2023–25) | How models learned to call tools; CodeAct lets the model write and run code | Explains why hosted models are good at tools. We don't adopt CodeAct: running model-written code breaks our "read-only tools" rule |

## 3. The paper you sent: HybridAgent (Stanford and Together AI, Oct 2026)

- **What it is.** It trains open models (4–14B parameters, on 8–32 GPUs) to "think" in hidden number
  space instead of words, switching back to text for tool calls. The result is about 10% better
  scores, half the tokens, and 2–3× faster.
- **The catch for us.** The "readable" hidden thoughts are just each hidden vector's nearest word.
  Nothing checks that this reading is faithful. In their own deployment story, only the model
  provider can read them. We need an audit trail the customer can read.
- **What we take.**
  - Every action and decision stays in text; hidden reasoning never counts as evidence.
  - Record runs exactly, so we never re-derive them (we already do).
  - Pin the model's reasoning mode and effort.
- **Later, maybe.** If we host our own open models, this could make research runs much cheaper.
  Training on our trading results, never.

## 4. LLM trading agents and why their results mislead

| Source | What it says | What we do |
|---|---|---|
| **TradingAgents** (2024) | Analyst, bull and bear, trader and "risk team" agents debate | Popular, but its "risk team" is just another opinion. Our risk gate is a hard rule. Its results come from a period the models had already seen |
| **FinMem, FinAgent, FinRobot** (2023–24) | Layered memory and reflection for trading agents | Ideas for structuring the agent's memory of past theses. Their results are short, leaky backtests |
| **FINSABER** (2025) | Over 20 years and 100+ stocks, LLM "edges" mostly disappear; too timid in bull markets, too aggressive in bear | Don't trust LLM backtests for judgment |
| **StockBench** (2025) | In a clean multi-month test, most models fail to beat buy-and-hold | Same |
| **LiveTradeBench** (2025) | 21 models over 50 live days: being smart on chat leaderboards doesn't mean trading well | Model choice needs its own evaluation |
| **Alpha Arena** (2025; primary page not verified) | Six models with $10k of real money each, choosing size and leverage: four lost money, one about −63% | The cautionary tale: the model must never size trades |
| **Look-ahead papers**: Glasserman & Lin, Profit Mirage, Look-Ahead-Bench, FinCAD, HindsightBench, Temporal Leakage (2023–26) | Models "remember" the future. Backtested returns vanish after the training cutoff. 2026 models guess the date and use what they know. Even "before vs after cutoff" checks don't work | Judgment is proven only by **forward paper trading** (DEC-99). Backtests prove the mechanics (sizing, gate, accounting), never the agent's skill. We never claim performance from backtests |

## 5. Stopping poisoned text (prompt injection)

| Source | What it says | What we do |
|---|---|---|
| **Dual LLM pattern** (Willison, 2023) | A trusted model never sees untrusted text; a quarantined one handles it | Inspired our rule: the trigger decides the run's tools before anything untrusted is read |
| **Spotlighting** (Microsoft, 2024) | Clearly marking outside text as "data" cut attack success from over 50% to under 2% | Every news or filing result is marked as data. An extra layer only; safety never depends on it |
| **AgentDojo** (2024) | A benchmark of attacks on tool-using agents: defenses help only partly | Test with attackers, not just happy paths |
| **CaMeL** (Google DeepMind, 2025) | Plan from the trusted request, quarantine untrusted data, and track where every value came from | Our **provenance** rule: each proposal and each message to you records what outside text the agent had read |
| **Design patterns for securing LLM agents** (2025) | Six patterns, among them action-selector, plan-then-execute, and minimal context | **Fixed tool menus per run kind**: a news-triggered run can only read, think and reduce risk, so a poisoned article can't cause a buy |

## 6. How to test agents

| Source | What it says | What we do |
|---|---|---|
| **τ-bench and τ²-bench** (2024–25) | Grade the end state, not the words. "pass^k": a task passes only if it succeeds every time; most agents drop sharply | Safety must pass **every** trial, graded from the journal |
| Anthropic, *Demystifying evals* (Jan 2026) | 75% per try is only 42% across 3 tries; read the transcripts | Same, plus a safety test suite with an adaptive "attacker" model |

## What changed in our design because of this

1. **Fixed tool menus per run kind:** news-triggered runs can only reduce risk.
2. **Provenance** on every proposal and every message to you.
3. **Our code checks corroboration itself**, not from what the model chose to read.
4. **Outside text is marked as data.**
5. **Refusals explain** without revealing limits, and a repeated refusal ends the run.
6. **The model provider, reasoning effort and settings are pinned,** with fallbacks off.
7. **Structured, untrusted run summaries** as memory.
8. **Safety is tested every trial against an adaptive attacker.**
9. **Grading is on mechanics, never returns.**
10. **A stable prompt order** for cheaper multi-turn runs.
11. **Later:** read-only sub-agents, and a second "monitor" model that can only add caution.

## Open questions for later

- **Hidden reasoning disclosure.** If the model provider hides its internal reasoning, the run view
  must say so. That wording is compliance text, so it's yours.
- **Steering ideas toward what's easy.** Could the agent drift toward ideas that are easy to approve
  rather than good ones? We should watch for that in evaluations.
- **Quoting externally.** Check the Alpha Arena numbers and the OpenAI posts before quoting them
  anywhere public.
