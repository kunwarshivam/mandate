# Harness research loop (E19-2, E19-3)

The agent harness's research loop ([DEC-996](../../../../docs/project/decisions/DEC-996.md),
[DEC-1120](../../../../docs/project/decisions/DEC-1120.md)): a confirmed version's research agent
runs on a schedule, reads only its pinned retrieval plan, journals every item before the model reads
it, makes one call through `mandate-gateway` (over its recorded fake unless the vault holds the
OpenRouter key and the cell supplies a hosted transport), and its answer is checked, filled with
platform facts, and judged by `mandate-research`'s ordered checks, with the verdicts in the call's
own record. An admitted thesis
reaches the order path only as a `ModelOutputRecorded` that the existing builder, the gate's dry run,
and the autonomy rules judge. **No run starts in any deployment yet:** `JournalCatalog` registers
neither the invalidation verdict (E19-5) nor the agent-stream `ModelInvocationRecorded` (E15-8), so
`Preconditions` never hold outside tests (agent harness spec §6.2).

- **Spec:** `docs/specs/agent-harness.md` §3 (HI-1, HI-2, HI-5 to HI-10, HI-13, HI-15 to HI-18,
  HI-22), §5.3 (run states), §5.4 (boundaries), §6.2 (the loop and its preconditions), §6.3
  (retrieval), §6.4 (inputs), §6.5 (output and checks), §7 (budgets and degradation), §10.3;
  `docs/specs/inference.md` §3 (the gateway's call contract); `docs/specs/mandate.md`
  §8.2, §8.5; `docs/specs/journal.md` §9.1, §9.4, §9.19.
- **Code:** `mandate-research-run` (layer 5, kept off every journal crate):
  `crates/mandate-research-run/src/plan.rs` (the retrieval plan and its scope),
  `crates/mandate-research-run/src/ports.rs` (the read-only reader ports),
  `crates/mandate-research-run/src/retrieve.rs` (`retrieve` and the capture records),
  `crates/mandate-research-run/src/dispatch.rs` (`dispatch`: an agentic run's reads and thinks,
  capped and spotlighted, E19-13 slice 3b, DEC-2209),
  `crates/mandate-research-run/src/call.rs` (`ResearchCall`, its gateway request, `call`, §7.3's
  treatments), `crates/mandate-research-run/src/entry.rs` (the pinned research entry and its output
  schema), `crates/mandate-research-run/src/backend.rs` (the OpenRouter-or-recorded-fake selection),
  `crates/mandate-research-run/src/output.rs` (`parse_output`). In `mandate-runtime`:
  `crates/mandate-runtime/src/research.rs` (`handle_research`, `ResearchState`, the schedule, the
  cancellations, the call record, and a lost exchange with the model-gateway service, DEC-2397 item
  3: the run ends, the operator is alerted naming its latest record, and the pin is kept) and `crates/mandate-runtime/src/research/judge.rs` (the harness
  checks, the platform fill, admission, and the judging batch).
  In `mandate-research-driver`, the shell's research driver ([DEC-2204](../../../../docs/project/decisions/DEC-2204.md), not yet
  wired into the run path, which waits for R0): `crates/mandate-research-driver/src/trigger.rs`
  (the per-agent trigger queue) and `crates/mandate-research-driver/src/driver.rs` (the step
  machine: commit, check, fold back, then hand on; hold on an unknown append; abandon and halt on a
  refused one; halt as diverged otherwise), with `mandate_runtime::abandon_refused_research`.
- **Tests:** `crates/mandate-runtime/tests/research.rs` and `tests/agentic.rs` (among them a lost
  call or turn, DEC-2397), `crates/mandate-research-run/tests/retrieve.rs`, `crates/mandate-research-run/tests/dispatch.rs`, `crates/mandate-research-run/tests/dispatch_news.rs`, `crates/mandate-research-run/tests/dispatch_news_get.rs`, `crates/mandate-research-run/tests/dispatch_filings.rs`, `crates/mandate-research-run/tests/dispatch_filings_get.rs`, `crates/mandate-research-run/tests/dispatch_quote.rs`, `crates/mandate-research-run/tests/turn.rs`, `crates/mandate-research-run/tests/call_turn.rs`, `crates/mandate-research-run/tests/agentic_entry.rs`
  (with a property over random answers and byte budgets), `crates/mandate-research-run/tests/output.rs`
  (with a property over arbitrary bytes and an integer-thousandths oracle),
  `crates/mandate-research-run/tests/call.rs`, `crates/mandate-research-run/tests/item.rs`,
  `crates/mandate-research-run/tests/entry.rs`, `crates/mandate-research-run/tests/backend.rs`;
  `crates/mandate-runtime/tests/research.rs` (run states, boundaries, every harness check, replay),
  `crates/mandate-runtime/tests/research_e2e.rs` (recorded bars and news to an ask, the injection
  fixture, and the demo's run through either backend), and `crates/mandate-runtime/tests/research_properties.rs` (random model
  outputs, failures, late and substituted answers, and a spent budget against an independent
  oracle, which `the_oracle_catches_seeded_breaches` shows failing on seeded journals),
  `crates/mandate-runtime/tests/invalidation_properties.rs` (E19-5: random active theses,
  invalidations, and theses against an oracle from the inputs, with its own seeded breaches), and
  `crates/mandate-journal/tests/thesis_invalidation_records.rs` (§9.22's vectors). Recorded
  fixtures: `crates/mandate-runtime/tests/fixtures/research/`.
  The driver: `crates/mandate-research-driver/tests/trigger.rs`,
  `crates/mandate-research-driver/tests/driver.rs` (every journal outcome on fakes), and
  `crates/mandate-research-driver/tests/runtime.rs` (one whole run through the driver journals what
  the runtime's own loop journals).
- **Run:** `cargo nextest run -p mandate-research-run -p mandate-runtime -p mandate-research-driver`.
