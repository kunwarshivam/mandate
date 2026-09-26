from datetime import UTC, datetime
from decimal import Decimal

import pytest

from research_spike.alpaca import DATA_HOST, TRADING_HOST, AlpacaClient, AlpacaConfigError, Credentials
from research_spike.http import HttpError

from .conftest import FakeTransport, fixture_text

START, END = datetime(2026, 9, 1, tzinfo=UTC), datetime(2026, 9, 26, tzinfo=UTC)


def client(transport: FakeTransport) -> AlpacaClient:
    return AlpacaClient(Credentials("PKTESTKEY", "secret"), transport)


def test_only_paper_key_ids_are_accepted(monkeypatch):
    with pytest.raises(AlpacaConfigError, match="paper"):
        Credentials("AKLIVEKEY", "secret")
    monkeypatch.delenv("MANDATE_ALPACA_PAPER_KEY_ID", raising=False)
    with pytest.raises(AlpacaConfigError, match="MANDATE_ALPACA_PAPER_KEY_ID"):
        Credentials.from_env()
    assert "secret" not in repr(Credentials("PKTESTKEY", "secret"))


def test_bars_are_decimal_and_pages_are_merged():
    transport = FakeTransport(
        [
            ("page_token", fixture_text("stock_bars_page2.json")),
            ("stocks/bars", fixture_text("stock_bars_page1.json")),
        ]
    )
    bars = client(transport).stock_bars(["AAPL", "SPY"], START, END)
    assert len(bars["AAPL"]) == 3 and len(bars["SPY"]) == 3
    assert bars["AAPL"][0]["c"] == Decimal("231.5") and isinstance(bars["AAPL"][0]["h"], Decimal)
    assert "page_token=page-2" in transport.requests[1][1]
    assert all(url.startswith(f"{DATA_HOST}/v2/stocks/bars?") for _, url, _, _ in transport.requests)
    assert "feed=iex" in transport.requests[0][1] and "adjustment=raw" in transport.requests[0][1]


def test_crypto_bars_keep_every_digit():
    bars = client(FakeTransport([("crypto/us/bars", fixture_text("crypto_bars.json"))])).crypto_bars(
        ["BTC/USD"], START, END
    )
    assert bars["BTC/USD"][0]["o"] == Decimal("109250.1234567891")


def test_news_uses_alpaca_symbols_and_the_window():
    transport = FakeTransport([("v1beta1/news", fixture_text("news.json"))])
    news = client(transport).news(["AAPL", "BTC/USD"], START, END)
    assert [n["id"] for n in news] == [48123456, 48123457]
    url = transport.requests[0][1]
    assert (
        "symbols=AAPL%2CBTCUSD" in url
        and "start=2026-09-01T00%3A00%3A00Z" in url
        and "include_content=false" in url
    )


def test_quotes_and_credentials_headers():
    transport = FakeTransport([("quotes/latest", fixture_text("stock_quotes.json"))])
    quotes = client(transport).latest_stock_quotes(["AAPL", "SPY"])
    assert quotes["AAPL"]["ap"] == Decimal("233.45") and quotes["SPY"]["bp"] == Decimal("663.05")
    headers = transport.requests[0][2]
    assert headers["APCA-API-KEY-ID"] == "PKTESTKEY" and headers["APCA-API-SECRET-KEY"] == "secret"


def test_trading_calls_go_only_to_the_paper_host():
    transport = FakeTransport(
        [
            ("v2/account", "{}"),
            ("v2/positions", "[]"),
            ("v2/orders", "[]"),
            ("v2/clock", "{}"),
            ("v2/orders", '{"id": "o"}'),
        ]
    )
    c = client(transport)
    c.account(), c.positions(), c.open_orders(), c.clock()
    assert c.submit_order({"symbol": "AAPL"})["id"] == "o"
    assert all(url.startswith(f"{TRADING_HOST}/v2/") for _, url, _, _ in transport.requests)
    assert transport.requests[-1][0] == "POST" and transport.requests[-1][3] == b'{"symbol": "AAPL"}'
    with pytest.raises(AlpacaConfigError, match="refusing"):
        c._call("https://api.alpaca.markets", "GET", "v2/account")


def test_http_errors_carry_status_and_body():
    with pytest.raises(HttpError, match="HTTP 404") as error:
        client(FakeTransport([])).account()
    assert error.value.status == 404
