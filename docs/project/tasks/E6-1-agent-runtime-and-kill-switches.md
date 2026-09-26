# Task: E6-1 and E6-5 the agent runtime skeleton and kill switches

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story; this one implements two that cannot be separated, because a kill switch is the one command an
agent runtime must honour before anything else it does (E6-5 is the first clause of E6-1's loop, not
a feature beside it). It opens M5: the runtime is the shape every later M5 crate plugs into, so this
brief fixes that shape before the gate, the builder, and the research agent are written.

## Story

- **Stories:** E6-1 and E6-5 ([backlog](../06-backlog-v1.md#e6-agent-runtime-and-risk))
- **Acceptance criteria (verbatim):**
  - E6-1 (Must): "As an operator, I want an agent to run a mandate continuously so that it trades
    without supervision." Neither story carries an *Accepted when* clause, so this brief sets the
    bar: **the core is a pure state machine whose state is a fold of journaled events, and a replay
    of any run reproduces that state and emits no new event** (the invariant table below), with every
    output an intent handed to a sink, never a broker call.
  - E6-5 (Must): "As an owner, I want kill switches per agent, connection, and workspace so that I
    can stop everything immediately." Bar: **the command is honoured from folded state alone**, the
    final mode is applied before anything else, every pending approval is cancelled, no intent that
    adds risk can follow, and the runtime never cancels or closes anything itself.
- **PRD / HLD / spec anchors:** [HLD](../../HLD.md) §5 (the agent runtime, its components,
  "Isolation and custom logic", "Durability", "Agent lifecycle"), §4 (the data plane), §6.A;
  [ADR-0001](../../adr/0001-engineering-setup.md) ES-02 (crates and layering), ES-06 (core state
  machines are `handle(&mut State, Input) -> Vec<Effect>`, one fenced `StreamWriter` per stream,
  effects run only after `Committed` or `AlreadyCommitted`, `IdGen` injected), ES-09 (typed errors
  with stable codes), ES-20 (in-process `IntentSink` and `TimerSource`; DEC-17 decided at the start
  of M5; DEC-16 before M7), ES-21 (determinism), ES-22 (spec anti-drift), ES-24 (the latency budget,
  benchmarked from M5); [journal spec](../../specs/journal.md) §2 (streams, single writers, copied
  cross-stream facts, "a user's kill switch is a command to the stream owners"), §3 (the envelope),
  §5.1 (the append protocol and fencing), §5.2 (write before acting, crash recovery), §8 (replay,
  snapshots, `fold_version`), §9 (the agent-stream catalogue); [mandate
  spec](../../specs/mandate.md) §2 (lifecycle), §2.2 (applying a version), §2.3 (the working
  universe as runtime state), §5.2 (inputs, the risk clock, determinism, MI-13), §5.5 (the
  agent-scoped kill switch), §5.9 (restrictions and the effective mode, MI-6), §5.10 (the events),
  §6.4 (approvals and the `skip` timeout), §10 (records); [trading domain
  spec](../../specs/trading-domain.md) §5.5 (kill switch), §5.6 (exit pricing), §7.4 (agent modes);
  [glossary](../../product/glossary.md) (deployment, workspace, kill switch, agent modes).
- **Decisions that apply:** DEC-08 (one process per agent deployment), DEC-05 and DEC-06 (reducing
  risk needs no approval; ambiguity resolves safe), DEC-16 and DEC-17 through ES-20 (DEC-17's
  recommendation is the ground this brief stands on and is still `Proposed (founder)`), DEC-72
  (ADR-0001), DEC-77 (brief, tests PR, implementation PR), DEC-79 (agents land their own changes;
  the founder's reserved list), DEC-80 (no plain comments), DEC-83 (a tests PR holds stubs only),
  DEC-85 (an uninterpreted input fails loudly), DEC-89 and ES-21 (exact arithmetic, ordered
  containers, no clock, no randomness), DEC-97 (the owner sets the envelope, the platform brings the
  ideas), DEC-103 (the Phase 1 thin slice), DEC-110 (every pending test fails on the stubs),
  DEC-111, DEC-117 to DEC-126 (the spec v0.6 answers this brief reads; still `Proposed (founder)`,
  and a veto reopens this brief), DEC-127 (the E4-2 loop this runtime's tick is the live analogue
  of), and DEC-131 (this story's interpretations, below).

## Scope

### Reference cases

**None move from pending to passing.** `crates/mandate-refcases/status.toml` does not change and
this story ships as a tests PR and an implementation PR with no status PR (the DEC-105 precedent).
Two case families look as though they belong here and do not:

- `trading_domain::RC-14`'s `kill_switch` variant needs the harness's `kill_switch` step, which
  `crates/mandate-refcases/src/trading_domain.rs` attributes to E6-5, but its expectations are
  `actions` (E7-2) and `agent_mode` (E6-9), both executor-side. The variant therefore turns green
  with the executor stories, not here, and this story does not interpret the step.
- The mandate suite's agent-flatten family `MC-F01` to `MC-F04` is a **plan** computed from open
  orders, agent positions, broker positions, the session, and the initiator. The M5 work graph gives
  family F to stream G (`mandate-risk`), and the 298 mandate cases have no Rust harness until stream
  F builds one. This story consumes the plan behind a port and asserts what it does with it.

### Stream boundaries

| Concern | Owner |
|---|---|
| The runtime loop, the agent stream, mode application, command routing, approvals' lifecycle, timers, recovery | **This story (stream I)** |
| The mandate document, validation, policy, change classification, risk state | Stream F (`mandate-spec`) |
| The binding risk gate, forced-flatten plans (family F), account rules, eligibility, conduct | Stream G (`mandate-risk`) |
| Autonomy classification (AUTO / ASK / DENY) and the order builder | Stream H (`mandate-builder`) |
| Theses, admission, the working universe's content | Stream J (E17 thin slice) |
| Cancelling orders, selling, `IntentReceived`, `GateDecided`, `OrderSubmitted`, reconciliation | M6 (`mandate-executor`, `mandate-alpaca`) |

Each of the first five arrives behind an injected port, so the four other M5 streams land without
editing this core. The last one is behind `IntentSink` and is never called directly: **the runtime
never talks to a broker** (AGENTS.md rule 12).

### Invariants touched

Each row gets a named test whose oracle computes the answer its own way.

| Clause or invariant | Test |
|---|---|
| ES-06, ES-21 the core is a pure state machine: `handle` is the only producer of effects and reads nothing but its arguments | `properties::two_runs_of_the_same_inputs_give_equal_effects`, and the crate's lint header |
| Journal §8, ES-21 state is a fold of events: folding the drafts a run journaled reproduces the run's state | `properties::folding_the_journaled_drafts_reproduces_the_live_state`, `hand::the_golden_journal_folds_to_the_committed_state` |
| Journal §5.2 a replay emits nothing: `fold` is effect-free and total over the catalogue | `properties::a_replay_of_any_run_emits_no_draft_and_no_intent`, `hand::a_restart_mid_run_journals_nothing_new` |
| Journal §5.2, AGENTS.md rule 5 journal before acting: every `Effect::Intent` is preceded in the same list by the `Effect::Journal` that records it, **or, for `Input::Started` alone, by a draft the fold saw committed before the call** | `properties::every_intent_effect_follows_the_draft_that_records_it`, `hand::a_proposal_journals_before_it_reaches_the_sink`, `hand::a_started_handoff_names_a_draft_the_fold_already_saw`, `properties::no_input_but_started_hands_an_intent_whose_draft_is_absent_from_the_list` |
| Journal §5.2 a crash between the append and the handoff re-hands the same intent and never re-journals it | `hand::a_restart_after_an_intent_committed_re_hands_it_without_re_journaling`, `properties::started_re_journals_nothing_but_the_startup_hold` |
| Review finding 2 a restart re-hands only what the folded mode permits, and drops an opening intent outside `normal` | `hand::a_restart_under_stopped_re_hands_no_opening_intent`, `hand::a_restart_under_exits_only_still_re_hands_an_exit`, `properties::no_re_handed_intent_is_one_the_mode_forbids` |
| Review finding 5 the startup hold is journaled before anything is re-handed, and lifts only on a reconciliation at or after the last submission | `hand::started_journals_the_startup_hold_before_its_first_handoff`, `hand::an_earlier_reconciliation_does_not_lift_the_startup_hold` |
| Journal §5.1 idempotency: a retry after `Unavailable` or `Ambiguous` derives the same `event_id` | `hand::a_retried_append_derives_the_same_event_id`, `properties::a_derived_event_id_is_a_function_of_epoch_head_and_ordinal` |
| Journal §5.1 a batch in doubt is retried with the same drafts, and no input is handled at an unresolved head | `hand::an_unresolved_append_is_retried_before_any_new_input`, `hand::a_tick_during_an_unresolved_append_does_not_change_the_drafts` |
| Journal §5.1 fencing: a new process increments `writer_epoch`, and a `Fenced` append stops the runtime instead of retrying | `hand::a_fenced_append_stops_the_runtime`, `hand::two_epochs_never_derive_one_event_id` |
| Journal §2 gapless `seq`: the fold rejects a gap, a repeat, and a stream it does not follow | `hand::a_gap_in_seq_fails_the_fold`, `hand::a_repeated_seq_fails_the_fold`, `properties::the_fold_rejects_every_out_of_order_sequence` |
| Journal §2 copied facts carry `causation_id` pointing at the originating event | `hand::a_copied_mode_change_points_at_the_originating_event`, `properties::every_copied_draft_cites_its_origin` |
| Mandate §5.2, MI-13 unjournaled ticks change nothing: extra ticks between inputs never change the journaled drafts | `properties::extra_ticks_never_change_the_journaled_drafts`, `hand::a_tick_that_changes_nothing_emits_no_effect` |
| Mandate §5.2 the only time the core knows is the risk-clock second on its input; durations are integer seconds credited by the state at the interval's start | `hand::a_deadline_is_measured_in_whole_seconds_of_risk_clock`, `hand::event_time_never_moves_a_deadline`, `properties::durations_match_the_interval_oracle` |
| Mandate §5.9, MI-6 the effective mode is the strictest of the copied mode, the lifecycle state, and the startup hold, and `AgentModeChanged` is journaled only when it changes | `properties::the_effective_mode_is_the_maximum_of_the_restriction_lattice`, `hand::a_restriction_that_lifts_while_another_is_active_does_not_restore_normal`, `hand::an_owner_resume_does_not_lift_the_copied_account_mode`, `hand::an_unchanged_mode_journals_nothing` |
| Trading §7.4, AGENTS.md rule 2 no mode change adds risk: no opening or increasing intent is ever proposed outside `normal` | `properties::no_opening_intent_is_proposed_outside_normal`, `hand::exits_only_still_proposes_an_exit` |
| Trading §5.5, mandate §5.5 the final mode is applied first: `stopped` for an owner kill switch, `paused` for an automated flatten, before the cancel and the sells | `hand::an_owner_kill_switch_applies_stopped_before_anything_else`, `hand::an_automated_flatten_applies_paused_first`, `properties::the_mode_draft_precedes_every_other_effect_of_a_kill_switch` |
| DEC-100, trading §5.5 every initiator maps to a mode and a purpose, and none returns an error | `hand::a_platform_operator_stop_applies_stopped_as_a_risk_exit`, `properties::every_initiator_yields_a_mode_and_never_an_error` |
| Trading §7.3 a broker-driven restriction is a copied mode change, never a kill switch | `hand::a_broker_restriction_arrives_as_a_mode_copy_not_a_flatten` |
| Mandate §5.8 a risk-limit flatten's restriction lifts only through the account stream | `hand::the_runtime_never_lifts_a_risk_limit_restriction_itself` |
| Journal §9, mandate §5.10, trading §5.5 an owner-initiated stop journals `OwnerExitRequested`, with the confirmed bid and the floor price, before the handoff | `hand::an_owner_kill_switch_journals_the_owner_exit_request_before_the_handoff`, `hand::an_unconfirmed_owner_exit_leaves_the_equity_sells_for_the_session` |
| E6-5 a kill switch cancels every pending approval, so nothing proposes afterwards | `hand::an_approval_that_arrives_after_a_kill_switch_proposes_nothing`, `hand::a_kill_switch_cancels_every_pending_approval`, `properties::no_intent_follows_a_kill_switch_in_any_input_order` |
| Mandate §5.9 entering `exits_only` or stricter cancels every pending approval, whatever put the runtime there | `hand::a_copied_exits_only_cancels_every_pending_approval`, `hand::an_owner_pause_cancels_every_pending_approval`, `properties::no_approval_outlives_the_mode_that_forbids_its_action` |
| E6-5 scope: agent, connection, and workspace each stop exactly their scope, and a runtime outside the scope is untouched | `hand::a_connection_kill_switch_stops_every_agent_on_that_connection`, `hand::a_workspace_kill_switch_stops_every_agent_in_the_workspace`, `hand::a_kill_switch_for_another_agent_changes_nothing` |
| Trading §5.5, AGENTS.md rule 13 an agent-scoped kill switch never uses cancel-all or close-position, and the runtime performs no cancel and no sell itself | `hand::the_runtime_never_emits_a_cancel_all_or_a_close_position`, `hand::a_kill_switch_hands_one_flatten_plan_to_the_sink` |
| AGENTS.md rule 13, trading §5.5 the kill switch is always available: it is honoured in every mode, needs no model state, and needs no approval | `properties::a_kill_switch_is_honoured_from_every_reachable_state`, `hand::a_kill_switch_with_no_model_output_still_stops_the_agent` |
| Trading §7.4 `stopped` is terminal: nothing resumes it and no intent ever follows | `hand::a_stopped_agent_is_terminal`, `properties::no_effect_after_stopped_proposes_an_intent` |
| Mandate §2.3, §5.9 an instrument outside the working universe or under `removed_instrument` is exits-only for the agent | `hand::a_removed_instrument_still_exits`, `hand::a_removed_instrument_proposes_no_opening` |
| Mandate §6.4 an approval binds quantity, limit price, and mandate version; a response under a changed version is skipped; `on_timeout` is always `skip` | `hand::an_approval_binds_the_quantity_and_the_version`, `hand::an_approval_under_a_changed_version_is_skipped`, `hand::an_approval_deadline_skips_the_action`, `properties::no_timeout_ever_acts` |
| Mandate §2.2 a risk-increasing version applies at the next evaluation with no `Unknown` orders; a reducing one applies at once and cancels pending approvals | `hand::a_risk_increasing_version_waits_for_a_safe_point`, `hand::a_reducing_version_applies_at_once_and_cancels_approvals` |
| HLD "Agent lifecycle" after a replay the runtime is `paused` until the account stream reconciles at or after its last submission | `hand::a_restart_stays_paused_until_the_account_reconciles`, `hand::an_unexplained_position_keeps_the_runtime_paused` |
| AGENTS.md rule 4 LLMs produce opinions, never orders: no model output reaches an intent except through the injected builder, and a missing or stale output proposes nothing | `hand::a_stale_model_output_proposes_nothing`, `properties::every_intent_carries_the_builder_output_that_produced_it` |
| AGENTS.md rule 1, rule 12 the runtime's gate call is a dry run that can only narrow: a dry-run deny skips the proposal, and no verdict the port returns authorises anything | `hand::a_dry_run_deny_skips_the_proposal`, `properties::no_effect_reaches_a_broker` |
| AGENTS.md rule 6 notification payloads carry opaque IDs and generic text only | `hand::a_notification_carries_only_opaque_ids`, `properties::no_notification_payload_holds_an_instrument_or_a_price` |
| DEC-85 an uninterpreted input fails loudly, naming the owning story | `hand::an_unknown_event_type_fails_the_fold`, `hand::an_unhandled_command_names_its_story`, `properties::every_catalogue_event_is_interpreted_or_named` |
| ES-09 every error variant has a stable `code()` and the set is exhaustive | `hand::every_error_code_is_stable_and_unique` |
| ES-21 ordered containers, no floats, no clock, no randomness; `fold_version` is 1 and the golden journal is committed | `hand::the_fold_version_is_pinned_with_the_golden_journal`, and the crate's lint header |
| ES-20 the transport is not a core input: the same run is produced whether the shell was notified or polled | `properties::the_same_events_give_the_same_run_whatever_woke_the_shell` |

### Oracles

`crates/mandate-runtime/tests/properties.rs` holds three implementations that share no code with
the crate:

1. **A shadow fold.** It rebuilds mode, restrictions, working universe, pending approvals,
   outstanding intents, and deadlines from the emitted drafts alone, as a `BTreeMap` of fields keyed
   by name, reading the canonical bytes of each draft rather than any core type. It is what
   `folding_the_journaled_drafts_reproduces_the_live_state` compares against, so a core that keeps
   state the journal does not carry fails it.
2. **A restriction lattice.** The effective mode is recomputed as the maximum of a separately
   written ordering over an independently accumulated restriction set (mandate §5.9), so a core that
   confuses `paused` with `exits_only`, or clears a latched restriction, fails.
3. **An interval accumulator.** Deadlines and durations are recomputed by summing whole seconds
   between consecutive inputs, crediting each interval to the state at its start (mandate §5.2), so
   a core that credits at the end, or resets a timer on every input, fails.

Every property first compares the **number** of emitted effects with the oracle's, so no property
can pass on an empty effect list (the E4-1 lesson). Each oracle is shown to fail on a seeded bug
before it is trusted; the planted-bug table below is that evidence.

### Crates

- **In scope, new:** `mandate-runtime`. Recommended `layer = 6`, `pure = true`,
  `safety_critical = true`, `allowed_external = ["thiserror"]`, with a CODEOWNERS line like the other
  safety-critical crates. It depends on `mandate-num`, `mandate-time`, `mandate-canon`,
  `mandate-journal`, and `mandate-accounting`. Layer 6 is the lowest layer that can see the journal
  (2) and, later, `mandate-spec` (3), `mandate-risk` (4), and `mandate-builder` (5); it sits beside
  `mandate-executor` rather than above it, which is exactly why the executor arrives behind
  `IntentSink` and not as a dependency. `xtask/layers.toml` and `CODEOWNERS` are founder-owned, so
  the entry is the one item this brief cannot settle alone (Decisions needed 1).
- **In scope, touched:** `Cargo.toml` workspace members (the founder's file: one line),
  `docs/dependencies.md` "Used by" cells for `thiserror` and `proptest` (no new dependency).
- **Out of scope:** `mandate-journal` and `mandate-journal-pg` (used as they are; the append protocol
  is E5-1's and E5-3's), `mandate-accounting`, `mandate-sim`, `mandate-marketdata`, `mandate-cli`,
  `crates/mandate-refcases/` and `status.toml`, every file under `docs/specs/`, `schemas/`,
  `reference/`, and `fixtures/`.
- **New dependencies:** none. `thiserror` and `proptest` are registered.
- **Safety-critical:** yes (every path here is on the AGENTS.md list: kill switches, autonomy
  routing, crash recovery, notification payloads). DEC-77 sequence: this brief, then the tests PR
  (crate, stubs, pending tests, planted-bug report), then the implementation PR in which test files
  change only by deleting `#[ignore = "pending E6-1"]` and `#[ignore = "pending E6-5"]` lines.
- **Size budget:** 400 non-generated lines per PR (ES-13); the tests PR may exceed it for test code
  and says how it splits.

## Data shapes

The core is one module tree with **two** entry points and no constructor that reads the world.

```rust
/// The whole of what the runtime knows, derived from journaled events and nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeState { /* every field private */ }

/// Replays one journaled event into the state. Total over the catalogue, effect-free.
pub fn fold(state: &mut RuntimeState, event: &FoldedEvent) -> Result<(), RuntimeError>;

/// The live step (ES-06), the only producer of effects. `ports` carries the injected pure
/// collaborators, which is how ES-06's "`IdGen` is injected" reaches a function with no `self`.
pub fn handle(
    state: &mut RuntimeState,
    input: Input,
    ports: &Ports<'_>,
) -> Result<Vec<Effect>, RuntimeError>;
```

Recovery is not a third entry point. After a replay the shell calls `handle` with
`Input::Started`, which journals the startup hold **before** it re-hands anything and never
re-journals an intent; an earlier draft of this brief had a separate `resume`, which could not
journal that hold at all (review finding 5).

`RuntimeState` carries the folded position of every stream it follows, so a restart re-reads from
`seq` 1 (or a verified snapshot, journal §8) and re-derives those positions; **no durable state lives
outside the journal**. `fold_version` is a crate constant, 1 for this story, bumped whenever fold
output changes (ES-21), and a golden journal in `crates/mandate-runtime/tests/` pins the pair.

```rust
pub enum Input {
    /// The process has folded every followed stream and taken `writer_epoch`. The only input that
    /// may re-hand an intent, and the only one that journals the startup hold.
    Started(WriterEpoch),
    /// An event tailed from a stream the runtime follows, delivered in `seq` order. The only
    /// variant that carries a stream position.
    Journal(FoldedEvent),
    /// The scheduler's tick: a whole-second risk clock and nothing else.
    Tick(RiskClock),
    /// A market-data or news observation the runtime will journal as `ObservationRecorded`.
    Observation(Observation),
    /// A signal model's output, journaled as `ModelOutputRecorded` with its `as_of` and
    /// `expires_at`, from which freshness is judged at the risk-clock second (mandate §8.1).
    ModelOutput(ModelOutput),
    /// An approver's response, journaled as `ApprovalResponded`, then applied or skipped (§6.4).
    ApprovalResponse(ApprovalOutcome),
    /// A command addressed to this deployment: kill switch, pause, resume, stop, owner exit.
    Command(Command),
}

pub enum Effect {
    /// Append to the agent stream. The shell runs these first and in order.
    Journal(EventDraft),
    /// Hand a proposed intent to the `IntentSink`, after the draft that recorded it committed.
    Intent(IntentHandoff),
    /// Arm or cancel one keyed timer through the `TimerSource`.
    Timer(TimerRequest),
    /// An owner alert: opaque IDs and generic text only (AGENTS.md rule 6).
    Notify(NotificationRef),
}

pub enum Command {
    KillSwitch { scope: KillScope, initiator: Initiator, confirmation: Option<OwnerConfirmation> },
    Pause,
    Resume,
    Stop,
    OwnerExit { instrument: InstrumentId, confirmation: Option<OwnerConfirmation> },
}

pub enum KillScope { Agent(AgentId), Connection(ConnectionId), Workspace(WorkspaceId) }

/// Who ordered the stop. `Broker` is deliberately absent: a broker-driven restriction is an
/// account-state change (trading §7.3) that reaches the runtime as a copied `AgentModeApplied`,
/// not a kill-switch command (review finding 1).
pub enum Initiator { Owner, RiskLimit, PlatformOperator }
```

The ports come in two kinds, and the difference matters because only one kind can affect what
`handle` returns.

**Core-side, pure, passed in `Ports<'_>`.** Each is a pure function of its arguments, so `handle`
stays deterministic and a test injects fixed implementations:

```rust
/// Deterministic event identity (ES-06, ES-21). The Phase 1 implementation is a derivation, not a
/// generator: same (epoch, head, ordinal) in, same id out.
pub trait IdGen { fn event_id(&self, epoch: WriterEpoch, head: Seq, ordinal: u32) -> EventId; }

/// Stream G's gate, called as a dry run only. A verdict can narrow, never authorise.
pub trait GateDryRun { fn check(&self, proposal: &Proposal) -> DryRunVerdict; }

/// Stream H's classification and sizing, and stream F's mandate view.
pub trait OrderPlan { fn plan(&self, view: &MandateView, inputs: &SignalInputs) -> Option<Proposal>; }

pub struct Ports<'a> {
    pub ids: &'a dyn IdGen,
    pub gate: &'a dyn GateDryRun,
    pub plan: &'a dyn OrderPlan,
}
```

**Shell-side, effectful, never called by the core.** The core only *describes* these in its effect
list; the shell performs them in the order the list gives (ES-20):

```rust
/// Where an intent goes, driven from `Effect::Intent`. In-process in Phase 1; the executor's
/// adapter implements it.
pub trait IntentSink { fn hand(&mut self, handoff: &IntentHandoff) -> Result<(), SinkError>; }

/// Keyed timers, driven from `Effect::Timer`. Deadlines live in folded state; this only arms and
/// cancels, so a lost timer costs a late evaluation and never a lost deadline.
pub trait TimerSource { fn arm(&mut self, id: TimerId, at: RiskClock); fn cancel(&mut self, id: TimerId); }
```

Everything crossing a port is exact: quantities and money are `mandate-num` types, timestamps are
`mandate-time`, and drafts are `mandate_journal::Draft` built from `mandate_canon::Value`. Every
collection in the crate is a `BTreeMap` or `BTreeSet` (ES-21).

Errors are one `thiserror` enum with a stable `code()` per variant (ES-09), including
`Unimplemented { story }`, the body of every stub in the tests PR, and `NotInterpreted { what,
story }`, the loud failure DEC-85 requires.

## The tick

The live analogue of E4-2's backtest loop (DEC-127 item 2), with one order fixed so that nothing
depends on arrival luck:

1. **Fold what the input carries.** Only `Input::Journal` carries a stream position, and it is
   folded in `seq` order; every other variant is handled at the folded position as it stands, which
   is why a tailer that is behind delays a decision rather than deciding on stale state.
2. **Apply the input** itself: an observation, a model output, or an approval response is journaled;
   a command is routed; a tick advances the risk clock.
3. **Recompute the effective mode** from the restriction set (mandate §5.9) and journal
   `AgentModeChanged` only if it changed (MI-6).
4. **Expire what is due** at this risk-clock second: approval deadlines (`ApprovalTimedOut`, then
   the action is skipped), thesis horizons, and lift delays. Durations are sums of whole seconds over
   the intervals between inputs, each credited by the state at the interval's start, so a tick that
   crosses no boundary emits nothing (MI-13).
5. **Decide**, only in `normal` for anything that opens or increases and in `exits_only` for
   anything that reduces: ask `OrderPlan` for a proposal, journal `DecisionMade` with the dry-run
   verdict and the autonomy classification, then either propose (`IntentProposed`, AUTO), ask
   (`ApprovalRequested`, ASK), or record the refusal (DENY). A proposal with no fresh model output,
   or in an instrument outside the working universe, is not made.
6. **Emit** the effect list: every draft before the handoff it authorises, then timers, then alerts.

The shell runs the list in order, appends through one fenced `StreamWriter`, and acts only on
`Committed` or `AlreadyCommitted` (ES-06). It stops at the first append that is neither and discards
the rest of the list; the next start re-derives from the fold, which is why discarding is safe.

ES-24's budget is measured over step 5 alone: p99 under 1 ms from input to draft bytes, excluding
the append and the sink. The implementation PR reports the measurement; the tests PR does not
benchmark.

## Modes, restrictions, and the effective mode

The runtime **never derives a mode from risk limits**. `AgentModeApplied` originates on the account
stream, whose owner is the executor, and the runtime copies it into the agent stream as
`AgentModeChanged` with `causation_id` pointing at the original (journal §2). The effective mode it
acts on is the strictest (`normal` < `exits_only` < `paused` < `stopped`) of three sources, so it is
always at least as strict as what the account stream says:

| Source | What it is | How it lifts |
|---|---|---|
| The copied account-stream mode | The strictest of the agent's risk and account restrictions (mandate §5.9), computed by the executor | Only by another `AgentModeApplied`; the runtime never lifts it |
| The deployment's lifecycle state | An owner `Pause` or `Stop`, or a kill switch (HLD "Agent lifecycle", mandate §2) | `Resume` for a pause; `stopped` never lifts |
| `awaiting_reconciliation` | The runtime's own startup hold: the replay is not yet confirmed by the reconciliation that trading §11 runs at startup | A `ReconciliationRun` on the account stream at or after the runtime's last observed submission. A *mismatch* pauses the agent through the account stream instead, and only owner acknowledgment with step-up lifts that (§11) |

**Entering `exits_only` or stricter cancels every pending approval**, whatever put the runtime
there: a copied `AgentModeApplied`, an owner `Pause`, or a kill switch. Mandate §5.9 requires the
cancellation and the runtime is the agent stream's single writer, so the runtime is what journals
`ApprovalCanceled` (see Decisions needed 5 for the wording conflict this resolves). Anything still
wanted is re-proposed at the next evaluation under the new state, which is the §2.2 pattern.

| Mode | What the runtime still does |
|---|---|
| `normal` | Everything the mandate, the dry run, and the autonomy rules allow |
| `exits_only` | Proposes risk-reducing and protective intents; requests no approval that could open |
| `paused` | Proposes nothing but the re-placement of protection before it expires; keeps folding, keeps timers for protection, honours the kill switch |
| `stopped` | Terminal: folds, answers queries, and proposes nothing, ever. Only a new deployment (a new agent stream) starts fresh |

An instrument under `removed_instrument` or outside the working universe is exits-only for that
instrument alone (mandate §2.3, §5.9): protection stays, exits are still proposed, and model outputs
are still requested so a discretionary exit can be built.

## Kill switches (E6-5)

A kill switch is a **command to the stream owners** (journal §2), so the runtime and the executor each
journal `KillSwitchActivated` in their own stream. The runtime's half, in order, as one effect list
from one `handle` call:

1. **The final mode first** (trading §5.5, mandate §5.5). It is fixed by the initiator, and every
   initiator has a mapping, because a kill switch that returned an error instead of stopping would
   be the worst bug in this crate:

   | Initiator | Final mode | Purpose of the sells | Authority |
   |---|---|---|---|
   | `Owner` (kill switch or stop) | `stopped` | `owner_exit`: paced only by the participation caps, and outside the regular session it sells equities through the exit price ladder once the owner has confirmed the displayed bid, bid size, and a floor price; without that confirmation equity sells wait for the session | Trading §5.5, §5.6; mandate §6.1 |
   | `RiskLimit` (a ladder `flatten_and_pause` rung, the daily-loss flatten, the lifetime floor) | `paused` | `risk_exit`: exempt from conduct controls, equities sell only in the regular session with protection left in place, crypto sells go immediately | Trading §5.5; mandate §5.5 |
   | `PlatformOperator` (a stop, the global kill switch, an acceptable-use action) | `stopped` | `risk_exit`, on the automated schedule above | DEC-100, journal §9 `PlatformOperatorAction`; the pricing is the conservative default, see Decisions needed 6 |

   `Broker` is not an initiator. A broker-driven restriction is account state (trading §7.3) whose
   agent effect (`paused` or `exits_only`) reaches the runtime as a copied `AgentModeApplied`, and
   the runtime treats it as a mode change, not a flatten.

   A `RiskLimit` flatten's restriction belongs to the account stream, so **the runtime never lifts
   it**, whichever limit latched, and each lifts differently: `drawdown_flatten` by owner
   acknowledgment once flat (§5.8); `daily_loss` automatically once a new risk day has started,
   `daily_breach_min_s` has passed, and the owner has acknowledged a `flatten_and_pause` (§5.4);
   `lifetime_floor` only when a version raising `max_loss_from_allocation` applies with equity
   strictly above the new floor, never by acknowledgment at all (§5.7). Each lift reaches the runtime
   the same way: journaled by its owner, then copied to the agent stream as another
   `AgentModeChanged`. `stopped` is terminal and lifts for no one (interpretation 12).

   The `AgentModeChanged` draft is the first effect in the list; nothing else can precede it.
2. **`OwnerExitRequested`**, for an `Owner` initiator only: the scope or instrument, the bid and bid
   size shown, whether the owner confirmed them, the floor price, the opaque user, and the step-up
   evidence (journal §9, mandate §5.10, trading §5.5). Without this draft the executor cannot price
   an owner exit outside the regular session, so it precedes the handoff rather than trailing it.
3. **`KillSwitchActivated`** on the agent stream with the scope and the initiator, and nothing else
   in the payload that a notification could leak.
4. **Cancel every pending approval** (`ApprovalCanceled`), because an approval that outlives the
   kill switch is an order after the stop. A response that arrives later is dropped with a journaled
   refusal.
5. **Disarm every timer** except those protection needs.
6. **Hand exactly one flatten to the sink**, carrying the plan stream G computes from the agent's
   sub-ledger (mandate spec `MC-F01` to `MC-F04`): the client order IDs to cancel, the sells, and
   the deferred sells. The runtime does not compute the plan, does not cancel, does not sell, and has
   no way to reach `cancel-all` or `close-position`, which exist only on the account-wide path and
   are the executor's (AGENTS.md rule 13).

**Outstanding intents.** An intent is *outstanding* from the moment its `IntentProposed` commits
until the account stream carries a terminal outcome for its intent id: an `IntentReceived` followed
by a `GateDecided` denial, an `OrderAbandoned`, or a terminal order state. A kill switch does not
retract an outstanding intent, because the runtime cannot: whatever reached the executor is covered
by the flatten plan's cancel list, and the executor's own binding gate denies an opening intent under
the mode the account stream now carries. What the runtime does control is that it never hands such an
intent again: **`Input::Started` re-hands only what the folded mode permits**, which is the mode
table's own permission set rather than one flat list:

| Folded mode | Re-handed |
|---|---|
| `normal` | Every outstanding intent |
| `exits_only` | Exits, protective intents, and an unfinished flatten |
| `paused` | The re-placement of protection before it expires, and an unfinished flatten; a discretionary exit is held, which rule 13 allows only `paused`, `stopped`, an `Unknown` order in the instrument, or the broker to do |
| `stopped` | An unfinished flatten, and nothing else. The flatten survives because the kill switch is always available and `stopped` is the mode a kill switch sets, so a `stopped` agent that could not finish its own flatten could never be flattened at all |

So an opening intent outstanding at a restart under `exits_only` or stricter is dropped, not re-sent
(review finding 2). The agent-stream catalogue has no event for an intent the runtime abandons, so
the record is the intent's own draft plus the mode change that precedes the drop; adding one would be
a journal-spec change (Decisions needed 7).

**`Started` also re-arms the timers**, from the deadlines the fold carries: approval deadlines, lift
delays, thesis horizons, and the protection re-placement a `paused` agent still owes. A timer is a
hint about *when* to look, never the deadline itself (the deadline is folded state), so a lost timer
costs a late evaluation and `Started` is what restores the schedule after a crash.

| Scope | What it reaches |
|---|---|
| Agent | This deployment. Other agents on the same account and the owner's unattributed shares are untouched |
| Connection | Every deployment on that connection, each getting exactly the agent-scoped treatment above |
| Workspace | Every deployment in the workspace, the same way |

**The executor is the binder on the wider scopes.** At connection or workspace scope each runtime
hands its own agent-scoped flatten, and the executor is what turns those into the account-wide
`cancel-all` and `close-position` of trading §5.5, once per account, de-duplicating by intent id.
Every handoff is therefore idempotent by design: the sink is at-least-once, so a runtime that hands
the same flatten twice after a restart must cost nothing.

A runtime whose deployment is outside the scope emits nothing at all. The switch is honoured in every
mode, from folded state alone, with no model output, no approval, and no gate call, which is the one
property that has to hold from **every** reachable state rather than from a named one.

## Crash recovery and the fold

- **Recovery is a replay, then one `handle` call.** The process starts, increments `writer_epoch` on
  its agent stream (journal §5.1), folds every stream it follows from `seq` 1 or a verified snapshot,
  and then calls `handle(state, Input::Started(epoch), ports)`. `fold` produces no effects, so the
  replay itself cannot re-send anything. `Started` is the one input that may re-hand an intent, and
  the only draft it may emit is the `AgentModeChanged` for the startup hold: it never re-journals an
  intent, an observation, a decision, or an approval.
- **The startup hold is journaled before anything is re-handed.** `Started`'s first effect is the
  `AgentModeChanged` draft that records `awaiting_reconciliation` when it changed the mode, and the
  re-handoffs follow it in the same list. That ordering is what makes the hold visible in the journal
  at all, and it is what the shadow-fold oracle reads (review finding 5).
- **An intent whose append committed is re-handed, not re-proposed** — and only if the folded mode
  permits it. The fold sees `IntentProposed` with no terminal outcome and leaves it outstanding;
  `Started` re-hands exits, protective intents, and an unfinished flatten, and drops an opening
  intent outside `normal`. A *terminal outcome* for an intent id is an `IntentReceived` followed by a
  `GateDecided` denial, an `OrderAbandoned`, or a terminal order state on the account stream. The
  sink is at-least-once by design: the intent ID is the `IntentProposed` `event_id`, so the executor's
  own fold deduplicates a repeat (journal §2, §5.2).
- **An intent whose append is in doubt** (`Ambiguous`, `Unavailable`) is retried with the same
  drafts, and the derived `event_id` makes the retry `AlreadyCommitted` rather than a second event.
  No new input is handled while a head's append is unresolved, or a recomputed batch would collide
  with its own derived IDs.
- **A `Fenced` append stops the process.** A newer epoch owns the stream, so this process is a
  ghost; it exits rather than retrying, and the effects it computed are void.
- **Nothing resumes trading until the account reconciles.** The runtime holds
  `awaiting_reconciliation` (mode `paused`) until the account stream carries a `ReconciliationRun` at
  or after its last observed submission, which is the reconciliation trading §11 runs at startup and
  the reconciliation restriction mandate §5.9 names. HLD's lifecycle sends `Recovering` to `Paused` on
  anything unexplained, and on a mismatch §11's own rule takes over: the agent is paused through the
  account stream and only owner acknowledgment with step-up lifts it. Holding before the
  confirmation arrives, rather than only on a mismatch, is stricter than either document and never
  adds risk (AGENTS.md rule 3).
- **`fold_version` is 1**, with a golden journal committed beside it (ES-21). A change to fold output
  bumps it and the golden journal is regenerated in the same change.

## The process model and the DEC-17 recommendation

One process per agent deployment (DEC-08), with OS-level CPU and memory limits (HLD §5). Phase 1
follows the coordinator's DEC-17 recommendation, which is still `Proposed (founder)`: Postgres
`LISTEN`/`NOTIFY` plus journal tailing on the workspace's Postgres, with intents and timers behind
`IntentSink` and `TimerSource` (ES-20).

The recommendation touches the shell and nothing else, because of one rule this brief fixes:

> **The journal is the channel; a notification is only a hint.** Every input the core reads is an
> event the shell tailed from a stream in `seq` order, a tick, or a command. No notification payload
> reaches the core. A lost notification therefore costs latency, never correctness, and the shell
> also polls on a bounded interval.

- **Inbound:** the shell `LISTEN`s on a channel per followed stream (`acct:…`, `ctl:…`, `clock:…`)
  and, on a notification or a poll, tails `mandate-journal-pg` from its folded position.
- **Outbound:** the runtime journals `IntentProposed` on its agent stream and then hands it to the
  sink. In one process the sink is a direct call. Across processes the Phase 1 sink **notifies and
  lets the executor tail**: it writes nothing of its own, `NOTIFY`s the account's channel, and the
  executor reads the agent stream from its folded position and journals `IntentReceived`. The durable
  path is the journal, so a dropped notification is caught by the next tail.
- **Priority:** kill-switch and risk-exit commands arrive on a priority channel the shell reads
  first (ES-06), and the order in which inputs are handled is what gets journaled.

**If the founder chooses NATS JetStream instead**, the sink publishes to a subject and the shell
consumes one, the poll becomes a consumer's redelivery, and a new external dependency joins
`docs/dependencies.md` and the shell crate's `allowed_external`. The core, all five ports (the three pure ones
and the two the shell drives), every type above, and every test in this story are untouched, because
none of them names a transport.
`mandate-runtime` stays `pure = true` either way: tokio, sqlx, and any client library live in the
shell crate, which is impure, layer 7, and arrives with M6 rather than here.

## Interpretations (recorded as DEC-131)

Each item fixes how code realises a rule the spec already states. Item 1 is the only one that needs
the founder, because it edits founder-owned files.

1. **Crate, layer, and criticality.** A new safety-critical `mandate-runtime`, `pure = true`,
   `layer = 6`, `allowed_external = ["thiserror"]`, with a CODEOWNERS line. Layer 6 is the lowest
   layer that can see `mandate-journal` (2) and the M5 crates `mandate-spec` (3), `mandate-risk` (4),
   and `mandate-builder` (5); it is the same layer as `mandate-executor`, so the executor cannot be a
   dependency, which is what makes `IntentSink` a trait rather than a call (ES-20). `xtask/layers.toml`
   and `CODEOWNERS` are founder-owned: the tests PR adds the entry and the founder may veto it.
2. **Two entry points, and recovery is an input.** `fold` replays a journaled event and produces no
   effects; `handle` is the only producer of effects (ES-06), and it takes the injected pure ports.
   Recovery is `Input::Started`, whose only permitted draft is the startup hold's `AgentModeChanged`
   and which never re-journals an intent. This is the whole of crash safety: a replay cannot re-send,
   and recovery cannot re-journal. Splitting `fold` from `handle` is what a single entry point used
   for both replay and live input cannot give, because a replayed `Input` would emit the effects the
   original run already ran. An earlier draft made recovery a third entry point, `resume`, that was
   forbidden to draft at all; it could then not journal the startup hold, which no fold and no oracle
   could see.
3. **The M5 crates arrive behind ports.** The gate, the order builder, the autonomy classification,
   the mandate view, and the research agent's outputs are injected traits, so streams F, G, H, and J
   land without editing this core and the four briefs run in parallel. Waiting for them would
   serialise M5's largest block for no design benefit.
4. **The runtime's gate call is a dry run.** The binding gate runs on the account stream, whose owner
   is the executor (journal §9 puts `GateDecided` there; AGENTS.md rules 1 and 12). The runtime's
   verdict only narrows: a dry-run deny skips the proposal and a dry-run allow authorises nothing. A
   swapped or broken port can therefore make the runtime propose less, never more.
5. **The journal is the channel; a notification is a hint.** No transport payload is a core input;
   every event arrives by tail in `seq` order. A lost notification costs latency, not correctness,
   and the founder's DEC-17 answer changes only the shell.
6. **Event identity is derived, not random.** The injected `IdGen`'s Phase 1 implementation derives
   the `event_id` from the stream ID, the `writer_epoch`, the expected head, and the draft's ordinal
   in the batch. A retry after `Unavailable` or `Ambiguous` therefore re-derives the same ID and the
   append answers `AlreadyCommitted` (journal §5.1), which removes the "re-query by `event_id` before
   acting" round trip. The epoch is in the derivation so that a fenced writer's retry can never
   collide with the new writer's ID at the same head, which would return `IdempotencyConflict` and
   hide the `Fenced` the old process needs to see. A ULID's time component carries no meaning
   (journal §3), so a derived ULID is a conforming one, and uniqueness holds because a stream's
   (epoch, head, ordinal) is unique. A derived ID puts one obligation on the shell: **a batch whose
   outcome is in doubt is retried with the same drafts**, and no new input is handled at a head whose
   append is unresolved (journal §5.1 says `Unavailable` is safe to retry with the same drafts). If
   the runtime instead re-folded and recomputed at the same head, a tick that expired a deadline in
   the meantime would produce different drafts under the same derived ID and the append would answer
   `IdempotencyConflict` for ever. The retry is therefore part of the contract, asserted on the
   in-memory shell the tests drive.
7. **Effects are ordered and write-before-acting is structural.** Within one list every
   `Effect::Intent` follows the `Effect::Journal` that records it, and the shell runs the list in
   order, stopping at the first append that is neither `Committed` nor `AlreadyCommitted` and
   discarding the rest. The property is asserted on the list, not on the shell, so it cannot be lost
   to a future shell. **`Input::Started` is the one exception, and it is stated rather than
   implicit:** its handoffs re-send intents whose `IntentProposed` committed in an earlier run, which
   is the whole point of re-handing rather than re-proposing, so for `Started` alone the rule reads
   "preceded by the draft that records it, **or** by a draft the fold saw committed before the call".
   Every other input obeys the unqualified form. An earlier draft of this brief asserted the
   unqualified rule over all inputs, which correct recovery would have failed — so the tests PR would
   have had to weaken the property quietly, which is worse than having no property (review round 2,
   finding 1).
8. **A tick that changes nothing emits nothing.** The core evaluates every tick, and durations are
   sums of whole seconds over the intervals between inputs, each credited by the state at the
   interval's start (mandate §5.2), so inserting ticks never changes a journaled draft (MI-13). The
   risk-clock second on the input is the only time the core knows: `event_time` and `recorded_at` are
   never read for timing, and nothing reads a clock.
9. **Mode is copied, never derived from limits.** `AgentModeApplied` originates on the account stream
   and the runtime copies it as `AgentModeChanged` with `causation_id` (journal §2). The effective mode
   is the strictest of three: that copy, the deployment's lifecycle state (an owner pause, stop, or kill
   switch), and `awaiting_reconciliation`. The runtime is therefore never less strict than the account
   stream, it never lifts what the account stream set, and `AgentModeChanged` is journaled only when the
   mode changes (MI-6). The last two are not in mandate §5.9's restriction list, which is the executor's
   risk restrictions; see Decisions needed 4.
10. **The kill switch's order is fixed and its scope is exact.** The final mode is the first effect
    (`stopped` for an owner switch or stop, `paused` for an automated flatten; item 21 gives every
    initiator), then `OwnerExitRequested` for an owner-initiated stop, then `KillSwitchActivated`, then
    every pending approval cancelled, then timers disarmed, then exactly one flatten handed to the sink. A deployment outside the scope emits nothing. The runtime computes
    no plan, cancels nothing, sells nothing, and has no path to `cancel-all` or `close-position`
    (trading §5.5, AGENTS.md rule 13).
11. **Cancelling approvals is part of the kill switch, not a tidy-up.** An approval that outlives a
    kill switch is an order after the stop; a response arriving afterwards is dropped with a journaled
    refusal. This is the failure the planted-bug table's first row exists for.
12. **`stopped` is terminal.** The core keeps folding so the stream stays readable and queries stay
    answerable, and proposes nothing ever again. A restart of a stopped deployment resumes as stopped;
    only a new deployment, which is a new agent stream, starts fresh (HLD's lifecycle).
13. **Recovery holds `paused` until the account reconciles.** After a replay the runtime carries its
    own `awaiting_reconciliation` (mode `paused`) until the account stream shows a `ReconciliationRun`
    at or after its last observed submission: the reconciliation trading §11 runs at startup, and the
    reconciliation restriction mandate §5.9 already names. On a mismatch §11's rule takes over and only
    owner acknowledgment with step-up lifts the pause, which the runtime never does itself. Holding
    before the confirmation arrives, rather than only on a mismatch, is stricter than either document
    and never adds risk. `Input::Started` journals the hold's `AgentModeChanged` as its first effect,
    so the hold exists in the journal before anything is re-handed.
14. **Approvals live in the core; delivery does not.** The core creates requests, binds quantity,
    limit price, and mandate version, applies responses, and applies the `skip` timeout (mandate §6.4),
    because the timeout is what makes an unattended loop safe. Channels, quiet hours, step-up
    evidence, and two-approver routing are M7's. A response under a changed mandate version is
    skipped, not re-priced.
15. **Version application waits for a safe point when it adds risk.** A risk-increasing version
    applies at the next evaluation with no `Unknown` orders for the agent; a reducing or neutral one
    applies at once, cancels pending approvals, and re-proposes under the new version (mandate §2.2).
    Latched limits are never cleared by a version change.
16. **An uninterpreted input fails loudly** (DEC-85): an event type, payload field, or command the
    core does not interpret returns `NotInterpreted { what, story }` naming the owning story, never a
    silent no-op. The fold is total over the catalogue in exactly this sense.
17. **Read positions are derived, not stored.** The folded position of every followed stream is part
    of the folded state, re-derived by replay, so nothing durable lives outside the journal and a
    restart cannot mistake an old event for a new one. Snapshots (journal §8) are an optimisation a
    later story adds; they change no result.
18. **Fencing at startup, and a `Fenced` append is fatal.** The process increments `writer_epoch`
    before it appends anything, and an append answering `Fenced` stops the process rather than
    retrying, because a newer epoch owns the stream (journal §5.1).
19. **Notifications carry opaque IDs and generic text only** (AGENTS.md rule 6). `NotificationRef`
    cannot hold an instrument, a quantity, a price, or thesis content: it holds the subject event's
    ID and a message key, and a test walks every payload the core can build to prove it.
20. **Determinism is asserted, not assumed.** `BTreeMap` and `BTreeSet` only, no floats, no clock, no
    randomness, exact `mandate-num` arithmetic, `fold_version` 1 with a committed golden journal, and
    a property that two runs of the same inputs give equal effect lists (ES-21).
21. **Every initiator has a final mode**, because a kill switch that errors instead of stopping would
    be this crate's worst defect: `Owner` and `PlatformOperator` give `stopped`, `RiskLimit` gives
    `paused` (trading §5.5, mandate §5.5, DEC-100). `Broker` is not a kill-switch initiator at all: a
    broker-driven restriction is account state (trading §7.3) that arrives as a copied
    `AgentModeApplied`. A `RiskLimit` flatten's restriction lifts only through the account stream by
    owner acknowledgment (mandate §5.8); the runtime never lifts one, and `stopped` lifts for no one.
22. **`Input::Started` re-hands only what the folded mode permits**, per mode rather than as one
    list: every outstanding intent in `normal`; exits, protective intents, and an unfinished flatten in
    `exits_only`; protection re-placement and an unfinished flatten in `paused`, a discretionary exit
    being held as rule 13 permits `paused` to; and in `stopped` an unfinished flatten alone, which
    survives because the kill switch is always available and `stopped` is the mode a kill switch sets.
    An opening intent still outstanding at a restart under `exits_only` or stricter is dropped rather
    than re-sent, because whatever already reached the executor is covered by the flatten plan's cancel
    list and by the executor's binding gate. An intent is outstanding until the account stream carries a
    terminal outcome for its id: an `IntentReceived` then a `GateDecided` denial, an `OrderAbandoned`, or
    a terminal order state. `Started` also re-arms every timer from the deadlines the fold carries; a
    timer is a hint about when to look and never the deadline itself.
23. **Entering `exits_only` or stricter cancels every pending approval**, whatever put the runtime
    there: a copied `AgentModeApplied`, an owner `Pause`, or a kill switch. Mandate §5.9 requires the
    cancellation; the agent stream has a single writer, so the runtime is what journals
    `ApprovalCanceled` (Decisions needed 5). Anything still wanted is re-proposed at the next
    evaluation under the new state, as §2.2 does for a version change.
24. **An owner-initiated stop journals `OwnerExitRequested` before the handoff**, carrying the scope or
    instrument, the bid and bid size shown, whether the owner confirmed them, the floor price, the
    opaque user, and the step-up evidence (journal §9, mandate §5.10, trading §5.5). Without it the
    executor cannot price an owner exit outside the regular session, so it is part of the kill-switch
    list rather than a later record.

## Planted bugs

Eighteen, each to be seeded alone in a throwaway implementation of the stubs (kept out of the tests PR
per DEC-83), run, and reverted. The tests PR reports the result for each; a row whose bug is not
caught means the test is wrong, not the bug.

| Planted bug | Must be caught by |
|---|---|
| A kill switch leaves a pending approval, so a response arriving afterwards proposes an order while the agent is `stopped` | `hand::an_approval_that_arrives_after_a_kill_switch_proposes_nothing`, `hand::a_kill_switch_cancels_every_pending_approval` |
| A restart re-journals `IntentProposed` for an intent whose append already committed, so the executor receives two intents for one decision | `hand::a_restart_after_an_intent_committed_re_hands_it_without_re_journaling`, `properties::started_re_journals_nothing_but_the_startup_hold`, `properties::a_replay_of_any_run_emits_no_draft_and_no_intent` |
| The mode lattice orders `paused` below `exits_only`, so a paused agent proposes an opening order: a mode change that adds risk | `properties::the_effective_mode_is_the_maximum_of_the_restriction_lattice`, `properties::no_opening_intent_is_proposed_outside_normal` |
| A restriction that lifts clears the whole set rather than its own entry, restoring `normal` while a latched restriction is still active | `hand::a_restriction_that_lifts_while_another_is_active_does_not_restore_normal` |
| The kill switch journals `KillSwitchActivated` before applying the mode, so a decision in the same list runs while the agent is still `normal` | `hand::an_owner_kill_switch_applies_stopped_before_anything_else`, `properties::the_mode_draft_precedes_every_other_effect_of_a_kill_switch` |
| An agent-scoped kill switch hands a plan carrying the account-wide `cancel-all`, so another agent's orders and the owner's shares are touched | `hand::the_runtime_never_emits_a_cancel_all_or_a_close_position`, `hand::a_kill_switch_for_another_agent_changes_nothing` |
| The intent effect is emitted before the draft that records it, so a crash between them sends an order the journal never recorded | `properties::every_intent_effect_follows_the_draft_that_records_it`, `hand::a_proposal_journals_before_it_reaches_the_sink` |
| `IdGen` mints a fresh ULID on every call, so a retry after `Ambiguous` appends a second `IntentProposed` | `hand::a_retried_append_derives_the_same_event_id`, `hand::two_epochs_never_derive_one_event_id` |
| The shell handles the next input before an unresolved append is retried, so a recomputed batch collides with its own derived IDs and every retry answers `IdempotencyConflict` | `hand::an_unresolved_append_is_retried_before_any_new_input`, `hand::a_tick_during_an_unresolved_append_does_not_change_the_drafts` |
| A duration is credited by the state at the end of each interval, so inserting ticks changes when a deadline fires | `properties::extra_ticks_never_change_the_journaled_drafts`, `properties::durations_match_the_interval_oracle` |
| The fold accepts a `seq` gap and keeps going, so a missed event silently changes the state a run resumes from | `hand::a_gap_in_seq_fails_the_fold`, `properties::the_fold_rejects_every_out_of_order_sequence`, `properties::folding_the_journaled_drafts_reproduces_the_live_state` |
| An unknown event type folds as a no-op instead of failing, so a later story's event passes unnoticed | `hand::an_unknown_event_type_fails_the_fold`, `properties::every_catalogue_event_is_interpreted_or_named` |
| An owner alert includes the instrument and the quantity so the message reads better | `hand::a_notification_carries_only_opaque_ids`, `properties::no_notification_payload_holds_an_instrument_or_a_price` |
| A `PlatformOperator` kill switch falls through the initiator match and returns `NotInterpreted`, so the global kill switch errors instead of stopping the agent | `hand::a_platform_operator_stop_applies_stopped_as_a_risk_exit`, `properties::every_initiator_yields_a_mode_and_never_an_error` |
| A restart re-hands an outstanding opening intent although the folded mode is `stopped`, so a stopped agent's order reaches the executor after the stop | `hand::a_restart_under_stopped_re_hands_no_opening_intent`, `properties::no_re_handed_intent_is_one_the_mode_forbids` |
| A copied `AgentModeApplied(exits_only)` leaves pending approvals standing, so an approval granted afterwards proposes an opening order in an exits-only agent | `hand::a_copied_exits_only_cancels_every_pending_approval`, `properties::no_approval_outlives_the_mode_that_forbids_its_action` |
| An owner kill switch omits `OwnerExitRequested`, so the executor has no confirmed bid or floor price and an after-hours owner exit either waits or prices unbounded | `hand::an_owner_kill_switch_journals_the_owner_exit_request_before_the_handoff` |
| The startup hold is applied to the state but its `AgentModeChanged` is not journaled, so a replay of the journal shows a `normal` agent that was actually held | `hand::started_journals_the_startup_hold_before_its_first_handoff`, `properties::folding_the_journaled_drafts_reproduces_the_live_state` |

## Decisions needed

1. **The `mandate-runtime` entry in `xtask/layers.toml` and `CODEOWNERS`** (founder-owned files).
   Recommendation: `layer = 6`, `pure = true`, `safety_critical = true`,
   `allowed_external = ["thiserror"]`, plus `/crates/mandate-runtime/ @kunwarshivam`. ADR-0001 ES-02's
   Phase 1 crate list names `mandate-spec`, `mandate-risk`, `mandate-builder`, `mandate-executor`, and
   `mandate-alpaca`, and does not name a runtime crate, because ES-02 was written when the runtime was
   assumed to be the executor's process. This brief separates them: the executor owns the account
   stream and the broker, the runtime owns the agent stream and the loop, and they are two writers of
   two streams (journal §2). If the founder prefers one crate, the whole of this brief lands inside
   `mandate-executor` unchanged except the crate name, and the layer question disappears; the
   separation is the recommendation because a single crate would put the broker's client and the
   agent's loop under one lint header and one review surface.
2. **DEC-17 itself stays `Proposed (founder)`.** This brief is built on the coordinator's
   recommendation (Postgres `LISTEN`/`NOTIFY` plus journal tailing). Interpretation 5 is what makes the
   choice cheap: if the founder picks NATS JetStream, the shell crate and one `docs/dependencies.md`
   row change, and the core, the ports, and every test in this story do not. No veto reopens this
   brief; it reopens only the shell story in M6.
3. **The shell crate's own entry, later.** The impure shell (tokio, the Postgres client or a NATS
   client) needs its own `xtask/layers.toml` row at layer 7 when M6 wires the executor and the
   connector. This brief does not add it; it is named here so the founder sees it coming rather than
   as a surprise in an M6 PR.

4. **Mandate §5.9's restriction list does not name a runtime-local state.** It lists the risk
   restrictions and the trading spec's account, external-activity, reconciliation, and rate-limit ones,
   all of which the executor computes on the account stream. This brief needs two states that no
   account stream can know: the owner's pause or stop of a *deployment*, and the startup hold before
   the reconciliation trading §11 runs at startup has confirmed the replay. Recommendation, and what
   the brief does meanwhile: keep §5.9 as the list of risk restrictions the executor owns, treat the
   owner's pause and stop as the deployment lifecycle HLD's "Agent lifecycle" already defines, and read
   the startup hold as the reconciliation restriction §5.9 already names, held from startup rather than
   only on a mismatch. The runtime's effective mode is then the strictest of the three, which is never
   less strict than the account stream's. A sentence in mandate §5.9 or trading §7.4 saying that a
   runtime may hold a stricter mode of its own would settle it; either is a spec change, so it is the
   founder's.
5. **Who journals `ApprovalCanceled`.** Mandate §5.9 says "the executor cancels the agent's working
   opening orders and pending approvals", but journal §2 and §9 make the whole `Approval*` family the
   agent runtime's, on the agent stream, with one writer per stream. Reading: the executor cancels the
   **orders**, the runtime cancels the **approvals** on its own stream, and the mode change that
   triggers both carries the `causation_id`. The alternative, a second writer to the agent stream,
   contradicts journal §2 outright. Recommendation: amend §5.9's sentence to say "the executor cancels
   the agent's working opening orders and the runtime cancels its pending approvals".
6. **A platform-operator flatten's pricing is not stated.** Trading §5.5 gives pricing for an
   *automated* flatten (mandate limits) and for an *owner* one, but DEC-100's platform-operator stop
   and global kill switch are neither: the operator is not the owner and cannot confirm a bid.
   Recommendation, and the conservative default the brief uses: `stopped` for the mode, because an
   operator stop is terminal, with the automated pricing schedule, because nothing may sell equities
   outside the regular session without an owner's confirmed bid and floor price. Both directions are
   the safer one.
7. **No agent-stream event records an intent the runtime abandons.** When a restart drops an
   outstanding opening intent because the folded mode forbids it (interpretation 22), the record is the
   intent's own `IntentProposed` plus the mode change that precedes the drop. The account stream has
   `OrderAbandoned` for an order; the agent stream has nothing for an intent. Recommendation: leave it
   as is for this story, since the pair of events makes the drop derivable, and consider an
   `IntentAbandoned` in the journal spec's agent-stream catalogue when the executor story shows whether
   a reviewer of one stream alone needs it.

Nothing under `docs/specs/`, `schemas/`, `reference/`, or `fixtures/` changes in any PR of this story:
items 4 to 7 are recorded here and in DEC-131 rather than fixed by editing a spec, and the brief
proceeds on the conservative reading of each (AGENTS.md rule 9).

## Not done

- **No connector and no broker call.** Cancelling orders, selling, `OrderSubmitted`,
  `BrokerExchangeRecorded`, reconciliation, and every Alpaca path are M6 (E7-1 to E7-5). The runtime's
  output stops at `IntentSink`.
- **No escalation service.** The core creates, cancels, times out, and applies approvals; delivery
  channels, quiet hours, step-up evidence, two-approver routing, and the durable-wait engine are M7
  and DEC-16. Phase 1 re-arms journaled deadlines from the fold, `Input::Started` being the input that
  does it after a restart (ES-20's first option).
- **No risk gate, no order builder, no autonomy rules, no mandate validation, and no research
  agent** in this crate: streams F, G, H, and J own them and arrive behind the ports.
- **No flatten plan.** Family `MC-F01` to `MC-F04` is stream G's; this story asserts what the runtime
  does with a plan, not what the plan is.
- **No shell.** No tokio, no sqlx, no `LISTEN`/`NOTIFY`, no process supervision, no OS limits, and no
  WebAssembly plug-ins (HLD §5's user-supplied logic). The tests PR ships the core and an in-memory
  shell used only by tests.
- **No snapshots.** Replay runs from `seq` 1; journal §8's snapshots are a later optimisation that
  changes no result.
- **No reference case moves and no `status.toml` change** (see Scope).
- **No benchmark in the tests PR.** ES-24's p99 is measured and reported by the implementation PR.
- **No live or paper run.** Nothing in this story connects anywhere (AGENTS.md rule 8).

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-runtime
cargo xtask ci pending
MANDATE_BASE_REF=$(git merge-base HEAD origin/main) cargo xtask ci spec-guard
cargo mutants -p mandate-runtime
```

## Stop conditions

Stop and write a DEC proposal instead of continuing if any of these happen:

- the spec is ambiguous or seems wrong;
- a new dependency seems necessary;
- a test would have to be weakened, skipped, or deleted;
- anything would deviate from an accepted decision.

## Definition of done

- [ ] An agent runs its mandate continuously from journaled state alone: the core folds, ticks,
      decides, proposes, and recovers, with every output an intent handed to a sink.
- [ ] Kill switches at agent, connection, and workspace scope are honoured from every reachable
      state, apply the final mode first, cancel every pending approval, and reach nothing outside their
      scope.
- [ ] Tests came first; every invariant above has a named test whose oracle computes the answer its
      own way and was shown to fail on a planted bug.
- [ ] New state changes emit journal events: `ObservationRecorded`, `ModelOutputRecorded`,
      `DecisionMade`, `IntentProposed`, the `Approval*` family (`ApprovalRequested`,
      `ApprovalResponded`, `ApprovalTimedOut`, `ApprovalCanceled`), `AgentModeChanged`,
      `OwnerExitRequested`, and `KillSwitchActivated`, all on the agent stream (journal §9).
- [ ] Docs updated: this brief, DEC-131, the feature map's "Agent runtime and kill switches" entry,
      and the tracker's M5, Stories, Claims, and work-graph rows.
- [ ] `cargo xtask check` is green (summary in each PR), and every pending test fails on the stubs.
- [ ] Each PR description is complete (see the PR template).
