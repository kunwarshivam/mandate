from datetime import UTC, date, datetime
from decimal import Decimal

import pytest

from research_spike import execute
from research_spike.config import MAX_POSITIONS, MIN_CONVICTION
from research_spike.execute import Intent, Position, Quote
from research_spike.journal import Journal
from research_spike.propose import Thesis

TODAY = date(2026, 9, 26)
NOW = datetime(2026, 9, 26, 14, 0, tzinfo=UTC)
D = Decimal


def thesis(symbol="AAPL", conviction="0.6", confidence="0.5", horizon=5) -> Thesis:
    return Thesis(symbol, "long", D(conviction), D(confidence), horizon, "observation", (), "condition")


@pytest.mark.parametrize(
    ("conviction", "confidence", "expected"),
    [
        ("0.6", "0.5", "600.00"),
        ("1", "1", "2000.00"),
        ("0.3", "0.333", "199.80"),
        ("0.333", "0.333", "221.78"),
    ],
)
def test_entry_notional_is_conviction_times_confidence_times_the_cap(conviction, confidence, expected):
    assert execute.entry_notional(D(conviction), D(confidence)) == D(expected)


def test_entry_quantity_truncates_shares_and_six_places_for_crypto():
    assert execute.entry_quantity(D("600.00"), D("123.45"), crypto=False) == D("4")
    assert execute.entry_quantity(D("100.00"), D("123.45"), crypto=False) == D("0")
    assert execute.entry_quantity(D("600.00"), D("65000.12"), crypto=True) == D("0.009230")


@pytest.mark.parametrize(
    ("ask", "crypto", "expected"),
    [
        ("123.45", False, "123.70"),
        ("0.9000", False, "0.9018"),
        ("65000.12", True, "65130.12"),
        ("1.00", False, "1.00"),
    ],
)
def test_entry_limit_is_ask_plus_twenty_bps_rounded_to_the_tick(ask, crypto, expected):
    assert execute.entry_limit(D(ask), crypto) == D(expected)


def test_exit_limit_is_bid_minus_twenty_bps_rounded_to_the_tick():
    assert execute.exit_limit(D("123.40"), crypto=False) == D("123.15")
    assert execute.exit_limit(D("65000.12"), crypto=True) == D("64870.12")


def test_choose_quote_falls_back_to_the_close_when_the_quote_is_missing_or_far():
    assert execute.choose_quote({"bp": D("233.40"), "ap": D("233.45")}, D("231.5")) == (
        QUOTES["AAPL"],
        "quote",
    )
    assert execute.choose_quote({"bp": D("233.40"), "ap": D("233.45")}, None) == (QUOTES["AAPL"], "quote")
    assert execute.choose_quote({"bp": 0, "ap": 0}, D("231.5")) == (Quote(D("231.5"), D("231.5")), "close")
    assert execute.choose_quote(None, D("231.5")) == (Quote(D("231.5"), D("231.5")), "close")
    assert execute.choose_quote({"bp": D("540"), "ap": D("544.69")}, D("516.16")) == (
        Quote(D("516.16"), D("516.16")),
        "close",
    )
    assert execute.choose_quote({"bp": D("540"), "ap": D("541.96")}, D("516.16"))[1] == "quote"
    assert execute.choose_quote({"bp": 0, "ap": 0}, None) == (None, "none")


QUOTES = {
    "AAPL": Quote(D("233.40"), D("233.45")),
    "SPY": Quote(D("663.05"), D("663.10")),
    "BTC/USD": Quote(D("109000"), D("109010")),
}


def test_plan_entries_sizes_a_new_position_by_hand_computed_values():
    (intent,) = execute.plan_entries([("t1", thesis())], [], set(), QUOTES, TODAY)
    assert intent == Intent(
        "AAPL", "buy", D("2"), D("233.92"), D("467.84"), "day", "entry", "t1", "2026-10-01"
    )
    assert intent.order("cid") == {
        "symbol": "AAPL",
        "qty": "2",
        "side": "buy",
        "type": "limit",
        "time_in_force": "day",
        "limit_price": "233.92",
        "extended_hours": False,
        "client_order_id": "cid",
    }


def test_plan_entries_skips_low_conviction_held_open_and_unquoted():
    theses = [
        ("a", thesis(conviction=str(MIN_CONVICTION - D("0.01")))),
        ("b", thesis("SPY")),
        ("c", thesis("BTC/USD")),
        ("d", thesis("QQQ")),
    ]
    assert execute.plan_entries(theses, [Position("SPY", D(1))], {"BTC/USD"}, QUOTES, TODAY) == []


def test_plan_entries_respects_the_position_cap():
    positions = [Position(f"P{i}", D(1)) for i in range(MAX_POSITIONS - 2)]
    theses = [("a", thesis("AAPL")), ("b", thesis("SPY", "1", "1")), ("c", thesis("BTC/USD"))]
    planned = execute.plan_entries(theses, positions, {"OPEN"}, QUOTES, TODAY)
    assert [i.symbol for i in planned] == ["AAPL"]
    assert execute.plan_entries(theses, positions, set(), QUOTES, TODAY)[1].symbol == "SPY"


def test_crypto_entries_use_gtc_and_six_places():
    (intent,) = execute.plan_entries([("t", thesis("BTC/USD", "1", "1"))], [], set(), QUOTES, TODAY)
    assert (intent.qty, intent.limit_price, intent.time_in_force) == (D("0.018346"), D("109228.02"), "gtc")


def test_plan_exits_sells_the_whole_position_on_negative_conviction_or_expired_horizon():
    positions = [Position("AAPL", D(3)), Position("SPY", D(1)), Position("BTC/USD", D("0.01"))]
    entries = {
        "AAPL": {"thesis_id": "old", "expires_on": "2026-09-30"},
        "SPY": {"thesis_id": "s", "expires_on": "2026-09-26"},
    }
    exits = execute.plan_exits([("n", thesis(conviction="-0.3"))], positions, set(), QUOTES, entries, TODAY)
    assert exits == [
        Intent("AAPL", "sell", D(3), D("232.93"), D("698.79"), "day", "exit_conviction", "old", ""),
        Intent("SPY", "sell", D(1), D("661.72"), D("661.72"), "day", "exit_horizon", "s", ""),
    ]


def test_plan_exits_holds_otherwise():
    positions = [Position("AAPL", D(3))]
    entries = {"AAPL": {"thesis_id": "old", "expires_on": "2026-09-27"}}
    assert (
        execute.plan_exits([("n", thesis(conviction="-0.29"))], positions, {"SPY"}, QUOTES, entries, TODAY)
        == []
    )
    assert execute.plan_exits([], positions, {"AAPL"}, QUOTES, entries, TODAY) == []


class RecordingClient:
    def __init__(self, journal: Journal):
        self.journal = journal
        self.orders: list[tuple[int, dict]] = []

    def submit_order(self, order: dict) -> dict:
        self.orders.append((len(self.journal.records()), order))
        return {"id": "o-1", "status": "accepted"}


def test_submission_is_journaled_before_the_http_call(tmp_path):
    journal = Journal(tmp_path)
    client = RecordingClient(journal)
    intent = execute.plan_entries([("abcdef123456", thesis())], [], set(), QUOTES, TODAY)[0]
    execute.submit(client, journal, intent, dry_run=False, now=NOW)
    ((records_before, order),) = client.orders
    assert records_before == 1 and journal.records("order_submitted")[0]["payload"]["order"] == order
    assert (
        order["type"] == "limit"
        and order["side"] == "buy"
        and order["client_order_id"] == "rs-20260926-buy-AAPL-abcdef12"
    )
    assert journal.records("order_update")[0]["payload"]["order_id"] == "o-1"
    assert execute.submitted_today(journal, TODAY) == {"AAPL"}
    assert execute.latest_entries(journal)["AAPL"]["expires_on"] == "2026-10-01"


def test_dry_run_journals_but_never_calls_the_broker(tmp_path):
    journal = Journal(tmp_path)
    client = RecordingClient(journal)
    intent = execute.plan_entries([("t", thesis())], [], set(), QUOTES, TODAY)[0]
    execute.submit(client, journal, intent, dry_run=True, now=NOW)
    assert client.orders == [] and journal.records("order_submitted")[0]["payload"]["dry_run"] is True
    assert execute.submitted_today(journal, TODAY) == set() and execute.latest_entries(journal) == {}
