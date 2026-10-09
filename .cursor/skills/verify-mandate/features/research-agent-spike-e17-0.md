# Research-agent spike (E17-0)

- **Spec:** ADR-0002; mandate spec §8.1 to §8.3 (the thesis shape and the sizing idea);
  `docs/project/tasks/RS-1-research-spike.md`. A spike, not product code.
- **Code:** `python/research_spike/src/research_spike/config.py` (basket, caps, model, data dir),
  `python/research_spike/src/research_spike/alpaca.py` (data host and paper host only),
  `python/research_spike/src/research_spike/openrouter.py` (completions, tokens, cost),
  `python/research_spike/src/research_spike/propose.py` (prompt and strict thesis validation),
  `python/research_spike/src/research_spike/journal.py` (hash-chained JSON Lines, artifacts),
  `python/research_spike/src/research_spike/execute.py` (deterministic sizing, journal before
  submit), `python/research_spike/src/research_spike/score.py` (returns versus SPY, cost per thesis),
  `python/research_spike/src/research_spike/__main__.py` (`run`, `score`, `report`, `status`).
- **Tests:** `python/research_spike/tests/` (validation, sizing against hand-computed values, the
  hash chain, scoring on a synthetic series, recorded market-data fixtures).
- **Run:** `cd python && uv run pytest research_spike`; live against the paper account,
  `uv run python -m research_spike run --dry-run` (see `python/research_spike/README.md`).
