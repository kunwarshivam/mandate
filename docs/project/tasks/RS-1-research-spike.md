# Task: E17-0 Research-agent spike (Track B)

Agent task brief ([ADR-0001](../../adr/0001-engineering-setup.md) ES-15). One task implements one
story. **This is a spike**, timeboxed to two to three weeks of paper trading; its exit is a
decision-log entry that records what the report showed and what E17 does with it.

## Story

- **Story:** E17-0 ([backlog](../06-backlog-v1.md#e17-research-agent-and-dynamic-universe)), the
  spike that de-risks [ADR-0002](../../adr/0002-autonomous-ideation-and-retail.md)'s research agent
  (E17-2) before it is built as product code.
- **Acceptance criteria (verbatim):** "a report with hit rate, expectancy versus SPY, and cost per
  thesis after two to three weeks of paper trading."
- **PRD / HLD / spec anchors:** PRD 6.3 (FR-3.9), 6.5; HLD §5 (research agent beside the signal
  models); mandate spec §8.1 (LLM outputs are observations, evidence, and invalidation; signal models
  never place orders), §8.2 (the output shape the theses take), §8.3 (the sizing idea: conviction ×
  confidence × cap, at the ask, truncated to the increment).
- **Decisions that apply:** DEC-04 (LLMs produce opinions, never orders), DEC-72 (ES-10 Python
  workspace, ES-19 local paper secrets), DEC-79 (agents merge after review), DEC-90 (research
  basket), DEC-97 and DEC-98 (ADR-0002).

## Scope

- **Reference cases that must move from pending to passing:** none.
- **Invariants touched:** none of the spec's; the spike's own "never" clauses, each a test:

  | Clause | Test |
  |---|---|
  | Theses match §8.2 and are validated strictly; symbols outside the basket, forbidden language, unknown evidence, and malformed envelopes are rejected | `test_propose.py::test_invalid_theses_are_rejected`, `test_extra_or_missing_fields_are_rejected`, `test_malformed_envelopes_raise` |
  | Sizing is deterministic and matches hand-computed values; caps hold | `test_execute.py::test_entry_notional_*`, `test_entry_quantity_*`, `test_entry_limit_*`, `test_plan_entries_respects_the_position_cap` |
  | Never a market order or a short; exits sell the whole position | `test_execute.py::test_plan_entries_sizes_*` (type limit, side buy), `test_plan_exits_*` |
  | Journal before acting | `test_execute.py::test_submission_is_journaled_before_the_http_call` |
  | Dry run never calls the broker | `test_execute.py::test_dry_run_journals_but_never_calls_the_broker` |
  | Hash chain detects tampering | `test_journal.py::test_tampering_is_detected` |
  | Prices, quantities, and money are never floats | `test_alpaca.py::test_bars_are_decimal_*`, `test_crypto_bars_keep_every_digit`, `test_propose.py::test_valid_thesis_is_accepted_with_decimal_numbers` |
  | Paper host only; paper key IDs only | `test_alpaca.py::test_trading_calls_go_only_to_the_paper_host`, `test_only_paper_key_ids_are_accepted` |
  | Scoring against a synthetic series | `test_score.py::test_outcomes_against_hand_computed_returns` |

- **Crates in scope:** none. New uv workspace member `python/research_spike/` (package
  `research_spike`), plus one `xtask` change so the dependency check reads the new manifest and
  skips uv workspace members.
- **Crates out of scope:** every Rust crate. Nothing here feeds the runtime, the gate, or the
  journal crate.
- **New dependencies allowed:** none (standard library only: `urllib`, `json`, `tomllib`,
  `hashlib`, `decimal`, `datetime`).
- **Safety-critical:** no. Spike code; it trades only the agent paper account.
- **Size budget:** 800 non-generated lines per PR; the code and docs ship first, the tests and
  fixtures as a second PR.

## Interpretations and decisions

1. **Shape.** A thesis is `instrument`, `direction` (long only), `conviction` in [−1, 1] (negative
   means exit a held position and is rejected for symbols not held), `confidence` in [0, 1],
   `horizon_days` in [1, 30], `thesis` (observations only; "buy", "sell", "should", "must", "price
   target", "guaranteed", and "will rise/fall" are rejected), `evidence` (news ids from the prompt),
   `invalidation`. At most 4 per call, one per instrument; an empty list is valid. Rejected theses
   are journaled with the reason, as §8.1 asks.
2. **Sizing** (no LLM): notional = round(conviction × confidence × 2000, 2), capped at 2000; whole
   shares, crypto to six places; buy limit ask × 1.002 and sell limit bid × 0.998, half-up to the
   tick (0.01; 0.0001 under 1 USD); equities `day` in the regular session only, crypto `gtc`
   (Alpaca's crypto orders do not accept `day`). Skip when positions plus open entries reach 6 or
   the symbol already has an order today. Exits sell the whole position when conviction ≤ −0.3 or
   the horizon (entry day + `horizon_days`, calendar days) has passed.
3. **Prices** come from the latest IEX quote (crypto: the latest crypto quote). A quote that is
   empty or whose ask is more than 5% from the last daily close (a stale weekend quote, one bad
   tick) is not used; the last close prices the plan instead, so a wrong quote can only produce a
   limit that does not fill. Equity orders are submitted only while the regular session is open.
4. **Cost** is computed from OpenRouter's per-token prices for the model, fetched once and cached;
   the model is `anthropic/claude-sonnet-5`, temperature 0, JSON output. The founder provided the
   key; spend is bounded to one call per UTC day.
5. **Scoring:** per filled thesis, the return from the entry fill to the exit fill (or the latest
   journaled close if open) against SPY's closes over the same window; hit rate is the share with
   positive excess return; expectancy is the mean P&L in USD per filled thesis; cost per thesis is
   the total LLM cost over accepted theses; turnover is filled notional over 12,000 USD (six
   positions at the cap).
6. **Journal.** JSON Lines with a light SHA-256 chain, not the journal spec's canonical form or
   streams; enough to make the record append-only and tamper-evident for the report.

## Commands

```bash
cargo xtask check
cd python && uv run pytest research_spike
uv run python -m research_spike run --dry-run
```

## Stop conditions

Stop and write a DEC proposal instead of continuing if any of these happen:

- the spec is ambiguous or seems wrong;
- a new dependency seems necessary;
- a test would have to be weakened, skipped, or deleted;
- anything would deviate from an accepted decision.

## Definition of done

- [ ] The cited reference cases pass, and none that passed before now fails (none cited).
- [x] Tests came first for the sizing rule, validation, the hash chain, and scoring, each against
      hand-computed values.
- [x] New state changes emit journal events (the spike's own JSON Lines journal).
- [x] Docs updated where behavior, interfaces, or decisions changed.
- [ ] `cargo xtask check` is green (paste the summary in the PR).
- [ ] The PR description is complete (see the PR template).
- [ ] Exit: after two to three weeks of paper trading, `python -m research_spike score` and a
      decision-log entry with the hit rate, expectancy versus SPY, and cost per thesis.
