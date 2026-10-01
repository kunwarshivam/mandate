# Task: E17-8 the forward-paper evaluation harness, its pure scoring half

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story. Fill every section; write "none" rather than deleting one.

## Story

- **Story:** [E17-8](../06-backlog-v1.md) — "As the founder, I want a forward paper evaluation
  harness, so that thesis quality is judged on outcomes the model cannot have seen
  ([DEC-99](04-decision-log.md#decisions))."
- **Acceptance criteria (verbatim):** "the evaluation window, metric, and pass threshold come from
  a recorded decision made before the evaluation starts; it runs on the team's internal paper
  workspaces (DEC-103), so no user's results are aggregated; every thesis is scored after its
  horizon against buy-and-hold of the eligible basket and a broad index ETF, net of the cost
  model; the report states pass or fail against the threshold and is reproducible from the
  journal."
- **This slice is the pure scoring half:** the evaluator as deterministic code — a closed thesis,
  close series, the pre-registered decision, and the modeled round-trip cost in; a scorecard out.
  The live half — running research agents on the team's internal paper workspaces over DEC-122's
  window (the later of three months of forward paper trading and 100 closed theses) — waits on
  the founder's go, exactly as E17-0's paper runs do; nothing in this crate runs it.
- **PRD / HLD / spec anchors:** mandate spec §8.6 (the horizon is when DEC-99 scores the thesis;
  a revision is scored only by the forward-paper evaluator; MI-18's revision counts), §8.2
  (`as_of`, `expires_at`); DEC-99 (the evidence loop), DEC-122 (the window, the metric, the
  threshold), DEC-103 (the thin slice), DEC-111 (the revision loop this unblocks); RAID R-27.
- **Decisions that apply (DEC-NN):** DEC-99, DEC-122, DEC-132 (the crate's readings), DEC-120
  (the cost cap — the cost model's fees are the same §6.4 schedule the backtest charges).

## Scope

- **Reference cases that must move from pending to passing:** none. The evaluator has no
  reference-case family; its oracle is hand-computed exact decimals, the E4-2 pattern.
- **Invariants touched:** MI-18 (a revision starts with no track record: the evaluator's input
  carries no predecessor's score, and every scored row shows its lineage's revision count), ES-21
  (no clock, no randomness, no floats: `mandate-num` exact decimals and `UtcNanos` inputs), ES-09
  (the new error codes join `ResearchError`'s stable registry, add-only, each pinned live),
  MI-16 untouched (the evaluator reads theses; it never reads or changes an envelope field).
- **Crates in scope:** `mandate-research` (a new `score` module; `lib.rs` gains `pub mod score;`
  and the new error variants); `mandate-num` (one live addition, `Ratio::checked_mul`, the exact
  product the confidence bound's `z × σ` needs — the shared-crate pattern of #136 and #175, live
  with live tests because the crate is implemented).
- **Crates out of scope:** `mandate-marketdata` (layer 6 sits above this crate; the close series
  arrives as typed input, so no dependency), the journal (the executor's stream already carries
  `ThesisProposed`/`ThesisRevised`; how the shell selects the window's closed theses from it is
  the shell's), `mandate-backtest` (§8.6: a backtest may check a revision's mechanics, never
  score it — the evaluator and the backtest stay apart).
- **New dependencies allowed:** none.
- **Safety-critical:** the module is part of a safety-critical crate but adds no trading-path
  behaviour: it computes and reports, `admit()` and every other entry point untouched. The DEC-77
  two-PR flow applies (this is the tests PR).
- **Size budget:** the module's non-test surface is types, the boundary rules, and one aggregate
  — expected well under 400 non-test lines at implementation.

## The design (DEC-281)

DEC-99 fixes the evidence rule and DEC-122 the window, the metric, and the threshold; §12 leaves
the mechanics open. DEC-281 pins them:

1. **The evaluator is deterministic pure code in `mandate-research`'s `score` module.** No model
   call, no clock, no randomness, no floats: every figure is an exact decimal or one named
   rounding of one formula, the E4-2 report discipline. It scores ideas, not executions: the
   input thesis carries no admission outcome and no predecessor's score, so a refused thesis and
   an admitted one score the same (DEC-99's "every thesis"), and a revision starts from zero by
   construction (MI-18) — its row shows the lineage's revision count.
2. **The pre-registered decision is a typed input** (`EvaluationDecision`): the calendar window a
   scored thesis's horizon must close inside, the minimum count of scoreable closed theses, and
   the one-sided bound's `z` as an exact decimal (1.645 for DEC-122's 95%). The evaluator refuses
   an early run — fewer scoreable closed theses than the minimum — with `WindowNotClosed`, and a
   thesis whose horizon closes outside the registered window is refused outright
   (`ThesisOutsideWindow`): the caller selects, the evaluator re-checks, and nothing is silently
   included or dropped. The window's edges are inclusive: a horizon closing exactly at `from` or
   at `to` is inside. The registered figures themselves — DEC-122's three months and 100 closed
   theses — are the **shell's** to confirm when the decision is recorded: the evaluator applies
   whatever minimum was registered, so a registered minimum of one reports on one thesis, and
   guarding DEC-122's own figures happens at registration, not in the scoring.
3. **The boundary rule is no-lookahead at both edges.** The entry price is the first close
   *strictly after* the thesis's `as_of` — the first price the thesis could have acted on; a
   close at the same instant is not it, because the model's output is dated `as_of`. The exit
   price is the last close *at or before* the horizon end — nothing past the horizon is ever
   read. An edge with no close on its side of the rule makes the thesis **unscoreable**: it is
   excluded, named with its reason in the scorecard, and never counts toward the mean *or* the
   minimum — an unscoreable thesis can never help the evaluation pass.
4. **The per-thesis score is its direction's window return net of the modeled round-trip cost.**
   The window return is `(exit − entry) ÷ entry`, one rounding at 12 places half-even (the
   report scale of DEC-127 item 4); the round-trip cost arrives as an exact-decimal fraction of
   notional, computed by the caller from the same §6.4 fee model the backtest charges, so the
   evaluator never duplicates the fee schedule. v1 scores `Long` theses; `Direction::Other` is
   unscoreable, the ignored-output reading of DEC-32 carried into scoring.
5. **Excess is against each baseline over the same window.** The basket baseline is the
   equal-weighted mean of the eligible basket's members' own window returns; the index baseline
   is the broad index ETF's own window return. Each scored thesis carries its excess over both,
   thesis minus baseline.
6. **The aggregate is the mean excess per scoreable closed thesis against each baseline, and the
   pass threshold is the one-sided lower confidence bound above zero against both.** The margin
   is `z × σ ÷ √n` computed exactly where the existing arithmetic allows it:
   `root_ceiling(squared_quotient(z × σ, n))` — the product is exact (`Ratio::checked_mul`, the
   one `mandate-num` addition), the quotient one rounding, and the root *ceilinged*, so the
   margin is never understated; the bound is the mean less that margin, and pass is the bound
   above zero against **both** baselines, DEC-122's "against each". With fewer than two theses
   the sample variance is absent and the margin is zero: the bound is the mean itself, the
   tightest honest statement a single outcome supports.
7. **The scorecard is the report, and it recomputes from its own fields** (the E4-2 discipline,
   DEC-127 item 5's precedent that the report carries what a reader needs to recompute it): the
   report echoes the decision's `z` and each row's round-trip cost, the mean recomputes from the
   excess sum and the scoreable count, the margin from `z`, the variance, and the count, the
   bound from the mean less the margin, and pass from both bounds. Pure inputs give an identical
   scorecard every run, and the rows are ordered by thesis id, so the report is reproducible
   from the journal alone.

## Commands

```bash
cargo xtask check
cargo nextest run -p mandate-research --test score
cargo xtask ci pending
```

## Interpretations (the readings this brief takes)

1. "Scored after its horizon" (§8.6) reads the scoring window as the thesis's own claim window,
   `as_of` to `expires_at`, not the position's trading window: the evaluator measures the idea
   the model stated, on prices the model could not have seen beyond (R-27's whole point).
2. "Net of the cost model" (DEC-99) reads the cost as a modeled round-trip drag the caller
   supplies per thesis — the §6.4 schedule on the thesis's instrument — rather than realized
   fees, because the evaluator scores ideas, and a refused thesis still costs its round trip.
3. "Buy-and-hold of the eligible basket" (DEC-122) reads the basket as equal-weighted across its
   members over the same window: each member's own window return, then the mean. DEC-90's basket
   is the eligible basket of the thin slice.
4. "The later of three months and 100 closed theses" (DEC-122) reads "closed theses" as
   *scoreable* closed theses: a thesis that closed in the window but cannot be scored (no price
   on an edge, a non-Long direction, no series supplied) never counts toward the minimum —
   otherwise a data gap could admit an early, weaker evaluation.
5. An unscoreable thesis is reported, never silently dropped: the scorecard names each with its
   reason (`NoEntryClose`, `NoExitClose`, `NoSeries`, `NotScoreableDirection`), the fail-loud
   reading of a missing answer (AGENTS.md rule 3 applied to reporting: what cannot be scored is
   shown as unscored, not read as a quiet zero).
6. The evaluator refuses, never reports early: a run whose window is not yet closed is an error,
   because a report that says "pass" before the pre-registered window closes would be exactly the
   choose-the-answer-after-the-fact that DEC-99's pre-registration exists to prevent.
7. `z` is an input of the pre-registered decision, not a constant the code owns: the founder
   registers the confidence level's `z` (DEC-122's 95% is 1.645) with the window and the minimum,
   and the evaluator applies what was registered.

## Planted bugs (the seeds run against the implementation PR's real code, each caught by a named
test; round 1's review additionally ran the original 18 against a throwaway implementation,
18/18 caught)

| # | Bug | Caught by |
|---|---|---|
| 1 | The entry boundary reads at-or-before `as_of` (lookahead: the model's own close) | `the_entry_close_is_the_first_strictly_after_as_of` |
| 2 | The exit boundary reads past the horizon | `the_exit_close_is_the_last_at_or_before_the_horizon` |
| 3 | An unscoreable thesis counts toward the minimum | `unscoreable_theses_never_count_toward_the_minimum` |
| 4 | An unscoreable thesis reads as a quiet zero in the mean | `a_thesis_with_no_close_on_an_edge_is_unscoreable_and_never_counts` |
| 5 | The cost is added to the return | `a_long_thesis_scores_its_window_net_of_the_round_trip_cost` |
| 6 | Excess is baseline minus thesis | `excess_subtracts_each_baseline_over_the_same_window` |
| 7 | The basket baseline is its first member alone | `the_basket_is_the_equal_weighted_mean_of_its_members` |
| 8 | The mean is over all closed theses, not the scoreable ones | `a_thesis_without_a_series_is_unscoreable_and_never_counts` |
| 9 | The window reports with too few theses | `the_window_refuses_an_early_run` |
| 10 | A thesis outside the registered window is silently scored | `a_thesis_outside_the_registered_window_is_refused` |
| 11 | The margin's root is floored (understated) | `the_lower_bound_is_the_mean_less_the_ceilinged_margin` |
| 12 | A single thesis reports a variance | `a_single_thesis_has_no_dispersion_and_the_bound_is_the_mean` |
| 13 | Pass needs only one baseline | `pass_requires_the_bound_above_zero_against_both_baselines` |
| 14 | The scored rows forget the revision count (MI-18) | `every_scored_thesis_shows_its_lineage_s_revision_count` |
| 15 | The scorecard depends on the input order of theses or basket members | `the_scorecard_is_independent_of_the_input_order` |
| 16 | The window return rounds at the wrong scale or mode | `buy_and_hold_rounds_once_at_twelve_places` |
| 17 | The series accepts unordered closes, so the boundary rules read noise | `a_series_refuses_unordered_or_empty_closes` |
| 18 | A non-Long thesis is scored as Long | `a_non_long_thesis_is_unscoreable_and_never_counts` |
| 19 | σ's root is floored, so the margin is understated | `the_sigma_root_is_ceilinged_so_the_margin_is_never_understated` |
| 20 | A horizon before `from` is silently scored | `the_window_edges_are_inclusive` |
| 21 | `to` is treated as exclusive | `the_window_edges_are_inclusive` |
| 22 | The unscoreable rows keep the input's order | `a_thesis_with_no_close_on_an_edge_is_unscoreable_and_never_counts` |
| 23 | Every row's baselines are taken over the first thesis's window | `each_row_excess_uses_its_own_window` |

## Not done

- **The live evaluation:** running research agents on the internal paper workspaces over
  DEC-122's window, and the founder's go for it (the same gate as E17-0's paper runs).
- **The shell's selection:** reading the window's closed theses from the journal, fetching the
  close series from the datasets, and computing each thesis's round-trip cost from the §6.4
  model — the evaluator receives all three typed.
- **E17-9's revision loop** (DEC-111 item 5): unblocked by this, a story of its own.
- **Signal-model scorecards** (E15-3): a different report, per model, not per thesis.
- **User-facing scorecards:** §8.4 bars them until counsel answers question 35; this report is
  the founder's own Phase 1 quality gate, on internal paper workspaces only (DEC-103).
- **The property suite** lands with the implementation PR, where real behaviour exists to fuzz.
