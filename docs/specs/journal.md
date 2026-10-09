# Journal Spec (v1)

| | |
|---|---|
| **Status** | v0.31 (v0.2 founder sign-off 2026-09-25, [DEC-71](../project/04-decision-log.md#decisions); v0.3 amendment [DEC-81](../project/04-decision-log.md#decisions); v0.4 adds the research-agent events of [DEC-97](../project/04-decision-log.md#decisions) and [DEC-111](../project/04-decision-log.md#decisions); v0.5 approval escalation v0, [DEC-173](../project/04-decision-log.md#decisions), amended by [DEC-181](../project/04-decision-log.md#decisions), whose `DecisionMade` members [DEC-252](../project/04-decision-log.md#decisions) closes in §9.1; v0.6 closes the agent stream's payload schemas, [DEC-177](../project/04-decision-log.md#decisions); v0.7 closes the control-stream schemas `ValidationContext` reads, `AccountSnapshotRecorded`, and `OwnerCommandRefused`, [DEC-261](../project/04-decision-log.md#decisions); v0.8 closes the account-stream risk-state records `MandateVersionApplied` and `UniverseChanged`, [DEC-403](../project/decisions/DEC-403.md); v0.9 closes the research agent's thesis records `ThesisProposed` and `ThesisRevised`, [DEC-413](../project/decisions/DEC-413.md); v0.10 types `UniverseChanged`'s instrument as an asset ID and states what §9.3's mapping refuses, [DEC-404](../project/decisions/DEC-404.md); v0.11 types the thesis records' `instrument_id` as an asset ID, [DEC-413](../project/decisions/DEC-413.md) item 7; v0.12 adds the notice stream, [DEC-438](../project/decisions/DEC-438.md) items 5 and 28; v0.13 closes the account-stream executor records of §9.5, [DEC-446](../project/decisions/DEC-446.md) and [DEC-447](../project/decisions/DEC-447.md); v0.14 closes `OrderStateChanged`, [DEC-459](../project/decisions/DEC-459.md); v0.15 closes the approval and reconciliation records in §9.6, [DEC-460](../project/decisions/DEC-460.md); v0.16 binds effective policy and model-registry snapshots to production decisions, [DEC-484](../project/decisions/DEC-484.md); v0.17 closes the owner's approval answer and the runtime's two records of it, [DEC-533](../project/decisions/DEC-533.md); v0.18 adds the policy overlay's `decided_by` label, [DEC-536](../project/decisions/DEC-536.md); v0.19 registers a broker's capability profile as configuration, [DEC-531](../project/decisions/DEC-531.md) item 4 and [DEC-630](../project/decisions/DEC-630.md); v0.20 adds the connection records and `ConnectionEstablished`'s `account_ref`, [DEC-800](../project/decisions/DEC-800.md); v0.21 closes the records the workspace API commits for drafts, the compiler, confirmation, and owner requests, [DEC-670](../project/decisions/DEC-670.md); v0.22 adds the `client` actor and closes `ConnectionRevoked`'s reason and the client records, [DEC-671](../project/decisions/DEC-671.md); v0.23 closes the hold on new openings, [DEC-672](../project/decisions/DEC-672.md); v0.24 suspends a connection on a later `tools_missing`, [DEC-674](../project/decisions/DEC-674.md), and reads a later check with no pin as `contract_drift`, [DEC-676](../project/decisions/DEC-676.md) item 2; v0.25 catalogues the identity records and closes the membership records, [DEC-648](../project/decisions/DEC-648.md); v0.26 closes the records-access, export, and verification records, [DEC-780](../project/decisions/DEC-780.md); v0.27 closes the anchor and segment records, [DEC-783](../project/decisions/DEC-783.md); v0.28 closes the alert and notice records and the notice stream's `StreamOpened`, [DEC-720](../project/decisions/DEC-720.md); v0.29 completes §9.13's check codes and verifies an operator read's break-glass cause, [DEC-774](../project/decisions/DEC-774.md); v0.30 brings the connection vectors to §9.8's and §11's readings, [DEC-696](../project/decisions/DEC-696.md); v0.31 states the membership writer's order rule in §9.12, [DEC-659](../project/decisions/DEC-659.md)); changes need a decision-log entry (safety-critical) |
| **Implements** | PRD 6.7 (FR-7.1 to FR-7.7), FR-5.6, FR-5.7; backlog E5; milestone M4 |
| **Depends on** | [Trading domain spec §12–§13](trading-domain.md#12-journal-events) |
| **Test vectors** | [reference-cases/journal.yaml](reference-cases/journal.yaml) (version 3, with the generated `agent_stream` and additive `production_config_refs` and `policy_overlay` sections of §9.1, additive `broker_profile` section of §9, `control_stream` section of §9.2, `risk_state` section of §9.3, `research` section of §9.4, `account_stream` section of §9.5, `approval_answers` section of §9.7, `connections` section of §9.8, `workspace_api` section of §9.9, `client_actor` section of §3 and §9.10, `hold` section of §9.11 with its `held_mismatch` range cases, `membership` and `membership_fold` sections of §9.12, `records_access` section of §9.13 with its `break_glass_cause_mismatch` range checks, and `cold_records` section of §9.14; [reference/journal/generate.py](../../reference/journal/generate.py)) |

The journal is the append-only, hash-chained record of everything the platform does: the source
of truth for agent and account state (event-sourced), the audit trail, and the input to replay.

## Change history

- **v0.31 ([DEC-659](../project/decisions/DEC-659.md)):** §9.12 gains **Order**, which states a
  reading DEC-659 already took under DEC-176 (it only tightens). The identity crate implements
  `check_order`, and E9-7's writer slice implements `write_membership` (DEC-646).
  - **The writer.** The one writer of the seven `Member*` types on `ctl:{workspace_id}`
    ([DEC-646](../project/decisions/DEC-646.md)) checks order inside the serialized append, against
    `last`, the stream's last membership record: an `event_time` before `last`'s, or a `seq` not
    above it, is refused, and an equal instant passes. The first record passes; a refused record is
    never clamped and commits nothing, and the caller retries; a moved head re-runs the check.
  - **The fold.** [DEC-657](../project/decisions/DEC-657.md) item 4, which the text did not state:
    a membership record out of order makes the fold unreadable from its `event_time`, so
    `workspace_users` reads 1, as for a record that does not fit its state. It stays the backstop.
  - **Vectors.** None changes: every `membership_fold` history is already in `seq` and
    `event_time` order. The vectors stay as they are.

- **v0.30 ([DEC-696](../project/decisions/DEC-696.md)):** the reference model of §9.8's stream rules
  and §11's connection checks takes three readings the text already states and the E7-17 fold
  (`crates/mandate-journal/src/connection_fold.rs`) already pins; one sentence of §11 says the
  third aloud (DEC-176).
  - **Rule 66, version 1.** A version-1 first establishment is never re-established, not even by
    another version 1 whose absent `account_ref` matches it. The reference admitted that one.
  - **Each stream on its own.** Rules 66 to 68 hold on their own stream, so the reference folds each
    control and account stream apart: a revocation on another workspace's control stream revokes
    nothing here, and one account stream's `reauthorize` check never admits another's rotation.
    The reference folded every account stream as one, so it also refused two connections' streams
    interleaved, which the text admits.
  - **§11, `reconnect` after either version.** A version-2 establishment of an id already
    established on the same control stream, at version 1 or 2, rests on a `reconnect` check; and a
    rotation rests on an establishment on its own control stream. The reference counted only
    version-2 establishments and keyed them across control streams; §11's sentence now says both.
  - **Vectors.** The `connections` section gains four `sequences` (two rule-66 refusals, the
    interleaved streams, and the other stream's rotation, refused) and four `chains` (a `reconnect`
    after version 1, valid; a `connect` there, refused; and two rotations on another control
    stream, refused), with two stream and two chain mutants. Every earlier case keeps its answer.
    The vectors stay version 3.
- **v0.29 ([DEC-774](../project/decisions/DEC-774.md)):** two readings that only tighten
  (DEC-176), one of the text alone, and a carve-out that keeps the freeze off risk reduction.
  - **§9.13's codes.** `VerificationRun`'s `failure.check` listed every §11 code except
    `held_mismatch` (v0.23), `connection_lifecycle_mismatch`, and `connection_cause_mismatch`
    (v0.20), so a run could not record those failures: the reference validator refused them as
    `non_canonical`. The list gains all three, and rule 111 classes each as reported at an event,
    since §11 reports each at the record that breaks it.
  - **An operator read's cause.** Rule 108 requires a `platform_operator`'s `RecordsAccessed` to
    name a `causation_id`, but `append` cannot see what it names. §11 gains the control stream's
    check `break_glass_cause_mismatch`, reported at the read: the cause must be an earlier
    `PlatformOperatorAction` on the same control stream. That record stays open (DEC-261 item 9,
    Proposed), and `append` refuses it as `unknown_schema`, so until item 9 is decided every
    operator read that `append` admits fails this check. §9.13's list and rule 111 admit the code.
  - **§11's headings.** §11's per-range checks are now labelled by where each is reported: for the
    range as a whole (`anchor_root_mismatch`, `tsa_token_invalid`, `segment_manifest_mismatch`,
    `segment_gap`) or at an event (`anchor_head_mismatch` and every check §11 lists by stream), as
    rule 111 and the reference validator already read them. Rule 111's behaviour does not change.
  - **The freeze never holds risk reduction.** §11's control-stream freeze, which an operator read
    can now reach, says what the account- and agent-stream case already says of the kill switch:
    risk exits, protective orders, owner exits, and the kill switch at any scope still work
    (`AGENTS.md` rule 13), citing workspace API API-7, which already keeps its risk-reducing
    operations recorded while the control stream is frozen. Nothing else the freeze covers changes.
  - **Vectors.** The `records_access` section gains a valid run failing on each of the four codes
    with its `seq`, an invalid run naming none for each, and `range_checks`, control-stream chains
    the reference verifier and an oracle of its own judge, the first of them failing the operator
    read `read_by_an_operator` (still valid at `append`). The vectors stay version 3.
- **v0.28 ([DEC-720](../project/decisions/DEC-720.md), [DEC-795](../project/decisions/DEC-795.md) items 5 to 7):**
  §9.15 closes the records of v0.12's notice stream at schema version 1, with rules 119 to 130:
  the subject stream owner's `OwnerAlertSent` and the dispatcher's notice-stream `StreamOpened`,
  `NoticeIssued`, and `NoticeAttempted` ([notifications spec §5.5](notifications.md#55-records)).
  Three types join §9.1's: `notice_id` (32 lowercase hex digits, never an event id), `stream_id`
  (§2's form), and `opaque` (a provider's message id, with no space). The kinds are notifications
  spec §3.2's 33, each with its class, and the attempt reasons are §5.2's closed enum with the
  dispatcher's `not_pending` and `retry_window_ended`; `notification_address_changed` and
  `address_missing` are DEC-795's (items 6 and 7). None of the four records holds free text. Three
  readings only refuse more (DEC-720 item 8, DEC-176): `OwnerAlertSent` is on the agent, account,
  or control stream, not "any stream type", since no kind has a scheduler subject; a `NoticeIssued`'s
  cause is an `OwnerAlertSent`, an `ApprovalRequested`, or for `channel_lost` a `NoticeAttempted`,
  never an owner command, which its alert's `owner_command` names; and notifications spec §3.4 says
  so for a user's kill switch. §9's control-stream table gains the `NotificationAddressChanged` row
  the workspace API's push-address routes write (DEC-795 item 5); its payload schema closes with
  E8-14. The vectors are unchanged: `crates/mandate-journal/tests/notices.rs` pins §9.15 (DEC-720
  item 7), and its pending tests list DEC-720's 32 kinds and 12 reasons, which DEC-795's two
  additions extend without a test refusing either.
  - **Order of the changes (ES-22).** The tests PR merged before this spec text; E8-9's
    implementation PR registers the four schemas and deletes only the tests' `#[ignore]` lines.
- **v0.27 ([DEC-783](../project/decisions/DEC-783.md)):** §9.14 closes the control stream's
  `AnchorComputed` and `SegmentExported` at schema version 1, with rules 113 to 118, from their §9
  rows, §6.2 and §10, using the shapes the code already reads: the anchor file of DEC-115 item 6
  and DEC-263's six manifest fields with the manifest hash. A verifier can therefore take a trusted
  start from either record in the journal itself. §11 gains the control stream's per-range check
  `anchor_self_mismatch`: an anchor's leaf for its own control stream names the event just before
  it, which an append rule cannot check because the journal assigns the `seq`. §9.13's
  `VerificationRun` admits the new code. The vectors gain a generated `cold_records` section and
  stay version 3.
- **v0.26 ([DEC-780](../project/decisions/DEC-780.md)):** §9.13 closes the control stream's
  `RecordsAccessed`, `ExportCreated`, and `VerificationRun` at schema version 1, with rules 107 to
  112, from the members their §9 rows list (accessor and scope; export manifest; scope and result),
  and `RecordsAccessed` adds the opaque resources read and an optional stored result, so the audit
  routes and the workspace API's other journaled reads write one record. v0.22's `client` actor
  records its own reads there, in §3's one shape (rules 81 to 83), with its own `id` as `accessor`
  (rule 108), and rule 83's note that the payload was not closed now points here. Each names what
  it covers as stream ranges of the writer's own workspace, bounded by event hashes, and nothing
  else: no instrument, order, position, or mandate content. Two types join §9.1's: `stream_id`
  (§2's form) and `digest` (64 lowercase hex: an event's hash, or the canonical export's verifier
  digest, which is not a `ref` and so is not listed in `artifact_refs`). Rule 107 refuses a range on
  another workspace's stream. The examination bundle (§12), which may carry resolved identities, is
  not an `ExportCreated` form at this version, and an export is named by its verifier digest
  (DEC-265 item 3), so no second manifest shape is defined. §7 now says a read or export is
  journaled before it is served, as API-16 already does for exports. The vectors gain a generated
  `records_access` section and stay version 3.
  - **Order of the changes (ES-22).** Spec and vectors first; `mandate-journal` registers the three
    schemas in E12-3's tests PR and implementation PR, so no Rust test reads the new section yet.
- **v0.25 ([DEC-437](../project/decisions/DEC-437.md) item 9, [DEC-648](../project/decisions/DEC-648.md)):**
  §9's control-stream table catalogues the identity spec's §12.1 records. §9.12 closes the seven
  membership records at schema version 1, with rules 96 to 106, and states the fold that identity
  spec §5.3's `workspace_users` reads. It adds `MemberInvitationRevoked`, which §12.1 lacked for its
  §5.1 `invited` to `revoked` transition, the accepted `invitation` on `MemberActivated`, and the
  cool-off end on `MemberReactivated`; each journals a state change identity spec §5 already defines.
  `MemberInvited` also carries `invited_at`, each grant the effective `independent_approval_required`,
  `MemberReactivated` the roles it restores, and each membership record, until the envelope gains
  identity spec §12.2's session field, a payload `session_ref`. A record's own instant is its
  envelope's `event_time`.
  The credential, session, service-account, host-CLI, and break-glass records stay open until
  their own change; the client records are §9.10's (v0.22, [DEC-671](../project/decisions/DEC-671.md)). The vectors gain a generated, additive `membership` section, so they stay
  version 3. A generated `membership_fold` section holds membership histories with their expected
  states and counts, reproduced by a reference fold.
- **v0.24 ([DEC-674](../project/decisions/DEC-674.md), [DEC-676](../project/decisions/DEC-676.md)):** §9.8 states that a later check (occasion
  `executor_start` or `daily`) whose `contract` result failed with `tools_missing` moves the
  connection to `suspended`, journaled as `ConnectionStateChanged` with reason `check_failed`, as
  [connections spec §8.1](connections.md#81-at-connect-at-every-executor-start-and-daily) check 1
  says for an allowlisted tool that is absent; `contract_drift` with no other failure degrades. §9.8's
  reasons table also reads a later check that finds no pin as `contract_drift`
  ([DEC-676](../project/decisions/DEC-676.md) item 2). No rule changes: rule 59 already admits `check_failed` into `suspended`. The vectors gain the valid drafts
  `checked_daily_with_tools_missing` and `suspended_on_a_failed_check` and the sequence
  `mcp_later_tools_missing_suspends`, and stay version 3.
- **v0.23 ([DEC-672](../project/decisions/DEC-672.md)):** §9.11 closes the hold on new openings
  ([DEC-191](../project/04-decision-log.md#decisions), [workspace API spec §4.2](workspace-api.md#42-deployments-and-the-agent-lifecycle-deployment-manager)).
  `OwnerCommandIssued` is closed for its two new commands, `hold_openings` and `lift_hold`, and only
  for them. The agent runtime's copies gain the hold at schema version 2: `AgentModeChanged` adds
  `held` and the reasons `owner_hold` and `owner_lift_hold`, and `OwnerCommandRefused` adds the
  command `lift_hold`. Rules 90 to 95: a client may hold and only a user lifts, a hold carries no
  step-up, and the mode a copy leaves is never looser than the owner's pause or hold, so a resume
  never clears a hold and a lift never clears a pause or a latched limit. Rule 83 admits a client's
  `OwnerCommandIssued`, which rule 90 confines to `hold_openings`. Version 1 of both agent-stream
  records stays registered and unchanged. The vectors gain a generated `hold` section and stay
  version 3.
- **v0.22 ([DEC-671](../project/decisions/DEC-671.md)):** §3's closed `actor.kind` set gains
  `client`, an owner-connected agent ([workspace API spec §3.3](workspace-api.md#33-authentication-and-sessions)
  item 4, [identity spec §12.2](identity.md)), whose actor alone carries `on_behalf_of`, the user it
  acts for. Every other actor keeps exactly its four members, so no recorded event changes. Rules 81
  to 83 shape the client actor and confine it to the control stream's `MandateDraftSaved`,
  `OwnerRequestSubmitted`, and `RecordsAccessed`. §9.9's rules 70 and 79 already had client branches,
  which now admit. §9.10 closes `ConnectionRevoked` version 2, with the reason `owner` or
  `compromised` and its step-up (workspace API §5.6), and `ClientConnected` and `ClientRevoked`
  (identity spec §12.1), with rules 84 to 89; `ClientRevoked` records why with a closed `reason`.
  Version 1 of `ConnectionRevoked` stays registered and unchanged. The vectors gain a generated
  `client_actor` section and stay version 3.
- **v0.21 ([DEC-670](../project/decisions/DEC-670.md)):** §9.9 closes four control-stream records
  the [workspace API](workspace-api.md) commits (its §4.1, §4.6, §5.1; DEC-436 item 14), with rules
  69 to 80. `MandateDraftSaved` records one explicit draft save, its draft stored as an artifact.
  `ModelInvocationRecorded` may now be written on the control stream, for the compiler alone, closed
  there with [DEC-432](../project/decisions/DEC-432.md) item 11's members; this answers the
  [inference spec's §13 question 1](inference.md#13-open-questions). On the agent stream it stays
  open, for E15-8. `MandateConfirmed` gains `agent_id` and `base_version` at schema version 2, so a
  confirmation names the agent it is for and the version it replaces. `OwnerRequestSubmitted`
  records an owner's request. These are the first rules that read `actor.kind`: only a user
  confirms, the compiler's record is the services', and `requested_by` follows the actor, never
  the body. `MandateConfirmed` version 1 stays registered and unchanged. No other record changes.
  The vectors gain a generated `workspace_api` section and stay version 3.
- **v0.20 ([DEC-800](../project/decisions/DEC-800.md)):** §9.8 journals a connection's history
  (connections spec CN-10, backlog E7-17). `ConnectionEstablished` gains `schema_version` 2, its
  version-1 members then `account_ref`, the connecting `user`, `step_up`, and the owner's
  `margin_attestation` for a live connection; `account_ref` binds the account stream to its
  connection (DEC-261 item 10). New records: `ConnectionRefused` (a refused check or a teardown)
  and `ConnectionCredentialRotated` on the control stream, and `ConnectionChecked`,
  `ConnectionStateChanged`, `ConnectionCredentialRefreshed`, and the executor's copies of the
  establishment and each rotation on the account stream, with consistency rules 54 to 65, of which
  61 and 63 are copy rules. The connecting executor journals its permission checks on the account
  stream, and `ConnectionEstablished` names them as its cause. Stream rules 66 to 68 allow a second
  `ConnectionEstablished` for a `connection_id` only after its `ConnectionRevoked`, with the same
  broker, environment, and `account_ref`; keep a rotation's scopes the same or narrower; keep one
  `account_ref` and one account stream to one connection; keep the account stream `connecting`
  until it holds its establishment's copy; and let the owner's acknowledgment return a connection
  to `active` only after its cause cleared, a suspension only on a credential the control services
  accepted after the suspension. The connection's owner checks them before appending, §11's new
  `connection_lifecycle_mismatch` checks every range, and the full-chain run's
  `connection_cause_mismatch` follows each cause across the two streams. No rule refuses a
  `ConnectionRevoked`. The vectors gain a generated, additive `connections` section and stay
  version 3.
- **v0.19 ([DEC-531](../project/decisions/DEC-531.md) item 4, [DEC-630](../project/decisions/DEC-630.md);
  [DEC-529](../project/decisions/DEC-529.md)):** `ConfigSnapshotRegistered` version 3 admits the
  kind `broker_profile`: a connector's capability profile (trading spec §5.2), registered under
  its content hash so replay and audit see which broker rules applied. §9 states its stored
  object as DEC-630 item 6 fixes it, and rule 21b checks it at append as rule 21a checks the
  version-2 kinds. Versions 1 and 2 keep their closed vocabularies and refuse the new kind. No
  record names a `broker_profile` in `config_refs` yet. The vectors gain a generated, additive
  `broker_profile` section, so the `production_config_refs` cases and their counts are unchanged,
  and they stay version 3.
- **v0.18 ([DEC-536](../project/decisions/DEC-536.md)):** `DecisionMade`'s `decided_by` may be
  `policy_overlay`, for a decision the effective policy changed at
  [mandate spec §6.2 step 5c](mandate.md#62-evaluation): an `auto` it narrowed to `ask` because the
  effective `auto_allowed` is false (§4.3), or an opening or increase it denied because the
  confirmed version is nonconforming ([DEC-534](../project/decisions/DEC-534.md) item 2). It is
  the same label the approval content's `trigger.decided_by` carries (mandate spec §6.4, where its
  `trigger.rule` is `null`). Rule 7 refuses it on an `auto`, and rule 5 already refuses it on a
  decision that is not an opening or an increase. The vectors gain a generated, additive
  `policy_overlay` section, so the `agent_stream` cases and their counts are unchanged, and they
  stay version 3. Version 1 and version 2 of `DecisionMade` keep their payload: only the label's
  closed set grows.
- **v0.17 ([DEC-533](../project/decisions/DEC-533.md)):** §9.7 closes the control stream's
  `ApprovalResponseSubmitted` and the agent stream's `ApprovalResponded` and
  `ApprovalRevalidated` at schema version 1, with rules 46 to 53, from the members their §9 rows
  and mandate spec §6.4 list. Their times are integer risk-clock seconds, as §9.6's
  `ApprovalRequested.deadline` and every writer of them are. That differs from the step-up
  evidence of §9.2's and §9.3's closed records, whose `authenticated_at` is a §4.7 timestamp;
  unifying the two is a later version (backlog). The vectors gain a generated `approval_answers`
  section and stay version 3.
- **v0.16 ([DEC-484](../project/decisions/DEC-484.md)):** `config_refs` gains `policy_set` and
  `model_registry`. Version 2 of `DecisionMade` requires both beside `mandate_version`; version 2
  of `ModelOutputRecorded` requires `model_registry` beside `mandate_version`; both keep version
  1's payload unchanged. `ConfigSnapshotRegistered` version 2 admits the two new kinds. These
  hashes bind the independently mutable policy and registry snapshots that admitted the
  production decision and model output. Existing version-1 records are unchanged.
- **v0.15 ([DEC-460](../project/decisions/DEC-460.md)):** §9.6 closes
  `ApprovalRequested`, including its nested canonical content, `ApprovalDelivered`,
  `BrokerPositionObserved`, `AgentModeApplied`, and `CompensatingEvent` at schema version 1.
- **v0.14 ([DEC-459](../project/decisions/DEC-459.md)):** `OrderStateChanged` version 1 is a
  closed record whose nullable evidence and boolean lifecycle markers are always present. This
  closes new appends without changing the executor fold's tolerance of historical sparse records.
- **v0.13 ([DEC-446](../project/decisions/DEC-446.md), [DEC-447](../project/decisions/DEC-447.md)):**
  §9.5 closes the account stream's executor records ([DEC-360](../project/decisions/DEC-360.md),
  option (c)). `IntentReceived`, `GateDecided`, and `OrderSubmitted` gain `risk_clock` at
  `schema_version` 2, each its version 1's members with `risk_clock` last; version 1 is never
  edited (§8) and stays registered. `OrderSubmitted`'s executor-only members move to a companion
  record, `OrderRequestRecorded` (version 1), journaled immediately before its order in one
  `append` batch and named by its `causation_id` (rule 45). `ProtectionChanged` is closed with
  the members the fold reads (rules 39 to 44), its action vocabulary gaining `intended` for the
  receipt-time protective prices, which leave `IntentReceived`, never a member there at either
  version. `StreamOpened` and `OwnerCommandRefused` carry no `risk_clock` (DEC-447). The vectors
  gain a generated `account_stream` section and stay version 3.
  - **Why:** §9.2 already says the executor writes `risk_clock` on every account-stream event,
    and the fold requires it (DEC-302 item 1); the three records were the only ones that sentence
    and the version-3 form left without it. A schema once closed is never edited in place, so the
    member joins at a new version, which no conforming version-1 record is refused by. The rest
    keeps `IntentReceived` the exact copy of the proposal §9 and rule 10 make it, and
    `OrderSubmitted` the version-3 request, instead of widening either toward the code (DEC-360
    item 2, the founder's ruling). The fold's reads of legacy records stay as tolerant as they
    were, so a recorded stream stays replayable (§8); `append` is what tightens.
  - **Order of the changes (ES-22).** Spec and vectors first, as DEC-174 item 4 and the
    coordinator's comment on #482 sequence §9.x work; the `mandate-journal` registration and the
    fold's reads follow in the tests PR and the implementation PR.
- **v0.12 ([DEC-438](../project/decisions/DEC-438.md) items 5 and 28):** the notice stream. A new
  stream type, `ntf:{workspace_id}`, whose single writer is the workspace's notification dispatcher
  ([notifications spec §5.1](notifications.md#51-the-dispatcher)), holds `NoticeIssued` and
  `NoticeAttempted`. `OwnerAlertSent` becomes the subject stream owner's record that an alert was
  raised, written in that stream and the subject's batch, with the alert's kind; its delivery
  members move to `NoticeAttempted`. Nothing writes `OwnerAlertSent` today, so no record changes
  meaning. No payload schema is closed here; E8-9's tests PR closes them.
- **v0.11 ([DEC-413](../project/decisions/DEC-413.md) item 7):** §9.4 types `instrument_id` as §9.3's `asset_id`, not any `id`. It only refuses more (DEC-176), and adds no member.
  - **Why:** v0.9 typed it `id` on DEC-403's reading, which v0.10 tightened for `UniverseChanged.instrument`. A thesis admitted on an instrument that is not an asset ID would reach the executor's `UniverseChanged`, which v0.10 refuses, so the admission would be journaled with no universe change to follow it. No conforming writer emits anything else: the platform resolves the instrument before it writes the entry.
  - **The vectors gain four `research` drafts**, as §9.3's: a ticker (`BTCUSD`, a valid `id`), an asset ID with a trailing newline, and one in capitals, each refused as `non_canonical` at `payload.instrument_id`, and a number, refused as `schema`. The reference validator reuses §9.3's `asset_id` type, and its four seeded bugs (`types.asset_id`, `types.asset_id_case`, `types.asset_id_ident`, `types.asset_id_trailing_newline`) are each caught in the `research` section too.
  - **Order of the changes (ES-22).** Unlike v0.10's §9.3 drafts, these can come first: `mandate-journal` does not register §9.4 yet, and every Rust test that reads the `research` section is pending until that registration (DEC-77).
- **v0.10 ([DEC-404](../project/decisions/DEC-404.md) item 9):** §9.3 tightens two things; neither adds a member, and both only refuse more (DEC-176).
  - **`UniverseChanged`'s `instrument` is a new `asset_id` type**: mandate spec §3's lowercase uuid form, not any `id`. An `id` that is not an asset ID appended before this and then left the stream's `ValidationContext` unbuildable, since the mapping parses it as an asset ID. A context that will not build is not a hold an exit may have (`AGENTS.md` rule 13). No conforming writer emits anything else.
  - **§9.3's mapping table states the classification check.** A `MandateVersionApplied` whose `classification` is not mandate spec §9.2's verdict of its two stored documents is refused, whether it was applied or rejected. That is what the registration implements (#497).
  - **The vectors gain three report-order drafts:** rules 29 then 30, 30 then 33, and 31 then 32, each with its seeded bug.
  - **Order of the changes (ES-22).** The `asset_id` drafts and the reference validator's type follow the `mandate-journal` code change that types the member. `mandate-journal` parses every §9.3 vector, so vectors added first would turn `main` red, as #443 and #474 ordered rule 28. Until that code change, both `mandate-journal`'s schema and the reference validator still typed `instrument` as `id` (#509, then the reference PR that added the `asset_id` drafts).
- **v0.9 ([DEC-413](../project/decisions/DEC-413.md)):** §9.4 closes the payload schemas of the
  agent stream's `ThesisProposed` and `ThesisRevised`, which v0.6 left to their own story. Both
  share one schema. Every member comes from mandate spec §8.2's output fields, §8.4's thesis table,
  or §9's row, and is traced to `mandate-research`'s `ThesisEntry` or the writer that adds it. The
  new rules are 34 to 38: the entry type follows the revision, the reason is null exactly on
  admission, checks 1 to 3 and check 15 are recomputed from the record, and the model reference
  binds the content hash. The cited sources are recorded in the order given. No fold reads the records yet, so §9.4
  maps them to no `JournaledFact`. The test vectors gain a generated `research` section and stay
  version 3.
- **v0.8 ([DEC-403](../project/decisions/DEC-403.md)):** §9.3 closes the payload schemas of the two
  account-stream records `ValidationContext::from_journal` reads that v0.7 left to their own story
  (DEC-303 item 6): `MandateVersionApplied` and `UniverseChanged`. Every member comes from mandate
  spec §5.10's row, from a writer that exists (`mandate-spec`'s risk fold, `mandate-research`'s
  admission and removal), or from §2's `risk_clock`. The new rules are 29 to 33, and §9.3 maps each
  record to its `JournaledFact`. The test vectors gain a generated `risk_state` section and stay
  version 3.
- **v0.7, amended ([DEC-351](../project/decisions/DEC-351.md) item 5):** `OwnerCommandRefused` may
  carry `not_independent`, the executor's refusal of an acknowledgment that lifts a fired tripwire
  under `independent_approval_required` from the user who requested the lift ([mandate spec
  §6.7](mandate.md#67-tripwires-dec-187-dec-350-dec-351)); rule 28 keeps the reason to the account
  stream, and `OwnerAcknowledged` names the requesting user and carries the independence requirement
  as it stood when the lift was requested. The reference validator's reason list
  gains it. `mandate-journal`'s schema and rule 28 came first, in their own code PR
  ([#474](https://github.com/kunwarshivam/mandate/pull/474)), because it parses every §9.2 vector and
  code cannot ship with a reference change (ES-22); then rule 28's validator check, its vectors (an
  acknowledgment refused as `not_independent`, accepted; a Stop refused as `not_independent`,
  refused), and its seeded bug.
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
| Notice | `ntf:{workspace_id}` | The workspace's notification dispatcher ([notifications spec §5.1](notifications.md#51-the-dispatcher)) | Notices issued and every delivery attempt. Never a risk input; no other stream's owner copies from it |

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
  approval as `ApprovalResponseSubmitted`, a pause, resume, Stop, owner exit, or kill switch as
  `OwnerCommandIssued` ([mandate spec §6.1](mandate.md#61-purposes)), and a request for an order as
  `OwnerRequestSubmitted` (§9.9). The runtime hands a request to its order builder, which sizes,
  clips, and classifies it as it does any proposal, and its `DecisionMade` names the request as
  `causation_id`, with the request's `requested_by` and `client_id`. The link crosses streams, so
  `append` does not check it, as for the other copies. The agent runtime copies each
  event addressed to its agent at most once, with `causation_id` pointing to it (`ApprovalResponded`,
  `AgentModeChanged`, `OwnerExitRequested`, `KillSwitchActivated`, or, for a resume, Stop, or lift
of a hold its step-up does not count, `OwnerCommandRefused`; a hold and a lift are copied at
schema version 2, §9.11); the control stream's `event_id`
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
| `actor` | writer | `{kind, id, version, build}`; `kind` ∈ `system`, `agent`, `user`, `broker`, `platform_operator`, `client`; `build` is the binary digest (`sha256:…`) or `null` for external actors; users are opaque IDs. A `client` actor, and only it, has a fifth member, `on_behalf_of` (below) |
| `config_refs` | writer | Map of content hashes; required keys per event type (§9) |
| `payload` | writer | Per the event type's schema |
| `artifact_refs` | writer | Sorted, de-duplicated set of every `sha256:` reference in the payload |
| `pii_refs` | writer | Sorted set of opaque vault references (random IDs, never hashes of personal data) |
| `hash` | *journal* | SHA-256 of the canonical body; **not part of the body** |

**The client actor** ([DEC-671](../project/decisions/DEC-671.md)). An owner-connected agent
(DEC-141) is recorded as `{kind: "client", id, version, build: null, on_behalf_of}`: `id` is the
client's opaque ID, and `on_behalf_of` the opaque ID (an `id`) of the user it acts for, from its
token, never from a request body. This is the one actor shape every record a client may write uses.
A client is never a `user`, so a rule that requires a user actor, such as
[mandate spec §6.4](mandate.md#64-approvals) check 3, refuses a client from the record alone. A rule
that compares people reads the **human** of an actor: `on_behalf_of` for a client, `id` for anyone
else. So check 7's "not the mandate's author", [mandate spec §5.8](mandate.md)'s requester, and
[identity spec ID-6](identity.md) count a client as its user, and a person's own client is never
their second party. Checked at append, after the envelope's types (reason `schema`):

81. `on_behalf_of` is present exactly when `kind` is `client`, and is then an `id`
    (`actor.on_behalf_of`; a well-typed string of the wrong form is `non_canonical`). Every other
    actor is exactly as before. The API sets it from the client's token, which names a user, so it
    is never the client's own `id`. The host CLI's `system` actor with `on_behalf_of` (identity spec
    §6.4) is not admitted by this version; the change that adds `HostCliRegistered` widens this rule.
82. A `client` actor's `build` is `null` (`actor.build`): a client is external.
83. A `client` actor is on the control stream, on `MandateDraftSaved`, `OwnerRequestSubmitted`,
    `RecordsAccessed`, or `OwnerCommandIssued` only (`actor.kind`), reported before the payload is
    read. `RecordsAccessed`'s payload is closed in §9.13, where a client's read uses this actor
    shape and names the client's own `id` as `accessor` (rule 108). These four records are what its
    `propose`, `request`, `read` and `dry_run`, and `hold` scopes allow (workspace API §3.8); rule 90 confines its
    `OwnerCommandIssued` to `hold_openings`. Whatever else identity spec ID-11 forbids a client (confirming, approving,
    acknowledging, pausing and every other owner command, connections, membership) is refused at
    append, and check 3 refuses an approval again at the runtime.

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
`ExportCreated`; §9.13) before it is served. In hybrid mode the platform receives anchors (hashes)
only.

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
`mandate_version`, `mod` = `model_version`, `pol` = `policy_set`, `reg` = `model_registry`).
`policy_set` names the canonical effective-policy snapshot: an object with exactly
`kind: "policy_set"`, `policy_set_version: 1`, and `levels`. Each `levels` item is a complete
document valid against [`policy.schema.json`](../../schemas/policy.schema.json), ordered
`platform`, `organization`, `workspace`, with at most one of each. `model_registry` names an object
with exactly `kind: "model_registry"`, `model_registry_version: 1`, and `models`; each model has
exactly `model_id`, `model_version`, `content_hash`, `params`, and `admits_instruments`. Models are
strictly sorted and unique by `model_id`; `params` are strictly sorted and unique. These objects
are canonical configuration artifacts registered on the control stream before use.
At append, these two new kinds have an additional object-shape check: a `policy_set` or
`model_registry` reference whose stored object is absent is `missing_artifact` at that
`config_refs.<kind>`, and an object whose own `kind` differs is `config_ref_kind` at the same path.
The older kinds retain their existing object contracts. For a version-2 `ConfigSnapshotRegistered`
of either new kind, the corresponding paths are `payload.content_hash` and `payload.kind`.

`broker_profile` names a connector's capability profile ([trading spec §5.2](trading-domain.md),
[DEC-531](../project/decisions/DEC-531.md), [DEC-630](../project/decisions/DEC-630.md)): an object
with exactly `kind: "broker_profile"`, `profile_version` (an integer, at least 1), `rows`, and
`idempotency`.

- `rows` is a non-empty array of objects with exactly `asset_class` (`crypto`, `us_equity`),
  `session` (`overnight`, `pre_market`, `regular`, `after_hours`, `crypto`), and `cells`, strictly
  sorted and unique by `asset_class` then `session`.
- `cells` is a non-empty array of objects with exactly `order_type` (`limit`, `market`, `stop`,
  `stop_limit`), `quantity_form` (`fractional`, `notional`, `whole`), `times_in_force`, and
  `protection_forms`, strictly sorted and unique by `order_type` then `quantity_form`.
- `times_in_force` is a non-empty set of `day`, `gtc`, `ioc`, and `protection_forms` a set, possibly
  empty, of `bracket`, `oco`, `stop_limit`; each set is strictly ascending by bytes.
- `idempotency` has exactly `client_order_id` and `query_by_client_order_id` (booleans) and `retry`
  (`idempotent`, `not_idempotent`, `unknown`). Without a client order id, `retry` is
  `not_idempotent` and `query_by_client_order_id` is false.

Every comparison is by the bytes of the spelling. The profile is a canonical configuration artifact
registered on the control stream before use. No record names it in `config_refs` yet.
The producer enforces this shape: `mandate-domain`'s profile constructor refuses a profile that
breaks it (DEC-630 item 7), and the vectors' `invalid_artifacts` use its refusal codes
(`profile_version_zero`, `profile_no_rows`, `profile_empty_row`, `profile_duplicate_row`,
`profile_duplicate_cell`, `profile_no_time_in_force`, `profile_claim_without_client_id`). Two codes
are the vectors' own, for stored bytes the constructor cannot produce: `schema` for a member or
spelling outside the contract, and `non_canonical` for an array that is not strictly sorted, since
the constructor sorts what it is given. Append binds only the object's kind, its presence and its
hash (rule 21b), as rule 21a does for the version-2 kinds.

**Account stream** (owner: executor). Risk inputs also carry `risk_clock` (§2).

| Event type | Required refs | Key payload fields |
|---|---|---|
| `StreamOpened` | — | stream type, subject, environment |
| `IntentReceived` | man | intent ID, agent, instrument, side, type, TIF, quantity, limit, purpose |
| `GateDecided` | fee, cal, ins, rule, man | intent ID, verdict, reason code, **`checks: [{id, result, inputs, computed}]`** with IDs matching trading spec §9.1 (`account_status`, `agent_mode`, `eligibility`, `concentration`, `order_size`, `session`, `halt`, `order_constraints`, `mark_freshness`, `collar`, `conduct`, `buying_power`, `gross_exposure`, `day_trade_budget`), `quotes_used`, `marks_used`, `data_profile` |
| `OrderSubmitted`, `OrderAbandoned` | — | client order ID, attempt, broker status, internal state |
| `OrderStateChanged` | — | Closed at schema version 1: required `client_order_id`, `state`, and `risk_clock`; always-present nullable `attempted`, `broker_status`, `filled_qty`, `reject_code`, `replaces`, `replaced_by`, `replaced_by_broker_order_id`, and `lookup`; always-present booleans `ignored`, `cancel_requested`, `cancel_confirmed`, `cancel_overdue`, `adopted`, and `ladder_step`. `state` and non-null `attempted` use the complete executor order-state vocabulary; non-null `lookup` is `absent`; `filled_qty` is a decimal; opaque text is non-empty ([DEC-459](../project/decisions/DEC-459.md)) |
| `OrderRequestRecorded` | — | the exact request a version-2 `OrderSubmitted` names (DEC-360 option (c)): agent, intent, purpose, extended hours, the protective trigger and legs, and a ladder rung's; closed in §9.5 |
| `BrokerExchangeRecorded` | — | direction, endpoint, `raw` (or artifact), status; credentials redacted |
| `FillApplied`, `LateFillApplied` | fee, cal, set, ins | fill ID, client order ID, gross quantity, price, fees, trade date |
| `FeesCharged` | fee | family, day, accrued, charged |
| `MarkUpdated` | — | instrument, price, source, feed |
| `SettlementPosted`, `DividendPaid`, `CashInLieuPosted` | set | date, instrument, amount |
| `CorporateActionPrepared`, `CorporateActionApplied` | ins | instrument, action, ratio or amount, ex-date |
| `ProtectionChanged` | — | instrument, action, orders, unprotected-interval start or end; closed in §9.5 |
| `BrokerPositionObserved` | — | Closed at schema version 1: instrument, broker and model quantities, mismatch flag, and risk clock (§9.6) |
| `ReconciliationRun` | — | result (`clean`, `adopted`, `mismatch`), nullable activity checkpoint, snapshot head, difference count, risk clock (DEC-453) |
| `CompensatingEvent`, `AccountSnapshotRecorded` | — | Closed `CompensatingEvent` version 1: order subject, order-state difference, from/to states, corrected event IDs, and risk clock (§9.6); daily snapshot |
| `AccountStateObserved` | — | Closed at schema version 1: `status`, `crypto_status`, `trading_blocked`, `account_blocked`, `trade_suspended_by_user`, `multiplier`, `equity`, `cash`, `buying_power`, `non_marginable_buying_power`, `accrued_fees`, and `risk_clock` ([DEC-458](../project/decisions/DEC-458.md)); no broker account identifier or personal data |
| `RejectObserved`, `AccountRestrictionChanged` | — | reject code and message, restriction; for `AccountRestrictionChanged`, its `cause` (`broker_reject`, `broker_notice`, `connection_unavailable`; [trading spec §7.3](trading-domain.md#73-account-restrictions), [DEC-441](../project/decisions/DEC-441.md) item 23) |
| `ExternalActivityIngested`, `RelatedAccountsCoordination` | — | unattributed activity; canceled opening orders across the group |
| `ConductBreachDetected` | rule | control, agent, instrument, measured value |
| `AgentModeApplied`, `TradingDayStarted`, `KillSwitchActivated` | — | Closed `AgentModeApplied` version 1: agent (including `*`), mode, restriction, originated flag, and risk clock (§9.6); other gating facts copied or originated (with `causation_id`); kill-switch scope, initiator, orders canceled, sells planned or deferred |
| `OwnerAcknowledged` | — | copied from the control stream (with `causation_id`); a risk input |
| `ConnectionChecked`, `ConnectionStateChanged`, `ConnectionCredentialRefreshed` | — | The executor's permission checks and their results; the connection's state (`active`, `degraded`, `suspended`) and why; a refreshed token's scopes; each with `risk_clock`, closed in §9.8 |
| `ConnectionEstablished`, `ConnectionCredentialRotated` (copies) | — | The executor's copies of the control stream's establishment (version 2) and each rotation, with `risk_clock` and the original as `causation_id`; closed in §9.8 |
| `OwnerCommandRefused` | — | An acknowledgment the executor refused, for its step-up or, under `independent_approval_required`, because it is not independent ([mandate spec §6.1](mandate.md#61-purposes), [§6.7](mandate.md#67-tripwires-dec-187-dec-350-dec-351)): the command (`acknowledge`), the reason (`step_up_missing`, `step_up_stale`, `step_up_reused`, `step_up_method`, `not_independent`), and the effective time it was judged at; `causation_id` is the control stream's `OwnerAcknowledged`, copied at most once. The agent runtime records a refused resume or Stop the same way on the agent stream |
| `MandateVersionApplied`, `RiskDayStarted`, `RiskLimitTriggered`, `RiskLimitLifted`, `HighWaterMarkReset`, `PositionReleased`, `InstrumentRestrictionChanged`, `GoalCompleted` | man | agent risk state ([mandate spec §5.10](mandate.md#510-journal-events)): version result, classification, and allocation change; day-start equity; limit, action, E, H, drawdown, E₀, capital base C, inherited loss L, net contributed N, and for a tripwire (limit `tripwire:<id>`, reason `tripwire_condition`, [mandate spec §6.7](mandate.md#67-tripwires-dec-187-dec-350-dec-351)) its metric, threshold, and the value reached; reset evidence; released positions; stale-mark and removed-instrument changes with the reason; goal completion |
| `UniverseChanged` | man | The working universe changed ([mandate spec §2.3, §8.5](mandate.md#23-the-working-universe-at-runtime-dec-97)); a risk input, so it carries `risk_clock`: agent, instrument, change (`admitted`, `removed`), reason (`thesis_admitted`, `thesis_expired`, `thesis_invalidated`, `lineage_retired`, `eligibility_lost`, `operator_halt`, `version_applied`), thesis and lineage ids, working-universe size after |

**Agent stream** (owner: agent runtime)

| Event type | Required refs | Key payload fields |
|---|---|---|
| `StreamOpened` | — | stream type, subject, environment |
| `ObservationRecorded` | — | source, instrument, data (artifact) |
| `ModelInvocationRecorded` | mod | purpose (fast model, research; the compiler's is on the control stream, §9.9), provider, model and version, parameters, seed, prompt and retrieved context (artifact), response (artifact), provider request ID |
| `ModelOutputRecorded` | v1: man; v2: man, reg | signal model id, version, content hash, instrument, as_of, expires_at, direction, conviction, confidence, horizon, thesis (artifact) and, for the research agent, its thesis and lineage ids; `ignored` reason if not used |
| `ThesisProposed`, `ThesisRevised` | man, mod | The research agent's output and its admission decision ([mandate spec §8.4, §8.5](mandate.md#84-the-research-agent-dec-97-adr-0002)): research agent id, version, and content hash; thesis id, lineage id, revision, and for `ThesisRevised` the `predecessor_thesis_id` and what the revision changed; instrument, asset class, direction, horizon, evidence and cited sources, corroboration kind, invalidation, conviction, confidence; the source-allowlist version; prompt and response (artifacts); `admitted` and the refusal reason from the ordered §8.5 checks; closed in §9.4 |
| `DecisionMade` | v1: man; v2: man, pol, reg | proposed action, combined conviction and combined score, outputs used, model weights, clips applied, gate dry-run result, autonomy classification and its source (`rule:<id>`, `default`, built-in, the admission ceiling, or the client ceiling), `delegation_id` when a delegation lifted it ([mandate spec §6.5](mandate.md#65-delegations-dec-181-adr-0003)), and `requested_by` (`agent`, `owner`, or `client`) with the client's id when a connected client asked (mandate §6.2 step 5a, DEC-185); `ask_suppressed` (`budget`, `skipped_today`, `recent_timeout`) when an `ask` was classified but not asked ([mandate spec §6.4](mandate.md#64-approvals)) |
| `IntentProposed` | man | intent fields (its `event_id` is the intent ID); after a grant, `causation_id` is the `ApprovalRevalidated` |
| `ApprovalRequested` | man | Closed at schema version 1: approval (its `event_id`), instrument, asset class, side, quantity, limit price, purpose, mandate version, `decided_by`, score, approver requirements, nullable reference mark, deadline, timeout/default, recursively closed content, and its canonical hash (§9.6) |
| `ApprovalDelivered` | man | Closed at schema version 1: approval, `cli_inbox` channel, delivery status (`delivered`, `suppressed_quiet_hours`, `failed`), and nullable message ID (§9.6) |
| `ApprovalResponded` | man | approval, verdict (`approved`, `skipped`; a legacy `denied` reads as `skipped`), responder (opaque) and role, result (`admitted`, `counted`, `refused`; a legacy `recorded` or `refused` reads as terminal), reason, effective time, step-up evidence (assertion ID, authentication time, method), separation-of-duties result, and for a grant that reaches check 7 the approver count and independence it applied (the stricter of the bound values and the policy overlay, [mandate spec §6.4](mandate.md#64-approvals)); `causation_id` is the `ApprovalResponseSubmitted`, copied at most once; the delegation shape chosen, if any, with the new mandate version and delegation id (mandate §6.4, §6.5) |
| `ApprovalRevalidated` | man | approval, result (`act`, `skip`), reason, and every value compared: bound and current mandate version, mode, instrument restriction, `decided_by` then and now, dry-run verdict and reason, `m_req`, `m_now`, `band_bp` |
| `ApprovalTimedOut`, `ApprovalCanceled` | man | approval, `on_timeout: skip`; approval, cancel reason (`version_applied`, `mode_tightened`, `owner_pause`, `owner_stop`, `kill_switch`; a legacy `rebound` is a cancellation for either of the first two) |
| `AgentModeChanged`, `KillSwitchActivated` | — | from, to, reason; scope and initiator; `AgentModeChanged` version 2 also records the owner's hold (§9.11) |
| `OwnerExitRequested` | man | instrument or scope, bid shown and confirmed, user (opaque), step-up evidence |
| `OwnerCommandRefused` | — | A resume or Stop the runtime refused for its step-up ([mandate spec §6.1](mandate.md#61-purposes)): the command (`resume`, `stop`), the reason (`step_up_missing`, `step_up_stale`, `step_up_reused`, `step_up_method`), and the effective time it was judged at; `causation_id` is the `OwnerCommandIssued`, copied at most once. The executor records a refused acknowledgment the same way on the account stream |

**Workspace control stream** (owner: workspace services)

| Event type | Required refs | Key payload fields |
|---|---|---|
| `MandateDraftSaved` | — | One explicit save of a mandate draft: draft, the draft stored (artifact), origin, the save it replaces, the version it started from; closed in §9.9 |
| `ModelInvocationRecorded` | mod | The compiler's model call, for a draft ([inference spec §3.6](inference.md#36-journaling)); closed in §9.9 |
| `MandateVersionCreated`, `MandateConfirmed` | — | per [mandate spec §10](mandate.md#10-records-dec-51-dec-97): source text (artifact), compiled fields, provenance per path with quoted spans, template, policy-set hashes, validation results and warnings, classification, diff; version hash, confirmed paths, rendered confirmation (artifact) and UI build, warnings acknowledged, step-up evidence, confirming user (opaque); `MandateConfirmed` version 2 also names the agent and the version it replaces (§9.9) |
| `AgentDeployed`, `DeploymentRejected`, `AgentStopped` | man | agent, mandate version, reason (`goal_complete`, `profit_stop_reached`, `end_date`, owner stop), net dollar loss added to the connection's loss carry; for `AgentDeployed`: the rendered go-live screen (artifact), backtest and paper-run IDs shown, performance legend and disclosure versions shown, approving users, step-up evidence ([mandate spec §10](mandate.md#10-records-dec-51-dec-97)) |
| `PolicyChanged`, `WorkspaceProfileAssigned` | — | level, diff, author (opaque), step-up evidence, affected agents; profile, basis, assigning user |
| `ConnectionEstablished`, `ConnectionRevoked` | — | broker, scopes granted, permission-check result; closed in §9.2, version 2 of `ConnectionEstablished` with `account_ref`, user, step-up, and the live margin attestation in §9.8 |
| `ConnectionRefused`, `ConnectionCredentialRotated` | — | A connect, reconnect, or credential replacement the permission checks refused, with the check and reason, or the teardown that ended it; a credential replaced on a connection that is not revoked; closed in §9.8 |
| `ClientConnected`, `ClientRevoked` | — | An owner-connected client issued or revoked ([identity spec §12.1](identity.md)): client, user, scopes, agents, step-up; closed in §9.10 |
| `DisclosureAccepted` | — | document and version hash, user (opaque), step-up evidence |
| `OwnerAlertSent` | — | Written by the owner of the subject event's stream, in that stream and in the subject's batch, on the agent, account, or control stream ([notifications spec §5.5](notifications.md#55-records)): subject event, kind (notifications spec §3.2), and for a kill switch the owner command it carries out, if any. It records that an alert was raised; delivery is the notice stream's. Closed in §9.15 |
| `NotificationAddressChanged` | — | A member set or removed one of their own push addresses ([workspace API spec §4.11, §5.7](workspace-api.md), [DEC-795](../project/decisions/DEC-795.md) item 5): member (opaque), channel (`web_push`), action (`added`, `removed`), `address_ref` (the `added` event's own id, listed in `pii_refs`), and step-up evidence (assertion ID, authentication time, method), `null` only for a `removed` that rides a deactivation's commit; written with its `OwnerAlertSent` (`notification_address_changed`) in one batch. The endpoint and keys stay in the vault. Payload schema closed with E8-14 |
| `OwnerAcknowledged` | — | user (opaque), the user who requested the lift (opaque) and the independence requirement as it stood when the lift was requested, carried so the executor applies the stricter of it and the overlay at processing ([mandate spec §5.8, §6.7](mandate.md#67-tripwires-dec-187-dec-350-dec-351)), authentication method, step-up evidence (assertion ID, authentication time, method) |
| `ApprovalResponseSubmitted` | — | The owner's answer to an approval ([mandate spec §6.4](mandate.md#64-approvals)): agent, approval, verdict (`approved`, `skipped`), content hash, `submitted_at`, step-up evidence (assertion ID, authentication time, method) or null, responder (opaque) and role |
| `OwnerRequestSubmitted` | — | An owner's request for an order (workspace API §4.6): agent, instrument, side, optional size, who asked; closed in §9.9 |
| `OwnerCommandIssued` | — | The owner's command ([mandate spec §6.1](mandate.md#61-purposes)): agent or kill-switch scope, command (`pause`, `resume`, `stop`, `kill_switch`, `owner_exit`, and `hold_openings` and `lift_hold`, closed in §9.11), the release choice and warning shown for a Stop with release, the bid, bid size, and floor confirmed for an owner exit, `submitted_at`, step-up evidence or null, user (opaque) |
| `ConfigSnapshotRegistered` | — | configuration kind (fee, calendar, instrument snapshot, rule set, mandate, model, policy set, model registry, broker profile), content hash |
| `SurveillanceReportGenerated`, `BacktestRunRecorded` | rule | period, report (artifact), breaches; data snapshot, code build, configuration, results, paper/live/backtest marker |
| `PlatformOperatorAction` | — | action (stop, global kill switch, acceptable-use action, `model_withdrawn` with model and reason, `research_thesis_halt` with the instrument and optionally the research agent's pinned content hash, [DEC-100](../project/04-decision-log.md#decisions)), operator (opaque), approval |
| `AnchorComputed`, `VerificationRun`, `IntegrityIncidentRecorded` | — | leaves, root, timestamp token (artifact), closed in §9.14; scope and result, closed in §9.13; last good hash and anchor |
| `SegmentExported`, `SegmentEvicted`, `RetentionExtended`, `LegalHoldChanged` | — | manifest hash, range, retain-until, hold. `SegmentExported` is closed in §9.14 |
| `KeyRotated`, `KeyRevoked`, `RecordsAccessed`, `ExportCreated`, `PersonalDataErased` | — | key version; accessor (opaque), scope; export manifest; subject reference. `RecordsAccessed` and `ExportCreated` are closed in §9.13 |
| `MemberInvited`, `MemberInvitationRevoked`, `MemberActivated`, `MemberRoleChanged`, `MemberDeactivated`, `MemberReactivated`, `MemberRemoved` | — | A workspace membership ([identity spec §5](identity.md#5-membership-lifecycle)); closed in §9.12 |
| `CredentialEnrolled`, `CredentialRemoved` | — | member, credential (opaque reference, never the key), kind, enrolment cool-off end ([identity spec §10.1, §12.1](identity.md#121-events-journal-9-control-stream)) |
| `SessionOpened` | — | member, session (opaque), method, device (opaque), `first_seen_device`; the subject of the notifications spec's `new_device` kind |
| `SessionRevoked` | — | member, session (opaque), reason (`sign_out`, `deactivated`, `deprovisioned`, `refresh_reuse`, `refresh_failed`, `expired`, `admin`, as [identity spec §12.1](identity.md#121-events-journal-9-control-stream) defines them); with `deprovisioned`, the subject of the notifications spec's `deprovisioned` kind |
| `ServiceAccountIssued`, `ServiceAccountRevoked` | — | account, scopes, workspaces, expiry, issuing user |
| `HostCliRegistered`, `HostCliRevoked` | — | registration (its ULID), host (opaque), operating-system account (opaque), registering admin, step-up evidence ([identity spec §6.4](identity.md#64-the-risk-reduction-path)) |
| `BreakGlassRequested`, `BreakGlassGranted`, `BreakGlassEnded` | — | operator (opaque), reason code, window, approvers ([identity spec §10.3](identity.md#103-break-glass-for-platform-staff-managed-mode)) |

The identity records ([identity spec §12.1](identity.md#121-events-journal-9-control-stream),
DEC-437 item 9) are control-stream records. The membership records close in §9.12. The client
records `ClientConnected` and `ClientRevoked` are closed in §9.10 with the `client` actor (§3), journal
spec v0.22 ([DEC-671](../project/decisions/DEC-671.md)). The
credential, session, service-account, host-CLI, and break-glass records are listed with the members
the identity spec names and close in their own change, as §9.2's other records do; until then
`append` refuses them as `unknown_event_type`. `ScopeHalted` and `ScopeReenabled` are not catalogued: they
exist only if DEC-437 item 21 (Proposed) is accepted (identity spec §4.4). Nor is
`NotificationAddressChanged`: its row and schema come with the notification records' change
([DEC-720](../project/decisions/DEC-720.md), [DEC-795](../project/decisions/DEC-795.md)).

**Scheduler stream:** `ClockAdvanced`, `TradingDayStarted`, `ClockOffsetRecorded`,
`ClockToleranceExceeded`.

**Notice stream** (owner: the workspace's notification dispatcher; [notifications spec §5.5](notifications.md#55-records))

| Event type | Required refs | Key payload fields |
|---|---|---|
| `StreamOpened` | — | stream type, workspace; closed in §9.15 |
| `NoticeIssued` | — | notice id (random, notifications spec §4.2), kind, class, cause (the `OwnerAlertSent`, `ApprovalRequested`, or (for `channel_lost`) `NoticeAttempted` it answers, with its stream), recipients (opaque); closed in §9.15 |
| `NoticeAttempted` | — | notice id, recipient (opaque), channel, attempt, status (`delivered`, `failed`, `suppressed_quiet_hours`, `deferred_quiet_hours`, `abandoned`), reason, provider message id, `coalesced_into`; closed in §9.15 |

### 9.1 Agent-stream payload schemas ([DEC-177](../project/04-decision-log.md#decisions))

This subsection closes the payload schemas of the agent stream's `StreamOpened`,
`ObservationRecorded`, `ModelOutputRecorded`, `DecisionMade`, `IntentProposed`, `AgentModeChanged`,
`KillSwitchActivated`, and `OwnerExitRequested`. For these events it replaces the "Key payload fields"
column of §9, and the required `config_refs` stay as §9 lists them. Each schema is `schema_version` 1:
a record with exactly the listed members, every one present (§4.2). The test vectors' `agent_stream`
section holds at least one chain event per schema, an invalid draft for every rule below, and valid
drafts for the cases a rule might be misread to refuse. The other agent-stream events are not
closed yet: the approval events that v0.5 added (§9, mandate spec §6.4) close in their own change,
and `ModelInvocationRecorded` in its own story. `ThesisProposed` and `ThesisRevised` are closed in
§9.4.
`OwnerCommandRefused` is closed in §9.2, on both streams that write it.

Journal spec v0.16 adds `schema_version` 2 for `ModelOutputRecorded` and `DecisionMade`. Each has
exactly its version-1 payload, with the stricter `config_refs` §9 lists. Rule 13a requires a
version-2 model output's `model_registry` object to contain exactly one entry whose `model_id`,
`model_version`, and `content_hash` equal the payload's. Version 1 remains registered and replayable;
a writer emits version 2 once it has the two new registered snapshots. The additive
`production_config_refs` vectors hold both version-2 drafts and the version-2 control records that
register their snapshots, without rewriting the historical version-1 chain.

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
| `decided_by` | `text?` | What decided `autonomy` (mandate spec §6.2), one label of: `builtin_risk_reducing` (step 3), `rule:<id>` or `default` (step 4), `delegation:<id>` (step 4a), `admission_ceiling` (step 5), `client_ceiling` (step 5a), `review_ceiling` (step 5b), `policy_overlay` (step 5c), with `<id>` an `id`; the same labels as the approval content's `decided_by` ([mandate spec §6.4](mandate.md#64-approvals)). `null` exactly when `autonomy` is: rule 5 |
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
   `payload.decided_by`; and a decision `decided_by` `policy_overlay` is never `auto` (the policy
   overlay, mandate spec §6.2 step 5c and §4.3, which only narrows an `auto` to an `ask` or denies
   an opening or increase under [DEC-534](../project/decisions/DEC-534.md)) — `payload.decided_by`.
   So the record itself refuses a connected client's opening that ran unasked, a decision that
   claims the review ceiling and ran unasked, and one that claims the policy overlay and ran
   unasked.
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
13a. `ModelOutputRecorded` version 2: the canonical object named by
    `config_refs.model_registry` contains exactly one model whose `model_id`, `model_version`, and
    `content_hash` equal the payload's — `payload.model_version`; otherwise
    `config_ref_mismatch`.

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
`MandateVersionApplied` (account stream), which close in §9.3, and
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
| `kind` | version 1: `fee_config` \| `trading_calendar` \| `settlement_calendar` \| `instrument_snapshot` \| `rule_set` \| `mandate_version` \| `model_version`; version 2 adds `policy_set` \| `model_registry`; version 3 adds `broker_profile` | §9's `config_refs` kinds. A signal model is `model_version`. Version 1's vocabulary stays closed |
| `content_hash` | `ref` | The snapshot. For a model, its content hash (mandate spec §8.1) |
| `model_id`, `model_version` | `text?` | A model's id and version, registered together with its hash (V-007): rule 21 |
| `params` | `[text]` | A model's declared parameters (V-007), empty for any other kind: rules 20 and 21 |
| `admits_instruments` | `boolean?` | Whether the model may admit instruments (mandate spec §8.4): rule 21 |

Journal spec v0.16 registers `ConfigSnapshotRegistered` schema version 2 with the same payload
members and consistency rules as version 1, plus the two new `kind` values. A writer uses version 2
for `policy_set` and `model_registry`; it may use either registered version for an older kind.
For either new kind, before a configuration reference is appended, its canonical object is stored
under `content_hash` and its object `kind` equals the registration payload's `kind`; older kinds
retain their existing object contracts. Rules 20 and 21 make the model-only members null or empty
for both new kinds.

Journal spec v0.19 registers `ConfigSnapshotRegistered` schema version 3 with the same payload
members and consistency rules as version 2, plus the `kind` value `broker_profile`. A writer uses
version 3 for `broker_profile`; it may use any registered version for an older kind. Rules 20 and 21
make the model-only members null or empty for it, and rule 21b binds its stored object.

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
21a. `ConfigSnapshotRegistered` version 2: `policy_set` and `model_registry` are valid kinds.
    The canonical object stored under `payload.content_hash` has a `kind` equal to
    `payload.kind`; absence is `missing_artifact` at `payload.content_hash`, and a kind mismatch is
    `config_ref_kind` at `payload.kind`. Version 1 refuses either new kind as `non_canonical` at
    `payload.kind`. §11 check 6 independently verifies that the stored bytes re-hash to their
    address and reports `artifact_mismatch`.
21b. `ConfigSnapshotRegistered` version 3: `broker_profile` is a valid kind. The canonical object
    stored under `payload.content_hash` has a `kind` equal to `payload.kind`; absence is
    `missing_artifact` at `payload.content_hash`, and a kind mismatch is `config_ref_kind` at
    `payload.kind`. Versions 1 and 2 refuse it as `non_canonical` at `payload.kind`. §11 check 6
    independently verifies that the stored bytes re-hash to their address and reports
    `artifact_mismatch`.
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
    the agent stream (`payload.command`), or at version 2 also `lift_hold`, which is registered on the
    agent stream only (§9.11). The executor refuses acknowledgments, and the runtime
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
connection. Which connection an account stream belongs to is journaled by version 2 of
`ConnectionEstablished` (§9.8, DEC-800, closing DEC-261 item 10); the mapping still takes it as an
argument from its owner, who reads it from that binding. `ValidationContext` takes
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

### 9.3 Account-stream risk-state records ([DEC-403](../project/decisions/DEC-403.md))

This subsection closes the payload schemas of the account stream's `MandateVersionApplied` and
`UniverseChanged`, the two records `ValidationContext::from_journal` reads that §9.2 left to their
own story ([DEC-303](../project/decisions/DEC-303.md) item 6). For these two events it replaces the
"Key payload fields" column of §9, as §9.1 and §9.2 do for theirs. §9.2's conventions apply unchanged:
§9.1's and §9.2's types, the absent-member rule, the report order, the required `config_refs` as §9
lists them, and the rules numbered on from §9.2's. Each schema is `schema_version` 1. Both are risk
inputs, so each carries `risk_clock` (§2).

**Types**, beyond §9.1's and §9.2's:

| Type | Values | Refused as |
|---|---|---|
| `asset_id` | Mandate spec §3's asset ID: `8-4-4-4-12` lowercase hexadecimal with hyphens, the form `AssetId` parses. Uppercase is refused, never folded, so one asset has one spelling ([DEC-404](../project/decisions/DEC-404.md) item 9) | Not a string: `schema`; otherwise `non_canonical` |

Every member comes from mandate spec §5.10's row, from a writer that exists, or from §2, and DEC-403
traces each one: `mandate-spec`'s risk fold writes `MandateVersionApplied`'s result, rejection
reason, allocation change, and floor fraction, and `mandate-research` writes `UniverseChanged`'s
instrument, change, reason, thesis and lineage, and size. The test vectors' `risk_state` section
holds base drafts of both records on the account stream, the stored mandate documents they name (each
a valid mandate, with every version record stating the classification mandate spec §9.2 gives its two
documents, and every admission through a thesis naming an unpinned mandate), the
`JournaledFact` each maps to, an invalid draft for every rule, and valid drafts for the cases a rule
might be misread to refuse. No payload carries a credential or personal data: an agent and a
thesis are named by opaque IDs, and step-up evidence by its assertion.

**`MandateVersionApplied`** on the account stream ([mandate spec §2.2, §5.10](mandate.md#22-applying-a-new-version)):
a version took effect for an agent, or was refused at application.

| Member | Type | Meaning |
|---|---|---|
| `agent_id` | `id` | The agent the version is for |
| `old_version` | `ref` | The version in force before |
| `new_version` | `ref` | The version applied, or refused |
| `classification` | `risk_increasing` \| `risk_reducing` \| `neutral` | Mandate spec §9.2's verdict of `new_version` against `old_version`. An invalid change never validates (V-031), so it never reaches application: rules 29 and 33 |
| `step_up` | `{assertion_id: text, authenticated_at: timestamp, method: text}?` | The owner's step-up evidence, as `DisclosureAccepted`'s: rule 29 |
| `result` | `applied` \| `rejected` | |
| `reason` | `increase_blocked_while_latched` \| `equity_below_exposure` \| `would_trigger_limit` \| `not_loosening` \| `waiting_period` \| `still_below_new_floor`, or `null` | Why it was refused (mandate spec §5.1, §5.7): rule 30 |
| `allocation_change` | `decimal?` | The signed dollar change in the allocation it applied (mandate spec §5.1): rule 30 |
| `max_loss_from_allocation` | `decimal?` | The fraction a floor-loosening version raised the floor to (mandate spec §5.7): rule 30 |
| `risk_clock` | `risk_clock` | The latest tick the executor had seen (§2) |

**`UniverseChanged`** on the account stream ([mandate spec §2.3, §8.5](mandate.md#23-the-working-universe-at-runtime-dec-97)):
an instrument was admitted to, or removed from, an agent's working universe.

| Member | Type | Meaning |
|---|---|---|
| `agent_id` | `id` | The agent whose working universe changed |
| `instrument` | `asset_id` | The instrument's asset ID (mandate spec §3) |
| `change` | `admitted` \| `removed` | Rule 31 |
| `reason` | `thesis_admitted` \| `thesis_expired` \| `thesis_invalidated` \| `lineage_retired` \| `eligibility_lost` \| `operator_halt` \| `version_applied` | Mandate spec §5.10's reasons: rule 31 |
| `thesis_id`, `lineage_id` | `id?` | The thesis the change follows from, and its lineage: rule 32 |
| `universe_size_after` | `integer` | The working universe's size after the change |
| `risk_clock` | `risk_clock` | The latest tick the executor had seen (§2) |

**Consistency rules** (reason `schema`; the path is the member named):

29. `MandateVersionApplied`: `step_up` is non-null when `classification` is `risk_increasing`
    (`payload.step_up`). Mandate spec §9.2: a risk-increasing version requires step-up.
30. `MandateVersionApplied`: `reason` is non-null exactly when `result` is `rejected`, and
    `allocation_change` and `max_loss_from_allocation` are `null` when it is. Reported at the first
    offending member, in that order. A refused version changes nothing.
31. `UniverseChanged`: `thesis_admitted` only admits; `thesis_expired`, `thesis_invalidated`,
    `lineage_retired`, `eligibility_lost`, and `operator_halt` only remove; `version_applied` does
    either (`payload.reason`). Mandate spec §8.5 admits only through a thesis or a version, and
    removes for the rest.
32. `UniverseChanged`: `thesis_id` and `lineage_id` are `null` together, reported at the one that is
    `null` (as rule 13). They are non-null for `thesis_admitted`, `thesis_expired`,
    `thesis_invalidated`, `lineage_retired`, and `operator_halt`, which each follow from a thesis
    (an operator halts per thesis, DEC-100), and `null` for an admission with `version_applied`,
    which follows from a pinned list and no thesis (`payload.thesis_id`). A removal with
    `version_applied` may name its thesis or not: DEC-121's pinning switch removes instruments the
    research agent had admitted through a thesis (mandate spec §9.2), and a pinned list's own
    removal has none. `eligibility_lost` may be either too: a pinned instrument can lose
    eligibility.
33. `MandateVersionApplied`: `classification` is `risk_increasing` when the record itself shows a
    risk-increasing change: an `applied` one with `allocation_change` greater than 0 or with a
    non-null `max_loss_from_allocation` (mandate spec §9.2's maximums row), or a `rejected` one with
    `reason` `increase_blocked_while_latched`, `waiting_period`, or `still_below_new_floor`, which
    the risk fold reaches only on an allocation increase or a floor raise (mandate spec §5.1, §5.7)
    (`payload.classification`). It reports after rules 29 and 30, so a record whose `step_up` is
    missing because its classification is wrong is reported here, at the classification. With rule
    29 it keeps a risk-increasing version from being journaled as applied without step-up.

**The mapping to `JournaledFact`**, as §9.2's:

| Record | Fact | From |
|---|---|---|
| `MandateVersionApplied` with `result` `applied` | `AgentVersionActive` | `agent_id`; `connection_id`, `environment`, `capital.allocation_usd`, and `universe.pinned_instruments[].asset_id` of the stored document `new_version` names, as for `AgentDeployed`. Applied or rejected, `old_version` and then `new_version` must each name a stored document that hashes to it; one that does not is refused, as malformed (`InvalidInput`) at that member: the context cannot be built, and nothing is skipped. Whatever its `result`, a record whose `classification` is not mandate spec §9.2's verdict of its two stored documents is refused the same way, at `classification` ([DEC-404](../project/decisions/DEC-404.md)) |
| `MandateVersionApplied` with `result` `rejected` | none | A refused version changes nothing: the record yields no fact and no error, once both documents are stored and its classification matches them |
| `UniverseChanged` | `UniverseChanged` | `agent_id`, `instrument`, and whether `change` is `admitted` |

### 9.4 Research-agent thesis records ([DEC-413](../project/decisions/DEC-413.md))

This subsection closes the payload schemas of the agent stream's `ThesisProposed` and
`ThesisRevised`, which §9.1 left to their own story. For these two events it replaces the "Key
payload fields" column of §9, as §9.1 to §9.3 do for theirs. §9.2's conventions apply unchanged:
§9.1's types and §9.3's `asset_id`, the absent-member rule, the report order, the required
`config_refs` as §9 lists them (`mandate_version` and `model_version`), and the rules numbered on
from §9.3's. Both events share
one schema, `schema_version` 1. The entry type follows the revision number, not the verdict, so an
ignored revision is still a `ThesisRevised` (rule 34).

A record is written for every thesis the admission judges, whether it is admitted, refused, or
ignored ([mandate spec §8.5](mandate.md#85-admission-and-removal-dec-97-dec-101-dec-103)): a refusal
admits nothing and is journaled, and an output the first three checks refuse is ignored and journaled
with its reason (mandate spec §8.2). So no rule refuses a record that a correct writer makes for a
thesis the admission judged, and the rules recompute from the record only the checks the record
itself decides: checks 1 to 3, and check 15 from the platform-derived corroboration (rules 36 and
37). The other checks read the mandate, the policy
overlay, the working universe, and facts no record carries, so they are the admission's, not this
schema's.

Every member comes from mandate spec §8.2's output fields, §8.4's thesis table and its
`ThesisProposed` sentence, or §9's row, and DEC-413 traces each one to `mandate-research`'s
`ThesisEntry` or to the writer that adds it. The test vectors' `research` section holds base drafts
of both records on a research agent's stream, the artifacts they name (the research agent's mandate,
which is the mandate reference cases' `research_equity` with its admitting model's content hash set
to the stored model content, and each thesis's prompt, response, evidence, and autopsy), an invalid
draft for every rule and member type, and valid drafts for the cases a rule might be misread to
refuse. Every thesis record the approved mandate reference cases journal passes these rules, and the
section lists them. The vectors also pin the report order between rule 34, its autopsy clause, rules
35 to 37, and rule 38. No payload carries a credential or a personal-data field, and the prompt,
response, evidence, and autopsy are carried only by artifact. Two members are the model's own text in
the payload: `invalidation`, and the cited `evidence_sources`.

**`ThesisProposed`, `ThesisRevised`** on the agent stream ([mandate spec §8.4, §8.5, §8.6](mandate.md#84-the-research-agent-dec-97-adr-0002)):
one thesis the research agent proposed, and the admission's verdict on it.

| Member | Type | Meaning |
|---|---|---|
| `model_id`, `model_version` | `text` | The research agent's id and version, as its output states them |
| `content_hash` | `ref` | The research agent's content hash ([mandate spec §8.1](mandate.md#81-signal-model-contract-dec-52-dec-97)); the content object is an artifact: rule 38 |
| `thesis_id`, `lineage_id` | `id` | The thesis and its lineage ([mandate spec §8.6](mandate.md#86-thesis-lifetime-and-revision-lineages-dec-118-dec-111)) |
| `revision` | `integer` | 0 for a first thesis: rules 34 and 36 |
| `predecessor_thesis_id` | `id?` | The thesis this one revises, as the output states it: rule 36 |
| `autopsy_ref` | `ref?` | What the autopsy of the predecessor's failure found and what this revision changes, as an artifact ([DEC-111](../project/04-decision-log.md#decisions)): rule 34 |
| `instrument_id` | `asset_id` | The instrument's asset ID (mandate spec §3, §9.3's type), resolved by the platform |
| `asset_class` | `us_equity` \| `crypto` | From instrument reference data, never the thesis's claim |
| `direction` | `text` | As given; a direction v1 does not allow is recorded and ignored: rule 36 |
| `as_of`, `expires_at` | `timestamp` | Mandate spec §8.2's data cut-off and expiry: rule 36 |
| `horizon_s` | `integer` | Seconds: rule 36 |
| `conviction`, `confidence` | `decimal` | As given |
| `evidence_ref` | `ref?` | The evidence, as an artifact |
| `evidence_sources` | `[text]` | The sources cited, in the order given, duplicates kept. Text, not `id`, and in no required order, because check 14 refuses a source off the allowlist and the record keeps what was cited (mandate case MC-N07 cites `src.filings` before `src.anonymous_blog`) |
| `corroboration` | `independent_source` \| `market_data`, or `null` | How the platform corroborated the thesis, never what the model asserted; `null` when it did not: rule 37 |
| `invalidation` | `text` | The conditions that end the thesis before its horizon, as the model gave them |
| `allowlist_version` | `integer` | The source allowlist's version in effect ([DEC-101](../project/04-decision-log.md#decisions)) |
| `prompt_ref`, `response_ref` | `ref` | The research agent's prompt and response, as artifacts |
| `admitted` | `boolean` | Whether the admission admitted or renewed the instrument: rule 35 |
| `reason` | one of mandate spec §8.5's seventeen reasons (`direction_not_allowed` to `universe_full`), or `null` | The first failing check: rules 35 to 37 |

**Consistency rules** (reason `schema` unless stated; the path is the member named):

34. `ThesisProposed` has `revision` 0 and `ThesisRevised` a `revision` above 0 (`payload.revision`);
    and `autopsy_ref` is `null` on `ThesisProposed` (`payload.autopsy_ref`). Mandate spec §8.4:
    "a revision is `ThesisRevised`". No rule requires the autopsy on a revision, so a revision whose
    output omits it is still recorded (DEC-413 item 5).
35. `reason` is `null` exactly when `admitted` is `true` (`payload.reason`). Mandate spec §8.5: the
    first failure decides and is the journaled reason, and a success has none.
36. Checks 1 to 3 recomputed from the record (`payload.reason`): check 1 fails when `direction` is
    not `long`, check 2 when `expires_at` is not `as_of` plus `horizon_s` seconds (compared exactly,
    to the nanosecond), and check 3 when `predecessor_thesis_id` is non-null and `revision` is 0, or
    `null` and `revision` above 0. When one fails, `reason` is the first that fails, in that order;
    when none fails, `reason` is none of the three.
37. Check 15 recomputed from the record (`payload.reason`): when `corroboration` is `null`, the
    thesis fails check 15, so `reason` is a check numbered 15 or lower and never `null`,
    `lineage_retired`, or `universe_full`; when it is non-null, `reason` is not `no_corroboration`.
38. `config_refs.model_version` equals `content_hash` (`payload.content_hash`), as rule 22 binds
    `AgentDeployed`'s mandate version. A record whose `model_version` ref is missing is reported by
    §9's required references, not here.

**No mapping to `JournaledFact`.** No fold reads these records yet: the risk-state fold reads the
account stream's `UniverseChanged` (§9.3), which the executor writes with its `causation_id` naming
the agent stream's `ThesisProposed` or `ThesisRevised`. The lineage fold's reading of these records
(mandate spec §8.6) follows with its own story.

### 9.5 Account-stream executor records ([DEC-446](../project/decisions/DEC-446.md), [DEC-447](../project/decisions/DEC-447.md))

This subsection closes the payload schemas of the account stream's `IntentReceived`,
`GateDecided`, and `OrderSubmitted` at `schema_version` 2, and of the account stream's
`OrderRequestRecorded` and `ProtectionChanged` at `schema_version` 1
([DEC-360](../project/decisions/DEC-360.md), option (c)). For these events it replaces the "Key
payload fields" column of §9, as §9.1 to §9.4 do for theirs. §9.2's conventions apply unchanged:
§9.1's types and §9.2's, the absent-member rule, the report order, the required `config_refs` as
§9 lists them, and the rules numbered on from §9.4's.

The three version-2 schemas are their version 1's members with `risk_clock` last and nothing else
moved: version 1 is never edited (§8), stays registered, and keeps replaying as it always has,
pinned byte for byte by the version-3 vectors. Each version-2 record carries `risk_clock` because
§9.2 says the executor writes it on every account-stream event and folds each at it, and the fold
requires it (DEC-302 item 1). `StreamOpened` carries none — at seq 1 the stream has seen no tick —
and `OwnerCommandRefused` none, whose schema is shared with the agent stream
([DEC-447](../project/decisions/DEC-447.md)). `FillApplied` and `MarkUpdated` carry it already at
version 1 (DEC-81); the rest of the account catalogue closes with its own stories. At either
version the protective prices are never members of `IntentReceived`
([DEC-446](../project/decisions/DEC-446.md) item 5): they move to the `intended`
`ProtectionChanged`, journaled beside the intent in its own batch, so the copy of the proposal
stays exact and replay restores the protection the opening was handed in with (`AGENTS.md` rule
13).

`OrderRequestRecorded` is the companion DEC-360 option (c) rules for: journaled immediately
before its `OrderSubmitted` in the same `append` batch, named by its `causation_id` (rule 45). It
carries what the fold rebuilds the exact request and the order's owner from, so a resubmission
after a confirmed absence re-sends the same body under the same id (trading spec §5.7), and the
restrictions and the exit rules read the same agent and purpose after a restart. One batch writes
both, so a crash cannot leave one without the other (§5.1: a batch is refused whole).

`ProtectionChanged` is closed whole. A schema closes per record, not per action, and the fold
reads members across every action, so the `intended` action cannot close alone. The members are
the fold's reads, traced per member in [DEC-446](../project/decisions/DEC-446.md) item 6; no
member is inferred. Every member is present on every record (§4.2, §9.1's absent-member rule);
rules 41 to 44 say per action which are null or empty. A legacy record — written while the schema
was open, with `instrument` for `instrument_id`, `agent` for `agent_id`, or `orders` as space- or
comma-joined text — stays replayable: the fold's reads are unchanged, tolerance of name and form
included, and `append` is what tightens to the closed form from here on.

The test vectors' `account_stream` section holds a hash-chained account stream from
`StreamOpened` at seq 1, with at least one chain event per closed schema; the `intended` record
in its intent's batch; the companion and its order in the next; a `placed` and an
`unprotected_start` for replay; the fold oracle (`fold_oracle`) the chain replays to — the
restored protection and the request the companion rebuilds; an invalid draft for every rule and
each member-typing case; valid drafts for the cases a rule might be misread to refuse; and rule
45's valid and invalid batches. The vectors stay version 3.

**`IntentReceived`** on the account stream at `schema_version` 2: §9's nine members, as the
version-1 schema registers them (DEC-174 item 5), and `risk_clock`.

| Member | Type | Meaning |
|---|---|---|
| `intent_id` | `ulid` | The intent: the agent stream's `IntentProposed` event (§2) |
| `agent_id` | `id` | The agent the intent is for |
| `instrument_id` | `text` | |
| `side` | `text` | As proposed |
| `type` | `text` | `limit`: an intent's body carries a limit and nothing else (`AGENTS.md` rule 12) |
| `tif` | `text` | As proposed, never the submission's (DEC-389 item 3) |
| `qty` | `decimal` | |
| `limit_price` | `decimal?` | |
| `purpose` | `text` | As proposed |
| `risk_clock` | `risk_clock` | The latest tick the executor had seen (§2) |

**`GateDecided`** on the account stream at `schema_version` 2: the version-1 members and
`risk_clock`. The check id vocabulary is §9's, matching trading spec §9.1.

| Member | Type | Meaning |
|---|---|---|
| `intent_id` | `ulid` | The intent decided |
| `verdict` | `text` | `allow`, `deny`, `hold`, or `defer` |
| `reason_code` | `text?` | The gate's reason code (trading spec §9.1); `null` on `allow` |
| `data_profile` | `text` | The data profile the decision read |
| `quotes_used` | `[{instrument_id: text, bid: decimal, ask: decimal, as_of: timestamp, feed: text}]` | |
| `marks_used` | `[{instrument_id: text, price: decimal, source: text, kind: text}]` | |
| `checks` | `[{id: §9's check ids, result: text, inputs: {}, computed: {}}]` | Each check run, in trading spec §9.1's order |
| `risk_clock` | `risk_clock` | The latest tick the executor had seen (§2) |

**`OrderSubmitted`** on the account stream at `schema_version` 2: the version-1 members and
`risk_clock`. Its `causation_id` names its `OrderRequestRecorded` (rule 45).

| Member | Type | Meaning |
|---|---|---|
| `client_order_id` | `text` | The broker's name for the order, derived from the intent (trading spec §2.3) |
| `attempt` | `integer` | The submission attempt under this id; 1 for the first |
| `instrument_id` | `text` | |
| `side` | `text` | |
| `type` | `text` | The order type submitted |
| `tif` | `text` | The time in force submitted |
| `qty` | `decimal` | |
| `limit_price` | `decimal?` | `null` for a market order |
| `risk_clock` | `risk_clock` | The latest tick the executor had seen (§2) |

**`OrderRequestRecorded`** on the account stream at `schema_version` 1: the executor-only members
of the order that follows it (DEC-446 item 2).

| Member | Type | Meaning |
|---|---|---|
| `agent_id` | `id` | The order's owner: the restrictions and the exit rules read it |
| `intent_id` | `ulid`? | The intent the order is for, when it is for one |
| `purpose` | `open` \| `increase` \| `discretionary_exit` \| `risk_exit` \| `owner_exit` \| `protective` \| `flatten` | The order's purpose (trading-domain spec §6.1) |
| `extended_hours` | `boolean` | Whether the order may trade outside the regular session |
| `stop_price` | `decimal?` | A protective or stop order's trigger |
| `order_class` | `bracket` \| `oco`, or `null` | The protective legs' class: rule 39 |
| `take_profit` | `decimal?` | The take-profit leg: rule 39 |
| `stop` | `decimal?` | The stop leg: rule 39 |
| `rung` | `integer?` | The exit price ladder's rung, from 0; non-null exactly for a ladder's submission (trading spec §5.6): rule 40 |
| `at_floor` | `boolean?` | Whether the rung is the ladder's floor: rule 40 |
| `risk_clock` | `risk_clock` | The latest tick the executor had seen (§2) |

**Consistency rules** for the companion (reason `schema`; the path is the member named):

39. `order_class` is null exactly when `take_profit` is, and `take_profit` null exactly when
    `stop` is, reported at the first offending member in the order `order_class`, `take_profit`,
    `stop`. The bracket's and the OCO's legs travel together: the fold rebuilds a class from both
    legs or from neither.
40. `at_floor` is null exactly when `rung` is (`payload.at_floor`). A ladder's rung carries its
    floor flag with it; there is no `laddered` member ([DEC-446](../project/decisions/DEC-446.md)
    item 2): `rung` non-null says a ladder rung, and a lone ladder's first rung writes `rung` 0.

**`ProtectionChanged`** on the account stream at `schema_version` 1: §5.4's protective orders and
unprotected intervals, and the receipt-time protective prices of `intended` (DEC-446 item 4).

| Member | Type | Meaning |
|---|---|---|
| `instrument_id` | `text` | |
| `action` | `intended` \| `placed` \| `cancelled` \| `passive_start` \| `unprotected_start` \| `watchdog` \| `exit_unpriced` \| `ladder_floor` \| `expiry_unreplaceable` \| `rung_short` \| `interval_limit` \| `unprotected_end` | What changed: rule 41 on |
| `orders` | `[text]` | The protective orders the action names, as client order ids: rule 41 |
| `awaiting` | `[text]` | The orders an interval still awaits the confirmed cancel of: rule 41 |
| `qty` | `decimal?` | `placed`: the quantity the orders cover; `cancelled`: the quantity they uncovered, where journaled: rule 42 |
| `stop` | `decimal?` | The protective stop: rule 43 |
| `take_profit` | `decimal?` | The protective take-profit, `null` where the sequence has none (crypto's): rule 43 |
| `intent_id` | `ulid`? | `intended`: the intent the prices came in with; `rung_short` and the starts: the exit's intent: rule 44 |
| `bracket` | `text?` | The entry a bracket action names: rule 44 |
| `entry` | `text?` | The exit sequence's entry: rule 44 |
| `agent_id` | `id?` | The exit sequence's owner: rule 44 |
| `replacing` | `boolean?` | The start replaces resting protection: rule 44 |
| `created_on` | `date?` | `placed`: the day the resting orders were created: rule 44 |
| `sent` | `decimal?` | `rung_short`: the quantity the short rung sent: rule 44 |
| `uncovered` | `boolean?` | The interval ends with the position uncovered: rule 44 |
| `acknowledged` | `boolean?` | `unprotected_end`: the owner acknowledged the interval: rule 44 |
| `risk_clock` | `risk_clock` | The latest tick the executor had seen (§2) |

**Consistency rules** for `ProtectionChanged` (reason `schema`; the path is the member named):

41. `orders` is empty except on `placed`, `cancelled`, `passive_start`, and `unprotected_start`,
    where it names at least one order (`payload.orders`); `awaiting` is empty except on
    `interval_limit` and `unprotected_end` (`payload.awaiting`).
42. `qty` is non-null on `placed` — the record's covered quantity — and null on every action but
    `placed` and `cancelled` (`payload.qty`).
43. `stop` and `take_profit` are null on every action but `intended`, `placed`, `passive_start`,
    and `unprotected_start`, reported at `payload.stop`; and `take_profit` is null when `stop`
    is, reported at `payload.take_profit`.
44. The remaining members are null except where the fold reads them, each reported at its own
    member in the table's order, from `intent_id` to `acknowledged`: `intent_id` is non-null on
    `intended` and `rung_short`, and on `passive_start` and `unprotected_start` `intent_id`,
    `entry`, and `agent_id` are null together or non-null together — an exit sequence names all
    three — reported at the first that disagrees, in the order `intent_id`, `entry`, `agent_id`;
    `replacing` may be non-null only on those two starts; `bracket` may be non-null only on
    `placed`, the two starts, and `unprotected_end`; `created_on` only on `placed`; `sent` only
    on `rung_short`; `uncovered` only on `interval_limit` and `unprotected_end`; and
    `acknowledged` only on `unprotected_end`.

**Batch rule** (reason `schema`):

45. An `OrderSubmitted` at `schema_version` 2 is immediately preceded in its `append` batch by
    exactly one `OrderRequestRecorded`, and names it as its `causation_id`. A submission with no
    companion immediately before it, or with more than one `OrderRequestRecorded` in the run of
    records immediately before it, is reported at `event_type`; one whose `causation_id` does not
    name that companion is reported at `causation_id`. An `OrderRequestRecorded` is then reported
    at its own `event_type` when no `OrderSubmitted` in the batch names it. Checked draft by
    draft first, in §9.1's order, then on the submissions in batch order, then the orphans. A
    version-1 `OrderSubmitted` carries no rule here: it precedes the companion (DEC-446 item 1).
    No §11 check repeats rule 45: the pairing sits inside one batch, which a range never splits
    (DEC-446 item 3).

**No mapping to `JournaledFact`.** None of these records is a `ValidationContext` input: §9.2's
mapping covers the context's reads alone (DEC-446 item 7), and the executor's own fold reads
these directly.

### 9.6 Approval and reconciliation records ([DEC-460](../project/decisions/DEC-460.md))

These version-1 records are closed recursively: every listed member is present, `null` is used
only where the type is nullable, and any extra member is refused.

**`ApprovalRequested`** on the agent stream:

| Member | Type |
|---|---|
| `instrument` | `text` |
| `asset_class` | `us_equity` \| `crypto` |
| `side` | `buy` |
| `qty`, `limit` | `decimal` |
| `purpose` | `open` \| `increase` |
| `mandate_version` | `ref` |
| `decided_by` | `text` |
| `combined_score` | `decimal` |
| `reference_mark` | `{price: decimal, seq: integer}?` |
| `approvers_required` | `integer` |
| `independent_required` | `boolean` |
| `deadline` | `integer` risk-clock seconds |
| `timeout_s` | `integer` |
| `on_timeout` | `skip` |
| `content` | The closed object below |
| `content_hash` | `ref` |

`content` has exactly:

- `action`: `{instrument: text, asset_class: us_equity | crypto, side: buy, qty: decimal,
  limit: decimal, order_usd: decimal, purpose: open | increase}`;
- `trigger`: `{mandate_version: ref, decided_by: text}`;
- `evidence`: `{combined_score: {label: text, value: decimal}, outputs:
  [{event_id: ulid, artifact: ref?, label: Output of software you selected | platform-authored}]}`;
- `risk_impact`: `[{field: order_usd | position_usd_after | gross_usd_after | bought_today_usd |
  drawdown | daily_pnl_fraction, value: decimal, cap: decimal?}]`;
- `reference_mark: {price: decimal, seq: integer}?`, `deadline: risk_clock`,
  `default: text`, `choices: [approve | skip]`, and
  `approvers: {required: integer, independent: boolean}`.

The score label is exactly `combined model score, not a probability of profit`; `default` is
exactly `If you do nothing, this action is skipped`; and `choices` is exactly
`["approve", "skip"]`. Every top-level action, trigger, score, reference-mark, deadline, and
approver member equals its nested counterpart; the top-level integer deadline and nested
whole-second timestamp name the same instant. `content_hash` equals the `sha256:` reference of the
canonical `content` bytes.

**`ApprovalDelivered`** on the agent stream:

| Member | Type |
|---|---|
| `approval` | `ulid` |
| `channel` | `cli_inbox` |
| `status` | `delivered` \| `suppressed_quiet_hours` \| `failed` |
| `message_id` | `text?` |

**`BrokerPositionObserved`** on the account stream:

| Member | Type |
|---|---|
| `instrument` | `text` |
| `broker_qty`, `model_qty` | `decimal` |
| `mismatch` | `boolean` |
| `risk_clock` | `risk_clock` |

**`AgentModeApplied`** on the account stream:

| Member | Type |
|---|---|
| `agent` | `text` (an agent id or the executor's `*` all-agent sentinel) |
| `to` | `normal` \| `exits_only` \| `paused` \| `stopped` |
| `restriction` | `text` |
| `originated` | `boolean` |
| `risk_clock` | `risk_clock` |

**`CompensatingEvent`** on the account stream:

| Member | Type |
|---|---|
| `subject` | `text` |
| `difference` | `order_state` |
| `from`, `to` | `intent` \| `submitting` \| `accepted` \| `partially_filled` \| `pending_cancel` \| `pending_replace` \| `unknown` \| `filled` \| `canceled` \| `rejected` \| `expired` \| `replaced` \| `abandoned` |
| `corrected_event_ids` | `[ulid]` |
| `risk_clock` | `risk_clock` |

### 9.7 Approval answers ([DEC-533](../project/decisions/DEC-533.md))

The owner's answer to an approval and the runtime's two records of it ([mandate spec
§6.4](mandate.md#64-approvals)), closed at schema version 1 as §9.6's records are: every listed
member is present, `null` only where the type is nullable, and any extra member is refused. §9.1's
types apply, with `null`, a member that is always `null` at this version. The rules number on from
§9.5's. The times are **integer risk-clock seconds**, as `ApprovalRequested.deadline` is and as the
CLI and the runtime write them ([DEC-257](../project/04-decision-log.md#decisions) item 5); a
`risk_clock` timestamp is refused there. §9.2's and §9.3's step-up evidence types `authenticated_at`
as a §4.7 timestamp instead; the two are unified only by a later version (DEC-533 item 2).

**`ApprovalResponseSubmitted`** on the control stream. It names no configuration.

| Member | Type | Meaning |
|---|---|---|
| `agent` | `id` | The agent whose approval it answers |
| `approval` | `ulid` | The approval: its `ApprovalRequested`'s `event_id` |
| `verdict` | `approved` \| `skipped` | |
| `content_hash` | `ref` | The content hash the owner was shown, repeated (mandate spec §6.4 check 5) |
| `submitted_at` | `integer` | The risk-clock second the owner answered at |
| `step_up` | `{assertion_id: text, authenticated_at: integer, method: text}?` | The step-up evidence, or `null`; the runtime judges it (check 6) |
| `responder` | `text` | The owner who answered (opaque): rule 46 |
| `role` | `approver` | |

**`ApprovalResponded`** on the agent stream: the runtime's copy of one `ApprovalResponseSubmitted`,
which is its `causation_id` (rule 50), with the result admission gave it.

| Member | Type | Meaning |
|---|---|---|
| `approval`, `verdict`, `responder`, `role` | As `ApprovalResponseSubmitted`'s | Copied |
| `result` | `admitted` \| `counted` \| `refused` | |
| `reason` | `not_pending` \| `late` \| `not_an_approver` \| `not_delivered` \| `content_mismatch` \| `step_up_missing` \| `step_up_stale` \| `step_up_reused` \| `step_up_method` \| `duplicate_approver` \| `not_independent` (nullable) | The first admission check that failed: rules 47 and 49 |
| `effective_at` | `integer` | The risk-clock second the response was judged at |
| `step_up` | As `ApprovalResponseSubmitted`'s | Copied |
| `quorum` | `{independent: boolean, required: integer}?` | The approver count and independence check 7 applied, the stricter of the bound values and the policy overlay: rule 48 |
| `separation_of_duties` | `null` | The separation-of-duties result. Always `null` at this version; E8's delegation story gives it a type at a later one |
| `delegation` | `null` | The delegation shape chosen, with its new mandate version and delegation id (mandate spec §6.5). Always `null` at this version, as `separation_of_duties` |

**`ApprovalRevalidated`** on the agent stream: re-validation of an admitted grant (checks 8 to 12),
with every value compared.

| Member | Type | Meaning |
|---|---|---|
| `approval` | `ulid` | |
| `result` | `act` \| `skip` | |
| `reason` | `text?` | The first failing check's reason, or the gate's reason code (check 11): rule 51 |
| `mandate_version_bound`, `mandate_version_now` | `ref` | Check 8 |
| `mode` | `normal` \| `exits_only` \| `paused` \| `stopped` | The effective mode (check 9) |
| `instrument_restricted` | `boolean` | Check 9 |
| `decided_by_bound` | `text` | The bound trigger's label (check 10) |
| `decided_by_now` | `text?` | The re-classification's `ask` label, or `null` when it is not an `ask`, as an empty label is (§4.2) |
| `dry_run` | `allow` \| `deny` | The gate dry run (check 11) |
| `dry_run_reason` | `text?` | The gate's reason code: rule 52 |
| `m_req`, `m_now` | `decimal?` | The reference mark at the request and the latest folded mark (check 12) |
| `band_bp` | `integer` | The drift band: rule 53 |

**Consistency rules** (reason `schema` unless stated; the path is the member named):

46. `ApprovalResponseSubmitted`: `responder` equals the envelope's `actor.id`
    (`payload.responder`), so the answer names the identity that wrote it.
47. `ApprovalResponded`: `reason` is non-null exactly when `result` is `refused`
    (`payload.reason`).
48. `ApprovalResponded`: `quorum` is non-null exactly when check 7 was judged: `verdict` is
    `approved` and either `result` is `admitted` or `counted`, or `reason` is
    `duplicate_approver` or `not_independent` (`payload.quorum`).
49. `ApprovalResponded`: a `skipped` verdict runs checks 1 to 5 only. It is never `counted`
    (`payload.result`), and when refused its reason is one of `not_pending`, `late`,
    `not_an_approver`, `not_delivered`, and `content_mismatch` (`payload.reason`).
50. `ApprovalResponded`: `causation_id` is non-null: the `ApprovalResponseSubmitted` it copies
    (`causation_id`).
51. `ApprovalRevalidated`: `reason` is non-null exactly when `result` is `skip`
    (`payload.reason`).
52. `ApprovalRevalidated`: `dry_run_reason` is non-null exactly when `dry_run` is `deny`
    (`payload.dry_run_reason`).
53. `ApprovalRevalidated`: `band_bp` is 100 or 200 (`payload.band_bp`), and an `act` passed every
    check it records. Its members are checked in this order, and the first that shows a failure is
    reported, at its own path:
    `mandate_version_now` equals `mandate_version_bound`; `mode` is `normal`;
    `instrument_restricted` is false; `decided_by_now` is `null` or equals `decided_by_bound`
    (check 10: an `auto` passes, a different `ask` skips); `dry_run` is `allow`; `m_req` and
    `m_now` are non-null, each
    reported at itself; and |`m_now` − `m_req`| × 10 000 ≤ `band_bp` × `m_req`, on exact decimals
    (`payload.m_now`).

### 9.8 Connection records ([DEC-800](../project/decisions/DEC-800.md))

A connection's history, from connect to revocation ([connections spec §3, §8.1,
§9](connections.md), CN-10), closed at schema version 1 as §9.7's records are, except
`ConnectionEstablished`, which gains version 2. §9.1's types and report order apply, with §9.2's
`risk_clock`, and the rules number on from §9.7's. No member is a credential, a broker account
number, or the account fingerprint (CN-1, connections spec §3.1): a draft carrying one has no member
to sit in and is refused as `schema`. No `ConnectionEstablished` version 1 exists outside the
paper fixtures (CN-12): the first writer of a connection is E7-17's, and it writes version 2.

**Who writes what.** The workspace control services write what happens at the owner's request on
the control stream: a connect, a reconnect, a credential the owner replaces, and each one refused.
The account's executor writes what it finds on the account stream: its permission checks, the
connection's state, and a token it refreshes. It also copies the two control-stream records that
change what it may do, as §2 copies owner input: the version-2 `ConnectionEstablished` that binds
its stream, and each `ConnectionCredentialRotated`. Each account-stream record carries
`risk_clock`, as the executor writes it on every event (§2), and the `connection_id` it acts for,
since the stream exists before the connection is established.

**The connect sequence** (connections spec §5.2). The connection manager assigns the connection's
`connection_id` and `account_ref` when the connect starts. For an OAuth connect, the token-exchange
process (connections spec §5.2 step 4, DEC-821 item 2) redeems the code and stores the token in the
vault; it appends nothing to the journal. The connecting executor, started only once a credential is
stored, opens the account stream `acct:{workspace_id}:{account_ref}` with `StreamOpened`, runs §8.1
checks 1, 2, 3, and 7 against the credential in the vault, using only reads and, for a paper
connection, only paper hosts, and journals `ConnectionChecked` (occasion `connect`) there.
It stores the broker's account id in the personal-data vault and records only that reference
(`account_pii_ref`, §6.4), never the id. The control services read that record, have the vault
compute the account fingerprint from the reference (connections spec §3.1; they receive only the
keyed hash), complete check 3 against the connection's record and run check 4 (uniqueness), and
append `ConnectionEstablished` version 2, whose `causation_id` is that `ConnectionChecked` (rule
63), only after the passing results are journaled. The control services are the API process, which
never calls the broker (connections spec §5.2). Before appending, they confirm it is on the stream `account_ref` names, has the same
`connection_id`, an occasion of `connect` (or `reconnect`), every result `passed`, and, for an MCP
connection, the `contract` result. The executor then copies the establishment onto its stream
(rule 68): until that copy, the stream is `connecting`, and the executor may only run and record
the connect sequence's checks. Checks 5 and 6 are read from `AccountStateObserved` and never refuse
a connection (connections spec §8.1: connected, agents not deployable, status shown).

**A refused connect** is `ConnectionRefused`. Its `causation_id` is the failed `ConnectionChecked`
when the executor's check failed, and `null` when the control services refused (check 3's
fingerprint comparison, `account_mismatch`, or check 4) or when no check refused at all: the
teardown of connections spec §5.2 step 6, after the deadline (`timeout`), on an API restart past
it (`restart_past_deadline`), after an executor restarted with no token to check
(`executor_stopped`), or when no executor was started (`start_failed`: the token-exchange process or
the executor failed to start after the vault write, step 3, or the exchange stored no token, step 4), with `check` `null` (rules 54 and 65). Its account stream is an orphan: its
`account_ref` is never bound, nothing is written to it again, and it is kept like every stream,
under §6.2's retention, because it records the refusal's evidence. The executor that opened it has
exited, and its writer epoch is never reused (§5.1).

**A `causation_id` may name an event on another stream** (§3: it is an event ID). The connect
sequence uses it from the control stream to the account stream, and the copies the other way, as
§2's copies do. §11's per-range checks follow a cause only within the range's own stream; the
full-chain run's `connection_cause_mismatch` follows each one across. A reconnect and a credential
replacement run the same sequence, with occasion `reconnect` or `reauthorize`.

**`ConnectionEstablished` version 2** on the control stream: version 1's four members, then:

| Member | Type | Meaning |
|---|---|---|
| `account_ref` | `ulid` | The account stream `acct:{workspace_id}:{account_ref}` this connection writes through (§2). This is the binding DEC-261 item 10 asked for: an account stream belongs to the connection whose establishment names its `account_ref` |
| `user` | `text` | The workspace admin who connected (opaque) |
| `step_up` | `{assertion_id: text, authenticated_at: timestamp, method: text}` | Never `null`: connecting needs step-up (identity spec ID-4). This is the connecting user and step-up DEC-261 item 8 owed the connect flow |
| `margin_attestation` | `cash_account` \| `margin_disabled` (nullable) | The owner's attestation, confirmed with the step-up, that the account is a cash account or has margin disabled ([DEC-529](../project/decisions/DEC-529.md) item 11, FR-2.6); non-null exactly for a `live` connection: rule 64 |

Rule 19 applies to both versions, and rules 63 and 64 to version 2: its `causation_id` is the
passing `ConnectionChecked` of the connect sequence. Version 1 stays registered and replayable, and
maps to its `JournaledFact` as before; version 2 maps the same way. A writer that has an
`account_ref` writes version 2.

**The account stream's copy of `ConnectionEstablished`** is version 2 only: version 2's members,
then `risk_clock`. Its `causation_id` is the control-stream original (rule 63), whose members it
carries unchanged.

**`ConnectionRefused`** on the control stream: a connect, reconnect, or credential replacement the
§8.1 checks refused, or the teardown that ended it. Nothing else is kept of a refused connect
(connections spec §9.1); the credential was deleted from the vault before this record is written.

| Member | Type | Meaning |
|---|---|---|
| `connection_id` | `id` | The connection the attempt was for: a new id for a connect, the existing one otherwise |
| `broker` | `text` | |
| `environment` | `paper` \| `live` | |
| `occasion` | `connect` \| `reconnect` \| `reauthorize` | A new connection; a revoked one connected again; a credential replaced on a connection that is not revoked |
| `check` | `scope` \| `environment` \| `account` \| `uniqueness` \| `contract` (nullable) | The first §8.1 check that failed (checks 1, 2, 3, 4, and 7), or `null` for a teardown no check caused: rule 54 |
| `reason` | See the reasons table, or `timeout` \| `restart_past_deadline` \| `executor_stopped` \| `start_failed` | Rule 54 |
| `existing_connection_id` | `id?` | For `uniqueness`, the connection that already holds the account: rule 55 |
| `user` | `text` | The owner who attempted it (opaque) |
| `step_up` | As `ConnectionEstablished`'s | Never `null`: the attempt started only after step-up |

**`ConnectionCredentialRotated`** on the control stream: the owner replaced the credential of a
connection that is not revoked, and checks 1 to 4 and 7 passed against it. Its `causation_id` is
the executor's `ConnectionChecked` with occasion `reauthorize` and every result `passed` (rule 63),
which the control services confirm as for `ConnectionEstablished`. Check 3 compares the account the
new credential reaches with the connection's fingerprint, so the replacement is the same account;
the connection keeps its `connection_id`, and with it its `broker`, `environment`, and
`account_ref`, and its scopes stay the same or narrow (rule 67). The executor copies it onto the
account stream, with `risk_clock`, its `causation_id` the original (rule 63). It is how a
`suspended` connection gets a working credential again ([DEC-800](../project/decisions/DEC-800.md)
item 5): the copy alone does not leave `suspended`; rule 68 requires a `condition_cleared` that
names the copy, and then the owner's acknowledgment.

| Member | Type | Meaning |
|---|---|---|
| `connection_id` | `id` | |
| `scopes` | `[text]` | The scopes the new credential grants, strictly ascending by bytes: rule 56 |
| `user` | `text` | The owner who replaced it (opaque) |
| `step_up` | As `ConnectionEstablished`'s | Never `null` (identity spec ID-4: changing a connection) |

**`ConnectionChecked`** on the account stream: the executor's §8.1 checks 1, 2, 3, and 7, at a
connect, a reconnect, or a credential replacement (before the control stream records it), at each
executor start, and daily. Check 4 is the control services' alone, and so is check 3's fingerprint
comparison, made from `account_pii_ref`: an executor never reports `account_mismatch` (rule 58).

| Member | Type | Meaning |
|---|---|---|
| `connection_id` | `id` | The connection the checks are for: rule 68 |
| `occasion` | `connect` \| `reconnect` \| `reauthorize` \| `executor_start` \| `daily` | |
| `results` | `[{check: scope \| environment \| account \| contract, result: passed \| failed, reason: (see the reasons table)?}]` | Every check run: rules 57 and 58. `uniqueness` is the control services' (connections spec §8.1) |
| `account_pii_ref` | `pii_ref?` | The personal-data vault reference of the broker account id the credential reaches (§6.4): `pii_` and a ULID, never the id; `null` exactly when the account could not be read: rule 62 |
| `risk_clock` | `risk_clock` | |

Checks 5 (1× buying power) and 6 (account status) are not here: `AccountStateObserved` (§9)
journals the multiplier and the status they read, trading spec §7.2 and §7.3 act on them, and
neither refuses a connection or moves its state.

**`ConnectionStateChanged`** on the account stream: the connection's state (connections spec §9.1)
on the executor's side. The stream is `connecting` until its copy of the establishment, and the
connection `active` from then. `connecting` leaves no record but a `ConnectionRefused`, and
`revoked` is the control stream's `ConnectionRevoked`.

| Member | Type | Meaning |
|---|---|---|
| `connection_id` | `id` | As `ConnectionChecked`'s |
| `from`, `to` | `active` \| `degraded` \| `suspended` | Rules 59 and 60 |
| `reason` | `network_errors` \| `rate_headroom` \| `contract_drift` \| `authorization_failed` \| `credential_expired` \| `refresh_failed` \| `check_failed` \| `lease_expired` \| `condition_cleared` \| `acknowledged` | Why: rule 59 |
| `risk_clock` | `risk_clock` | |

The first three reasons are connections spec §8.2's degrading signals; the next five are §9.1's
`suspended` causes and §9.2's vault outage. A later `contract` check (occasion `executor_start` or
`daily`) that fails with `tools_missing` is connections spec §8.1 check 1's, so it suspends: it is
journaled with `check_failed` into `suspended`, never with `contract_drift`. A failed `contract`
check with reason `contract_drift` and no other failure degrades, and a failed `scope`,
`environment`, or `account` check suspends whatever else failed with it. A connect-time occasion
(`connect`, `reconnect`, `reauthorize`) moves no state: its failure is a refusal
([DEC-674](../project/decisions/DEC-674.md)).
`condition_cleared` records that the cause has cleared (good probes, a released connector version
for drift, or, out of `suspended`, a replaced credential the control services accepted) while the
state stays where it is. `acknowledged` returns the
connection to `active`, and its `causation_id` is the `OwnerAcknowledged` the executor copied (rule
61). A reconnect continues its account stream (CN-12), and with it the state that stream last
recorded: a connection revoked while `suspended` is still `suspended` when it is established again.
A reconnect journals a `ConnectionEstablished` copy and a `reconnect` check, never a rotation, so it
does not by itself leave `suspended`: after reconnecting, the owner still re-authorizes, and the
connection returns to `active` only as the next paragraph says, by `condition_cleared` and then
`acknowledged`, as trading spec §7.3 lifts its restriction.

**Out of `suspended`** ([DEC-800](../project/decisions/DEC-800.md) item 5), by re-authorization only,
in order, each on the account stream (rule 68): the owner replaces the credential; the executor journals a
`ConnectionChecked` with occasion `reauthorize`, after the `ConnectionStateChanged` that entered
`suspended`, in which every result passed (with `contract` for an MCP connection); the control
services confirm it, compare the fingerprint (the same account), and append
`ConnectionCredentialRotated` naming it; the executor copies the rotation; a `condition_cleared`
names that copy as its `causation_id`; and the owner's acknowledgment follows. A check the control
services refused is named by no rotation, so it never clears a suspension, and neither does a check
or a rotation from before the suspension.

**The halt is never behind one of these records** (CN-6, `AGENTS.md` rule 3). Into `degraded` or
`suspended`, the executor first commits `AccountRestrictionChanged` (`closing_only`, cause
`connection_unavailable`) and `AgentModeApplied` (trading spec §7.3), and only then the
`ConnectionChecked` that failed and the `ConnectionStateChanged`, in a later batch. A refused or
failed append of these records leaves openings halted. Out of it, the `acknowledged` state change is
committed in the batch that lifts the restriction, or before it, so a refused state change keeps
the restriction. No rule here applies to an exit, a protective order, or a kill switch.

**`ConnectionCredentialRefreshed`** on the account stream: the executor refreshed an expiring token
through the vault (connections spec §5.4), only where the broker issues refresh tokens (U-A3).

| Member | Type | Meaning |
|---|---|---|
| `connection_id` | `id` | As `ConnectionChecked`'s |
| `scopes` | `[text]` | The scopes the refreshed token grants, strictly ascending by bytes: rule 56 |
| `risk_clock` | `risk_clock` | |

**Reasons** a check fails, by check. A reason belongs to its check: rule 54 (`ConnectionRefused`)
and rule 58 (`ConnectionChecked`).

| Check | Reasons |
|---|---|
| `scope` | `scope_mismatch` (the grant differs from the request), `fund_movement` (a permission, scope, or tool that can move funds out, CN-2), `permissions_unreadable` (a live credential whose permissions cannot be read) |
| `environment` | `wrong_environment` (it does not work against its own environment), `reaches_both` (the broker's documentation does not show it cannot reach the other, CN-3; DEC-441 item 21) |
| `account` | `account_unreadable` (the account could not be read), `account_mismatch` (another account than the connection names, by fingerprint; the control services' only), `not_dedicated` (for Robinhood, not the dedicated agentic account) |
| `uniqueness` | `already_connected` (CN-5) |
| `contract` | `tools_missing` (an allowlisted tool is absent), `contract_drift` (the pinned hash differs, or a later check finds no pin: [DEC-676](../project/decisions/DEC-676.md) item 2) |

An **MCP connection** is one whose broker connects through MCP (connections spec §3,
`mcp_oauth`): `robinhood`. Its checks always list `contract`; a connection that is not MCP has no
contract to check, so an absent `contract` passes only for it (rule 68, §11).

**Consistency rules** (reason `schema` unless stated; the path is the member named):

54. `ConnectionRefused`: `reason` belongs to `check`, or `check` is `null` and `reason` is
    `timeout`, `restart_past_deadline`, `executor_stopped`, or `start_failed` (`payload.reason`).
55. `ConnectionRefused`: `existing_connection_id` is non-null exactly when `check` is `uniqueness`,
    and then differs from `connection_id` (`payload.existing_connection_id`).
56. `ConnectionCredentialRotated`, its copy, and `ConnectionCredentialRefreshed`: `scopes` strictly
    ascending by bytes (`non_canonical` at `payload.scopes`), as rule 19.
57. `ConnectionChecked`: `results` strictly ascending by `check` bytes, so no check is listed twice
    (`non_canonical` at `payload.results`), and it includes `scope`, `environment`, and `account`
    (`payload.results`). Whether `contract` must be listed depends on the connection's broker,
    which the record does not carry: rule 68 and §11 check it.
58. `ConnectionChecked`: in each result, `reason` is non-null exactly when `result` is `failed`, and
    then belongs to `check` and is not `account_mismatch` (`payload.results[i].reason`).
59. `ConnectionStateChanged`: `reason` fits `to` (`payload.reason`): the three degrading reasons
    only into `degraded`, the five suspending reasons only into `suspended`, `condition_cleared`
    only with `to` equal to `from` and not `active`, and `acknowledged` only into `active`.
60. `ConnectionStateChanged`: `from` fits `to` (`payload.from`): `active` only from `degraded` or
    `suspended`, and `degraded` never from `suspended`, since a credential that does not work is
    not made better by a network error.
61. `ConnectionStateChanged` with reason `acknowledged` has a non-null `causation_id`
    (`causation_id`).
62. `ConnectionChecked`: `account_pii_ref` is `null` exactly when the `account` result failed with
    `account_unreadable`, and when non-null it is listed in the envelope's `pii_refs` (§3)
    (`payload.account_pii_ref`). Its type, `pii_ref`, is `pii_` and a ULID (§6.4): anything else,
    an account number among them, is `non_canonical`.
63. `ConnectionEstablished` version 2 and `ConnectionCredentialRotated`, on either stream, have a
    non-null `causation_id` (`causation_id`): on the control stream, the passing
    `ConnectionChecked` of their connect sequence; on the account stream, the control-stream
    original the executor copied.
64. `ConnectionEstablished` version 2 and its copy: `margin_attestation` is non-null exactly when
    `environment` is `live` (`payload.margin_attestation`).
65. `ConnectionRefused` with `check` `null` has a `null` `causation_id` (`causation_id`): no check
    failed.

**Stream rules** (connections spec §3, CN-5, §9.1). These need the stream's earlier records, which
`append` does not fold. The stream's owner checks them against its own fold and refuses the draft
before appending; §11's `connection_lifecycle_mismatch` checks them on every range. Each holds on its
own stream:

66. Control stream, `ConnectionEstablished`: when an earlier `ConnectionEstablished` names the same
    `connection_id`, the latest earlier `ConnectionEstablished` or `ConnectionRevoked` of that id is
    a `ConnectionRevoked`, and `broker`, `environment`, and `account_ref` equal the first
    establishment's. A version-1 first establishment has no `account_ref`, so it is never
    re-established. No earlier `ConnectionEstablished` of another `connection_id` names the same
    `account_ref` (CN-5).
67. Control stream: a `ConnectionCredentialRotated`, or a `ConnectionRefused` with `occasion`
    `reauthorize`, names a connection established and not since revoked; a `ConnectionRefused` with
    `reconnect` names one whose latest record is `ConnectionRevoked`; one with `connect` names an id
    never established. For `reconnect` and `reauthorize`, `broker` and `environment` equal the
    establishment's. A rotation's `scopes` are a subset of the connection's latest scopes, its
    establishment's or the latest rotation's: a replaced credential never widens a grant.
68. Account stream: every connection record on it names the same `connection_id` (CN-5: one
    account stream, one connection), and:
    - **Binding.** The copy of `ConnectionEstablished` names this stream's `account_ref`, and the
      latest `ConnectionChecked` before it with occasion `connect` (`reconnect` for a second copy)
      passed every result, with `contract` for an MCP connection. Before the first copy, the stream
      holds only `ConnectionChecked` records of occasion `connect`, `reconnect`, or `reauthorize`;
      a `ConnectionStateChanged`, a `ConnectionCredentialRefreshed`, a rotation's copy, or an
      `executor_start` or `daily` check is refused.
    - **Contract.** After a copy whose broker is MCP, every `ConnectionChecked` lists `contract`.
    - **Rotation.** A rotation's copy follows a `ConnectionChecked` with occasion `reauthorize`,
      the latest before it, that passed every result (with `contract` for an MCP connection).
    - **State.** On `ConnectionStateChanged`: `from` equals the state the stream's previous
      `ConnectionStateChanged` left, or `active` when there is none; `acknowledged` follows a
      `condition_cleared`, with no other state change between them; and a `condition_cleared` out
      of `suspended` names as its `causation_id` a rotation's copy on this stream whose
      `reauthorize` check came after the `ConnectionStateChanged` that last entered `suspended`.
      The owner's acknowledgment never lifts a cause that has not cleared, and a suspended
      connection clears only on a credential the control services accepted after the suspension.

No rule here refuses a `ConnectionRevoked`, so §5.6's compromised revocation, whose kill switch
shares its batch, is never held by one. §9.8 adds no `ConnectionRevoked` version; version 2
arrives in §9.10 (v0.22, DEC-671), and these rules count a revocation of either version.

**No mapping to `JournaledFact`** beyond `ConnectionEstablished`'s (§9.2): the control-stream
version, never its copy. The executor folds the account-stream records itself, and the connection
manager the control-stream ones.

### 9.9 Workspace API records ([DEC-670](../project/decisions/DEC-670.md))

Four control-stream records the [workspace API](workspace-api.md) commits (its §4.1, §4.6, §5.1;
[DEC-436](../project/decisions/DEC-436.md) item 14), closed as §9.7's are: every listed member is
present, `null` only where the type is nullable, and any extra member is refused. §9.1's and §9.2's
types apply, with §9.3's `asset_id`. The rules number on from §9.7's. Each record names the
principal that committed it in `actor` (§3, [identity spec ID-1](identity.md)), and rules 70, 76,
78, and 79 are the first that read `actor.kind`; each reports `schema` at `actor.kind`.

**What these records never do.** A draft, a compile, and a request change no envelope (`AGENTS.md`
rule 11): a draft is not a version, nothing reads it as one, and it may fail validation; a request
reaches only the order builder (rule 4). No member holds model text, a credential, or personal
data: the draft, the prompt, and the response are stored artifacts named by `ref` (§6.3, §6.4;
workspace API API-18), and users are opaque IDs.

**`MandateDraftSaved`**: one explicit save of a mandate draft (DEC-436 item 11). The draft is the
document as edited, with each path's provenance (mandate spec §2.1), stored under its hash.

| Member | Type | Meaning |
|---|---|---|
| `draft_id` | `id` | The draft, the same across its saves |
| `draft` | `ref` | This save's stored draft |
| `origin` | `description` \| `goal_answers` \| `template` \| `version` \| `edit` \| `compile` | How this save came to be. The first four start a draft; `edit` is the owner's change to one; `compile` is the compiler's result written into one |
| `base_draft` | `ref?` | The `draft` of the save this one replaces, as the API's `If-Match` named it (workspace API API-19): rule 69 |
| `base_version` | `ref?` | The stored mandate version a draft of origin `version` starts from: rule 69 |

**`ModelInvocationRecorded`** on the control stream: the compiler's model call. The compiler acts
for a user and has no agent, so its record is on the workspace's control stream, written by the
workspace services that called the gateway ([inference spec §3.6](inference.md#36-journaling)); this
answers inference spec §13 question 1. Its members are [DEC-432](../project/decisions/DEC-432.md)
item 11's, typed from inference spec §3.1 to §3.5. `config_refs.model_version` names the compiler's
registered model. On the agent stream the record stays open until E15-8 closes it, with these
members and the agent harness's.

| Member | Type | Meaning |
|---|---|---|
| `call_id` | `ulid` | The gateway's call (inference §3.1); its retries reuse it |
| `purpose` | `compiler` | The only purpose on the control stream |
| `draft_id` | `id` | The draft compiled |
| `draft` | `ref` | The saved draft the compiler read |
| `model` | `{model_id: text, model_version: text, content_hash: ref}` | The registry entry called: rule 71 |
| `endpoint` | `text` | The endpoint that served it |
| `request_digest` | `ref` | SHA-256 of the canonical request (inference §3.1), which is stored |
| `sampling` | `{temperature: decimal, seed: integer?}` | `seed` is `null` where the provider takes none |
| `prompt_ref` | `ref` | The rendered prompt |
| `response_ref` | `ref?` | The response, or `null` when no response bytes arrived: rules 72 to 74 |
| `reported_identity` | `text?` | What the provider says served the call (INF-2) |
| `provider_request_id` | `text?` | |
| `outcome` | `ok` \| `policy_denied` \| `budget_exhausted` \| `rate_limited_local` \| `meter_unavailable` \| `input_rejected` \| `model_withdrawn` \| `deadline_exceeded` \| `provider_unavailable` \| `rate_limited_provider` \| `credential_invalid` \| `content_refused` \| `schema_invalid` \| `identity_mismatch` | Inference §3.3. The API's `compile_failed` is `schema_invalid` |
| `attempts` | `integer` | Attempts sent to a provider: rules 73 and 74 |
| `tokens` | `{input: integer, output: integer, cached: integer}` | |
| `cost_usd` | `decimal` | From the price table in effect (inference §7.2): rule 75 |
| `price_table_ref` | `ref` | The price table version used |
| `cache_hit` | `boolean` | Served from the gateway's cache (inference §3.5): rule 74 |
| `deadline` | `timestamp` | The call's absolute deadline |
| `completed_at` | `timestamp` | When the gateway finished with it: rule 75 |

**`MandateConfirmed`** version 2: version 1's members, then the agent link (DEC-436 item 14). Rules
18 and §9.2's mapping apply to it as to version 1, and version 1 stays registered and unchanged
(§8). The workspace API writes version 2; a writer without an agent link (the CLI of E10-16) may
write version 1.

| Member | Type | Meaning |
|---|---|---|
| `mandate_version`, `confirmed_paths`, `record_ref` | As version 1's | |
| `agent_id` | `id?` | The deployed agent the version is for, or `null` for a new mandate |
| `base_version` | `ref?` | The agent's version in force when the owner confirmed (workspace API §5.1): rule 77 |

**`OwnerRequestSubmitted`**: an owner's request for an order (workspace API §4.6). The runtime
hands it to the order builder, which sizes, clips, and classifies it as any proposal (§2).

| Member | Type | Meaning |
|---|---|---|
| `agent_id` | `id` | |
| `instrument_id` | `asset_id` | |
| `side` | `buy` \| `sell` | |
| `quantity` | `decimal?` | The size asked for, or `null` for the builder to size: rule 80 |
| `requested_by` | `owner` \| `client` | Who asked, from the actor: rule 79. The builder carries it to `DecisionMade` (mandate spec §6.2 step 5a) |
| `client_id` | `id?` | The client that asked: rule 79 |

**Consistency rules** (reason `schema` unless stated; the path is the member named):

69. `MandateDraftSaved`: `base_draft` is `null` exactly when `origin` is `description`,
    `goal_answers`, `template`, or `version` (`payload.base_draft`); `base_version` is non-null
    exactly when `origin` is `version` (`payload.base_version`); and `causation_id` is non-null
    when `origin` is `compile`: the compiler's `ModelInvocationRecorded` (`causation_id`). The
    first that fails, in that order, is reported.
70. `MandateDraftSaved`: the actor is a `user`, or a `client` whose `origin` is `version`
    (`actor.kind`): a client's `propose` scope creates a draft from a base version and nothing
    else (workspace API §3.8).
71. `ModelInvocationRecorded` on the control stream: `model.content_hash` equals
    `config_refs.model_version` (`payload.model.content_hash`), as rule 38 binds a thesis's model.
    A missing ref is already `missing_config_ref`.
72. `ModelInvocationRecorded`: an `ok` has its response: `response_ref` and `reported_identity`
    are non-null, each reported at itself in that order.
73. `ModelInvocationRecorded`: a refusal (`policy_denied`, `budget_exhausted`,
    `rate_limited_local`, `meter_unavailable`, `input_rejected`, `model_withdrawn`) sent nothing
    (inference §3.3): `response_ref`, `reported_identity`, and `provider_request_id` are `null`,
    `attempts` is 0, every `tokens` member is 0 (reported at `payload.tokens`), and `cost_usd` is
    0; the first that fails, in that order.
74. `ModelInvocationRecorded`: `cache_hit` is true only when `outcome` is `ok`
    (`payload.cache_hit`), and a hit made no attempt and no provider request and cost nothing:
    `attempts` 0, `provider_request_id` `null`, and `cost_usd` 0, the first that fails in that
    order. Any other call that is not a refusal reached a provider: `attempts` ≥ 1
    (`payload.attempts`).
75. `ModelInvocationRecorded`: `cost_usd` ≥ 0 (`payload.cost_usd`), and an `ok` completed no later
    than its deadline: `completed_at` ≤ `deadline`, compared to the nanosecond
    (`payload.completed_at`). A response that completes late is `deadline_exceeded` and never used
    (inference INF-3).
76. `ModelInvocationRecorded` on the control stream: the actor is a `system` (`actor.kind`), the
    services that called the gateway.
77. `MandateConfirmed` version 2: `base_version` is `null` exactly when `agent_id` is, and differs
    from `mandate_version` (`payload.base_version`): a confirmation for a deployed agent names the
    version it replaces, which is never itself.
78. `MandateConfirmed` version 2: the actor is a `user` (`actor.kind`). Only a user confirms
    (workspace API §5.1, identity spec ID-11).
79. `OwnerRequestSubmitted`: the actor is a `user` or a `client` (`actor.kind`); `requested_by`
    is `owner` for a user and `client` for a client (`payload.requested_by`); and `client_id` is
    the actor's `id` for a client and `null` for a user (`payload.client_id`). So who asked is the
    authenticated principal's, never the request body's (workspace API API-6, DEC-185 item 2).
80. `OwnerRequestSubmitted`: a non-null `quantity` is greater than 0 (`payload.quantity`).

These records map to no `JournaledFact` except `MandateConfirmed` version 2, which maps as version 1
does (§9.2), from the same members.

### 9.10 Connection revocation and client records ([DEC-671](../project/decisions/DEC-671.md))

Closed as §9.9's are, on the control stream. Rules number on from §3's 81 to 83. No member carries
a credential or a token: a client is named by its opaque ID and scopes, and a connection by its ID
(§9.2).

**`ConnectionRevoked`** version 2: version 1's member, then why and with what step-up. Version 1
stays registered and unchanged (§8); the workspace API writes version 2.

| Member | Type | Meaning |
|---|---|---|
| `connection_id` | `id` | |
| `reason` | `owner` \| `compromised` | `owner`: the ordinary revoke, which the API refuses while an agent on the connection holds positions or is not stopped (workspace API §4.5). `compromised`: the revoke now of workspace API §5.6, which follows its kill switch: rule 84 |
| `step_up` | `{assertion_id: text, authenticated_at: timestamp, method: text}` | Never `null`: a revocation needs step-up (identity spec ID-4). Without it the API commits only the kill switch (workspace API §5.6) |

**`ClientConnected`**: a user issued a client its token (identity spec §12.1, workspace API §3.8).

| Member | Type | Meaning |
|---|---|---|
| `client_id` | `id` | The client, as its actor's `id` names it |
| `user` | `text` | The user it acts for (opaque), its actors' `on_behalf_of`: rule 88 |
| `scopes` | `[read \| request \| propose \| dry_run \| hold]` | Workspace API §3.8's closed list: rules 86 and 87 |
| `agents` | `[id]` | The agents it may see: rules 86 and 87 |
| `step_up` | `{assertion_id: text, authenticated_at: timestamp, method: text}` | Never `null`: connecting a client needs step-up (identity spec ID-4) |

**`ClientRevoked`**: a client's token revoked. It needs no step-up, because it only removes access
(identity spec ID-5); revocation takes effect for every request authorized after it commits (ID-11).

| Member | Type | Meaning |
|---|---|---|
| `client_id` | `id` | |
| `user` | `text` | The user it acted for (opaque) |
| `reason` | `owner` \| `admin` \| `member_deactivated` \| `deprovisioned` \| `compromised` | Who ended it and why: the client's own user, a workspace admin (identity spec §4.2), the deactivating admin or the system on a member's deactivation (identity §5.2), the system on deprovisioning (§11.1), or a suspected compromise: rule 89 |

**Consistency rules** (reason `schema` unless stated; the path is the member named):

84. `ConnectionRevoked` version 2: `causation_id` is non-null exactly when `reason` is
    `compromised` (`causation_id`); an ordinary revoke has no cause. A compromised revocation's
    `causation_id` names an `OwnerCommandIssued` earlier in the same `append` batch whose `command`
    is `kill_switch`, whose `scope` is `connection`, and whose `subject` is the revoked
    `connection_id` (`causation_id`, as rule 45 checks its pair inside one batch). So the record
    shows this connection's kill switch ran first (workspace API §5.6), and a revocation citing any
    other event, a wider kill switch, or a cause outside its batch is refused. These are the
    members the CLI writes on a kill switch today; no §11 check repeats the clause, since a range
    never splits a batch.
85. `ConnectionRevoked` version 2: the actor is a `user` (`actor.kind`).
86. `ClientConnected`: `scopes` and `agents` are non-empty (the first that is empty).
87. `ClientConnected`: `scopes` and `agents` are each strictly ascending by bytes (`non_canonical`
    at the list, `scopes` first).
88. `ClientConnected`: the actor is a `user` (`actor.kind`), and `user` is its `id`
    (`payload.user`): a user connects their own client (identity spec §4.2).
89. `ClientRevoked`: the actor follows the `reason` (`actor.kind`): a `user` for `owner` and
    `admin`, the `system` for `deprovisioned`, and either a `user` or the `system` for
    `member_deactivated` (the deactivating admin, or a scheduled deactivation) and `compromised`. For `owner`, `user` is the actor's `id`; for `admin`, it is not
    (`payload.user`).

`ConnectionRevoked` version 2 maps to the `ConnectionRevoked` fact as version 1 does (§9.2). The
client records map to no `JournaledFact`.

### 9.11 The hold on new openings ([DEC-672](../project/decisions/DEC-672.md))

An owner, or a client the owner connected with the `hold` scope, may hold an agent's new openings:
the agent goes to `exits_only` and keeps every exit running. Only the owner lifts it, with step-up
([DEC-191](../project/04-decision-log.md#decisions), [mandate spec §6.1](mandate.md#61-purposes)).
The hold is owner state beside the lifecycle of §9.1's `AgentModeChanged`: it survives a pause, a
resume, and a restriction lifting, so the agent stream records it on every mode change. Closed as
§9.7's records are; times are integer risk-clock seconds as §9.7's are (DEC-533 item 2).

**`OwnerCommandIssued`**, for `hold_openings` and `lift_hold` only. The members, in order, are
the ones the CLI already writes for every owner command (`mandate-cli`'s `agent::issued`), so the
other commands close later (M7's agent-commands slice) without a member renamed; here the Stop's
and the owner exit's members are always `null`. **Every other command stays open at this
version:** `append` catalogues `OwnerCommandIssued` on the control stream, reads its `command`
(text, else `schema` at `payload.command`), refuses it from a client (rule 90), and judges nothing
else about it. So a pause or a kill switch is recorded from any other principal, including the
host CLI's `system` actor (identity spec §6.4), and is never refused for its members (`AGENTS.md`
rule 13); rule 84's batch reads a kill switch's `command`, `scope`, and `subject` from it.

| Member | Type | Meaning |
|---|---|---|
| `agent` | `id` | The agent held or released |
| `command` | `hold_openings` \| `lift_hold` | |
| `scope` | `agent` | A hold is per agent |
| `subject` | `id` | The scope's subject, the agent itself: rule 91 |
| `release`, `warning_shown` | `null` | A Stop's release choice and warning; none here |
| `bid`, `bid_size`, `floor` | `null` | An owner exit's confirmed bid; none here |
| `user` | `text` | The human who asked (opaque): rule 91 |
| `submitted_at` | `integer` | The risk-clock second it was committed at |
| `step_up` | `{assertion_id: text, authenticated_at: integer, method: text}?` | A lift's evidence, judged by the runtime; `null` for a hold: rule 92 |

**`AgentModeChanged`** version 2 on the agent stream: version 1's members, with the reasons
`owner_hold` and `owner_lift_hold`, then `held`. Version 1 stays registered and unchanged (§8), and
a runtime that records a hold writes version 2 for every mode change after it.

| Member | Type | Meaning |
|---|---|---|
| `from`, `to`, `lifecycle` | As version 1's | `to` obeys rule 94, which replaces rule 11 at this version |
| `reason` | version 1's reasons \| `owner_hold` \| `owner_lift_hold` | |
| `held` | `boolean` | Whether the owner's hold stands after the change: rule 93 |

**`OwnerCommandRefused`** version 2: version 1's members, with the command `lift_hold`, on the agent
stream only (rule 26). Version 2 is registered on the agent stream alone, so a version-2 refusal on
the account stream is `unknown_schema` at `payload`, and rule 26's `stream_mismatch` judges version
1's commands. A lift whose step-up does not count is refused like a resume (rule 28 keeps
`not_independent` off it).

**Consistency rules** (reason `schema` unless stated; the path is the member named):

90. `OwnerCommandIssued`: a `client` actor's command is `hold_openings` (`actor.kind`): a client
    never lifts a hold, pauses, resumes, stops, exits, or engages the kill switch (identity spec
    ID-11, DEC-141 item 3). A `hold_openings` is a `user`'s or a `client`'s, and a `lift_hold` a
    `user`'s (`actor.kind`). No other actor is constrained on another command, so the host CLI's
    kill switch is never refused here.
91. `OwnerCommandIssued`: `user` is the actor's human (§3) (`payload.user`), and `subject` is
    `agent` (`payload.subject`).
92. `OwnerCommandIssued`: a `hold_openings` has a `null` `step_up` (`payload.step_up`). A hold only
    reduces risk, so it needs none (`AGENTS.md` rule 2), and a client never presents one.
93. `AgentModeChanged` version 2: `held` is true when `reason` is `owner_hold` and false when it is
    `owner_lift_hold` (`payload.held`).
94. `AgentModeChanged` version 2: `to` is at least as strict as `lifecycle`, and at least
    `exits_only` while `held` (`payload.to`; `normal` < `exits_only` < `paused` < `stopped`). So a
    resume never clears a hold, and a lift never clears a pause. A lift that leaves `to` at
    `exits_only` is valid: a latched limit or a restriction may still hold the agent (MI-3).
95. `AgentModeChanged` version 2: a copy of an owner command, whose `reason` is `owner_pause`,
    `owner_resume`, `owner_stop`, `owner_hold`, or `owner_lift_hold`, has a non-null
    `causation_id`: its `OwnerCommandIssued` (`causation_id`). As rule 16 does at version 1, each
    command has one copy: its `AgentModeChanged`, or, for a lift, its `OwnerCommandRefused`.

**Carried, never dropped.** `held` changes only on `owner_hold` and `owner_lift_hold`: every other
version-2 copy (a restriction, a reconciliation, a kill switch, a pause, a resume, a Stop) carries
the last one's, and once a stream holds a version-2 `AgentModeChanged` every later one is version
2. `append` sees one record and cannot check either, so §11's per-range check `held_mismatch` does,
anchored on the stored chain before the range (or failing closed without it), and `mandate journal
verify` reports it; a stray `held: false` would otherwise drop the hold on replay.

**Lifecycle.** A hold is copied whatever the mode: on a paused or stopped agent the copy records
`held` with `to` unchanged, so command status reaches `applied`. A second hold, or a lift with no
hold standing, is copied too, with `held` as it already stood. A latched limit, a restriction, or a
version change never sets or clears the hold, and a restart folds it from the last copy.

### 9.12 Membership records ([DEC-437](../project/decisions/DEC-437.md) item 9, [DEC-648](../project/decisions/DEC-648.md))

The control-stream records of a workspace membership ([identity spec §5](identity.md#5-membership-lifecycle),
§12.1), closed at schema version 1 as §9.7's records are: every listed member is present, `null`
only where the type is nullable, and any extra member is refused. §9.1's types and report order
apply, and the rules number on from §9.11's. Instants are §4.7 timestamps, as the identity spec's
cool-off and expiry are wall-clock times. A record's own instant (`invited_at`, `activated_at`,
`changed_at`, `reactivated_at`) is the envelope's `event_time` exactly (rule 106), so a writer cannot
date a grant into the past to escape its cool-off or a stale step-up; every cool-off end and step-up
window is computed from `event_time`. Step-up evidence is §9.2's `DisclosureAccepted.step_up`
type, `null` only on a `MemberRoleChanged` that grants nothing (rule 101). The test vectors' `membership` section holds a base draft
of each record, an invalid draft for every member type and rule, and valid drafts for the cases a
rule might be misread to refuse.

The control stream is the workspace's, so no record names its workspace. A member is the opaque
ULID of a `user` principal (identity spec §3.1); clients, service accounts, agents, and platform
staff hold no membership. No payload carries an address, a name, an identity-provider subject, or an
invitation token (rule 6, ID-9, §6.4): an invitation is its opaque ULID, and the invited address is a
vault reference in the envelope's `pii_refs`.

Every membership record ends with `session_ref` (`text?`): the opaque reference of the session the
writing user acted through ([identity spec §6.2, §12.2](identity.md#122-actor-fields)), never a cookie
or a token, and `null` for a record the system writes (rule 105). It is a payload member here only.
Identity spec §12.2 asks every committed event to name its session, which is an envelope field and so
a new `envelope_version` (§8); that change is owed separately (DEC-648 item 8). Organization memberships and org roles close with the
organization story, not here.

**Types**, beyond §9.1's and §9.2's:

| Type | Values | Refused as |
|---|---|---|
| `role` | `approver` \| `auditor` \| `operator` \| `viewer` \| `workspace_admin`: the workspace roles of [identity spec §4.1](identity.md#41-roles) | As `id` |

**`MemberInvited`**: a workspace admin invited an address (identity spec §5.2).

| Member | Type | Meaning |
|---|---|---|
| `invitation` | `ulid` | The invitation |
| `roles` | `[role]` | The roles it names, granted only once accepted: rules 96 and 100 |
| `invited_by` | `text` | The inviting admin (opaque): rules 97 and 98 |
| `step_up` | `{assertion_id: text, authenticated_at: timestamp, method: text}` | Inviting needs step-up (identity spec §4.2): rule 102 |
| `invited_at` | `timestamp` | The instant the invitation was issued: rule 106 |
| `expires_at` | `timestamp` | The invitation expires unaccepted at this instant: rule 104 |
| `session_ref` | `text?` | Rule 105 |

**`MemberInvitationRevoked`**: an admin revoked an invitation before it was accepted (identity spec
§5.1, `invited` to `revoked`). An expired invitation needs no record: its `expires_at` ends it.

| Member | Type | Meaning |
|---|---|---|
| `invitation` | `ulid` | |
| `revoked_by` | `text` | The admin (opaque): rules 97 and 98 |
| `session_ref` | `text?` | Rule 105 |

**`MemberActivated`**: a membership begins, by an accepted invitation or by the founding grant that
creates a workspace (identity spec §3.2, ID-13). It enters `cooling_off`, and is `active` from
`cool_off_ends_at` (identity spec §5.1).

| Member | Type | Meaning |
|---|---|---|
| `member` | `ulid` | The user principal |
| `invitation` | `ulid?` | The invitation accepted, or `null` for the founding grant: rule 99 |
| `reason` | `invitation_accepted` \| `founding` | Rules 98 and 99 |
| `roles` | `[role]` | Rules 96, 99, and 100 |
| `method` | `passkey` \| `oidc` \| `email_link` | How the member signed in (identity spec §6.1) |
| `activated_at` | `timestamp` | The instant the membership began: rule 106 |
| `independent_approval_required` | `boolean` | The workspace's effective `independent_approval_required` at the grant ([mandate spec §4.3](mandate.md#43-policy-hierarchy-dec-51-dec-98)), which decides the cool-off: rule 103 |
| `cool_off_ends_at` | `timestamp` | The end of the activation's cool-off ([identity spec §8.3](identity.md#83-cool-off-against-sock-puppets)): rule 103 |
| `session_ref` | `text?` | The invitee's session; `null` for the founding grant: rule 105 |

**`MemberRoleChanged`**: a workspace admin granted or removed roles of another member.

| Member | Type | Meaning |
|---|---|---|
| `member` | `ulid` | |
| `changed_by` | `text` | The admin (opaque): rules 97 to 99 |
| `added` | `[{role: role, cool_off_ends_at: timestamp}]` | Each role granted, and the end of its cool-off: rules 96, 100, and 103 |
| `removed` | `[role]` | Rules 96 and 100 |
| `changed_at` | `timestamp` | The instant of the change: rule 106 |
| `independent_approval_required` | `boolean` | As on `MemberActivated`: rule 103 |
| `step_up` | `{assertion_id: text, authenticated_at: timestamp, method: text}?` | A grant needs step-up and a removal does not: rules 101 and 102 |
| `session_ref` | `text?` | Rule 105 |

**`MemberDeactivated`**: the member loses the workspace (identity spec §5.2, §11.1).

| Member | Type | Meaning |
|---|---|---|
| `member` | `ulid` | |
| `by` | `text` | Who committed it (opaque): rules 97 to 99 |
| `reason` | `admin` \| `left` \| `deprovisioned` \| `group_removed` | An admin deactivated them, they left, the customer's directory deprovisioned them (SCIM), or an identity-provider group removal did (identity spec §11.1) |
| `session_ref` | `text?` | Rule 105 |

**`MemberReactivated`**: an admin reactivated a deactivated member, with its kept roles. It enters
`cooling_off` again. It names the roles it restores, so its cool-off is decided by the record alone.

| Member | Type | Meaning |
|---|---|---|
| `member` | `ulid` | |
| `by` | `text` | The admin (opaque): rules 97 to 99 |
| `step_up` | `{assertion_id: text, authenticated_at: timestamp, method: text}` | Reactivating needs step-up (identity spec §5.1): rule 102 |
| `roles` | `[role]` | The kept roles it restores, exactly those the member held when deactivated: rules 96 and 100 |
| `reactivated_at` | `timestamp` | Rule 106 |
| `independent_approval_required` | `boolean` | As on `MemberActivated`: rule 103 |
| `cool_off_ends_at` | `timestamp` | Rule 103 |
| `session_ref` | `text?` | Rule 105 |

**`MemberRemoved`**: a deactivated member is removed. Identity records are kept for the records
period (§6.4).

| Member | Type | Meaning |
|---|---|---|
| `member` | `ulid` | |
| `by` | `text` | Who committed it (opaque): rules 97 to 99 |
| `reason` | `admin` \| `org_deleted` | An admin removed them, or the organization's deletion ended every membership (identity spec §5.2) |
| `session_ref` | `text?` | Rule 105 |

**Consistency rules** (reason `schema` unless stated; the path is the member named):

96. Role lists are strictly ascending by bytes, so no role is listed twice: `roles`, `removed`, and
    `added` by its `role` (`non_canonical` at the list).
97. The writer: `invited_by`, `revoked_by`, `changed_by`, and `by` equal the envelope's `actor.id`
    (at that member), so a record names the identity that wrote it (ID-1).
98. Who writes it (`actor.kind`): a `user` for `MemberInvited`, `MemberInvitationRevoked`,
    `MemberRoleChanged`, and `MemberReactivated`, for `MemberActivated` with reason
    `invitation_accepted`, for `MemberDeactivated` with reason `admin` or `left`, and for
    `MemberRemoved` with reason `admin`; `system` for the founding grant (ID-13), for
    `MemberDeactivated` with reason `deprovisioned` or `group_removed`, and for `MemberRemoved` with
    reason `org_deleted`. A client never changes a membership (ID-11).
99. Nobody changes their own membership but by leaving (ID-13). `MemberRoleChanged.changed_by`, and
    `by` on `MemberReactivated`, on `MemberDeactivated` with reason `admin`, and on `MemberRemoved`
    with reason `admin`, differ from `member` (at that member). `MemberDeactivated` with reason
    `left` has `by` equal to `member` (`payload.by`). `MemberActivated`: `invitation` is `null`
    exactly when `reason` is `founding` (`payload.invitation`), and an accepted invitation's
    `member` equals `actor.id`, the invitee who signed in (`payload.member`).
100. Roles: `MemberInvited.roles`, `MemberActivated.roles`, and `MemberReactivated.roles` are
    non-empty (`payload.roles`); the
    founding grant's include `workspace_admin`, since a workspace always has an `active` admin
    (identity spec §5.2; `payload.roles`); a `MemberRoleChanged` adds or removes at least one role
    (`payload.added`), and no role is both added and removed (`payload.removed`).
101. `MemberRoleChanged.step_up` is non-null exactly when `added` is non-empty (`payload.step_up`).
102. Step-up evidence is [mandate spec §6.1](mandate.md#61-purposes)'s, valid at the envelope's
    `event_time`: its method is `passkey` in a `live`
    envelope and `passkey` or `cli_confirm` otherwise ([identity spec §7.3](identity.md#73-rules);
    `payload.step_up.method`), and 0 ≤ `event_time` − `authenticated_at` ≤ 300 seconds, so evidence
    authenticated after the instant fails closed (`payload.step_up.authenticated_at`). That the
    assertion was never used before is a check across records, the identity crate's (identity spec
    §7.2).
103. Cool-off ([identity spec §8.3](identity.md#83-cool-off-against-sock-puppets), stated once there),
    exactly: each cool-off end is the envelope's `event_time` plus 86 400 seconds when the record's
    `independent_approval_required` is true and the grant adds `operator` or `approver` to an existing
    workspace, and is `event_time` itself otherwise (at that `cool_off_ends_at`). The grant adds them
    when a `MemberActivated` with reason `invitation_accepted` or a `MemberReactivated` holds either in
    its `roles`, or a `MemberRoleChanged` adds that role (each added role decided by itself). The
    founding grant creates the workspace, so it never cools off.
104. `MemberInvited.expires_at` is `invited_at` plus exactly 7 days (identity spec §5.2;
    `payload.expires_at`).
105. `session_ref` is non-null exactly when `actor.kind` is `user` (`payload.session_ref`): a user acts
    through a session, and the system writes without one.
106. A record's own instant (`invited_at`, `activated_at`, `changed_at`, `reactivated_at`) equals the
    envelope's `event_time` exactly (at that member).

**The fold** (identity spec §5.1, §5.3, ID-7). These records are the only source of a membership's
state. Folding the control stream in `seq` order: `MemberInvited` makes an invitation `invited`
before `expires_at` and `expired` from it; `MemberInvitationRevoked` makes it `revoked`; `MemberActivated` and
`MemberReactivated` make the member `cooling_off` until their `cool_off_ends_at` and `active` from it;
`MemberRoleChanged` adds and removes roles, each added role effective from its `cool_off_ends_at`
and each removal at once; on an activation or reactivation, `operator` and `approver` are effective
from its `cool_off_ends_at` and every other role at once;
`MemberDeactivated` makes the member `deactivated`, keeping its roles for a reactivation; and
`MemberRemoved` makes it `removed`. A `MemberRoleChanged` whose `added` is empty may remove kept
roles from a `deactivated` member, so an admin can strip a suspended member's roles while offboarding
([DEC-654](../project/decisions/DEC-654.md) item 7); a grant to a `deactivated` member is still
refused. A deactivated member whose kept roles are all removed cannot be reactivated, since rule 100
refuses a `MemberReactivated` with no roles: they return only by removal and a new invitation. `removed`, `expired`, and `revoked` are terminal: a removed
member comes back only through a new invitation, which starts a new membership with no role of the
old one. An invitation an activation used is `accepted`, the fold's own name for it: identity spec
§5.1 has no such state, because the membership it started takes over. A record that does not fit the
state it finds is refused: a `MemberInvited` whose invitation ULID was already issued; any record
naming an invitation never issued or a member never activated; a second activation of a member
whose membership is not `removed`; an activation by an invitation that is not `invited` at
`activated_at` (so `activated_at` < `expires_at`, and an invitation activates at most once) or with
roles other than the invitation's; a role change for a member who is not `active` or `cooling_off`,
other than a removal only from a `deactivated` member, or that removes a role the member does not
hold (for a `deactivated` member, one it does not keep) or adds one it does; a reactivation of a member who
is not `deactivated`, or with roles other than those kept; a deactivation of one who is not
`active` or `cooling_off`; a removal of one who is not `deactivated`; and a revocation of an
invitation that is not `invited`. Such a record is refused by workspace services before it is committed; a fold that meets one anyway reads the
membership as unreadable, and `workspace_users` as 1 (identity spec §5.3). The cross-record checks
are the identity crate's (E9-7), not `append`'s. Besides the state checks above, they are: the
author of an admin's record is an `active` member whose `workspace_admin` role is effective at
`event_time` ([DEC-654](../project/decisions/DEC-654.md) item 2); the last owner and last admin
rules (identity spec §5.2); that a step-up assertion was never used before (rule 102); that a
record's `independent_approval_required` equals the workspace's effective policy at `event_time`
(rule 103); and that no record carries an entry that changes nothing: `change_roles` passes one
(DEC-654 item 6), but the fold refuses granting a held role or removing one not held, and rule 100
refuses a record left with no entry, so workspace services drop such entries before committing and
commit nothing when none is left.

The state at an instant *t* folds the records whose `event_time` is at or before *t* (rule 106 makes
that each record's own instant), and reads each cool-off and expiry against *t*: `workspace_users`
(identity spec §5.3) counts the members `active` at *t*. The test vectors' `membership_fold` section
holds histories of membership records, valid under the rules above except a record a history lists
in `refused` with the refusal it expects (which the fold reads as out of state too), mixed with records by clients,
agents, and service accounts, which hold no membership and which the fold ignores. Each history
states, at named instants, each member's and invitation's state, each member's effective roles, and
the count, written by hand from identity spec §5 and §8.3. It also holds out-of-state histories that
read as unreadable, with a count of 1. The reference fold
([reference/journal/membership_fold.py](../../reference/journal/membership_fold.py)) reproduces
every one, and the identity crate's fold is tested against them.

**Order** ([DEC-659](../project/decisions/DEC-659.md), [DEC-657](../project/decisions/DEC-657.md)
item 4). A late event keeps its arrival `seq` (§8), so a membership record committed late, or after
the writer's clock stepped back, would follow one with a later `event_time`. The fold compares each
membership record with the membership record before it in `seq` order, whether or not either is at
or before the instant read: a `seq` not above that record's, or an `event_time` before it, is out of
order. An equal `event_time` is in order, by `seq`. A record out of order makes the fold unreadable
from its `event_time`, so `workspace_users` reads 1 from then on, as for a record that does not fit
its state, and a reading before that instant is unchanged. Records of other types are not compared:
their instants say nothing about the membership records' order. That is the fold's backstop. The
writer keeps a stream from reaching it:

- **One writer.** One writer commits the seven `Member*` types on `ctl:{workspace_id}`
  ([DEC-646](../project/decisions/DEC-646.md)); workspace services call it after their own checks
  and never append a membership record themselves.
- **Inside the append.** It checks order within the serialized append (§5.1, per control stream)
  that assigns the record's `seq` and commits it, against `last`, the stream's last record of the
  seven membership types as read in that append, and takes the record's `event_time` there too. A
  store that cannot serialize the append per workspace commits no membership record.
- **The rule.** It refuses the record when its `event_time` is before `last`'s or its `seq` is not
  above `last`'s; an equal `event_time` passes. The first membership record (there is no `last`)
  passes, whatever its `seq` and instant. Every one of the seven types is checked alike, as the
  record and as `last`, the founding grant included.
- **Refused, never clamped.** A refused record commits nothing. The writer never moves its
  `event_time` up to `last`'s, which rule 106 forbids and which would move the cool-off or expiry
  the record decides, and never commits it unchecked. The caller retries later, and nothing is
  granted meanwhile: after the writer's clock steps back, every membership record is refused until
  the clock is back at `last`'s `event_time`.
- **A moved head re-runs the check.** When the append finds the head moved (`HeadMismatch`), nothing
  is written, and the next attempt reads the head and `last` again, takes `event_time` again, and
  checks again, so a `last` read before a concurrent commit never admits a record. After a bounded
  number of such attempts the writer commits nothing and the caller retries later.

A writer that keeps the rule never commits a record the fold reads as out of order, so one late
membership record cannot latch a healthy workspace's `workspace_users` at 1 (ID-7). Like the other
cross-record checks, the rule is the identity crate's (E9-7), not `append`'s. Every
`membership_fold` history is in `seq` and `event_time` order, so the rule changes no vector; the
identity crate's tests hold the out-of-order cases.

### 9.13 Records access, export, and verification records ([DEC-780](../project/decisions/DEC-780.md))

The control stream's records of who read the records outside the product views, what was exported,
and what was verified (§7, §11, §12), closed at schema version 1 as §9.7's records are: every listed
member is present, `null` only where the type is nullable, and any extra member is refused. §9.1's
types apply, with two more below. The rules number on from §9.12's. None of the three names a
configuration.

**Writers.** The control stream's single writer, workspace services (§2), appends all three. The
workspace API's control-stream writer appends a `RecordsAccessed` or an `ExportCreated` before it
serves the records or the export ([workspace API spec](workspace-api.md) §4.8, API-16), so a read
or export that cannot be journaled is not served. It appends a `VerificationRun` for a verification
a principal requested (`POST /verifications`) once the run ends and before its result is served.
Workspace services append one for each scheduled run of §11 and for a restore drill's run. The
`actor` is the principal that asked: a `user`, a service account (`system`, its opaque ID), a
`client` for its own reads (§3, rule 83; of these three records, `RecordsAccessed` only), or, for a scheduled run, the
verifying process (`system`). Every identifier is opaque (§6.4).

**What they carry.** A payload names streams by §2's identifiers, positions by `seq`, and contents
by hash: stream ranges, digests, artifact references, check codes, and opaque IDs. It never
carries an instrument, an order, a position, a quantity, a price, or mandate content, so a record of
an audit read discloses nothing the read itself did not authorize. **Tenant isolation:** every range
names a stream of the control stream's own workspace (rule 107). A trace that follows a
`causation_id` lists every stream it read and never follows one into another workspace's stream; a
request that would reach one is answered as absent ([workspace API spec](workspace-api.md) API-9)
and journals nothing about that stream.

**Types.**

| Type | Values | Refused as |
|---|---|---|
| `stream_id` | §2's form: `acct:{workspace_id}:{account_ref}`, `agent:{workspace_id}:{agent_id}`, `ctl:{workspace_id}`, `clock:{workspace_id}`, or `ntf:{workspace_id}`, each segment `[A-Za-z0-9_-]+` | As `id` |
| `digest` | 64 lowercase hex SHA-256: an event's `hash` or `prev_hash` (§3), or a digest of bytes the journal does not store, such as §12's verifier digest. It is not a `ref`: it names no stored artifact, so it is not listed in `artifact_refs` and §11 check 6 does not read it | As `id` |

**`range`**, the unit every record covers: one stream's events from `from_seq` to `to_seq`, both
included.

| Member | Type | Meaning |
|---|---|---|
| `stream_id` | `stream_id` | The stream: rule 107 |
| `from_seq`, `to_seq` | `integer` | The first and last `seq` covered |
| `prev_hash` | `digest` | The hash before `from_seq`: the trusted start (§11), 64 zeros for seq 1 |
| `to_hash` | `digest` | The hash of the event at `to_seq`, the head the read or export reflects (workspace API API-14) |

**`RecordsAccessed`**: a read outside the product views, journaled before it is served (§7).

| Member | Type | Meaning |
|---|---|---|
| `accessor` | `text` | The principal that read (opaque): rule 108. A client is its own accessor, and the human it acts for is the envelope's `actor.on_behalf_of` (§3, rule 81; [workspace API spec](workspace-api.md) §3.3, §3.8) |
| `operation` | `id` | The workspace API operation that read, by name ([workspace API spec](workspace-api.md) §4) |
| `ranges` | `[range]` | Every stream range the response was built from, with the hashes it reflects |
| `resources` | `[id]` | The opaque IDs of the other resources the operation read (an agent, a notice, an approval), strictly ascending by bytes: rule 108. Never a name, a ticker, or any other content |
| `result` | `ref?` | The response served, stored as an artifact (§6.3), when the operation keeps it (a replay's result, [workspace API spec](workspace-api.md) §4.1); otherwise `null` |

**`ExportCreated`**: an export (§12), journaled before it is served (API-16). Its `event_id` is the
export's ID.

| Member | Type | Meaning |
|---|---|---|
| `form` | `canonical` \| `json` \| `csv` | The canonical export, or a JSON-lines or CSV view derived from one ([workspace API spec](workspace-api.md) §4.8). The examination bundle is not a form at this version |
| `ranges` | `[range]` | The ranges exported, each with its trusted start and its head as the export's segment manifests (§6.2) record them |
| `verifier_digest` | `digest` | The canonical export's verifier digest (§12, [DEC-265](../project/04-decision-log.md#decisions) item 3): one SHA-256 over every segment manifest and file, anchor root, and timestamp token, length-prefixed. A view names the digest of the canonical export it is derived from. Anyone holding the export recomputes it |
| `view` | `digest?` | The SHA-256 of the view's bytes as served: rule 109 |

**`VerificationRun`**: one run of §11's verification and its result.

| Member | Type | Meaning |
|---|---|---|
| `trigger` | `startup` \| `segment_export` \| `weekly` \| `request` \| `restore_drill` | Why it ran: §11's schedule, a principal's request, or a restore drill: rule 110 |
| `ranges` | `[checked_range]` | Every range verified, each with its own result |
| `result` | `pass` \| `fail` | Rule 112 |

A `checked_range` is a `range` whose `to_hash` is `digest?` (the head the run verified, or
`null` when a failure left none: rule 111), followed by:

| Member | Type | Meaning |
|---|---|---|
| `failure` | `{check, seq: integer?}?` | The first failure §11 reports for the range, or `null`. `check` is one of §11's codes: `non_canonical`, `column_mismatch`, `seq_gap`, `rehash_mismatch`, `prev_hash_mismatch`, `artifact_missing`, `artifact_mismatch`, `anchor_head_mismatch`, `anchor_root_mismatch`, `tsa_token_invalid`, `segment_manifest_mismatch`, `segment_gap`, `anchor_self_mismatch`, `break_glass_cause_mismatch`, `intent_action_mismatch`, `mode_event_mismatch`, `held_mismatch`, `connection_lifecycle_mismatch`, `connection_cause_mismatch`; `seq` is where it was reported, for a per-event check the position walked (the previous `seq` plus 1, or `from_seq` first), whatever the body there says: rule 111 |

A verification a principal requested for an export names that export's `ExportCreated` as its
`causation_id`. A failed run does not repair anything and does not replace §11's incident path:
`IntegrityIncidentRecorded` still records it.

**Consistency rules** (reason `schema`; the path is the member named):

107. Every record's `ranges` is non-empty (`payload.ranges`). Each range, in array order, with its
    clauses in this order, at `payload.ranges[i].<member>`:
    its `stream_id`'s `{workspace_id}` segment equals the envelope `stream_id`'s (`stream_id`), so
    no record names another workspace's stream; `from_seq` is at least 1 (`from_seq`); `to_seq` is
    at least `from_seq` (`to_seq`); `prev_hash` is 64 zeros exactly when `from_seq` is 1
    (`prev_hash`); and after the first range, its `stream_id` sorts after the previous range's by
    bytes, or equals it with a `from_seq` greater than the previous range's `to_seq` (`stream_id`),
    so ranges are ordered and never overlap.
108. `RecordsAccessed`: `accessor` equals the envelope's `actor.id` (`payload.accessor`);
    `actor.kind` is neither `agent` nor `broker` (`actor.kind`), so a `client` actor, in §3's one
    shape (rules 81 to 83), records its own reads with its own `id` as `accessor`; a
    `platform_operator`'s read has a non-null `causation_id`, the `PlatformOperatorAction` that
    opened its customer-approved break-glass window (§7) (`causation_id`); and each of `resources`
    sorts after the one before it by bytes (`payload.resources`). `append` cannot see what that
    `causation_id` names, so §11's `break_glass_cause_mismatch` checks it; until DEC-261 item 9
    closes `PlatformOperatorAction`, every operator read this rule admits fails that check. Rule 83 already keeps
    a client off `ExportCreated` and `VerificationRun`, so rules 109 and 110 refuse it again only as a
    second statement of workspace API §3.8 (a client never exports or verifies).
109. `ExportCreated`: `actor.kind` is `user` or `system` (`actor.kind`), and `view` is non-null
    exactly when `form` is `json` or `csv` (`payload.view`).
110. `VerificationRun`: `actor.kind` is `system`, or, for a `request`, `user` or `system`
    (`actor.kind`). Rules 109 and 110 refuse a `platform_operator`, so a §7 break-glass export or
    verification by platform staff cannot be journaled and is not served: it fails closed on
    purpose. Platform staff's break-glass reads are journaled as `RecordsAccessed` (rule 108 admits
    them); an export or a verification for them is run by the workspace's own user or service
    account.
111. `VerificationRun`: each range, in array order, at `payload.ranges[i].<member>`: a non-null
    `failure`'s `seq` is non-null exactly when its check is reported at an event (§11's per-event
    checks 1 to 6, `anchor_head_mismatch` at the anchored `seq`, `anchor_self_mismatch`,
    `break_glass_cause_mismatch`, `intent_action_mismatch`, `mode_event_mismatch`, `held_mismatch`,
    `connection_lifecycle_mismatch`, and `connection_cause_mismatch`) and null for `anchor_root_mismatch`, `tsa_token_invalid`,
    `segment_manifest_mismatch`, and `segment_gap` (`failure.seq`); a non-null `seq` lies from
    `from_seq` to `to_seq` (`failure.seq`); and `to_hash` is non-null when `failure` is null
    (`to_hash`).
112. `VerificationRun`: `result` is `pass` exactly when every range's `failure` is null
    (`payload.result`).

### 9.14 Anchor and segment records ([DEC-783](../project/decisions/DEC-783.md))

The control stream's records of an anchor (§10) and of a closed segment shipped to the cold store
(§6.2), closed at schema version 1 as §9.7's records are: every listed member is present, `null`
only where the type is nullable, and any extra member is refused. §9.1's types apply, with §9.13's
`stream_id` and `digest`. The rules number on from §9.13's. Neither record names a configuration.
Workspace services write both as a `system` actor: the anchoring job and the cold-store exporter.

Both take their shapes from what the code already reads. An anchor's `leaves` and `root` are the
anchor file the verification command reads "as `AnchorComputed` records it"
([DEC-115](../project/04-decision-log.md#decisions) item 6), member for member. A segment's members
are [DEC-263](../project/04-decision-log.md#decisions)'s six manifest fields, with the manifest's
hash beside them. So a verifier takes a trusted start from either record in the journal itself
(§11; [workspace API spec](workspace-api.md) §4.8.1).

**`AnchorComputed`**: one anchor of §10.

| Member | Type | Meaning |
|---|---|---|
| `leaves` | `[{hash: digest, seq: integer, stream_id: stream_id}]` | Every stream's head at the anchor, one per stream, strictly ascending by `stream_id` bytes. The control stream's leaf is its head **before** this event, which §11's `anchor_self_mismatch` checks: rule 113 |
| `root` | `digest` | §10's root over `leaves`: rule 114 |
| `token` | `ref?` | The RFC 3161 timestamp token, stored as an artifact (§10), or `null` while the timestamping authority is unavailable; the outage is retried and journaled as a gap (§10). An anchor whose `token` is `null` is not a trusted start (below) |

**`SegmentExported`**: one segment of §6.2, shipped and locked.

| Member | Type | Meaning |
|---|---|---|
| `stream_id` | `stream_id` | The segment's stream: rule 116 |
| `first_seq`, `last_seq` | `integer` | Its first and last `seq` |
| `first_prev_hash` | `digest` | The first event's `prev_hash`: the trusted start of a range entered at `first_seq` (§11) |
| `last_hash` | `digest` | The last event's `hash`, which the next segment chains from |
| `file_sha256` | `digest` | SHA-256 of the segment file |
| `manifest_hash` | `digest` | The hash of the segment's manifest (DEC-263 item 3): rule 117 |

The manifest and its file are stored in the cold store, not in the journal's artifact store, so
both hashes are `digest`s, not `ref`s, and §11 check 6 does not look for them; the cold per-range
checks do (`segment_manifest_mismatch`, `segment_gap`).

**What a verifier reads.** The trusted start of a range of stream `s` entered at `n` is
`{from_seq: n, prev_hash: first_prev_hash}` of a `SegmentExported` of `s` whose `first_seq` is `n`,
or `{from_seq: n, prev_hash: hash}` of the leaf for `s` in an `AnchorComputed` **that has a
`token`** and whose leaf `seq` is `n − 1`. An anchor whose `token` is `null` is not a trusted start:
without the timestamp nothing outside the journal vouches for it, and a start the journal vouches
for alone is what [DEC-115](../project/04-decision-log.md#decisions) item 5 refused. A range is then
entered from genesis, from a `SegmentExported`, or from a stamped anchor. A later record that
supplies the missing token (the anchor stamp record, backlog) makes such an anchor a start; this
version has none. Every start is looked up among the workspace's own control-stream records only,
so a record that is absent and one of another workspace give the same refusal (workspace API
§4.8.1, DEC-767). The test vectors' `cold_records.trusted_starts` hold these cases.

**Consistency rules** (reason `schema`; the path is the member named):

113. `AnchorComputed`: `leaves` is non-empty (`payload.leaves`). Each leaf, in array order, with its
    clauses in this order, at `payload.leaves[i].<member>`: its `stream_id`'s `{workspace_id}`
    segment equals the envelope `stream_id`'s (`stream_id`); its `seq` is at least 1 (`seq`); and
    after the first leaf, its `stream_id` sorts after the previous leaf's by bytes (`stream_id`), so
    no stream has two leaves. Then, when there are leaves, one names the envelope's own control
    stream (`payload.leaves`): a control stream always holds its `StreamOpened` before any anchor, so an
    anchor that leaves its own stream out would dodge §11's `anchor_self_mismatch` by omission.
114. `AnchorComputed`: `root` is §10's root over `leaves`, recomputed at append (`payload.root`):
    leaf = SHA-256(0x00 ‖ canonical(leaf)), node = SHA-256(0x01 ‖ left ‖ right), split at the
    largest power of two below the count. An anchor whose leaves do not produce its root proves
    nothing ([DEC-265](../project/04-decision-log.md#decisions) item 2's `MalformedAnchor`).
115. `AnchorComputed`: `actor.kind` is `system` (`actor.kind`).
116. `SegmentExported`: `stream_id`'s `{workspace_id}` segment equals the envelope `stream_id`'s
    (`payload.stream_id`); `first_seq` is at least 1 (`payload.first_seq`); `last_seq` is at
    least `first_seq` (`payload.last_seq`); and `first_prev_hash` is 64 zeros exactly when
    `first_seq` is 1 (`payload.first_prev_hash`).
117. `SegmentExported`: `manifest_hash` is the SHA-256, as 64 lowercase hex, of the canonical JSON
    of DEC-263's six fields built from this record, `{stream, first_seq, last_seq,
    first_prev_hash, last_hash, file_sha256}` with `stream` its `stream_id` and every hash as 64
    lowercase hex (`payload.manifest_hash`), so the record and its manifest cannot disagree.
118. `SegmentExported`: `actor.kind` is `system` (`actor.kind`).

### 9.15 Alert and notice records ([DEC-720](../project/decisions/DEC-720.md))

The notice stream of §2 and v0.12, and the records that feed it ([notifications spec §5.5](notifications.md#55-records)):
the subject stream owner's `OwnerAlertSent`, and the dispatcher's `StreamOpened`, `NoticeIssued`,
and `NoticeAttempted` on `ntf:{workspace_id}`. Each is closed at schema version 1, as §9.6 closes
its records: every listed member is present, `null` only where the type is nullable, and any other
member is `schema` at that member. §9.1's types and report order apply. None of the four holds free
text: every string is an identifier, a closed vocabulary, or a provider's opaque token, so nothing
about an order, a position, a mandate, or an address can reach them (`AGENTS.md` rule 6,
notifications spec NT-1 and NT-2).

**Streams.** `OwnerAlertSent` is catalogued on the agent, account, and control streams, written by
that stream's owner in the subject's own batch (notifications spec §3.2, §5.1); on the scheduler and
notice streams it is `wrong_stream` at `event_type`. It names no configuration and is not a risk
input, so it carries no `risk_clock`. The notice stream admits only its `StreamOpened`,
`NoticeIssued`, and `NoticeAttempted`; every other catalogued type is `wrong_stream` there, so no
stream owner copies a fact from it and the dispatcher writes nothing else.

**Types**, beside §9.1's:

| Type | Values | Refused as |
|---|---|---|
| `notice_id` | Exactly 32 lowercase hexadecimal digits: notifications spec §4.2's random notice id. Never an event id, which is a `ulid` and carries its creation time | As `id` |
| `stream_id` | A stream id in §2's grammar, of one of the five stream types | As `id` |
| `opaque` | 1 to 256 characters, each U+0021 to U+007E: a provider's message id, which has no space and so cannot carry a sentence | As `id` |
| `kind` | Notifications spec §3.2's closed set of 33 kinds, below | As `id` |

**Kinds and their classes** (notifications spec §3.2, in its order):

| Class | Kinds |
|---|---|
| `action` | `approval_requested`, `approval_reminder` |
| `safety` | `risk_limit`, `kill_switch`, `agent_held`, `account_restriction`, `protection`, `exit_stalled`, `reconciliation`, `external_activity`, `account_state`, `data_feed_down`, `integrity_incident`, `credential_added`, `new_device`, `notification_address_changed`, `recovery_used`, `role_granted`, `member_deactivated`, `deprovisioned`, `break_glass`, `version_risk_increasing`, `delegation_added`, `connection_added`, `went_live`, `client_connected`, `channel_lost` |
| `info` | `daily_brief`, `delegation_ended`, `model_status`, `research_status`, `spend_cap`, `approval_closed` |

**`OwnerAlertSent`** (agent, account, or control stream)

| Member | Type | Meaning |
|---|---|---|
| `subject` | `ulid` | The subject event: rules 119 and 130 |
| `kind` | `kind`, except `approval_requested`, `approval_reminder`, and `channel_lost` | No alert causes those three (notifications spec §3.4); any of them is `non_canonical` at `payload.kind` |
| `owner_command` | `ulid?` | For a user's kill switch, the `OwnerCommandIssued` it carries out, on which the dispatcher de-duplicates (notifications spec §3.4): rule 120 |

**`StreamOpened`** on the notice stream. As on the agent stream (§9.1), the schema is chosen by the
stream type of `stream_id`.

| Member | Type | Meaning |
|---|---|---|
| `stream_type` | `notice` | |
| `workspace_id` | `id` | The subject: rule 121 |

**`NoticeIssued`** (notice stream, before the first send)

| Member | Type | Meaning |
|---|---|---|
| `notice` | `notice_id` | The id the payload and the link carry |
| `kind` | `kind` | |
| `class` | `action` \| `safety` \| `info` | Rule 122 |
| `cause` | `ulid` | The event the notice answers: an `OwnerAlertSent`, an `ApprovalRequested`, or for `channel_lost` this stream's own `NoticeAttempted` (notifications spec §3.4). A kill switch's owner command is reached through its alert's `owner_command`, never named as a cause. Rule 124 |
| `cause_stream` | `stream_id` | The stream `cause` is on: rule 125 |
| `recipients` | `[id]` | Opaque user ids (NT-2): rule 123 |

**`NoticeAttempted`** (notice stream, one attempt's outcome on one push channel)

| Member | Type | Meaning |
|---|---|---|
| `notice` | `notice_id` | |
| `recipient` | `id` | The opaque user id; never an address (NT-2) |
| `channel` | `email` \| `phone` \| `slack` \| `sms` \| `telegram` \| `web_push` | The mandate schema's push channels (notifications spec §4.1). The pull channels are never attempted: `ApprovalDelivered` and the journal itself are their record |
| `attempt` | `integer` | 1 for the first attempt on the channel: rule 126 |
| `status` | `delivered` \| `failed` \| `suppressed_quiet_hours` \| `deferred_quiet_hours` \| `abandoned` | `delivered` means the provider accepted the message, not that anyone read it |
| `reason` | `timeout` \| `rate_limited` \| `provider_error` \| `address_rejected` \| `auth_failed` \| `too_large` \| `recipient_not_permitted` \| `address_missing` \| `bounced` \| `complained` \| `unsubscribed` \| `not_pending` \| `retry_window_ended` (nullable) | Notifications spec §5.2's closed enum, with the receipts `bounced`, `complained`, and `unsubscribed` (§5.6) and the dispatcher's own `address_missing` (§5.1, [DEC-795](../project/decisions/DEC-795.md) item 7), then §5.3's two stops: rule 127 |
| `provider_message_id` | `opaque?` | The provider's id for the accepted message, which a later receipt names: rule 128 |
| `coalesced_into` | `opaque?` | For a notice combined into one message (notifications spec §5.4), that message's provider id: rule 129 |

A `NoticeAttempted` whose `reason` is `address_rejected`, `auth_failed`, `bounced`, `complained`, or
`unsubscribed` is the record that marks the address on that channel `unreachable` (notifications
spec §5.6) and causes its `channel_lost` notice; one whose `reason` is `address_missing` causes a
`channel_lost` notice once per address and marks nothing. Neither needs a member of its own, since a
separate flag could only disagree with the reason. `recipient_not_permitted` marks nothing and
causes nothing (§5.2).

**Consistency rules** (reason `schema` unless stated; the path is the member named):

119. `OwnerAlertSent`: `causation_id` equals `subject` (`causation_id`).
120. `OwnerAlertSent`: `owner_command` is `null` unless `kind` is `kill_switch`
     (`payload.owner_command`). It is allowed on every alert stream: the v0.12 row lets a stream
     owner's alert name the command ("if any"), and refusing it would refuse the batch that holds
     the `KillSwitchActivated` beside it (`AGENTS.md` rule 13).
121. Notice-stream `StreamOpened`: `stream_id` equals `ntf:{workspace_id}` (`stream_mismatch` at
     `stream_id`).
122. `NoticeIssued`: `class` is the class of `kind` in the table above (`payload.class`).
123. `NoticeIssued`: `recipients` is strictly ascending by bytes, so none is listed twice
     (`payload.recipients`). It may be empty: a recipient with no push address keeps the pull
     channels.
124. `NoticeIssued`: `causation_id` equals `cause` (`causation_id`).
125. `NoticeIssued`: `cause_stream` is a stream of this stream's workspace: an agent stream for
     `approval_requested` and `approval_reminder`, this notice stream for `channel_lost`, and an
     agent, account, or control stream for every other kind (`stream_mismatch` at
     `payload.cause_stream`), so a notice never answers another workspace's event (NT-10).
126. `NoticeAttempted`: `attempt` is at least 1 (`payload.attempt`).
127. `NoticeAttempted`: `reason` is non-null exactly when `status` is `failed` or `abandoned`. An
     `abandoned` attempt's reason is a stop: `not_pending` (an `action` notice whose approval
     stopped being pending) or `retry_window_ended` (a `safety` notice's 24 hours or an `info`
     notice's 6 hours ran out, or the one last send to a removed address whose vault entry is
     gone; notifications spec §5.1, §5.3); a `failed` one's is any other (`payload.reason`).
128. `NoticeAttempted`: `provider_message_id` is non-null exactly when `status` is `delivered` or
     `reason` is a receipt, `bounced`, `complained`, or `unsubscribed`
     (`payload.provider_message_id`).
129. `NoticeAttempted`: `coalesced_into` is `null` when `status` is `suppressed_quiet_hours` or
     `deferred_quiet_hours`, since nothing was sent (`payload.coalesced_into`). A combined message
     that failed or was abandoned keeps the link, so NT-6's oracle attributes every joined notice's
     outcome to its cause, recipient, and channel.
130. **Batch rule.** An `OwnerAlertSent`'s `subject` is the `event_id` of an earlier draft in the
     same `append` batch that is not itself an `OwnerAlertSent`, and no other `OwnerAlertSent` in
     the batch names the same subject (`payload.subject`, at the first alert that breaks it):
     notifications spec §3.2's "in the subject's own batch" and §3.4's "one subject, one kind",
     checked at append.

The first violation is reported in §9.1's order: unlisted members, the listed members in the order
given, then rules 119, 120, 122 to 124, and 126 to 129 (only on a well-typed payload), then
`artifact_refs` and `pii_refs`, then the subject rules 121 and 125. Rule 130 spans a batch, so it is
checked once every draft in the batch passes these, as rule 10's second clause is.

**Lifecycle.** A cause is issued once: a cause that already has a `NoticeIssued` is skipped, and on
restart the dispatcher issues every cause without one and attempts again every notice that has no
terminal attempt (notifications spec §5.1). A later receipt for a delivered message is a new
`NoticeAttempted` naming the same `provider_message_id`; it never retracts the earlier `delivered`
(notifications spec §5.5). Nothing on the notice stream is a risk input, and no trading path waits on it (NT-9).

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

**Per-range checks** read more than the one event they judge. Each is reported in one of two ways,
which `VerificationRun`'s `failure.seq` records ([§9.13](#913-records-access-export-and-verification-records-dec-780)
rule 111):

- **for the range**, with no `seq`: `anchor_root_mismatch`, `tsa_token_invalid`,
  `segment_manifest_mismatch`, and `segment_gap`;
- **at an event**, with that event's `seq`: `anchor_head_mismatch` (the event at each anchored
  `seq` exists with the anchored hash, reported at that `seq`) and every check listed below.

On the control stream ([§9.14](#914-anchor-and-segment-records-dec-783), [§9.13](#913-records-access-export-and-verification-records-dec-780)):

- `anchor_self_mismatch` — an `AnchorComputed` has a leaf for its own control stream, and that leaf
  names the event just before it: its `seq` is one less than the `AnchorComputed`'s own, and its
  `hash` is that event's `hash` (§10), reported at the `AnchorComputed`. The test vectors'
  `cold_records.range_checks` hold a case for each clause;
- `break_glass_cause_mismatch` — a `RecordsAccessed` whose actor is a `platform_operator` names, as
  its `causation_id`, an earlier `PlatformOperatorAction` on this control stream, so of the same
  workspace: the customer-approved break-glass action that opened its window (§7). Rule 108 checks
  at `append` only that the `causation_id` is non-null, since `append` cannot see earlier batches.
  A cause that names a later event or an event of another type fails, and so does one that names no
  event in a range that starts at seq 1, the full-chain run among them (below); each is reported at
  the `RecordsAccessed`. **Until DEC-261 item 9 closes `PlatformOperatorAction`,
  `append` refuses that record (`unknown_schema`), so no control stream holds one, and every
  operator read that `append` admits fails this check** wherever it is judged, always in the
  weekly full-chain run. It fails closed on purpose, as rules 109 and 110 do for an operator's
  export or verification. The test vectors' `records_access.range_checks` hold a case for each
  clause, the first of them the read `records_access`'s valid draft `read_by_an_operator`.

On an agent stream ([§9.1](#91-agent-stream-payload-schemas-dec-177)):

- `intent_action_mismatch` — an `IntentProposed` whose `causation_id` names a `DecisionMade` has
  that decision's action members (rule 10), and one whose `causation_id` names an
  `ApprovalRevalidated` has the action members bound in that approval's `ApprovalRequested`,
  reported at the `IntentProposed`. The second clause is checked, with vectors, once §9.1 closes
  the approval events' schemas; until then no range fails on it;
- `mode_event_mismatch` — a `KillSwitchActivated` whose `mode_event` is non-null names an earlier
  `AgentModeChanged` on this stream with reason `kill_switch`, reported at the
  `KillSwitchActivated`;
- `held_mismatch` — the owner's hold is carried, never dropped (§9.11, [DEC-672](../project/decisions/DEC-672.md),
  [DEC-673](../project/decisions/DEC-673.md)). Only `AgentModeChanged` records are read; any other
  event between them leaves the hold as it was. The hold carried is the **expected** one, derived
  from the reasons: `owner_hold` sets it, `owner_lift_hold` clears it, and every other record keeps
  it whatever it wrote. A hold or lift whose `held` contradicts its reason fails; any other
  version-2 `AgentModeChanged` whose `held` differs from the carried one fails; and no version-1
  `AgentModeChanged` follows a version-2 one. **The check anchors on the stored chain, not on the
  range:** the verifier is given an anchor that its caller derives from the stored chain before
  the range's trusted start, one of no version-2 `AgentModeChanged` before it, or the `held` the
  last one before it carried (from a hold, a lift, or a carried copy), or none when the caller
  cannot read that chain. A full chain is anchored on nothing before it. With no anchor, the
  range's first version-2 record that is not a hold or a lift fails closed, reported as having no
  anchor, and a version-1 record before it is not judged: every stream written before §9.11 is all
  version 1, and the full-chain run or an anchored range catches a version 1 after an unseen
  version 2. Reported at the first record that breaks it.

On a control or account stream ([§9.8](#98-connection-records-dec-800)):

- `connection_lifecycle_mismatch` — a record that breaks stream rule 66, 67, or 68, reported at
  that record.

Across the control stream and its account streams, in the full-chain run only (a range never
holds the other stream):

- `connection_cause_mismatch` — a control-stream `ConnectionEstablished` version 2 whose
  `causation_id` is not a `ConnectionChecked` on the account stream its `account_ref` names, for the
  same `connection_id`, with occasion `connect` (`reconnect` for a later establishment of the
  same id on the same control stream, after an earlier one of either version), every result
  `passed`, and `contract` listed for an MCP connection; the same for a
  `ConnectionCredentialRotated`, with occasion `reauthorize`, for a connection established on its
  own control stream; or an account-stream copy of either
  whose `causation_id` is not a control-stream record of its type with the same payload but
  `risk_clock`. Reported at the record. The test vectors' `connections.chains` hold a case for
  each.

A reference to an event before the range's trusted start is not checked by that range; the weekly
full-chain run checks every one, and there a `mode_event`, or an operator read's `causation_id`,
that names no earlier event fails. The test vectors' `agent_stream.range_verification` holds a case
for each for `mode_event`, and `records_access.range_checks` for the operator read's cause.

**Schedule:** the tail of every stream at startup and before each segment export; the full chain
weekly; results journaled as `VerificationRun`.

**On failure:** SEV-1. Account- or agent-stream failures pause the affected agents (the kill switch
still works); control-stream failures freeze mandate and deployment changes, and the freeze never
holds risk reduction: risk exits, protective orders, owner exits, and the kill switch at any scope
still work (`AGENTS.md` rule 13), and the workspace API still records its risk-reducing operations
([workspace API spec](workspace-api.md#2-invariants) API-7). Affected segments are
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
  from the canonical export, and each names its verifier digest, which identifies the canonical
  export ([DEC-265](../project/04-decision-log.md#decisions) item 3; `ExportCreated`, §9.13).

## 13. Open questions

1. Timestamping authority choice and fallback.
2. Whether hybrid customers may opt out of sending anchor roots to the global control plane.
3. Personal-data scanning approach for free-text artifacts.
4. Export deadline and customer-notification deadline values.
