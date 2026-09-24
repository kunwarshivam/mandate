# Journal Spec (v1)

| | |
|---|---|
| **Status** | Draft v0.2: requires founder approval before implementation (safety-critical) |
| **Implements** | PRD 6.7 (FR-7.1 to FR-7.7), FR-5.6, FR-5.7; backlog E5; milestone M4 |
| **Depends on** | [Trading domain spec §12–§13](trading-domain.md#12-journal-events) |
| **Test vectors** | [reference-cases/journal.yaml](reference-cases/journal.yaml) (version 2) |

The journal is the append-only, hash-chained record of everything the platform does: the source
of truth for agent and account state (event-sourced), the audit trail, and the input to replay.

## Change history

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
| Agent | `agent:{workspace_id}:{agent_id}` | The agent's runtime | Observations, model invocations, advisor opinions, decisions, intents proposed, approvals, agent mode changes |
| Workspace control | `ctl:{workspace_id}` | Workspace control services | Mandates, deployments, connections, disclosures, policy and configuration registration, owner acknowledgments and alerts, surveillance reports, anchors, verification, records lifecycle, access and export |
| Scheduler | `clock:{workspace_id}` | The workspace scheduler | `ClockAdvanced`, `TradingDayStarted`, clock measurements |

- **Identifier grammar:** every `{…}` segment matches `[A-Za-z0-9_-]+`. `account_ref` is an opaque
  internal ULID; the broker's account number lives in the personal-data vault.
- **No global order** across streams. Relationships use `causation_id` and `correlation_id`;
  cross-stream displays sort by `recorded_at` for readability only.
- **Cross-stream facts are copied by the owner** into the consuming stream with a `causation_id`:
  the executor writes `AgentModeApplied` (from `AgentModeChanged`) and `TradingDayStarted` and time-
  driven events (from the scheduler) into the account stream. A user's kill switch is a **command**
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
   data (for example, advisor weights) are encoded as arrays of `{key, value}` sorted by key bytes.
2. **Presence:** envelope fields and schema-declared payload fields are always present, `null` when
   empty. Map-valued objects (such as `config_refs`) omit absent keys and never contain `null`.
   Empty collections are `[]` or `{}`, never `null`. Schemas use `required` and
   `additionalProperties: false`.
3. **Strings:** escape only `"`, `\`, and U+0000–U+001F, using `\b \f \n \r \t` where defined and
   lowercase `\u00xx` otherwise; `/`, U+007F, U+2028, U+2029, and all non-ASCII characters are raw
   UTF-8; no Unicode normalization. **Lone surrogates are rejected** at ingest.
4. **Integers** only for listed fields (`envelope_version`, `seq`, `schema_version`, `attempt`,
   counts), in `0 … 2^53 − 1`, no leading zeros. **Money, quantities, and prices are never integers.**
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

**Account stream** (owner: executor)

| Event type | Required refs | Key payload fields |
|---|---|---|
| `StreamOpened` | — | stream type, subject, environment |
| `IntentReceived` | man | intent ID, agent, instrument, side, type, TIF, quantity, limit, purpose |
| `GateDecided` | fee, cal, ins, rule, man | intent ID, verdict, reason code, **`checks: [{id, result, inputs, computed}]`** with IDs matching trading spec §9.1 (`account_status`, `agent_mode`, `eligibility`, `concentration`, `session`, `halt`, `order_constraints`, `mark_freshness`, `collar`, `conduct`, `buying_power`, `gross_exposure`, `day_trade_budget`), `quotes_used`, `marks_used`, `data_profile` |
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
| `AgentModeApplied`, `TradingDayStarted`, `KillSwitchActivated` | — | copies of gating facts (with `causation_id`); kill-switch scope and initiator |

**Agent stream** (owner: agent runtime)

| Event type | Required refs | Key payload fields |
|---|---|---|
| `StreamOpened` | — | stream type, subject, environment |
| `ObservationRecorded` | — | source, instrument, data (artifact) |
| `ModelInvocationRecorded` | mod | purpose (compiler, fast model, research), provider, model and version, parameters, seed, prompt and retrieved context (artifact), response (artifact), provider request ID |
| `AdvisorOpinionRecorded` | man | advisor, version, instrument, signal, conviction, horizon, thesis (artifact) |
| `DecisionMade` | man | proposed action, confidence, advisor weights, autonomy classification |
| `IntentProposed` | man | intent fields (its `event_id` is the intent ID) |
| `ApprovalRequested`, `ApprovalDelivered`, `ApprovalResponded`, `ApprovalTimedOut` | man | content shown (artifact), channel and message ID, delivery status, responder (opaque) and role, step-up evidence (assertion ID, authentication time, method), separation-of-duties result |
| `AgentModeChanged`, `KillSwitchActivated` | — | from, to, reason; scope and initiator |

**Workspace control stream** (owner: workspace services)

| Event type | Required refs | Key payload fields |
|---|---|---|
| `MandateVersionCreated`, `MandateConfirmed` | — | source text (artifact), compiled fields, inferred fields, diff, confirming user (opaque) |
| `AgentDeployed`, `DeploymentRejected`, `AgentStopped` | man | agent, mandate version, reason |
| `ConnectionEstablished`, `ConnectionRevoked` | — | broker, scopes granted, permission-check result |
| `DisclosureAccepted` | — | document and version hash, user (opaque) |
| `OwnerAlertSent`, `OwnerAcknowledged` | — | subject event, channel, delivery status; user (opaque), authentication method |
| `ConfigSnapshotRegistered` | — | configuration kind (fee, calendar, instrument snapshot, rule set, mandate), content hash |
| `SurveillanceReportGenerated`, `BacktestRunRecorded` | rule | period, report (artifact), breaches; data snapshot, code build, configuration, results, paper/live/backtest marker |
| `PlatformOperatorAction` | — | action (stop, global kill switch, acceptable-use action), operator (opaque), approval |
| `AnchorComputed`, `VerificationRun`, `IntegrityIncidentRecorded` | — | leaves, root, timestamp token (artifact); scope and result; last good hash and anchor |
| `SegmentExported`, `SegmentEvicted`, `RetentionExtended`, `LegalHoldChanged` | — | manifest hash, range, retain-until, hold |
| `KeyRotated`, `KeyRevoked`, `RecordsAccessed`, `ExportCreated`, `PersonalDataErased` | — | key version; accessor (opaque), scope; export manifest; subject reference |

**Scheduler stream:** `ClockAdvanced`, `TradingDayStarted`, `ClockOffsetRecorded`,
`ClockToleranceExceeded`.

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
