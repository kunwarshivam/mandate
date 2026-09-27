# Harness Engineering

| | |
|---|---|
| **Owner** | Product |
| **Status** | Research note, 2026-09-27. Input for [DEC-149](../project/04-decision-log.md#decisions) (the harness and the platform) and for the proposed [enterprise harness stories](../project/06-backlog-v1.md#enterprise-harness-proposed-dec-149). Not a PRD change, and not legal advice |
| **Method** | The canonical posts were read directly, or from an archive where the live page was blocked (OpenAI's post: Wayback snapshot `web.archive.org/web/20260924171239/https://openai.com/index/harness-engineering/`, whose six quoted passages the coordinator re-checked verbatim on 2026-09-27). Repository READMEs and docs were read through `gh api`. Star counts and last-push dates come from `gh api repos/...` on 2026-09-27. Web search was exhausted, so non-GitHub writing such as vendor enterprise pages and analyst reports was **not checked**. Inferences are marked **[inference]** |
| **Related** | [Strategy options](10-strategy-options.md#positioning-harness-and-platform-dec-149), [Competitive landscape](03-competitive-landscape.md), [Compliance](08-compliance-and-regulatory.md), [ADR-0002](../adr/0002-autonomous-ideation-and-retail.md) |

> Compliance, SOC 2, and regulatory points here summarize what vendors publish about their own
> tools. They are not legal advice and draw no legal conclusion. Legal and compliance text is
> reserved for the founder and counsel ([DEC-79](../project/04-decision-log.md#decisions)). This
> note involved no accounts, outreach, broker tools, or orders.

## Summary

1. "Harness engineering" names two things: the **inner harness** around a model (loop, tools,
   context, sandbox, permissions, hooks) and the **outer harness** a team builds so agents do reliable
   work (the repository as system of record, mechanical invariants, evals, feedback that compounds).
2. Generic harnesses and MCP gateways enforce **stateless, per-call** rules on tool names and
   arguments. None of those read here models positions, P&L, drawdown, settlement, or budgets
   [inference].
3. No well-adopted open-source guardrail harness for **trading** agents exists. The popular trading
   agent projects are research or educational, and their risk management is LLM debate or a YAML file
   with no enforcement layer.
4. Mandate's gate, autonomy rules, journal, and executor are an inner harness for a regulated domain,
   and its `AGENTS.md` trust ladder is an outer harness. That is the basis of
   [DEC-149](../project/04-decision-log.md#decisions): the harness becomes an enterprise product, and
   the retail platform runs through it.

## 1. What "harness engineering" means

**OpenAI**, "Harness engineering: leveraging Codex in an agent-first world" (Ryan Lopopolo,
2026-02-11), <https://openai.com/index/harness-engineering/>, read from the Wayback copy because the
live page returned 403:

- "Humans steer. Agents execute."
- The team's job became "to design environments, specify intent, and build feedback loops that allow
  Codex agents to do reliable work."
- "what capability is missing, and how do we make it both legible and enforceable for the agent?"
- "By enforcing invariants, not micromanaging implementations, we let agents ship fast without
  undermining the foundation."
- Layered domains are "enforced mechanically via custom linters … and structural tests … we write the
  error messages to inject remediation instructions into agent context."
- "Human taste is captured once, then enforced continuously on every line of code."

**Lopopolo's anthology**, <https://github.com/lopopolo/harness-engineering>:

- Harness engineering "holds a chosen model and coding agent constant as a black box. It improves the
  two external levers—context and tools."
- "The worker should be able to recover intent, operate the real system, respect authority, prove the
  outcome, and leave the next run better equipped."

**Martin Fowler / Birgitta Böckeler**, "Harness Engineering for Coding Agent Users" (2026-04-02),
<https://martinfowler.com/articles/exploring-gen-ai/harness-engineering.html>:

- "Agent = Model + Harness."
- Guides (feedforward) "steer it before it acts"; sensors (feedback) "observe after the agent acts and
  help it self-correct".
- "The agent harness acts like a cybernetic governor."

**LangChain**, "The Anatomy of an Agent Harness" (Vivek Trivedy, 2026-03-10),
<https://www.langchain.com/blog/the-anatomy-of-an-agent-harness>:

- "A harness is every piece of code, configuration, and execution logic that isn't the model itself …
  state, tool execution, feedback loops, and enforceable constraints."
- It lists prompts, tools, skills, MCP, sandbox, orchestration, and "Hooks/Middleware for
  deterministic execution."

**Anthropic**:

- "Effective harnesses for long-running agents" (2025-11-26),
  <https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents>: an initializer
  agent, `claude-progress.txt`, a feature list, one feature at a time; "It is unacceptable to remove
  or edit tests."
- "Beyond permission prompts" (2025-10-20),
  <https://www.anthropic.com/engineering/claude-code-sandboxing>: "sandboxing safely reduces
  permission prompts by 84%", through filesystem and network isolation.
- "Demystifying evals for AI agents" (2026-01-09),
  <https://www.anthropic.com/engineering/demystifying-evals-for-ai-agents>: separates an evaluation
  harness ("runs tasks concurrently, records all the steps, grades outputs") from an agent harness;
  "the outcome is whether a reservation exists in the environment's SQL database"; pass@k versus
  pass^k ("the probability that all k trials succeed").

**Synthesis [inference].**

- The **inner harness** is the loop, tools, context, memory, sandbox, permissions, and hooks.
- The **outer harness** is the repository as system of record, mechanical invariants, evals, and
  compounding feedback.
- Mandate's `AGENTS.md` trust ladder is the outer-harness idea almost word for word. The product
  itself (gate, journal, autonomy) is an inner harness aimed at a regulated domain.

## 2. The landscape

Stars and last push from `gh api repos/...` on 2026-09-27; star counts are as of 2026-09-27. "Not
recorded" means the research did not record a last-push date for that repository; "not checked" means
it says it did not check one. A dash means the research recorded nothing beyond the category.

| Category | Repository | Stars | Last push | What the research recorded |
|---|---|---|---|---|
| Teaching | walkinglabs/learn-harness-engineering | 16,403 | 2026-08-26 | A 14-lecture course: "Five subsystems: instructions, state, verification…"; teardowns of the Claude Code, Codex, Pi, and DeepSeek harnesses |
| Teaching | ai-boost/awesome-harness-engineering | 4,553 | 2026-09-24 | The best index: permissions and authorization, observability, human-in-the-loop, evals, security and sandboxing |
| Teaching | walkinglabs/awesome-harness-engineering | 4,176 | 2026-08-19 | — |
| Teaching | Picrew/awesome-agent-harness | 1,809 | 2026-09-20 | — |
| Teaching | lopopolo/harness-engineering | 2,703 | 2026-07-18 | Nonfunctional requirements as executable constraints; a source manifest with archived evidence |
| Teaching | QoderAI/better-harness | 2,342 | 2026-09-24 | Harness experiments; tagline "Delegate coding to agents. Improve the loop around them." |
| Reference harness | openai/codex | 126,755 | 2026-09-27 | Sandbox plus approvals; Starlark exec-policy rules (allow, prompt, forbidden; "the effective `decision` is the strictest severity across all matches (`forbidden` > `prompt` > `allow`)"; inline match and not_match tests) |
| Reference harness | anthropics/claude-code | 148,317 | 2026-09-26 | PreToolUse hooks return allow, deny, or ask: "exit 2 … even a JSON permissionDecision of "allow" can't override it" |
| Reference harness | anthropics/sandbox-runtime | 5,361 | 2026-09-27 | OS-level filesystem and network sandbox |
| Reference harness | anomalyco/opencode | 210,384 | 2026-09-27 | — |
| Reference harness | OpenHands/OpenHands | 89,295 | 2026-09-27 | — |
| Reference harness | aaif-goose/goose | 54,706 | 2026-09-25 | — |
| Reference harness | langchain-ai/deepagents | 29,811 | 2026-09-27 | "The batteries-included agent harness" |
| Guardrails and policy | microsoft/agent-governance-toolkit | 6,349 | 2026-09-26 | Deterministic interception of every tool call (YAML, OPA, or Cedar), `require_approval`, a Merkle audit, a kill switch; OWASP Agentic Top-10, NIST, EU AI Act, and SOC 2 mappings. Its own SOC 2 self-assessment says "3 of 4 audit chain implementations have integrity defects, post_execute() never blocks" and "0 of 5 criteria fully covered" |
| Guardrails and policy | open-policy-agent/opa | 12,279 | 2026-09-26 | — |
| Guardrails and policy | cedar-policy/cedar | 1,750 | 2026-09-25 | — |
| Guardrails and policy | invariantlabs-ai/invariant | 463 | 2026-01-12 | Flow rules across tool-call sequences, enforced as an MCP/LLM proxy |
| Guardrails and policy | meta-llama/PurpleLlama (LlamaFirewall) | 4,406 | 2026-08-18 | PromptGuard, AlignmentCheck, CodeShield |
| Guardrails and policy | NVIDIA-NeMo/Guardrails | 7,205 | Not recorded | — |
| Guardrails and policy | guardrails-ai/guardrails | 7,455 | Not recorded | — |
| Guardrails and policy | protectai/llm-guard | 3,210 | Not recorded | — |
| MCP / A2A gateway | IBM/mcp-context-forge | 4,534 | 2026-09-25 | Federates MCP, A2A, and REST; plugins, rate limits, OTel, Helm |
| MCP / A2A gateway | agentgateway/agentgateway | 5,062 | 2026-09-27 | "fine-grained RBAC with CEL policy engine", guardrail webhooks, OTel, A2A |
| MCP / A2A gateway | docker/mcp-gateway | 1,586 | Not recorded | Each MCP server in an isolated container; secrets management |
| MCP / A2A gateway | microsoft/mcp-gateway | 853 | Not recorded | — |
| MCP / A2A gateway | agentic-community/mcp-gateway-registry | 944 | 2026-09-25 | IdP SSO (Keycloak, Entra, Okta…), scopes, attributable audit logging, a fail-closed egress guard |
| MCP / A2A gateway | mcp-hangar/mcp-hangar | 16 | 2026-09-27 | Deterministic admission and egress policy, tool-schema digest pinning (fail closed), per-tenant tool projection, HITL approvals, SIEM export (CEF, LEEF, syslog), RFC 8707 audience binding |
| MCP / A2A gateway | snyk/agent-scan | 3,091 | Not recorded | — |
| Evals and red-teaming | UKGovernmentBEIS/inspect_ai | 2,865 | Not recorded | — |
| Evals and red-teaming | promptfoo/promptfoo | 25,498 | Not recorded | — |
| Evals and red-teaming | NVIDIA/garak | 9,368 | Not recorded | — |
| Evals and red-teaming | microsoft/PyRIT | 4,553 | Not recorded | — |
| Evals and red-teaming | ethz-spylab/agentdojo | 870 | 2026-06-02 | A prompt-injection benchmark for tool-using agents |
| Observability | open-telemetry/semantic-conventions-genai | 394 | 2026-09-26 | `invoke_agent` and `execute_tool` spans; MCP spans named "{mcp.method.name} {target}"; all still Development status; content attributes flagged "likely to contain sensitive information" |
| Observability | traceloop/openllmetry | 7,454 | Not checked | — |
| Observability | langfuse/langfuse | 35,104 | Not checked | — |
| Observability | Arize-ai/phoenix | 11,634 | Not checked | — |
| Sandboxing | e2b-dev/E2B | 13,987 | Not recorded | — |
| Protocols | modelcontextprotocol/modelcontextprotocol | 9,320 | Not recorded | — |
| Protocols | a2aproject/A2A | 25,945 | Not recorded | — |
| Protocols | google-agentic-commerce/AP2 | 3,195 | 2026-06-17 | Signed "mandates" as "deterministic, non-repudiable proof of intent" |

**Patterns across them.**

- **Policy as code:** the agent governance toolkit (AGT), Codex exec-policy, agentgateway's CEL, and
  Invariant's sequence rules. **[inference]** The common gap: rules are stateless and per call; none
  models positions, P&L, drawdown, settlement, or budgets.
- **MCP security best practices:** MCP servers "MUST only accept tokens that are valid for use with
  their own resources" and "MUST NOT accept or transit any other tokens"; progressive least-privilege scopes with "incremental elevation"; a
  warning against "wildcard or omnibus scopes". Alpaca's MCP server offers only coarse
  `ALPACA_TOOLSETS` filtering.
- **Approval gates:** Claude Code's PreToolUse ask, Codex's prompt, AGT's `require_approval`, and
  Hangar's HITL "attributed to a real principal".
- **Audit and tracing:** OTel GenAI spans (Development status) and SIEM formats.
- **Replay:** Hangar says "every verdict is reproducible from the policy that produced it".
  TradingAgents admits "Live data moves… a run today sees different inputs".
- **Red-teaming:** AGT quotes OWASP LLM01, "it is unclear if there are fool-proof methods of
  prevention for prompt injection", and concludes that interception must happen "in deterministic
  application code before the model's intent reaches the wire".
- **Multi-tenancy:** the MCP spec's `<user_id>:<session_id>` binding and "MUST NOT use sessions for
  authentication"; Hangar is "fail-closed on unknown identity".

## 3. Trading-specific findings

No well-adopted open-source guardrail harness for trading agents exists: a `gh` search found only
repositories with 0 to 2 stars.

| Project | Stars (as of 2026-09-27) | What it does | What it does not do |
|---|---|---|---|
| TauricResearch/TradingAgents | 108,883 | Multi-agent research; "risk management" is LLM debate; "The Portfolio Manager approves/rejects"; a simulated exchange | "designed for research purposes"; no deterministic enforcement layer |
| virattt/ai-hedge-fund | 63,770 | A YAML "mandate"; withholds ticker, industry, and dates in backtests to reduce memorization | "educational" and "is not intended for real trading"; no enforcement layer |
| alpacahq/alpaca-mcp-server | 994 | Paper by default; toolset filtering | No per-order limits, approvals, journal, or idempotency |
| coiltrade/claude-robinhood-deterministic-trading | 0 (last push 2026-09-07) | "The agent operates the system. The agent never originates a trading decision"; a "Policy gate … hard limits the agent CANNOT modify" | Evidence is README-level only |
| google-agentic-commerce/AP2 | 3,195 | User-signed Checkout and Payment mandates "anchored to deterministic, non-repudiable proof of intent" | A payments protocol, not a trading harness |

## 4. What MCP policy gateways do and do not do

What the gateways in section 2 document: identity, RBAC, allow or deny on tool name and arguments,
rate limits, approvals, and audit export. What they do not do **[inference]**, compared with what
Mandate specifies:

| Capability | MCP gateways (as documented) | Mandate's gate plus journal |
|---|---|---|
| Identity, RBAC, scopes | Yes (IdP SSO, CEL, OIDC/JWT) | Planned: E9-1, E9-2, E9-4 at M8 |
| Allow or deny per call on tool name and arguments | Yes | The gate decides per order intent, from state |
| Stateful domain limits (positions, P&L, drawdown, settlement, budgets) | No | The risk gate ([trading domain spec §9](../specs/trading-domain.md#9-risk-gate), [mandate spec §5](../specs/mandate.md#5-risk-state-and-limits)) |
| "Reducing risk never needs approval" asymmetry | No | `AGENTS.md` rules 2 and 13 |
| Write-ahead journaling with idempotency keys | No | `AGENTS.md` rule 5; [journal spec §5.2](../specs/journal.md#52-write-before-acting-and-crash-recovery) |
| Reconciliation | No | [Trading domain spec §11](../specs/trading-domain.md#11-reconciliation) |
| Crash recovery with `Unknown` orders | No | [Journal spec §5.2](../specs/journal.md#52-write-before-acting-and-crash-recovery), E7-2, E7-3 |
| Scoped kill switches | AGT lists a kill switch; its scope was not checked | `AGENTS.md` rule 13: an agent-scoped kill switch never uses cancel-all or close-position |
| Hash-chained, anchored evidence | AGT documents a Merkle audit, and its own self-assessment reports integrity defects | [Journal spec §10](../specs/journal.md#10-anchoring) |

The gateways are complementary, not competing: an enterprise may put one in front of Mandate's MCP
channel for identity and audit export, and Mandate still decides every order from state
**[inference]**.

## 5. What enterprise buyers appear to require

As advertised by the tools above; buyer surveys were **not checked**.

- **SSO and RBAC:** mcp-gateway-registry's IdP list; AGT's roles READER, WRITER, ADMIN, and AUDITOR;
  Hangar's OIDC/JWT.
- **Audit export:** Hangar's CEF, LEEF 2.0, RFC 5424 syslog, JSON lines, and OTLP;
  mcp-gateway-registry's "attributable audit trail … with credential masking".
- **SOC 2 mappings:** AGT, with its caution that "SOC 2 Type II requires evidence of operating
  effectiveness over a review period".
- **Self-hosted:** ContextForge (Helm), Hangar ("self-hosted, no SaaS"), and mcp-gateway-registry.
- **SRE:** AGT's "Kill switch, SLO monitoring, chaos testing".
- **Evidence packs:** AGT's "Decision BOM".
- **[inference]** Brokers add books-and-records retention, supervisory review, and pre-trade
  risk-control evidence. This was **not checked** against regulatory sources and is a question for
  counsel, not a conclusion.

## 6. What Mandate already does that most harnesses don't

State on 2026-09-27: "specified" means the spec is approved; "built" means merged code.

- **A stateful, domain-aware deterministic gate.** Specified; the gate spine and limits are built
  (E6-3).
- **Asymmetric safety and safe timeouts** (`AGENTS.md` rules 2, 3, and 13). Specified.
- **A write-ahead journal with idempotency keys, reconciliation, and crash recovery.** The journal is
  built (E5-1 to E5-4); reconciliation and recovery are being built in the executor (E7-2, E7-3).
- **A hash-chained, externally anchored journal with test vectors.** The chain, its
  [test vectors](../specs/reference-cases/journal.yaml), and `journal verify` are built; anchoring is
  specified ([journal spec §10](../specs/journal.md#10-anchoring)).
- **Scoped kill switches with no cancel-all at agent scope** (rule 13). Specified; the runtime's kill
  switches are built.
- **LLMs produce opinions, never orders** (rule 4).
- **A conformance suite** with reference cases, independent-oracle property tests, and zero missed
  mutants on safety-critical crates (DEC-79).
- **The outer harness itself:** the trust ladder and the `xtask` checks.

## 7. Recommendations

Each is proposed, not scheduled, as one story in the backlog's
[enterprise harness section](../project/06-backlog-v1.md#enterprise-harness-proposed-dec-149). None
changes milestone order.

| # | Recommendation | Component | Story |
|---|---|---|---|
| 1 | A tenant policy overlay that can only tighten; versioned, with inline tests | Gate, SDK | E18-1 |
| 2 | OTel GenAI spans derived from the journal, content off by default, the semantic-conventions version pinned. Neutral for the moat | Journal, executor | E18-2 |
| 3 | SIEM export plus a signed per-period evidence pack: chain segment, anchor proofs, mandate versions, verdicts, reconciliation | Journal | E18-3 |
| 4 | The MCP channel as a policy enforcement point: tokens issued to Mandate, no passthrough; intent-shaped tools (`propose_intent`, `explain_verdict`), never a raw `place_order`; read scopes first, then elevation; pinned tool schemas | MCP channel | E18-4 |
| 5 | AP2-style signed mandate versions and ASK approvals with the owner's step-up credential, journaled | Autonomy, journal | E18-5 |
| 6 | The conformance suite as a certification kit for connectors and agents, reporting pass^k over seeded fuzz runs | Conformance | E18-6 |
| 7 | An adversarial bench: injected news, filings, or tool outputs; assert zero limit breaches given 100% model compromise | Conformance | E18-7 |
| 8 | Multi-tenant isolation as a checked invariant: per-tenant chains and anchors, type or layer rules, cross-tenant fuzz | Journal, executor, gate | E18-8 |
| 9 | Enterprise identity: OIDC/SAML SSO, SCIM, AUDITOR and SUPERVISOR roles, ASK routing to supervisors. Neutral but required | Autonomy, approvals | E18-9 |
| 10 | Self-hosted or VPC deployment with a pluggable external anchor (a customer WORM store or a transparency log) | Journal | E18-10 |
| 11 | A sandbox for customer strategy code: no broker egress, no vault; intents only | SDK | E18-11 |
| 12 | Sequence and flow policies over journaled intents, for order splitting and churn | Gate | E18-12 |
| 13 | Deterministic replay of gate verdicts as a customer feature: `mandate replay <range>` | Journal, SDK | E18-13 |
| 14 | Deterministic remediation in denials: reason codes plus the tightest compliant alternative | Gate, MCP channel | E18-14 |
| 15 | A published compliance mapping, each control mapped to a test ID and a journal event type | Docs, conformance | E18-15 |

## 8. Repositories to study

- **microsoft/agent-governance-toolkit:** per-call interception, approvals, compliance mappings, and
  its own SOC 2 self-assessment of its gaps.
- **mcp-hangar/mcp-hangar:** reproducible verdicts, schema pinning, per-tenant projection, SIEM
  formats.
- **openai/codex:** exec-policy rules with inline tests; the strictest matching decision wins.
- **anthropics/claude-code:** hooks whose deny cannot be overridden.
- **modelcontextprotocol/modelcontextprotocol:** the security best practices (token audience,
  incremental scopes, sessions are not authentication).
- **google-agentic-commerce/AP2:** signed mandates as proof of intent.
- **open-telemetry/semantic-conventions-genai:** span names and the sensitive-content flag.
- **ethz-spylab/agentdojo:** prompt-injection benchmarking for tool-using agents.
- **invariantlabs-ai/invariant:** rules over sequences of tool calls.
- **lopopolo/harness-engineering**, with **walkinglabs/learn-harness-engineering:** the outer
  harness as a discipline.
