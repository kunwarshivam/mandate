"""Realized return per thesis against SPY over the same window, with the LLM cost per thesis."""

from dataclasses import dataclass
from decimal import Decimal
from pathlib import Path

from research_spike.config import BENCHMARK, MAX_POSITION_USD, MAX_POSITIONS
from research_spike.journal import Journal

PLACES = Decimal("0.000001")


@dataclass(frozen=True)
class Outcome:
    thesis_id: str
    symbol: str
    conviction: Decimal
    confidence: Decimal
    entry_price: Decimal
    qty: Decimal
    entry_at: str
    exit_price: Decimal
    exit_at: str
    closed: bool
    benchmark_entry: Decimal
    benchmark_exit: Decimal

    @property
    def ret(self) -> Decimal:
        return ((self.exit_price - self.entry_price) / self.entry_price).quantize(PLACES)

    @property
    def benchmark_ret(self) -> Decimal:
        return ((self.benchmark_exit - self.benchmark_entry) / self.benchmark_entry).quantize(PLACES)

    @property
    def excess(self) -> Decimal:
        return self.ret - self.benchmark_ret

    @property
    def pnl_usd(self) -> Decimal:
        return ((self.exit_price - self.entry_price) * self.qty).quantize(Decimal("0.01"))


def _close_at(marks: list[dict], symbol: str, at: str) -> Decimal | None:
    """The close from the latest mark at or before `at`, else the earliest mark after it."""
    chosen = None
    for mark in marks:
        if symbol in mark["payload"]["closes"] and (mark["ts"] <= at or chosen is None):
            chosen = mark
        if mark["ts"] > at and chosen is not None:
            break
    return Decimal(str(chosen["payload"]["closes"][symbol]["close"])) if chosen else None


def outcomes(journal: Journal) -> tuple[list[Outcome], list[str]]:
    """Filled theses with their outcomes, and the ids of accepted theses that never filled."""
    marks = journal.records("mark")
    fills = journal.records("fill")
    latest_ts = marks[-1]["ts"] if marks else ""
    results: list[Outcome] = []
    unfilled: list[str] = []
    for record in journal.records("thesis"):
        thesis = record["payload"]
        if thesis["status"] != "accepted" or Decimal(str(thesis["conviction"])) < 0:
            continue
        thesis_id = record["hash"]
        entry = next(
            (
                f["payload"]
                for f in fills
                if f["payload"]["thesis_id"] == thesis_id and f["payload"]["side"] == "buy"
            ),
            None,
        )
        if entry is None:
            unfilled.append(thesis_id)
            continue
        symbol = thesis["instrument"]
        exit_fill = next(
            (
                f["payload"]
                for f in fills
                if f["payload"]["thesis_id"] == thesis_id and f["payload"]["side"] == "sell"
            ),
            None,
        )
        if exit_fill:
            exit_price, exit_at, closed = Decimal(exit_fill["price"]), exit_fill["filled_at"], True
        else:
            exit_price, exit_at, closed = (
                _close_at(marks, symbol, latest_ts) or Decimal(entry["price"]),
                latest_ts,
                False,
            )
        bench_entry = _close_at(marks, BENCHMARK, entry["filled_at"])
        bench_exit = _close_at(marks, BENCHMARK, exit_at)
        results.append(
            Outcome(
                thesis_id,
                symbol,
                Decimal(str(thesis["conviction"])),
                Decimal(str(thesis["confidence"])),
                Decimal(entry["price"]),
                Decimal(entry["qty"]),
                entry["filled_at"],
                exit_price,
                exit_at,
                closed,
                bench_entry or Decimal(1),
                bench_exit or bench_entry or Decimal(1),
            )
        )
    return results, unfilled


def _mean(values: list[Decimal]) -> Decimal:
    return (sum(values, Decimal(0)) / len(values)).quantize(PLACES) if values else Decimal(0)


def summarize(journal: Journal) -> dict:
    results, unfilled = outcomes(journal)
    calls = journal.records("llm_call")
    llm_cost = sum((Decimal(str(c["payload"]["cost_usd"])) for c in calls), Decimal(0))
    tokens = sum(int(c["payload"]["prompt_tokens"]) + int(c["payload"]["completion_tokens"]) for c in calls)
    accepted = [r for r in journal.records("thesis") if r["payload"]["status"] == "accepted"]
    traded = sum(
        (f.qty * f.entry_price + (f.qty * f.exit_price if f.closed else Decimal(0)) for f in results),
        Decimal(0),
    )
    return {
        "llm_calls": len(calls),
        "tokens": tokens,
        "llm_cost_usd": str(llm_cost),
        "theses_accepted": len(accepted),
        "theses_filled": len(results),
        "theses_closed": sum(1 for r in results if r.closed),
        "theses_unfilled": len(unfilled),
        "hit_rate": str(_mean([Decimal(int(r.excess > 0)) for r in results])),
        "mean_return": str(_mean([r.ret for r in results])),
        "mean_benchmark_return": str(_mean([r.benchmark_ret for r in results])),
        "mean_excess_return": str(_mean([r.excess for r in results])),
        "expectancy_usd": str(_mean([r.pnl_usd for r in results])),
        "total_pnl_usd": str(sum((r.pnl_usd for r in results), Decimal(0))),
        "cost_per_thesis_usd": str((llm_cost / len(accepted)).quantize(PLACES) if accepted else llm_cost),
        "turnover": str((traded / (MAX_POSITIONS * MAX_POSITION_USD)).quantize(PLACES)),
        "outcomes": [
            {
                "thesis_id": r.thesis_id[:12],
                "symbol": r.symbol,
                "entry": str(r.entry_price),
                "exit": str(r.exit_price),
                "closed": r.closed,
                "return": str(r.ret),
                "benchmark": str(r.benchmark_ret),
                "excess": str(r.excess),
                "pnl_usd": str(r.pnl_usd),
            }
            for r in results
        ],
    }


def render(summary: dict, as_of: str) -> str:
    lines = [f"Research spike score as of {as_of}", ""]
    for key in (
        "llm_calls",
        "tokens",
        "llm_cost_usd",
        "cost_per_thesis_usd",
        "theses_accepted",
        "theses_filled",
        "theses_closed",
        "theses_unfilled",
        "hit_rate",
        "mean_return",
        "mean_benchmark_return",
        "mean_excess_return",
        "expectancy_usd",
        "total_pnl_usd",
        "turnover",
    ):
        lines.append(f"{key:<24}{summary[key]}")
    columns = ("thesis_id", "symbol", "entry", "exit", "closed", "return", "benchmark", "excess", "pnl_usd")
    lines += ["", "".join(f"{name:>12}" for name in columns)]
    for outcome in summary["outcomes"]:
        lines.append("".join(f"{outcome[name]!s:>12}" for name in columns))
    return "\n".join(lines) + "\n"


def write_report(journal: Journal, data_dir: Path, as_of: str) -> str:
    summary = summarize(journal)
    text = render(summary, as_of)
    reports = data_dir / "reports"
    reports.mkdir(exist_ok=True)
    (reports / f"score-{as_of[:10]}.txt").write_text(text, encoding="utf-8")
    journal.append("score", {"as_of": as_of, **{k: v for k, v in summary.items() if k != "outcomes"}})
    return text
