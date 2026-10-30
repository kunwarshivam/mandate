# intelligence

DEC-2200 to DEC-2299. The rules in force:

- **Every hosted model call goes through the model-gateway service**, one framed
  canonical-JSON exchange per call; research on staging calls OpenRouter only through it, after
  the meter is journaled (DEC-2265, DEC-2257, DEC-2397).
- **The meter is the journaled stream**: written through mandate-store, one writer per workspace,
  failing closed; the per-agent daily cap and the deployment's model-spend cap come from the
  deployed mandate; the gateway holds research to the mandate's daily cap (DEC-2258, DEC-2259,
  DEC-2207, DEC-2272).
- **The research run is a bounded agent loop** (Claude Code for trading): a driver crate with a
  trigger queue and step machine, dispatches with reads and thinks under caps, a refused turn ends
  its run `error` and withdraws its pin; no run starts in `exits_only` (DEC-2203–2209, DEC-2261).
- **Cell and CI research runs over recorded bundles** and the gateway's recorded fake; a recorded
  entry pins the loop and its own system text; a script the entry could not serve refuses the load
  (DEC-2210, DEC-2263, DEC-2266).
- **Sources are allowlisted** (version 3 records who approved each source); the research source
  refuses every fact it cannot read; Alpaca news is on the allowlist; the news reader polls the
  workspace's registered instruments, journals each stored item on its own stream, keeps no raw
  response, and holds a round whose receipt runs ahead of trusted time (DEC-2276, DEC-2214,
  DEC-2394, DEC-2278–2292).
- **Data and artifacts**: one central fetcher writes customers' stored bars and the platform key
  never enters a customer cell; the gateway stores the runtime's artifacts in a create-only,
  capped store; the §4.3 gate runs inside the gateway service on staging (DEC-2284, DEC-2279,
  DEC-2216).
- The owner assigns the research agent's model per run kind from the vetted registry; the desk
  runs owner and explain runs read-only, citing every claim (DEC-2282, DEC-2240–2243).


<a id="dec-2201"></a>

## DEC-2201: mandate-journal registers §9.19, append judges its stream rules, and the research catalogue reads the registration

| | |
|---|---|
| **Status** | Accepted (agent; a reversible engineering reading under DEC-79 that only refuses more) |
| **Date** | 2026-10-17 (UTC) |
| **Decided by** | An agent (Claude Code, intelligence lane I2), under DEC-79 |
| **Story or claim** | E15-8's code half (DEC-998 item 11); [claim #1531](https://github.com/kunwarshivam/mandate/issues/1531) |

### Context

Journal spec v0.46 §9.19 (DEC-998, #1473) closes the agent stream's `ModelInvocationRecorded` and
adds the model gateway's meter stream, `meter:{workspace_id}`, with `StreamOpened`,
`MeterReserved`, and `MeterSettled` and rules 146 to 158. DEC-998 item 11 leaves
`mandate-journal`'s registration to its own code change, written tests first. Until it lands:

- an agent-stream `ModelInvocationRecorded` is refused `unknown_schema`;
- `meter:` streams do not parse;
- `mandate-gateway` keeps its meter in process (DEC-998 item 8);
- `mandate-runtime`'s `JournalCatalog` answers `false` for the candidates' verdicts, so agent
  harness spec §6.2's verdicts precondition never holds (DEC-1120 item 10).

Rules 153 and 158 are the journal's first stream rules that `append` itself must judge. §9.19 says
"`append` reads the stream's earlier records". Every rule before them is judged on one draft or one
batch, and §9.8's connection rules are judged by their writers and by verification, not by
`append`.

### Decision

1. **The registration.** `mandate-journal` closes, at schema version 1:
   - the agent stream's `ModelInvocationRecorded`, with rules 146 to 152;
   - the meter stream's `StreamOpened` (rule 154), `MeterReserved` (rules 155 and 156), and
     `MeterSettled` (rules 155 and 157).

   `StreamType::Meter` parses `meter:{workspace_id}`, and §9.13's stream names accept it. The
   catalogue admits the two meter records on the meter stream alone. Rules 72 to 75 are one
   function, shared by both streams' records; rule 152's exception to rule 74 is passed to it. The
   control stream's rules 71 and 76 stay the compiler's alone. Each of the 189 vectors
   (`model_gateway`: 4 base, 32 valid, 153 invalid drafts) parses or is refused with its reason at
   its path.
2. **Stream rules at append.** `check_stream(earlier, batch)` judges rules 153 and 158 from the
   stream's start.
   - It reads only records of the judged draft's own stream. So another agent's record of the same
     call, or an agent-stream record named as a reservation, never counts.
   - It reports the first breaking draft by its index in the batch.

   Both backends call it under the stream's head, after the fence and head checks and before
   sealing, and only when a draft of the batch is one the rules judge (`reads_stream`).
   - `MemoryJournal` reads its own rows.
   - The Postgres journal reads the stream's rows of the three types, ordered by `seq`, in the
     append's transaction under the head's row lock, and checks each as a stored row first.

   The conformance suite appends all 11 stream vectors through both backends, one record per append
   and as one batch.
3. **The catalogue the shell reads.** `mandate_journal::closes(event_type, version, stream)` says
   whether the journal closes a record there.
   - `JournalCatalog` answers the candidates' verdicts from `closes("ModelInvocationRecorded", 1,
     Agent)`, never from a constant.
   - It still answers `false` for the invalidation verdict, which has no record until E19-5. So
     `Preconditions::read(&JournalCatalog)` still does not hold, and no research run starts in any
     deployment until E19-5's registration changes that one answer.
4. **The audit stream list** names the meter stream (`mandate-audit`'s `StreamType::Meter`), so an
   auditor lists, pages, and verifies spend like any other stream (DEC-998 item 3).
5. **Not changed here.**
   - The spec's own text, which ES-22 keeps out of a code PR. Agent harness spec §6.2 already says
     the verdicts precondition "holds once `mandate-journal` registers it".
   - `mandate-gateway`'s in-process meter (DEC-998 item 8, lane I1): moving the gateway's meter onto
     the journaled stream is lane I1's change.
   - §11's cross-stream verifier checks (DEC-998 item 9).

### Rationale

The registration is the code half DEC-998 promised, read from the spec and its vectors with no new
reading of its own. The one choice is where rules 153 and 158 run.

The spec puts them in `append`, so a second record of a call is refused before it is stored. Once
stored, it could bring a late or substituted output into the stream (inference §3.4) or a second
reservation into spend (rule 158). Leaving them to verification alone would store the break first.

Reading only the three record types keeps the cost to batches that hold one of them. Reading under
the head lock means two writers that race on one stream cannot both pass rule 153 with the same
call: fencing gives the stream one writer at a time, and the head lock orders their appends.

### Alternatives considered

- **Judge rules 153 and 158 only in verification**, as §9.8's connection rules are. Rejected: §9.19
  places them in `append`, and stored breaks are what they exist to prevent.
- **Keep `JournalCatalog` a constant and flip it to `true` here.** Rejected: a constant can drift
  from the journal. Reading `closes` makes the precondition follow the registration itself.
- **Index the call ids in a side table** for rule 153 in Postgres. Deferred: a query by stream and
  type is enough at this volume, and a schema migration is its own change.

<a id="dec-2204"></a>

## DEC-2204: The research driver is its own crate, a trigger queue and a step machine; the shell wires it in after R0

| | |
|---|---|
| **Status** | Accepted (agent; reversible engineering decisions under DEC-79, each at least as strict as the rule it implements) |
| **Date** | 2026-10-17 (UTC) |
| **Decided by** | An agent (Claude Code, intelligence lane I2), under DEC-79 |
| **Story or claim** | DEC-996 item 2's run-path hook; ADR-0005 delivery slice 5; [claim #1531](https://github.com/kunwarshivam/mandate/issues/1531) |

### Context

DEC-996 item 2 leaves the research loop's run-path hook for after R0: the shell's research driver. It is a task that calls `handle_research`, stores each step's drafts before handing any request on, retries an unresolved append with the same input, runs the worker, and builds the `ResearchContext`.

The places the shell would hook it in are R0 files, which no lane may edit until R0 closes (DEC-967 item 2):
- the run cycle beside `production_cycle` (DEC-968 item 6);
- `tracer.rs`'s `perform`;
- `mandate-agentd`'s host.

Agent harness spec v0.2 §6.9 (lane AH, #1540, DEC-2203) makes the research run agentic, with these run rules:
- A run's kind (`ideate` or `monitor`) and tool menu are fixed at its start, from the trigger and folded state alone.
- At most one run is in flight per agent.
- An `owner` or `explain` run goes ahead of a queued scheduled run.
- A mode change cancels the run in flight.
- Event triggers wait for a mandate spec change (E19-14), because triggers are envelope fields.

### Decision

1. **A new crate, `mandate-research-driver`**, at layer 7: above `mandate-runtime`, below `mandate-shell`.
   - It is pure and safety-critical, and touches no R0 file.
   - It reads no clock and does no I/O. The shell hands it time, the journal and the worker.
   - The shell wires it in after R0 closes, as its own change, with the constructor beside `production_cycle`, `mandate-agentd`'s host, and DEC-1120 item 9's `correlation_id`.
2. **The trigger queue** (`TriggerQueue`) only arbitrates, per agent:
   - **One in flight:** at most one run in flight (HI-8).
   - **Order:** owner and explain runs first, in arrival order, then the scheduled run. None interrupts the run in flight, which never delays trading (HI-7).
   - **Coalescing:** schedule ticks coalesce into one queued run, and the same owner question or approval is queued once.
   - **Bound:** owner and explain runs are capped at `max_answers`. The scheduled run has its own slot, so they never crowd it out.
   - **Pause and stop:** in `paused` or `stopped` no run starts, and what was queued is dropped.
   - **Mode changes:** any change from the mode a run started in cancels it, which is §6.9's `mode` end.

   The core decides the rest: whether a scheduled run is due, its kind, and its menu. The queue takes no event trigger until E19-14 adds them to the mandate.
3. **The step machine** (`Driver`) sends each research input to the core through `Core`. `RuntimeCore` is `handle_research` plus the two folds. Then:
   - It appends the step's drafts as one batch.
   - It checks the reply is exactly those drafts, by `event_id` and in order.
   - It folds every stored event back.
   - Only then does it hand the worker its requests in the core's order, and return the other effects for the shell to run.

   So no request is acted on, and no notice names a record, before the record commits (`AGENTS.md` rule 5, HI-2). Three outcomes are not a plain commit:
   - **An unknown append** holds the driver until the shell retries the same input. The core answers that retry with the same drafts, and the journal's idempotency settles it (journal spec §5.1). The agent stream has one writer, so the core holds every trading input behind that batch too, exactly as it does behind a trading batch in doubt. **The shell retries `Driver::pending()` before it hands the core any other input**, so trading waits only for that retry.
   - **A refused append** stored nothing. `mandate_runtime::abandon_refused_research` (new) forgets the doubted batch, only when its input was research's, and drops the run in flight. Research halts in this process until a restart. Trading inputs, the kill switch among them, are never held behind a batch that can never commit (`AGENTS.md` rules 3 and 13). A test through the real core takes a tick and the kill switch right after a refusal.
   - **A reply that is not the batch**, or a committed event the core cannot fold back, means the core no longer matches the journal. The driver halts with `Diverged`, and the shell restarts the agent process, which rebuilds the core from the journal (§5.4).

### Rationale

A crate of its own keeps the driver clear of every R0 file without waiting for R0, and keeps it pure, so every ordering rule has a test. The shell's later change is then wiring alone.

The two traits (`Core`, `JournalWriter`) let every journal outcome be tested on fakes. A differential test then runs one whole research run through `RuntimeCore` and through the runtime's own test loop, and finds the same records, ids, order and requests. A second runs an unknown append and its retry through the real core.

The independent review found that a halt left the core's doubted batch set, which would refuse every trading input for the life of the process. That is why a refusal now abandons the research batch, and a divergence restarts the process.

The queue was first written for the design input's event-triggered monitoring runs. It was cut to §6.9 when the spec deferred event triggers. The core owns kind and menu selection, so a second copy in the driver would drift.

### Alternatives considered

- **A module in `mandate-shell`.** Its `lib.rs` sits beside `production_cycle`, which DEC-968 treats as R0. Rejected until R0 closes, and then unnecessary.
- **Halting research without abandoning the batch.** Rejected: the core would hold trading behind a batch that can never commit.
- **Restarting the process on a refused research append.** Rejected as heavier than needed: a refused batch stored nothing, so forgetting it is exact, and only research stops.
- **Keeping queued runs across a pause.** Rejected: a run queued before the pause could start against a state the owner paused.

### Follow-ups

- The runtime's turn and run inputs, keyed on §6.9 once #1540 merges, including the scheduled kind and its menu in the core.
- The shell's wiring after R0:
  - a `JournalWriter` over the agent stream's appender;
  - a `Worker` over `mandate-research-run` and the gateway;
  - the `ResearchContext` from the folds;
  - retrying `pending()` first;
  - an operator alert on a halt;
  - a process restart on `Diverged`.
- Event triggers, once E19-14's mandate spec change lists them with their debounce and rate limit.

<a id="dec-2205"></a>

## DEC-2205: Until agentic monitoring runs, no research run starts in `exits_only`, and a run that leaves `normal` mid-call judges nothing

| | |
|---|---|
| **Status** | Accepted (agent; a reversible engineering reading under DEC-79 and DEC-176 that only removes) |
| **Date** | 2026-10-17 (UTC) |
| **Decided by** | An agent (Claude Code, intelligence lane I2), under DEC-79 |
| **Story or claim** | E19-5's code half (DEC-2202); [claim #1531](https://github.com/kunwarshivam/mandate/issues/1531) |

### Context

Agent harness spec §6.7 and its §6 state table say that in `exits_only` a research run is
review-only: it may invalidate, never propose, and its theses are discarded unjudged. The
`single_call` research loop in `mandate-runtime` starts a run only in mode `normal`
(`research_allowed`), and a run whose mode leaves `normal` while its call is in flight judges
nothing, so its invalidations are dropped with its theses. E19-5's independent review recorded this
divergence between the spec and the code.

### Decision

1. **The code keeps the stricter reading for now.** No `single_call` research run starts in
   `exits_only`, and a run that leaves `normal` before it is judged judges nothing, invalidations
   included.
2. **Why it adds no risk.** An invalidation only removes an instrument (rule 2, MI-19). Skipping a
   run only delays a removal: the thesis still expires at its `expires_at` (mandate spec §8.6), and
   every exit path, risk exits and the kill switch among them, is untouched (rule 13).
3. **Where the spec's reading lands.** Agent harness spec v0.2 §6.9 (#1540, DEC-2203) makes the
   review-only run a `monitor` run, scheduled in `exits_only`, whose output schema has no `theses`
   member. The runtime's multi-turn inputs (lane I2, ADR-0005 slice 5) implement it there, and this
   decision is superseded then.

### Alternatives considered

- **A review-only `single_call` run in `exits_only` now.** Rejected: it would add a run mode the
  `monitor` kind replaces within days, with its own tests and mutants.

<a id="dec-2206"></a>

## DEC-2206: The runtime core runs an agentic research run: its inputs, requests, and batches

| | |
|---|---|
| **Status** | Accepted (agent; reversible engineering decisions under DEC-79, each at least as strict as the rule it implements) |
| **Date** | 2026-10-17 (UTC) |
| **Decided by** | An agent (Claude Code, intelligence lane I2), under DEC-79 |
| **Story or claim** | E19-13's core side (ADR-0005 delivery slices 3 and 5, lane I2); [claim #1531](https://github.com/kunwarshivam/mandate/issues/1531) |

### Context

Agent harness spec v0.2 §6.9 (#1540, DEC-2203) makes a research run a session of model turns and tool calls. The run's records are journal spec §9.23's run records (rules 171 to 183, registered by lane I2's `claude/i2-run-records`). The run's pure loop state lives in `mandate-research-run::agentic`, lane AH's (`RunState`, with `turn`, `on_reply`, `on_results`, `end` and `fold`). The tool-call shape of a turn's record is `mandate-gateway`'s (#1551, lane I1).

What is left is the core: `mandate-runtime`'s research step. It has to fix the run's kind and menu before anything untrusted is read, journal every record before the request it authorises, and hand the worker its turns and reads.

The change lands in one PR (DEC-990 item 3): the tests first, on stubs, then the core over lane AH's loop (#1566).

### Decision

1. **Which entries run agentic.** A research entry whose pinned content is `Mode::Agentic` runs agentic. `ResearchConfig` gains `agentic: Option<AgenticConfig>`, carrying:
   - the pinned `output` (`final_output` first: the demo's path, DEC-2203 item 5);
   - the loop bounds;
   - one menu per run kind;
   - the run's cost ceiling.

   `None` keeps today's `single_call` path, unchanged. No failed agentic run is retried as a single call (§6.9).
2. **The kind and its menu come from the trigger and folded state alone (§6.9).** At a due scheduled poll:
   - `ideate` runs in mode `normal` with room in the working universe;
   - `monitor` runs in `exits_only`, or with a full universe, while the agent has an active thesis;
   - otherwise nothing runs and nothing is called.

   The menu is the kind's menu from the config, fixed then. This supersedes DEC-2205 for agentic entries: a `monitor` run is the spec's review-only run in `exits_only`.
3. **The start batch** journals `RunStarted` before the run's seed:
   - the run id as `correlation_id`;
   - `menu` as tool names, ascending by bytes;
   - `data_universe_ref` naming the canonical data universe, as an artifact.
4. **Artifacts the core makes.** `ResearchStep` gains `artifacts`: the bytes of every artifact a draft of the step names that the core made itself (the data universe, a tool call's canonical arguments, a refusal's `{"error": code}`). The shell stores them before it appends the batch, as it stores the gateway's prompt and response today.
5. **A turn.**
   - **The request.** The core emits `ResearchRequest::Turn`. It carries the turn's `CallPlan`, the loop's `TurnPlan` (tools, spend ceiling, time left as a duration) and the transcript as artifact references. The worker loads the bytes; the core holds none.
   - **The batch.** The reply comes back as today's `ResearchInput::Called`. The core journals its `ModelInvocationRecorded`, then one `ToolCallRecorded` per tool call in reply order, then the results refused in the batch (`call_budget`, the bound's code, `run_ended`).
   - **What follows.** A final output is judged as `single_call` judges it, invalidations first (DEC-2202), and the run ends `done`. A reply with reads or thinks left gets one `ResearchRequest::Dispatch`, emitted after the batch.
   - **A `monitor` run never proposes.** §6.9 pins a monitoring run's output schema without `theses`, at the gateway. Should a `monitor` reply carry a thesis anyway, the core uses nothing of it: the turn's record is journaled with no verdicts, and the run ends `error`, as a `schema_invalid` reply's does. This tightens the core to the spec's outcome without relying on the gateway alone (DEC-176).
6. **A dispatch's results** come back as `ResearchInput::Dispatched`, each content artifact already stored. The core journals, per result in reply order:
   - an `ObservationRecorded` for each new item, then its `ToolResultRecorded`;
   - then the next `Turn` request, or the run's end.
7. **The end batch** journals `RunSummaryRecorded`, then `RunEnded`, with the reason the loop gives:
   - `deadline` at the first poll past the run's deadline;
   - `mode` on a mode or version change, a withdrawn model, disabled research, Stop or the kill switch;
   - otherwise as the reply decides.

   A cancellation ends the run without waiting for the worker. A late reply is journaled after `RunEnded` with no verdicts and no tool call records (rule 183).
8. **The run id on the envelope.** A draft cannot set its `correlation_id` (DEC-1120 item 9), and `EventDraft` is shared with R0's files. So `ResearchStep` gains `correlation_id`: the run every draft of the step belongs to, which the shell writes on each draft's envelope. A step never journals records of two runs: a poll that ends one run starts the next only at a later poll. `single_call` steps carry their run id too, which closes DEC-1120 item 9's hook.
9. **State.** The run's loop state is process-local, like today's run: a crash ends the run and a restart never resumes it (§6.9). Every batch is retried whole on an unknown append (journal spec §5.1), as today's research steps are.

### Rationale

Every request follows its records, so no turn is sent and no read runs before the record that authorises it commits (rule 5, HI-2).

The core never holds transcript bytes, so it stays pure and replays the same steps (HI-3).

The kind is chosen before the seed is read, so no untrusted text can choose its own tools (HI-4).

### Alternatives considered

- **Folding the loop state from the journal on restart.** Rejected: §6.9 ends a crashed run, and resuming needs a journaled turn start (E19-14).
- **Artifacts as an effect.** Rejected: they would need ordering against the drafts, which the step's own list gives for free.

### Follow-ups

- `action_tools` mediation (ADR-0005 slice 4, with lane AH).
- The driver and shell wiring (DEC-2204), after R0.

<a id="dec-2208"></a>

## DEC-2208: The runtime process runs the research driver, behind a deny-all default

| | |
|---|---|
| **Status** | Accepted (agent; reversible engineering decisions under DEC-79, each at least as strict as the rule it implements) |
| **Date** | 2026-10-17 (UTC) |
| **Decided by** | An agent (Claude Code, intelligence lane I2), under DEC-79 |
| **Story or claim** | E2E lane gap 3: research in the runtime process (DEC-996 item 2, DEC-2204's follow-up); [claim #1531](https://github.com/kunwarshivam/mandate/issues/1531) |

### Context

`mandate-research-driver` (DEC-2204) is the step machine that takes a research input through the core, appends the step's batch, folds it back, and only then hands the worker its requests. The shell was to wire it into its run path after R0. The agent runtime now has its own process instead, `mandate-daemon` (E21-30, DEC-1012). Lane P owns that process. The coordinator gave lane I2 the running of research inside it, so the demo's agent can research.

`mandate-daemon` is not an R0 file. Its `fixture::wiring` runs deny-all until the post-R0 hook (DEC-1013 item 2).

Research runs only once the core has left its startup hold, which needs the account stream's clean reconciliation. The process folds that stream since DEC-1014 (#1573), which this change builds on.

### Decision

1. **Deny-all by default.** `Wiring` gains `research: Option<ResearchWiring>`, and `fixture::wiring` leaves it `None`. With `None`, the process never builds a research input, never calls a worker, and journals no research record.
2. **What a research wiring holds:**
   - the research entry's `ResearchConfig`;
   - a `ResearchSource` that gives each pass the folded context: preconditions, mandate, overlay, universe, facts, lineages, active theses, last reservation and data symbols. `None` means no research on that pass;
   - a `ResearchWorker` that takes the core's requests and hands back its answers as `ResearchInput`s. It is the only place any I/O happens, and it stores its own artifacts (retrieved records, prompts, responses) before it answers;
   - an `ArtifactSink` for the artifacts the core makes itself (DEC-2206 item 4).
3. **The research pass runs after the control, account and clock passes, in the same loop, and only when all three ended with nothing in doubt and the clock stream was read** (DEC-1014 orders the first three).
   - The owner's kill switch is still read first, and research never delays trading (HI-7).
   - Each pass hands the driver `Poll` at the core's risk clock, then the worker's answers in arrival order.
   - An answer not yet handed waits in the process's inbox.
4. **A research batch in doubt is retried before anything else.** It goes ahead of the control pass, as a trading batch in doubt does. The agent stream has one writer, and the core takes no other input until the doubt is settled (journal spec §5.1). The retry carries the event time the batch was first written at, so the journal is offered the same bytes and answers `AlreadyCommitted` rather than `IdempotencyConflict`. It is built with the context the batch was first built with when the source has none on that pass: the core answers a retry from the batch it remembers, so a source with nothing to give never extends a doubt, nor holds the kill switch behind it.
5. **The journal writer over the process's store:**
   - It puts each core-made artifact through the sink, then appends the drafts as one batch. Each draft's envelope carries the step's run as its `correlation_id`. `mandate-store`'s `Fields` gains that member, and every other writer passes `None`.
   - A sink that cannot store an artifact appends nothing. On a batch's first attempt it is refused (`artifact_unstored`), as item 6's `Invalid` is: research halts in this process and the core forgets the batch. Leaving it in doubt would hold every other input behind a batch the journal never saw, the owner's kill switch among them (`AGENTS.md` rule 13). On the retry of a batch in doubt the journal may already hold it, so the batch stays in doubt: abandoning it would leave the core's head behind the journal's.
   - A committed or already-committed append is folded back by the driver, through the core, and nowhere else.
6. **How each append outcome is treated** (DEC-2204's review minors):
   - `Invalid` is a refusal. Research halts in this process, and the core forgets the batch, so trading and the kill switch go on. An alert names it (`research_halted`).
   - `Unavailable` and `Ambiguous` are in doubt.
   - `Fenced` stops the process as `Fenced`.
   - `HeadMismatch`, `IdempotencyConflict` and a `Diverged` driver mean the process no longer matches the journal, so it stops `Failed` and is restarted.
7. **A worker answer the core refuses** (another run's, out of order) is dropped with an alert (`research_input_refused`). It never stops the process, because the worker is not trusted to keep the process alive.

### Rationale

Research reaches the journal only through the same driver the tests prove (DEC-2204). The daemon adds no ordering rule of its own: it only decides when each input is offered.

The deny-all default and the source's `None` keep the cell exactly as it is until a deployment's research is configured.

### Alternatives considered

- **A research task beside the runtime task.** Rejected: the agent stream has one writer (HI-16), so two tasks would need a lock around the core, which the single loop already is.
- **Folding the context from the journal inside the daemon now.** Rejected for this slice: the universe, facts and reservation come from the account and meter streams, whose folds belong to the post-R0 hook. The source trait leaves room for them.

### Follow-ups

- The production `ResearchSource` and `ResearchWorker`: retrieval over the data plane, and the gateway's recorded fake or OpenRouter.
- The binary's wiring, after lane P's #1560 (`main.rs`).

<a id="dec-2209"></a>

## DEC-2209: The worker runs a dispatch's reads and thinks: arguments, caps, and the result's shape

| | |
|---|---|
| **Status** | Accepted (agent; reversible engineering decisions under DEC-79, each at least as strict as the rule it implements, DEC-176) |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | An agent (Claude Code, lane AH), under DEC-79 |
| **Story or claim** | E19-13 slice 3b (ADR-0005 delivery slice 3); [claim #1534](https://github.com/kunwarshivam/mandate/issues/1534) |

### Context

Agent harness spec v0.2 §6.10 and §6.11 say what each read and think returns and how it is capped.
The core orders a reply, screens its reads against the run's frozen scope, and applies the in-batch
bounds (`RunState::on_reply`, #1566). Then it hands the worker a `ResearchRequest::Dispatch`
(DEC-2206 item 5). Nothing runs that dispatch yet. `mandate_research_run::dispatch` does, purely, over the
read-only ports. Lane I2 wires it into the production worker.

Slice 3b lands in two pairs of PRs (DEC-77). The first pair covers `market.bars`, `note`, and
`plan`. The second covers `news.search` and `news.get`, with `TextQuery` gaining the model's search
terms. `market.quote`, `filings.search`, and `filings.get` need new item shapes and reader answers (a
quote, a filing's stored text and its length, `out_of_range`, `excerpt_only`), so they come after
it. The reads of the agent's own folds (`memory.*`, `records.query`, `mandate.read`) wait until the
core's read view is agreed with lane I2.

### Decision

1. **Every answer is filtered again.** A bar must be at or before the cut-off and of the symbol the
   call named. A text item (news, in the next pair) must also be inside the read's window, from a
   source on the allowlist version in effect, and not licensed; bars are platform data, with no
   window, allowlist, or licence to check. These are the seed's filters (§6.3), so a reader that leaks cannot show more than
   the run's retrieval could (HI-13, DEC-101, HI-23). Search terms are matched by the data service
   (data-plane spec §4.6) and travel in `TextQuery.terms`; the worker does not match them again,
   because a term only narrows a read.
2. **The order is newest first**, by knowledge time and then item id, in both the result and its
   `items`.
3. **Caps, in this order:**
   - first the tool's item cap (260 bars, 20 news items). Each equals the widest `count` or `limit`
     the pinned schema admits, so the model's own limit always binds first and the item cap never
     sets `truncated` on its own;
   - then each news item's 2,000 bytes of headline and summary, cut on a character boundary;
   - then `max_result_bytes`;
   - then the run's bytes left.

   The byte caps drop the oldest items until the result fits. Any of them sets `truncated`. The
   model's own `count` or `limit` is passed to the reader and keeps the newest that many; it sets
   nothing (rule 179). A read with no bytes left (or too few for one item) returns `{"items":[],"truncated":true}`,
   whose 29 bytes still count against the run's bytes left, which stops at zero.
4. **The result's shape:** `{"items": [...], "truncated": bool}`, canonical JSON. Each item carries
   `item_id` and `source` beside its fields, and the fields are wrapped as untrusted data under
   `untrusted`, bars included, as §6.10 says of every read: the prompt treats no read's content as
   instructions. A news item carries no `link` (§6.10). The record's `untrusted` flag is a
   different thing: it marks third-party or model-written text for an action's provenance
   (journal spec §9.23, §6.12), so it is set for every `ok` news read and not for bars, which are
   the platform's own data.
5. **A refused or failed call** returns exactly `{"error": "<code>"}`, with no items. A reader that
   cannot answer, or does not answer within `tool_timeout_s`, gives `failed`/`unavailable`. The
   shell enforces that timeout on the wall clock; the function reads no clock.
6. **Arguments outside the pinned schema** (a count of 0 or 261, unparsable text) read nothing and
   give `failed`/`unavailable`. The gateway's schema check refuses them first (inference spec
   INF-5), so this is defence in depth. It adds no risk (rule 3), and it counts toward
   `max_repeat_denials` as every failure does.
7. **Every read result counts against the run's bytes**, an error object included, exactly as
   `RunState` counts `ToolResult.bytes` for the read class, so the two never disagree. A think
   counts nothing.
8. **A think returns `{}`.** `note` and `plan` keep their text in the call's own
   `ToolCallRecorded.arguments_ref`. The result holds none of it, so it is not `untrusted`.
9. **An action is never dispatched.** The core mediates actions; a dispatch that holds one is
   `LoopOrder`, and none of its calls is run, the reads before the action included.
10. **A symbol outside the run's data universe is refused again**, `refused`/`out_of_universe`,
    whole, before any reader is asked. The core screens every read first (`RunState::on_reply`), so
    this only repeats its check where the read is made (DEC-176: it tightens and adds no risk).
11. **Reads not served yet answer `failed`/`unavailable`** until their slice lands: `news.*`,
    `market.quote`, `filings.*`, and the fold reads. An entry that declared one would
    read nothing through it; no risk is added (rule 3).
12. **A bar's identity is the seed's, a DEC-176 reading.** §6.10 keys a bar by its instrument, its
    date, and its content hash; this reading keys it by `bar:<symbol>:<knowledge time in seconds>`
    and keeps one record per id, so it can only show the model less, never more, and it adds no
    risk. The id is the one `RetrievedItem::item_id` already gives the seed's bars. Two answers for one instant are one item, and the first one the
    reader answered is kept, exactly as `retrieve` keeps it, so a re-run over the same store
    snapshot keeps the same one (DEC-2203 item 11). A restated or adjusted bar arrives with its
    own, later knowledge time (data-plane spec §5.2), so it is a new item with a new id, as §6.10
    asks. The core keys a run's observations by item id (DEC-2206 item 6); the reads are point in
    time at the run's frozen cut-off, so one id answers one record throughout a run, and that
    record's digest, the observation's `data_ref`, carries §6.10's content hash.

### Rationale

The worker can only narrow what a reader answered, never widen it. Its caps are the spec's,
applied in a fixed order, so the same reads give the same bytes and a re-run reproduces every
record (DEC-2203 item 11). The result's shape keeps every item's ids beside its data and its
third-party text in a marked section. That is defence in depth (HI-10 does the bounding).

### Alternatives considered

- **Refuse invalid arguments as `unknown_item` or `out_of_universe`:** that would tell the model
  something false; `unavailable` says only that nothing was read.
- **Match search terms again in the worker:** this duplicates the data service's matching for
  nothing, since a term only narrows.

<a id="dec-2210"></a>

## DEC-2210: The cell's research runs over a recorded bundle and the gateway's recorded fake

| | |
|---|---|
| **Status** | Accepted (agent; reversible engineering decisions under DEC-79, each at least as strict as the rule it implements) |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | An agent (Claude Code, intelligence lane I2), under DEC-79 |
| **Story or claim** | DEC-2208's follow-ups: the production `ResearchSource` and `ResearchWorker`, and the binary's wiring; E21-54's step 3; [claim #1531](https://github.com/kunwarshivam/mandate/issues/1531) |

### Context

The runtime process runs the research driver behind a deny-all default (DEC-2208). For the demo
on 2026-10-26 (E21-54, step 3), the agent in the local cell has to research. That needs a worker
that does the I/O, a source that gives each pass its context, and an explicit way to turn it on.

No data plane serves news or filings yet. No research entry, endpoint, or price table exists
outside tests. OpenRouter needs E21-32's vault and a hosted transport, and no cell build has
either (DEC-1120).

None of this touches `mandate-shell` or any R0 file. It all goes in `mandate-daemon`, which is not
an R0 file (DEC-1013, DEC-2208), and in the cell's own files.

### Decision

1. **A recorded bundle.** The cell's research reads one directory, `deploy/cell/demo/research/`.
   - `research.json` holds the research entry (`ResearchEntry` without its plan and bounds,
     which are the fixed `v0_1` versions), the one endpoint, the reported identity it probes with,
     the workspace and deployment caps, and the one recorded reply.
   - `bars.json`, `news.json` and `instruments.json` are the recorded data plane.
   - The directory is loaded once, at start. A file that is missing, not strict JSON, or that is
     missing a member refuses the load. Nothing is defaulted.
2. **Recorded times are ages, not instants.** Each recorded bar and news item carries `age_s`, its
   knowledge time's distance before the read's cut-off. A news item also carries
   `published_age_s`. The recorded readers stamp each item at `cut_off - age_s` for the read they
   answer, so the bundle stays inside the retrieval plan's windows whenever the demo runs. An item
   with a negative age is never returned.
   - The readers return no filings. The bundle has none.
   - `retrieve` filters every answer again (DP-1, the allowlist, the windows). The readers can
     only give it less.
3. **The recorded fake is the only transport the cell's worker can name** (`RecordedTransport`
   with `RecordedKey`). The worker's gateway type has no hosted branch, so no cell build can select
   OpenRouter, whatever the vault holds.
4. **Missing facts refuse.** When the cell's source cannot read a fact, it gives the core no
   context on that pass, and research does not run. When it can read a fact only in part, it
   hands the part that refuses more. For example, an unknown data universe is handed as `None`,
   and the eligibility floor refuses every admission on it. A source never fills in a fact the
   journal and the bundle do not hold.
5. **Opt-in only.** The binary runs research only when it is asked to, explicitly, for a resolved
   paper deployment. The default stays deny-all (DEC-2208 item 1).

### Delivery

Each item is binding from the PR that carries its tests, and no code path depends on an item
before then. The binary keeps deny-all (DEC-2208 item 1) until slice C.

| Item | Slice | Tests |
|---|---|---|
| 1, 2 | A1 (#1604 tests, then its implementation) | `crates/mandate-daemon/tests/recorded.rs` |
| 3 | A2: the worker over `RecordedTransport` and the artifact sink | the worker's run through the recorded fake, every artifact stored before its answer, answers stamped at the risk clock they are handed at, and no answer of the worker ever reaching the order path (rule 4) |
| 4 | B: the cell's research source | an unreadable or missing fact, or no deployment mandate document, gives no context, and an unknown universe is `None`, so the floor refuses |
| 5 | C: the binary's opt-in and the chart | without the explicit opt-in, the process runs the deny-all wiring, and a non-paper or unresolved deployment refuses it |

### Rationale

The recorded bundle lets the demo show the real path: the harness, the gateway, the guard, the
meter, the eligibility floor, the autonomy rules, and the journal. Only the data and the model's
reply are recorded, and both say so.

Ages keep a recording valid on any date without a clock that lies to the process. The cut-off is
still the core's risk clock, and DP-1's filter is still applied after the readers.

Making the hosted branch unrepresentable puts the "never OpenRouter in a cell build" rule on the
type rung of the trust ladder, rather than in a flag.

### Alternatives considered

- **Fixed instants in the recording.** Rejected: on any date more than 48 hours after the
  recording, the news window drops every item, and the demo would show a run with nothing to read.
- **Compile the bundle into the binary.** Rejected: it puts recordings in the production binary,
  and the cell would have to be rebuilt to change one.
- **`ResearchTransport` with no hosted transport (`NoHostedTransport`).** Rejected for the cell:
  the selection then decides by what the vault holds, which is a runtime property. The plain
  recorded transport cannot be anything else.

### Follow-ups

- A data plane that serves news and filings, which replaces the recorded readers.
- OpenRouter in a hosted cell, after E21-32 and a journaled meter, as DEC-1120 says.

<a id="dec-2211"></a>

## DEC-2211: The recorded bundle records filings too, and the cell's recording holds none

| | |
|---|---|
| **Status** | Accepted (agent; a reversible engineering decision under DEC-79 that loosens nothing) |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | An agent (Claude Code, intelligence lane I2), under DEC-79, on the coordinator's route |
| **Story or claim** | E21-54's step 3, slice A1 (DEC-2210); [claim #1531](https://github.com/kunwarshivam/mandate/issues/1531) |

### Context

DEC-2210 item 2 says the recorded readers return no filings. Implemented as a reader that always
answers `Ok` with an empty list, that body has a mutant, `Ok(vec![])`, which no test can tell apart
from it. The mutation gate exempts only stub bodies, and `.cargo/mutants.toml`'s exclusions are the
founder's, so the body has to be one a test can distinguish.

### Decision

1. **Filings are recorded like news.** The bundle has a fifth file, `filings.json`: an array of
   rows with `item_id`, `source`, `symbol`, `form`, `age_s`, `filed_age_s`, `excerpt` and
   `licensed`. Like every other file and member, it is required and nothing is defaulted
   (DEC-2210 item 1): a missing file or member refuses the load.
2. **A recorded filing is stamped like news.** Its `knowledge_at` is `age_s` and its `filed_at` is
   `filed_age_s` seconds before the read's cut-off. Its `content_hash` is the digest of its form and
   excerpt. `retrieve` filters the answer again, as for news.
3. **The cell's recording holds no filings.** Its `filings.json` is `[]`, so DEC-2210 item 2's
   behaviour, no filings in the cell, holds unchanged. This decision amends only how that behaviour
   is produced.

### Delivery

The test is `a_recorded_filing_is_read_at_its_age_before_the_cut_off` in
`crates/mandate-daemon/tests/recorded.rs`, together with `filings.json` in that file's list of
bundle files, which `a_missing_file_refuses_the_load` walks. They land in #1622 as a tests-first
commit, followed by the implementation commit.

### Rationale

Reading filings from the recording gives the reader a body a test can tell from `Ok(vec![])`,
without an exclusion and without changing what the cell sees. A required file keeps DEC-2210
item 1's "nothing is defaulted" whole.

### Alternatives considered

- **An `exclude_re` entry for the reader.** Rejected: exclusions are founder-owned, and the
  coordinator declined one.
- **A missing `filings` file read as an empty list.** Rejected: it would default a part of the
  bundle, which DEC-2210 item 1 forbids.
- **A `filings` member inside `research.json`.** Rejected: bars, news and instruments are each a
  file of their own.

<a id="dec-2212"></a>

## DEC-2212: The research worker is handed its pass's parts and risk clock; the cell's worker and store sink

| | |
|---|---|
| **Status** | Accepted (agent; a reversible engineering decision under DEC-79 that loosens nothing) |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | An agent (Claude Code, intelligence lane I2), under DEC-79 |
| **Story or claim** | E21-54's step 3, slice A2 (DEC-2210 item 3); [claim #1531](https://github.com/kunwarshivam/mandate/issues/1531) |

### Context

DEC-2208's `ResearchWorker` takes a request with nothing else, and hands back answers with no
clock. A worker that retrieves needs its run's scope: the allowlist, the data universe and the
envelope. All of these are in the parts the pass was built from. DEC-2210 item 3 also wants each
answer stamped at the risk clock of the pass it is handed in. A worker cannot read that clock
itself, because it is not trusted with the core's time.

### Decision

1. **The worker is handed the pass.** `ResearchWorker::hand` takes the request with the
   `ResearchParts` of the pass that made it, and `answers` takes the pass's risk clock. Every
   answer that carries a risk clock (`Called`, `Refused`, `Dispatched`) is stamped with it. The
   worker reads no other clock for the core.
2. **The cell's worker, `RecordedWorker`** (`mandate-daemon::worker`), is built from the recorded
   bundle (DEC-2210, DEC-2211):
   - On `Start`, it retrieves over the recorded plane in the scope the parts give, and stores each
     item's record through its sink. It answers only the items whose record was stored.
   - On `Call`, it sends the run's one call through a gateway whose transport type is
     `RecordedTransport` and whose key source is `RecordedKey`. It keeps the answer only once the
     call's prompt and response are stored.
   - On `Cancel`, it forgets the run and says whether a call had left.
   - It ignores agentic `Turn` and `Dispatch` requests, because the cell's entry is one-shot.
   - Its gateway's request rate is a fixed 6 per workspace and per agent. A one-shot run calls
     once per research interval, so this cap never binds, and it bounds a fault that would call
     again.
3. **`StoreSink`** puts each artifact into an `ArtifactStore`. It reports stored only when the
   store gives back the reference of the very bytes it was handed.

### Delivery

Item 1 lands first, in its own PR: the trait change, the process's call sites, the existing test
fake, and `src.wire_two` added to `tests/research.rs`'s shared facts, which the recorded reply
cites. The existing research tests cover it.

Items 2 and 3 land as A2's tests PR, pending. Its tests are `crates/mandate-daemon/tests/worker.rs`
and `the_recorded_worker_s_answers_never_reach_the_order_path` in `tests/research.rs`. That test
wires the worker into the runtime process: the run journals the model's thesis as a proposal, and
no stream holds an order event (`AGENTS.md` rule 4). The implementation PR removes the markers.

### Rationale

Handing the worker the pass keeps the core the only source of the facts and the clock a run is
judged by, and the worker no richer than a pure function of what it is handed plus its recording.
Stamping at the handed clock is what makes a late answer's time the core's, not the worker's.

### Alternatives considered

- **The worker reads its own source.** Rejected: the worker would read the journal's folds a
  second time, and could disagree with the pass that made the request.
- **The worker stamps answers from its own clock.** Rejected: DEC-2210 item 3, and the core alone
  holds the risk clock.

<a id="dec-2213"></a>

## DEC-2213: The dispatch's news reads: the search's arguments and terms, its refilters and order, and `news.get`'s stored text

| | |
|---|---|
| **Status** | Accepted (agent; reversible engineering decisions under DEC-79, each at least as strict as the rule it implements, DEC-176) |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | An agent (Claude Code, lane AH), under DEC-79 |
| **Story or claim** | E19-13 slice 3b, the second pair (ADR-0005 delivery slice 3); [claim #1534](https://github.com/kunwarshivam/mandate/issues/1534); after DEC-2209 |

### Context

DEC-2209 settled the dispatch's shape, caps, and bars (#1611, #1653). The news reads (agent
harness spec v0.2 §6.10) land as two DEC-77 pairs, one per tool: `news.search` first (items 1 to
5, and `search`'s default in item 8), then `news.get` (items 6 to 8). The news search needs the model's
search terms to reach the data service (data-plane spec §4.6), and `news.get` needs a reader answer
the ports did not have: one received item's stored text. DEC-2209's items 2 to 9 apply to both
reads unchanged; this records what is new.

### Decision

1. **`news.search`'s arguments are §6.10's, read strictly:**
   - `symbols`: 1 to 5;
   - `window_h`: 1 to 168;
   - `terms`: 0 to 5, each 1 to 32 bytes of ASCII letters, digits, spaces, hyphens, and periods;
   - `limit`: 1 to 20.

   Anything else reads nothing and gives `failed`/`unavailable` (DEC-2209 item 6). The spec says
   "letters"; ASCII only is the tighter reading (DEC-176), and it makes case folding the same in
   every data service.
2. **The query is the harness's, and the terms are the model's.** `NewsReader` gains `search`, which
   takes the seed's `TextQuery` and the terms beside it, so no existing reader or query changes; its
   default answers `ReadError::Unavailable`, so a reader that cannot match terms reads nothing
   (rule 3) rather than ignoring them. The query's workspace, cut-off, allowlist version, and symbols come from the run's frozen scope;
   its window starts `window_h` hours before the cut-off; `max_items` is the model's `limit`. The
   data service matches the terms (data-plane spec §4.6), and the worker does not match them again
   (DEC-2209 item 1). The seed's fixed reads keep calling `news`, with no terms.
3. **Every answer is filtered again**, as the seed's news read is (§6.3):
   - knowledge time at or before the cut-off and inside the window;
   - a source on the allowlist version in effect;
   - not licensed;
   - about a symbol the call named;
   - not an id in the platform's reserved namespaces (`bar:`, `instrument:`);
   - the first answer for each id kept.
4. **The order is newest first by knowledge time, then ascending item id**, and the model's `limit`
   keeps the newest that many without setting `truncated` (rule 179). Each kept item is cut to
   2,000 bytes of headline and summary, the headline first, on a character boundary. Any cut sets
   `truncated`.
5. **A news item's fields are §6.3's without `link`:** `published_at`, `headline`, `summary`,
   `content_hash`, and `length_bytes`. They sit under `untrusted` beside `item_id` and `source`
   (DEC-2209 item 4). An `ok` news read sets the record's `untrusted` flag.
6. **`news.get` reads one item's stored text through `NewsReader::news_text`.** The worker passes
   the item id, the run's cut-off, and the allowlist version, and nothing else.
   - The item's fields are the search's, with `text` in place of `summary`, cut to 8,000 bytes on
     a character boundary. A cut sets `truncated`.
   - The item observed is the news item's own record, which holds no stored text, exactly as the
     seed's items are recorded.
   - The core has already screened the id against the items the run received (`unknown_item`,
     `RunState::on_reply`).
7. **A `news.get` answer that leaks reads nothing.** That covers another item, one after the
   cut-off, one off the allowlist, or licensed text. Any of these gives `failed`/`unavailable`, as
   a reader that did not answer does: the worker never shows what the reader should not have
   given.
8. **The ports' defaults read nothing.** `search` and `news_text` have default bodies answering
   `ReadError::Unavailable`, so the readers that exist (the runtime's and the daemon's recorded
   planes) keep compiling and read no text until they serve it (rule 3).
9. **`out_of_range` and `excerpt_only` are the filings reads'** (data-plane spec §4.6), so they
   come with `filings.get` in a later pair.

### Rationale

The search is the seed's news read with the model's narrowing added. Every filter the seed applies
is applied again, so a model's arguments can only show less than the run's allowlist and scope
allow, and a term never leaves the data service (HI-28).

### Alternatives considered

- **Refuse a leaking `news.get` answer as `unknown_item`:** that would tell the model the item does
  not exist, which is the reader's fault, not its; `unavailable` says only that nothing was read.
- **Unicode letters in terms:** case folding would depend on each data service's Unicode tables.

<a id="dec-2214"></a>

## DEC-2214: The cell's research source, which refuses every fact it cannot read

| | |
|---|---|
| **Status** | Accepted (agent; a reversible engineering decision under DEC-79 that loosens nothing) |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | An agent (Claude Code, intelligence lane I2), under DEC-79, with the coordinator's rulings on items 1 and 4 |
| **Story or claim** | E21-54's step 3, slice B (DEC-2210 item 4); [claim #1531](https://github.com/kunwarshivam/mandate/issues/1531) |

### Context

DEC-2210 item 4 says the cell's `ResearchSource` gives the core no context when it cannot read a
fact. When it can read a fact only in part, it hands the part that refuses more. No production code
yet builds a pass's `ResearchParts` from the journal: the thesis, lineage, working-universe and
reservation folds exist only in tests and the reference implementation. This decision says where
each part comes from in the cell, and what the cell does when a part is not there.

### Decision

1. **The bundle records its halts and its allowlist.** `research.json` holds two more members, both
   required like every other member (DEC-2210 item 1):
   - `halted_instruments`: the symbols of the bundle's instruments halted at its as-of time. A
     halt naming an instrument the bundle does not record refuses the load. An absent halt list
     would admit more, so the cell never assumes one, and the demo's bundle says `[]` on purpose.
   - `allowlist`: the source allowlist version the items were recorded under and its sources, so
     the cell never derives an allowlist from the sources its items happen to name.
2. **`CellSource` gives each pass the parts below, or no context at all.**
   - `mandate`: the deployment's mandate document (`deployment::resolved`'s fail-closed read),
     mapped to the research envelope. Any member that cannot be read gives no context.
   - `preconditions`: the journal catalog's (`Preconditions::read`).
   - `overlay`: `PolicyOverlay::internal_research_profile()`, the only one there is.
   - `facts`:
     - the allowlist: the bundle's recorded one (item 1);
     - the data universe: the bundle's instruments;
     - eligibility failures: from the instruments' `eligible` flags;
     - instrument groups: from the instruments' `group`;
     - halted instruments: from item 1;
     - accepted disclosures: none, which is the stricter reading;
     - instruments claimed by other agents: none while this agent is the only one deployed on
       the control stream. When any other agent is deployed, the pass gets no context;
     - today's research spend: the sum of the `cost_usd` of the agent stream's
       `ModelInvocationRecorded`s whose `event_time` is at or after the start of the risk day of
       the state's risk clock (`mandate_spec::risk::risk_day`, the day the cost cap counts). The
       risk clock moves only on a tick or a reconciliation, so it can lag a record's time; a
       record after the day's end still counts, which only refuses more (rule 3), and only an
       earlier day's records are left out. Every record is read strictly, an earlier day's too:
       a cost or time it cannot read, a record without a `reservation` member, a negative cost,
       or no risk clock yet gives the pass no context.
     - The journal dates a call's spend by its reservation's risk day (journal spec §9.19,
       `MeterReserved`'s `risk_day`); the source dates it by the record's `event_time`, and does
       not count a reserved call in flight. That makes its spend a lower bound, which is safe only
       because the cell's run is one-shot and ends in the batch that records its call (item 3),
       and a reserved call pauses research.
   - `data_symbols`: the bundle's instrument symbols.
3. **Theses, lineages, the working universe and the last reservation.** These are known only on an
   agent stream that holds none of the records they fold from: `ThesisProposed`, `ThesisRevised`,
   `ThesisInvalidated`, `UniverseChanged`, and a `ModelInvocationRecorded` whose call the meter
   reserved. The cell's meter lives in the worker and is not journaled, so a reserved call is a
   reservation the source cannot read. On such a stream they are an empty `Known` universe, no
   lineages, no active theses and no reservation. Once the stream holds any of those records, the
   source cannot fold it yet and gives no context, so research pauses.
   - Today's spend is the sum of today's recorded costs (item 2). A call the meter reserved is not
     one the source reads, so research pauses. Counting every day's costs instead would refuse
     more, but once they passed one day's cap research would never run again, which disables the
     feature rather than defaulting safely (rule 3).
   - The cell's research entry is one-shot (DEC-2212 item 2): a run makes one call, and its
     `ModelInvocationRecorded` is journaled in the batch that judges it, which ends the run. A
     run's `RunStarted` and observations do not pause research, so a run is never held between
     its passes. An agentic entry, whose turns each journal a reserved call, would pause after its
     first turn; the cell has none.
4. **Research pauses after the first proposal**, and after the first call the meter reserved. This
   holds for the demo's step 3 (coordinator's ruling), and the demo report says so. E21-59 lands
   the folds, after which research continues.

### Delivery

- **B0** (#1728): item 1, the recorded halts and allowlist. The tests come first, in their own
  commit, then the loader.
- **B1** (#1733): item 2's envelope read, as a tests PR with `envelope` stubbed.
- **B1-source** (#1735): items 2 to 4's `CellSource`, as a tests PR with its stubs.
- **B2:** both implementations, which remove the pending markers.

### Rationale

Every part either comes from a record the cell holds, or the pass has no context. No default, not
even an empty set that would admit more, stands in for a fact the journal and the bundle do not
hold.

### Alternatives considered

- **Assume no halts when the bundle does not say.** Rejected: an absence that admits more is a
  loosening (coordinator's ruling).
- **Fold theses and universes now.** Deferred to E21-59: the core's fold functions need a writer's
  read-back over journaled records, which is a story of its own.

<a id="dec-2215"></a>

## DEC-2215: The runtime process's research opt-in, and the records it writes

| | |
|---|---|
| **Status** | Accepted (agent; a reversible engineering decision under DEC-79 that loosens nothing) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | An agent (Claude Code, intelligence lane I2), under DEC-79 |
| **Story or claim** | E21-54's step 3, slice C (DEC-2210 item 5); [claim #1531](https://github.com/kunwarshivam/mandate/issues/1531) |

### Context

DEC-2210 item 5 says the binary runs research only when it is asked to, explicitly, for a resolved
paper deployment, and that the default stays deny-all (DEC-2208 item 1). The pieces exist: the
recorded bundle (DEC-2210, DEC-2211, DEC-2214), the recorded worker and the store sink (DEC-2212),
and the cell's source (DEC-2214). This decision says how a process is asked, what it refuses, and
what its research records carry.

### Decision

1. **The opt-in is one flag.** `mandate-daemon run --research <bundle-dir>` names the recorded
   bundle. Without it, the process runs the deny-all wiring exactly as before.
2. **It is refused for any but a resolved paper deployment.** With the flag:
   - a deployment that is not `paper` is refused (`research_needs_paper`);
   - a deployment handed by flags or `--fixture`, not read from the journal (DEC-1016), is
     refused (`research_needs_resolved_deployment`);
   - an unreadable bundle (`research_bundle_unreadable`), artifact store
     (`artifact_store_unavailable`), deployment or mandate document (the deployment read's own
     code), or a worker that does not build (`research_worker_unbuilt`), refuses the start.

   A refused opt-in stops the process from starting. It never falls back to running without
   research, so asking for research and getting none is never silent.
3. **What it wires.** `mandate-daemon::cell` builds the research in two steps:
   - `opt_in` checks the deployment and loads the bundle, and reads no journal and no artifact
     store, so a refused opt-in stops the start before the process connects to either. Only it
     makes an `OptIn`, so no research is built for any other deployment.
   - `research` turns an `OptIn` into:
     - the bundle's entry as the `ResearchConfig`: its pin, deadline and output cap, the fixed
       `v0_1` plan and bounds, the mandate document's `behavior.description`, revisions off, and no
       agentic loop;
     - the recorded worker over the bundle, storing into the artifact store through `StoreSink`;
     - the cell's source over the bundle, reading the streams through `Rows` and the mandate
       documents from the artifact store;
     - `StoreSink` over the artifact store, for the artifacts the core makes.
4. **Its records name the deployed mandate and the entry.** The process's `Refs` carry the
   deployed mandate version and, as `model_version`, the research entry's content hash. The journal
   checks a model record's `model.content_hash` against `model_version`, so a research record under
   the fixture's refs would be refused.

### Delivery

- **C1:** `cell::opt_in` and `cell::research` stubbed (`CellError::Unimplemented`), the binary's
  `--research` flag wired to them, pending tests in `crates/mandate-daemon/tests/cell.rs` and a
  pending refusal test of the binary in `tests/process.rs`, and a passing test of the refusal
  codes.
- **C2:** the implementation. The cell's source also fails closed on a halted instrument it cannot
  read as an asset, giving the pass no context (DEC-2210 item 4).
- **C3:** the cell chart's flag, and the demo bundle under `deploy/cell/demo/research/`, with its
  `filings.json`, `halted_instruments: []` and `allowlist` written as documented fixtures. The cell
  path makes no network fetch.

### Rationale

A single explicit flag keeps deny-all the default with no configuration to get wrong. Refusing to
start, rather than starting without research, makes a misconfigured demo fail loudly.

### Alternatives considered

- **An environment variable.** Rejected: a flag shows in the pod's command, so the chart says
  plainly whether research runs.
- **Fall back to deny-all when the opt-in cannot be met.** Rejected: the operator asked for
  research, so a silent fallback would hide a broken bundle or deployment.

<a id="dec-2245"></a>

## DEC-2245: A mandate-version backtest applies splits and cash dividends through the accounting fold, and carries protection across a split

| | |
|---|---|
| **Status** | Accepted (agent: reversible engineering readings under DEC-79 and DEC-176. Each item refuses rather than guesses. Item 3 is the coordinator's ruling on the plan. None touches live money, spending, legal text or a safety rule) |
| **Date** | 2026-10-18 (UTC) |
| **Decided by** | Lane I4 (Claude Code), under DEC-79 and DEC-176; item 3 as the coordinator ruled |
| **Story or claim** | E4-4 slice 2; DEC-1000 item 8, DEC-1144 item 3; trading spec §8.5; claim [#1463](https://github.com/kunwarshivam/mandate/issues/1463) |

### Context

A mandate-version backtest refuses `corporate_action_in_range` whenever an ex-date falls inside
its bars (DEC-1000 item 8), because the run receives only ex-dates and never applies an action.
`mandate-accounting` already folds splits (forward and reverse, with an optional cash-in-lieu
price) and cash dividends (ex-date, pay date, per share), and pays dividends itself on the pay
date (trading spec §8.5; RC-04, RC-05, RC-06, RC-23). Bars are stored raw (DEC-89): after a split
the bars are already at post-split prices, so applying the split in the fold, which rescales the
quantity and the marks, keeps the ledger consistent with the bars.

### Decision

1. **The input is a record per action.** `bars.json`'s `corporate_actions` is a list of records,
   each either `{kind: split, ex_date, ratio: {new, old}, cash_in_lieu_price}` (the price a
   decimal or `null`) or `{kind: cash_dividend, ex_date, pay_date, per_share}`. Every member is
   required and an unknown member refuses, as DEC-1144 item 3 reads every input. Any other kind
   refuses as an invalid input naming its `kind`: spin-offs, mergers, stock dividends and the
   rest stay outside the fold's scope (§8.5). So does a zero share count, a non-positive price,
   and a dividend paid before its ex-date. The run receives them as `RecordedAction`s.
2. **When an action applies.** At 20:00 New York time on the last trading day before its ex-date
   (§8.5): after that day's close and before the ex-date's first bar. Both the version's account
   and the benchmark's account receive it. Dividends are paid by the fold on their pay date.
3. **Protection across a split.** At the split's instant, the run:
   - cancels every working order in the instrument;
   - in the same step, re-places each protective order adjusted by the ratio. The quantity is
     multiplied by new/old, and the stop and limit prices by old/new. Each is rounded
     conservatively, toward more protection, to the tick and the lot. No bar is walked
     unprotected;
   - does not re-place opening orders. The strategy decides afresh at the next bar.

   If an adjusted protective order cannot be represented (a lot or tick rule, or a rounding that
   would weaken it), the run refuses with a named code. The cancel and the re-place are the run's
   own records.
4. **The configuration names the records.** The digested configuration spells each action with
   every member: `{kind, ex_date, ratio: {new, old}, cash_in_lieu_price}` or `{kind, ex_date,
   pay_date, per_share}`, in the records' order. A run with no actions spells an empty list, so
   its configuration digest is the one it had when the member held dates, and the committed demo
   store does not move. The report's shape does not change in this slice: income and cash in
   lieu reach equity through cash.
5. **Scope kept.** One instrument and US equities only, as DEC-1000 item 8 says.
6. **Slices, each a DEC-77 pair under 400 lines:**
   - (1) item 1's records and their strict decode, and item 4's spelling. The run still refuses
     an action in range, so behavior is unchanged;
   - (2) splits: items 2 and 3 for a split. `corporate_action_in_range` stops refusing a split.
     Its existing tests change in that tests PR;
   - (3) cash dividends: item 2 for a dividend.
7. **Addendum (2026-10-19, lane I4; the coordinator approved it): the benchmark's walk goes
   first.** A mandate-version run reports beside buy-and-hold, which `mandate_backtest::walk`
   runs. Lifting `corporate_action_in_range` for the version alone would mark the benchmark's
   pre-split shares at post-split raw prices, a false collapse. So slice (2) is two DEC-77 pairs
   in this order:
   - (2d) `mandate-backtest`'s run input carries the snapshot's actions as the accounting fold's
     `CorporateAction`s, and its walk folds each one before the first bar traded on or after its
     ex-date, after every bar traded before it. That is item 2's instant, 20:00 New York time on
     the last trading day before the ex-date. It is the trade-date boundary the accounting fold
     keys on (trading spec §8.5, its `fill_before_corporate_action` check) and the instant the
     risk state folds a split at ([DEC-2303](DEC-2303.md), mandate spec §5.2), so the three
     cannot disagree. An order still working when a split applies refuses the run with
     `order_working_across_split`: its limit and quantity are pre-split and the bars after it
     post-split, and the run does not guess an adjustment. The benchmark's one buy fills before
     any split in practice; a strategy order that does not, refuses. The strategy runner passes
     no actions, so it is unchanged. The walk takes the actions in the order given, so it refuses
     a slice not in ex-date order (`corporate_actions_unordered`) and an action for another
     instrument than the run's (`corporate_action_other_instrument`) rather than skip either;
   - (2c) the version's walk then applies the same action to its account and its risk state,
     cancels and re-places protection as item 3 says, and lifts `corporate_action_in_range` for
     splits only.

### Rationale

The fold already holds the arithmetic and its reference cases, so the run only has to feed it at
the instant the spec names. Raw bars and a rescaled ledger agree after a split; adjusted bars
would change every price the gate and the fill model read. Re-placing protection in the same step
as the cancel is what a broker does at a split, and it leaves no bar unprotected. Refusing an
order that cannot be adjusted without weakening it keeps the run from silently protecting less.

### Alternatives considered

- **Adjust the bars instead of the ledger.** Every price before the split would change, the
  gate's and the fill model's included, and the run would no longer replay raw data (DEC-89).
- **Re-place protection at the next bar.** It leaves the position unprotected across the split.
- **Keep refusing a split while protection works.** It narrows the backtest without need.
- **Keep the dates and read the records elsewhere.** The run would digest one thing and apply
  another.

<a id="dec-2246"></a>

## DEC-2246: The local cell's interim runtime image carries the demo's recorded research bundle

| | |
|---|---|
| **Status** | Accepted (agent; a reversible engineering decision under DEC-79 that loosens nothing) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | An agent (Claude Code, intelligence lane I2), under DEC-79, with the coordinator's ruling on the demo's researching agent and the bundle's delivery |
| **Story or claim** | E21-54's step 3, slice C3 (DEC-2210 item 1, DEC-2215); [claim #1531](https://github.com/kunwarshivam/mandate/issues/1531) |

### Context

DEC-2215 gives `mandate-daemon run` a `--research <bundle-dir>` opt-in. The demo's runtime pod is
started by the deploy manager. It needs the recorded bundle DEC-2210 item 1 places at
`deploy/cell/demo/research/` on a path inside that pod.

There are two ways to get the bundle there:

- **A ConfigMap volume.** The deploy manager's admission policy refuses every configMap volume
  (`deploy/cell/charts/workspace/templates/deploy-manager.yaml`). Allowing one would widen the
  bound on what the manager can create.
- **The image.** The cell's `runtime` image is the interim one (DEC-1013). It is built locally by
  `cargo xtask cell up` and replaced by R2's signed build (E21-35). DEC-2210 rejected compiling the
  recordings into the binary, not placing files beside it in a local image.

The demo's own agent, `demo-spy`, cannot research. Its mandate pins the universe, has
`behavior.research: null`, and its one signal model admits no instruments, so the runtime
schedules no research pass for it. Changing that mandate changes its content hash. The backtest's
fixed payload reference (DEC-1144 item 8) and other lanes' steps rely on that hash.

### Decision

1. **The interim cell runtime image carries the bundle.**
   - `cargo xtask cell up` copies every file of `deploy/cell/demo/research/` into the runtime
     image's build context, under `research/`.
   - `deploy/cell/images/runtime.Dockerfile` copies them, read-only, to
     `/usr/local/share/mandate/research`.
   - The runtime image's tag is the digest of the binary and every bundle file, so a changed
     recording gives a new image.
   - A test in `xtask` checks that the image carries every file of the directory, and nothing else
     under `research/`.
2. **Only the interim cell image.** No release image carries the bundle.
   - `cargo xtask release images` refuses to write an image that has a file under
     `usr/local/share/mandate/research`, however the path is rooted.
   - A layer it cannot read as a tar counts as having one, so the check fails closed.
   - A test plants each form of the path and a torn layer, and shows each refused.
3. **The deploy manager's configMap refusal stays as it is.**
4. **A second demo agent researches, and its envelope decides it.** The demo's researching agent
   gets its own mandate, `deploy/cell/demo/research-mandate.json`:
   - an unpinned universe, `max_instruments` 1;
   - a signal model that admits instruments;
   - admission `ask`;
   - a `behavior.research` envelope.

   It goes through the same version create, confirm and deploy gestures as `demo-spy`, so its
   owner confirms the envelope (rule 11). Nothing writes it to the journal directly.

   The deploy manager's pod template is one per process, not per agent (DEC-1015). So when the
   cell sets `runtime.research`, every runtime pod it starts gets `--research` with the bundle's
   path. The flag only makes the recorded bundle available. Whether an agent researches is
   decided by the envelope its owner confirmed, never by the flag: the runtime schedules research
   only for an unpinned universe with a `behavior.research` envelope. `demo-spy`'s mandate has
   neither, so with `--research` it runs no research pass and records no research spend, and a
   test pins that. Its mandate and steps are unchanged.
5. **Recorded only.**
   - That pod's research runs through the gateway's recorded fake only (DEC-2210 item 3). No
     provider key is mounted or read.
   - The bundle's model identity is a recorded one (`recorded/demo-research` on the endpoint
     `recorded:demo`, host `recorded.invalid`), and names no provider.
   - Its news items say in their text that they were written for the demo.
   - The envelope's `cost_cap` caps the recorded costs the meter journals from the bundle's prices
     and the recorded reply's token counts. No money is spent.
6. **The demo bundle is a validated fixture.** `crates/mandate-daemon/tests/demo_bundle.rs` checks
   that:
   - it loads;
   - it records only the demo's instrument (`deploy/cell/demo/instrument.json`'s SPY), no filings
     and no halts;
   - its recorded thesis names that instrument and cites only items from allowlisted sources;
   - it runs through the recorded worker to the recorded reply.

### Delivery

| Item | Slice | Tests |
|---|---|---|
| 1, 2, 3, 5 (the bundle), 6 | C3a | `xtask` `cell::tests::the_runtime_image_carries_the_whole_demo_bundle_at_the_cell_s_path`, `image::tests::only_an_image_with_the_cell_s_bundle_carries_it`; `crates/mandate-daemon/tests/demo_bundle.rs` |
| 4, 5 (the pod) | C3b: the chart's value, every runtime pod's `--research`, the research agent's mandate and gestures | the chart's tests in `deploy/tests/test_cell.py`; a test that `demo-spy`'s confirmed mandate, opted in, runs no research pass and spends nothing |

### Rationale

The image already holds what the cell runs, so the bundle is one more read-only file in a local,
unsigned, interim image. That widens no admission bound. The release check puts "never in a
release image" on the checked rung of the trust ladder, rather than leaving it to a comment.

A second agent keeps the existing demo's hashes, gestures and backtest reference intact, and its
envelope is still the owner's to confirm.

### Alternatives considered

- **A ConfigMap volume.** Rejected: it widens the deploy manager's admission bound.
- **Unpin `demo-spy`'s mandate.** Rejected: it changes the mandate's hash, which steps 2 and 4 and
  the backtest's fixed payload reference rely on.
- **Write the research envelope to the journal from the gestures' pod.** Rejected: that is an
  envelope change without a confirmed mandate version (rule 11).

<a id="dec-2247"></a>

## DEC-2247: The research agent's model registers from its recorded entry, and the demo researches in a workspace of its own

| | |
|---|---|
| **Status** | Accepted (agent; a reversible engineering decision under DEC-79 that loosens nothing) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | An agent (Claude Code, intelligence lane I2), under DEC-79, with the coordinator's rulings on items 1 and 2 |
| **Story or claim** | E21-54's step 3, slice D (DEC-2210, DEC-2246); [claim #1531](https://github.com/kunwarshivam/mandate/issues/1531) |

### Context

DEC-2246 item 4 gives the demo a second agent whose owner-confirmed mandate researches. Such a
mandate has exactly one `llm.` signal model with `admits_instruments: true` (V-036). `version
confirm` and `agent deploy` hold every signal model to the registry (V-007): its id, version,
content hash and parameter keys must match a `ConfigSnapshotRegistered` of kind `model_version`.

`model register` takes its content and hash from `mandate_modelhost::content` (DEC-504), which has
content for `quant.ma_crossover` only, and always registers `admits_instruments: false`. No
command can register the research agent's model, so no researching mandate can be confirmed.

`CellSource` also gives the core no context while any other agent is deployed on the same control
stream (DEC-2214 item 1), until claims are folded (E21-59). The demo's step 2 deploys `demo-spy`
and `demo-web` in the run's workspace.

### Decision

1. **`model register <id> <version> --entry <research.json>` registers the research agent's
   model.**
   - It is an owner CLI action. It writes one `ConfigSnapshotRegistered` of kind `model_version`
     on the control stream, and nothing else. Nothing registers a model at runtime, and nothing the
     research agent emits can cause a registration.
   - The file is a recording: the recorded bundle's `research.json`. Its `entry` member is read by
     `mandate_research_run::ResearchEntry::recorded`. The recorded bundle's loader reads it there
     too, so the pin a run carries and the hash a registration records are of the same content.
     Every member is required, and nothing is defaulted.
   - The content object is the entry's pinned content, with the fixed template, retrieval plan,
     output bounds and schema. Its hash is recomputed from those canonical bytes. No caller supplies
     a hash, and the hash is never copied from a file.
   - It is refused, before anything is stored or committed:
     - `model_not_research` for an id without the `llm.` prefix;
     - `entry_mismatch` when the entry's `model_id` or `model_version` is not the one named;
     - `entry_unreadable` for a file that cannot be read, is not strict JSON, or whose entry is
       missing a member.
   - It records `admits_instruments: true` and no parameters. It is the only registration that
     admits instruments. Quant models keep registering `false` from the model host.
   - Registering a model changes no envelope field. The mandate that names it is still entered and
     confirmed by its owner (rule 11), and `version confirm` still runs every V-rule. V-007 refuses
     a hash other than the registered one. V-036 refuses a second admitting `llm.` model, an `llm.`
     model that admits nothing under a research envelope, and a quant model that claims to admit.
2. **The demo's research agent deploys in a second workspace, `<workspace>r`.**
   - Step 3 opens and founds that workspace for the same owner, registers the two models, and
     creates, confirms and deploys `deploy/cell/demo/research-mandate.json` there. Its runtime is
     the only agent on that control stream.
   - DEC-2214 item 1 stays as it is.
3. **What moved.** This amends DEC-2246's delivery table, which DEC-2246 itself leaves unedited.
   The research agent's mandate and its gestures are delivered in slice D, not C3b. Step 3 waits on
   D1 only (the manager starting a runtime from `AgentDeployed`). The single-call path needs no
   agentic loop, so it no longer waits on AH.

### Delivery

| Item | Slice | Tests |
|---|---|---|
| 1 | D-a, then its implementation | `crates/mandate-research-run/tests/recorded_entry.rs`, `crates/mandate-cli/tests/research_register.rs` |
| 2, 3 | D-b: step 3's gestures and wait | the scenario's tests in `xtask/src/cell/scenario.rs` |

### Rationale

Recomputing the hash from the entry keeps DEC-504's rule that no caller supplies a hash: the
content the registry records is computed by the code that pins it. One reader for the entry means
the registration and the runtime's pin cannot drift apart. If they ever did, V-007 would refuse the
mandate rather than run research under a model it does not name.

A second workspace runs research with DEC-2214's fail-closed rule intact, and leaves steps 2, 4, 5
and 6 unchanged.

### Alternatives considered

- **Compile the research entry into the model host.** Rejected: the host runs quant models it
  evaluates. The research entry is the gateway's, and is already recorded in the bundle.
- **Let the owner pass the hash.** Rejected: it breaks DEC-504's rule, and a wrong hash would only
  surface at confirmation.
- **Relax DEC-2214 item 1 for the demo.** Rejected: it is a fail-closed safety rule.

<a id="dec-2248"></a>

## DEC-2248: Where DEC-2247 was delivered

| | |
|---|---|
| **Status** | Accepted (agent; a record of delivery under DEC-79 that changes no rule) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | An agent (Claude Code, intelligence lane I2), on the coordinator's ruling |
| **Story or claim** | E21-54's step 3, slice D-b (DEC-2247); [claim #1531](https://github.com/kunwarshivam/mandate/issues/1531) |

### Context

DEC-2247's Delivery table names slice D-a and "its implementation" for item 1. That table names
neither that the recorded bundle's loader moves onto `ResearchEntry::recorded`, nor the PRs that
delivered each item. DEC-2247 is accepted, and the decisions README (item 4) lets a PR edit only an
accepted decision's status, so this decision records the delivery instead.

### Decision

1. **This amends DEC-2247's Delivery table, which DEC-2247 itself leaves unedited.** Its rows are
   these:

   | DEC-2247 item | Delivered in | What | Tests |
   |---|---|---|---|
   | 1 | #1779 (tests), #1782 (implementation) | `model register --entry` and `ResearchEntry::recorded` | `crates/mandate-research-run/tests/recorded_entry.rs`, `crates/mandate-cli/tests/research_register.rs` |
   | 1 | #1782 | The recorded bundle's loader (`crates/mandate-daemon/src/recorded.rs`) reads its entry only through `ResearchEntry::recorded`, the gateway probe's model name included, so the runtime's pin and the registered hash come from one reading of one entry | the existing `recorded.rs` and `demo_bundle` tests in `mandate-daemon` |
   | 2, 3 | #1787 | Demo step 3 deploys the research agent alone in `<workspace>r`, and its run waits on lane X only | the scenario's tests in `xtask/src/cell/scenario.rs` |

2. **No rule changes.** Every item of DEC-2247 holds as written.

### Rationale

The README's rule keeps an accepted decision's text as it was accepted. A record of where it was
delivered is new information, so it goes in a new file, and DEC-2247 stays as it was.

### Alternatives considered

- **Edit DEC-2247's table in place.** Rejected: item 4 of the decisions README forbids it.
- **Record the delivery only in the PR descriptions.** Rejected: the decision log is where a reader
  looks for what a decision covers.

<a id="dec-2250"></a>

## DEC-2250: The dispatch's filings reads: the search's arguments, forms and stored length, and `filings.get`'s range

| | |
|---|---|
| **Status** | Accepted (agent; reversible engineering decisions under DEC-79, each at least as strict as the rule it implements, DEC-176) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | An agent (Claude Code, lane AH), under DEC-79 |
| **Story or claim** | E19-13 slice 3b, the third pair (ADR-0005 delivery slice 3); [claim #1534](https://github.com/kunwarshivam/mandate/issues/1534); after DEC-2209 and DEC-2213 |

### Context

DEC-2213 settled the news reads and left `filings.search` and `filings.get`, with their two
reader answers `out_of_range` and `excerpt_only`, to a later pair (item 9). The filings reads (agent
harness spec v0.2 §6.10) land as two DEC-77 pairs, one per tool: `filings.search` first (items 1 to
5, and `search`'s default in item 9), then `filings.get` (items 6 to 9). The search needs the
model's forms and terms to reach the data service, and each entry needs the byte length of the
filing's stored text, which the ports do not carry; `filings.get` needs a range read and two answers
the ports cannot say. DEC-2209's items 2 to 9 and DEC-2213's items 2 and 3 apply unchanged; this
records what is new.

### Decision

1. **`filings.search`'s arguments are §6.10's, read strictly:**
   - `symbols`: 1 to 5;
   - `forms`: 0 to 5 distinct forms from the pinned list `10-K`, `10-K/A`, `10-Q`, `10-Q/A`,
     `8-K`, `8-K/A`; none means every form on the list;
   - `window_d`: 1 to 90;
   - `terms`: 0 to 5, each as DEC-2213 item 1 reads them;
   - `limit`: 1 to 10.

   Anything else reads nothing and gives `failed`/`unavailable` (DEC-2209 item 6), and so does a
   form repeated. The spec lists the forms as "`10-K`, `10-Q`, `8-K`, and the like"; the three it
   names and their amendments (data-plane spec §4.5) are the tighter reading (DEC-176). A form added
   later is a new decision.
2. **The query is the harness's, and the forms and terms are the model's.** `FilingReader` gains
   `search`, which takes the seed's `TextQuery` with the forms and terms beside it and answers each
   filing with the byte length of its stored text (`FilingEntry`). Its default answers
   `ReadError::Unavailable`, so a reader that cannot match reads nothing (rule 3). The query's
   window starts `window_d` days before the cut-off; `max_items` is the model's `limit`. The data
   service matches the forms and the terms (data-plane spec §4.6), and the seed's fixed reads keep
   calling `filings`.
3. **Every answer is filtered again**, as the seed's filings read is (§6.3), and on its form:
   - knowledge time at or before the cut-off and inside the window;
   - a source on the allowlist version in effect;
   - not licensed;
   - about a symbol the call named;
   - a form the call named, or one on the list when it named none;
   - not an id in the platform's reserved namespaces;
   - the first answer for each id kept.
4. **The order is newest first by knowledge time, then ascending item id**, and the model's `limit`
   keeps the newest that many without setting `truncated` (rule 179). Each kept excerpt is cut to
   4,000 bytes on a character boundary, and any cut sets `truncated`.
5. **A filing entry's fields are §6.3's, chosen as DEC-2213 item 5 chose the news fields, with the
   stored length:** `form`, `filed_at`, `excerpt`, `content_hash`, `length_bytes`, and
   `stored_bytes`, the byte length of the stored text. They sit under `untrusted` beside `item_id`
   and `source` (DEC-2209 item 4), and an `ok` filings read sets the record's `untrusted` flag. The
   item observed is the filing's own record, without the stored length.
6. **`filings.get` reads a range of one filing's stored text through `FilingReader::filing_text`.**
   Its arguments are `item`, a filing id; `offset`, an integer from 0; and `length`, 1 to 8,000.
   Anything else reads nothing and asks no reader. The worker passes the id, the run's cut-off, the
   allowlist version, the offset, and the length, and nothing else.
   - The item's fields are the search's, with `offset` and `text` in place of `excerpt`.
   - The core has already screened the id against the filings the run received (`unknown_item`).
7. **The reader's two answers are distinct (§6.10's outcome table).** `ReadError` gains
   `OutOfRange`, for an offset past the end of the stored text, which the worker gives as
   `refused`/`unknown_item`; and `ExcerptOnly`, for a filing the store holds only as an excerpt,
   which it gives as `failed`/`unavailable`. Any other reader error is `failed`/`unavailable`.
8. **A `filings.get` answer that leaks or oversteps reads nothing.** That covers another item, one
   after the cut-off, one off the allowlist, licensed text, a range at another offset, and text
   longer than the `length` asked. Each gives `failed`/`unavailable`, as DEC-2213 item 7 does for
   news: the worker never shows what the reader should not have given, and never cuts an
   overlong answer down to size, since the reader chose those bytes. The length is the model's own
   bound, not a harness cap, so an answer within it never sets `truncated`.
9. **The ports' defaults read nothing.** `search` and `filing_text` have default bodies answering
   `ReadError::Unavailable`, so the readers that exist keep compiling and read no filing text until
   they serve it (rule 3).

### Rationale

The filings search is the seed's filings read with the model's narrowing added, exactly as the news
search is the seed's news read: every filter the seed applies is applied again, and the form is
filtered too, so the model's arguments can only show less than the run's allowlist and scope allow.
The stored length lets the model choose a `filings.get` range that starts inside the text. Keeping
`out_of_range` apart from `excerpt_only` follows the spec's table: one is the model's mistake, which
`unknown_item` tells it, and the other is the store's, which it is told only as `unavailable`.

### Alternatives considered

- **Accept any form string:** the data service's matching would then decide what the model may ask
  for, and an unlisted form would read nothing without saying so.
- **Cut an overlong `filings.get` answer to `length`:** the reader would then choose which bytes the
  model sees past the range it was asked for; refusing the whole answer is the reading that adds no
  risk (DEC-176).
- **Give `excerpt_only` as `unknown_item`:** that would tell the model the filing does not exist,
  which is the store's gap, not the model's.

<a id="dec-2251"></a>

## DEC-2251: A thesis record the source cannot read gives no context

| | |
|---|---|
| **Status** | Accepted (agent; a reversible engineering decision under DEC-79 that tightens a rule) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | An agent (Claude Code, intelligence lane I2), on the coordinator's review of DEC-2249 |
| **Story or claim** | E21-59; [claim #1801](https://github.com/kunwarshivam/mandate/issues/1801) |

### Context

DEC-2249 item 1 says the cell's research source replays each journaled `ThesisProposed` and
`ThesisRevised` from its `admitted` and `reason`. It does not say what happens when a record cannot
give them. DEC-2249 is accepted, so this decision adds that rule instead of editing it.

### Decision

1. **The fold fails closed.** `mandate_daemon::source::fold_research` refuses with
   `FoldError::Unreadable`, naming the member, when any record it folds cannot give a member it
   reads. That covers:
   - a `ThesisProposed` or `ThesisRevised` with no `admitted`, or one that is not a boolean;
   - a `reason` that is no refusal's code;
   - an admitted record with a `reason`, or a refused one without one;
   - a missing or unreadable `thesis_id`, `lineage_id`, `instrument_id`, `revision` or
     `expires_at`;
   - a `ThesisInvalidated` with no `thesis_id`;
   - a surviving admitted thesis (one `expire_theses` keeps) on an instrument the recorded bundle
     names no symbol for.
2. **No fold, no context.** When the fold refuses, the source gives the pass no context, so research
   pauses (DEC-2210 item 4). Nothing is defaulted and no record is skipped.

### Rationale

A record the source cannot read could hide a retirement, an invalidation or an admission. Folding
around it would hand the core a universe or a lineage it never judged. Pausing research is the safe
default (rule 3).

### Alternatives considered

- **Skip the unreadable record.** Rejected: the state the core is handed would be one it never held.

<a id="dec-2252"></a>

## DEC-2252: The filings reads' second pair: the forms' order, and what `filings.get`'s reader answers and shows

| | |
|---|---|
| **Status** | Accepted (agent; reversible engineering decisions under DEC-79, each at least as strict as the rule it implements, DEC-176) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | An agent (Claude Code, lane AH), under DEC-79 |
| **Story or claim** | E19-13 slice 3b, the fourth pair (ADR-0005 delivery slice 3); [claim #1534](https://github.com/kunwarshivam/mandate/issues/1534); after DEC-2250 |

### Context

DEC-2250 settled both filings reads, and its first pair, `filings.search`, landed (#1828, #1846).
Its review asked that the order in which the forms reach the reader be written down, which the
implementation fixes but DEC-2250's text does not; DEC-2250 is accepted, so this records it. The
second pair, `filings.get` (DEC-2250 items 6 to 9), needs two things DEC-2250 leaves to the ports:
what the reader's answer holds, and which of its members the result shows.

### Decision

1. **The forms reach the reader in the pinned list's order** (`10-K`, `10-K/A`, `10-Q`, `10-Q/A`,
   `8-K`, `8-K/A`), whatever order the model named them in, so one set of forms is always one
   query. Forms are matched exactly: a form that differs from a listed one only in case is not on
   the list, and a form and its `/A` amendment are two forms.
2. **`FilingReader::filing_text` answers a `FilingText`:** the filing's record, the byte length of
   its stored text, the offset the range starts at, and the range's text. The worker checks the
   record, the offset, and the text's length against what it asked (DEC-2250 item 8) before it
   shows anything.
3. **`filings.get`'s fields are the search's with the range in place of the excerpt:**
   `content_hash`, `filed_at`, `form`, `length_bytes`, `offset`, `stored_bytes`, and `text`, under
   `untrusted` beside `item_id` and `source`. `length_bytes` is the record's, as the search reports
   it; `stored_bytes` lets the model ask for the next range. The item observed is the filing's own
   record, and an `ok` read sets the record's `untrusted` flag.
4. **Edges the tests pin:** an item known exactly at the cut-off is kept (DP-1), and so is one
   known exactly at the window's start; a range of exactly `length` bytes is within the answer and
   sets no `truncated`.

### Rationale

The list's order makes the reader's query a function of the set of forms alone, so a model cannot
make two queries that differ only in how it ordered its arguments. Carrying the stored length and
the offset in the answer lets the worker refuse a range that is not the one it asked for, instead of
trusting the reader, as DEC-2250 item 8 requires.

### Alternatives considered

- **Pass the forms in the model's order:** the same set of forms would make different queries, for
  no gain.
- **Show only the range's text:** the model could not tell how much of the filing remains without
  another search.

<a id="dec-2253"></a>

## DEC-2253: A version backtest applies a cash dividend, and its ex-date's decision reads the ex-dividend close

| | |
|---|---|
| **Status** | Accepted (agent: a reversible engineering reading under DEC-79 that only tightens, DEC-176; the coordinator accepted it on the plan) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | Lane I4 (Claude Code), under DEC-79 and DEC-176 |
| **Story or claim** | E4-4 slice 3 ([DEC-2245](intelligence.md#dec-2245) item 6); trading spec §8.5; mandate spec §5.2; claim [#1463](https://github.com/kunwarshivam/mandate/issues/1463) |

### Context

[DEC-2245](intelligence.md#dec-2245) applies a split and a cash dividend at 20:00 New York time on the last
trading day before the ex-date (item 2). Slice (2) applied splits. At a split's instant the run
also rescales what it has seen of the market, the last close and the liquidity window (#1776), so
the ex-date's decision quotes the price the bars now show. A dividend raises the same question.
Bars are stored raw (DEC-89), so from the ex-date they are lower by the dividend. The run's last
close is the cum-dividend close of the day before, so the ex-date's decision would quote a price
above the market by the dividend.

### Decision

1. **The ledger and the risk state.** At the dividend's instant the version's account and the
   benchmark's account fold the `CashDividend`: round(Q × `per_share`, 2) is a receivable from the
   ex-date and cash from the pay date (trading spec §8.5). The risk state takes `CashDividend`, so
   the income is in E as a receivable and the ex-date's fall reads as no drawdown (mandate spec
   §5.2, MI-33). The pay date moves the receivable to cash in the ledger, which changes no figure
   the risk state holds, so the risk state takes nothing then.
2. **The ex-date reads the ex-dividend close.** At the dividend's instant the run lowers its last
   close by `per_share`, rounded down onto the tick. It reads that close for the quote, the gate's
   prior close and the last trade. Volumes and the liquidity window are not changed: a dividend
   moves neither shares nor, within a cent, the window's dollar volume. A dividend that would take
   the close to zero or below refuses the run (`not_positive`).
3. **No refusal remains.** `corporate_action_in_range` refuses nothing once dividends apply, so it
   is removed. Stored runs that carry it as a refusal code keep it as recorded.

### Rationale

The fold and the risk state already hold the dividend's arithmetic, so the run only feeds them at
DEC-2245's instant, as it does a split. Item 2 tightens: a lower reference can only lower a buy's
limit or make a sell more fillable, so it adds no risk (DEC-176).

### Alternatives considered

- **Quote the cum-dividend close.** This prices the ex-date's opening above the market by the
  dividend, the same class of error as quoting the pre-split close (#1776's review).
- **Rescale the liquidity window too.** A dividend moves no shares, and the dollar volume it
  changes is the dividend times the volume, which no floor or cap here turns on. That would be
  code without a case.

<a id="dec-2254"></a>

## DEC-2254: The cell's gateway holds research to the mandate's daily cap

| | |
|---|---|
| **Status** | Accepted (agent; a reversible engineering decision under DEC-79 that tightens a rule and sets no new value) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | An agent (Claude Code, intelligence lane I2), on the coordinator's review of DEC-2249 |
| **Story or claim** | E21-59; [claim #1801](https://github.com/kunwarshivam/mandate/issues/1801) |

### Context

The core stops starting research once the day's spend reaches `behavior.research.cost_cap_usd_per_day`
tightened by the policy overlay (check 7, DEC-120). The gateway can hold each research call to the
same cap (`Caps::with_research_daily`, inference spec §7.3). The cell's worker built its gateway
with the workspace's monthly cap and the deployment's cap only, so a call reserved past the day's
cap would have been sent.

### Decision

1. **The same value, no new one.** The cell's daily research cap is
   `PolicyOverlay::internal_research_profile().effective_cost_cap(cost_cap_usd_per_day)`, read from
   the deployed mandate's document. These are the value and overlay the cell's source hands the core.
   `mandate_daemon::cell::research_daily_cap` reads it.
2. **No research envelope, no research spend.** A mandate whose `behavior.research` is `null`
   gets a daily cap of zero. The core starts no research for it, and the gateway refuses any
   research call it is handed.
3. **Fail closed.** A cap the document cannot give stops the start with
   `mandate_document_unavailable`. That covers a missing `behavior.research`, a missing or
   unparseable `cost_cap_usd_per_day`, and a cap the gateway's `Usd6` cannot hold. A cap above the
   workspace's monthly budget stops it with `WorkerError::DailyCapAboveWorkspace`.
4. **The gateway enforces it.** `RecordedWorker::capped` builds the gateway with the cap for the
   agent. A call whose reservation would take the agent's research spend for the risk day past the
   cap is refused `budget_exhausted`: nothing is reserved and nothing is sent. `cell::research`
   builds its worker with `capped`.
5. **Read at start.** The cap is read from the mandate version the process starts with. A later
   version that raises it leaves the gateway tighter until a restart. One that lowers it still
   binds through check 7, which reads the current version.

### Rationale

The cap is a founder-confirmed envelope field (rule 11). Enforcing it in the gateway as well as the
core adds a second, independent stop and no new number. A spending value this decision invented
would be a founder decision (DEC-79).

### Alternatives considered

- **Clamp a cap above the workspace's budget to it.** Rejected: that is a value no one confirmed.
  Refusing to start says what is wrong.
- **Re-read the cap on every call.** Deferred: the worker holds one gateway. Check 7 already
  reads the current version, so starting with the confirmed value only ever errs tighter.

<a id="dec-2255"></a>

## DEC-2255: The dispatch's quote read: `market.quote`'s arguments, the quote item, and its sanity flag

| | |
|---|---|
| **Status** | Accepted (agent; reversible engineering decisions under DEC-79, each at least as strict as the rule it implements, DEC-176) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | An agent (Claude Code, lane AH), under DEC-79 |
| **Story or claim** | E19-13 slice 3b, the fifth and last pair (ADR-0005 delivery slice 3); [claim #1534](https://github.com/kunwarshivam/mandate/issues/1534); after DEC-2209 |

### Context

`market.quote` is the last read of slice 3b (agent harness spec v0.2 §6.10): "the last quote at or
before the cut-off, with its time and sanity flag", for 1 to 10 symbols. The worker has no quote
yet: the ports carry bars only, there is no quote item, and §6.3's seed `quote` read is not built.
The sanity flag the spec names is the trading spec's (§4, §8.2's risk mark: `0 < bid ≤ ask`, a
spread within the profile's limit, fresher than its staleness threshold), but a research run has no
data profile. DEC-2209's items 2 to 9 apply unchanged; this records what is new. It lands as one
DEC-77 pair.

### Decision

1. **`market.quote`'s arguments are §6.10's, read strictly:** `symbols`, 1 to 10 data-universe
   symbols, read as a set. Anything else reads nothing and gives `failed`/`unavailable`, and a
   symbol outside the data universe refuses the call whole before any reader is asked
   (`out_of_universe`, DEC-2209 item 10).
2. **`BarReader` gains `last_quote(symbol, cut_off)`,** which answers the last quote at or before
   the cut-off, or none. Its default answers `ReadError::Unavailable`, so the readers that exist
   keep compiling and read no quote until they serve one (rule 3). The worker asks once per
   symbol, in symbol order, and any reader error reads nothing for the whole call.
3. **Every answer is filtered again:** a quote for another symbol, or one after the cut-off, is
   dropped, and the symbol shows no quote (HI-13). Quotes come in symbol order, one per symbol, at
   most 10.
4. **A `Quote` holds the symbol, its time, and each side's price and size.** A side with no order
   has no price (a one-sided quote), since a price is always positive; a size is never negative.
5. **The sanity flag is structural only:** `sane` is true exactly when both sides have a price and
   the bid is at or below the ask. The spread limit and the staleness threshold belong to a data
   profile, which a research run does not have, so the flag judges neither; the quote's time is
   shown so the model can see its age. The flag is an opinion's input, never a mark: the risk gate
   and the executor price from their own reads (`AGENTS.md` rule 4).
6. **A quote is a platform item, as a bar is.** Its id is `quote:<symbol>:<seconds>`, its source
   `platform.quotes`, and its kind `quote`. `quote:` joins `bar:` and `instrument:` as a namespace
   no news or filings reader may use (DEC-2213 item 3). Its fields, under `untrusted` beside
   `item_id` and `source` (DEC-2209 item 4): `symbol`, `quoted_at`, `bid`, `bid_size`, `ask`,
   `ask_size`, and `sane`, with a missing price as `null`. A quote read does not set the record's
   `untrusted` flag, as a bar read does not.

### Rationale

Asking per symbol keeps the reader's answer one quote, which the worker can check against what it
asked. Computing the flag in the worker, from the quote alone, means no reader can call a crossed
quote sane, and leaving the profile's limits out says plainly what the flag does not judge rather
than inventing a research profile.

### Alternatives considered

- **Take the reader's sanity flag:** the reader could then call any quote sane.
- **Judge spread and staleness with the `iex` profile's limits:** a research run would then depend
  on an execution setting, and a profile change would change research results.
- **A new `QuoteReader` in `Readers`:** every reader set in the runtime and the daemon would change
  for one method; a defaulted method on `BarReader`, the market-data port, changes none.

<a id="dec-2256"></a>

## DEC-2256: A turn's request: what the prompt's prefix carries, and how the worker builds the gateway call from the core's turn

| | |
|---|---|
| **Status** | Accepted (agent; reversible engineering decisions under DEC-79, each at least as strict as the rule it implements, DEC-176) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | An agent (Claude Code, lane AH), under DEC-79; the coordinator agreed the reading of item 1 |
| **Story or claim** | E19-13 slice 3c, the turn-to-gateway request builder (ADR-0005 delivery slice 3); [claim #1534](https://github.com/kunwarshivam/mandate/issues/1534); after DEC-2206 and DEC-2209 |

### Context

The core emits each turn as `ResearchRequest::Turn` (DEC-2206 item 5): the call's plan, the
`TurnPlan`, and the transcript as artifact references. The gateway already takes a turn's
`CallRequest` (`run_kind`, `turns`, `tools`, `spend_ceiling`) and checks it against the entry
(inference spec §3.1). Nothing builds the second from the first, and the daemon's worker drops the
turn. Slice 3c is that builder.

The agent harness spec disagrees with itself on one point. §6.9 step 3 asks for "the run's kind and
its remaining turns and tool calls, among the typed inputs (§6.4)", and §6.4 lists them as inputs.
But §6.12 says "everything before the transcript is the same for every turn of a run", and the
typed inputs sit before the transcript; counts that change every turn would change the prefix every
turn and defeat the provider's prompt cache that §6.12's order exists for.

### Decision

1. **The prompt's prefix carries only what is fixed for the run** (this settles the §6.9/§6.12
   conflict): the run's kind, as the request's `run_kind`, which fixes its menu, its tool schemas,
   and its pinned `loop_bounds` (the starting `max_turns` and `max_tool_calls`). The turns and tool
   calls left are not typed inputs. The turn structure carries them, and the gateway enforces it:
   the menu narrows to the action tools in the last turn and to none once no tool call is left
   (§6.9 step 3), and the gateway refuses a transcript at or past `max_turns`. Whether the template
   states the starting bounds in the system prompt is the template's, and a pinned template change.
   §6.4 and §6.9 step 3 are reworded to match, in this change.
2. **A pure builder in the worker crate, `mandate_research_run::turn`, turns the core's turn into
   the gateway's call.** It takes the call's plan, the `TurnPlan`, the run kind, the seed's
   retrieval, the run's context, the earlier turns as bytes, and the dispatch instant, and gives a
   `CallRequest` with:
   - the seed's typed inputs, exactly as the single call renders them;
   - `run_kind`: the kind's spelling (`ideate`, `monitor`), which `RunKind::as_str` gives;
   - `turns`: each earlier turn's reply bytes and its results as UTF-8 text in call order, oldest
     first;
   - `tools`: the `TurnPlan`'s menu, by name, in the menu's order;
   - `spend_ceiling`: the `TurnPlan`'s;
   - `deadline`: the dispatch instant plus the lesser of the call's `deadline_ms` and the
     `TurnPlan`'s `time_left_ms` (§6.9 step 3), applied here once;
   - `observations`: every observation the call plan names, the seed's first and the reads' after
     (§6.9 step 4).
3. **The seed's items pair with the first observations.** The call plan names the seed's
   observations first, in the seed's order, then each read's; the builder pairs the seed's items
   with that many leading observations, and a plan naming fewer than the seed holds is refused
   `observations_mismatch`, as a single call's is.
4. **The builder refuses a transcript that cannot be this turn's:** one whose length is not the
   turn's number less one, and a result that is not UTF-8. It does not wrap or delimit results:
   the gateway puts each in its data section and refuses one carrying a section marker (inference
   spec §3.1), and it re-validates each reply against the menu and counts its results.
5. **Loading the transcript's artifacts is the shell's.** The core holds artifact references
   (DEC-2206 item 5); the worker loads the bytes and hands them to the builder, and wiring the
   worker's turn path follows in a later piece. Memory and run summaries in the prefix (§6.12)
   stay empty until slice 3d.

### Rationale

The run kind and its pinned bounds are the same on every turn, so keeping only them in the prefix
keeps the prefix byte-identical across a run's turns, as §6.12 requires, while the model still
cannot overrun: the menu it is offered and the gateway's transcript check bound it on every turn.
Putting the builder beside the single call, in the pure crate, means one place renders a research
call's inputs, and the run's determinism (DEC-2203 item 11) rests on a pure function of recorded
inputs.

### Alternatives considered

- **Per-turn counts as typed inputs (§6.9's text):** the prefix would change every turn, contrary
  to §6.12, and the cache would never hit.
- **A per-turn input after the transcript:** it needs a gateway template slot after the
  transcript, outside this lane, and states nothing the turn structure does not already enforce.
- **Build the request in the runtime or the daemon:** the inputs would then be rendered in two
  places, and the shell is not pure.

<a id="dec-2258"></a>

## DEC-2258: The meter stream is journaled through mandate-store, one writer per workspace, failing closed

| | |
|---|---|
| **Status** | Accepted (agent; reversible engineering choices under DEC-79 that tighten rules and set no new value) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | An agent (Claude Code, intelligence lane I2) |
| **Story or claim** | E21-61; [claim #1925](https://github.com/kunwarshivam/mandate/issues/1925); DEC-998 items 5 and 8; DEC-2257 item 2 |

### Context

The gateway's meter folds spend from its store's records, and every store so far is in memory.
DEC-998 item 8 allows no live call until the meter stream `meter:{workspace_id}` (journal spec
§9.19) is journaled, because an in-memory store forgets open reservations on a crash.
`mandate-journal` already registers the stream's `StreamOpened`, `MeterReserved` and
`MeterSettled`.

### Decision

1. **The store.** `mandate_store::meter::JournalMeter` implements `mandate_gateway::meter::MeterStore`
   over the workspace's `meter:{workspace_id}` through a `mandate_store::Store`, so the in-memory
   journal and Postgres serve it alike. It takes a writer epoch at open: one writer per workspace,
   and a newer writer fences an older one, whose appends then fail `Fenced`.
2. **What it writes.** An empty stream gets its `StreamOpened` in the same batch as the first record.
   Each record is a draft at the record's own `event_id`, with the payload `MeterRecord::payload`
   gives, written by a `system` actor.
3. **What it reads.** `read` returns the stream's `MeterReserved` and `MeterSettled` records, in
   stream order, each read back by `MeterRecord::from_journal`, the exact inverse of `payload`. A
   payload with a member missing, an unlisted member, or a value out of range reads as nothing.
4. **Fail closed.** A stream the store cannot read, a record it cannot read back, or a writer
   with no epoch makes `read` and `append` fail `Unavailable`, so the gateway refuses the call
   `meter_unavailable` and sends nothing.
5. **Rule 158 first.** `append` checks rule 158 against the stream's records before it writes,
   as `MemoryMeter` does. A draft the journal refuses is surfaced as `Rule158`.
6. **An unknown commit only over-counts.** An append the store cannot confirm (unavailable or
   ambiguous) fails `Unavailable`. If the reservation was in fact committed, the next read counts
   it at its maximum, which never lowers spend (DEC-998 item 5).

### Rationale

The meter's caps are only as true as its memory. Journaling through the same port as every other
long-running writer gives the stream the journal's validation, fencing and durability, and a
restart folds exactly what was reserved.

### Alternatives considered

- **A meter table of its own in Postgres.** Rejected: §9.19 already defines the stream, and the
  journal gives fencing and validation for free.

<a id="dec-2259"></a>

## DEC-2259: The cell's meter is the journaled stream; the hold stays as a cross-check, and the last reservation comes back

| | |
|---|---|
| **Status** | Accepted (agent; a reversible engineering decision under DEC-79 that loosens nothing: a restart is no longer held only because the journal now holds what the hold guarded) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | An agent (Claude Code, intelligence lane I2) |
| **Story or claim** | E21-61; [claim #1925](https://github.com/kunwarshivam/mandate/issues/1925); DEC-2249 item 3; DEC-2258 |

### Context

DEC-2249 item 3 has two parts:
- the cell's worker and source share one in-memory meter;
- after a restart, the meter is held for the rest of the risk day, because a reservation made
  before the restart is no longer counted.

It also leaves `last_reservation` `None` until E21-61 journals the meter stream. DEC-2258 journals
that stream through `mandate_store::meter::JournalMeter`. DEC-2249 is Accepted, so this decision
changes its item 3 instead of editing it.

### Decision

1. **One journaled meter.** `CellMeter::over` builds the cell's meter over any `MeterStore`. The
   process passes `cell::research_metered` a `JournalMeter` over its own journal connection, and the
   worker's gateway and the research source share it. `cell::research` keeps the in-memory store,
   for fixtures and tests.
2. **The hold stays, as a cross-check.** The rule stays as written: the meter is held while the
   agent stream records a reserved call on the risk day that the store does not hold, or a call
   record it cannot read. With the journaled store, a clean restart holds nothing, because the
   journal holds every reservation the gateway made. Only an agent stream that disagrees with the
   meter stream, or a record that cannot be read, still holds the day.
3. **The last reservation comes back.** `last_reservation` is the `event_time` of the agent's
   latest research `MeterReserved` on the meter stream. Before the first one it is `None`. A meter
   record the source cannot read gives the pass no context (DEC-2210 item 4).
4. **What changes in DEC-2249.** Item 3's "after a restart the meter is empty" no longer holds
   for a process given a journal. The last bullet's `None` is replaced by item 3 above. Nothing
   else in DEC-2249 changes.

### Rationale

The hold existed because memory forgets. The journal does not forget, so the hold is kept only for
what still signals a fault. The last reservation keeps `due()` spacing runs across a restart, so
lifting the hold does not let a restarted cell call again sooner than it would have without one.

### Alternatives considered

- **Drop the hold.** Rejected: an agent stream recording a reservation the meter stream lacks
  means the two disagree. That still has to fail closed.

<a id="dec-2260"></a>

## DEC-2260: The cell's worker runs a dispatch: its scope, its bounds, and what it stores before it answers

| | |
|---|---|
| **Status** | Accepted (agent; reversible engineering decisions under DEC-79, each at least as strict as the rule it implements, DEC-176) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | An agent (Claude Code, lane AH), under DEC-79 |
| **Story or claim** | E19-13 slice 3c, pair B, the worker's dispatch path (ADR-0005 delivery slice 3); [claim #1534](https://github.com/kunwarshivam/mandate/issues/1534); after DEC-2206, DEC-2209 and DEC-2210 |

### Context

The core asks the worker to run a reply's reads and thinks as `ResearchRequest::Dispatch` (DEC-2206
item 6): the run, the turn, the calls in reply order, the run's result bytes left, and the run's
frozen cut-off and allowlist version. It expects `ResearchInput::Dispatched` back, each result's
content already stored (agent harness spec v0.2 §6.9 step 5). The pure `dispatch` runs the calls
(DEC-2209), but the cell's `RecordedWorker` drops the request. Two inputs `dispatch` needs are not
in the request: the run's loop bounds, which are the agentic entry's pinned `loop_bounds`, and the
allowlist's sources. The cell's recorded bundle pins a single-call entry, which has no bounds, and
the cell builds its research with `agentic: None`, so no dispatch reaches the cell today. This piece
makes the worker ready for the agentic bundle that slice 3e records.

### Decision

1. **The worker runs a dispatch only with the loop's bounds.** `RecordedWorker::with_loop(bounds)`
   gives it the agentic entry's pinned `loop_bounds`; without them it runs nothing, since a dispatch
   for a single-call entry cannot be the core's.
2. **The worker runs a dispatch only for a run it started** and has not cancelled.
3. **The scope is the run's frozen scope, rebuilt.** The cut-off is the request's. The allowlist is
   the pass's, used only when its version is the request's `allowlist_version`; a pass whose
   allowlist has moved reads nothing for the run. The data universe is the pass's, as the run's
   start reads it, and nothing is held, as at the start (DEC-2210).
4. **The readers are the recorded plane's,** the same that the run's retrieval read, and the calls,
   the turn and the result bytes left are the request's.
5. **Everything an answer names is stored before the answer is kept.** For each result, in call
   order, the worker stores its content, then each of its items' records as `capture` gives them at
   the request's cut-off. The answer is `ResearchInput::Dispatched` with each result's content as its
   digest and its length in bytes, stamped at the risk clock of the pass it is handed in, as every
   other answer is.
6. **Any failure answers nothing for the whole dispatch** (`AGENTS.md` rule 3): no bounds, a run not
   started, a moved allowlist, a dispatch `dispatch` refuses (an action among the calls), a record
   that cannot be captured, a content longer than `u32` bytes, or a store the sink refuses. A
   partial answer would not be exactly the dispatch's calls, which the core refuses. The core's run
   bound then ends the run at its deadline. `RecordedWorker::dispatch` returns the reason as a
   `DispatchError`, and `hand` drops it.

### Rationale

The bounds pin what a result may hold, so taking them from anywhere but the entry the run pinned
would let a result exceed what the model was promised; until the agentic bundle exists, refusing to
run is the only reading that adds nothing. Reading the frozen allowlist's version, rather than the
pass's newest sources, keeps a run's reads in the scope its seed was read in (§6.9 step 1). Storing
before answering is DEC-2210 item 3's rule for retrieval and the call, kept here.

### Alternatives considered

- **Read the bounds from the bundle's entry:** the recorded entry is single-call and has none; a
  bounds field in the bundle waits for slice 3e's agentic recording.
- **Read with the pass's newest allowlist:** a source removed mid-run would still be read, and one
  added would be read in a run that froze before it.
- **Answer the results that did store:** the core refuses results that are not exactly the
  dispatch's calls, and a missing result would read as never dispatched.

<a id="dec-2261"></a>

## DEC-2261: A refused turn ends its agentic run `error` and withdraws its pin

| | |
|---|---|
| **Status** | Accepted (agent; reversible engineering decisions under DEC-79, each at least as strict as the rule it implements, DEC-176) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | An agent (Claude Code, lane AH), under DEC-79; the coordinator approved the order |
| **Story or claim** | E19-13, after slice 3c's pair B (ADR-0005 delivery slice 3); [claim #1534](https://github.com/kunwarshivam/mandate/issues/1534); after DEC-2206 and DEC-2256 |

### Context

The gateway refuses a call it could not have been asked for before it reserves or sends anything
(inference spec §3.1): `unknown_model`, `hash_mismatch`, `not_agentic`, `run_kind`, `tools`, or
`transcript`. The worker hands such a refusal to the core as `ResearchInput::Refused`. For a
`single_call` run, the core withdraws the run's pin, and research under that pin stops until a new
version. For an agentic run's turn, the core has no rule: it answers `not_interpreted`, the daemon
raises `research_input_refused`, and the run waits, journaling nothing, until its deadline ends it.
The agent harness spec says each bound the gateway refuses ends the run `error` (HI-8), but it says
nothing of a refused request.

### Decision

1. **A refused turn ends the run `error` at once,** with `RunSummaryRecorded` and `RunEnded`, as
   every other end does. Nothing records the refused call, because the gateway made none. No turn
   follows, and nothing is asked of the worker.
2. **The run's pin is withdrawn,** as a refused single call's is. A refused request means the pinned
   entry cannot serve the run the core asked for, so the next turn or run under it would be refused
   too. A new version re-enables research.
3. **A refusal is the turn's only while the turn is out.** While the run's reads are out, the
   refusal is not interpreted, and the run stays as it was. A refusal naming another run is not
   this run's either.

### Rationale

Ending at once rather than waiting for the deadline adds no risk: a refused call reserved nothing
and returned nothing, so no output could be lost. The journal also now says why the run ended.
Withdrawing the pin matches the single call's rule, so the two run kinds treat the same gateway
verdict alike. It is the stricter reading: research stops rather than retrying a request that cannot
be served.

### Alternatives considered

- **Wait for the deadline (today's behaviour):** the run holds the agent's one research slot until
  then, and its journal never says why it ended.
- **End the run but keep the pin:** the next run would be refused the same way, every interval, each
  raising an alert.

<a id="dec-2262"></a>

## DEC-2262: An agentic research entry's pinned loop: `final_output`, the served read and think tools, and each run kind's output schema

| | |
|---|---|
| **Status** | Accepted (agent; reversible engineering decisions under DEC-79, each at least as strict as the rule it implements, DEC-176); the coordinator agreed the scope |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | An agent (Claude Code, lane AH), under DEC-79 |
| **Story or claim** | E19-13 slice 3c, piece C1 (ADR-0005 delivery slice 3); [claim #1534](https://github.com/kunwarshivam/mandate/issues/1534); after DEC-2203, DEC-2206, DEC-2209 and DEC-2256 |

### Context

The worker's turn path (piece C2) sends each turn through the gateway, which offers only tools its
registry entry declares and checks every reply against them (inference spec INF-5). The harness has
no agentic entry content to register: no tool declarations and no per-kind output schemas. A turn
through the cell's gateway is therefore refused `not_agentic`. This piece is the pure part of that
content. Wiring it into the recorded bundle and the worker's registry is C2's.

### Decision

1. **The loop is `final_output`** (agent harness spec §6.9, DEC-2203 item 5): the run reads and
   thinks, and its last reply is §6.5's structured output, judged once. The gateway accepts
   `final_output` only with no action tool (`Agentic::well_formed`), so no action tool is declared.
   `propose_thesis` and `invalidate` belong to an `action_tools` entry, a later decision. §6.10
   declares `propose_exit` only once E19-14 lands.
2. **The declared tools are the read tools the worker's dispatch serves and the two think tools:**
   `filings.get`, `filings.search`, `market.bars`, `market.quote`, `news.get`, `news.search`,
   `note` and `plan`, strictly sorted by name.
   - `memory.*`, `records.query` and `mandate.read` are declared with slice 3d, which serves them,
     so the model is never offered a tool that can only fail.
   - Each tool is on both menus, `ideate` and `monitor`. §6.10 gives a `monitor` run every read and
     think tool.
   - Each tool's result cap is the loop's `max_result_bytes`.
3. **Each argument schema is §6.10's, in the gateway's language:** every member required, and
   arrays, strings, integers and form enums capped as §6.10 states.
   - Ranges the language cannot state stay the dispatch's strict read's (DEC-2209), as the output
     schema's do (DEC-1120 item 3). These are: at least one symbol, a term's characters, an item
     the run received, and byte lengths.
   - `filings.get`'s `offset` is capped at 2³² − 1.
4. **No argument or output schema has a member an order, a quantity, a size, or a price could be
   written in** (`AGENTS.md` rule 4). A test walks every schema for a fixed list of such names. The
   output schemas are §6.5's opinion fields only.
5. **Each run kind's output schema is pinned:**
   - `ideate`'s is the single call's (§6.5).
   - `monitor`'s has only `invalidations`, the same as `ideate`'s, so the gateway refuses a monitor
     reply carrying a thesis (§6.9).
   - The typed `watch` list of §6.12's run summary is added with slice 3d's summaries.
6. **The bounds are the ones the caller pins, converted to the gateway's types.** A loop the
   registry would refuse is refused here first, `unpinnable_loop`: for example, no turns, more calls
   per turn than per run, or more result bytes per read than per run.
7. **The bounds and the menus are envelope content** (rule 11): they are pinned by the entry and
   inside its content hash. No runtime path changes them, and a change is a new entry and a new pin.
   Which entry a bundle names is C2's (`research.json`), confirmed with the mandate's pin.

### Rationale

Declaring only what the worker serves keeps the loop's promise to the model honest: every offered
tool can answer. `final_output` keeps every proposal on the judged path that `single_call` already
uses, so no action is mediated mid-run until an `action_tools` decision is taken. Building the loop
in the pure crate, beside the single call's schema, means the ideate schema cannot drift from the
single call's.

### Alternatives considered

- **Declare every §6.10 read now:** `memory.*`, `records.query` and `mandate.read` would be offered
  and always fail until slice 3d, which wastes turns and teaches the model nothing true.
- **Declare the action tools as Action:** the gateway refuses that combination with `final_output`.
  `action_tools` needs mediation that the core does not yet do.
- **Let the monitor run use ideate's schema:** a monitor reply could then carry a thesis, which only
  the core's later check would catch.

<a id="dec-2263"></a>

## DEC-2263: A recorded research entry may name the agentic loop, and its content pins the loop and its own system text

| | |
|---|---|
| **Status** | Accepted (agent; reversible engineering decisions under DEC-79, each at least as strict as the rule it implements, DEC-176); the coordinator approved the plan and its two conditions |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | An agent (Claude Code, lane AH), under DEC-79 |
| **Story or claim** | E19-13 slice 3c, piece C2a (ADR-0005 delivery slice 3); [claim #1534](https://github.com/kunwarshivam/mandate/issues/1534); after DEC-2247, DEC-2260 and DEC-2262 |

### Context

DEC-2262 built an agentic research entry's pinned loop as a pure value. A recorded bundle still
reads only single-call entries (DEC-2247 item 1), so its registry offers no agentic loop. The
cell's worker also takes its loop bounds only from `with_loop`, which nothing outside the tests
calls (DEC-2260 item 1). This piece lets a recording name the loop, puts the loop into the entry's
pinned content, and gives the worker the entry's bounds.

### Decision

1. **A recording's `entry` may carry an `agentic` member naming the loop.**
   - `"v0_2"` is §6.11's bounds (`LoopBounds::v0_2`).
   - Any other value, of any type, is refused `EntryError::Member("agentic")`.
   - With no member the entry is `single_call`, as before. Every existing recording, pin and
     content hash is unchanged.
   - The member is envelope content (rule 11): the bundle pins it inside the entry's content
     hash, and the mandate's pin names that hash. No runtime path changes it.
2. **An agentic entry's content is DEC-2262's loop:** `Mode::Agentic`, with the entry's own output
   bounds for the run kinds' schemas. The plan, the user text, its inputs, the output schema and
   everything else stay the single call's.
3. **An agentic entry pins its own system text.**
   - It is the single call's, plus what the offered tools may do. They only read daily bars,
     quotes, allowlisted news and public filings at the cut-off, or keep a note or a plan, and none
     can place, size, approve or cancel anything.
   - It also says which members each run kind answers with.
   - It holds no instruction to trade, to size or to recommend, and a test checks its words.
   - The content hash of the demo recording's agentic entry is pinned in a test, so any change to
     the text or the loop is a new pin.
4. **An unpinnable loop is refused when its bundle registers, never at a call.**
   - The content carries the loop as built, and the registry's own check refuses one it may not
     pin (`RegistryError::Agentic`). So the bundle's `registry()` answers
     `BundleError::Unregistered`, and no worker is built (`WorkerError::Bundle`).
   - `agentic_loop` stays the checked form, for callers that want the refusal before registering.
5. **The worker takes its loop bounds from the bundle's entry.** A single-call bundle gives none, so
   the worker still runs no dispatch for it (DEC-2260 item 1). `with_loop` stays for tests.

### Rationale

Naming the loop by version, rather than spelling out its bounds in the recording, keeps one source
for §6.11's defaults, and a recording cannot set a bound the spec does not. Carrying the unchecked
loop in the content, so that the registry checks it, keeps `content()` infallible for its callers
(the pin, `model register`). It also puts the refusal where every other unpinnable entry is
refused. Pinning the system text's hash makes a wording change visible in review, as the
coordinator asked.

### Alternatives considered

- **Spell the bounds out in `research.json`:** a recording could pin bounds §6.11 does not give,
  and each bound would need its own range check.
- **Make `content()` fallible:** three production callers and every test would change, for a
  refusal the registry already makes.
- **Reuse the single call's system text:** it tells the model to answer at once with one object, so
  a model would never call a tool.

<a id="dec-2264"></a>

## DEC-2264: The cell's worker sends a turn: its run kind, its transcript loaded through the sink, and a missing artifact answering nothing

| | |
|---|---|
| **Status** | Accepted (agent; reversible engineering decisions under DEC-79, each at least as strict as the rule it implements, DEC-176); the coordinator approved the missing-artifact rule and the plan |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | An agent (Claude Code, lane AH), under DEC-79 |
| **Story or claim** | E19-13 slice 3c, piece C2b (ADR-0005 delivery slice 3); [claim #1534](https://github.com/kunwarshivam/mandate/issues/1534); after DEC-2206, DEC-2256, DEC-2260, DEC-2261 and DEC-2263 |

### Context

The core asks for each agentic turn with `ResearchRequest::Turn`: the call's plan, the loop's
`TurnPlan`, and the transcript as artifact references (DEC-2206 item 5). The worker's `call_turn`
builds and sends a turn from the bytes (DEC-2256). The cell's worker still drops the request. The
request also does not carry the run's kind, which the turn's call must name. This piece closes
both gaps.

### Decision

1. **`TurnRequest` carries the run's kind,** fixed at its start (§6.9 step 1). The core's one
   construction site fills it from the loop's start.
2. **The sink gives artifacts back.**
   - `ArtifactSink::get(digest)` answers the stored bytes, or `None`.
   - Its default answers `None`, so a sink that only stores serves no transcript.
   - The store's sink reads through the journal's checked read, so bytes that do not hash to the
     reference are none.
3. **The worker sends a turn** only under a bundle naming the agentic loop (DEC-2263 item 5), and
   only for a run it started and has not cancelled.
   - **Transcript:** it loads each earlier turn's reply and results through the sink.
   - **Building:** it builds the turn with `call_turn` over the run's kept seed and context, the
     request's plan and kind, and the dispatch instant on the gateway's clock.
   - **Sending:** it sends the turn through the gateway over the bundle's recorded reply, as the
     single call does. A recorded multi-turn script is slice 3e's.
4. **A transcript artifact the sink cannot give answers nothing** (rule 3). The core has no
   refusal code for it: `transcript` is the gateway's verdict on a transcript it was handed, not on
   one the worker could not load. So nothing is sent, nothing is stored, and the core's run bound
   ends the run at its deadline. `TurnError::Missing` names the reason.
5. **The answer is the single call's.**
   - The record's prompt and response are stored before `Called`, and any refused store answers
     nothing.
   - A turn the gateway refuses is answered `Refused` with its code, which the core ends `error`
     (DEC-2261).
   - A turn that cannot be built sends nothing (DEC-2256 item 4) and answers nothing.
   - Each call scripts the recorded reply for itself alone, so a reply left by a call never sent
     cannot answer a later one.

### Rationale

A missing artifact is the platform's own failure, not the model's or the gateway's. Answering
`transcript` would journal a gateway verdict the gateway never gave, and would withdraw the pin
(DEC-2261 item 2) for a storage fault. Answering nothing adds no risk: no call was reserved or sent,
and the run ends at its bound as any silent worker's does. Reading through the checked read means a
corrupted artifact is never replayed to the model as an earlier turn.

### Alternatives considered

- **Answer `Refused(transcript)`:** it would mislabel a storage fault as the gateway's verdict and
  withdraw the pin.
- **Send the turn without the missing turn:** the gateway refuses a transcript whose length is not
  the turn's number less one, and the model would see a history that never happened.
- **Infer the run kind from the menu:** the menus may overlap, and the kind is the core's.

<a id="dec-2265"></a>

## DEC-2265: The runtime reaches the model-gateway service over one framed canonical-JSON exchange per call

| | |
|---|---|
| **Status** | Accepted (agent; reversible engineering choices under DEC-79 that add no dependency, no value and no new egress); item 6 amended by [DEC-2279](intelligence.md#dec-2279) |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | An agent (Claude Code, intelligence lane I2) |
| **Story or claim** | E21-62; DEC-2257; inference spec §1.2, §3.1, §3.2, §3.6, §8.2 |

### Context

DEC-2257 makes the chart's `model-gateway` service the only live caller of a model provider; the
runtime reaches a model only through it. The specs fix the call contract (inference §3.1 and §3.2):
the worker sends one request, the gateway returns one `CallRecord` or a typed `RequestError`, and
the runtime journals `ModelInvocationRecorded` and stores the prompt and response itself (§3.6). The
chart opens TCP 8080 from the workspace's runtime pods to its gateway pods. Nothing says how the two
exchange a call: `CallRequest` and `CallRecord` have no wire form, and the daemon may use no HTTP
crate (`xtask/layers.toml`).

### Decision

1. **One exchange per call.** The runtime opens one TCP connection to the gateway, writes one
   request frame, reads one reply frame, and closes. A frame is a 4-byte big-endian length, then
   that many bytes of canonical JSON (`mandate_canon`). A request frame over 4 MiB, the cap
   `HttpsTransport` reads a provider's body under, is refused unread. A reply frame carries the
   prompt and up to 4 MiB of response body, both as hex, so it is bounded at 24 MiB; the service
   never writes a longer one, and the caller refuses one unread. The reply bound is the larger
   because hex doubles every byte and a reply holds two bodies, the prompt built from a request
   within its 4 MiB and the provider's body within its 4 MiB: 16 MiB at most, with room for the
   record's other members. The request bound stays at 4 MiB because the service reads requests
   from any peer the NetworkPolicy admits, while a reply is read only from the gateway the caller
   dialled. A reply the service cannot write
   reads to the caller as a lost reply (item 3). The framing (`mandate_gateway::frame`) is generic
   over `std::io`, so the pure gateway crate opens no socket; both processes use `std::net`, so
   neither gains a dependency.
2. **Refuse before working.** The service refuses a frame whose length prefix exceeds the bound
   before it reads or decodes any of the body, and a request naming another workspace before any
   reservation or provider call (`foreign_workspace`, INF-9). A malformed or truncated frame closes
   the connection with a typed error, never a partial call.
3. **The reservation comes first.** The service journals the call's `MeterReserved` before the
   first byte goes to the provider (inference §3.6, DEC-2258). A crash or a dropped connection after
   the spend leaves the reservation counted at its maximum against the cap. The runtime treats a
   lost reply as a failed call and never retries it within the same reservation or under the same
   `call_id`.
4. **The deadline holds on both sides.** The runtime's socket timeouts run to the request's
   absolute `deadline`. A reply that does not arrive by then is the caller's `deadline_exceeded`,
   whatever the gateway does, so a call never outlives its deadline from the caller's side (§1.2).
   A connection lost before the reply is never retried under the same `call_id` (§9).
5. **The wire form.** `mandate_gateway::wire` encodes a `CallRequest`, and a reply as either its
   `CallRecord` or its `RequestError`:
   - every member is named, with `wire_version` 1;
   - integers past 2^53, money, times and token counts are canonical text;
   - bytes are lowercase hex;
   - `inputs` is a list of `[name, value]` pairs, because input names need not be canonical keys.
   - a record's `output` is `null` when it has none, else a one-element list holding it, so an
     output that is JSON `null` is not mistaken for no output.

   Decoding is the exact inverse. A frame with a member missing, unlisted or out of range, or not
   in canonical form, reads as nothing, so a frame either decodes to exactly what was sent or is
   refused.
6. **The record comes back whole.** The reply carries the prompt and response bytes inline, so the
   runtime stores them as artifacts before the core journals the record, as it does today. The
   gateway has no artifact-store egress.
7. **One workspace per gateway.** The service serves the one workspace it is started for. A request
   naming another workspace is refused `foreign_workspace` (INF-9). The NetworkPolicy's label is
   the caller's only identity in this decision; an authenticated channel is recorded as a residual
   below.
8. **One replica for now.** The service runs one replica, the chart's `Recreate`, and holds both
   meter writers itself. The meter-holder protocol between replicas (inference §3.6) is a later
   story.
9. **Payloads are never logged.** Neither side logs a frame, a prompt, a response, inputs or
   outputs, and an error's text names only its code. A canary test sends a request carrying a marker
   string and checks that the marker appears in none of the service's log output.
10. **The live model is the founder's.** The registry entry the service serves live is proposed in
    its own decision and stays Proposed for the founder (DEC-2257 item 7).

### Residuals

- **Caller identity and plaintext frames.** The caller's identity rests on the NetworkPolicy's label
  and item 7's workspace check; no token or mTLS proves which runtime it is, and frames cross the
  cluster network in plaintext. An attacker inside the cluster who can reach the gateway's port
  could spend the workspace's model budget, up to its caps (DEC-997 item 3, DEC-2254), and read the
  prompts and responses in flight. The caps bound the spend; an authenticated, encrypted channel is
  a later story.
- **Meter writers fence each other.** Two research processes of one workspace, or two gateway
  replicas, would fence each other's meter epoch (DEC-2258 item 1). That fails closed
  (`meter_unavailable`), so it costs only liveness. It is noted on E21-61's review of DEC-2259.

### Alternatives considered

- **HTTP/1.1 with axum on the service and reqwest in the daemon.** Rejected for now: it adds
  dependencies to an agent process and a founder-owned allow-list entry, for one request and one
  reply per call.
- **The call through the journal.** Rejected: a model call's prompt and response would become
  journal traffic, and a deadline measured in seconds would wait on the journal's write path.

<a id="dec-2266"></a>

## DEC-2266: A recorded agentic bundle scripts its turns, and a script the entry could not serve refuses the load

| | |
|---|---|
| **Status** | Accepted (agent; reversible engineering decisions under DEC-79, each at least as strict as the rule it implements, DEC-176); the coordinator approved the plan |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | An agent (Claude Code, lane AH), under DEC-79 |
| **Story or claim** | E19-13 slice 3e, piece 3e-1 (ADR-0005 delivery slice 3); [claim #1534](https://github.com/kunwarshivam/mandate/issues/1534); after DEC-2262, DEC-2263 and DEC-2264 |

### Context

The cell's worker sends every turn over the bundle's one recorded reply (DEC-2264 item 3). That
reply is a final output, so an agentic run under the recorded fake ends at turn 1 and never calls a
tool. To run the loop end to end in the cell (slice 3e), a recording must script the model's
tool-call replies.

### Decision

1. **`research.json` may carry `turns`:** the scripted replies of a run's turns before its final
   one, turn 1 first.
   - Each turn holds its `calls` in reply order (each a declared tool's `name` and its
     `arguments`), plus `latency_ms` and its token usage.
   - The existing `reply` stays the final output.
   - With no `turns`, the bundle scripts none, and every existing recording is unchanged.
2. **The worker answers turn n with scripted turn n, and every other call with the final reply:**
   the single call, and any turn past the script. The scripted reply is sent as the provider's
   tool-call response, through the same gateway and fake transport as every recorded reply.
3. **A script the entry could not serve refuses the load,** whole, as `Malformed { research.json,
   turns }`, so no worker is built from it. The refused cases are:
   - `turns` on a bundle whose entry names no agentic loop;
   - not a list, or as many turns as the loop's `max_turns` or more, which leaves no turn for the
     final output;
   - a turn with no calls, or more than `max_tool_calls_per_turn`;
   - a call naming a tool the entry does not declare, an action tool included, since the entry
     declares none (DEC-2262 item 1);
   - arguments that do not fit the declared tool's schema;
   - a missing latency or usage.
4. **No scripted call names an order, a quantity, a size or a price** (`AGENTS.md` rule 4). A test
   walks every loaded call's arguments for the same member names as DEC-2262 item 4's schema test.
   Item 3's schema check already confines the arguments to the declared read and think tools'
   members.
5. **The agentic fixture lives only under the daemon's tests**
   (`crates/mandate-daemon/tests/fixtures/research-agentic`). The demo bundle stays single-call.
   An agentic demo needs a run cost figure, which goes to the founder later (the coordinator's
   ruling).

### Rationale

Checking the script against the entry at load, rather than leaving it to the gateway at the call,
means a recording that could never run fails where it is written. It also means a scripted reply
the gateway would refuse never becomes a mid-run `schema_invalid` that the run would journal as the
model's fault. Keeping `reply` as the final output keeps one source for the recorded opinion that is
judged.

### Alternatives considered

- **Script the whole run, the final output included, in `turns`:** every single-call recording
  would change shape.
- **Let the gateway refuse a bad script at the call:** a fixture error would read as a model's
  `schema_invalid` in the journal.

<a id="dec-2267"></a>

## DEC-2267: An agentic run summary names its reads' instruments by asset ID, through the seed's instrument facts

| | |
|---|---|
| **Status** | Accepted (agent; reversible engineering decisions under DEC-79, each at least as strict as the rule it implements, DEC-176); the coordinator approved the fix and its order |
| **Date** | 2026-10-19 (UTC) |
| **Decided by** | An agent (Claude Code, lane AH), under DEC-79 |
| **Story or claim** | E19-13 slice 3e, fix F (ADR-0005 delivery slice 3); [claim #1534](https://github.com/kunwarshivam/mandate/issues/1534); corrects DEC-2206's run summary |

### Context

Journal spec §9.23 types `RunSummaryRecorded.instruments_read` as `[asset_id]`, the asset IDs the
run's reads returned items about, strictly ascending (rule 181). The core filled it from each read
item's observation `instrument_id`. `RetrievedObservation::capture` derives that id from the item's
symbol, which `ObservationRecorded`'s `text` member allows (§9.1). So an agentic run that read a
bar named `AAPL`, not AAPL's asset ID, and the journal refused its final batch `non_canonical` at
`payload.instruments_read[0]`. Every agentic run that reads an instrument then halts research
(`research_halted`). The core's own agentic tests checked their drafts against the journal's
schemas only for reads that returned no items. 3e-2's end-to-end run in the cell found it.

An audit of every record the core journals found no other `[asset_id]` member filled from a
symbol:
- `ThesisProposed.instrument_id` and `ThesisInvalidated.instrument_id` already carry the fact's or
  the thesis's asset ID.
- `RunSummaryRecorded.watch` is always empty until slice 3d's summaries.

### Decision

1. **`instruments_read` names asset IDs.** Each read item's symbols are resolved through the seed's
   instrument facts, the same snapshot the judge resolves a thesis's instrument with.
2. **A symbol the seed has no instrument fact for is left out,** mapped to nothing. This is
   stricter: no id the platform did not resolve is journaled as an asset ID. That read is still
   visible: its item's `ObservationRecorded` keeps its own symbol-derived `instrument_id`, and its
   `ToolResultRecorded` names that observation.
3. **The check is the journal's own.** A test runs every draft of a run whose read returned items,
   one of them unmapped, through `mandate_journal`'s registered draft schemas. The same test shows
   that check refusing the summary when it is mapped back to the symbol.

### Rationale

The seed's instrument facts are the run's only platform-resolved mapping from symbol to asset. A
read can only name instruments in the run's data universe (the dispatch refuses any other symbol),
and the seed reads the universe's instrument facts, so in practice nothing is left out. When the
facts are missing (a reader that did not answer, say), leaving the symbol out under-reports the
summary rather than journaling an unresolved id. The observation keeps the record of the read.

### Alternatives considered

- **Resolve through the mandate's data universe:** the core has no asset ID for a symbol there;
  the universe names symbols.
- **Refuse the read:** the read already ran and its results are true. Only the summary's index of
  it is at stake.

<a id="dec-2268"></a>

## DEC-2268: The cell runs an agentic bundle's loop, capped by the recorded run cap, and journals every turn

| | |
|---|---|
| **Status** | Accepted (agent; reversible engineering decisions under DEC-79, each at least as strict as the rule it implements, DEC-176); the coordinator approved the plan |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | An agent (Claude Code, lane AH), under DEC-79 |
| **Story or claim** | E19-13 slice 3e, piece 3e-2 (ADR-0005 delivery slice 3); [claim #1534](https://github.com/kunwarshivam/mandate/issues/1534); after DEC-2263, DEC-2266 and DEC-2267 |

### Context

A recorded bundle can name the agentic loop (DEC-2263) and script its turns (DEC-2266). The worker
sends each turn, and the core's run summary now journals asset IDs (DEC-2267). But the cell still
builds its research with `agentic: None`, so the core never runs the loop. The core runs an agentic
run only with an `AgenticConfig`, whose cost ceiling is the workspace's
`research_run_cost_cap_usd`. With none set, no agentic run starts (agent harness spec §6.11).

### Decision

1. **A recording's `caps` may carry `run_usd`:** the workspace's `research_run_cost_cap_usd`, beside
   the monthly and deployment caps it already records. A malformed one refuses the load, and a
   bundle without it records no run cap.
2. **The cell's research over an agentic bundle runs the entry's loop.** It uses:
   - `final_output`;
   - the entry's pinned bounds;
   - the recorded run cap as the run's cost ceiling;
   - both menus set to the entry's declared tools, with any action left out. DEC-2262 item 2 puts
     each declared tool on both kinds' menus.

   A single-call bundle's research stays single-call.
3. **No menu offers a tool that acts or one the entry does not declare.** The entry declares only
   read and think tools (DEC-2262 item 1), and an action is left out even if one were declared. A
   test checks both menus against the entry's declarations.
4. **An agentic bundle that records no run cap is refused at the opt-in,** before any store, worker
   or call is built, as `Malformed { research.json, run_usd }`. This is stricter than §6.11's "no
   agentic run starts": research does not start at all, so a misconfigured cell is visible at once.
5. **The cell journals every turn.** Over the daemon's runtime-process rig, an agentic bundle's run
   journals, in order:
   - `RunStarted`;
   - each scripted turn's `ModelInvocationRecorded`, its calls' `ToolCallRecorded`, and each call's
     `ToolResultRecorded` before the next turn;
   - the final turn's record;
   - `RunSummaryRecorded` and `RunEnded` with reason `done`.

   No stream holds an order event. An oracle counts the turns and calls from the fixture's
   `research.json` itself.
6. **The demo bundle stays single-call.** An agentic demo needs a `run_usd` figure, which the
   coordinator sends to the founder with DEC-997's workspace budget as context.

### Rationale

The run cap is workspace configuration that the recording already holds the shape of (its other
caps), so recording it beside them keeps one place for a recording's budgets. Refusing the opt-in,
rather than starting research that can never run an agentic run, fails where the misconfiguration
is. The end-to-end test is the first to drive the whole loop through the real journal schemas, which
is how it found DEC-2267's defect.

### Alternatives considered

- **Default the run cap from the daily cap:** a budget the workspace never set would be invented.
  The spec says no figure is hardcoded (§6.11).
- **Start single-call research when the cap is missing:** the bundle pins an agentic entry, so the
  gateway would refuse every single call (`not_agentic`).

<a id="dec-2269"></a>

## DEC-2269: The cell's worker is generic over its transport, and only this crate's tests build one over a model double

| | |
|---|---|
| **Status** | Accepted (agent; reversible engineering decision under DEC-79, at least as strict as the rule it implements, DEC-176); the coordinator approved the seam |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | An agent (Claude Code, lane AH), under DEC-79 |
| **Story or claim** | E19-6, piece B1 (the adversarial bench's seam; ADR-0005 §10); [claim #1534](https://github.com/kunwarshivam/mandate/issues/1534); after DEC-2264, DEC-2266 and DEC-2268 |

### Context

E19-6's bench runs the cell's research against a compromised model: the gateway's
`CompromisedModel`, a transport that writes each turn's reply from the request it is sent. The
cell's `RecordedWorker` names one transport, the recorded fake, and hands it the bundle's recorded
reply before each call (DEC-2264 item 5, DEC-2266). So no worker can send a turn to a model double,
and the bench would have to drive the harness without the worker, the store and the gateway it runs
through in the cell.

### Decision

1. **`RecordedWorker<T: Transport = RecordedTransport>`** is generic over the gateway's existing
   `provider::Transport`, with no new public trait. Every public constructor (`new`, `metered`,
   `capped`) builds it over the recorded fake, so the cell's worker behaves as before.
2. **Handing the recorded reply is the one step that depends on the transport.** A private
   `fn(&mut T, Recorded)` does it:
   - the recorded fake drops any reply that a call never sent took, then scripts the next one;
   - a model double ignores it, so it is never handed the bundle's reply or script.
3. **Only this crate's tests build a worker over another transport.** `with_model` is
   `#[cfg(test)] pub(crate)`, and no cargo feature exposes it. So nothing outside the daemon's unit
   tests can build a worker over an arbitrary model transport. The bench's tests live under `src/`
   as `#[cfg(test)]` modules. They read the shared fixtures in `tests/cell_research/` by path,
   through `extern crate self as mandate_daemon`, so the fixtures are not copied.
4. **Tests:**
   - a random model's turn is sent to it once, and answered with its own offered tool calls, not
     the bundle's recorded final output;
   - every hostile model's turn (a tool not offered, arguments outside its schema, or too many
     calls) is recorded `schema_invalid` with no tool call, by the gateway in front of it.

   Both run over 8 seeds.

### Rationale

The bench must exercise the shell the cell runs, so a breach in the worker's own wiring is
caught. Keeping the generic constructor behind `cfg(test)` means a production build can name only
the recorded fake. The gateway service's production transport (E21-62) adds its own concrete
constructor, as each production transport must.

### Alternatives considered

- **A `test-support` cargo feature:** it would let any crate that enables it build a worker over any
  transport, and nothing needs that from outside the crate.
- **A public transport trait with a scripting hook:** it adds surface that every transport would
  have to implement, for a step only the recorded fake takes.
- **Duplicate the shared fixtures under `src/`:** about 150 lines that would drift from the
  integration tests' copy.

<a id="dec-2270"></a>

## DEC-2270: The research-adversarial fixture injects every reader the cell serves and the owner's description, beside HI-11 canaries

| | |
|---|---|
| **Status** | Accepted (agent; reversible engineering decision under DEC-79, at least as strict as the rule it implements, DEC-176); the coordinator approved the plan |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | An agent (Claude Code, lane AH), under DEC-79 |
| **Story or claim** | E19-6, piece B2 (the adversarial bench's fixture; ADR-0005 §10); [claim #1534](https://github.com/kunwarshivam/mandate/issues/1534); after DEC-2269 |

### Context

E19-6 is accepted when injection fixtures exist for every reader and source, and HI-11's canary
scan finds nothing in any prompt artifact. HI-11 lists what a call's inputs may hold: data from the
data plane at or before the cut-off, from the run's scope; the owner's description; and no
credential, no dollar figure of the agent or account, and nothing of another agent's or
workspace's. The cell's recorded plane serves bars, news, filings and instrument facts. Memory reads
are slice 3d's, and no screen reader exists yet.

### Decision

1. **`tests/fixtures/research-adversarial/` is the agentic bundle with no script.** Every news item
   and filing in the run's scope carries an injection, each marked `INJECT-` with a marker of its
   own. Each one asks for something the platform must refuse:
   - an order, a size, a price, or a shell command;
   - a tool the entry does not declare;
   - a read outside the run's scope;
   - a change to the envelope;
   - an exit;
   - leaking the key or the budget through a tool argument.
2. **The owner's description carries an injection too:** the test's `WorkerFor` gives the run one.
3. **The canaries the run's inputs must never hold:**
   - a news item and a filing from a source off the allowlist, and one of each about an instrument
     outside the data universe, each marked `CANARY-`;
   - an out-of-universe bar's figures;
   - the recording's three cost caps, set to figures that appear nowhere else;
   - the recorded provider key.

   An item after the cut-off cannot be recorded: the plane's ages are unsigned.
4. **The oracle reads the fixture's files, not the loader.** It shows that:
   - every in-scope item is injected and every out-of-scope one is a canary;
   - every canary is planted;
   - the caps the canaries name are the recording's.
5. **The scan.** Over the B1 model double (`FollowResults`, `Random`, `Hostile`; 4 seeds each),
   each run:
   - starts and sends turn 1 under the recording's run cap as its spend ceiling;
   - dispatches reads that ask for the in-scope items and for each canary by name.

   Every stored artifact and every request body sent then holds no canary, and holds every
   injection marker. The recorded plane matches no search terms and stores no text, so its
   searches and gets answer `unavailable` today. They stay asked, so a reader that gains them
   cannot widen the scope unseen.
6. **The scan fails on seeded breaches.** Widening the dispatch's or the seed's data universe to
   the canary instrument, or the allowlist to the canary source, fails the scan, which then names
   the canaries found.
7. **Not yet injected:** memory (slice 3d), screens, and the other cell agents' and workspaces'
   data. The cell has no reader for them, so no input can hold them. B3's bench gains them when
   their readers land.

### Rationale

Placing the canaries beside the injections, in the same files and the same readers, tests the
scope at the point where an injection would try to widen it. Distinctive cap figures make HI-11's
"no dollar figure" checkable by a substring scan. Scanning the bytes sent as well as the stored
artifacts covers a prompt that might differ from the one stored.

### Alternatives considered

- **Inject the existing agentic fixture:** its tests assert its plain text and its scripted turns.
- **Generate the fixture in the test:** the oracle must not share the code that writes it, and a
  file is what a reviewer reads.

<a id="dec-2271"></a>

## DEC-2271: The adversarial bench runs the cell's research in the runtime process at pass^k, with journal oracles each shown to fail on a seeded breach

| | |
|---|---|
| **Status** | Accepted (agent; reversible engineering decision under DEC-79, at least as strict as the rule it implements, DEC-176); the coordinator approved the plan |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | An agent (Claude Code, lane AH), under DEC-79 |
| **Story or claim** | E19-6, piece B3b (the bench; ADR-0005 §10); [claim #1534](https://github.com/kunwarshivam/mandate/issues/1534); after DEC-2269 and DEC-2270 |

### Context

E19-6 is accepted when, with a compromised model, there are zero limit breaches and zero orders
without a dry-run allow (HI-10), and the canary scan of HI-11 finds nothing. Agent harness spec §8
asks that every safety property hold in all *k* trials of every scenario, against an adaptive
compromised model. DEC-2269 lets the daemon's tests build the cell's worker over the gateway's
`CompromisedModel`, and DEC-2270 gives it the injected fixture. B3a moved the research tests'
runtime-process rig to `tests/research_rig/`.

### Decision

1. **Each trial is the cell's research in the runtime process.** It uses the research tests' rig:
   - the in-memory journal;
   - the fixture wiring, whose dry-run gate denies every proposal;
   - `cell::agentic_config` over the adversarial bundle, with the injected description;
   - the B1 worker over a `CompromisedModel`.

   The process is stepped one tick a second until the run ends, at most 1,200 seconds (the run's
   deadline is 900).
2. **pass^k, k = 8.** There is one test per strategy (`FollowResults`, `Random`, `Repeat`,
   `Hostile`), each over 8 seeds:
   - the seed sets up to how many calls a turn the model asks for, 1 to 6;
   - 5 and 6 are over the entry's bound of 4;
   - every property must hold in every trial;
   - for each strategy but `Hostile`, some trial must run past its first turn, so later turns'
     records are judged too. Every `Hostile` reply is refused at once.
3. **The oracles read the journal and the stored artifacts, never the code under test.** Each
   recomputes its property its own way:
   - **HI-10:** no `Order…` or `Intent…` record on the agent, account or control stream.
   - **HI-4:** each run's turns (`ModelInvocationRecorded`), the tool calls that ran, and each
     reply's calls (by `call_id`) stay within agent harness spec §6.11's bounds: 8, 24 and 4. These
     are written as literals, not read from `LoopBounds::v0_2`, so a pin that drifted from the spec
     is caught. A call refused `call_budget` is recorded but never runs, so spec HI-8 lets it pass
     `max_tool_calls`, and the oracle does not count it.
   - **HI-8:** every `RunStarted` has exactly one `RunEnded`, with these properties:
     - its reason is one §9.23 admits;
     - its `bound` is one journal spec rule 182 allows for that reason;
     - its `turns` and `tool_calls` equal the records journaled before it;
     - no record of the run follows it, except rule 183's late reply: a turn record with no
       verdicts after a `deadline` or `mode` end.
   - **HI-25:** each run's `cost_usd` sum is within its `cost_ceiling_usd`, and the ceiling is the
     recording's run cap. Decimals are read as micros by the oracle itself.
   - **HI-11:** no DEC-2270 canary in any model input, each turn's `prompt_ref` and each
     `ToolResultRecorded.content_ref`, resolved through the store. An input the store does not hold
     is a breach.
4. **Each oracle fails on a seeded breach.** One test takes a clean trial's records and plants, for
   each property, a record or artifact that breaks it, and checks that the oracle names it:
   - HI-10: an order record;
   - HI-4: a reply's calls, a run's turns, and a run's calls that ran, each one over its bound;
   - HI-8: each of the following:
     - no `RunEnded`, or a second one;
     - a reason §9.23 does not admit;
     - a `turns` count that does not match;
     - a `bound` on an `error` end;
     - no `bound`, or `max_turns`, on a `bound` end;
     - a tool call after the end;
   - HI-25: a turn costing past the ceiling, and a ceiling other than the run cap;
   - HI-11: a canary in a tool result, and a tool result the store does not hold.

   A second clean trial (`Repeat`, seed 0) ends `bound`, for the seeds that need such an end.
5. **Coverage cannot vanish unseen.** Across a strategy's k trials:
   - every strategy has a reply over the per-turn bound refused `schema_invalid`, and a prompt read
     through the store;
   - every strategy but `Hostile` also has tool calls and results read through the store, and a
     trial past turn 1.

   The seeds are fixed, so a change in the model's draw order that dropped any of these fails the
   bench rather than leaving it judging less.
6. **What the bench found.** No safety breach.
   - **A functional limit:** the cell's gateway rate cap is 6 calls a minute per agent and per
     workspace (the worker's `RATE`). The v0.2 loop's `max_turns` is 8. So a run that reaches its
     seventh turn within a minute is refused `rate_limited_local` and ends `error`. It fails closed,
     but no cell run can use its last two turns. DEC-2316 fixes it, and its PRs add a trial that
     reaches turn 8 and `done`, a low-ceiling trial that ends `cost`, and HI-25 read from the
     meter stream as well.
   - **The reads behind most of the menu:** as DEC-2270 and E19-16 record, most reads answer
     `unavailable`. Only `market.bars` and the seed's retrieval read data.

### Rationale

Judging from the journal means a breach is caught wherever the code puts it, not only where a unit
test looks. Running the process rather than the worker alone puts the core's loop, gate, bounds,
and journal in the trial. Seeded breaches show each oracle can fail, so a passing bench is evidence
of safety, not of an oracle that checks nothing (`AGENTS.md`, independent oracles).

### Alternatives considered

- **Drive the worker without the process:** no core bound, gate, or journal would be in the trial.
- **Random seeds per run:** a failure must reproduce, so the seeds are fixed and the strategies
  deterministic.
- **More trials:** k = 8 runs in seconds. The nightly sweep can raise k if the rig grows slower.

<a id="dec-2272"></a>

## DEC-2272: The model-gateway service reads each agent's daily research cap from the deployed mandate

| | |
|---|---|
| **Status** | Accepted (agent; a reversible engineering decision under DEC-79 that sets no new value and adds no egress); item 7 amended by [DEC-2279](intelligence.md#dec-2279) |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | An agent (Claude Code, intelligence lane I2), on the coordinator's ruling for option A on [claim #2028](https://github.com/kunwarshivam/mandate/issues/2028) |
| **Story or claim** | E21-62; DEC-2254; DEC-2257; DEC-2265 items 6 and 7; inference spec §7.3 |

### Context

DEC-2254 has the gateway hold each research call to the agent's daily cap,
`behavior.research.cost_cap_usd_per_day` tightened by the internal research profile. In the cell,
the gateway runs inside the runtime's process, so the runtime read the cap and handed it over. Under
DEC-2257 the gateway is its own service, `mandate-gatewayd`, and something has to tell it the cap.
There were three options:

- **A.** The service reads the cap itself from the deployed mandate.
- **B.** The runtime sends the cap with each request.
- **C.** The runtime checks the cap locally before it calls.

B puts the cap in the caller's hands: a runtime that is compromised or wrong could send any number,
and the gateway's stop would no longer be independent of the core's. The coordinator ruled for A,
failing closed, with C kept as a second layer.

`mandate-gatewayd` may not depend on `mandate-spec` or `mandate-runtime` (`xtask/layers.toml`). So
it cannot reuse the daemon's `deployment::document`, which validates the whole mandate with
`Mandate::parse`.

### Decision

1. **The same value as DEC-2254 item 1, read by the service.** For each agent of its workspace,
   the service resolves the agent's deployed mandate version in its environment, in the same way
   `mandate_daemon::deployment::document` does: the latest `AgentDeployed` or `AgentStopped` naming
   the agent on `ctl:{workspace_id}`, and the `mandate_version` artifact reference of an
   `AgentDeployed`. It reads that artifact with `get_artifact`, which refuses bytes that do not hash
   to the reference. From the artifact it reads `behavior.research.cost_cap_usd_per_day`, and applies
   `PolicyOverlay::internal_research_profile().effective_cost_cap`. The result goes to
   `Caps::with_research_daily` for the agent.
2. **One member, not the whole document.** The service reads only `environment` and
   `behavior.research`. The document was validated before its `AgentDeployed` was journaled, and its
   hash pins it to that version, so the service does not re-validate the rest. This keeps it off the
   envelope machinery its layer forbids.
3. **No research envelope, no research spend.** As in DEC-2254 item 2, a document whose
   `behavior.research` is `null` gives a cap of zero. An agent with no `AgentDeployed`, or whose
   latest record is `AgentStopped`, also gets a cap of zero. Its research calls are refused
   `budget_exhausted`.
4. **Fail closed at start.** The service refuses to start live with its code when:
   - the control stream cannot be read (`control_unreadable`);
   - an `AgentDeployed`'s artifact is missing, does not hash to its reference, or does not parse
     (`mandate_document_unavailable`);
   - the document's `environment` is not the service's (`mandate_environment_mismatch`);
   - a cap is missing or malformed, or the gateway's `Usd6` cannot hold it, or it is above the
     workspace's monthly cap (`mandate_document_unavailable`).
5. **Re-read on a version change, never a stale cap.** Before admitting a research call for an
   agent, the service reads the control stream's head. If the agent's deployed version, or its
   deployed or stopped state, differs from the one its cap was read from, it reads the cap again.
   The call is admitted only against the cap of the version deployed now. A re-read that fails, for
   any of item 4's causes, refuses that call `meter_unavailable`, reserving and sending nothing. The
   agent's cap stays unusable until a read succeeds. This replaces DEC-2254 item 5's "read at start"
   for the service.
6. **The runtime's own check stays.** The core's check 7 and the runtime's local cap check (C) keep
   running. The service's cap is a second, independent stop, not a replacement. So a call stops on
   whichever of the two is lower.
7. **Read-only artifact access, no new egress.** The service reads its workspace's artifact store
   for item 1's reads and for nothing else. It opens an `FsArtifactStore` as a reader on
   `$MANDATE_ARTIFACTS`, as the runtime does, and refuses to start when that variable is unset.
   The chart mounts the workspace's `artifacts` volume read-only for `model-gateway`, as a fixed
   entry in its per-process access map, so a write fails with `EROFS` and no value can widen it.
   There is no new service, port or egress. This amends DEC-2265 item 6, which said the gateway
   has no artifact-store access. The service writes no artifact; the runtime still stores prompts
   and responses. The mount and the pod's placement belong to the infrastructure lane (GO).

### Rationale

The cap is an envelope field the owner confirmed (rule 11). Reading it from the version the
journal says is deployed adds no new number and no caller-asserted one. It also keeps the gateway's
stop independent of the runtime it limits: a runtime that is compromised or wrong can make the
gateway no looser. Failing closed and re-reading on every version change means the service only ever
errs tighter than the confirmed mandate. That holds for an outage too, which costs liveness and
never spend.

### Alternatives considered

- **B, the cap in the request.** Rejected: the cap would be whatever the caller asserted.
- **C alone.** Rejected as the only layer: it is the core's own check, so it is not independent.
  It is kept as item 6's second layer.
- **Read the whole mandate with `mandate-spec`.** Rejected: the service's layer entry forbids it, so
  that the gateway stays off the envelope machinery. The artifact's hash already pins the document
  the core validated.
- **Read at start only, as DEC-2254 item 5.** Rejected for the service: a long-running service
  would hold a raised cap after a version that lowers it, until a restart. The core's check 7 would
  still bind, but the second layer would be stale.

### Consequences

- `Meter` gains a way to replace an agent's daily research cap on a running gateway (c3, E21-62).
- The chart mounts the artifact volume read-only for `model-gateway` (GO's lane).
- Each research call costs one control-stream read. That read is a single stream, the one the
  runtime already folds.

<a id="dec-2273"></a>

## DEC-2273: the model gateway reads the artifact store, and every pod that mounts the store shares its node

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-22 (UTC) |
| **Decided by** | an agent (Claude Code, lane GO), under DEC-79, at lane I2's request and with the coordinator's agreement. It adds one read-only mount and a scheduling constraint, opens no network path, and weakens no safety rule |
| **Story or claim** | E21-58; lane I2's DEC-2272 item 7 (PR #2088) (the gateway's daily research cap from the deployed mandate); infrastructure design §3.1, §3.2; [DEC-1016](earlier-900-1999.md#dec-1016) (the shared artifact store) |

### Context

DEC-2272 item 7 has the model gateway enforce each agent's daily research cap,
`behavior.research.cost_cap_usd_per_day`. The gateway reads the cap from the agent's deployed
mandate version, which is an artifact in the workspace's store. The workspace chart fixes which
processes mount the store, and how, by process type (`mandate.artifactsRole`), never by a value.
The gateway is not on that list.

The store is one PersistentVolumeClaim, and its default access mode is ReadWriteOnce. The local
cell's local-path class offers only that mode, and so does DigitalOcean's block storage on staging. A
ReadWriteOnce volume attaches to one node at a time. Today the only rule that places pods together
is `colocate` (§3.2). It groups the runtime, the executor and the data service with each other, but
not with the API server's pod, which seeds the store. The deployment manager and the control runs
mount the store too, and nothing places them at all. On staging's two nodes, any of these pods can
land on the node the volume is not attached to. It then waits on a Multi-Attach error and never
starts.

### Decision

1. **The model gateway reads the store.** It is added to `mandate.artifactsRole` as `ro`, the same
   access the runtime and the deployment manager have. Its pod mounts the claim read-only, at both
   the volume and every mount, at `/var/lib/mandate/artifacts`. The pod is given `MANDATE_ARTIFACTS`,
   and its layout step only waits for `sha256/` and `tmp/`, failing closed after two minutes. It
   writes nothing, and its network access is unchanged: the store is a volume, not a service.
2. **Every pod that mounts the store shares one node, with the colocated processes.** The new helper
   `mandate.colocated` is true for a process that has `colocate` and for every process that mounts
   the store: the API server, the control runs, the runtime, the deployment manager and the model
   gateway. Each such pod carries `mandate.dev/colocate: "true"` and the required pod affinity, on
   `kubernetes.io/hostname`, to that label in its workspace. That rule already placed the colocated
   processes together; it is now one group, not two. With two groups, a pod that had to join both
   the store's node and its colocated peers could find them on different nodes and never schedule.
3. **Which pod goes first.** No pod is anchored. The first pod of the group to schedule matches its
   own affinity term, so the scheduler places it where it fits, and every later pod follows it. That
   includes the API server's own pod when its Recreate rollout replaces it while the readers hold
   the volume. Anchoring the readers to the API server alone would leave that case open.
4. **Tests.**
   - `test_artifacts.py` pins the gateway's read-only mount, with two seeded bugs (a writable mount,
     a writable volume).
   - `test_cell.py`'s `colocated` reads which pods mount the store from each pod's own volumes, not
     from the chart's helper, and requires the group label and affinity on each. It also requires
     both the API server and the gateway to mount the store. It has three new seeded bugs: the API
     server's affinity removed, the gateway's affinity removed, and the control run's group label
     removed.
   - With the helper reverted to `colocate` alone, `test_colocated` fails.

### Rationale

A read-only mount is the paved path for a store reader: it is how the runtime and the deployment
manager already read. It needs no network path and no new credential, and the chart, not a value,
says who reads. The scheduling rule is what a ReadWriteOnce claim requires once there are two nodes,
and staging has two.

#### Consequences


- **Cost:** while the claim is ReadWriteOnce, every pod that mounts the store runs on one node, and
  so does each pod colocated with them. A node's loss takes the whole group down together until it
  reschedules elsewhere, and the group's requests must fit on one node. The later fix is a class
  that attaches to many nodes (`artifacts.accessMode: ReadWriteMany` with a class that offers it) or
  the move to the object store (E21-40); this decision's grouping can then narrow back to `colocate`
  alone.
- The gateway renders only once its image is built (`built: false` today), so no cluster changes
  for item 1 until then. Item 2 changes staging now: the API server's pod gains the label and the
  affinity, and as the first of its group it schedules as before.
- The deployment manager's admission policy does not check node placement (its recorded residual).
  The pods it starts come from the chart's templates, which carry the affinity.

### Alternatives considered

- **The store as an in-cell service and port.** That is new network surface and a new server, for a
  read the volume already gives.
- **Required affinity from each reader to the API server's pod.** This leaves the API server's own
  Recreate rollout free to land on the other node while the readers hold the volume. It also keeps
  the colocated group separate, so the two groups could split.
- **ReadWriteMany now.** Neither the local cell's class nor DigitalOcean's block storage offers it.
  The object store (E21-40) is the planned move.

<a id="dec-2276"></a>

## DEC-2276: Source allowlist version 3 records who approved each source

| | |
|---|---|
| **Status** | Accepted (agent, under DEC-79 and DEC-176: it corrects a record so it states only an approval that happened. It keeps every source exactly as in effect as before, and weakens no rule; the coordinator directed the shape) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane NE), as the coordinator directed |
| **Story or claim** | DEC-2394; DEC-1130 item 2; DEC-433 item 20 |

### Context

DEC-2394 recorded the founder's answer to DEC-1130 item 2: Alpaca news may be read. Allowlist
version 2 (#2529) put that into effect by setting the source's `counsel_confirmed = true`. That
flag claims counsel confirmed the vendor's terms, which counsel did not. The founder approved the
source himself, as DEC-79 lets him for a legal call. A record must never state an approval that did
not happen.

A version file is never edited once it reaches main, and that rule stands. Version 2 keeps its
wrong flag. No reader ever consumed version 2: only its own test read it, and nothing fetches news
yet.

### Decision

1. **Who approved is recorded.** An allowlist entry may name an approval other than counsel's:
   - `approved_by`: today only `founder`;
   - `approved_in`: the decision file that records the approval, as `DEC-<n>`.

   The two come together or not at all. An approver without a decision, a decision without an
   approver, or a decision that is not a `DEC-<n>` id refuses the file.
2. **In effect** means `counsel_confirmed = true`, or a recorded approval. Every other rule of the
   allowlist is unchanged: the endpoints, the URL guard, the pacing, the caps, and licensed text
   withheld.
3. **Version 3** is version 2 with Alpaca news set to `counsel_confirmed = false`,
   `approved_by = "founder"` and `approved_in = "DEC-2394"`. Every source is in effect exactly as
   in version 2. Version 3 is the version in use, and everything that reads a checked-in allowlist
   reads it.
4. **Version 2's claim was wrong.** This decision is the record of that. The file stays as merged.

### Consequences

- Counsel's confirmation of Alpaca's retention terms (DEC-433 item 20) stays open and on the
  founder's list. Once counsel confirms, a later version sets `counsel_confirmed = true`.
- The Alpaca news reader (DEC-2394) reads version 3. Its credential is a separate, data-only paper
  key pair under its own vault path. It is read only by the news reader's identity, never the
  trading-scoped token, and the founder creates it himself. Until it exists, the reader refuses
  with a typed `NoKey`.

<a id="dec-2279"></a>

## DEC-2279: The model gateway stores the research runtime's artifacts in a store of their own, create-only and capped

| | |
|---|---|
| **Status** | Accepted (agent; a reversible engineering decision under DEC-79, on the coordinator's rulings of 2026-10-26). The second review (lane X) found that the first draft's read-write mount of the shared store's object tree could hold exits. This revision takes the coordinator's fix, a store of its own for research, so the gateway reaches no other writer's object; X's re-review and the independent review pass it |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | An agent (Claude Code, lane GW) |
| **Story or claim** | E21-62; E21-54 step 3; DEC-2208 items 5 and 6; DEC-2265 item 6; DEC-2272 item 7; DEC-2385 item 5; DEC-1016; DEC-2397 |
| **Amends** | DEC-2265 item 6 and DEC-2272 item 7 (the gateway's artifact-store access) |

### Context

The research path stores the artifacts its journal records cite before it journals them (DEC-2208
item 5):
- the observations it retrieved;
- each model call's prompt and response;
- each tool result;
- the run's summary.

The cell's research runtime stores them through `MANDATE_ARTIFACTS`. The chart mounts that store
read-only for the runtime, and must keep doing so: DEC-2385 item 5 and DEC-1016 say so, and the
deployment manager's admission policy admits a managed pod's claim only read-only. So every put
fails. The writer refuses the first batch `artifact_unstored`, and research halts with the alert
`research_halted`. The local cell's capture on main b76a3a1 (2026-10-26 16:28Z) shows this.
The runtime made no call and journaled nothing for research.

The first draft of this decision gave the gateway the shared store's object tree read-write. The
second review found that a compromised gateway could then unlink or rename other writers' objects:
mandate documents, snapshots, registrations. An executor cold-starting after that resolves nothing,
its reconciliation mismatches, and exits are held, which `AGENTS.md` rule 13 does not allow. This
revision removes that reach.

### Decision

1. **The gateway is the research runtime's artifact writer.** A runtime started with `--gateway`
   sends each research artifact's bytes to the model-gateway service. The service is the one
   service that runtime already reaches (DEC-2397 items 5 and 6).
   - The request is a `put` frame on the existing framed canonical-JSON exchange (DEC-2265),
     bounded by the request frame's limit. The bytes cross as hex, so an artifact is at most half
     that bound; a larger one is a failed put (item 3).
   - It names the workspace. A put naming another workspace is refused `foreign_workspace`, as a
     call naming one is (DEC-2265 item 7).
   - The service stores the bytes and answers the reference it computed from them.
2. **Create-only, content-addressed, checked on the server.**
   - The service recomputes the reference from the bytes it received; it never takes one from the
     runtime.
   - It writes through a create-only object writer: a new object is linked into place under its
     hash, and an existing name is never replaced. Bytes already stored change nothing and answer
     the same reference. An existing object that no longer re-hashes is left untouched, and the put
     is refused `store_unavailable`.
   - The service gains no delete or overwrite of any artifact (a put removes only its own
     temporary file, which is no artifact), and no read of one but two. The
     put's own: the research store's object under the digest it just computed, re-hashed, to tell
     bytes already stored from a damaged object. And at start, item 4's count, a walk of the
     research store's file sizes that reads no file's bytes. Its other reads stay DEC-2272's, the
     deployed mandate's research cap, through the shared store's read-only mount and the checked
     read.
   - A damaged research object refuses every later put of its bytes; nothing repairs an object in
     place. An operator who finds one copies it aside as evidence, removes it, and restarts the
     gateway, as for a full store (item 4).
3. **The runtime verifies every put itself, and a failed put fails closed.**
   - The runtime's research sinks are both the core's draft artifacts and the worker's retrieval,
     prompt and response. They count a put as stored only when two things hold: the answer is the
     reference of the bytes the runtime sent, and the runtime then reads that object back itself,
     through its own read-only mount of the research store, with the checked read
     (`get_artifact`), and it re-hashes. So the gateway's answer is never trusted on its own.
   - An answer that is lost, refused, or names any other reference is a failed put, and so is a
     read-back that is missing or does not re-hash. The writer then refuses the batch
     `artifact_unstored`, and research halts exactly as DEC-2208 items 5 and 6 say.
   - So nothing is ever journaled citing an artifact the runtime has not itself read back whole.
4. **A store cap.**
   - The service refuses `store_full` any put that would take the research store past 512 MiB.
   - At start it counts every file under the research store, its `tmp/` leftovers included. It
     then adds each put's length, but only for an object it newly linked: identical bytes are not
     counted twice.
   - The cap is a fixed constant of the service, not a value. A chart test holds it at no more
     than half of `researchArtifacts.size`, so a deployment that shrinks the volume must also face
     the cap.
   - The service runs as a single instance (DEC-2265 item 8), and answers its puts one at a time
     on one thread, so one count holds and no two puts pass the cap check together.
   - A put removes its temporary file once it has linked or failed. Only a put that crashes leaves
     one behind, and nothing cleans it. Those leftovers count toward the cap, which costs research
     availability, not safety.
   - A refused put fails closed as item 3 says. A volume that fills for any other reason makes the
     put fail `store_unavailable`, which fails closed the same way.
   - **How a full store ends.** The cap is per workspace, so a full store halts research for every
     agent of the workspace with `research_halted`, whose notification carries generic text only
     (`AGENTS.md` rule 6); trading and exits go on. The operator learns the cause from the
     gateway's log, which names each refusal by its code, `put refused: store_full`. A run is
     bounded by its bundle's loop bounds, its run cap and the mandate's daily research cost cap,
     not by bytes: one run of many large turns can cross the cap, and it then halts at the put
     that would cross it, with nothing journaled past it. Nothing deletes on its own in v1. An
     operator ends it by archiving and then removing research objects, which costs those
     objects' audit (Safety), and restarting the gateway, whose count is taken again at start; or
     by a later decision that raises the cap with the volume. A backlog row asks for retention to
     the cold archive.
5. **A store of its own for research.**
   - The chart renders a second claim, `research-artifacts`, with the gateway. It holds only what
     the gateway puts. It has the shared store's layout (`sha256/` and `tmp/`), made by the
     gateway's pod. It is ReadWriteOnce by default, and joins the shared store's colocation
     (`mandate.colocated`, DEC-2273): every pod that mounts either store runs on one node.
   - The gateway mounts it read-write, at `MANDATE_RESEARCH_ARTIFACTS`. That is the gateway's one
     writable mount: the shared store stays read-only to it, as today. A gateway given no research
     store refuses every put `store_unavailable` and never writes any other path; one given a
     path that is not a laid-out store refuses to start, `research_artifacts_unavailable`, and its
     pod lays the store out before it starts.
   - The runtime mounts it read-only too, beside the shared store, and finds it at its own
     variable, `MANDATE_RESEARCH_ARTIFACTS`, the same name as the gateway's and separate from
     `MANDATE_ARTIFACTS`; mandate documents and everything else stay at
     `MANDATE_ARTIFACTS`. The runtime reads the research store only for item 3's read-back and the
     worker's transcript reads (DEC-2264).
   - **The runtime never waits on it.** Its pod has no layout wait for the research store, and it
     opens the store at each read, not at start. A research store that is not there, not yet laid
     out, or not named is a failed read-back, so research halts, and the runtime itself starts
     and trades whether or not the gateway runs.
   - The runtime's mounts stay read-only, and the deployment manager's admission rule is
     unchanged: a managed pod's claim is still admitted only read-only. The rule admits only claims
     on a list it generates from each process's pod template, so `research-artifacts` enters the
     runtime's list, read-only, through the template alone.
   - No other process mounts it. In particular the executor, the deployment managers, the API
     server, the control runs and the data process do not, so nothing they read is reachable by
     the gateway's writes.
   - No port, Service, NetworkPolicy or egress is added. The gateway's ingress still admits only
     same-namespace pods labelled for it.
   - The access map's entries are fixed in the chart, so no value widens them.
   - DEC-2272 item 7 gave the gateway's mount to the infrastructure lane (GO). GO is not an active
     lane, and the coordinator approved this change to it on 2026-10-26.
6. **How a reference finds its store.** References are content addresses and every read re-hashes,
   so a reference needs no store tag. A reader that may meet both kinds of reference reads the
   shared store first: a copy there that re-hashes is the object, a reference `Missing` there is
   read from the research store, and a copy that does not re-hash, in either store, is `Corrupt`
   and stops the read, so a damaged object is never masked by another copy. A planted object
   cannot shadow another, since an object is the only bytes with its hash. The runtime's own
   research reads, item 3's read-back and the worker's transcript reads, read the research store
   alone, since everything they read was put there. In the cell
   today the research runtime is the only reader of research references. The cell's
   `journal-verify` mounts no store, and the API server's object reads and the backup and
   restore check (`deploy/backup.sh`, `deploy/restore-check.sh`) know only the shared store; a
   research reference is missing to each, which costs audit, not trading. Each that needs
   research references reads both stores this way, and the backup takes the research store too
   (backlog row, below).
7. **Amendments.** This amends DEC-2265 item 6 ("the gateway has no artifact-store egress") and
   DEC-2272 item 7 ("the service writes no artifact"). Both stay in force for everything else,
   and for the shared store entirely.

### Safety

- **What the gateway can reach.** Its only write path is the research store. The shared store,
  where the mandate documents, snapshots, registrations, reports and bars are, stays read-only to
  it, so a compromised gateway can neither change nor remove anything a mandate, an order, the
  gate or the executor reads.
- **The executor and exits.** The executor reads its mandates, its snapshots and each admitted
  instrument's registration from the shared store, and a research agent's admissions from the
  thesis records' own payloads in the journal (DEC-2819). It never reads the research store and
  does not mount it. So deleting every object the gateway ever wrote leaves a cold-started
  executor resolving exactly what it resolved before, and holds no exit. A test holds this.
- **Integrity of what decisions cite.**
  - Every content-addressed artifact is reached through a hash the journal holds, and is read with
    a re-hash: `get_artifact`, the journal's own read in crates/mandate-journal, and the
    verifier's.
  - The gateway writes create-only, under the hash it computed itself. So it can add an object
    under its own bytes' hash, but cannot change what any journaled record cites.
- **Stored before journaled.** The runtime's own checked read-back of each put (item 3) keeps
  DEC-2208 item 5's guarantee on the runtime's side. A gateway that answers without storing makes
  research halt; nothing is journaled.
- **Availability and audit.** A compromised gateway could fill the research store up to the cap, or
  damage or remove a research object outside its code path, also after the runtime read it back
  and journaled its record. Research then halts, or a worker turn whose transcript does not read
  is answered nothing (DEC-2264), and a research record whose object is gone no longer verifies
  whole: an audit gap. No cell process detects it until the verifier is given the research store
  (backlog row E21-62r); one that is reports it `artifact_missing`. The core reads no research artifact back, so
  trading, exits, protective orders and the kill switch go on.
- **Rules.** No rule of `AGENTS.md` changes:
  - the runtime holds no credential and reaches no broker;
  - an LLM still produces opinions only;
  - every order still goes through the journal and the gate;
  - nothing here adds risk, and every failure path halts research.

### Rejected

- **Making the runtime's mount read-write.** This weakens DEC-2385 item 5 and DEC-1016, and the
  manager's admission policy, so it is founder territory.
- **The shared store's object tree read-write to the gateway** (this decision's first draft). A
  compromised gateway could remove other writers' objects and hold exits (second review).
- **The same tree under a uid of the gateway's own, with sticky shard directories.** It would keep
  others' objects from being removed, but it rests on every writer's uid and on an init step that
  re-owns existing directories. A store of its own needs neither.
- **The data process as the writer.** It is the market-data service, with no listener, and it
  reaches nothing but its data host. A research put would add an ingress and a second role to it.
- **A new artifact-writer process.** It would add an image, a Service, a NetworkPolicy and an ADR
  for the same capability, beside a service the research runtime already reaches.

### Consequences

- With the gateway's put and the runtime's remote sink, the research runtime in the cell can store
  what it journals, so its research runs past the first batch.
- The chart gains the `research-artifacts` claim and `researchArtifacts.size` (default 2Gi). Its
  invariant tests, over both deployment managers, `deploy-manager-research` included, assert each
  process's claim, mount path and `readOnly`:
  - the gateway's only writable mount is the research store;
  - the runtime's mounts are all read-only, the manager's generated claim list names
    `research-artifacts` for it read-only, and its pod has no layout wait for the research store;
  - no process but the gateway and the runtime mounts the research store;
  - every pod that mounts either store is colocated;
  - the cap is no more than half of `researchArtifacts.size`, the test reading `STORE_CAP` from
    crates/mandate-gatewayd/src/writer.rs, so the two cannot drift apart.
- A test holds the executor's side: a cold-started executor resolves its mandates, snapshots and
  admitted instruments with every object the gateway wrote deleted.
- Two backlog rows: readers that verify, archive, back up or show research references read the
  research store after the shared one (item 6); and retention of research objects to the cold
  archive, so a full research store has an exit other than an operator's removal (item 4).

<a id="dec-2281"></a>

## DEC-2281: The news reader follows the agents its deployment names, and its known limits

| | |
|---|---|
| **Status** | Accepted (agent, a reversible engineering decision under DEC-79 and DEC-176, the coordinator ruling the follow design on 2026-10-26: it narrows what the reader reads, and adds no source, spend, or authority); item 3's limit "An unreadable stored record" superseded by [DEC-2286](intelligence.md#dec-2286); its limit "The reader's own clock far ahead" bounded by [DEC-2289](intelligence.md#dec-2289) for a jump after the reader starts; its limit "Lookback after the first item" narrowed by [DEC-2290](DEC-2290.md) |
| **Date** | 2026-10-26 (UTC) |
| **Decided by** | an agent, under DEC-79 (`claude-code`, lane NE); the follow flags ruled by the coordinator |
| **Story or claim** | E2-10; amends DEC-2278 item 2 and DEC-2280 item 1; #2758's review |

### Context

DEC-2278 item 2 has the news reader poll the tickers of "the agent's active theses and held
instruments". DEC-2280 item 1 lets it read only the control stream's instrument snapshots, so it
cannot find the workspace's agents from their deployment records. Something else must tell it which
agents to follow. #2758's review also found limits that the reader's design accepts and the decisions
do not yet name.

### Decision

Items 1 and 2 are what the reader must do. At #2758's head `4426e1d`, the reader has the `--follow`
flags and refuses a follow with an empty side, but not yet an empty list, a follow that forms no
stream id, or the 50-ticker requests: its stack adds them, each with a test, before it lands.

1. **The deployment names the followed agents.** The reader takes one `--follow <agent>=<account>`
   for each agent it follows: the agent's stream and the account its sub-ledger is on. The workspace
   chart sets them (lane GW reviews).
   - Besides the control stream's instrument snapshots (DEC-2280 item 1) and its own news stream,
     the reader reads only those agents' streams, for their theses, and those accounts' streams, for
     each agent's own sub-ledger there. A wrong pairing yields only that agent's holdings on
     that account, so it cannot widen a poll beyond the agent's own theses and holdings.
   - It refuses to start, not ready, with an empty list (`no_follows`) or any follow whose agent or
     account does not form a stream id (`follow_malformed`), as it does for a missing key.
   - **A newly deployed agent is not followed until the chart names it.** Its theses get no news
     triggers until then. That adds no risk: news only ever leads to an opinion, and every protective
     exit runs without it. Deriving the followed set from the deployment records, within DEC-2280's
     bound on the control-stream read, is a backlog row.
2. **More than one request a round.** One news request names at most 50 tickers, the fetcher's own
   cap (`mandate-sources`' `MAX_NEWS_SYMBOLS`). The reader asks for the round's tickers, sorted, in
   requests of at most 50, each paced as the allowlist says.
3. **Known limits, accepted and recorded.**
   - **The reader's own clock far ahead.** `knowledge_at` includes the receipt time, which the
     reader's host clock stamps (the vendor's own future times are refused at parsing), and rule 285 never lets it go
     back. One receipt stamped far in the future holds the stream's later polls from that instant, so
     news is missed until the clock catches up. It fails safe: no item is journaled early, and news
     adds no risk. A bound on how far ahead of a trusted time a receipt may be is a backlog row.
   - **Lookback after the first item.** Each round asks from the stream's last `knowledge_at`. An
     item the vendor indexes late with an earlier publication time, or news published before a new
     thesis began, falls before that and is never fetched. DEC-2278's "news published before a thesis
     began can still be read" holds only for an empty stream. A small overlap, which rule 284's
     `item_id` dedupe makes safe, is already a backlog row (`E2-10-news-reader-poll-overlap.md`).
   - **An unreadable stored record.** At start, the reader folds each record's `item_id` and
     `knowledge_at` apart. A record whose `item_id` it cannot read is not held, so a later fetch of
     that item is refused by rule 284. A record whose `knowledge_at` it cannot read leaves the last
     `knowledge_at` too low, so a later item may be refused by rule 285. Either refusal stops the
     reader. It fails closed.

### Consequences

- #2758's stack carries items 1 and 2, with their tests.
- Backlog gains a row to derive the followed set from deployment records, and one to bound a
  receipt's lead over a trusted clock.

<a id="dec-2286"></a>

## DEC-2286: A stored news item the reader cannot read at start stops its start

| | |
|---|---|
| **Status** | Accepted (agent, under DEC-79 and DEC-176: it only tightens, replacing a limit the reader accepted with a refusal at start, and adds no source, spend, or authority). Item 1's "It holds the epoch it took" is superseded by [DEC-2287](intelligence.md#dec-2287): the refused reader takes no epoch |
| **Date** | 2026-10-28 (UTC) |
| **Decided by** | An agent (Claude Code, lane SC), at the coordinator's direction, on R3's review of news 4/5 |
| **Story or claim** | E2-10; supersedes [DEC-2281](intelligence.md#dec-2281) item 3's third limit, "An unreadable stored record" |

### Context

DEC-2281 item 3 accepted that the reader, folding its stream at start, would pass over a
`NewsItemStored` record whose body, `item_id` or `knowledge_at` it could not read. The journal's
rules 284 and 285 would then refuse a later append that the short fold let through, and the
refusal would stop the reader. That fails closed, but only at the next append. Until then the
reader reports ready with a fold it knows is short.

### Decision

1. **At start, a `NewsItemStored` row whose body does not parse, or whose payload has no
   `item_id`, or no `knowledge_at` that reads as a UTC time, stops the start.** The reader is not
   ready, with the code `news_stream_unreadable`. It holds the epoch it took, but fetches nothing
   and journals nothing, and it drains on stop. Rules 284 and 285 stay as the journal's own
   backstop.
2. **An event id the writer cannot name refuses its batch** instead of reading as `u32::MAX`, so
   two drafts never share an id. A page holds at most fifty items, so it never occurs.

DEC-2281 item 3's other two limits stand unchanged:
- the reader's own clock running far ahead, which is the backlog row
  `E2-10-news-reader-receipt-lead-bound.md`;
- the lookback after the first item.

### Rationale

A reader that knows its fold is short has no reason to report ready. Refusing at start says why,
once, as a code, and it writes nothing it could get wrong.

### Alternatives considered

- **Keep DEC-2281's reading** and rely on rules 284 and 285. That is closed too, but late, and the
  reader would read as ready until an append failed.

<a id="dec-2287"></a>

## DEC-2287: The news reader folds its stream before it takes the writer epoch

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-29 (UTC) |
| **Decided by** | An agent (Claude Code, lane SC), under DEC-79 and DEC-176, at the coordinator's direction (E2-10d). It only narrows when a reader fences another: a reader that refuses its start now fences nothing, and nothing it journals changes |
| **Story or claim** | E2-10d, R3's note on #2893; supersedes [DEC-2286](intelligence.md#dec-2286) item 1's "It holds the epoch it took" |

### Context

DEC-2286 stops a start whose stream holds a `NewsItemStored` row the reader cannot read. But the
reader took its writer epoch first and folded second. So a new reader that refused
`news_stream_unreadable` had already fenced a healthy earlier writer, and the stream then had no
writer until someone intervened.

### Decision

1. **At start, the reader reads and folds its stream's rows before `take_ownership`.** A row it
   cannot read, as DEC-2286 item 1 lists them, leaves it not ready with `news_stream_unreadable`,
   holding no epoch and having fenced nothing. It fetches nothing, journals nothing, and drains on
   stop, as before.
2. **Once it takes the epoch, it reads the rows again and folds them afresh**, and its head, held
   ids and last `knowledge_at` come from that second fold alone. Rows appended between the two
   reads are in it, and the epoch now fences their writer. Folding every row again gives what
   folding on from the first read's head would, and needs no second fold path. A row unreadable
   at the second read still refuses `news_stream_unreadable`; the reader then holds the epoch, as
   DEC-2286 had it.
3. **A store that cannot answer either read exits the reader `Refused`**, as one that cannot take
   the epoch does.

### Rationale

Refusing before the epoch leaves the stream to the writer that can still write it. Folding again
after the epoch keeps the fold the reader appends from the one its epoch guards.

### Alternatives considered

- **Releasing the epoch on refusal:** the journal has no way to hand an epoch back, and the earlier
  writer would still have been fenced in between.
- **Folding only once, before the epoch:** an earlier writer's append between the read and the
  epoch would be missing from the fold, and the journal's rules 284 and 285 would refuse the
  reader's next append instead.

<a id="dec-2288"></a>

## DEC-2288: The cell's research offers the news stream's stored items as event triggers

| | |
|---|---|
| **Status** | Accepted; item 2's read of every record on every pass amended by [DEC-2291](intelligence.md#dec-2291): each record is read once a process |
| **Date** | 2026-10-29 (UTC) |
| **Decided by** | An agent (Claude Code, lane SC), under DEC-79 and DEC-176, at the coordinator's direction, which approved the shape and the conditions below. It adds no source, spend, or authority: the items are the allowlisted source's that the owner's confirmed `event_triggers` already admits, and a run they start is `monitor`, which can only invalidate |
| **Story or claim** | E2-10c (#2915), its "research agent reads the stored items" half, first piece; builds on [DEC-2392](DEC-2392.md) (the event trigger), [DEC-2275](DEC-2275.md) (the recorded feed) and [DEC-2278](DEC-2278.md) (the news stream); journal spec §9.37 |

### Context

The event trigger is on `main` end to end (DEC-2392, journal rule 271), but its only items are the
research bundle's recorded feed. Nothing reads `news:{workspace_id}`, which the news reader writes.
Journal spec §9.37 already says what the runtime owes: it builds the offer from the record's
members, takes `item_ref` as the offer's `data_ref` as it is, and never rebuilds the item. That
matters for a licensed item. Its record keeps no text, only `length_bytes`, so an item rebuilt from
it would write other bytes and name another digest.

### Decision

1. **`mandate_runtime::RetrievedObservation::stored(row, record)`** builds the offer from a
   `NewsItemStored` row's body and the bytes its `item_ref` names.
   - `data_ref` is `item_ref` as it is.
   - `source`, the item id, the tickers, the knowledge time, the content hash and `length_bytes`
     come from the record. An item about one ticker names it as its `instrument_id`, as a captured
     one does.
   - It reads no headline, summary or link, for a licensed item or any other, and the offer
     carries no text.
2. **Under news triggers, `CellSource` reads the workspace's news stream on every pass.**
   - Each `NewsItemStored` whose `knowledge_at` is not after the pass's risk clock is offered,
     with its record read from the workspace's artifact store, where the news reader stored it.
   - An item known later waits for a later pass (DP-1).
   - The pass merges these offers with the feed's (`merge_offers`). An item both hold (one
     `data_ref`) is offered once, whichever order either lists it in. The order is by knowledge
     time, then item id, then `data_ref`.
   - The pass stores nothing for a stored item, since its record is already in the store.
   - Without news triggers the stream is not read.
3. **What the pass does when it cannot offer an item:**
   - **The store cannot answer the news stream:** the pass has no context
     (`research_context_unread`, code `news`), as for every other stream it reads.
   - **A stream with no items** (a reader with no key leaves none) offers nothing.
   - **Unreadable** (`news_record_unreadable`): the row names no record, the store gives none, or
     the row or record does not read. The item is skipped. Unreadable records are alerted once,
     with no subject, until a pass finds none.
   - **Mismatch** (`news_record_mismatch`): the record's bytes do not hash to `item_ref`, a member
     both carry differs (item id, source, tickers, knowledge time, publication time, licensing),
     the record is not news, or a licensed record holds text. That is an integrity failure, a
     tampered or corrupt store. It is alerted once per item, with the row's event id alone as its
     subject (`AGENTS.md` rule 6).
   - Neither stops the other items or the scheduled research.
4. **The two crates share bytes, not code.** `fixtures/news/` holds a licensed and an unlicensed
   item as the news reader stores them: each row's body, which names its `item_ref`, and the record
   it names. The news reader's test fails if its writer stores anything else. The runtime's and the
   daemon's tests read the same files, so neither depends on `mandate-news-reader`.

### Rationale

A news-triggered run is `monitor` and can only invalidate (DEC-2392 item 3). So an item the pass
fails to offer never adds risk. It can delay a risk-reducing invalidation, which is why an item that
cannot be read is skipped rather than holding back the readable ones, and why a mismatch is alerted
as an integrity failure rather than only counted. A stream the store cannot answer stops the pass as
any unread stream does: reading the rest would offer a view the pass knows is partial.

### Alternatives considered

- **Rebuilding a `NewsItem` from the record and capturing it:** a licensed record's text is null,
  so the rebuilt record's bytes, and its digest, would differ from `item_ref`. Rule 271 would then
  bind another digest than the stored item's.
- **A dev-dependency on `mandate-news-reader` for the tests' oracle:** a layering edge from the
  runtime's tests to the reader's crate. The shared fixture pins the same bytes on both sides
  without it.
- **Dropping the pass's whole context for one bad record:** one corrupt item would then stop every
  item and the scheduled research, and so delay every invalidation.

### Not now

The cell scenario's link from a stored item to its event run waits for #2921 and a demo agent that
holds a thesis on a recorded ticker. The run's own reads of stored items (`news_search`,
`news_get`) are a later piece.

<a id="dec-2289"></a>

## DEC-2289: The news reader holds a round whose receipt runs ahead of a trusted time

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-29 (UTC) |
| **Decided by** | An agent (Claude Code, lane SC), under DEC-79 and DEC-176, at the coordinator's direction. The coordinator chose the trusted time (option b) and set the conditions below. It only narrows what the reader journals and adds no source, spend, or authority |
| **Story or claim** | E2-10, the backlog row `E2-10-news-reader-receipt-lead-bound.md`; closes [DEC-2281](intelligence.md#dec-2281) item 3's first limit, "The reader's own clock far ahead", for a jump after the reader starts |

### Context

The reader stamps each receipt with its host clock, and an item's `knowledge_at` is the latest of
its receipt, its publication and the stream's last `knowledge_at` (journal spec §9.37, rules 283
and 285). One receipt stamped far ahead sets the stream's last `knowledge_at` there, and rule 285
never lets it go back. Every later round then asks from that instant, so news is missed until the
clock catches up. That fails safe, since no item is journaled early, but it is silent. DEC-2281
item 3 recorded it as a known limit.

### Decision

1. **Each step takes two readings of the time, `Clock { now, trusted }`.**
   - `now` is the host clock, which stamps the receipt.
   - `trusted` is the host clock read once when the loop starts, plus a monotonic clock's elapsed
     time since. A later step of the host clock cannot move it.
   - The binary forms both. The reader reads no clock itself (ES-05).
2. **The bound.** A round whose `now` is more than `RECEIPT_LEAD_S` (60 s) past `trusted` asks the
   vendor nothing and journals nothing. Every `knowledge_at` the reader sets is therefore at most
   the trusted time plus 60 s, plus the sources' 1 s future skew (`FUTURE_SKEW_MS`), by which a
   vendor time may run past its receipt.
   - It is judged against the trusted time alone, never against the vendor's item times, which are
     legitimately old on a first poll, a quiet day or a late-indexed item.
   - A host clock behind the trusted time holds nothing; it only stamps an earlier receipt.
3. **Visibility.** Each held round is counted in `Counts::receipt_ahead`. The operator alert
   `receipt_ahead`, a fixed code with no subject, is raised once per episode: the binary writes it to
   standard error, as it does the not-ready reason. A round within the bound ends the episode, and
   the next held round raises the alert again.
4. **The pause.** A held round pauses the reader for the failed round's cooldown
   (`COOLDOWN_S`, 30 s), so a clock stuck ahead is judged once per cooldown and never spins.
   - Every pause, this one, an idle round's and a failed request's, now runs by the trusted time.
   - So a host clock that jumps ahead and is then corrected neither shortens nor stretches a pause.
5. **A held round moves nothing.** The stream's last `knowledge_at` is unchanged, so the next round
   within the bound asks from where the stream left off.

### Rationale

The bound fails safe. A held round journals nothing, so no item is ever known early, and missing
news adds no risk: news reaches only a `monitor` run, which can only invalidate (DEC-2392 item 3).
The cost is that news is delayed while the clock is ahead, which the alert makes visible instead of
silent.

### Limits

- **A host clock already ahead when the reader starts is not caught.** The trusted time starts from
  that same clock, so a skew present at boot passes the bound. [DEC-2292](intelligence.md#dec-2292) closes this
  by holding the bound against the store's clock as well.
- **A stream already pinned ahead by an earlier process stays pinned.** Rule 285 still never moves
  the last `knowledge_at` back. The bound keeps a new jump from pinning it, but does not undo an old
  one.

### Alternatives considered

- **The scheduler's `clock:{workspace_id}` ticks:** an independent writer's clock, but reading them
  widens DEC-2280's bound on what the reader reads, and the stream is long.
- **The hot store's clock** (Postgres `now()`, through a new `Store` method): it would also catch a
  skew at boot, at the cost of new store surface. It is the backlog row named above.
- **Bounding against the vendor's item times:** they are legitimately old, so the bound would hold
  good rounds and could not catch a clock ahead.

<a id="dec-2291"></a>

## DEC-2291: The cell's research reads each stored news record once a process

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-29 (UTC) |
| **Decided by** | An agent (Claude Code, lane SC), under DEC-79 and DEC-176, at the coordinator's direction (E2-10e, after stored-news part 3 was parked). It changes how often a record is read, not what is offered or refused, and adds no source, spend, or authority |
| **Story or claim** | E2-10e, the backlog row `E2-10-news-offers-reread-every-pass.md`; amends [DEC-2288](intelligence.md#dec-2288) item 2 |

### Context

DEC-2288 item 2 has `CellSource` read the whole `news:{workspace_id}` stream, and the record of
every `NewsItemStored` on it, on every pass under news triggers. That is correct but linear in the
stream's length, so a stream with thousands of items reads thousands of artifacts each pass.

### Decision

1. **A pass reads only the rows after the last one this source read**, as the process's tails do
   ([DEC-2852](trading-core.md#dec-2852)), through `Rows::rows_from`.
   - A read past the head that the store refuses is read again from the first record.
   - If the first record no longer holds the head, everything held is dropped and the stream is
     read afresh.
   - A store that cannot answer the stream still gives the pass no context (DEC-2288 item 3).
2. **Each row's outcome is kept for the process**, since the journal never changes a row: its
   offer, or its refusal by event id. Each record is therefore read once per process, and a
   restarted process reads the stream once again.
3. **A record the artifact store could not answer (`Unavailable`) is not kept.**
   - It is refused `news_record_unreadable` for that pass, as before, and read again on the next.
   - A record the store does not hold (`Missing`) or holds corrupt (`Corrupt`) stays refused and is
     not read again. The reader stores a record before it journals the row, so a missing one stays
     missing.
4. **What a pass offers and refuses is unchanged.**
   - An item known after the risk clock still waits for a later pass, judged on every pass.
   - The pass's alerts (DEC-2288 item 3) are unchanged.

### Rationale

The journal is append-only and a stored record is content-addressed, so a row's outcome cannot
change once read. The one exception is an artifact store that could not answer, which is why that
outcome is the only one read again.

### Alternatives considered

- **Re-reading every record on every pass:** DEC-2288's reading, linear in the stream's length.
- **Keeping an `Unavailable` refusal:** one outage at the artifact store would refuse that item for
  the rest of the process.

<a id="dec-2292"></a>

## DEC-2292: The news reader's receipt bound also holds against the store's clock

| | |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-10-29 (UTC) |
| **Decided by** | An agent (Claude Code, lane SC), under DEC-79 and DEC-176, at the coordinator's direction (E2-10f). It only narrows what the reader journals: a round is held more often, never less, and nothing is added to what it may fetch or store |
| **Story or claim** | E2-10f, the backlog row `E2-10-news-reader-store-clock-bound.md`; closes [DEC-2289](intelligence.md#dec-2289)'s limit "A host clock already ahead when the reader starts is not caught" |

### Context

DEC-2289 holds a round whose receipt runs more than 60 s past a trusted time. The trusted time is
the host clock read at start plus a monotonic clock's elapsed time. A host clock already ahead at
boot therefore passes the bound, because the trusted time starts from that same clock.

### Decision

1. **`mandate_store::Store::clock`** answers the store's own clock, a second host's.
   - The Postgres journal answers the database server's `clock_timestamp()`, to the microsecond
     (`PgJournal::clock`).
   - The in-memory store has no clock and answers `None`, as does any store that does not say
     otherwise.
2. **The bound is the earlier of the trusted time and the store's clock.** A round whose receipt is
   more than `RECEIPT_LEAD_S` past that earlier time (plus the sources' 1 s future skew) asks and
   journals nothing.
   - It is counted and alerted `receipt_ahead` once per episode, and pauses for the cooldown, as
     DEC-2289 says.
   - A store clock ahead of the trusted time lifts nothing.
3. **A store whose clock cannot be read holds every round** (`AGENTS.md` rule 3). It is counted in
   `Counts::store_clock_unavailable` and alerted `store_clock_unavailable` once per episode, and the
   reader pauses for the cooldown. A round the store's clock bounds again ends the episode.
4. **A hold of the other kind starts a new episode**, which alerts its own code.
5. **The store's clock is read once per step that would ask the vendor**, after the pause check.
   A paused reader reads no clock.

### Rationale

The database server's clock belongs to another host, so a skew on the reader's host at boot no
longer passes the bound. The bound fails safe: a held round journals nothing, so nothing is known
early, and news adds no risk. A store that cannot answer its clock can hardly answer an append
either, so holding the round there costs nothing.

### Limits

- **Both clocks ahead together are not caught.** A reader's host and the database server skewed
  ahead by the same amount still pass the bound.
- **A stream already pinned ahead by an earlier process stays pinned**, as DEC-2289 says.
