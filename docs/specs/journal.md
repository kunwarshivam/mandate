# Journal Spec (v1)

| | |
|---|---|
| **Status** | Draft v0.1: requires founder approval before implementation (safety-critical) |
| **Implements** | PRD 6.7 (FR-7.1 to FR-7.7), FR-5.6, FR-5.7; backlog E5; milestone M4 |
| **Depends on** | [Trading domain spec §12–§13](trading-domain.md#12-journal-events) |
| **Test vectors** | [reference-cases/journal.yaml](reference-cases/journal.yaml) |

The journal is the append-only, hash-chained record of everything the platform does: the source
of truth for agent and account state (event-sourced), the audit trail, and the input to replay.
This spec defines streams, the event envelope, canonical serialization, hashing, write and
storage rules, replay, versioning, verification, and export.

## 1. Principles

1. **Append-only.** Events are never updated or deleted. Corrections are new events
   (`CompensatingEvent`) that reference what they correct.
2. **Write before acting.** An event describing an intent or decision is durably committed before
   any side effect it authorizes (for example, sending an order).
3. **One writer per stream.** Each stream has exactly one owning component; ordering within a
   stream is total and gapless.
4. **State is a pure fold.** Replaying a stream's events in `seq` order with the configuration they
   reference reproduces state exactly. Handlers never read the wall clock or live configuration.
5. **Tamper-evident.** Each event commits to its predecessor by hash; stream heads are anchored
   externally.
6. **No personal data in events.** Events carry references; personal data lives in a separate
   vault so crypto-shredding never alters the journal.

## 2. Streams

| Stream type | `stream_id` format | Owner (single writer) | Contents |
|---|---|---|---|
| Account | `acct:{workspace_id}:{account_ref}` | The account's serialized executor (trading spec §7.1) | Intents received, gate decisions, orders, fills, fees, cash, settlement, corporate actions, reconciliation, account state, restrictions, related-account coordination |
| Agent | `agent:{workspace_id}:{agent_id}` | The agent's runtime | Observations, advisor opinions, decisions, intents proposed, approval requests and outcomes, agent mode changes, deployment lifecycle |
| Workspace control | `ctl:{workspace_id}` | Workspace control services | Mandate versions and confirmations, deployments, policy changes, connections, owner acknowledgments, configuration and instrument snapshot registration, surveillance reports |
| Scheduler | `clock:{workspace_id}` | The workspace scheduler | `ClockAdvanced`, `TradingDayStarted` (source of time-driven events consumed by other streams) |

- There is **no global order** across streams. Relationships across streams use `causation_id`
  and `correlation_id`. Displays across streams sort by `recorded_at` for readability only.
- When an account stream consumes a time-driven event (for example `SettlementPosted`), the
  executor writes it into the account stream with `causation_id` pointing to the scheduler event.
- A stream begins with `StreamOpened` (seq 1).

## 3. Event envelope

| Field | Type | Rules |
|---|---|---|
| `event_id` | ULID string | Globally unique; idempotency key for appends |
| `stream_id` | string | Per §2 |
| `seq` | integer | Starts at 1; gapless; the **only** ordering key within a stream |
| `prev_hash` | 64 lowercase hex | Hash of the previous event in the stream; 64 zeros for seq 1 |
| `hash` | 64 lowercase hex | SHA-256 of the canonical envelope without `hash` (§4) |
| `event_type` | string | From the catalogue (§8) |
| `schema_version` | integer | Version of this event type's payload schema |
| `event_time` | timestamp | Time at the source (broker, exchange, or local); informational |
| `recorded_at` | timestamp | Local time when appended; informational |
| `clock_source` | string | `broker`, `exchange`, `local`, or `scheduler` |
| `causation_id` | ULID or null | The event that directly caused this one |
| `correlation_id` | ULID or null | Groups a flow (for example, the intent ID for everything about one order) |
| `actor` | object | `{kind: system \| agent \| user \| broker, id, version}`; users are referenced by opaque ID |
| `config_refs` | object | Content hashes (`sha256:...`) of configuration the handler needs: `fee_config`, `trading_calendar`, `settlement_calendar`, `instrument_snapshot`, `rule_set`, `mandate_version`, `model_version`, as applicable |
| `payload` | object | Event-type specific (§8) |
| `artifact_refs` | array | Content hashes of large artifacts in object storage (§6.3) |
| `pii_refs` | array | Opaque references into the personal-data vault (§6.4) |

**Timestamps** are RFC 3339 UTC with exactly 9 fractional digits and `Z`
(`2026-09-21T14:00:00.000000000Z`).

## 4. Canonical serialization and hashing

- **Canonical form** is JSON per RFC 8785 (JSON Canonicalization Scheme). Because the envelope
  forbids floating-point numbers and uses ASCII snake_case keys, this equals UTF-8 JSON with keys
  sorted by code point, no insignificant whitespace, and `null` for absent optional values.
- **Numbers:** only integers (`seq`, `schema_version`). **All decimals are strings** in normalized
  form: no exponent, no leading `+`, trailing fractional zeros removed, no trailing decimal point,
  and zero written as `"0"` (for example `150.00 → "150"`, `0.020140 → "0.02014"`).
- **Hash:** `hash = SHA-256(canonical_bytes(envelope with the "hash" field omitted))`, written as
  lowercase hex. The omitted envelope still contains `prev_hash`, which chains each event to its
  predecessor.
- **Test vectors** in [reference-cases/journal.yaml](reference-cases/journal.yaml) give exact
  canonical bytes and hashes; implementations must reproduce them byte for byte.

## 5. Writing

- **Single writer.** Only the stream owner appends. The append operation takes
  `expected_seq`; a mismatch fails (optimistic concurrency) and the writer must re-read.
- **Idempotency.** Appending an `event_id` that already exists returns the stored event if the
  content is identical, and fails if it differs.
- **Durability before side effects.** An append returns only after the event is committed
  durably. Components that act on an event (the executor sending an order, the notifier sending an
  approval request) act only after the append returns.
- **Group commit** is allowed for throughput, provided no event is acknowledged before its batch
  is durable.
- **Latency target:** p99 append (commit acknowledged) under 5 ms in the managed and hybrid
  reference deployments.
- **Clock:** `recorded_at` comes from the host clock, disciplined by NTP; the journal records the
  measured clock offset in a daily `ClockOffsetRecorded` event on the scheduler stream.

## 6. Storage

### 6.1 Hot store

Postgres table (one per workspace deployment):

| Column | Notes |
|---|---|
| `stream_id`, `seq` | Primary key |
| `event_id` | Unique |
| `event_type`, `schema_version`, `recorded_at` | Indexed for queries |
| `prev_hash`, `hash` | Stored as text |
| `envelope` | Canonical JSON bytes (the exact bytes that were hashed, plus `hash`) |

- The database role used by writers has **INSERT and SELECT only**; no role used by application
  code has UPDATE or DELETE on the table. A trigger rejects updates and deletes as defense in depth.
- The stored bytes are the canonical bytes; readers never re-serialize to verify.

### 6.2 Cold store and retention

- Daily, closed segments of each stream are exported as canonical JSON Lines plus a manifest
  (first and last `seq`, first `prev_hash`, last `hash`, SHA-256 of the file) to object storage
  with **object lock** (write-once) for the retention period in trading spec §13.
- The hot store may drop segments older than a configured age only after the cold copy and its
  manifest are verified.

### 6.3 Artifacts

Large items (LLM prompts and responses, data snapshots, raw broker payloads above a size
threshold, reports) are stored in object storage under their SHA-256 (`sha256:{hex}`), with the
same object lock. Events reference them in `artifact_refs`; verification recomputes each hash.

### 6.4 Personal data

Names, emails, phone numbers, and similar data live in a personal-data vault, encrypted with a
per-person key. Events carry opaque `pii_refs`. Deleting a person's key (crypto-shredding) makes
their personal data unreadable without changing any journal byte or hash.

### 6.5 Encryption

Storage is encrypted at rest with per-workspace keys (bring-your-own-key for businesses). Hashes
are computed over canonical plaintext, so verification requires read access; anchors reveal only
hashes.

## 7. Replay, snapshots, and versioning

- **Replay** folds a stream from seq 1 (or from a verified snapshot) in `seq` order. Configuration
  is resolved from an immutable, content-addressed configuration store using `config_refs`; a
  missing configuration object halts replay (unknown means stop).
- **Snapshots** are derived state at a given `seq`, stored with the snapshot's own hash and the
  `hash` of the last event folded. They are optimizations: a background job regularly replays from
  an earlier snapshot (or seq 1) and verifies it reproduces the same snapshot; a mismatch is an
  incident.
- **Event schema versions** increase when a payload changes. Old events are never rewritten;
  readers apply **upcasters** (pure functions from version N to N+1) at read time. Hashes always
  cover the stored bytes.
- **Late events** (for example, a fill that arrives after its order reached a terminal state) are
  appended at their arrival `seq`; replay never re-sorts by `event_time`.

## 8. Event catalogue

Payload schemas are defined per event type in code (with JSON Schema exported to
`schemas/events/`). The catalogue below fixes names, streams, and producers; it extends trading
spec §12.

| Event type | Stream | Producer | Key payload fields |
|---|---|---|---|
| `StreamOpened` | Any | Owner | stream type, workspace, subject |
| `ObservationRecorded` | Agent | Runtime | source, instrument, data reference (artifact) |
| `AdvisorOpinionRecorded` | Agent | Runtime | advisor, version, instrument, signal, conviction, horizon, thesis (artifact) |
| `DecisionMade` | Agent | Runtime | proposed action, confidence, advisor weights, autonomy classification |
| `IntentProposed` | Agent | Runtime | intent ID, instrument, side, type, quantity, limit, purpose, mandate version |
| `IntentReceived` | Account | Executor | intent ID, agent |
| `GateDecided` | Account | Risk gate | intent ID, verdict, reason code, checks evaluated, quotes and marks used |
| `ApprovalRequested` / `ApprovalResponded` / `ApprovalTimedOut` | Agent | Runtime | request ID, deadline, default, responder (opaque), channel, authentication method |
| `OrderSubmitted` / `OrderStateChanged` / `OrderAbandoned` | Account | Executor | client order ID, attempt, broker status, internal state |
| `FillApplied` / `LateFillApplied` | Account | Executor | fill ID, client order ID, gross quantity, price, fees, trade date |
| `FeesCharged` | Account | Executor | family, day, accrued, charged |
| `MarkUpdated` | Account | Executor | instrument, price, source, feed |
| `SettlementPosted` / `DividendPaid` / `CashInLieuPosted` | Account | Executor | date, instrument, amount |
| `CorporateActionPrepared` / `CorporateActionApplied` | Account | Executor | instrument, action, ratio or amount, ex-date |
| `ProtectionChanged` | Account | Executor | instrument, action (placed, canceled, re-placed), orders, unprotected interval start or end |
| `BrokerPositionObserved` / `ReconciliationRun` / `CompensatingEvent` | Account | Executor | observed values, differences, corrected event IDs |
| `AccountStateObserved` / `RejectObserved` / `AccountRestrictionChanged` | Account | Executor | status, flags, reject code, restriction |
| `ExternalActivityIngested` | Account | Executor | broker order or fill details, unattributed |
| `RelatedAccountsCoordination` | Account | Executor | canceled opening orders across the group |
| `ConductBreachDetected` | Account | Risk gate | control, agent, instrument, measured value |
| `AgentModeChanged` | Agent | Runtime | from, to, reason |
| `KillSwitchActivated` | Agent or Account | Runtime or executor | scope, initiator (opaque) |
| `MandateVersionCreated` / `MandateConfirmed` | Control | Workspace services | mandate version, fields, inferred fields, confirming user (opaque) |
| `AgentDeployed` / `DeploymentRejected` / `AgentStopped` | Control | Workspace services | agent, mandate version, reason |
| `OwnerAcknowledged` | Control | Workspace services | subject event ID, user (opaque), authentication method |
| `ConfigSnapshotRegistered` / `SnapshotUpdated` | Control | Workspace services | configuration kind, content hash |
| `SurveillanceReportGenerated` | Control | Workspace services | period, report artifact, breaches |
| `ClockAdvanced` / `TradingDayStarted` / `ClockOffsetRecorded` | Scheduler | Scheduler | time, trading day, measured offset |

## 9. Anchoring

- Every hour (configurable) and at each end of day, the platform computes a **Merkle root** over
  the current head `hash` of every stream in the workspace (leaves sorted by `stream_id`) and
  records it in an `AnchorComputed` event on the control stream.
- The root is sent to an **RFC 3161 timestamping authority**; the signed timestamp token is stored
  as an artifact. In hybrid mode, the root (a hash only) is also sent to the global control plane.

## 10. Verification

A verification tool (and API) checks, for any stream range, walking events in stored `seq`
order and applying these checks to each event **in this order** (the first failure is reported):

1. `seq` equals the previous event's `seq` + 1 (seq 1 first) — `seq_gap`.
2. The stored envelope re-hashes to its `hash` — `rehash_mismatch`.
3. `prev_hash` equals the previous event's `hash` (64 zeros for seq 1) — `prev_hash_mismatch`.
4. Every artifact re-hashes to its reference.
5. Anchored roots recompute from the stream heads at anchor time, and timestamp tokens validate.
6. Cold-store segments match their manifests and join the hot store without gaps.

Any failure is a SEV-1 incident (quality plan). Results are journaled as `VerificationRun`.

## 11. Export

- **Canonical export:** JSON Lines of stored envelopes for a stream and range, plus a manifest
  (range, first `prev_hash`, last `hash`, file SHA-256). An auditor can verify it with the
  published verification tool alone.
- **Views:** CSV and human-readable reports (for example, the causal trace from a fill back to its
  observations) are derived from the canonical export and are not themselves authoritative.

## 12. Open questions

1. Postgres partitioning strategy for large workspaces (per stream type, per month).
2. Timestamping authority choice and fallback when unavailable (queue and retry; anchoring never
   blocks trading).
3. Whether hybrid customers may opt out of sending anchor roots to the global control plane.
