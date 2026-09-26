# Research-agent spike (E17-0, Track B)

Answers one question in two to three weeks of paper trading: can an LLM loop over news and prices
produce theses with positive expectancy net of costs, and at what cost per thesis? A spike: not
product code, not safety-critical, no gate, no journal crate, standard library only. Task brief:
[docs/project/tasks/RS-1-research-spike.md](../../docs/project/tasks/RS-1-research-spike.md).

## Commands

Run from `python/` with the paper credentials and the OpenRouter key in the environment
(`set -a; . ~/.config/mandate/paper.env; set +a`; ADR-0001 ES-19):

```bash
uv run python -m research_spike status          # account, positions, open orders, LLM spend so far
uv run python -m research_spike run --dry-run   # ingest, propose, journal, price orders; submit nothing
uv run python -m research_spike run             # the same, and submit paper limit orders
uv run python -m research_spike score           # realized returns versus SPY, cost per thesis; writes a report
uv run python -m research_spike report          # print the latest report
uv run pytest research_spike                    # offline tests against recorded fixtures
```

`run` is idempotent within a UTC day: a second run proposes nothing new, but still syncs fills and
plans exits. Equity orders go out only while the regular session is open; crypto orders any time.

## Rules (all constants in `config.py`)

- Basket from `config/research-basket.toml`; at most 4 theses per day, at most 6 positions,
  2000 USD per position and per order, conviction at least 0.3 to enter.
- Sizing is fixed and has no LLM in it: notional = round(conviction × confidence × 2000, 2); whole
  shares (crypto: six places); buy limit = ask × 1.002 and sell limit = bid × 0.998, both rounded
  to the tick; equities `day` in the regular session, crypto `gtc`. A quote more than 5% from the
  last close is replaced by the last close. Never a market order, a short, or an option.
- Exits: conviction ≤ −0.3 on a held symbol, or the horizon has passed, sell the whole position.
- Every submission is journaled before the HTTP call; the trading host is the paper host only, and
  the key ID must start with `PK`.

## Data directory

`~/.local/share/mandate/research-spike/` (never inside the repository):

- `journal.jsonl`: append-only records (`thesis`, `order_submitted`, `order_update`, `fill`, `mark`,
  `score`, `llm_call`), each with a UTC timestamp and the SHA-256 of the previous record;
- `artifacts/<sha256>.txt`: the prompt and the response of every LLM call;
- `openrouter-models.json`: the model catalogue with per-token prices, fetched once;
- `reports/score-<date>.txt`: the plain-text score reports.
