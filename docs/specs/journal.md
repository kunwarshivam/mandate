# Journal Spec (v1)

| | |
|---|---|
| **Status** | v0.5 (v0.2 founder sign-off 2026-09-25, [DEC-71](../project/04-decision-log.md#decisions); v0.3 amendment [DEC-81](../project/04-decision-log.md#decisions); v0.4 adds the research-agent events of [DEC-97](../project/04-decision-log.md#decisions) and [DEC-111](../project/04-decision-log.md#decisions); v0.5 closes the agent stream's payload schemas, [DEC-177](../project/04-decision-log.md#decisions)); changes need a decision-log entry (safety-critical) |
| **Implements** | PRD 6.7 (FR-7.1 to FR-7.7), FR-5.6, FR-5.7; backlog E5; milestone M4 |
| **Depends on** | [Trading domain spec §12–§13](trading-domain.md#12-journal-events) |
| **Test vectors** | [reference-cases/journal.yaml](reference-cases/journal.yaml) (version 3, with the generated `agent_stream` section of §9.1; [reference/journal/generate.py](../../reference/journal/generate.py)) |

The journal is the append-only, hash-chained record of everything the platform does: the source
of truth for agent and account state (event-sourced), the audit trail, and the input to replay.

## Change history

- **v0.5 ([DEC-177](../project/04-decision-log.md#decisions)):** §9.1 closes the payload schemas of
  the agent stream's `StreamOpened`, `ObservationRecorded`, `ModelOutputRecorded`, `DecisionMade`,
  `IntentProposed`, `AgentModeChanged`, `KillSwitchActivated`, and `OwnerExitRequested`, and rules on
  every contradiction [DEC-174](../project/04-decision-log.md#decisions) item 3 found in them: `null`,
  never empty strings; timestamps, never risk-clock seconds; artifact references, never inline data;
  `IntentProposed` as the `IntentReceived` vector's intent fields. The test vectors gain a generated
  `agent_stream` section (a hash-chained stream from `StreamOpened` with its artifacts, and an invalid
  draft for every rule) and stay version 3 until the harness reads it. The approval events close with
  the escalation spec change (M7).
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
| Workspace control | `ctl:{workspace_id}` | Workspace control services | Mandates, deployments, connections, disclosures, policy and configuration registration, owner acknowledgments and alerts, surveillance reports, anchors, verification, records lifecycle, access and export |
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
  account stream as `OwnerAcknowledged`, with `causation_id` pointing to the original. A mode change that **originates** on the account stream (account restrictions,
  mandate risk limits) is journaled there first as `AgentModeApplied`, and the agent runtime
  copies it into the agent stream as `AgentModeChanged`; the `causation_id` always points to the
  originating event. A user's kill switch is a **command**
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
| `MandateVersionApplied`, `RiskDayStarted`, `RiskLimitTriggered`, `RiskLimitLifted`, `HighWaterMarkReset`, `PositionReleased`, `InstrumentRestrictionChanged`, `GoalCompleted` | man | agent risk state ([mandate spec §5.10](mandate.md#510-journal-events)): version result, classification, and allocation change; day-start equity; limit, action, E, H, drawdown, E₀, capital base C, inherited loss L, net contributed N; reset evidence; released positions; stale-mark and removed-instrument changes with the reason; goal completion |
| `UniverseChanged` | man | The working universe changed ([mandate spec §2.3, §8.5](mandate.md#23-the-working-universe-at-runtime-dec-97)); a risk input, so it carries `risk_clock`: agent, instrument, change (`admitted`, `removed`), reason (`thesis_admitted`, `thesis_expired`, `thesis_invalidated`, `lineage_retired`, `eligibility_lost`, `operator_halt`, `version_applied`), thesis and lineage ids, working-universe size after |

**Agent stream** (owner: agent runtime)

| Event type | Required refs | Key payload fields |
|---|---|---|
| `StreamOpened` | — | stream type, subject, environment |
| `ObservationRecorded` | — | source, instrument, data (artifact) |
| `ModelInvocationRecorded` | mod | purpose (compiler, fast model, research), provider, model and version, parameters, seed, prompt and retrieved context (artifact), response (artifact), provider request ID |
| `ModelOutputRecorded` | man | signal model id, version, content hash, instrument, as_of, expires_at, direction, conviction, confidence, horizon, thesis (artifact) and, for the research agent, its thesis and lineage ids; `ignored` reason if not used |
| `ThesisProposed`, `ThesisRevised` | man, mod | The research agent's output and its admission decision ([mandate spec §8.4, §8.5](mandate.md#84-the-research-agent-dec-97-adr-0002)): research agent id, version, and content hash; thesis id, lineage id, revision, and for `ThesisRevised` the `predecessor_thesis_id` and what the revision changed; instrument, asset class, direction, horizon, evidence and cited sources, corroboration kind, invalidation, conviction, confidence; the source-allowlist version; prompt and response (artifacts); `admitted` and the refusal reason from the ordered §8.5 checks |
| `DecisionMade` | man | proposed action, combined conviction and combined score, outputs used, model weights, clips applied, gate dry-run result, autonomy classification |
| `IntentProposed` | man | intent fields (its `event_id` is the intent ID) |
| `ApprovalRequested`, `ApprovalDelivered`, `ApprovalResponded`, `ApprovalTimedOut`, `ApprovalCanceled` | man | content shown (artifact), bound quantity, limit price, and mandate version, cancel reason, channel and message ID, delivery status, responder (opaque) and role, step-up evidence (assertion ID, authentication time, method), separation-of-duties result |
| `AgentModeChanged`, `KillSwitchActivated` | — | from, to, reason; scope and initiator |
| `OwnerExitRequested` | man | instrument or scope, bid shown and confirmed, user (opaque), step-up evidence |

**Workspace control stream** (owner: workspace services)

| Event type | Required refs | Key payload fields |
|---|---|---|
| `MandateVersionCreated`, `MandateConfirmed` | — | per [mandate spec §10](mandate.md#10-records-dec-51-dec-97): source text (artifact), compiled fields, provenance per path with quoted spans, template, policy-set hashes, validation results and warnings, classification, diff; version hash, confirmed paths, rendered confirmation (artifact) and UI build, warnings acknowledged, step-up evidence, confirming user (opaque) |
| `AgentDeployed`, `DeploymentRejected`, `AgentStopped` | man | agent, mandate version, reason (`goal_complete`, `profit_stop_reached`, `end_date`, owner stop), net dollar loss added to the connection's loss carry; for `AgentDeployed`: the rendered go-live screen (artifact), backtest and paper-run IDs shown, performance legend and disclosure versions shown, approving users, step-up evidence ([mandate spec §10](mandate.md#10-records-dec-51-dec-97)) |
| `PolicyChanged`, `WorkspaceProfileAssigned` | — | level, diff, author (opaque), step-up evidence, affected agents; profile, basis, assigning user |
| `ConnectionEstablished`, `ConnectionRevoked` | — | broker, scopes granted, permission-check result |
| `DisclosureAccepted` | — | document and version hash, user (opaque), step-up evidence |
| `OwnerAlertSent`, `OwnerAcknowledged` | — | subject event, channel, delivery status; user (opaque), authentication method |
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
drafts for the cases a rule might be misread to refuse. The other
agent-stream events are not closed yet: the approval events close with the escalation spec change
(M7), `ModelInvocationRecorded`, `ThesisProposed`, and `ThesisRevised` with their own stories.

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
then the numbered consistency rules (only on a payload whose members are all well typed), then
`artifact_refs` and `pii_refs` (§3), then the subject rules 14 and 15 (`stream_mismatch`). Paths are
dotted from the envelope (`payload.step_up.authenticated_at`).

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
| `purpose` | `open` \| `increase` \| `discretionary_exit` \| `risk_exit` | The proposer's label: the order builder's, or the risk engine's `trim_to_target`. The gate assigns the purpose it enforces ([trading spec §9.1](trading-domain.md#91-evaluation-order-and-reason-codes)) |
| `exit_conviction`, `buy_conviction`, `combined_score` | `decimal?` | c, b, and s of §8.3 step 1: rule 8 |
| `outputs_used` | `[ulid]` | The `ModelOutputRecorded` event IDs of the fresh outputs combined: rule 9 |
| `model_weights` | `[{key: text, value: decimal}]` | The weight of **every** configured model, fresh or not (§4.1): rule 9 |
| `clips_applied` | `[max_order_usd` \| `position_cap` \| `gross_exposure_cap` \| `target_qty` \| `max_spend_usd` \| `max_avg_price]` | Each §8.3 bound that reduced the proposal (the position cap is cap − MV − working): rule 9 |
| `dry_run` | `allow` \| `deny` \| `defer` | The gate dry run: rules 4 to 6 |
| `reason_code` | `id?` | The gate's reason code (trading spec §9.1): rule 4 |
| `autonomy` | `auto` \| `ask` \| `deny`, or `null` | The §6.2 classification, reached only after an `allow`: rules 5 and 7 |

**`IntentProposed`**: exactly the `IntentReceived` vector's intent fields less `intent_id`, which is
this event's `event_id` (§2), and `agent_id`, which is the stream's. The executor's `IntentReceived`
copies the members and adds those two. Its `causation_id` is the `DecisionMade` whose action members
it repeats exactly, or for an owner's exit of one instrument the `OwnerExitRequested` (rule 10).

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
unlocks nothing (mandate spec §6.1) while the exit proceeds. So no step-up state can hold a
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
   after a `deny` or `defer` (mandate spec §6.2) — `payload.autonomy`.
6. `DecisionMade`: `defer` only for `discretionary_exit` — `payload.dry_run`.
7. `DecisionMade`: a non-null `autonomy` is `auto` unless `purpose` is `open` or `increase` (built-in
   AUTO, §6.2 step 3) — `payload.autonomy`.
8. `DecisionMade`: `exit_conviction`, `buy_conviction`, and `combined_score` are each `null` exactly
   when `purpose` is `risk_exit` — the first offending, in that order.
9. `DecisionMade`: `outputs_used` strictly ascending, `model_weights` keys strictly ascending by bytes,
   and `clips_applied` strictly in the table's order — `non_canonical` at the list.
10. `IntentProposed`: `causation_id` is non-null — `causation_id`.
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

Facts that span events are the writer's to keep and are checked by the vectors, not at `append`:
`IntentProposed` repeats its `DecisionMade`'s action, and `mode_event` names this stream's
`AgentModeChanged` with reason `kill_switch`.

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
`segment_gap`.

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
