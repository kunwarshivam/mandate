from datetime import UTC, datetime
from decimal import Decimal

from research_spike import score
from research_spike.journal import Journal


def at(day: int, hour: int = 14) -> datetime:
    return datetime(2026, 10, day, hour, tzinfo=UTC)


def build(tmp_path) -> Journal:
    journal = Journal(tmp_path)
    call = journal.append(
        "llm_call", {"cost_usd": "0.030000", "prompt_tokens": 4000, "completion_tokens": 200}, at(1, 13)
    )
    common = {
        "status": "accepted",
        "call": call["hash"],
        "direction": "long",
        "horizon_days": 3,
        "thesis": "",
        "evidence": [],
        "invalidation": "",
    }
    won = journal.append(
        "thesis", {**common, "instrument": "AAPL", "conviction": "0.6", "confidence": "0.5"}, at(1, 13)
    )
    lost = journal.append(
        "thesis", {**common, "instrument": "QQQ", "conviction": "0.5", "confidence": "0.5"}, at(1, 13)
    )
    journal.append(
        "thesis", {**common, "instrument": "GLD", "conviction": "0.4", "confidence": "0.5"}, at(1, 13)
    )
    journal.append(
        "thesis",
        {**common, "status": "rejected", "instrument": "TSLA", "conviction": "0.9", "confidence": "0.9"},
        at(1, 13),
    )
    journal.append(
        "mark",
        {"closes": {"SPY": {"close": "100"}, "AAPL": {"close": "200"}, "QQQ": {"close": "50"}}},
        at(1, 13),
    )
    for thesis, symbol, price in ((won, "AAPL", "200"), (lost, "QQQ", "50")):
        fill = {
            "submission": "s",
            "order_id": "o",
            "thesis_id": thesis["hash"],
            "symbol": symbol,
            "side": "buy",
            "qty": "2",
            "price": price,
            "filled_at": at(1).isoformat(),
        }
        journal.append("fill", fill, at(1))
    journal.append(
        "mark",
        {"closes": {"SPY": {"close": "102"}, "AAPL": {"close": "210"}, "QQQ": {"close": "49"}}},
        at(2, 13),
    )
    journal.append(
        "fill",
        {
            "submission": "s2",
            "order_id": "o2",
            "thesis_id": won["hash"],
            "symbol": "AAPL",
            "side": "sell",
            "qty": "2",
            "price": "212",
            "filled_at": at(2).isoformat(),
        },
        at(2),
    )
    journal.append(
        "mark",
        {"closes": {"SPY": {"close": "101"}, "AAPL": {"close": "215"}, "QQQ": {"close": "48"}}},
        at(3, 13),
    )
    return journal


def test_outcomes_against_hand_computed_returns(tmp_path):
    results, unfilled = score.outcomes(build(tmp_path))
    aapl, qqq = results
    assert aapl.closed and (aapl.ret, aapl.benchmark_ret, aapl.excess, aapl.pnl_usd) == (
        Decimal("0.06"),
        Decimal("0.02"),
        Decimal("0.04"),
        Decimal("24.00"),
    )
    assert not qqq.closed and (qqq.exit_price, qqq.ret, qqq.benchmark_ret) == (
        Decimal("48"),
        Decimal("-0.04"),
        Decimal("0.01"),
    )
    assert qqq.excess == Decimal("-0.05") and qqq.pnl_usd == Decimal("-4.00")
    assert len(unfilled) == 1


def test_summary_and_report(tmp_path):
    journal = build(tmp_path)
    summary = score.summarize(journal)
    assert summary["hit_rate"] == "0.500000" and summary["mean_excess_return"] == "-0.005000"
    assert summary["expectancy_usd"] == "10.000000" and summary["total_pnl_usd"] == "20.00"
    assert summary["cost_per_thesis_usd"] == "0.010000" and summary["llm_cost_usd"] == "0.030000"
    assert (
        summary["theses_accepted"],
        summary["theses_filled"],
        summary["theses_closed"],
        summary["theses_unfilled"],
    ) == (3, 2, 1, 1)
    assert summary["turnover"] == str(
        ((Decimal(400) + Decimal(424) + Decimal(100)) / Decimal(12000)).quantize(Decimal("0.000001"))
    )
    text = score.write_report(journal, tmp_path, "2026-10-03T15:00:00+00:00")
    assert (tmp_path / "reports" / "score-2026-10-03.txt").read_text() == text
    assert (
        "hit_rate                0.500000" in text
        and journal.records("score")[-1]["payload"]["hit_rate"] == "0.500000"
    )
