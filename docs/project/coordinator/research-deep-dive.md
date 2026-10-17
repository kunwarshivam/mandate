# Harness research deep dive: what each source means for Owlhead / Mandate

Date: 2026-10-17. Companion to `harness-research-memo.md` and to ADR-0005 §16 ("Revisions from the research review"), whose items are cited below as **§16.n**.

Each source gets six short parts: in one line; what it actually found; why it matters for us; what we do because of it; what we deliberately don't take; and the open question or risk it raises. "Not accessed" means I relied on secondary coverage, and says so.

Shorthand for our parts:

- **Run kinds:** ideation, monitoring, owner, explain.
- **Proposal tools:** `propose_thesis`, `revise_thesis`, `invalidate`, `propose_exit`, `ask_owner`, `propose_mandate_change`.
- **The doors:** the builder (sizes), the risk gate (limits and US account rules), and autonomy AUTO/ASK/DENY (permissions).
- **The record:** the journal and replay without a model.
- **DEC-99:** thesis quality is judged on forward paper results only.
- **Compliance:** no advice wording, no performance claims (DEC-126, DEC-79).

---

## A. Building agents and harnesses

### 1. Anthropic, "Building effective agents" (2024-12-19)

https://www.anthropic.com/engineering/building-effective-agents

**In one line.** Use the simplest pattern that works. Workflows (fixed code paths) beat agents wherever the steps are predictable.

**What it actually found.** This is a practitioner synthesis from Anthropic's customer work, not an experiment. It separates workflows (prompt chaining, routing, parallelization, orchestrator-workers, evaluator-optimizer) from agents, where the model directs its own tool use. Its advice:

- Agents cost more and compound errors, so use them only for open-ended work.
- Treat the agent-computer interface like a UI: "poka-yoke" tools, which are hard to misuse. On SWE-bench, requiring absolute file paths removed a whole class of errors.
- Get ground truth from the environment each step, use stopping conditions, and pause for human checkpoints.

**Why it matters for us.** Our product is literally "agentic where it thinks, deterministic where it touches money". That is a workflow around the money and an agent around the research. The one-call research run that exists today is a workflow, and the agent loop is the upgrade. The post argues the upgrade has to earn its cost. It also argues the fallback is not a failure, just the simpler pattern.

**What we do because of it.**

- §16.1: the guaranteed demo path is a read-and-think loop ending in today's structured output, judged by today's §6.5 checks. Action tools with mid-run feedback are the stretch goal.
- Design every tool to be poka-yoke. `propose_thesis` takes a symbol and evidence ids, never a size or price, and the platform resolves the symbol. That is already ADR-0005 §5.
- Keep the step, call, and cost bounds (§8) as the stopping conditions.

**What we deliberately don't take.** The evaluator-optimizer loop as a quality gate on theses: a second model grading the first adds cost and no guarantee. The only judges that bind are the deterministic checks and the owner. An optional monitor that can only downgrade is allowed later (§16.12).

**Open question or risk.** Does the agentic loop beat the one-call run on anything we can measure before forward paper results exist? If not, the demo shows a more expensive version of the same output. We should measure at least the evidence diversity and the refusal rate per run.

### 2. Anthropic, "Writing effective tools for agents" (2025-09-11)

https://www.anthropic.com/engineering/writing-tools-for-agents

**In one line.** Tools are prompts. Design, test, and evaluate them like an interface for a new colleague.

**What it actually found.** Practitioner guidance backed by Anthropic's internal tool evals:

- Consolidate related actions into one tool, and namespace tools by service.
- Return human-readable identifiers, and offer concise and detailed response formats.
- Paginate and filter, and truncate with guidance on how to narrow the query. Claude Code caps a tool result at 25k tokens by default.
- Write errors that are "specific and actionable", not opaque codes.
- Evaluate tools on realistic multi-step tasks with verifiable outcomes and a held-out set, and track redundant calls and tool errors.

**Why it matters for us.** Our "feedback is the point" design (ADR-0005 §5) is exactly the error-message lesson applied to refusals: `gate:max_position`, `not_eligible`, `cooldown`. Our result size caps and recorded truncation match the pagination advice. Read tools like `news.search` and `filings.search` will dominate token spend under the $50 deployment cap.

**What we do because of it.**

- §16.6: verdicts come back actionable, naming what the model can do next (a tool or a field), but never a limit value beyond what `mandate.read` shows.
- Every read result is truncated with a hint ("narrow by date or form type"), and the truncation is journaled (`ToolResultRecorded`).
- Namespaced tool names (`market.*`, `news.*`, `memory.*`), already in §5.
- Build a small tool eval: recorded scenarios, counting redundant calls, schema errors, and truncations per run, inside the §8.3 regression evaluation.

**What we deliberately don't take.** "Detailed" response modes for read tools that would put raw licensed text into prompts. HI-23 keeps uncited licensed text out of what we store, so results are summaries or fields plus item ids.

**Open question or risk.** Can actionable refusals teach the model to game admission ("cite two sources to pass corroboration")? That is intended within a run. Across versions it shapes what the agent proposes, so admission rate must never be a quality metric.

### 3. Anthropic, "Effective context engineering for AI agents" (2025-09-29)

https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents

**In one line.** Context is a finite budget that rots as it grows. Fetch just in time, compact, take notes, and delegate to clean contexts.

**What it actually found.** Guidance, citing the "context rot" research: recall accuracy falls as context grows. Its techniques:

- Just-in-time retrieval: keep references, not content, and load content through tools.
- Compaction near the limit, with the lightest form being clearing old tool results.
- Structured notes outside the window, such as a NOTES.md.
- Subagents that explore in their own window and return condensed summaries.
- A minimal, non-overlapping tool set.

**Why it matters for us.**

- *Monitoring runs* read a position's thesis lineage plus an event, so they are short.
- *Ideation runs* read screens, filings, and news, so they are long.
- *Compaction* in ADR-0005 §7 must be replayable, so the summary is journaled and replay never re-summarizes.
- *Memory* (`memory.runs`, run summaries) is the "notes outside the window".

**What we do because of it.**

- §16.13: a stable prompt prefix (pinned system prompt, tool schemas, brief, memory, trigger), so prompt caching cuts multi-turn cost.
- Prefer clearing old tool results over model-written compaction. Clearing is deterministic and replays trivially; a model summary is another model output to journal and taint.
- §16.8: structured run summaries, not prose.
- §16.11: subruns, after the demo, read-only.

**What we deliberately don't take.** Open-ended autonomous exploration as the default. The tool menu per run kind (§16.2) and the eligible-universe screen bound what the model can look at.

**Open question or risk.** A compaction summary is model-written text that later turns treat as fact. If an injected news item shaped it, the injection survives compaction. Compaction summaries need the same taint as run summaries (§16.3, §16.8).

### 4. Anthropic, "Effective harnesses for long-running agents" (2025-11-26)

https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents

**In one line.** Long work survives across sessions only through durable, structured hand-off artifacts and a fixed startup routine.

**What it actually found.** From building apps with Claude over many sessions:

- An *initializer* session creates an init script, a progress log, a git commit, and a JSON feature list with every feature marked failing.
- Each later *coding* session reads the progress file and git log, runs a smoke test, works on one feature, and flips only that feature's `passes` field. "It is unacceptable to remove or edit tests."

The failure modes it names: declaring victory early, leaving undocumented half-work, and marking features done without end-to-end testing.

**Why it matters for us.** Our runs are sessions too. An ideation run today and a monitoring run tomorrow share state only through memory: `memory.positions`, `memory.theses`, `memory.runs`, and `RunSummaryRecorded`. The post's lesson is that the hand-off artifact must be structured, mostly platform-owned, and narrowly editable by the model. That matches HI-18 (the platform decides the facts).

**What we do because of it.**

- §16.8: run summaries are typed records. The platform fills open theses, pending asks, refusals and reasons, and items read. The model writes one bounded "next steps" field, which is quoted and tainted.
- A fixed startup routine for each run kind: read mode, mandate version, and open theses near expiry before any model turn, all deterministic.
- The thesis lifecycle (`expires_at`, invalidation) plays the role of the feature list. The model can propose and invalidate, but can't silently delete or rewrite history. That is already true through the journal.

**What we deliberately don't take.** Letting the model maintain its own progress file as free text. That is a persistent injection channel.

**Open question or risk.** How much model-written text should memory keep at all? Every free-text field is a channel from one run's injected input to the next run's context.

### 5. OpenAI, "Harness engineering: leveraging Codex in an agent-first world" (2026-02-11, Ryan Lopopolo)

https://openai.com/index/harness-engineering/. **Not accessed:** openai.com returned 403 through the proxy. Details are from secondary coverage (InfoQ Japan, March 2026; noze.it). OpenAI's Codex agent-loop post was also blocked.

**In one line.** When agents write the code, the environment (docs as a map, mechanical checks) is the product.

**What it actually found (secondary).** An internal team shipped a product over about five months with no hand-written code. The reported practices:

- AGENTS.md as a short index (around 100 lines) pointing to deeper docs.
- Invariants and layering enforced by custom linters, many written by Codex itself.
- "Humans steer, agents execute", and human attention treated as the scarce resource.

**Why it matters for us.** It is mostly about how we *build* (this repo's trust ladder already puts rules on "unrepresentable" and "checked" rungs). For the *product*, the same principle applies to the trading agent: a rule the model must follow belongs in code (the gate, the builder, schema-level tool absence), not in the system prompt.

**What we do because of it.** It confirms ADR-0005's choice that "the gate is the hook that can't be overridden" and the HI-1/HI-4 crate-layer checks. Any limit we find ourselves putting in the prompt ("never buy more than…") is a bug: it belongs in the builder or the gate, or in `mandate.read` as information only.

**What we deliberately don't take.** Nothing specific. We can't verify the details, so we cite nothing from it externally.

**Open question or risk.** Not verified. Re-fetch from a network that reaches openai.com before quoting.

### 6. Claude Code docs: permissions and hooks (read 2026-10-17)

https://code.claude.com/docs/en/permissions and https://code.claude.com/docs/en/hooks

**In one line.** Permissions are evaluated deny, then ask, then allow; hooks fail closed only when told to; and pattern rules are not a security boundary.

**What it actually found.**

- *Rule order:* rules are evaluated deny, then ask, then allow, and the first match wins regardless of specificity.
- *Tool removal:* a bare-tool deny ("Bash") *removes the tool from the model's context*. A scoped deny leaves the tool visible and refuses matching calls.
- *Bash rules* match command text, so they are "not a security boundary"; the OS sandbox is.
- *Managed settings* can't be overridden, and bypass and auto modes can be disabled.
- *Hooks:* a PreToolUse hook returns allow, deny, ask, or defer; exit code 2 blocks even if the JSON says allow. A hook that times out or crashes does *not* block unless `onFailure: "block"` is set.

**Why it matters for us.** This is our product metaphor (ADR-0005 §2: AUTO/ASK/DENY as permission modes, the gate as the hook). The details show where the metaphor would mislead:

- Our gate must be the sandbox, not a pattern rule. It is: it runs on structured intents in deterministic code, and the binding gate runs again at submission on the account stream.
- Our gate must fail closed. It does: missing means safe, and rule 3.
- Tools a run may not use should not be in the schema at all.

**What we do because of it.**

- §16.2: tools outside a run kind's menu are omitted from the schema, not refused.
- Apply deny-first ordering in autonomy evaluation (mandate §6 already does).
- An error, timeout, or unparseable verdict in the harness yields no proposal (HI-6).
- The envelope is our "managed settings": the agent can't edit it (rule 11, HI-5).

**What we deliberately don't take.** Claude Code's auto mode, where a classifier model approves actions. Our approvals come from the owner or from owner-picked delegations (ADR-0003), never from a model.

**Open question or risk.** Omitting tools changes the prompt per run kind, which splits the prompt cache and the content hash. Is one research entry with several menus one signal-model version or several? (§8.4)

### 7. Claude Code docs: subagents (read 2026-10-17)

https://code.claude.com/docs/en/sub-agents

**In one line.** Subagents get a clean context, their own tool allowlist and model, and return only a final result.

**What it actually found.**

- Each subagent has its own system prompt, `tools` allowlist or `disallowedTools`, permission mode, and model.
- The parent sees only the final result, not intermediate tool calls.
- Claude Code scans subagent reports for instruction-like text and annotates it before the parent reads it.
- Subagents count toward the same usage limits.

**Why it matters for us.** `spawn_subrun(goal, budget)` is "later" in ADR-0005 §5. The docs show the safe shape: a narrower tool set, a slice of the budget, and a summary returned as data. Their scanning of reports for instructions admits that a subagent's output is untrusted input to the parent. In our case it may have read a hostile filing.

**What we do because of it.** §16.11: subruns get read and think tools only, return a typed summary (item ids, extracted fields, and one quoted note), and inherit the parent's taint. A subrun's spend comes out of the parent's reservation (§8). Every subrun turn is journaled like a run turn, so replay covers it.

**What we deliberately don't take.** Subruns with action tools, or with a different model than the pinned one. Each model is a pin (INF-2) and part of the content hash.

**Open question or risk.** Do we need subruns at all in v1? Per-run token bounds may be enough. Building them adds a journal shape and a budget split for a benefit we haven't measured.

---

## B. Tool use and the agent loop

### 8. ReAct (Yao et al., arXiv:2210.03629, 2022-10-06)

**In one line.** Interleave reasoning text with tool actions, and the agent both plans better and hallucinates less.

**What it actually found.** Prompting a model to alternate thought → action → observation:

- On HotpotQA and FEVER, using a simple Wikipedia API, ReAct reduced the hallucination and error propagation of chain-of-thought alone.
- On ALFWorld and WebShop it beat imitation and RL baselines by 34 and 10 absolute points of success rate, with one or two in-context examples.
- Its trajectories were more interpretable to humans.

**Why it matters for us.** ReAct *is* the loop in ADR-0005 §3: model turn, tool call, harness result, model turn. Its interpretability claim is the basis of our transcript view ("every step your agent took, what it read, and why"). The hallucination result supports moving from "code gathers, the model reads" (one call) to letting the model fetch what it needs. Grounding in retrieved items cuts invented facts, and our `unknown_evidence` check catches the rest.

**What we do because of it.**

- Build the loop as plain ReAct over native tool calls, not a framework (see source 1).
- Keep `note` and `plan` as explicit think tools, so reasoning shows in the transcript and the plan view as text we record.

**What we deliberately don't take.** The idea that the reasoning text explains the action. With today's reasoning models, the visible "thought" can differ from the hidden reasoning (see HybridAgent below). The transcript view should say "what it read, wrote, and proposed", not "why".

**Open question or risk.** The ReAct gains came from weaker models on tasks with crisp feedback. In markets the observation rarely says you were wrong, so the loop's self-correction may be weak, and the refusal codes become most of the feedback signal.

### 9. Reflexion (Shinn et al., arXiv:2303.11366, 2023-03-20)

**In one line.** Agents improve across trials by writing verbal post-mortems into episodic memory, with no weight updates.

**What it actually found.** After each trial, the agent reflects on the feedback signal (a scalar, a test result, or free text) and stores the reflection. The next trial reads it. It reached 91% pass@1 on HumanEval, against GPT-4's 80% at the time. Ablations showed that the feedback source and the memory format matter.

**Why it matters for us.** This is the only "learning" that fits our constraints: no post-training, a pinned prompt, and a pinned model. Our memory tools (`memory.theses` with outcomes, `records.query` for refusals) let the next run see what happened to the last thesis. That is a Reflexion loop with market feedback.

**What we do because of it.**

- Make thesis outcomes visible to the next run as *platform-computed* fields (expired, invalidated, the price path since `as_of`), not model-written lessons.
- Allow one bounded, tainted "what I'd do differently" field in the run summary (§16.8).

**What we deliberately don't take.**

- Reflections that change behavior beyond the run's context: no prompt edits, no new rules, no self-set thresholds. That would move the envelope (HI-5) or the content hash (§8.4).
- Scoring the agent on whether its reflections "improved" it. DEC-99 means only forward paper outcomes count.

**Open question or risk.** Market feedback is noisy. Reflections drawn from a few outcomes will mostly encode luck. Should the model see per-thesis outcomes at all before the DEC-99 window closes, given the risk of over-fitting its own style to noise?

### 10. Toolformer (Schick et al., arXiv:2302.04761, 2023-02-09)

**In one line.** A model can teach itself when to call tools: keep the self-inserted calls that lower its own prediction loss.

**What it actually found.** GPT-J (6.7B) annotated text with candidate API calls (calculator, Q&A, two search engines, translation, calendar). It kept those whose results reduced perplexity on the following tokens, and fine-tuned on them. Zero-shot results improved, often matching much larger models, without hurting language modeling.

**Why it matters for us.** Mostly historical context. It shows tool use as a learned skill, which is why today's hosted models call tools reliably. The relevant idea for us is the *filter*: a tool call is worth keeping only if it changes the outcome.

**What we do because of it.** In the regression evaluation (§8.3), track how many read calls per run were later cited as evidence. Calls never cited are wasted spend, and a high share means the tool descriptions or menus need work. That is a cheap metric from the journal.

**What we deliberately don't take.** Self-supervised fine-tuning. We don't train.

**Open question or risk.** None for v1.

### 11. CodeAct (Wang et al., arXiv:2402.01030, 2024-02-01)

**In one line.** Let the agent write executable Python as its actions, and it composes tools better than with JSON calls.

**What it actually found.** Across 17 LLMs on API-Bank and a new benchmark (M3ToolEval), code actions beat JSON or text actions by up to 20 points of success rate. Code composes calls, loops, and self-debugs. The authors fine-tuned CodeActAgent (Llama 2, Mistral) on 7k multi-turn interactions.

**Why it matters for us.** It is the strongest argument against our narrow typed tools, and we should be clear why we refuse it. A Python interpreter is a general capability: network, files, arbitrary computation, and a place to hide sizing logic. HI-4 (reads only, no code, no network) and HI-28 (queries stay inside) would be unprovable.

**What we do because of it.** Nothing positive. It confirms that "no code execution" is a deliberate trade of capability for provable bounds (§16.14). If the model needs computation (ratios, growth rates), add *deterministic* derived-data read tools, such as `fundamentals.get` with computed fields.

**What we deliberately don't take.** Code actions, a sandboxed interpreter, or "code-then-execute" plans.

**Open question or risk.** Users and investors will compare us with agents that run code. The answer is the mandate: we sell bounded action, and the bound is what code would break.

### 12. Agentic RL: Search-R1 (arXiv:2503.09516, 2025-03-12), ReTool (arXiv:2504.11536, 2025-04-15), and the Agentic RL survey (arXiv:2509.02547, 2025-09-02)

**In one line.** Outcome-reward RL over multi-turn tool use teaches models when and how to call tools, and it now drives frontier agent quality.

**What it actually found.**

- *Search-R1:* RL with masking of retrieved tokens and a simple outcome reward. It beat RAG baselines by 41% (Qwen2.5-7B) and 20% (3B) on seven QA sets.
- *ReTool:* code-interpreter RL took a 32B model to 67% on AIME in 400 steps, against 40% in 1,080 steps for text-only RL, with emergent self-correction.
- *The survey* (500+ works) recasts agent training as a POMDP and catalogs environments.

**Why it matters for us.** These explain why hosted models are good at our loop shape. They also show where the field is heading: models trained on outcome rewards in tool environments. Our environment (journaled runs with typed verdicts) would make a fine RL environment, and that is exactly the temptation to resist.

**What we do because of it.** Treat model choice as the lever: pick and pin the hosted model whose tool use is most reliable on our recorded scenarios (§8.3), and re-run the suite on each new pin.

**What we deliberately don't take.** RL or fine-tuning on our outcomes (§16.14). A P&L reward trains toward backtest noise and leakage (section E), and would make the model a moving target outside the content hash.

**Open question or risk.** Providers RL-train their models on agentic tasks we can't see. A new snapshot may change tool behavior (eagerness to call, refusal handling). That is why a model change is a new version with a regression run (§8.4).

---
## C. HybridAgent

### 13. Hybrid Reasoning Agents (Zou, Qi, Chen, Bianchi, Zhu, Lu, He, Zou; arXiv:2610.18861, 2026-10-13)

https://arxiv.org/abs/2610.18861 (abstract, HTML, and PDF read in full). Code: https://github.com/togethercomputer/HybridAgent (README read; MIT license).

**In one line.** Post-train an open-weight model to "think" in hidden-state vectors between text tool calls. It gets faster, cheaper, and somewhat more accurate, while the text channel stays monitorable.

**What it actually found.**

*Mechanism.*

- **HybridLink** is a small residual MLP, about 10M parameters for a 4B model. It maps the last-layer hidden state into the input-embedding space, so the model can feed its own hidden state back as the next input.
- Emitting `<latent>` enters latent mode: forward passes with no decoding. The mode exits when the hidden state's nearest token embedding is `</latent>`.
- Tool calls, final answers, and "key steps" stay in text.
- Multi-turn context is carried as the **KV cache itself**, because latent spans have no token form to re-encode. Appendix A proves this is lossless, but only given the same weights, settings, and random draws.

*Training.*

- **HybridSFT** turns text agent trajectories into hybrid ones, choosing which steps go latent by LLM-judge labels and the model's entropy. It then trains format, then HybridLink alignment, then a two-pass "self-conditioned" step in which the model learns from its own latents.
- **HybridRL** is GRPO-style. Only sampled text positions carry likelihoods, and they prove that this suffices. "Exact rollout replay" feeds the *recorded* latent vectors back unchanged when computing the update. The reward is correctness, minus a format penalty, plus a multi-tool bonus.
- The backbones are Qwen3 4B, 8B, and 14B. The README assumes 8 to 16 GPUs for SFT and 32 for RL.

*Results.*

- Eleven benchmarks: GAIA, WebWalker, HLE, SealQA, DSQA, AIME25/26, MATH500, GPQA-D, MedQA, and MuSiQue.
- The headline: +10.4% over matched text RL, 35.5% to 54.7% fewer tokens, 1.8× to 2.6× faster, and 2.7× to 3.7× cheaper.
- 14B on GAIA averages 52.4, against 43.7 for ARPO at 14B and 37.9 for GPT-5.6-Luna in their table.
- The causal tests are solid. Swapping in another problem's latents cuts MATH500 accuracy by about 45%, and shuffling them costs about 14%, so the latents do real work.

*Inspectability.*

- Appendix F defines "readability" as the average, over latent steps, of the maximum cosine similarity to any vocabulary embedding, compared against Gaussian noise. "Specificity" is word overlap of the nearest-token readout with the question.
- Table 7, the auditability study, *hides the latents* and asks whether the visible text still lets a monitor predict correctness and a judge find the first error. It does: AUROC is within 0.022 of a text agent, and first errors are localized in 98% to 99% of cases. Note this is on MATH500 and AIME only.
- Section 8 says the latents are "readable solely through the provider-owned HybridLink", and pitches that as protecting a provider's proprietary reasoning from clients.

**Why it matters for us.**

*Audit.* Our audit model is "everything a decision used is a journaled text record, and replay rebuilds every decision from records with no model" (HI-2, HI-3, HI-24). HybridAgent's latents fail that in three ways:

1. **The readout is not a record of reasoning.** A nearest-token projection with a cosine score shows that a vector points roughly toward some word. It does not show that the word is what the computation meant, and nothing in the paper measures faithfulness. "Directly inspectable" means "you can print a gloss", not "you can audit it".
2. **The paper's own study audits the text, not the latents.** Its good news is that the text channel stays useful. That supports our rule that text actions and records are the audit surface. It says nothing in favor of latent audit.
3. **Provider-only readability is the opposite of what we need.** An owner, a reviewer, or a regulator must be able to see what we saw. A trace only the provider can read is the same problem as hidden reasoning tokens in today's hosted models.

*The KV cache as memory.* This breaks journal-before-use. We could not run our canary scan over it (HI-11, no credentials or account figures in any turn), screen it for licensed text (HI-23), or replay it without the exact weights and GPU kernels. Our replay folds recorded outputs, and that is the right design whatever the architecture.

*Hosting.* We call hosted models through OpenRouter. HybridAgent needs open weights we host and post-train ourselves, which we don't do and which would need a new forward paper evaluation per content hash (DEC-431 item 16).

*What is genuinely aligned.*

- **The text/latent split mirrors ours.** Think freely; act only through text tools that something outside the model checks. Their design and ours put every consequential step in the text channel.
- **"Exact rollout replay" is our INF-10 in another field.** They also found that the only sound replay records what happened and feeds it back, rather than regenerating it.
- **Efficiency shows that reasoning cost is elastic.** Under our $50 deployment cap, cost per run matters. Hosted reasoning effort is the knob we have.

**What we do because of it.**

- **§16.14:** latent reasoning is out while we use hosted models, and out as audit evidence in any case. Hidden reasoning is never evidence; only text actions and records count.
- **§16.7:** pin and record the reasoning mode and effort per turn, and record whether the provider used hidden reasoning tokens. The transcript view then says plainly that the internal reasoning isn't visible, rather than implying the `note` text is the reasoning. (That wording is compliance text, so it goes to the founder.)
- **Keep the text checkpoints mandatory.** Every proposal is a typed text tool call, judged by the harness, and every owner-facing explanation is a text record (the explain run). This is the "text mode for tool calls and human-facing checkpoints" their section 8 recommends, enforced by schema rather than learned.
- **Use their monitorability idea later, carefully.** Their weak monitor (GPT-4o-mini) predicted correctness from visible traces with AUROC around 0.84. A downgrade-only monitor over our transcripts (§16.12) is the same idea. It must only move AUTO to ASK, never approve, and its output is journaled.

**What we deliberately don't take.**

- **Never, for v1:** KV-cache memory, latent working memory, and latent agent-to-agent messages. These break HI-2, HI-11, HI-23, and HI-3.
- **Never:** HybridRL or any RL on our outcomes (§16.14).
- **Later, only if we self-host open weights:** a hybrid model for cheaper ideation runs, on four conditions. Latent spans only between journaled text tool calls. The latent vectors saved as artifacts, so a regulator request can be met. A full new evaluation. And a pinned, digest-identified HybridLink adapter as part of the identity (INF-2 already covers `weights_digest` and quantization).

**Open question or risk.**

1. Hosted reasoning models already hide or summarize their reasoning. HybridAgent just makes the hiding architectural. Does our owner-facing promise ("every step your agent took") need rewording now? I think yes: "every read, note, proposal, and decision", not "every thought".
2. If a future pinned hosted model uses latent reasoning internally, we wouldn't know. That is another reason the audit rests on inputs, outputs, and verdicts, never on reasoning.
3. Their results are on QA, math, and search with verifiable answers. Nothing suggests latent reasoning helps with market judgment, which DEC-99 says only forward paper can test.

---

## D. LLM trading and finance agents

### 14. TradingAgents (Xiao et al., arXiv:2412.20138, 2024-12-28; v7 2025-06-03)

**In one line.** A simulated trading firm of LLM agents (analysts, bull and bear researchers, a trader, a risk team) debating to a decision.

**What it actually found.** Seven roles, all prompted in ReAct style, with 11 LLM calls and 20+ tool calls per decision. On AAPL, GOOGL, and AMZN, from 1 Jan to 29 Mar 2024, it reported cumulative returns of 23% to 27% and Sharpe ratios of 5.6 to 8.2. The authors note the Sharpe "exceeds our expected empirical range" and that they ran three months "due to intensive LLM and tool use". The window falls inside the models' training data.

**Why it matters for us.** It is the most-cited open-source template, and founders and users will ask "why not this?". Its "risk management team" is LLMs discussing exposure. Our risk management is the gate and the builder: deterministic, testable, and independent of any model (rule 1, HI-14). It also shows the cost shape: 30+ calls per decision would blow our $50 cap in days.

**What we do because of it.** §16.14: no bull/bear debate as risk management. If we ever want diverse views for *ideation*, the output is still one thesis per instrument through `propose_thesis`, judged and sized like any other.

**What we deliberately don't take.**

- The multi-agent structure as a safety feature. Multiple samples of correlated models are not independent checks.
- The model choosing buy, sell, or hold and quantity directly.
- Any backtested return as evidence.

**Open question or risk.** Marketing pressure to show "our agents debate". If we ever show debate, it must be labeled research text, never a risk control.

### 15. FinMem (Yu et al., arXiv:2311.13743, 2023-11-23)

**In one line.** An LLM trader with a persona and layered memory (shallow, intermediate, and deep) that decays by recency and importance.

**What it actually found.** Three modules: profiling (the trader's character and risk appetite), layered memory, and decision. Reported backtests on a few stocks beat algorithmic agents, and tuning the "perceptual span" and character raised returns further. Two independent re-tests undercut this:

- **FINSABER (source 18):** FinMem had *negative alpha in every scenario* over longer windows, and two extra months changed its reported results drastically.
- **Profit Mirage (source 24):** FinMem's total return decayed by 71.85% after the model's cutoff, the worst of the frameworks tested.

**Why it matters for us.** Layered memory is a reasonable *structure* for `memory.theses` and `memory.runs`. Recent events weigh most, and older theses stay summarized. But FinMem's "character" design is a warning: tuning a persona's risk appetite is calibration of behavior on backtests. We forbid that (no calibration in v1), and risk appetite belongs in the envelope.

**What we do because of it.** Memory retrieval by recency and relevance, with platform-computed outcomes (§16.8). Risk appetite comes only from the mandate, and `mandate.read` shows it.

**What we deliberately don't take.** Persona tuning, self-evolving knowledge, and its performance claims.

**Open question or risk.** The owner's strategy brief can read like a persona ("be aggressive"). The validator should warn where brief text implies risk beyond the envelope (ADR-0005 §7 already says so). It should also never be "tuned".

### 16. FinAgent (Zhang et al., arXiv:2402.18485, 2024-02-28)

**In one line.** A multimodal (numbers, text, and K-line charts) tool-augmented trading agent with two-level reflection.

**What it actually found.** It reported over 36% average profit improvement against nine baselines on six stock and crypto datasets, including a 92.27% return on one. FINSABER's longer re-test found a "restrained, though still unskilled" strategy, with positive alpha only in one selection regime (+6.57% under momentum). Profit Mirage found its decay past the cutoff somewhat smaller than FinMem's, which the authors attribute to external tools.

**Why it matters for us.** It is weak evidence that *tool-grounded* agents leak less memorized outcome than prompt-only ones. That is mildly supportive of the agentic loop over memory-only reasoning. Charts as input are a future read-tool option.

**What we do because of it.** Nothing now. Note for later: an image read tool (a chart rendered from our bars) is deterministic and safe, and costs tokens.

**What we deliberately don't take.** The reported returns, and reflection that edits strategy.

**Open question or risk.** None new.

### 17. FinRobot (Yang et al., arXiv:2405.14767, 2024-05-23)

**In one line.** An open-source platform layering financial chain-of-thought agents, model-routing strategies, LLMOps, and multiple foundation models.

**What it actually found.** It is a platform description, with no controlled trading evaluation. Its four layers: agents with Financial CoT, algorithms that pick a model strategy per task, LLMOps and DataOps for fine-tuning, and a multi-model foundation.

**Why it matters for us.** It is a competitor-shaped reference for "an agent platform for finance". Its routing layer (choosing a model per task at runtime) is the opposite of our pinning. For us, a model is part of the content hash, and changing it is a mandate change the owner confirms (§8.4, INF-2).

**What we do because of it.** Confirm pinning per research entry. Different run kinds may pin different models (a cheaper one for explain runs), but each is fixed and recorded, never routed at runtime.

**What we deliberately don't take.** Dynamic model routing, and fine-tuning ops.

**Open question or risk.** Whether explain runs (owner-facing text, no trading effect) can use a cheaper pinned model without a mandate change. That is a reversible engineering decision, but it touches compliance wording.

### 18. FINSABER (Li et al., arXiv:2505.07078, 2025-05-11)

**In one line.** Re-test LLM timing strategies over 20 years and 100+ symbols, and their advantage mostly disappears.

**What it actually found.** Prior LLM strategies looked good because of narrow windows, few symbols, survivorship, and data snooping. Over two decades:

- LLM strategies underperform buy-and-hold: too conservative in bull markets, too aggressive in bear markets.
- FinMem shows negative alpha everywhere. FinAgent is unskilled except under momentum selection.
- Extending FinMem's original test by two months changes its results materially.

The authors conclude that trend detection and regime-aware *risk controls* matter more than framework complexity.

**Why it matters for us.**

- *DEC-99:* this is direct evidence for it. Backtests prove mechanics, not judgment.
- *Compliance:* it supports "no performance claims".
- *Our split:* the call for regime-aware risk controls is our split exactly. Deterministic limits and the drawdown ladder do the risk work; the model only supplies ideas.

**What we do because of it.**

- §16.10: scenario runs with a live model are graded on mechanics only.
- In demo and sales material, never show backtest returns of the agent.
- Keep the agent's influence on size bounded by the builder and the limits, so a bear-market over-aggression has a cap.

**What we deliberately don't take.** Its own FINSABER framework as a quality gate. It is still a backtest.

**Open question or risk.** If the research agent is "unskilled" on average, the product value has to rest on the harness (bounded, explained, journaled autonomy), not on alpha. That is a positioning question for the founder.

### 19. StockBench (arXiv:2510.02209, 2025-10-02)

**In one line.** On a contamination-free four-month window, most LLM agents fail to beat equal-weight buy-and-hold.

**What it actually found.**

- The universe is the 20 heaviest DJIA stocks. Data runs 3 Mar to 30 Jun 2025, after the evaluated models' cutoffs.
- Each day the agents get prices, fundamentals, and news, and output buy, sell, or hold decisions through a four-step workflow (overview, analysis, decision, validation).
- Each model was run three times with different seeds, and the average reported.
- Most models fail to beat equal-weight buy-and-hold on return and on risk-adjusted return. Strong financial-QA scores don't transfer. Workflow errors matter.

**Why it matters for us.** It confirms that a *post-cutoff window* is the minimum for any quality signal. It is close to our forward paper idea, though four months of one regime is still short. Its three-seed averaging is a pass@k-style view; we need pass^k for safety.

**What we do because of it.** Keep DEC-99's forward window and pre-registered metric (DEC-122). For internal sanity checks on recorded post-cutoff data, use an equal-weight buy-and-hold baseline, which the scorecards (§8.2) already include.

**What we deliberately don't take.** Its decision format (the model picks the trade).

**Open question or risk.** "Post-cutoff" depends on the true effective cutoff, which can differ from the vendor's date (see HindsightBench, source 27).

### 20. LiveTradeBench (arXiv:2511.03628, 2025-11-05)

**In one line.** Fifty days of live trading by 21 LLMs on US stocks and Polymarket. Arena ranking doesn't predict trading.

**What it actually found.**

- Live price and news streams, so no backtest leakage. Agents output *percentage allocations* each step.
- High LMArena scores didn't imply better outcomes. Models showed stable "portfolio styles" (risk appetite).
- A rolling-delay test suggested decisions weren't random.
- Large reasoning models conferred no advantage.

**Why it matters for us.** It shows that model "personality" (risk appetite) is a stable property of a pinned model. If the model sized positions, risk would be whatever the model's temperament is. Ours can't: allocations come from the builder and the envelope (HI-1). It also shows that live evaluation is the honest form, which matches DEC-99.

**What we do because of it.**

- §16.14: the model never outputs weights or allocations.
- When choosing which model to pin, look at the *distribution* of conviction and confidence it reports on recorded scenarios. Uniformly high confidence is a known bad-model case (spec §9), and the builder clips it, but a calmer model means fewer clipped or refused proposals.

**What we deliberately don't take.** Allocations as model output.

**Open question or risk.** "Reasoning models confer no advantage" questions paying for high reasoning effort. Measure it on our scenarios before pinning effort high (§16.7).

### 21. Alpha Arena Season 1 (Nof1, Oct to early Nov 2025)

https://nof1.ai. **Primary results not accessed:** only the homepage loaded. Figures are from secondary coverage (forklog.com, gncrypto.news).

**In one line.** Six frontier models each traded $10k of real crypto perpetuals for about 17 days, choosing size and leverage. Most lost money.

**What it actually found (secondary).** On Hyperliquid, with a stop-loss and take-profit required per trade:

- Qwen3 Max was reported at about +22% and DeepSeek at about +5%.
- Claude Sonnet 4.5 was about -31%, Grok 4 -45%, Gemini 2.5 Pro -57%, and GPT-5 -63%.
- The founder reported stable, persistent model "biases", in effect personalities.
- Nof1's homepage now says current LLMs are "shockingly bad at navigating financial markets" and that markets are non-stationary.

**Why it matters for us.** It is the public demonstration of what we forbid: the model picks size and leverage, on one regime, for weeks, with real money. Losses of 30% to 60% in 17 days show what happens when risk depends on model temperament. It is the best cautionary example for our "agentic where it thinks, deterministic where it touches money" pitch.

**What we do because of it.** Use it (carefully, with verified figures) to explain rule 4 to users and investors: the model proposes, deterministic code sizes. Confirms §16.14.

**What we deliberately don't take.** Leaderboard framing of models' returns. One 17-day window is noise, and showing returns invites performance claims.

**Open question or risk.** The figures aren't verified from the primary source. Don't quote numbers externally until they are.

### 22. "The Self Driving Portfolio" (arXiv:2604.02279, 2026-04-02)

Abstract read. **Full text not accessed** (the HTML returned 404).

**In one line.** Forty-four agents produce capital-market assumptions and portfolios, vote on each other's work, and a meta-agent rewrites agent code and prompts based on realized returns.

**What it actually found (abstract).** A strategic asset-allocation pipeline using 21 construction methods, a researcher agent that invents new methods, and a meta-agent that compares forecasts with outcomes and *rewrites prompts and code*. The whole pipeline is governed by an Investment Policy Statement. (Secondary coverage also says it flags agent-to-agent interaction as a leakage channel. That is unverified.)

**Why it matters for us.** Its Investment Policy Statement is our mandate, and the framing is the same: owner-written policy constrains autonomous agents. But the meta-agent that rewrites prompts is self-modification of the strategy. For us that would move the content hash (and so the signal-model version) without the owner's confirmation, which violates rule 11 and §8.4.

**What we do because of it.** It validates "the mandate is the contract" as an institutional idea. It also confirms §16.14: no self-rewriting prompts.

**What we deliberately don't take.** The meta-agent, voting as a control, and agents writing code.

**Open question or risk.** Institutional buyers may expect self-improvement. Our answer is versioned change with owner confirmation and a regression run (§8.4).

---

## E. Look-ahead and leakage

### 23. Glasserman and Lin, "Assessing Look-Ahead Bias in Stock Return Predictions Generated by GPT Sentiment Analysis" (arXiv:2309.17322, 2023-09-29)

**In one line.** Strip company names from headlines: in-sample, anonymized headlines *beat* named ones, so the model's general knowledge of the company distracts more than it leaks.

**What it actually found.** It ran sentiment trading strategies on news headlines, original versus anonymized. In-sample, anonymized did better, especially for large companies the model knows well. That is a "distraction effect" bigger than look-ahead. Out-of-sample, look-ahead is not a concern but distraction remains.

**Why it matters for us.** Our research agent sees names (it must resolve instruments). Distraction means it may lean on what it "knows" about a big company rather than the evidence in front of it. That isn't a safety problem (the builder and gate bind), but it is a quality and explainability one, since the thesis cites evidence the model didn't really use.

**What we do because of it.** For the evaluation suite (§16.10), add an anonymized variant: same items with names and tickers masked. A large gap between named and anonymized outputs flags reliance on parametric knowledge. That is cheap with recorded scenarios.

**What we deliberately don't take.** Anonymizing in production. The agent must know what it is proposing.

**Open question or risk.** Should `forbidden_language` or the explain run catch theses whose text cites knowledge not in any retrieved item ("as everyone knows, X will…")? That is hard to check deterministically.

### 24. Profit Mirage (Li et al., arXiv:2510.07920, 2025-10-09)

**In one line.** LLM trading agents' backtested returns collapse after the model's training cutoff.

**What it actually found.** Using GPT-4o (cutoff October 2023) as the backbone for seven frameworks:

- Sharpe decayed by 51% (QuantAgent) to 62% (FinCON) after the cutoff.
- Total return decayed by 50% (TradingAgents) to 72% (FinMem).
- Tool-using agents degraded somewhat less.

They release FinLake-Bench and propose FactFin, which uses counterfactual perturbations.

**Why it matters for us.** It is the cleanest number behind DEC-99, and behind the compliance rule that we never show backtested agent performance. A demo built on a recorded historical scenario with a live model *is* a backtest inside the model's knowledge window.

**What we do because of it.**

- §16.10: the demo scenario is graded and presented on mechanics (what was proposed, admitted, refused, and why), never on P&L.
- Any demo P&L shown is labeled paper and mechanics, or omitted (a founder call).

**What we deliberately don't take.** FactFin. It uses strategy code generation and MCTS, which is code execution (HI-4).

**Open question or risk.** The demo scenario dates. If they are inside the pinned model's knowledge window, the model may "predict" what it remembers, and the demo looks better than reality. Prefer post-cutoff dates for the demo, or say so.

### 25. Look-Ahead-Bench (Benhenda, arXiv:2601.13770, 2026-01-20)

**In one line.** A benchmark measuring look-ahead bias through alpha decay across market regimes, contrasting standard LLMs with "point-in-time" models trained only on data up to a date.

**What it actually found.** Llama 3.1 (8B, 70B) and DeepSeek 3.2 show significant look-ahead bias, measured as alpha decay across regimes. A family of point-in-time models (Pitinf-Small, -Medium, -Large) generalizes better as it scales.

**Why it matters for us.** Point-in-time models are the only clean way to backtest LLM judgment. We use hosted frontier models, which are not point-in-time. So no backtest of our agent's judgment can be clean, and DEC-99's forward-only rule is the only option until such models exist at quality.

**What we do because of it.** Note point-in-time models as a "later" option for the evaluation suite (open weights, self-hosted), never for production ideation. Confirms DEC-99.

**What we deliberately don't take.** Treating its benchmark as an admission test for our pinned model. It measures bias in a workflow unlike ours.

**Open question or risk.** The author is affiliated with the point-in-time vendor (PiT-Inference). Weigh it accordingly.

### 26. FinCAD, "Summoning the Oracle to Slay It" (Li, Wang, Ma; arXiv:2605.24564, 2026-05-23)

**In one line.** Reduce memorized-outcome influence at inference time with context-aware decoding scaled per entity and date.

**What it actually found.** On five 7B to 14B LLMs and five mega-caps, in-sample return corrections reached -67.1%. That shows how much of the in-sample return was memory. Out-of-sample returns barely moved. FinCAD needs logits and decoding control.

**Why it matters for us.** It quantifies the inflation: two-thirds of in-sample returns can be memorized. It needs token-level decoding control, which hosted APIs through OpenRouter don't give us.

**What we do because of it.** Nothing in the product. It is evidence for §16.10 and DEC-99.

**What we deliberately don't take.** The method itself (it needs open weights and decoding access).

**Open question or risk.** None new.

### 27. HindsightBench (arXiv:2607.18867, 2026-07-21)

**In one line.** A cheap black-box audit shows that 2026-generation models use their knowledge of a period as soon as they see its date, and their real cutoffs differ from vendor claims.

**What it actually found.**

- *Design:* a four-arm date matrix (revealed, date-only, masked, and "transplanted" to another date), plus probes for date recovery and outcome recall. Tested on 15 models from seven vendors over a 258-node macro panel.
- *The date-trigger reflex:* it is present in every 2026-generation model tested, absent in 2024 open-weight models, and switches on within one lineage (Qwen3 to Qwen3.6). It tracks training recency, not scale.
- *Cutoffs:* effective cutoffs span 22 months across vendors and precede vendor-reported dates by up to eight months.
- *Serving:* results change with how the model is served. Serving an FP8-referenced model in BF16 broke the trigger estimate's stability, and a provider-locked reasoning regime made one probe non-convergent. The protocol therefore pins quantization and thinking regime.

**Why it matters for us.**

- *Look-ahead:* our HI-13 controls knowledge time of *retrieved* data, and every read filters on the cut-off. But the model's *parametric* knowledge isn't filtered. Give a 2026 model a date in 2025, and it uses what it knows happened next. Every recorded-scenario replay with a live model is contaminated.
- *Pinning:* the serving finding is an INF-2 point. The same named model behaves differently by quantization and reasoning mode, so both are part of identity.

**What we do because of it.**

- §16.7: pin and record quantization (already in INF-2) *and* reasoning mode and effort, with OpenRouter provider fallbacks off.
- §16.10: scenario grading on mechanics only.
- Add a HindsightBench-style probe to the gateway's model evaluation gate (inference spec §4.3) after the demo. It is cheap, needs no logprobs, and estimates the effective cutoff so that "post-cutoff" scenario windows are chosen honestly.

**What we deliberately don't take.** Hiding dates from the model in production. The cut-off and `as_of` must be real.

**Open question or risk.** Does HI-13's owner-facing promise ("no look-ahead") need qualifying? Retrieved data is point-in-time; the model's memory isn't. In forward paper it is moot (the future isn't in the model). In anything historical it isn't.

### 28. "Temporal Leakage in LLM Backtesting: Measurement, Validation, and Adjusted Scores" (arXiv:2608.02985, 2026-08-04)

**In one line.** Comparing scores before and after the cutoff can't detect leakage. You need an outside reference, such as a known cutoff or a matched clean control.

**What it actually found.**

- Four flagship models "fail" the before/after check even on questions resolved after their cutoffs, which they can't have memorized. Models legitimately know more about times near their cutoff, so recency mimics leakage.
- The authors prove that no passive backtest can separate leakage from skill.
- With a matched clean control, they estimate a leakage-adjusted score, validated by planting leakage in twin models.
- Leakage concentrates on outcomes that surprised the crowd and were well covered in training.

**Why it matters for us.** It kills the tempting middle path: "we'll backtest, but only on post-cutoff data, and compare". Even that is ambiguous without a control. Only forward paper (DEC-99) is clean by construction. Our regression evaluation (§8.3) correctly measures mechanics only.

**What we do because of it.** Write into the spec's evaluation section (§16.10 and §8.3) that historical quality numbers are never evidence, *including* post-cutoff ones, unless measured against a matched control. Cite this paper.

**What we deliberately don't take.** Building their estimator now. It is a research project.

**Open question or risk.** "Surprising, well-covered outcomes" are exactly the dramatic news cases a demo would choose. Avoid demo scenarios built around famous events.

---

## F. Prompt-injection defenses

### 29. Simon Willison, "The Dual LLM pattern for building AI assistants that can resist prompt injection" (2023-04-25)

https://simonwillison.net/2023/Apr/25/dual-llm-pattern/

**In one line.** A privileged model with tools never sees untrusted text. A quarantined model without tools processes it into opaque variables that controller code passes around.

**What it actually found.** It is a design essay, not an experiment. It names confused-deputy and exfiltration attacks, proposes the privileged/quarantined split, and warns that "you're still vulnerable to social engineering": the quarantined output shown to a user can still persuade the user. It also warns against chaining. A 2025 update points to CaMeL as fixing its flaws.

**Why it matters for us.**

- *Order path:* we already have a stronger version. The model's actions can't touch money; they are proposals judged by code.
- *The owner:* Willison's social-engineering warning is our open channel. `ask_owner` text, approval-card explanations, and `propose_mandate_change` reasons are model-written text that injected content may have shaped, and they are shown to the owner, who *can* act.

**What we do because of it.**

- §16.3: provenance on every owner-facing text ("written after reading: [result ids]").
- `ask_owner` options are typed choices.
- The model text on cards is quoted and labeled as research-agent text (DEC-126).
- §16.2: news-triggered monitoring runs can't call `propose_mandate_change` or `propose_thesis`.

**What we deliberately don't take.** A full two-model split now. It doubles calls and cost, and our proposal-only design already removes the confused-deputy risk on the order path.

**Open question or risk.** The owner is the remaining deputy. How many mandate-change proposals per month (HI-27) can a determined injection push before the owner says yes from fatigue? See CaMeL's user-fatigue point (source 32).

### 30. Spotlighting (Hines et al., Microsoft, arXiv:2403.14720, 2024-03-20)

**In one line.** Transform untrusted input so the model can always tell it apart from instructions: delimit it, interleave a marker token through it, or encode it.

**What it actually found.**

- Three variants: delimiting, datamarking (a special token interleaved through the text), and encoding (for example base64).
- On GPT-family models, datamarking and encoding cut attack success from over 50% to under 2%, with little task loss.
- The authors recommend randomized, dynamic markers, since static whitespace marking is easy to evade.
- They say plainly that it "is not perfect", likening it to in-band signaling.

**Why it matters for us.** Every news, filing, and fundamentals result is untrusted text entering a turn. Spotlighting is cheap and lives in the pinned template and the readers.

**What we do because of it.** §16.5: untrusted tool results are delimited and datamarked, with a per-run random marker recorded in the journal so replay reproduces the exact prompt. It is a defense in depth only, never a control the invariants rest on.

**What we deliberately don't take.** Encoding (base64). It costs tokens, degrades comprehension on long filings, and complicates the licensed-text rules.

**Open question or risk.** The marker must be in the recorded prompt artifact, or replay of the prompt diverges. Recording it is easy if we remember to (the journal spec's prompt record).

### 31. AgentDojo (Debenedetti et al., ETH Zurich, arXiv:2406.13352, 2024-06-19)

**In one line.** A live benchmark of 97 tool-using tasks and 629 injection cases. Utility and security are scored from environment state, not by an LLM judge.

**What it actually found.**

- Frontier models fail many tasks even without attacks.
- Prompt-level defenses help partly.
- The standout defense was the simple **tool filter**: before seeing any untrusted data, the model restricts itself to the tools the task needs. It cut targeted attack success to **7.5%**. It works because most user tasks need read access, while attacks need write access.
- It fails where the needed tools are themselves the attack vector.

**Why it matters for us.** This is the direct evidence for §16.2. Our version is stronger than theirs: the *trigger*, not the model, chooses the menu, and it is fixed in code. A news-triggered monitoring run needs reads plus risk-reducing actions, and an attacker needs `propose_thesis` or `propose_mandate_change`. Those aren't in the schema.

**What we do because of it.**

- §16.2 (confirmed).
- Model our injection bench (E19-6, HI-10) on AgentDojo's format: grade from journal state (proposals, admissions, limit breaches), not from the model's words.
- §16.9: include adaptive attacks.

**What we deliberately don't take.** Letting the model choose its own tool filter. Ours is set by the trigger.

**Open question or risk.** The remaining attack surface is inside the allowed menu. For a monitoring run, that is churn through `invalidate` and `propose_exit`. Invalidations only remove, exits are paced by conduct controls, and re-entry waits for the cooldown. Our bench must cover that churn case.

### 32. CaMeL (Debenedetti et al., Google/DeepMind/ETH, arXiv:2503.18813, 2025-03-24)

**In one line.** Extract the control flow from the trusted query, let untrusted data flow only as values with capabilities attached, and enforce security policies at each tool call.

**What it actually found.**

- A privileged model writes a restricted Python program from the user's query. A quarantined model parses untrusted data into values. An interpreter tracks data provenance and enforces policies.
- On AgentDojo it solves **77% of tasks with provable security**, against 84% undefended. It costs **2.82× input and 2.73× output tokens** against native tool calling (Claude 3.5 Sonnet, under attack).
- The authors admit side channels (leaking a bit through exceptions, and indirect dependencies).
- They also note **user fatigue**: too many security prompts and users approve malicious actions.

**Why it matters for us.** Its key idea, that untrusted data may never change *which* actions are possible, is what §16.2 does at the run level. Its capability tracking is what §16.3 does lightly: provenance on actions and owner-facing text. Its user-fatigue warning is our ASK flood. Too many asks and the owner rubber-stamps them, which weakens the autonomy policy.

**What we do because of it.**

- §16.2 and §16.3 (confirmed).
- §16.6: a repeated denial ends the run.
- HI-27 rate limits on asks and mandate-change proposals.
- Delegations (ADR-0003) as the owner's sanctioned way to reduce asks, so fatigue is answered by policy, not by approving blindly.

**What we deliberately don't take.** The full interpreter and code-generating planner. It is code execution (HI-4), at 2.8× token cost. Our proposal-only action space gets most of the benefit for free.

**Open question or risk.** Side channels in our system: could an injection make the agent leak what it holds through *which* reads it calls? HI-28 says queries stay inside the deployment. That is true for external leaks. But the read pattern is journaled, and owners see it, which isn't a leak.

### 33. "Design Patterns for Securing LLM Agents against Prompt Injections" (Beurer-Kellner et al., arXiv:2506.08837, 2025-06-10)

**In one line.** Six patterns that give provable injection resistance by limiting what untrusted input can cause.

**What it actually found.** The six patterns:

1. **Action-selector:** the model only picks from fixed actions, with no feedback from them.
2. **Plan-then-execute:** the plan is fixed before untrusted data arrives, as a form of control-flow integrity.
3. **LLM map-reduce:** isolated sub-calls over each untrusted item.
4. **Dual LLM.**
5. **Code-then-execute.**
6. **Context-minimization:** remove the user's prompt from context once it has been turned into a query.

They work through case studies (an OS assistant, a SQL agent, and others). The principle: once an agent has read untrusted input, it must be impossible for that input to trigger consequential actions.

**Why it matters for us.**

- *Monitoring runs:* plan-then-execute at the run level is §16.2. The trigger fixes the menu, and the news item can't widen it.
- *Owner runs:* context-minimization fits. The owner's message is turned into a typed request, shown and confirmed (HI-26), and the confirmed typed request is what proceeds, not the raw chat.
- *Read results:* map-reduce fits big filings. Extract typed fields per item in isolation, then reason over the fields.

**What we do because of it.**

- §16.2 (confirmed).
- HI-26 is implemented as context-minimization: the confirmed typed request, not the chat text, drives the action.
- After the demo, consider map-reduce extraction for long filings as a *deterministic-schema* read tool, journaled per item.

**What we deliberately don't take.** Code-then-execute (HI-4).

**Open question or risk.** Map-reduce adds a model call per item, which costs budget. Is it cheaper than spotlighting plus bounded results? Measure it.

---

## G. Evaluation

### 34. τ-bench (Yao et al., Sierra, arXiv:2406.12045, 2024-06-17)

**In one line.** Simulated users, domain policies, and tools. Grade by final database state, and measure reliability with pass^k.

**What it actually found.**

- Retail and airline domains, with an LLM-simulated user and policy documents the agent must follow.
- Success means the database end state matches the annotated goal.
- gpt-4o with function calling scored pass^1 of about 61% (retail) and 35% (airline). Retail pass^8 fell to about 25%.
- Failures were wrong decisions, wrong arguments, and partially solved compound requests.

**Why it matters for us.** Our domain policy is the mandate, and our database state is the journal. "Grade by end state" means grading a run by what was proposed, admitted, refused, and changed, never by the transcript's prose. That is ADR-0005 §12, confirmed. pass^k is the right reliability measure for *safety*.

**What we do because of it.**

- §16.9: safety properties hold in all k trials (pass^k). Quality is reported at pass@k.
- After the demo, build an owner-simulator for desk runs. Owner messages are generated, and the grade is whether HI-26 held (every action seen and confirmed) and whether the typed request matched the intent.

**What we deliberately don't take.** LLM-judged success for anything safety-related.

**Open question or risk.** With live models, k trials cost k times as much. Under the $50 cap, most safety k-trials must use recorded or double models, with live trials reserved for the injection bench.

### 35. τ²-bench (Barres et al., arXiv:2506.07982, 2025-06-09)

**In one line.** Dual control: the user also acts on the shared world, and agents do much worse when they must guide a user.

**What it actually found.** A telecom domain modeled as a Dec-POMDP, where both agent and user have tools. Tasks are generated compositionally, and the user simulator is constrained by tools and state. Performance drops significantly from no-user to dual-control. Ablations separate reasoning errors from communication errors.

**Why it matters for us.** The desk is dual control. The owner approves, rejects, delegates, and changes the mandate, while the agent proposes and explains. Most of our risk of *owner* error comes from the communication step: the approval card and the explanation. That matches τ²'s finding that guiding the user is the hard part.

**What we do because of it.** In the owner-simulator (after the demo), include owners who misread cards, approve fast, or ask the agent to "just do it". Grade on HI-26, on the step-up holding, and on the agent never pressing for a riskier mandate beyond HI-27's limits.

**What we deliberately don't take.** Its telecom specifics.

**Open question or risk.** Card wording is compliance text (founder and counsel). Simulator results could pressure wording choices, so keep them as input to counsel, not decisions.

### 36. Anthropic, "Demystifying evals for AI agents" (2026-01-09)

https://www.anthropic.com/engineering/demystifying-evals-for-ai-agents

**In one line.** Grade outcomes, read transcripts, isolate trials, and pick pass@k or pass^k by whether one success or consistency matters.

**What it actually found.** Practitioner guidance:

- *Graders:* code (fast, brittle), model (flexible, needs calibration), and human (gold standard, used to calibrate the others).
- *Kinds of eval:* capability evals start low; regression evals sit near 100%.
- *Grading:* grade the result, not the path.
- *Isolation:* in one case Claude read git history left from earlier trials, making trials non-independent.
- *Arithmetic:* 75% per trial is about 42% at pass^3.
- *Debugging:* a 0% pass rate usually means a broken task or grader.
- "Read the transcripts!"

**Why it matters for us.** It is the method for §8.3 regression evaluation and §16.9 safety testing. Its isolation warning applies directly. Our memory tools (`memory.runs`, `records.query`) mean a later trial can read an earlier trial's records unless each trial gets a fresh agent stream. And the transcript view we're building for owners is also our main debugging tool.

**What we do because of it.**

- Each eval trial runs in its own agent stream and workspace, with no shared memory.
- Safety evals are regression evals at 100% pass^k.
- Code graders read the journal.
- Model graders only for wording quality of explanations, calibrated by a human, never for anything that gates.
- Read a sample of transcripts at every version change (§8.4).

**What we deliberately don't take.** Path-based grading of tool sequences. Our safety properties are about outcomes (no limit breach, no un-dry-run intent).

**Open question or risk.** Saturation. A 100%-passing safety suite proves only what it covers, so the adaptive attacker (§16.9) has to keep evolving.

---

## Patterns across the papers

1. **Put the boundary in code, not in the model.** CaMeL, the design patterns paper, AgentDojo's tool filter, and Claude Code's permission docs ("not a security boundary"; use the sandbox) all say the same thing. Our gate, builder, and schema-level tool menus are that boundary. Spotlighting and prompts are only depth.

2. **Fix the options before reading untrusted text.** The biggest single win in the injection literature (AgentDojo's 7.5%, plan-then-execute, CaMeL's control flow from the trusted query) is deciding what actions are possible before the attacker's text is seen. For us that is the trigger-set tool menu (§16.2).

3. **The owner is the last deputy.** On the order path we are safe by construction. The residual channel is model text that persuades a human: Willison's social engineering, CaMeL's user fatigue, τ²'s guidance gap. Provenance (§16.3), typed asks, rate limits, delegations, and step-up are how we defend it.

4. **Model-chosen inputs need platform-chosen facts.** Once the model picks its own reads, it can cherry-pick (Glasserman's distraction, Profit Mirage's memorized narratives). Corroboration, timing, and asset class must come from the platform's own reads (§16.4, HI-18).

5. **Backtested LLM judgment is not evidence, and the field keeps proving it.** FINSABER, Profit Mirage, StockBench, Temporal Leakage, HindsightBench, and FinCAD all agree. The only clean test is forward and live (DEC-99, LiveTradeBench). Every demo and every evaluation must grade mechanics, not returns (§16.10).

6. **Record outputs; never re-derive them.** HybridAgent's exact rollout replay, HindsightBench's serving sensitivity, and the hidden reasoning in hosted models all say that a model call isn't reproducible from its name. Pin everything that changes behavior (quantization, reasoning mode, provider) and replay from records (§16.7, INF-10).

7. **Visible reasoning is not the reasoning.** HybridAgent hides it in vectors, hosted models hide or summarize it, and even ReAct traces can be post-hoc. The audit surface is reads, proposals, verdicts, and owner actions. The transcript view should promise that, and no more.

8. **Reliability beats peak.** τ-bench's pass^k, Anthropic's evals post, and Alpha Arena's 17-day swings all show agents are inconsistent. For a product that touches money, the safety properties must hold every time (pass^k at 100%), and quality claims must stay off the screen.
