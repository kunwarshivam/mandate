# Journal Spec (v1)

| | |
|---|---|
| **Status** | v0.7 (v0.2 founder sign-off 2026-09-25, [DEC-71](../project/04-decision-log.md#decisions); v0.3 amendment [DEC-81](../project/04-decision-log.md#decisions); v0.4 adds the research-agent events of [DEC-97](../project/04-decision-log.md#decisions) and [DEC-111](../project/04-decision-log.md#decisions); v0.5 approval escalation v0, [DEC-173](../project/04-decision-log.md#decisions), amended by [DEC-181](../project/04-decision-log.md#decisions), whose `DecisionMade` members [DEC-252](../project/04-decision-log.md#decisions) closes in §9.1; v0.6 closes the agent stream's payload schemas, [DEC-177](../project/04-decision-log.md#decisions); v0.7 closes the control-stream schemas `ValidationContext` reads, `AccountSnapshotRecorded`, and `OwnerCommandRefused`, [DEC-261](../project/04-decision-log.md#decisions)); changes need a decision-log entry (safety-critical) |
| **Implements** | PRD 6.7 (FR-7.1 to FR-7.7), FR-5.6, FR-5.7; backlog E5; milestone M4 |
| **Depends on** | [Trading domain spec §12–§13](trading-domain.md#12-journal-events) |
| **Test vectors** | [reference-cases/journal.yaml](reference-cases/journal.yaml) (version 3, with the generated `agent_stream` section of §9.1 and `control_stream` section of §9.2; [reference/journal/generate.py](../../reference/journal/generate.py)) |

The journal is the append-only, hash-chained record of everything the platform does: the source
of truth for agent and account state (event-sourced), the audit trail, and the input to replay.

## Change history

- **v0.7, amended ([DEC-351](../project/decisions/DEC-351.md) item 5):** `OwnerCommandRefused` may
  carry `not_independent`, the executor's refusal of an acknowledgment that lifts a fired tripwire
  under `independent_approval_required` from the user who requested the lift ([mandate spec
  §6.7](mandate.md#67-tripwires-dec-187-dec-350-dec-351)); rule 28 keeps the reason to the account
  stream, and `OwnerAcknowledged` names the requesting user and carries the independence requirement
  as it stood when the lift was requested. The reference validator's reason list
  gains it. Rule 28's validator check, its vectors (an acknowledgment refused as `not_independent`,
  accepted; a Stop refused as `not_independent`, refused), its seeded bug, and `mandate-journal`'s
  schema follow in one code PR, because `mandate-journal` already parses every §9.2 vector and code
  cannot ship with this change (ES-22).
- **v0.7, amended ([DEC-302](../project/04-decision-log.md#decisions)):** §9.2's
  `AccountSnapshotRecorded` gains `risk_clock`, which the executor writes on every account-stream
  event and folds each event at; v0.7 had missed it, so a conforming snapshot could not have been
  replayed. The new `risk_clock` type is a whole-second timestamp. The vectors gain five invalid
  drafts: the member absent, `null`, as integer seconds (the executor's form today), not a
  timestamp, and off the second.
- **v0.7 ([DEC-261](../project/04-decision-log.md#decisions)):** §9.2 closes the payload schemas of
  the control-stream and account-stream records among those `ValidationContext::from_journal` reads
  ([DEC-169](../project/04-decision-log.md#decisions)).
  These are the control stream's `StreamOpened`, `ConnectionEstablished`, `ConnectionRevoked`,
  `DisclosureAccepted`, `ConfigSnapshotRegistered`, `MandateVersionCreated`, `MandateConfirmed`,
  `AgentDeployed`, and `AgentStopped`, and the account stream's `AccountSnapshotRecorded`. §9.2 also
  closes `OwnerCommandRefused` on both streams that write it, with DEC-291's members. Every member
  comes from a writer that exists or from a clause that names it.
  - `MandateVersionCreated`, `MandateConfirmed`, and `AgentDeployed` carry what `JournaledFact` reads
    as members, and the rest of their mandate spec §10 row as a stored record named by its hash.
  - No payload carries a credential or personal data.
  - The new types are `pointer` and `date`, and the new rules are 17 to 27.
  - §9.2 maps each record to its `JournaledFact`.
  - `PlatformOperatorAction` stays open, with a Proposed note, because no clause names the members of
    three of its five actions.
  - The test vectors gain a generated `control_stream` section and stay version 3.

- **v0.6, amended ([DEC-280](../project/04-decision-log.md#decisions)):** the agent stream gains
  `OwnerCommandRefused`, the record of a resume or Stop the runtime refused for its step-up, and the
  account stream the same for an acknowledgment the executor refused ([mandate spec §6.1](mandate.md#61-purposes),
  which already said such a refusal is journaled). §9 lists it on both streams, §2's copy paragraphs
  name it, and copy rule 16 counts it. Its payload schema is **not closed yet** (§9.1), and neither
  are the approval events v0.5 added; the catalogue entry, the closed schema, and the runtime's
  write of it land together in their own change. The test vectors are unchanged.
- **v0.6, amended ([DEC-188](../project/04-decision-log.md#decisions), [DEC-271](../project/04-decision-log.md#decisions)):**
  `DecisionMade`'s `decided_by` may be `review_ceiling`, the review date of
  [mandate spec §6.2 step 5b](mandate.md#62-evaluation), the same label the approval content's
  `trigger.decided_by` carries ([mandate spec §6.4](mandate.md#64-approvals), where its
  `trigger.rule` is `null`). Rule 7 refuses it unless `autonomy` is `ask`, as it refuses
  `client_ceiling`. The clause's vectors (an invalid and a valid draft), the generator's validator
  rule and mutant, and the agent-stream harness's count follow in one code change (ES-22; backlog
  E6-14), as DEC-177 items 6 and 14 did.
- **v0.6 ([DEC-177](../project/04-decision-log.md#decisions)):** §9.1 closes the payload schemas of
  the agent stream's `StreamOpened`, `ObservationRecorded`, `ModelOutputRecorded`, `DecisionMade`,
  `IntentProposed`, `AgentModeChanged`, `KillSwitchActivated`, and `OwnerExitRequested`, and rules on
  every contradiction [DEC-174](../project/04-decision-log.md#decisions) item 3 found in them: `null`,
  never empty strings; timestamps, never risk-clock seconds; artifact references, never inline data;
  `IntentProposed` as the `IntentReceived` vector's intent fields. `OwnerExitRequested` records the
  owner and `step_up_status` on every owner exit; `DecisionMade` carries `exit_origin` and
  `ask_suppressed`, and the order builder's numbers only on decisions it produced. §11 gains the
  per-range checks `intent_action_mismatch` and `mode_event_mismatch`. The test vectors gain a
  generated `agent_stream` section (a hash-chained stream from `StreamOpened` with its artifacts, an
  invalid draft for every rule, valid drafts, batches, and range-verification cases) and stay
  version 3 until the harness reads it. The approval events that v0.5 added are not closed here.
  Reconciled with v0.5 (DEC-177 items 21 to 24): `IntentProposed` may be caused by an
  `ApprovalRevalidated` and then repeats its approval's bound action, and rule 16 makes each copy of
  an owner command name its `OwnerCommandIssued`.
- **v0.5, amended ([DEC-181](../project/04-decision-log.md#decisions)):** `DecisionMade` names the
  source of its autonomy decision and, when a delegation lifted it, the `delegation_id` that
  delegation usage is counted from ([mandate spec §6.5](mandate.md#65-delegations-dec-181-adr-0003));
  `ApprovalRequested`'s content object lists the delegation shapes offered in `choices`, and
  `ApprovalResponded` records the shape chosen.
  `DecisionMade` also records `requested_by`, and the client's id for a client-requested order, and the
  content object's `trigger` carries both
  ([DEC-185](../project/04-decision-log.md#decisions)).
  No new event type. §9.1's closed `DecisionMade` schema lists them as `decided_by`,
  `delegation_id`, `requested_by`, and `client_id`, with rule 5 and rule 7 clauses that tie them
  to `autonomy` and refuse a client-requested opening recorded as `auto`
  ([DEC-252](../project/04-decision-log.md#decisions)); the generated `agent_stream` vectors cover
  every clause and the test vectors stay version 3.
- **v0.5 ([DEC-173](../project/04-decision-log.md#decisions), with [DEC-155](../project/04-decision-log.md#decisions),
  [DEC-156](../project/04-decision-log.md#decisions), and [DEC-158](../project/04-decision-log.md#decisions)):**
  approval escalation v0 ([mandate spec §6.1, §6.4](mandate.md#64-approvals)). The workspace
  control stream gains `ApprovalResponseSubmitted` and `OwnerCommandIssued`, and `OwnerAcknowledged`
  carries step-up evidence; owner input is journaled there first and copied by the agent runtime
  with causation (§2). The agent stream's approval rows are split: `ApprovalRequested` carries the
  content object and its hash, `ApprovalResponded` the admission result with the quorum it
  applied, and the new `ApprovalRevalidated` the re-validation result; `DecisionMade` gains
  `ask_suppressed`. Test vectors are unchanged (version 3).
- **v0.4 ([DEC-97](../project/04-decision-log.md#decisions), [DEC-111](../project/04-decision-log.md#decisions)):**
  `ThesisProposed` and `ThesisRevised` join the agent stream and `UniverseChanged` the account
  stream, where it is a risk input carrying `risk_clock` ([mandate spec §2.3, §8.4 to
  §8.6](mandate.md#23-the-working-universe-at-runtime-dec-97)); `PlatformOperatorAction` gains the
  research-agent halt of [DEC-100](../project/04-decision-log.md#decisions); `ModelOutputRecorded`
  carries the thesis direction and lineage. Test vectors are unchanged (version 3): the new events
  have no vector until the Rust harness exists.
- **v0.3 ([DEC-81](../project/04-decision-log.md#decisions)):** `risk_clock` is required on every
  account-stream risk input, named by reference to the mandate spec §5.2 list; `OwnerAcknowledged`
  is copied into the account stream as a risk input; test vectors version 3 (the `FillApplied`
  vector carries `risk_clock`).
- **v0.2:** engineer review ("needs rework") and compliance review ("approve with changes").
  Stored bytes are exactly the hashed bytes; complete canonical rules (null vs absent, key charset,
  escaping, integer and decimal grammar, timestamps); writer/journal field split, idempotency order,
  gap prevention and writer fencing; Postgres append-only hardening; self-contained per-stream
  folds; required configuration references per event type; full gate-decision records; Merkle
  anchoring definition; ordered verification checks with codes; retention, access, export,
  redundancy, clock, and incident rules; expanded catalogue; regenerated test vectors.
- **v0.1:** initial draft.

## 1. Principles

1. **Append-only.** Events are never updated or deleted. Corrections are new events
   (`CompensatingEvent`) that reference what they correct.
2. **Write before acting.** An event describing an intent, decision, or submission is durably
   committed before any side effect it authorizes.
3. **One writer per stream,** fenced by an epoch. Ordering within a stream is total and gapless.
4. **Self-contained folds.** A stream's state is a pure fold over **its own events** plus
   content-addressed configuration. Facts from other streams are copied in by the stream's owner.
5. **Tamper-evident, then write-once.** Each event commits to its predecessor by hash; stream heads
   are anchored externally; closed segments move to write-once storage.
6. **Identity is retained; personal data is referenced.** Events carry opaque references; personal
   data lives in a vault, is retained for the records retention period, and is erased only after it.

## 2. Streams

| Stream type | `stream_id` | Owner (single writer) | Contents |
|---|---|---|---|
| Account | `acct:{workspace_id}:{account_ref}` | The account's executor (trading spec §7.1); the risk gate is a pure library it calls | Intents received, gate decisions, orders, fills, fees, cash, settlement, corporate actions, reconciliation, account state, restrictions, protection changes, copies of gating facts from other streams |
| Agent | `agent:{workspace_id}:{agent_id}` | The agent's runtime | Observations, model invocations, signal-model outputs, decisions, intents proposed, approvals, agent mode changes |
| Workspace control | `ctl:{workspace_id}` | Workspace control services (in Phase 1, the founder's CLI) | Mandates, deployments, connections, disclosures, policy and configuration registration, owner acknowledgments, approval responses, owner commands, and alerts, surveillance reports, anchors, verification, records lifecycle, access and export |
| Scheduler | `clock:{workspace_id}` | The workspace scheduler | `ClockAdvanced`, `TradingDayStarted`, clock measurements |

- **Identifier grammar:** every `{…}` segment matches `[A-Za-z0-9_-]+`. `account_ref` is an opaque
  internal ULID; the broker's account number lives in the personal-data vault.
- **No global order** across streams. Relationships use `causation_id` and `correlation_id`;
  cross-stream displays sort by `recorded_at` for readability only.
- **Cross-stream facts are copied by the owner** into the consuming stream with a `causation_id`:
  the executor writes `AgentModeApplied` (from the agent stream's `AgentModeChanged`),
  `UniverseChanged` (from the agent stream's `ThesisProposed` or `ThesisRevised`, or from a
  `MandateVersionApplied` that changed a pinned universe; mandate spec §2.3),
  `TradingDayStarted`, `ClockAdvanced` (the risk clock, mandate spec §5.2: copied only when a tick
  emits an event or crosses midnight), and time-driven events
  (from the scheduler) into the account stream, and derives `RiskDayStarted` there from the copied
  `ClockAdvanced` crossing midnight America/New_York. Every other account-stream risk input
  ([mandate spec §5.2](mandate.md#52-inputs-the-risk-clock-and-determinism): `MarkUpdated`,
  `FillApplied`, `LateFillApplied`, `FeesCharged`, `CorporateActionApplied`, `CashInLieuPosted`,
  `CompensatingEvent`, `MandateVersionApplied`, `RiskDayStarted`, and the copied
  `OwnerAcknowledged`) carries a required `risk_clock` payload field (whole-second timestamp, the
  latest tick the executor had seen); `append` rejects a `risk_clock` lower than the stream's last
  one. Owner acknowledgments are recorded in the control stream and copied by the executor into the
  account stream as `OwnerAcknowledged`, with `causation_id` pointing to the original; one whose
  step-up does not count is recorded there as `OwnerCommandRefused` instead, with the same
  `causation_id`, so the account stream records every acknowledgment the executor read
  ([mandate spec §6.1](mandate.md#61-purposes)). A mode change that **originates** on the account stream (account restrictions,
  mandate risk limits) is journaled there first as `AgentModeApplied`, and the agent runtime
  copies it into the agent stream as `AgentModeChanged`; the `causation_id` always points to the
  originating event. **Owner input is journaled on the control stream first**: an answer to an
  approval as `ApprovalResponseSubmitted`, and a pause, resume, Stop, owner exit, or kill switch as
  `OwnerCommandIssued` ([mandate spec §6.1](mandate.md#61-purposes)). The agent runtime copies each
  event addressed to its agent at most once, with `causation_id` pointing to it (`ApprovalResponded`,
  `AgentModeChanged`, `OwnerExitRequested`, `KillSwitchActivated`, or, for a resume or Stop its
step-up does not count, `OwnerCommandRefused`); the control stream's `event_id`
  is the idempotency key. A user's kill switch is therefore a **command**
  to the stream owners, which journal `KillSwitchActivated` in their own streams.
- A stream begins with `StreamOpened` (seq 1), which records the stream type, subject, and
  environment.
- `intent_id` equals the `event_id` of the agent stream's `IntentProposed`.

## 3. Event envelope

The **body** is the envelope without `hash`. Fields marked *journal* are assigned by the journal
service at append; all others come from the writer's draft.

| Field | Source | Type and rules |
|---|---|---|
| `envelope_version` | writer | Integer; `1` for this spec |
| `environment` | writer | `paper`, `live`, or `backtest` |
| `event_id` | writer | ULID; globally unique; idempotency key. The ULID time component carries no meaning |
| `stream_id` | writer | Per §2 |
| `seq` | *journal* | Integer ≥ 1; gapless; the **only** ordering key in a stream |
| `prev_hash` | *journal* | 64 lowercase hex; hash of the previous event; 64 zeros for seq 1 |
| `event_type`, `schema_version` | writer | Catalogue name (§9) and payload schema version |
| `event_time` | writer | Timestamp at the source (broker, exchange, local); informational |
| `recorded_at` | *journal* | Timestamp when appended; informational |
| `clock_source` | writer | `broker`, `exchange`, `local`, `scheduler` |
| `causation_id`, `correlation_id` | writer | Event ID or `null` |
| `actor` | writer | `{kind, id, version, build}`; `kind` ∈ `system`, `agent`, `user`, `broker`, `platform_operator`; `build` is the binary digest (`sha256:…`) or `null` for external actors; users are opaque IDs |
| `config_refs` | writer | Map of content hashes; required keys per event type (§9) |
| `payload` | writer | Per the event type's schema |
| `artifact_refs` | writer | Sorted, de-duplicated set of every `sha256:` reference in the payload |
| `pii_refs` | writer | Sorted set of opaque vault references (random IDs, never hashes of personal data) |
| `hash` | *journal* | SHA-256 of the canonical body; **not part of the body** |

## 4. Canonical serialization

The canonical form is JSON per **RFC 8785** (JCS), with these constraints that make it
implementation-independent:

1. **Keys** match `^[a-z][a-z0-9_]{0,63}$` (so code-point and UTF-16 ordering agree). Maps keyed by
   data (for example, signal-model weights) are encoded as arrays of `{key, value}` sorted by key bytes.
2. **Presence:** envelope fields and schema-declared payload fields are always present, `null` when
   empty. Map-valued objects (such as `config_refs`) omit absent keys and never contain `null`.
   Empty collections are `[]` or `{}`, never `null`. Schemas use `required` and
   `additionalProperties: false`.
3. **Strings:** escape only `"`, `\`, and U+0000–U+001F, using `\b \f \n \r \t` where defined and
   lowercase `\u00xx` otherwise; `/`, U+007F, U+2028, U+2029, and all non-ASCII characters are raw
   UTF-8; no Unicode normalization. **Lone surrogates are rejected** at ingest.
4. **Integers** only for listed fields (`envelope_version`, `seq`, `schema_version`, `attempt`,
   counts and whole-second durations declared as integers in a schema), in `0 … 2^53 − 1`, no leading zeros. **Money, quantities, and prices are never integers.**
5. **Booleans** are `true` / `false`. **Floating-point numbers are rejected.**
6. **Decimals are strings** matching `^-?(0|[1-9][0-9]*)(\.[0-9]*[1-9])?$`, excluding `-0`, with at
   most 28 fractional digits and absolute value below 7.9 × 10²⁸. Inputs are normalized (exponent
   removed, trailing fractional zeros and trailing point removed, `-0` → `0`); values outside the
   bounds or not parseable (for example `NaN`, `Infinity`, `+1`, `1_000`, whitespace) are
   **rejected, never rounded**.
7. **Timestamps** match `^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{9}Z$` (UTC; seconds 00–59, leap
   seconds smeared; years 1970–9999). Dates are `YYYY-MM-DD`.
8. The canonicalizer serializes a **sorted value tree**; it never relies on struct declaration order
   or insertion-order maps.

**Hash:** `hash = SHA-256(canonical body bytes)`, written as lowercase hex.

## 5. Writing

### 5.1 Append protocol

`append(stream_id, expected_head, writer_epoch, drafts[])`, where `expected_head` is the current
head `seq` (0 for an empty stream). The journal service, not the writer, assigns `seq`,
`prev_hash`, `recorded_at`, and `hash`. In one transaction:

1. **Idempotency first.** For each draft, look up `event_id`. If every draft already exists with an
   identical canonical draft, return `AlreadyCommitted` with the stored events. If any exists with
   different content, or the batch partially overlaps, fail with `IdempotencyConflict`.
2. **Fencing and head check:** `UPDATE stream_heads SET … WHERE stream_id = $s AND seq =
   $expected_head AND writer_epoch = $epoch`. No row updated → `HeadMismatch` (returns the actual
   head) or `Fenced` (a newer epoch owns the stream).
3. Assign `seq = head + 1, …`, chain `prev_hash`, compute `hash`, insert the events, and update the
   head. Batches are all-or-nothing.

Outcomes: `Committed`, `AlreadyCommitted`, `HeadMismatch`, `IdempotencyConflict`, `Fenced`,
`Invalid` (schema, missing required `config_refs`, non-canonical values), `Unavailable` (safe to
retry with the same drafts), `Ambiguous` (the writer must re-query by `event_id` before acting).

A new writer takes ownership by incrementing `writer_epoch`; the previous writer is fenced out.

### 5.2 Write before acting, and crash recovery

- Components act only after `Committed` or `AlreadyCommitted`. The executor journals
  `OrderSubmitted` **before** the broker request.
- **Recovery:** the owner folds its stream; for every `OrderSubmitted` without a broker
  acknowledgment, it queries the broker by `client_order_id` and resubmits with the same ID only if
  the broker confirms the order is absent (trading spec §5.7). Domain duplicates (fill IDs,
  attempts) are rejected by folded state; `event_id` covers transport retries only.

### 5.3 Durability and latency

- **Durable** means committed with `synchronous_commit = on`. In managed live deployments, at least
  one synchronous standby in a second failure domain must acknowledge before the append returns.
- Group commit is allowed; `prev_hash` is chained across unacknowledged events only within one
  transaction, and any error discards the in-memory head and re-reads it.
- **Target:** p99 append under 5 ms including the synchronous standby; benchmarked in M4.

### 5.4 Clock

`recorded_at` comes from the host clock, disciplined by NTP. The scheduler measures offset at
startup, before the regular session, and at least every 10 minutes, recording sources and stratum in
`ClockOffsetRecorded`. If the offset exceeds the tolerance (default 50 ms), it journals
`ClockToleranceExceeded`, and events recorded while out of tolerance are flagged in exports. Broker
`event_time` remains authoritative for executions.

## 6. Storage

### 6.1 Hot store (Postgres)

| Table | Columns |
|---|---|
| `events` | `stream_id`, `seq` (primary key), `event_id`, `event_type`, `schema_version`, `environment`, `recorded_at`, `prev_hash bytea`, `hash bytea`, **`body bytea`** (exactly the canonical body bytes); `CHECK (octet_length(hash) = 32 AND hash = sha256(body))`; `workspace_id` with row-level security in multi-tenant deployments |
| `event_ids` | `event_id` (primary key), `stream_id`, `seq` — written in the same transaction (global uniqueness under partitioning) |
| `stream_heads` | `stream_id` (primary key), `seq`, `hash`, `writer_epoch` |

- The body is never stored as `json` or `jsonb`, and readers never re-serialize to verify.
- Partitioning, when needed, is by hash of `stream_id`.
- **Append-only enforcement:** a migration-only role owns the tables. Application roles have INSERT
  and SELECT only; UPDATE, DELETE, TRUNCATE, and TRIGGER are revoked; `BEFORE UPDATE`, `BEFORE
  DELETE`, and `BEFORE TRUNCATE` triggers reject changes. No application or operator role has DDL
  rights. DDL, trigger changes, and superuser sessions are logged to write-once storage and alert.
- **No hot-store eviction in Phase 0.** Later, a separate evictor role may delete only rows with
  `seq ≤` the last verified cold segment, journaling `SegmentEvicted`.

### 6.2 Cold store, segments, and retention

- **Segments** are contiguous `seq` ranges of one stream, shipped continuously to object storage
  (**within 1 minute** of closing before live capital; daily is acceptable in Phase 0).
- **Segment format:** JSON Lines with LF separators and a trailing LF; each line is the canonical
  form of `{"body": <body>, "hash": "<hex>"}`. Because `"body"` sorts before `"hash"`, the body bytes
  are an exact slice of the line. A **manifest** (canonical JSON: stream, first and last `seq`,
  first `prev_hash`, last `hash`, file SHA-256) is hashed and referenced by a `SegmentExported` event.
- **Object lock in compliance mode** (not governance mode) for segments, manifests, artifacts,
  configuration objects, timestamp tokens, and verifier releases. Retain-until = creation + 6 years,
  extended by a job (journaled as `RetentionExtended`) while the supported position, lot, or account
  remains open, and by legal holds (`LegalHoldChanged`). Hybrid and on-prem installers check lock
  mode and retention at startup and refuse live trading if they are wrong.
- A second-region replica of the cold store exists before live capital; a restore-and-verify drill
  runs quarterly and is journaled.

### 6.3 Artifacts

Large items (model prompts and responses, data snapshots, reports) are stored under
`sha256:{hex}` of their stored bytes. Artifacts that may contain personal data are encrypted under a
vault-held key and hashed as ciphertext. Raw broker requests and responses (credentials redacted)
below the size threshold are inline in `payload.raw`; larger ones are artifacts.

### 6.4 Personal data and identity

- Personal data (names, emails, phone numbers, broker account numbers) lives in the vault under
  per-person keys; events carry `pii_refs`.
- **Identity records** (who acted, their role, and identity-provider subject at the time) are
  retained for the records retention period under the legal-obligation exemption; erasure happens
  only after retention ends and no legal hold applies, journaled as `PersonalDataErased`.
- Free text destined for artifacts (plain-language mandates, chat replies, prompts) is scanned and
  personal data is redacted or encrypted before storage.

### 6.5 Encryption and keys

Storage is encrypted at rest with per-workspace keys (bring-your-own-key for businesses). Key
versions are retained for the retention period; rotation never destroys old versions; rotations and
revocations are journaled (`KeyRotated`, `KeyRevoked`). Hashes are over plaintext canonical bodies;
anchors reveal only hashes.

## 7. Access

| Role | Access |
|---|---|
| Owner, workspace admin | Read their workspace's records; request exports |
| Auditor (read-only) | Read records and verification results; exports |
| Records custodian | Legal holds, retention extensions, examination bundles |
| Platform staff (managed mode) | **Break-glass only**, with customer approval, journaled to a stream the customer can read |

Every read of records outside normal product views and every export is journaled (`RecordsAccessed`,
`ExportCreated`). In hybrid mode the platform receives anchors (hashes) only.

## 8. Replay, snapshots, and versioning

- **Replay** folds one stream from seq 1 (or a verified snapshot) in `seq` order. Configuration is
  resolved by `config_refs` from the immutable configuration store; a missing object halts replay.
- **Snapshots** are the canonical JSON of `{stream_id, seq, last_event_hash, fold_version, state}`,
  hashed. Starting from a snapshot requires `last_event_hash` to equal the stored hash at that seq.
  Snapshots are compared only within the same `fold_version`; a fold change requires a journaled
  re-derivation. A full replay from seq 1 runs periodically.
- **Schema versions:** stored events are never rewritten. Upcasters are pure, registered per
  `(event_type, from_version)`, and covered by golden tests on stored bytes. All versions are kept
  for the retention period. The envelope field set is frozen per `envelope_version`.
- **Late events** are appended at their arrival `seq`; replay never re-sorts by `event_time`.

## 9. Event catalogue

Payload schemas live in code with JSON Schema exported to `schemas/events/`. **Required
`config_refs`** keys are enforced at append (`fee` = `fee_config`, `cal` = `trading_calendar`,
`set` = `settlement_calendar`, `ins` = `instrument_snapshot`, `rule` = `rule_set`, `man` =
`mandate_version`, `mod` = `model_version`).

**Account stream** (owner: executor). Risk inputs also carry `risk_clock` (§2).

| Event type | Required refs | Key payload fields |
|---|---|---|
| `StreamOpened` | — | stream type, subject, environment |
| `IntentReceived` | man | intent ID, agent, instrument, side, type, TIF, quantity, limit, purpose |
| `GateDecided` | fee, cal, ins, rule, man | intent ID, verdict, reason code, **`checks: [{id, result, inputs, computed}]`** with IDs matching trading spec §9.1 (`account_status`, `agent_mode`, `eligibility`, `concentration`, `order_size`, `session`, `halt`, `order_constraints`, `mark_freshness`, `collar`, `conduct`, `buying_power`, `gross_exposure`, `day_trade_budget`), `quotes_used`, `marks_used`, `data_profile` |
| `OrderSubmitted`, `OrderStateChanged`, `OrderAbandoned` | — | client order ID, attempt, broker status, internal state |
| `BrokerExchangeRecorded` | — | direction, endpoint, `raw` (or artifact), status; credentials redacted |
| `FillApplied`, `LateFillApplied` | fee, cal, set, ins | fill ID, client order ID, gross quantity, price, fees, trade date |
| `FeesCharged` | fee | family, day, accrued, charged |
| `MarkUpdated` | — | instrument, price, source, feed |
| `SettlementPosted`, `DividendPaid`, `CashInLieuPosted` | set | date, instrument, amount |
| `CorporateActionPrepared`, `CorporateActionApplied` | ins | instrument, action, ratio or amount, ex-date |
| `ProtectionChanged` | — | instrument, action, orders, unprotected-interval start or end |
| `BrokerPositionObserved`, `ReconciliationRun`, `CompensatingEvent`, `AccountSnapshotRecorded` | — | observed values, differences, corrected event IDs, daily snapshot |
| `AccountStateObserved`, `RejectObserved`, `AccountRestrictionChanged` | — | status, flags, reject code and message, restriction |
| `ExternalActivityIngested`, `RelatedAccountsCoordination` | — | unattributed activity; canceled opening orders across the group |
| `ConductBreachDetected` | rule | control, agent, instrument, measured value |
| `AgentModeApplied`, `TradingDayStarted`, `KillSwitchActivated` | — | gating facts, copied or originated (with `causation_id`); kill-switch scope, initiator, orders canceled, sells planned or deferred |
| `OwnerAcknowledged` | — | copied from the control stream (with `causation_id`); a risk input |
| `OwnerCommandRefused` | — | An acknowledgment the executor refused, for its step-up or, under `independent_approval_required`, because it is not independent ([mandate spec §6.1](mandate.md#61-purposes), [§6.7](mandate.md#67-tripwires-dec-187-dec-350-dec-351)): the command (`acknowledge`), the reason (`step_up_missing`, `step_up_stale`, `step_up_reused`, `step_up_method`, `not_independent`), and the effective time it was judged at; `causation_id` is the control stream's `OwnerAcknowledged`, copied at most once. The agent runtime records a refused resume or Stop the same way on the agent stream |
| `MandateVersionApplied`, `RiskDayStarted`, `RiskLimitTriggered`, `RiskLimitLifted`, `HighWaterMarkReset`, `PositionReleased`, `InstrumentRestrictionChanged`, `GoalCompleted` | man | agent risk state ([mandate spec §5.10](mandate.md#510-journal-events)): version result, classification, and allocation change; day-start equity; limit, action, E, H, drawdown, E₀, capital base C, inherited loss L, net contributed N, and for a tripwire (limit `tripwire:<id>`, reason `tripwire_condition`, [mandate spec §6.7](mandate.md#67-tripwires-dec-187-dec-350-dec-351)) its metric, threshold, and the value reached; reset evidence; released positions; stale-mark and removed-instrument changes with the reason; goal completion |
| `UniverseChanged` | man | The working universe changed ([mandate spec §2.3, §8.5](mandate.md#23-the-working-universe-at-runtime-dec-97)); a risk input, so it carries `risk_clock`: agent, instrument, change (`admitted`, `removed`), reason (`thesis_admitted`, `thesis_expired`, `thesis_invalidated`, `lineage_retired`, `eligibility_lost`, `operator_halt`, `version_applied`), thesis and lineage ids, working-universe size after |

**Agent stream** (owner: agent runtime)

| Event type | Required refs | Key payload fields |
|---|---|---|
| `StreamOpened` | — | stream type, subject, environment |
| `ObservationRecorded` | — | source, instrument, data (artifact) |
| `ModelInvocationRecorded` | mod | purpose (compiler, fast model, research), provider, model and version, parameters, seed, prompt and retrieved context (artifact), response (artifact), provider request ID |
| `ModelOutputRecorded` | man | signal model id, version, content hash, instrument, as_of, expires_at, direction, conviction, confidence, horizon, thesis (artifact) and, for the research agent, its thesis and lineage ids; `ignored` reason if not used |
| `ThesisProposed`, `ThesisRevised` | man, mod | The research agent's output and its admission decision ([mandate spec §8.4, §8.5](mandate.md#84-the-research-agent-dec-97-adr-0002)): research agent id, version, and content hash; thesis id, lineage id, revision, and for `ThesisRevised` the `predecessor_thesis_id` and what the revision changed; instrument, asset class, direction, horizon, evidence and cited sources, corroboration kind, invalidation, conviction, confidence; the source-allowlist version; prompt and response (artifacts); `admitted` and the refusal reason from the ordered §8.5 checks |
| `DecisionMade` | man | proposed action, combined conviction and combined score, outputs used, model weights, clips applied, gate dry-run result, autonomy classification and its source (`rule:<id>`, `default`, built-in, the admission ceiling, or the client ceiling), `delegation_id` when a delegation lifted it ([mandate spec §6.5](mandate.md#65-delegations-dec-181-adr-0003)), and `requested_by` (`agent`, `owner`, or `client`) with the client's id when a connected client asked (mandate §6.2 step 5a, DEC-185); `ask_suppressed` (`budget`, `skipped_today`, `recent_timeout`) when an `ask` was classified but not asked ([mandate spec §6.4](mandate.md#64-approvals)) |
| `IntentProposed` | man | intent fields (its `event_id` is the intent ID); after a grant, `causation_id` is the `ApprovalRevalidated` |
| `ApprovalRequested` | man | approval (its `event_id`), instrument, asset class, side, quantity, limit price, purpose, mandate version, `decided_by`, `approvers_required`, `independent_required`, `reference_mark` (`{price, seq}` or null), deadline, `timeout_s`, `on_timeout: skip`, the content object inline (large parts by artifact reference), `content_hash` |
| `ApprovalDelivered` | man | approval, channel, delivery status (`delivered`, `suppressed_quiet_hours`, `failed`), message ID |
| `ApprovalResponded` | man | approval, verdict (`approved`, `skipped`; a legacy `denied` reads as `skipped`), responder (opaque) and role, result (`admitted`, `counted`, `refused`; a legacy `recorded` or `refused` reads as terminal), reason, effective time, step-up evidence (assertion ID, authentication time, method), separation-of-duties result, and for a grant that reaches check 7 the approver count and independence it applied (the stricter of the bound values and the policy overlay, [mandate spec §6.4](mandate.md#64-approvals)); `causation_id` is the `ApprovalResponseSubmitted`, copied at most once; the delegation shape chosen, if any, with the new mandate version and delegation id (mandate §6.4, §6.5) |
| `ApprovalRevalidated` | man | approval, result (`act`, `skip`), reason, and every value compared: bound and current mandate version, mode, instrument restriction, `decided_by` then and now, dry-run verdict and reason, `m_req`, `m_now`, `band_bp` |
| `ApprovalTimedOut`, `ApprovalCanceled` | man | approval, `on_timeout: skip`; approval, cancel reason (`version_applied`, `mode_tightened`, `owner_pause`, `owner_stop`, `kill_switch`; a legacy `rebound` is a cancellation for either of the first two) |
| `AgentModeChanged`, `KillSwitchActivated` | — | from, to, reason; scope and initiator |
| `OwnerExitRequested` | man | instrument or scope, bid shown and confirmed, user (opaque), step-up evidence |
| `OwnerCommandRefused` | — | A resume or Stop the runtime refused for its step-up ([mandate spec §6.1](mandate.md#61-purposes)): the command (`resume`, `stop`), the reason (`step_up_missing`, `step_up_stale`, `step_up_reused`, `step_up_method`), and the effective time it was judged at; `causation_id` is the `OwnerCommandIssued`, copied at most once. The executor records a refused acknowledgment the same way on the account stream |

**Workspace control stream** (owner: workspace services)

| Event type | Required refs | Key payload fields |
|---|---|---|
| `MandateVersionCreated`, `MandateConfirmed` | — | per [mandate spec §10](mandate.md#10-records-dec-51-dec-97): source text (artifact), compiled fields, provenance per path with quoted spans, template, policy-set hashes, validation results and warnings, classification, diff; version hash, confirmed paths, rendered confirmation (artifact) and UI build, warnings acknowledged, step-up evidence, confirming user (opaque) |
| `AgentDeployed`, `DeploymentRejected`, `AgentStopped` | man | agent, mandate version, reason (`goal_complete`, `profit_stop_reached`, `end_date`, owner stop), net dollar loss added to the connection's loss carry; for `AgentDeployed`: the rendered go-live screen (artifact), backtest and paper-run IDs shown, performance legend and disclosure versions shown, approving users, step-up evidence ([mandate spec §10](mandate.md#10-records-dec-51-dec-97)) |
| `PolicyChanged`, `WorkspaceProfileAssigned` | — | level, diff, author (opaque), step-up evidence, affected agents; profile, basis, assigning user |
| `ConnectionEstablished`, `ConnectionRevoked` | — | broker, scopes granted, permission-check result |
| `DisclosureAccepted` | — | document and version hash, user (opaque), step-up evidence |
| `OwnerAlertSent`, `OwnerAcknowledged` | — | subject event, channel, delivery status; user (opaque), the user who requested the lift (opaque) and the independence requirement as it stood when the lift was requested, carried so the executor applies the stricter of it and the overlay at processing ([mandate spec §5.8, §6.7](mandate.md#67-tripwires-dec-187-dec-350-dec-351)), authentication method, step-up evidence (assertion ID, authentication time, method) |
| `ApprovalResponseSubmitted` | — | The owner's answer to an approval ([mandate spec §6.4](mandate.md#64-approvals)): agent, approval, verdict (`approved`, `skipped`), content hash, `submitted_at`, step-up evidence (assertion ID, authentication time, method) or null, responder (opaque) and role |
| `OwnerCommandIssued` | — | The owner's command ([mandate spec §6.1](mandate.md#61-purposes)): agent or kill-switch scope, command (`pause`, `resume`, `stop`, `kill_switch`, `owner_exit`), the release choice and warning shown for a Stop with release, the bid, bid size, and floor confirmed for an owner exit, `submitted_at`, step-up evidence or null, user (opaque) |
| `ConfigSnapshotRegistered` | — | configuration kind (fee, calendar, instrument snapshot, rule set, mandate), content hash |
| `SurveillanceReportGenerated`, `BacktestRunRecorded` | rule | period, report (artifact), breaches; data snapshot, code build, configuration, results, paper/live/backtest marker |
| `PlatformOperatorAction` | — | action (stop, global kill switch, acceptable-use action, `model_withdrawn` with model and reason, `research_thesis_halt` with the instrument and optionally the research agent's pinned content hash, [DEC-100](../project/04-decision-log.md#decisions)), operator (opaque), approval |
| `AnchorComputed`, `VerificationRun`, `IntegrityIncidentRecorded` | — | leaves, root, timestamp token (artifact); scope and result; last good hash and anchor |
| `SegmentExported`, `SegmentEvicted`, `RetentionExtended`, `LegalHoldChanged` | — | manifest hash, range, retain-until, hold |
| `KeyRotated`, `KeyRevoked`, `RecordsAccessed`, `ExportCreated`, `PersonalDataErased` | — | key version; accessor (opaque), scope; export manifest; subject reference |

**Scheduler stream:** `ClockAdvanced`, `TradingDayStarted`, `ClockOffsetRecorded`,
`ClockToleranceExceeded`.

### 9.1 Agent-stream payload schemas ([DEC-177](../project/04-decision-log.md#decisions))

This subsection closes the payload schemas of the agent stream's `StreamOpened`,
`ObservationRecorded`, `ModelOutputRecorded`, `DecisionMade`, `IntentProposed`, `AgentModeChanged`,
`KillSwitchActivated`, and `OwnerExitRequested`. For these events it replaces the "Key payload fields"
column of §9, and the required `config_refs` stay as §9 lists them. Each schema is `schema_version` 1:
a record with exactly the listed members, every one present (§4.2). The test vectors' `agent_stream`
section holds at least one chain event per schema, an invalid draft for every rule below, and valid
drafts for the cases a rule might be misread to refuse. The other agent-stream events are not
closed yet: the approval events that v0.5 added (§9, mandate spec §6.4) close in their own change,
`ModelInvocationRecorded`, `ThesisProposed`, and `ThesisRevised` with their own stories.
`OwnerCommandRefused` is closed in §9.2, on both streams that write it.

**Types.**

| Type | Values | Refused as |
|---|---|---|
| `text` | A non-empty string; an empty value is `null` (§4.2) | Not a string: `schema`; empty: `non_canonical` |
| `id` | `[A-Za-z0-9_-]+` (§2) | Not a string: `schema`; otherwise `non_canonical` |
| `ulid` | An event ID (§3) | As `id` |
| `decimal` | §4.6, normalized on the way in | As `id` |
| `integer` | §4.4 | `schema` |
| `boolean` | `true` or `false` (§4.5) | `schema` |
| `timestamp` | §4.7. **Every instant is a timestamp**, never risk-clock seconds or an integer | As `id` |
| `ref` | `sha256:` and 64 lowercase hex: an artifact (§6.3) or a content hash. It is listed in `artifact_refs` (§3), so the object must be stored and re-hashes (§11 check 6) | As `id` |
| `a` \| `b` | One of the listed strings | As `id` |
| `T?`, `[T]` | `T` or `null`; an array of `T` | The array itself: `schema` |

A member not listed, or a listed member absent, is `schema` at that member; an absent member is
never read as `null` (§4.2), even where `null` would be valid. The first violation is
reported, in this order: unlisted members (in key order), then the listed members in the order given,
then the numbered consistency rules in number order, each rule's clauses in the order given (only on
a payload whose members are all well typed), then `artifact_refs` and `pii_refs` (§3), then the
subject rules 14 and 15 (`stream_mismatch`), then the copy rule 16 (also only on a well-typed
payload). Rule 10's second clause spans a batch, so it is checked only once every draft in the batch
passes these checks, and is reported at the first `IntentProposed` in the batch that breaks it.
Paths are dotted from the envelope (`payload.step_up.authenticated_at`).

**`StreamOpened`** on the agent stream. The schema of `StreamOpened` and of `KillSwitchActivated` is
chosen by the stream type of `stream_id`; the account stream's are unchanged.

| Member | Type | Meaning |
|---|---|---|
| `stream_type` | `agent` | |
| `workspace_id`, `agent_id` | `id` | The subject: rule 14 |

**`ObservationRecorded`**

| Member | Type | Meaning |
|---|---|---|
| `source` | `text` | The feed or connector the data came from |
| `instrument_id` | `text?` | `null` for data about no single instrument |
| `as_of` | `timestamp` | The data's cut-off |
| `data_ref` | `ref` | The observed data, stored as an artifact, never inline |

**`ModelOutputRecorded`** ([mandate spec §8.2](mandate.md#82-output)). An output is recorded as the
model gave it; one the agent does not use carries the reason in `ignored`.

| Member | Type | Meaning |
|---|---|---|
| `model_id`, `model_version` | `text` | As the output states them |
| `content_hash` | `ref` | The model's content hash ([mandate spec §8.1](mandate.md#81-signal-model-contract-dec-52-dec-97)); the content object is an artifact |
| `instrument_id` | `text` | |
| `as_of`, `expires_at` | `timestamp` | |
| `direction` | `text` | As given; a direction v1 does not allow is recorded and ignored |
| `conviction`, `confidence` | `decimal` | As given |
| `horizon_s` | `integer` | Seconds |
| `thesis_ref` | `ref?` | The thesis, as an artifact |
| `evidence` | `[ulid]` | Event IDs, in the order given |
| `invalidation` | `text?` | |
| `thesis_id`, `lineage_id` | `id?` | Research agent only: rule 13 |
| `ignored` | `not_pinned` \| `model_withdrawn` \| `output_limits` \| `not_in_universe` \| `direction_not_allowed` \| `horizon_mismatch` \| `revision_without_predecessor`, or `null` | `null` when the output is used. Otherwise why not: identity differs from the pinned values (§8.2), the version was withdrawn (§8.1), an LLM output broke the output limits (§8.1), the instrument is outside the working universe (§8.2), or the first three §8.5 checks |

**`DecisionMade`** (mandate spec §6.2, §8.3): one proposal and its outcome. The action members are
`IntentProposed`'s.

| Member | Type | Meaning |
|---|---|---|
| `instrument_id` | `text` | |
| `side` | `buy` \| `sell` | Rule 1 |
| `type` | `limit` \| `market` | Rules 2 and 3 |
| `tif` | `day` \| `gtc` \| `ioc` | [Trading spec §5.1, §5.2](trading-domain.md#51-v1-order-policy-dec-29-dec-37) |
| `qty` | `decimal` | |
| `limit_price` | `decimal?` | Rule 2 |
| `purpose` | `open` \| `increase` \| `discretionary_exit` \| `risk_exit` | The proposer's label: the order builder's, a goal completion's or a removed instrument's (`exit_origin`), or the risk engine's `trim_to_target`. The gate assigns the purpose it enforces ([trading spec §9.1](trading-domain.md#91-evaluation-order-and-reason-codes)) |
| `exit_origin` | `signal` \| `goal_completion` \| `removed_instrument`, or `null` | Which of the three origins [mandate spec §6.1](mandate.md#61-purposes)'s purpose table gives a `discretionary_exit`: the order builder's signal exit (§8.3 step 2), a goal's completion (§3.1), or a removed instrument (§2.3, including an expired or invalidated thesis, §8.6). `null` for every other purpose: rule 8 |
| `exit_conviction`, `buy_conviction`, `combined_score` | `decimal?` | c, b, and s of §8.3 step 1, for a decision a §8.3 evaluation produced (an opening, an increase, or a signal exit); `null` for every other: rule 8 |
| `outputs_used` | `[ulid]` | The `ModelOutputRecorded` event IDs of the fresh outputs combined; empty for a decision no §8.3 evaluation produced: rules 8 and 9 |
| `model_weights` | `[{key: text, value: decimal}]` | The weight of **every** configured model, fresh or not (§4.1); empty for a decision no §8.3 evaluation produced: rules 8 and 9 |
| `clips_applied` | `[max_order_usd` \| `position_cap` \| `gross_exposure_cap` \| `target_qty` \| `max_spend_usd` \| `max_avg_price]` | Each §8.3 bound that reduced the proposal (the position cap is cap − MV − working); empty for a decision no §8.3 evaluation produced: rules 8 and 9 |
| `dry_run` | `allow` \| `deny` \| `defer` | The gate dry run: rules 4 to 6 |
| `reason_code` | `id?` | The gate's reason code (trading spec §9.1): rule 4 |
| `autonomy` | `auto` \| `ask` \| `deny`, or `null` | The §6.2 classification, reached only after an `allow`: rules 5 and 7 |
| `ask_suppressed` | `budget` \| `skipped_today` \| `recent_timeout`, or `null` | Why an `ask` was classified but not asked, with the reasons and precedence of [mandate spec §6.4](mandate.md#64-approvals) ("Asking is bounded"; [DEC-156](../project/04-decision-log.md#decisions) item 5). `null` for every other decision, including an `ask` that was asked. Non-null only when `autonomy` is `ask` and no approval was requested: rule 7 |
| `decided_by` | `text?` | What decided `autonomy` (mandate spec §6.2), one label of: `builtin_risk_reducing` (step 3), `rule:<id>` or `default` (step 4), `delegation:<id>` (step 4a), `admission_ceiling` (step 5), `client_ceiling` (step 5a), `review_ceiling` (step 5b), with `<id>` an `id`; the same labels as the approval content's `decided_by` ([mandate spec §6.4](mandate.md#64-approvals)). `null` exactly when `autonomy` is: rule 5 |
| `delegation_id` | `id?` | The delegation that lifted an `ask` to `auto` (mandate spec §6.5, [DEC-181](../project/04-decision-log.md#decisions)); delegation usage is counted from these events. Non-null exactly when `decided_by` is `delegation:` and this id: rule 7 |
| `requested_by` | `agent` \| `owner` \| `client` | Who asked for the order (mandate spec §6.2 step 5a, [DEC-185](../project/04-decision-log.md#decisions)): the order builder, the owner through the web app or CLI, or an owner-connected client, set from the authenticated channel and never from the request's content |
| `client_id` | `id?` | The connected client that asked, by the id it was connected under ([DEC-141](../project/04-decision-log.md#decisions)); non-null exactly when `requested_by` is `client`: rule 7 |

**`IntentProposed`**: exactly the `IntentReceived` vector's intent fields less `intent_id`, which is
this event's `event_id` (§2), and `agent_id`, which is the stream's. The executor's `IntentReceived`
copies the members and adds those two. Its `causation_id` is one of three events (rule 10): the
`DecisionMade` whose action members it repeats exactly; for an owner's exit of one instrument, the
`OwnerExitRequested`; or after an approval's grant, the `ApprovalRevalidated` with result `act` that
precedes it in the same batch (§9, [mandate spec §6.4](mandate.md#64-approvals)). Re-validation
never re-prices, re-sizes, or changes the order, so an approved intent repeats exactly the action
members bound in its approval's `ApprovalRequested` content object.

| Member | Type | Meaning |
|---|---|---|
| `instrument_id` | `text` | |
| `side`, `type`, `tif`, `qty`, `limit_price` | As `DecisionMade` | Rules 1 to 3 |
| `purpose` | `open` \| `increase` \| `discretionary_exit` \| `risk_exit` \| `owner_exit` | The proposer's label, as for `DecisionMade`; `protective` and the kill switch's flatten are never proposed intents |

**`AgentModeChanged`** (mandate spec §5.9)

| Member | Type | Meaning |
|---|---|---|
| `from`, `to` | `normal` \| `exits_only` \| `paused` \| `stopped` | The effective mode before and after; `to` obeys rule 11 |
| `reason` | `restriction_changed` \| `awaiting_reconciliation` \| `owner_pause` \| `owner_resume` \| `owner_stop` \| `kill_switch` | |
| `lifecycle` | `normal` \| `paused` \| `stopped` | The deployment's own state after the change, set only by the owner's pause, resume, or stop. Restrictions lift independently (§5.9), so an owner's pause taken while a stricter restriction held must survive that restriction lifting on replay; this event is the only place the agent stream can record it (§1 principle 4) |

**`KillSwitchActivated`** on the agent stream (trading spec §5.5). The account stream's is not closed
here.

| Member | Type | Meaning |
|---|---|---|
| `scope` | `agent` \| `connection` \| `workspace` | |
| `subject` | `id` | Which agent, connection, or workspace: rule 15. "Touches only its scope" (`AGENTS.md` rule 13) is checkable only if the event names it |
| `initiator` | `owner` \| `risk_limit` \| `platform_operator` | |
| `mode_event` | `ulid?` | The `AgentModeChanged` this switch wrote, or `null` if it changed no mode. The executor's `AgentModeApplied` copy carries it as `causation_id` (§2), which is how this stream's fold knows the switch was applied, from its own events (§1 principle 4) |

**`OwnerExitRequested`** (trading spec §5.5, mandate spec §5.10, §6.1): the owner's close of a
position, or the owner's kill switch. Every one records who asked and what step-up they gave,
whether or not a bid was confirmed. A displayed bid is confirmed only where trading spec §5.5 needs
one (an equity sold outside the regular session); without confirmation, equity sells wait for the
regular session. `step_up_status` says which case of
[DEC-158](../project/04-decision-log.md#decisions) option (c) applied: a kill switch without valid
step-up still stops the agent and flattens as an automated flatten does, and only the owner-exit
privileges beyond that need `valid`. A correct writer can meet every rule here for every owner
exit, whatever its step-up: no member is required that only valid step-up or a confirmed bid would
supply, and a confirmed bid with `absent` or `stale` step-up is valid, recorded as given, and
unlocks nothing while the exit proceeds ([mandate spec §6.1](mandate.md#61-purposes), "Owner
controls and step-up": owner exit and kill switch). So no step-up state can hold a
reduction (`AGENTS.md` rule 13); the test vectors' `valid_drafts` hold these cases.

| Member | Type | Meaning |
|---|---|---|
| `scope` | `instrument` \| `agent` \| `connection` \| `workspace` | |
| `subject` | `id` | The instrument ID, or which agent, connection, or workspace: rule 15 |
| `confirmed` | `boolean` | Whether the owner confirmed a displayed bid: rule 12 |
| `bid`, `bid_size` | `decimal?` | The displayed bid and bid size the owner confirmed |
| `floor` | `decimal?` | The confirmed floor price, below which the exit price ladder never prices |
| `user` | `text` | The owner who gave the instruction (opaque), on every owner exit and owner kill switch |
| `step_up_status` | `valid` \| `absent` \| `stale` | The step-up at the moment the owner committed the command ([DEC-156](../project/04-decision-log.md#decisions) item 8): `valid` evidence, none (`absent`), or evidence older than its freshness window (`stale`): rule 12 |
| `step_up` | `{assertion_id: text, authenticated_at: timestamp, method: text}?` | The step-up evidence, exactly when `step_up_status` is `valid`: rule 12 |

**Consistency rules** (reason `schema` unless stated; the path is the member named):

1. `DecisionMade`, `IntentProposed`: `side` is `buy` exactly when `purpose` is `open` or `increase`
   (long-only, no short sales) — `payload.side`.
2. `limit_price` is non-null exactly when `type` is `limit` — `payload.limit_price`.
3. `type` `market` never has purpose `open` or `increase` (`AGENTS.md` rule 12, trading spec §5.1) —
   `payload.type`.
4. `DecisionMade`: `reason_code` is `null` exactly when `dry_run` is `allow` — `payload.reason_code`.
5. `DecisionMade`: `autonomy` is non-null exactly when `dry_run` is `allow`; no classification runs
   after a `deny` or `defer` (mandate spec §6.2) — `payload.autonomy`; `decided_by` is non-null
   exactly when `autonomy` is, and is then `builtin_risk_reducing` exactly when `purpose` is neither
   `open` nor `increase` — `payload.decided_by`; and a non-null `decided_by` is one of the member's
   labels — `non_canonical` at `payload.decided_by`.
6. `DecisionMade`: `defer` only for `discretionary_exit` — `payload.dry_run`.
7. `DecisionMade`: a non-null `autonomy` is `auto` unless `purpose` is `open` or `increase` (built-in
   AUTO, §6.2 step 3) — `payload.autonomy`; and `ask_suppressed` is non-null only when `autonomy` is
   `ask` — `payload.ask_suppressed`. So a suppression only ever skips an opening or an increase,
   never an exit. That no `ApprovalRequested` names a suppressed decision is checked when the
   approval events' schemas close here. Then: `delegation_id` is non-null exactly when `decided_by` is
   `delegation:` followed by it, and then `autonomy` is `auto` (mandate spec §6.2 step 4a, MI-26) —
   `payload.delegation_id`; `client_id` is non-null exactly when `requested_by` is `client` —
   `payload.client_id`; a decision `requested_by` `client` with purpose `open` or `increase` is
   never `auto`, whatever decided it (the client ceiling, mandate spec §6.2 step 5a, MI-30) —
   `payload.autonomy`; and `decided_by` is `client_ceiling` only when `requested_by` is `client` and
   `autonomy` is `ask` — `payload.decided_by`; and `decided_by` is `review_ceiling` only when
   `autonomy` is `ask` (the review ceiling, mandate spec §6.2 step 5b, MI-32) —
   `payload.decided_by`. So the record itself refuses a connected client's opening that ran
   unasked, and a decision that claims the review ceiling and ran unasked.
8. `DecisionMade`: `exit_origin` is non-null exactly when `purpose` is `discretionary_exit` —
   `payload.exit_origin`. A decision was produced by a §8.3 evaluation exactly when `purpose` is
   `open` or `increase` or `exit_origin` is `signal`; `exit_conviction`, `buy_conviction`, and
   `combined_score` are each non-null exactly then, and otherwise `outputs_used`, `model_weights`,
   and `clips_applied` are also empty — the first offending, in that order. Every exit's writer
   holds what this needs: a signal exit has its evaluation's numbers, and a risk exit, a goal
   completion, or a removed instrument needs none, so the rule never holds an exit (`AGENTS.md`
   rule 13).
9. `DecisionMade`: `outputs_used` strictly ascending, `model_weights` keys strictly ascending by bytes,
   and `clips_applied` strictly in the table's order — `non_canonical` at the list.
10. `IntentProposed`: `causation_id` is non-null — `causation_id`; and when the `DecisionMade` it
    names is in the same `append` batch, its action members (`instrument_id`, `side`, `type`,
    `tif`, `qty`, `limit_price`, `purpose`, decimals compared by value) equal that decision's — on
    the `IntentProposed`, the first member that differs, in that order. Like any invalid draft, it
    refuses the whole batch (§5.1).
11. `AgentModeChanged`: `to` is at least as strict as `lifecycle` (`normal` < `exits_only` < `paused` <
    `stopped`) — `payload.to`.
12. `OwnerExitRequested`: `confirmed` is `true` exactly when `bid`, `bid_size`, and `floor` are all
    non-null — the first member, in that order, that disagrees; and `step_up` is non-null exactly
    when `step_up_status` is `valid` — `payload.step_up`. `confirmed` governs only the bid members:
    `user` and the step-up are recorded on every owner exit.
13. `ModelOutputRecorded`: `thesis_id` and `lineage_id` are `null` together — the one that is `null`.

**Subject rules** (reason `stream_mismatch`):

14. `StreamOpened`: `stream_id` equals `agent:{workspace_id}:{agent_id}` — `stream_id`.
15. `KillSwitchActivated`, `OwnerExitRequested`: scope `agent` names the stream's agent and scope
    `workspace` its workspace — `payload.subject`.

**Copy rule** (reason `schema`):

16. The agent runtime's copies of an owner command (§2) have a non-null `causation_id` —
    `causation_id`. The copies are every `OwnerExitRequested`, a `KillSwitchActivated` whose
    `initiator` is `owner`, an `AgentModeChanged` whose `reason` is `owner_pause`, `owner_resume`,
    or `owner_stop`, and every `OwnerCommandRefused`. `causation_id` is the `event_id` of the control stream's
    `OwnerCommandIssued` the copy was made from. That ID is from another stream, which §2 allows and
    which neither `append` nor §11's per-range checks resolve, since both read only this stream. On
    one agent stream a command is copied at most once into each of these event types: an owner's
    kill switch is copied as its `OwnerExitRequested` and its `KillSwitchActivated`, both naming the
    one command, and every other command has one copy: for a resume or a Stop, its
    `AgentModeChanged` or its `OwnerCommandRefused`, never both ([mandate spec §6.1](mandate.md#61-purposes)).
    The copy always has its command in hand, so the rule
    never holds a pause, a Stop, an owner exit, or a kill switch (`AGENTS.md` rule 13). A mode
    change that originates on the account stream, and the `AgentModeChanged` a kill switch writes,
    are not owner copies and are not covered here.

Two facts span events. `IntentProposed` repeats its `DecisionMade`'s action, which `append`
checks when both are in one batch (rule 10), and `mode_event` names this stream's
`AgentModeChanged` with reason `kill_switch`. Across batches `append` cannot see the other event,
so §11's per-range checks `intent_action_mismatch` and `mode_event_mismatch` verify both on the
stored chain, and `mandate journal verify` reports them.

### 9.2 Control-stream payload schemas ([DEC-261](../project/04-decision-log.md#decisions))

This subsection closes the payload schemas of the control-stream and account-stream records among
those `ValidationContext::from_journal` reads ([DEC-169](../project/04-decision-log.md#decisions)):
the control stream's `StreamOpened`,
`ConnectionEstablished`, `ConnectionRevoked`, `DisclosureAccepted`, `ConfigSnapshotRegistered`,
`MandateVersionCreated`, `MandateConfirmed`, `AgentDeployed`, and `AgentStopped`, and the account
stream's `AccountSnapshotRecorded`. It also closes `OwnerCommandRefused` on both streams that write
it ([DEC-291](../project/04-decision-log.md#decisions)). The fold also reads `UniverseChanged` and
`MandateVersionApplied` (account stream), which close with their own stories, and
`PlatformOperatorAction`, which stays open (below). For these events it replaces the "Key payload
fields" column of §9, and the required `config_refs` stay as §9 lists them. §9.1's types, its
absent-member rule, and its report order apply unchanged, with the rules below numbered on from
§9.1's. Each schema is `schema_version` 1.

Every member comes from a writer that exists or from a clause that names it, and DEC-261 traces
each one. No member is inferred where neither exists. The test vectors' `control_stream` section
holds a hash-chained control stream from `StreamOpened`, with its artifacts; base drafts on the
account and agent streams; the `JournaledFact` each record maps to; an invalid draft for every rule;
and valid drafts for the cases a rule might be misread to refuse.

**No payload carries a credential or personal data** (`AGENTS.md` rules 6 and 7, §6.4). A connection
is named by its opaque ID and scopes, never a key, token, or account number, and a user by an opaque
ID. Every schema is closed, so a draft with any other member is refused as `schema` at that member.

**Not closed here.** `PlatformOperatorAction` stays open (**Proposed**, DEC-261 item 9). Its §9 row
lists five actions. Clauses name the members of only two of them: `model_withdrawn` names the model
and a reason (mandate spec §8.1), and `research_thesis_halt` an instrument and, optionally, the
research agent's pinned content hash ([DEC-100](../project/04-decision-log.md#decisions)). No clause
and no writer names the subject of an operator stop, the scope of the global kill switch, what an
acceptable-use action records, or what the row's "approval" is. So the schema closes when the
operator service is specified, not with a guess. Until then `append` refuses it as `unknown_schema`,
as it does today. This holds no risk reduction: an operator's stop or kill switch is applied by the
stream owners' own `KillSwitchActivated` and mode records (§2), not by this audit record. The other
control-stream events close with their own stories.

**Types**, beyond §9.1's:

| Type | Values | Refused as |
|---|---|---|
| `pointer` | A JSON Pointer (RFC 6901) into the mandate document: one or more `/`-prefixed reference tokens, with `~` only in `~0` or `~1`. It is never the empty pointer: a record names each path it means | Not a string: `schema`; otherwise `non_canonical` |
| `date` | `YYYY-MM-DD` (§4.7), a calendar date in years 1970 to 9999 | As `pointer` |
| `risk_clock` | A §4.7 timestamp on a whole second: the risk clock (§2, mandate spec §5.2). Never integer seconds | As `pointer` |

**`StreamOpened`** on the control stream.

| Member | Type | Meaning |
|---|---|---|
| `stream_type` | `control` | |
| `workspace_id` | `id` | The subject: rule 25 |

**`ConnectionEstablished`**: the owner connected a broker account. FR-2.2 rejects a connection whose
permission check fails, so the record exists only once the check has passed and needs no result
member.

| Member | Type | Meaning |
|---|---|---|
| `connection_id` | `id` | The connection, by the opaque ID mandates name it by ([mandate spec §3](mandate.md#3-structure)) |
| `broker` | `text` | The broker or venue |
| `environment` | `paper` \| `live` | The account the connection is, which V-001 matches against a mandate's `environment` |
| `scopes` | `[text]` | The scopes granted: rule 19 |

**`ConnectionRevoked`**

| Member | Type | Meaning |
|---|---|---|
| `connection_id` | `id` | |

**`DisclosureAccepted`** ([mandate spec §10](mandate.md#10-records-dec-51-dec-97), V-005)

| Member | Type | Meaning |
|---|---|---|
| `document` | `id` | Which disclosure, for example `leveraged_etp` |
| `version` | `ref` | The disclosure version's hash, the value a mandate's `leveraged_etp_disclosure_version` names. The document is stored, so the hash proves which text was accepted |
| `user` | `text` | The owner who accepted (opaque) |
| `step_up` | `{assertion_id: text, authenticated_at: timestamp, method: text}` | Never `null`: V-005 counts only an acceptance with step-up |

**`ConfigSnapshotRegistered`**: a configuration snapshot registered under its content hash. A
`config_refs` value names one of these.

| Member | Type | Meaning |
|---|---|---|
| `kind` | `fee_config` \| `trading_calendar` \| `settlement_calendar` \| `instrument_snapshot` \| `rule_set` \| `mandate_version` \| `model_version` | §9's `config_refs` kinds. A signal model is `model_version` |
| `content_hash` | `ref` | The snapshot. For a model, its content hash (mandate spec §8.1) |
| `model_id`, `model_version` | `text?` | A model's id and version, registered together with its hash (V-007): rule 21 |
| `params` | `[text]` | A model's declared parameters (V-007), empty for any other kind: rules 20 and 21 |
| `admits_instruments` | `boolean?` | Whether the model may admit instruments (mandate spec §8.4): rule 21 |

**`MandateVersionCreated`** and **`MandateConfirmed`** carry, as members, the parts `JournaledFact`
reads: the version, the provenance per path, and the confirmed paths. The rest of each
[mandate spec §10](mandate.md#10-records-dec-51-dec-97) row is a record stored as an artifact and
named by its hash (`record_ref`). Mandate spec §10 stays the one source of what that record holds.
The document is stored too, under its own version (`mandate_version`, mandate spec §9.1). Both are
`ref`s, so each must be stored and must re-hash (§11 check 6). Replay can therefore prove which
document was confirmed and which record was shown. A bare ID or a path is refused as
`non_canonical`.

| Member | Type | Meaning |
|---|---|---|
| `mandate_version` | `ref` | `MandateVersionCreated`, `MandateConfirmed`: the version, the hash of the stored canonical mandate document |
| `provenance` | `[{path: pointer, source: user_stated \| user_entered \| template_structure \| platform_proposed \| platform_default}]` | `MandateVersionCreated`: each path's source (mandate spec §2.1): rule 17. Quoted spans and what was proposed are in the record |
| `confirmed_paths` | `[pointer]` | `MandateConfirmed`: the paths the owner confirmed; a path confirms itself and every path under it: rule 18 |
| `record_ref` | `ref` | Both: the rest of the event's mandate spec §10 row, as a stored record |

**`AgentDeployed`** ([mandate spec §10](mandate.md#10-records-dec-51-dec-97)). The connection,
environment, allocation, and pinned instruments the deployment runs under are the document's. They
are read from the stored document that `mandate_version` names, never restated here.

| Member | Type | Meaning |
|---|---|---|
| `agent_id` | `id` | |
| `mandate_version` | `ref` | The version deployed: rule 22 |
| `record_ref` | `ref` | The rest of the row: the go-live screen and UI build, the backtest and paper-run IDs and the legend and disclosure versions shown, the approving users, and the step-up evidence |

**`AgentStopped`** ([mandate spec §5.7, §5.10](mandate.md#57-lifetime-loss-floor-dec-44-dec-55))

| Member | Type | Meaning |
|---|---|---|
| `agent_id` | `id` | |
| `connection_id` | `id` | The connection whose loss carry the loss joins |
| `reason` | `goal_complete` \| `profit_stop_reached` \| `end_date` \| `owner_stop` | §9's reasons |
| `retired_on` | `date` | The America/New_York date the agent retired. §5.7's 90-day carry counts from it |
| `loss_added` | `decimal` | The net dollar loss max(0, N − E) at retirement added to the carry: rule 23 |

**`AccountSnapshotRecorded`** on the account stream: the broker's account as reconciliation reads it
([trading spec §11](trading-domain.md#11-reconciliation)). The members are the executor's. The last three record the
cash comparison, which only a reconciliation that had a base to compare against makes, and are
otherwise `null`.

| Member | Type | Meaning |
|---|---|---|
| `status`, `crypto_status` | `text` | As the broker reports them |
| `trading_blocked`, `account_blocked`, `trade_suspended_by_user` | `boolean` | |
| `multiplier` | `integer` | |
| `equity`, `cash`, `buying_power`, `non_marginable_buying_power`, `accrued_fees` | `decimal` | |
| `model_cash` | `decimal?` | The cash the executor's model expected |
| `cash_band` | `decimal?` | The tolerance |
| `cash_in_band` | `boolean?` | Whether the broker's cash fell inside it: rule 24 |
| `risk_clock` | `risk_clock` | The latest tick the executor had seen. The executor writes it on every account-stream event and folds each at it, so a snapshot without one could not be replayed ([DEC-302](../project/04-decision-log.md#decisions)). `append`'s check that it never decreases (§2) applies |

**`OwnerCommandRefused`** on the agent and account streams ([mandate spec §6.1](mandate.md#61-purposes),
DEC-291). The owner input it refused is its `causation_id`: rule 27.

| Member | Type | Meaning |
|---|---|---|
| `command` | `resume` \| `stop` \| `acknowledge` | Rule 26 |
| `reason` | `step_up_missing` \| `step_up_stale` \| `step_up_reused` \| `step_up_method` \| `not_independent` | `not_independent` only on the account stream: rule 28 |
| `effective_at` | `timestamp` | The effective time the command was judged at. A §4.7 timestamp, never risk-clock seconds (§9.1's types). This supersedes DEC-291 item 1's whole second for this member (DEC-261 item 7) |

**Consistency rules** (reason `schema` unless stated; the path is the member named):

17. `MandateVersionCreated`: `provenance` strictly ascending by `path` bytes, so no path is listed twice
    (`non_canonical` at `payload.provenance`).
18. `MandateConfirmed`: `confirmed_paths` strictly ascending by bytes (`non_canonical` at
    `payload.confirmed_paths`).
19. `ConnectionEstablished`: `scopes` strictly ascending by bytes (`non_canonical` at `payload.scopes`).
20. `ConfigSnapshotRegistered`: `params` strictly ascending by bytes (`non_canonical` at
    `payload.params`).
21. `ConfigSnapshotRegistered`: `model_id`, `model_version`, and `admits_instruments` are non-null
    exactly when `kind` is `model_version`, and `params` is empty when it is not. Reported at the first
    offending member, in that order.
22. `AgentDeployed`: `mandate_version` equals `config_refs.mandate_version` (`payload.mandate_version`).
    A missing ref is already `missing_config_ref`.
23. `AgentStopped`: `loss_added` ≥ 0 (`payload.loss_added`). The loss carried is never negative
    (mandate spec §5.7).
24. `AccountSnapshotRecorded`: `cash_band` and `cash_in_band` are `null` exactly when `model_cash` is.
    Reported at the first that disagrees. When all three are present, `cash_band` ≥ 0
    (`payload.cash_band`), and `cash_in_band` is `true` exactly when |`cash` − `model_cash`| ≤
    `cash_band` (`payload.cash_in_band`). A snapshot's flag can never contradict its own numbers.

**Subject rules** (reason `stream_mismatch`):

25. Control-stream `StreamOpened`: `stream_id` equals `ctl:{workspace_id}` (`stream_id`).
26. `OwnerCommandRefused`: `command` is `acknowledge` on the account stream, and `resume` or `stop` on
    the agent stream (`payload.command`). The executor refuses acknowledgments, and the runtime
    refuses resumes and Stops (§2).
28. `OwnerCommandRefused` with reason `not_independent` is on the account stream (`payload.reason`):
    only an acknowledgment is judged for independence, and only under `independent_approval_required`
    ([mandate spec §5.8, §6.7](mandate.md#67-tripwires-dec-187-dec-350-dec-351)). A resume or Stop is
    refused only for its step-up.

**Copy rule** (reason `schema`):

27. `OwnerCommandRefused`, on either stream, has a non-null `causation_id` (`causation_id`). On the
    agent stream that is the `OwnerCommandIssued` (rule 16 already requires it there), and on the
    account stream the control stream's `OwnerAcknowledged` (§2). The refusal is written from the
    input it refuses, so the rule never holds one.

**The mapping to `JournaledFact`** ([DEC-169](../project/04-decision-log.md#decisions)). Each record
maps to one fact, and the test vectors list them. `AccountSnapshotRecorded` does not name its
connection. Which connection an account stream belongs to is journaled nowhere yet (**Proposed**,
DEC-261 item 10), so the mapping takes it as an argument from its owner. `ValidationContext` takes
what no event carries the same way.

| Record | Fact | From |
|---|---|---|
| `ConnectionEstablished`, `ConnectionRevoked` | `ConnectionEstablished`, `ConnectionRevoked` | `connection_id`, and for the first `environment` |
| `DisclosureAccepted` | `DisclosureAccepted` | `version` |
| `ConfigSnapshotRegistered` of kind `model_version` | `ModelRegistered` | `model_id`, `model_version`, `content_hash`, `params`, `admits_instruments`; other kinds map to none |
| `MandateVersionCreated` | `MandateVersionCreated` | `mandate_version`, `provenance` |
| `MandateConfirmed` | `MandateConfirmed` | `mandate_version`, `confirmed_paths` |
| `AgentDeployed` | `AgentVersionActive` | `agent_id`; `connection_id`, `environment`, `capital.allocation_usd`, and `universe.pinned_instruments[].asset_id` of the stored document `mandate_version` names. A document that is not stored maps to none, and the mapping refuses it |
| `AgentStopped` | `AgentStopped` | `agent_id`, `connection_id`, `retired_on`, `loss_added` |
| `AccountSnapshotRecorded` | `AccountSnapshot` | `equity`, with the account stream's connection given |

## 10. Anchoring

- **Frequency:** every 5 minutes (configurable) and at each end of day.
- **Leaves:** `[{stream_id, seq, hash}]` for every stream's head, sorted by `stream_id` bytes. The
  control stream's leaf is its head **before** the `AnchorComputed` event that records the anchor.
- **Tree (RFC 6962 style):** leaf = SHA-256(0x00 ‖ canonical(leaf)); node = SHA-256(0x01 ‖ left ‖
  right); for n > 1 leaves, split at the largest power of two below n; a single leaf is its own root.
- **Timestamp:** the imprint sent to an RFC 3161 timestamping authority is SHA-256 of the 32 raw
  root bytes. The token, certificate chain, and revocation data are stored as artifacts and
  re-stamped before expiry. In hybrid mode the root is also sent to the global control plane.
- `AnchorComputed` records all leaves and the root. Timestamping outages are queued and retried,
  never block trading, and are journaled as gaps.

## 11. Verification

Input: a stream range with a **trusted start** (`from_seq`, `trusted_prev_hash`) taken from a
manifest or anchor (seq 1 with 64 zeros for a full stream). Events are walked in `seq` order (file
line order for exports), reading `seq` from the body.

**Per-event checks, in order** (the first failure is reported):

1. `non_canonical` — the body parses (duplicate keys rejected) and re-canonicalizes to the same bytes.
2. `column_mismatch` — stored columns (`stream_id`, `seq`, `event_id`, `event_type`,
   `schema_version`, `environment`, `recorded_at`, `prev_hash`) equal the body.
3. `seq_gap` — `seq` equals the previous `seq` + 1.
4. `rehash_mismatch` — SHA-256(body) equals the stored `hash`.
5. `prev_hash_mismatch` — `prev_hash` equals the previous event's `hash` (or the trusted start).
6. `artifact_missing`, `artifact_mismatch` — every referenced artifact exists and re-hashes.

**Per-range checks:** `anchor_head_mismatch` (the event at each anchored `seq` exists with the
anchored hash), `anchor_root_mismatch`, `tsa_token_invalid`, `segment_manifest_mismatch`,
`segment_gap`, and on an agent stream ([§9.1](#91-agent-stream-payload-schemas-dec-177)):

- `intent_action_mismatch` — an `IntentProposed` whose `causation_id` names a `DecisionMade` has
  that decision's action members (rule 10), and one whose `causation_id` names an
  `ApprovalRevalidated` has the action members bound in that approval's `ApprovalRequested`,
  reported at the `IntentProposed`. The second clause is checked, with vectors, once §9.1 closes
  the approval events' schemas; until then no range fails on it;
- `mode_event_mismatch` — a `KillSwitchActivated` whose `mode_event` is non-null names an earlier
  `AgentModeChanged` on this stream with reason `kill_switch`, reported at the
  `KillSwitchActivated`.

A reference to an event before the range's trusted start is not checked by that range; the weekly
full-chain run checks every one, and there a `mode_event` that names no earlier event fails. The
test vectors' `agent_stream.range_verification` holds a case for each.

**Schedule:** the tail of every stream at startup and before each segment export; the full chain
weekly; results journaled as `VerificationRun`.

**On failure:** SEV-1. Account- or agent-stream failures pause the affected agents (the kill switch
still works); control-stream failures freeze mandate and deployment changes. Affected segments are
put on legal hold. **Nothing is repaired in place:** a new writer epoch continues from an
`IntegrityIncidentRecorded` event that references the last good hash and anchor. The customer (for
advisers, their chief compliance officer) is notified within the configured deadline, and the
post-mortem is retained.

## 12. Export

- **Canonical export:** segment files, manifests, the relevant anchors with inclusion proofs,
  timestamp tokens with certificate chains, and the verifier's digest.
- **Examination bundle** (scoped by account, agent, and period): every related stream (account,
  agent, control, scheduler), referenced configuration objects and artifacts, schemas and upcasters,
  the verifier release and format specification, an index, and, for authorized requests, resolved
  identities. A deterministic human-readable report is generated with its own hash. Bundles are
  produced within the configured deadline; verifier releases and format documents are retained for
  the retention period.
- Human-readable views (for example, the causal trace from a fill to its observations) are derived
  from the canonical export.

## 13. Open questions

1. Timestamping authority choice and fallback.
2. Whether hybrid customers may opt out of sending anchor roots to the global control plane.
3. Personal-data scanning approach for free-text artifacts.
4. Export deadline and customer-notification deadline values.
