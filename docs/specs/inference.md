# Inference and Model Gateway Spec (v1)

| | |
|---|---|
| **Status** | v0.2, draft ([DEC-432](../project/decisions/DEC-432.md)): v0.1 with the post-merge review's blockers and majors fixed (DEC-432 items 17 to 22). Items 1 to 12 and 17 to 22 are agent readings; the founder decided items 13 to 16 on 2026-10-03; item 23 is Proposed |
| **Implements** | [HLD §9](../HLD.md#9-intelligence-layer) (model gateway, speed tiers), [HLD §10](../HLD.md#10-billing) (model tokens at cost plus margin), PRD FR-3.7, FR-3.9, FR-10.2; backlog E15 |
| **Depends on** | [Mandate spec §7, §8](mandate.md#8-signal-models-and-the-order-builder), [journal spec §6.3, §8, §9](journal.md#9-event-catalogue), [DEC-67](../project/04-decision-log.md#decisions), [DEC-120](../project/04-decision-log.md#decisions) |
| **Caller side** | The [agent harness spec](agent-harness.md) says when the runtime asks for a model output and how long an evaluation waits. This spec says what happens to the call |

This spec covers every model call the platform makes and the gateway that brokers them. Today
the only model code is the research spike's client (`python/research_spike`); the gateway itself
is not built (§11).

## Change history

- **v0.2 ([DEC-432](../project/decisions/DEC-432.md) items 17 to 23):** fixes from the independent
  review of v0.1 (PR #551). Each only tightens or clarifies.
  - §4.1: `content_hash` covers the pinned content only. `endpoints` and `status` are outside it, so
    a routing change never invalidates an owner's pin. Mandate spec §8.1 carries the same definition.
  - §3.6: which stream holds the compiler's call record is open; no stream is stated as the rule.
  - §3.6, §7.3: the meter stream has one writer per workspace; a replica that cannot reserve refuses
    the call with `meter_unavailable` (§3.3).
  - §3.5, INF-3: a response that completed after its deadline is never cached.
  - From the agent harness spec's asks (§10.1 there): the retrieval plan and validation bounds are
    inside the content hash, a caller can read its own reservations, the prompt guard can be called
    without a call, and E15-8 gains two record members. The fifth ask, a prompt record without
    licensed text, is Proposed (item 23, §13).
  - The review's minors are backlog row E15-12 (freeze rule).

## Contents

1. [Scope](#1-scope)
2. [Invariants](#2-invariants)
3. [Call contract](#3-call-contract)
4. [Model registry and pinning](#4-model-registry-and-pinning)
5. [Laya and Jev](#5-laya-and-jev)
6. [Providers and hosting](#6-providers-and-hosting)
7. [Cost model hooks](#7-cost-model-hooks)
8. [Security](#8-security)
9. [Lifecycle and failure walk](#9-lifecycle-and-failure-walk)
10. [Adversary review](#10-adversary-review)
11. [What exists today and what is planned](#11-what-exists-today-and-what-is-planned)
12. [Decisions](#12-decisions)
13. [Open questions](#13-open-questions)

---

## 1. Scope

### 1.1 In scope

| Caller | Purpose | Speed tier ([HLD §9](../HLD.md#9-intelligence-layer)) | Status |
|---|---|---|---|
| Research agent ([mandate spec §8.4](mandate.md#84-the-research-agent-dec-97-adr-0002)) | Theses from market data, news, filings, and the agent's memory | Slow (seconds to minutes) | Spike only |
| LLM signal models (`llm.` prefix, §8.1) | Observations, evidence, invalidation | Slow | Planned |
| Fast decision models (`fast.` prefix) | Typed answers with scores | Fast (30 to 300 ms) | Not defined (§5) |
| Mandate compiler ([mandate spec §7](mandate.md#7-compiler-and-platform-proposals-dec-97)) | Extract stated values, propose envelope values | Slow, interactive | Planned |
| Future local models | Any of the above, served on the customer's site | As above | Planned |

The **model gateway** is the one component that sends a request to a model. It lives in the data
plane of the workspace deployment ([HLD §4](../HLD.md#4-architecture)). In-process models (for
example an in-process open-weight model) use the same contract as a library, so metering,
deadlines, and journaling are the same.

### 1.2 What the agent runtime may assume

- A call returns exactly one of: a schema-valid output from the pinned model, or a typed error
  (§3.3). There is no third case, such as a partial output or an output from another model.
- A call never outlives its deadline from the caller's point of view.
- A call never places an order, reads a credential, or changes a mandate.
- A refused call costs nothing. A failed call may cost money, and that cost is metered.
- When the call ends in an error, the model has no fresh output for that call. The order builder
  already treats that as missing ([mandate spec §8.3](mandate.md#83-order-builder-conviction_linear-dec-47-dec-60)).

### 1.3 Non-goals

- **Choosing models for users.** The owner selects and pins signal models (rule 11). The gateway
  never picks, ranks, or recommends one.
- **Calibration, fine-tuning on users' results, or weight changes** ([DEC-47](../project/04-decision-log.md#decisions)).
- **Model calls made by an owner-connected agent** ([DEC-141](../project/04-decision-log.md#decisions)).
  Those run on the owner's side; their requests enter as owner input, not through this gateway.
- **Quant models** (`quant.` prefix). They are deterministic code in the runtime and make no
  model call. They are pinned under §8.1 like every signal model.
- **Factual classification on the shared data plane** (filing type, entity tags). If it ever uses a
  model, that model is a platform component under its own spec, never a signal model, and emits no
  directional view ([DEC-62](../project/04-decision-log.md#decisions)).
- **Training our own models.** Out of scope for v1 (§5).

---

## 2. Invariants

Each invariant is a property the gateway must always hold. Each has a property-based test with
an independent oracle (§2.1). "Call" means one logical request; "attempt" means one transport try
inside it.

| # | Invariant |
|---|---|
| INF-1 | **Pinned identity.** Every call names a registered model identity: provider, model id, and an immutable version (a dated snapshot) or a weights digest. A floating alias (for example "latest") is refused at registration and never reaches a call |
| INF-2 | **No substitution** (DEC-67). An attempt goes only to an endpoint the registry lists as serving that exact identity. If the response reports a different identity, the call fails with `identity_mismatch`. The gateway never routes to another model, a smaller variant, or a different quantization |
| INF-3 | **Deadline.** Every call has a finite deadline, no later than the registry entry's deadline. A response that completes after the deadline is discarded, is never cached, and never becomes an output, in this evaluation or any later one |
| INF-4 | **Missing is safe.** Every error kind yields no output. Under §8.3 a missing output counts as fully bearish for buys, so it never enlarges a buy (MI-10), and as 0 for exits, never as a negative view. Any exit that follows is a risk reduction. No error path produces a default output, a cached output for different input, or an output from another model |
| INF-5 | **Opinions only** (rule 4). The gateway returns only an output validated against the model's output schema. It offers the model no tool, function, or network capability. Nothing a model returns is an order, a mandate field, or a policy value |
| INF-6 | **Metered.** Every call produces exactly one metering record, and every attempt that reached a provider is counted in it, including failures and cache hits. The record names the workspace, the agent (when there is one), and the purpose. Cost is fixed-point USD, never floating point |
| INF-7 | **Caps refuse, never add risk.** A call starts only if its reserved maximum cost fits under every cap that applies (§7.3) and every rate cap allows it. A cap only refuses calls. Reaching a cap never changes an envelope field, a limit, an order, or the agent's mode |
| INF-8 | **Prompt contents.** A prompt holds only the inputs the model's contract lists (mandate spec §7, §8.1, §8.4). It never holds a credential, an account identifier, personal data, or another workspace's data. A deterministic guard checks this before any byte leaves (§8.3) |
| INF-9 | **One call, one workspace.** No request mixes two workspaces' data. Caches and rate buckets are keyed by workspace. Shared provider-side cache prefixes hold no workspace data |
| INF-10 | **Journal before use; replay never calls.** A call's record, with its prompt and response stored as artifacts by hash, is journaled before its output is used. Replay, audit, and recovery read recorded outputs and never call a model |
| INF-11 | **Exact cache.** A cache hit requires the same workspace, the same model identity, and the same canonical request digest. A hit is journaled as a hit and points at the original response artifact |
| INF-12 | **Locality.** A workspace's policy lists the endpoints it allows. A local-only deployment has no route to a hosted endpoint, and a call to an endpoint not allowed is refused before any byte leaves |
| INF-13 | **Off the hot path.** The risk gate, protective orders, exits, kill switches, and reconciliation never call or wait on the gateway. A gateway outage changes only which outputs are fresh |
| INF-14 | **Credential custody** (rule 7). Provider keys live in the vault and are read only by the gateway. They never appear in a prompt, log, journal event, metering record, or the agent runtime |
| INF-15 | **Withdrawal is explicit.** A model stops being callable only through a journaled `PlatformOperatorAction` (`model_withdrawn`). Nothing replaces it automatically: a replacement is a new owner-confirmed mandate version |
| INF-16 | **Envelope untouched** (rule 11). Nothing in the gateway changes a pin, weight, deadline, cap, or any other mandate field. Those change only by a confirmed mandate version or a policy change |

### 2.1 How each invariant is tested

| Invariant | Test (independent oracle) |
|---|---|
| INF-1, INF-2 | Fuzz registry entries and responses. The oracle compares the response's reported identity with the pin by plain string equality on each part; seeded bugs: alias accepted, version ignored, quantization ignored |
| INF-3 | Fuzz response arrival times against a simulated clock. The oracle derives "late" from the journaled deadline and completion time, never from the gateway's own flag. It also asserts that no cache entry exists for any call whose completion time is past its deadline, and that a later identical request is a miss; seeded bug: a late response stored in the cache |
| INF-4 | For every error kind, assert the gateway returns no output, then feed the state to the reference order builder (`reference/mandate`) and assert the buy value is at most the value with the model's fresh output present (MI-10), and that the exit conviction equals §8.3's sum with the missing model's term set to 0 |
| INF-5 | Assert the request body the adapter builds has no tool or function members, for every provider adapter; feed outputs with extra members and assert refusal |
| INF-6, INF-7 | Fuzz sequences of calls, failures, cache hits, restarts, and risk-day changes. A separate accumulator over journaled records recomputes spend and asserts it equals the meter and never exceeds a cap |
| INF-8 | Plant credential patterns, account identifiers, and a second workspace's values in each input source; assert the guard refuses and nothing reaches the transport stub |
| INF-9, INF-11 | Two workspaces send identical requests; assert no cross-workspace hit and no shared bucket |
| INF-10 | Replay a journal with the transport stub set to fail every call; assert the fold is identical |
| INF-12 | Policy fuzz: for each disallowed endpoint, assert the transport stub saw no bytes |
| INF-13 | Kill the gateway mid-run in the runtime simulator; assert every exit, protective order, and kill switch completes |
| INF-14 | Scan every journaled event, artifact, log line, and metering record for the test key's bytes |
| INF-15, INF-16 | Assert that no gateway code path writes a mandate or policy document, by the crate layering in `xtask/layers.toml` and a test |

Each oracle must catch a seeded bug before it is trusted (`AGENTS.md`, "Independent oracles").

---

## 3. Call contract

### 3.1 Request

| Field | Rule |
|---|---|
| `call_id` | ULID, unique per call. Retries reuse it with a new `attempt` number |
| `workspace_id`, `agent_id` | `agent_id` is null only for the compiler, which acts for a user, not an agent |
| `purpose` | `research`, `llm_signal`, `fast_signal`, `compiler`. Matches journal spec §9's `ModelInvocationRecorded` purposes (compiler, fast model, research), plus `llm_signal` |
| `model` | The registry reference: signal model id, version, and content hash (mandate spec §8.1), or the compiler's registry entry |
| `inputs` | Typed inputs named by the model's contract; the gateway renders the prompt from them with the pinned template. Callers never pass free-form prompt text |
| `as_of` | The data cut-off of the inputs (§8.2). Part of the canonical request |
| `deadline` | Absolute UTC instant, nanosecond precision. No later than `now + entry.deadline_ms` |
| `max_output_tokens` | From the registry entry. Bounds the call's maximum cost (§7.2) |

The **canonical request** is the canonical JSON (journal spec §4) of the model identity, the rendered
prompt, sampling parameters (temperature, seed where supported), the output schema, and
`max_output_tokens`. Its SHA-256 is the `request_digest`.

### 3.2 Response

| Field | Rule |
|---|---|
| `call_id`, `outcome` | `ok` or one error kind (§3.3) |
| `output` | Present only when `outcome` is `ok`: the parsed output, valid against the entry's output schema |
| `reported_identity` | What the provider says served the call. Must equal the pin (INF-2) |
| `usage` | Input, output, and cached tokens; `attempts`; `cache_hit` |
| `cost_usd` | Fixed-point USD from the price table version in effect (§7.2) |
| `prompt_ref`, `response_ref` | Artifact hashes (journal spec §6.3). `response_ref` is null when no response bytes arrived |

### 3.3 Error kinds

| Kind | Meaning | Retried? | Costs money? |
|---|---|---|---|
| `policy_denied` | Endpoint, provider, or model type not allowed by policy, or local-only (INF-12) | No | No |
| `budget_exhausted` | A spend cap would be exceeded by the reservation (§7.3) | No | No |
| `rate_limited_local` | The gateway's own per-agent or per-workspace rate cap | No | No |
| `meter_unavailable` | The reservation could not be appended: the meter writer is unreachable, or this replica is fenced (§3.6) | No | No |
| `input_rejected` | The prompt guard found forbidden content (§8.3) | No | No |
| `model_withdrawn` | The entry is withdrawn (INF-15) | No | No |
| `deadline_exceeded` | No complete response before the deadline | No further attempts | Maybe |
| `provider_unavailable` | Connection failure, 5xx, or no endpoint healthy | Yes, within the deadline | Maybe |
| `rate_limited_provider` | Provider 429 | Yes, after the provider's wait, within the deadline | No |
| `credential_invalid` | Provider 401 or 403 | No | No |
| `content_refused` | The provider declined to answer | No | Maybe |
| `schema_invalid` | Response does not parse, or fails the output schema | No | Yes |
| `identity_mismatch` | Reported identity differs from the pin | No | Yes |

The first six are refusals: nothing leaves the deployment.

### 3.4 Deadlines, retries, streaming, idempotency

- **Deadlines.** Each registry entry carries `deadline_ms`, inside the model's content hash, so
  the owner pins it with the model (DEC-432 item 1). Policy may set a platform maximum. A fast
  model's deadline is at most 300 ms; a slow model's is at most 600 s.
- **Retries.** Only transport failures retry (`provider_unavailable`, `rate_limited_provider`),
  only to endpoints serving the same identity, and only while the deadline leaves time. A content
  failure (`schema_invalid`, `identity_mismatch`, `content_refused`) never retries: sampling again
  until something parses would select outputs (DEC-432 item 3). Each attempt is metered.
- **Streaming.** The gateway may stream from the provider to enforce the deadline and to stop at
  `max_output_tokens`. The caller never sees a partial output: it gets the whole validated output or
  an error.
- **Idempotency.** A model call has no side effect outside the platform except cost. `call_id`
  makes the journal record and the metering record exactly-once: a second record for the same
  `call_id` is refused at append. After a crash, a call with a reservation and no result is never
  re-sent under the same `call_id`; the caller may start a new call with new inputs (§9).
- **Determinism.** The gateway sets temperature 0 and a recorded seed where the provider supports
  it. Hosted providers do not promise identical outputs for identical input, so nothing relies on
  recomputation: replay reads the journal (INF-10).

### 3.5 Caching

- **What may be cached:** the complete response to a canonical request, stored as the response
  artifact. Key: `(workspace_id, model identity, request_digest)`.
- **What may not be cached:** errors, outputs that failed validation, a response that completed
  after its deadline, however complete and valid (INF-3, DEC-432 item 21), and anything keyed on
  similarity (no semantic cache). A response is never served to another workspace.
- **Freshness is unchanged.** `as_of` is part of the canonical request, so a hit carries the
  original `as_of`, and the §8.2 freshness rule applies to it as to any output. A hit cannot make an
  old output look new.
- **Withdrawal empties the cache** for that identity.
- **Provider-side prompt caching** (a discount for a repeated prefix) is allowed only for prefixes
  that hold no workspace data: the pinned system text and output instructions (INF-9).
- A hit is journaled with `cache_hit: true`, `cost_usd` 0, and the original `response_ref`.

### 3.6 Journaling

Journal spec §9 lists `ModelInvocationRecorded` (agent stream, `mod` ref) and leaves its payload
schema to its own story. This spec needs these members when that schema closes (DEC-432 item 11):
`call_id`, `purpose`, the model identity and endpoint, `request_digest`, sampling parameters and
seed, `prompt_ref`, `response_ref`, `reported_identity`, provider request ID, `outcome`,
`attempts`, token counts, `cost_usd`, the price table reference, `cache_hit`, `deadline`, and
completion time. The agent harness spec adds two for E15-8 (its §10.1 ask 3, DEC-432 item 22): the
retrieved context, as the event ids of the run's `ObservationRecorded`, and room for the
candidates' verdicts. The run id rides in `correlation_id`.

Every journal stream has a single writer (journal spec §2), and the gateway writes none of the
existing ones. So:

- **The caller appends the call record.** The gateway returns the record; the agent's runtime
  appends `ModelInvocationRecorded` to its agent stream.
- **The compiler's record is on the control stream** (DEC-432 item 19, answered by
  [DEC-670](../project/decisions/DEC-670.md)). The compiler has no agent, so the workspace services
  that call the gateway for it append its `ModelInvocationRecorded` to the workspace's control
  stream, closed there with this section's members ([journal spec §9.9](journal.md#99-workspace-api-records-dec-670)).
  Until `mandate-journal` registers that record, the compiler makes no call through the gateway,
  because INF-10 could not hold for it.
- A call with `outcome: ok` is followed by its output event (`ModelOutputRecorded` or
  `ThesisProposed`), which carries the content hash the call used.
- A call with any other outcome writes `ModelInvocationRecorded` and no output event. The model
  simply has no fresh output.
- **The gateway owns a meter stream.** Reservations and settlements (§7.3) go to a new
  per-workspace stream (`meter:{workspace_id}`, a proposed journal spec change, E15-8). The
  reservation is appended before the first attempt leaves the deployment, so a crash cannot lose a
  cost, and spend per agent and per workspace folds from that stream.
- **One meter writer per workspace** (DEC-432 item 20). The gateway may run as several replicas, but
  a stream has one writer, fenced by epoch (journal spec §2, §5). So exactly one replica holds the
  meter writer role for a workspace, and it alone checks caps and appends. Any other replica asks
  the holder to reserve before it sends anything. A cap is therefore checked against one ordered
  stream, never against two replicas' separate views.
- **A replica that cannot reserve refuses.** If the holder is unreachable inside the call's
  deadline, or the replica's own append is fenced, the call ends `meter_unavailable` (§3.3):
  nothing is sent and the output is missing (INF-4). Each such refusal is counted and alerts the
  operator, so it is never a silent partial outage. It has no meter-stream entry and costs nothing;
  the caller's `ModelInvocationRecorded` records the outcome.
- **Hand-over.** A new holder takes the next epoch and folds the stream before it reserves. A
  reservation the old holder left unsettled stays counted (§7.3).
- **A caller can read its own reservations** in the meter stream (agent harness spec §10.1 ask 2),
  and only its own agent's. Reading changes nothing.

---

## 4. Model registry and pinning

### 4.1 Registry entry

The registry lives in the workspace deployment; the global control plane distributes signed
entries ([HLD §4](../HLD.md#global-control-plane-thin)). An entry has two parts:

- **Pinned content,** immutable once published, and covered by `content_hash`. Any change to it is
  a new version with a new hash, which an owner must confirm to use.
- **Routing and status** (`endpoints`, `status`), outside the hash, versioned as journaled
  configuration. Changing them never changes the hash, so it never invalidates an owner's pin
  (DEC-432 item 17).

| Field | Rule |
|---|---|
| `model_id`, `version` | The signal model's id and semantic version (mandate spec §8.1) |
| `content_hash` | SHA-256 over the canonical JSON of the pinned content: `identity`, `template`, `retrieval_plan`, `output_schema`, `validation_bounds`, `params_schema`, `deadline_ms`, `max_output_tokens`, the sampling parameters, `methodology`, and `authorship`. **`endpoints` and `status` are not in it.** The mandate pins it (V-007); [mandate spec §8.1](mandate.md#81-signal-model-contract-dec-52-dec-97) gives the same definition (DEC-432 item 18) |
| `identity` | `provider`, `model_name`, and `snapshot` (an immutable, dated version) or `weights_digest`, plus `quantization` for self-served or third-party-served open weights |
| `template` | The prompt template and the input contract it renders |
| `retrieval_plan`, `validation_bounds` | For a model whose inputs are retrieved: which reads fill the input contract and their caps, and the bounds its output is checked against (agent harness spec §6.3, §6.5). Inside the hash, so the pin covers what the model is shown (rule 11, DEC-432 item 22) |
| `output_schema` | JSON Schema of the output (mandate spec §8.2, or §8.4's thesis) |
| `params_schema` | The user-set parameters; no defaults (DEC-52) |
| `deadline_ms`, `max_output_tokens`, sampling parameters | Pinned with the model |
| `endpoints` | Outside the hash. Endpoints attested to serve exactly `identity`, each with region and locality (`hosted` or `local`) |
| `methodology` | Documentation of method only; no performance claims (§8.1) |
| `authorship` | `platform` in v1 |
| `status` | Outside the hash. `evaluating`, `offered`, `deprecated` (with the provider's retirement date), `withdrawn` |

Adding or removing an endpoint changes the routing set, not the identity and not the hash. An
endpoint is added only after the attestation check of §4.3 passes on it, and the change is journaled
as configuration. INF-2 still holds: every listed endpoint serves exactly `identity`, quantization
included, and the reported identity is checked on every response.

### 4.2 How a model becomes selectable

1. **Registered** with status `evaluating`: not visible in the model picker.
2. **Evaluation gate** (§4.3) passes, recorded with the entry's content hash.
3. **Offered:** visible in the picker for workspaces whose policy allows its type
   (`signal_model_types`, [mandate spec §4.3](mandate.md#43-policy-hierarchy-dec-51-dec-98)) and at
   least one of its endpoints.
4. **Pinned** by the owner as an envelope field (rule 11): id, version, content hash, parameters,
   weight, `max_output_age_s`. Adding or changing a model classifies as increasing
   ([mandate spec §9.2](mandate.md#92-classification)), so it needs confirmation.

### 4.3 Evaluation gate before a model is offered

The gate checks mechanics and safety, never profitability. Performance claims are not allowed
(§8.1), and thesis quality comes only from forward paper ([DEC-99](../project/04-decision-log.md#decisions)).

| Check | Pass condition |
|---|---|
| Identity | The provider exposes an immutable snapshot or the weights digest matches; each endpoint's reported identity equals it on a probe set |
| Schema conformance | At least 99% of a fixed probe set parses and validates; failures are journaled, never repaired |
| Output limits | Zero outputs with imperatives, price targets, or profit statements on the probe set (§8.1), except the research agent's allowed fields |
| Latency | p99 within `deadline_ms` from the deployment's region |
| Injection fixtures | The prompt-injection fixture set (RAID R-05) does not produce an output that names a source off the allowlist or breaks the output schema |
| Cost bound | `max_output_tokens` and the price table give a finite maximum cost per call |
| Data terms | The provider's terms for this endpoint are on file (§6, DEC-432 item 15) |

The probe sets and thresholds are versioned configuration, fixed before the evaluation runs.

### 4.4 Provider deprecation

1. The provider announces a retirement date. The registry entry becomes `deprecated` with that
   date; this is journaled, and owners of agents that pin it are alerted (opaque ID and generic
   text, rule 6).
2. Nothing changes for running agents. The owner may confirm a new mandate version with another
   model; the platform never switches for them (INF-15).
3. At the retirement date, or earlier if the endpoint stops serving the identity, an operator
   journals `PlatformOperatorAction` (`model_withdrawn`). From then the model's outputs count as
   missing (§8.1). Exits, protection, and kill switches continue. If no model has a fresh output,
   the order builder holds (§8.3).
4. A withdrawn research agent proposes nothing; existing theses run to expiry or invalidation
   (§8.6).

---

## 5. Laya and Jev

**What the repository says.** [HLD §5 and §9](../HLD.md#9-intelligence-layer) name two fast
decision models: **Laya**, an open-weight model run in-process or in a pool, and **Jev**, a hosted
model behind the gateway. The glossary gives both as examples of a fast decision model. The PRD
lists "one fast decision model" at P1 (FR-3.7) and asks which ships first (open question 2).
[OD-02](../project/04-decision-log.md#open-decisions) holds that choice with a bake-off as its
input. RAID R-07 records Jev as an early-access vendor and D-03 as a dependency on Laya weights or a
Jev key. The retail profile keeps `fast` out of `signal_model_types` until counsel answers
compliance question 33 ([mandate spec §4.3](mandate.md#43-policy-hierarchy-dec-51-dec-98)).

**What it does not say.** No document defines either model's inputs, outputs, typed questions,
training data, licence, owner, or price. No weights, key, contract, or code exist. Neither is
built, and neither is defined well enough to build.

**Options** (DEC-432 item 13; the founder chose a hosted fast model on 2026-10-03):

| Option | What it takes | For | Against |
|---|---|---|---|
| A. Build an open-weight fast model (Laya path) | Choose a base model and licence, label financial decisions, fine-tune, serve on GPUs, evaluate | Runs on-prem and air-gapped; exact weights pinning | Training data and labeling cost; GPU spend; a model team we do not have; any fine-tune on users' results would be calibration (DEC-47) |
| B. Use a hosted fast model (Jev path) | A vendor contract and key; one adapter | Fastest to try | Early-access vendor (R-07); prompt content leaves the site; unusable air-gapped; spending is founder-reserved |
| C. Drop the fast tier for v1 | Nothing | Matches the retail profile, which excludes `fast` until counsel answers; v1 needs only quant models and the research agent (FR-3.7 P0) | No fast model for business design partners in v1 |

**Decision (DEC-432 item 13, the founder, 2026-10-03): a hosted fast model in v1**, not the drop
this draft recommended. Keep the `fast.` prefix, the policy key, and this call contract. Revisit
OD-02 at Phase 3 with the bake-off it names, and a definition of the typed questions first. The
fast tier is served by a hosted model through the gateway, under the same pinning, deadline,
metering, and evaluation-gate rules as any model (DEC-432 items 1 to 12). Which model follows
item 14 (§6), and its evaluation gate comes first. The retail profile still excludes `fast` until
counsel answers question 33.

---

## 6. Providers and hosting

Choosing a provider is spending and a vendor choice, so it is the founder's (DEC-79). The founder
decided DEC-432 item 14 on 2026-10-03: an aggregator for every call, with provider routing locked per
pinned model.

| Option | Pinning | Data handling | Locality | Cost and effort |
|---|---|---|---|---|
| 1. Direct provider API | Dated snapshots; one provider per endpoint | One contract and one set of data terms per provider | Hosted only | Provider list price; one adapter per provider |
| 2. Aggregator (the spike uses OpenRouter) | The aggregator may route one model name to several upstream hosts. For open weights, hosts can differ in quantization, which is a different model under INF-2. Pinning needs the routing locked to named upstreams | Adds a processor between us and the model provider | Hosted only | One key, many models; a fee on top of provider prices |
| 3. Cloud model platform (a hyperscaler's hosted models) | Snapshot ids per region | Enterprise data terms; region choice | Hosted, in a chosen region | Fits customers with an existing cloud contract |
| 4. Self-hosted open weights | Exact weights digest and quantization | Prompts never leave the site | Local; the only option for air-gapped | GPU capacity and operations |

**Decision (DEC-432 item 14, the founder, 2026-10-03): option 2**, not the direct APIs this draft
recommended.

- **Every hosted call:** every model, for the platform's own workspaces and for users' agents
  alike, is reached through one aggregator with provider routing locked per pinned model (INF-2).
  DEC-432 item 10 keeps DEC-67 whole on this path: a response whose reported provider or model
  differs from the pin counts as missing.
- **Data terms (DEC-432 item 15):** only endpoints whose terms exclude training on API data, with
  the shortest retention available; both the aggregator's terms and the underlying provider's must
  meet this.
- **Hybrid and on-prem:** local-only by default (DEC-432 item 8). A customer may enable hosted
  endpoints in policy, and may bring their own key under the same routing lock (§7.4, DEC-432
  item 16). Self-hosted open weights (option 4) remain the local path after v1.
- **Spend guard:** no new paid service beyond the spike key already provided. The gateway is built
  against recorded fixtures and a provider adapter interface. Live calls run only in the team's
  internal paper workspaces.

---

## 7. Cost model hooks

### 7.1 Metering record

One record per call (INF-6), written in the workspace deployment.

| Field | Note |
|---|---|
| `call_id`, `workspace_id`, `agent_id`, `purpose` | Opaque IDs only |
| `model_ref` | Registry id, version, and content hash; never prompt content |
| `endpoint_class` | `hosted` or `local`, and the provider |
| `key_owner` | `platform` or `customer`: whose provider key paid for the call. Customer-key calls count for caps and quotas and are billed zero for tokens ([billing design](../design/billing.md) BL-9, DEC-442 item 9) |
| `input_tokens`, `output_tokens`, `cached_tokens`, `attempts`, `cache_hit` | Counts |
| `cost_usd` | Fixed-point decimal, 6 places, rounded half up, as the spike does today |
| `price_table_ref` | The versioned price table used |
| `risk_day` | The risk day ([mandate spec §5.4](mandate.md#54-daily-loss-dec-40-dec-49-dec-54)) the call counts against |
| `outcome` | As §3.3 |

### 7.2 Prices and the maximum cost

- The **price table** is versioned configuration per endpoint (input, output, and cached token
  prices), registered like any configuration snapshot, so the cost of every call can be recomputed.
- A call's **maximum cost** is `prompt tokens × input price + max_output_tokens × output price`,
  counted before the call. Local endpoints have a price table too (zero, or an internal rate), so
  the meter is the same.
- Provider invoices are reconciled with the meter for billing. Reconciliation never changes a
  risk day's cap accounting (§7.3).

### 7.3 Budget enforcement

| Cap | Where it comes from | Scope |
|---|---|---|
| Research cost cap | `behavior.research.cost_cap_usd_per_day`, an envelope field ([DEC-120](../project/04-decision-log.md#decisions)), tightened by the policy's `research_cost_cap_usd_per_day` | The research agent's calls, per agent per risk day |
| Workspace model spend | HLD §8 quota ("model spend"), set by the operator or plan | All calls of the workspace per risk day |
| Workspace cycle allotment | The license's `model_spend_usd_per_cycle`, from the plan ([billing design](../design/billing.md) §3.4, DEC-442 item 4) | All calls of the workspace per billing cycle; enforced alongside the per-risk-day caps, never instead of them |
| Rate caps | Per agent: research calls only in proposal rounds no more often than `behavior.research.interval_s`; per workspace: a token bucket set by the plan | Calls per second |

- **Reservation.** Before the first attempt, the workspace's meter writer appends a reservation of
  the maximum cost to the meter stream (§3.6). A call starts only if `spent + reserved + this reservation ≤ cap` for every cap that applies.
  On completion the reservation becomes the actual cost. If the process dies first, the reservation
  stays counted for that risk day (DEC-432 item 4). Spend never decreases within a risk day.
- **Check 7 is unchanged.** Mandate spec §8.5 check 7 still refuses a thesis once the day's spend has
  reached the cap. The gateway's check is earlier and never lets a call push spend above a cap.
- **Degrade, never add risk.** A refused call means no fresh output. Under §8.3 that can only shrink buys,
  and any exit is a risk reduction (INF-4). Existing positions, exits, protection, and kill switches are untouched.
- **Other model spend per agent.** Today only the research agent has an envelope cap. Fast and LLM
  signal models are bounded by the workspace quota until a per-model cap is specified (backlog
  E15-9).

### 7.4 Billing feed

[HLD §10](../HLD.md#10-billing) bills model tokens at cost plus margin. The workspace deployment
sums metering records per organization, period, endpoint class, provider, and `key_owner`, and sends counts and
cost only, never content, model outputs, or instruments, to the metering pipeline in the global
control plane. Hybrid deployments send signed usage reports; air-gapped ones send offline reports.
The margin is applied by billing, never by the gateway. Managed deployments use platform keys only.
Hybrid and on-prem customers may bring their own key (DEC-432 item 16): either their own account at
the aggregator DEC-432 item 14 names, or a provider key that aggregator routes. Every call still goes
through the gateway, which enforces INF-2's routing lock to named upstreams and DEC-432 item 10's
reported-identity check exactly as for a platform key. A customer key is accepted only for registry
entries whose routing lock the gateway can set and verify on that key; one that cannot keep the
lock is refused when it is registered, so BYO is not offered for that model. Calls on a customer key
carry `key_owner = customer`, count toward every cap and rate limit, and are billed zero for tokens
([billing design](../design/billing.md) BL-9).

---

## 8. Security

### 8.1 Key custody

- Provider keys are vault secrets of the workspace deployment (managed: our vault; hybrid and
  on-prem: the customer's). Only the gateway's service identity can read them (INF-14).
- Keys are fetched per attempt or held in memory only; they are redacted in raw-request records
  (journal spec §6.3).
- Revocation and rotation are journaled (`KeyRotated`, `KeyRevoked`, journal spec §9). A revoked key
  ends calls with `credential_invalid` (§9).
- Agents and their tests use paper, demo, or fixture credentials only (rule 8). No live trading
  credential is ever near the gateway.

### 8.2 Egress control

- The gateway is the only data-plane component with egress to model endpoints. Agent runtimes have
  none: a runtime that tries is a test failure.
- Egress is an allowlist of the registry's endpoint hosts for the workspace's allowed endpoints,
  enforced at the network layer as well as in code.
- Local-only deployments have no route to any hosted endpoint (INF-12).

### 8.3 Prompt guard and prompt-injection boundaries

Prompt injection through news and filings is the top technical risk (RAID R-05). The gateway is one
layer; mandate spec §8.4 and §8.5 list the others.

- **Before sending** (deterministic, no model): the rendered prompt is scanned for the vault's
  credential formats, broker account identifiers, personal-data fields (journal spec §6.4), and any
  identifier of another workspace. A hit ends the call as `input_rejected`.
- **The guard can be called on its own,** with no model call, no reservation, and no cost (agent
  harness spec §10.1 ask 4), so construction can check `behavior.description` before any call. It
  is the same deterministic check with the same verdict.
- **Inputs are data.** Retrieved text is placed in delimited data sections of the pinned template.
  The template tells the model to treat them as untrusted, but no rule relies on the model obeying.
- **No capabilities.** No tools, no function calls, no browsing (INF-5). Retrieval happens before the
  call, in deterministic code, from allowlisted sources only (DEC-101).
- **After the response:** schema validation, the output limits of §8.1, and the §8.5 checks. Then the
  order builder, autonomy, and the risk gate, none of which read model text.

### 8.4 Data residency and tenancy

- In hybrid and on-prem mode, prompts and outputs stay on the customer's site unless the customer's
  policy allows a hosted endpoint ([HLD, Where data lives](../HLD.md#where-data-lives)).
- Each endpoint records its region. A workspace policy may require a region; calls to others are
  refused.
- One call, one workspace (INF-9). Rate and fairness queues are per workspace, so one tenant cannot
  starve another's calls.

---

## 9. Lifecycle and failure walk

Each case: how it starts, what it blocks, how it ends, who ends it, and what happens across the
risk-day boundary and a restart.

| Case | Entered when | Effect | Ends when | Who ends it | Day boundary and restart |
|---|---|---|---|---|---|
| Provider down | Every endpoint of the identity fails | Calls end `provider_unavailable`; outputs go stale by `max_output_age_s`; the builder sees missing outputs | An endpoint answers a call | Nobody: the next call | Nothing carries over; the next call retries fresh |
| Provider slow | Responses arrive after the deadline | `deadline_exceeded`; late bytes discarded and metered | Responses arrive in time | Nobody | As above |
| Wrong output schema | Response fails parsing or schema | `schema_invalid`; no retry; output missing; journaled with `response_ref` for review | The next call parses | Nobody; repeated failures alert the operator, who may withdraw the model | As above |
| Identity mismatch | Reported identity differs from the pin | `identity_mismatch`; endpoint marked unhealthy; operator alerted | Operator restores or removes the endpoint | Operator | Unhealthy mark is journaled configuration, so it survives restart |
| Rate-limited by provider | 429 | Retry after the provider's wait if the deadline allows; else `rate_limited_provider` | Provider accepts | Nobody | As above |
| Model deprecated | Provider announces retirement | Entry `deprecated`; owners alerted; nothing else changes | Owner confirms a new version, or withdrawal | Owner, then operator | Status is registry state, survives restart |
| Model withdrawn | `PlatformOperatorAction` `model_withdrawn` | Calls refused; outputs count as missing; cache emptied | Never for that version; a new version is a new pin | Operator; owner re-pins | Journaled, so replay sees it |
| Key revoked | Provider 401 or 403, or `KeyRevoked` | `credential_invalid` on every call to that provider; operator alerted | A valid key is in the vault | Operator (managed) or customer admin (hybrid) | Vault state; survives restart |
| Spend cap hit mid-day | A reservation would exceed a cap | `budget_exhausted`; research theses also refused by check 7; outputs missing | The next risk day | Nobody (the clock) | Resets at the risk-day boundary, never by restart or redeploy, since spend is folded from the journal by risk day |
| Workspace quota hit | As above for the workspace | Every agent's calls refused | Next risk day, or the operator raises the quota | Clock or operator (a journaled change) | As above |
| Meter unavailable | The meter writer is unreachable, or a replica's append is fenced | `meter_unavailable`; nothing sent; outputs missing; operator alerted | The holder is reachable, or a new holder has folded the stream | Supervisor (hand-over) | Unsettled reservations stay counted across the hand-over |
| Prompt guard hit | Forbidden content in the prompt | `input_rejected`; nothing sent; journaled with the rule, not the content | The input source stops supplying it | Nobody; repeats alert the operator | Nothing carries over |
| Gateway process crash | Process dies | In-flight calls lost; their reservations stay counted | Gateway restarts | Supervisor | On restart: unfinished reservations are settled as spent at their maximum; no call is re-sent under its old `call_id`; the runtime re-reads fresh outputs from the journal |
| Agent runtime restart | Runtime restarts | No model is called during replay (INF-10) | Replay and reconciliation finish | Runtime | Outputs still fresh by §8.2 count; others are missing until the next call |
| Mandate version change | Owner confirms a new version | Calls for the old pin stop; outputs of a model no longer pinned are ignored (`not_pinned`) | Immediately | Owner | Journaled |

No case ends in the gateway choosing another model, holding an exit, or adding exposure.

---

## 10. Adversary review

| Actor | Attempt | Blocked or disclosed by |
|---|---|---|
| Careless user | Pins an LLM model with a high weight and a long `max_output_age_s` | The weight is the user's choice (rule 11). Missing outputs count bearish for buys (MI-10); the policy caps `max_output_age_s` |
| Careless user | Sets a very high research cost cap | Policy's `research_cost_cap_usd_per_day` and the workspace quota bound it; the bill is the user's, shown in usage |
| Bad model | Returns imperatives, price targets, or out-of-range values | Output schema and §8.1 limits; ignored and journaled |
| Bad model | Claims to be another model, or the provider serves another | `identity_mismatch` (INF-2) |
| Bad model | Returns huge output to run up cost | `max_output_tokens` bounds each call; reservation bounds the day |
| Injected filing or news item | Tells the model to name an instrument or leak data | No tools or egress (INF-5); allowlist, corroboration, eligibility floor, `max_instruments`, autonomy (§8.5); the model has no secrets to leak (INF-8) |
| Malicious insider | Points a registry id at a different model | An entry's pinned content is immutable and signed; the content hash includes identity; the mandate pins the hash (V-007) |
| Malicious insider | Edits a cached response | Cache entries are artifacts addressed by hash; a mismatch is a miss and an alert |
| Malicious insider | Changes the price table to exhaust a cap or inflate a bill | Price tables are versioned configuration, journaled; each call names the version it used |
| Malicious insider | Adds an endpoint serving a cheaper quantization | Endpoint changes are journaled configuration and need the §4.3 identity probe |
| Other tenant | Probes a shared provider-side cache for our prefixes | Only data-free prefixes are cached (INF-9) |
| Other tenant | Floods the gateway | Per-workspace buckets and queues; the hot path never waits on the gateway (INF-13) |
| Provider | Silently changes weights behind a dated snapshot | Not fully detectable. Mitigation: periodic canary probes against the evaluation set, with drift alerts to the operator, who may withdraw. Disclosed to owners as a limit of hosted models |
| Provider | Retains or trains on prompts | Contract terms (DEC-432 item 15); local-only mode for customers who cannot accept that |
| Bad market tick | Feeds a wild price into a prompt | The model's output is an opinion; the builder clips, and the gate's collar and limits decide. The input-drift detector escalates unusual inputs (V-018) |
| Network attacker | Intercepts or redirects calls | TLS to allowlisted hosts; reported identity check; no key in any log |

**Advice risk.** The gateway adds no ranking, "recommended" label, or performance claim to any
model, and it never chooses a model on the user's behalf. A deprecation alert states the fact and
the date only; it does not suggest a replacement.

---

## 11. What exists today and what is planned

| Piece | Today | Planned |
|---|---|---|
| Model client | `python/research_spike/src/research_spike/openrouter.py`: one aggregator, JSON output, temperature 0, cost from a cached price catalogue, 60 s HTTP timeout, no retries. Spike code, not product code | The gateway (E15-6) |
| Pinning | The spike names a model by its aggregator name, with no dated snapshot, no upstream lock, and no check of the reported model | INF-1, INF-2, the registry (E15-7) |
| Deadline | Fixed 60 s transport timeout | Pinned `deadline_ms` per entry (INF-3) |
| Journaling | The spike writes `llm_call` records and prompt and response artifacts to its own hash-chained JSONL. `ModelInvocationRecorded` is registered in `mandate-journal`'s catalogue with its `mod` ref; its payload schema is not closed | §3.6 members, journal spec first (E15-8) |
| Spend | `mandate-research` reads `research_spend_usd_today` and refuses a thesis by check 7. Nothing produces that number in product code | Meter, reservation, and caps (E15-9) |
| Registry | Mandate pins id, version, and content hash (V-007); no registry service | E15-7 |
| Prompt guard, egress allowlist, local-only policy key | None | E15-10 |
| Billing feed | None | §7.4 (E15-11), shown in E14-3's usage view |
| Laya, Jev | Names only (§5) | Decided by the founder (DEC-432 item 13) |

---

## 12. Decisions

[DEC-432](../project/decisions/DEC-432.md) records this spec's decisions.

- **Items 1 to 12, agent readings** (DEC-79, DEC-176): each one tightens a rule or resolves a gap by
  the reading that adds no risk. They include the pinned deadline, late output discarded, no retry
  on content failures, the cost reservation, the prompt-content restriction, the exact per-workspace
  cache, no tools, local-only by default for hybrid and on-prem, the endpoint quantization rule, the
  identity check, the `ModelInvocationRecorded` members, and the RAID R-07 wording.
- **Items 17 to 22, agent readings from the post-merge review** (DEC-79, DEC-176): what the content
  hash covers, the matching mandate spec §8.1 sentence, the compiler's record left open, one meter
  writer per workspace, late responses never cached, and four of the agent harness spec's asks.
- **Item 23, Proposed for the founder:** a prompt record that keeps no licensed text (§13 question 6).
- **Items 13 to 16, decided by the founder on 2026-10-03** (DEC-432, "Founder decisions"): a hosted
  fast model (§5), an aggregator for every call with routing locked per pinned model (§6), provider
  data terms as recommended, and bring-your-own provider keys allowed for hybrid and on-prem, with
  platform keys for managed deployments (§7.4).

---

## 13. Open questions

1. **Answered** ([DEC-670](../project/decisions/DEC-670.md), journal spec v0.21 §9.9): the
   compiler's call record is on the workspace's control stream (§3.6). The meter stream's shape
   stays E15-8's.
2. The policy key that lists allowed endpoints and regions does not exist in `policy.schema.json`.
   Its shape (a set of providers, endpoints, or localities; child ⊆ parent) is part of E15-10.
3. Whether a per-model cost cap becomes an envelope field for fast and LLM signal models, or the
   workspace quota is enough (E15-9).
4. The thresholds of the §4.3 evaluation gate (99% schema conformance) are a first value and need
   review against real probe sets.
5. Canary probes for silent provider changes: how often, and what drift threshold alerts.
6. **A prompt record without licensed text** (agent harness spec §10.1 ask 5; DEC-432 item 23,
   Proposed). The ask: for a call whose inputs hold licensed text, `prompt_ref` names the rendered
   prompt with each licensed item's text replaced by its raw-bytes digest, while `request_digest`
   stays the digest of the full prompt. That keeps less than INF-10 says today, and whether such a
   record meets the retention duty is for the founder and counsel. Until it is decided, INF-10
   stands as written and research reads no licensed text.
