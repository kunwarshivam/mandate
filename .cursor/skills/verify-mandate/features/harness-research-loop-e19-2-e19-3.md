# Harness research loop (E19-2, E19-3)

The agent harness's research loop, first slice ([DEC-996](../../../../docs/project/decisions/DEC-996.md)):
a confirmed version's research agent runs on a schedule, reads only its pinned retrieval plan,
journals every item before the model reads it, makes one gateway call, and its answer is checked,
filled with platform facts, and judged by `mandate-research`'s ordered checks. An admitted thesis
reaches the order path only as a `ModelOutputRecorded` that the existing builder, the gate's dry run,
and the autonomy rules judge. **No run starts in any deployment yet:** `JournalCatalog` registers
neither the invalidation verdict (E19-5) nor the candidates' verdict record (E15-8), so
`Preconditions` never hold outside tests (agent harness spec §6.2).

- **Spec:** `docs/specs/agent-harness.md` §3 (HI-1, HI-2, HI-5 to HI-10, HI-13, HI-15 to HI-18,
  HI-22), §5.3 (run states), §5.4 (boundaries), §6.2 (the loop and its preconditions), §6.3
  (retrieval), §6.4 (inputs), §6.5 (output and checks), §7 (budgets and degradation), §10.3;
  `docs/specs/inference.md` §3 (the call contract the gateway port follows); `docs/specs/mandate.md`
  §8.2, §8.5; `docs/specs/journal.md` §9.1, §9.4.
- **Code:** `mandate-research-run` (layer 5, kept off every journal crate):
  `crates/mandate-research-run/src/plan.rs` (the retrieval plan and its scope),
  `crates/mandate-research-run/src/ports.rs` (the read-only reader ports and the gateway port),
  `crates/mandate-research-run/src/retrieve.rs` (`retrieve` and the capture records),
  `crates/mandate-research-run/src/call.rs` (`ResearchCall`, `settle`, §7.3's treatments),
  `crates/mandate-research-run/src/output.rs` (`parse_output`). In `mandate-runtime`:
  `crates/mandate-runtime/src/research.rs` (`handle_research`, `ResearchState`, the schedule, the
  cancellations, the call record) and `crates/mandate-runtime/src/research/judge.rs` (the harness
  checks, the platform fill, admission, and the judging batch).
- **Tests:** `crates/mandate-research-run/tests/retrieve.rs`, `crates/mandate-research-run/tests/output.rs`
  (with a property over arbitrary bytes and an integer-thousandths oracle),
  `crates/mandate-research-run/tests/call.rs`, `crates/mandate-research-run/tests/item.rs`;
  `crates/mandate-runtime/tests/research.rs` (run states, boundaries, every harness check, replay),
  `crates/mandate-runtime/tests/research_e2e.rs` (recorded bars and news to an ask, and the
  injection fixture), and `crates/mandate-runtime/tests/research_properties.rs` (random model
  outputs, failures, late and substituted answers, and a spent budget against an independent
  oracle, which `the_oracle_catches_seeded_breaches` shows failing on seeded journals). Recorded
  fixtures: `crates/mandate-runtime/tests/fixtures/research/`.
- **Run:** `cargo nextest run -p mandate-research-run -p mandate-runtime`.
