"""Thin Alpaca client: market data from the data host, trading only against the paper host."""

import os
from dataclasses import dataclass, field
from datetime import UTC, datetime

from research_spike.http import Transport, request_json, urllib_transport

DATA_HOST = "https://data.alpaca.markets"
TRADING_HOST = "https://paper-api.alpaca.markets"
KEY_ID_ENV = "MANDATE_ALPACA_PAPER_KEY_ID"
SECRET_ENV = "MANDATE_ALPACA_PAPER_SECRET"
STOCK_FEED = "iex"


class AlpacaConfigError(Exception):
    pass


@dataclass(frozen=True)
class Credentials:
    key_id: str = field(repr=False)
    secret: str = field(repr=False)

    def __post_init__(self) -> None:
        if not self.key_id.startswith("PK"):
            raise AlpacaConfigError(f"{KEY_ID_ENV} is not a paper key ID (must start with PK)")
        if not self.secret:
            raise AlpacaConfigError(f"{SECRET_ENV} is empty")

    @classmethod
    def from_env(cls) -> Credentials:
        key_id = os.environ.get(KEY_ID_ENV)
        secret = os.environ.get(SECRET_ENV)
        if not key_id or not secret:
            raise AlpacaConfigError(f"set {KEY_ID_ENV} and {SECRET_ENV}")
        return cls(key_id, secret)


def iso(moment: datetime) -> str:
    return moment.astimezone(UTC).strftime("%Y-%m-%dT%H:%M:%SZ")


def news_symbol(symbol: str) -> str:
    return symbol.replace("/", "")


class AlpacaClient:
    def __init__(self, credentials: Credentials, transport: Transport = urllib_transport):
        self._headers = {
            "APCA-API-KEY-ID": credentials.key_id,
            "APCA-API-SECRET-KEY": credentials.secret,
            "Accept": "application/json",
        }
        self._transport = transport

    def _call(self, host: str, method: str, path: str, params=None, body=None) -> dict:
        if host not in (DATA_HOST, TRADING_HOST):
            raise AlpacaConfigError(f"refusing to call {host}: only the data and paper hosts are allowed")
        result = request_json(self._transport, method, f"{host}/{path}", self._headers, params, body)
        if not isinstance(result, dict | list):
            raise AlpacaConfigError(f"unexpected response shape from {path}")
        return result

    def _pages(self, path: str, params: dict[str, str]):
        token = None
        for _ in range(50):
            page = self._call(DATA_HOST, "GET", path, {**params, **({"page_token": token} if token else {})})
            yield page
            token = page.get("next_page_token")
            if not token:
                return

    def _paged_bars(self, path: str, params: dict[str, str]) -> dict[str, list[dict]]:
        bars: dict[str, list[dict]] = {}
        for page in self._pages(path, params):
            for symbol, rows in (page.get("bars") or {}).items():
                bars.setdefault(symbol, []).extend(rows)
        return bars

    def news(self, symbols: list[str], start: datetime, end: datetime, limit: int = 50) -> list[dict]:
        """The newest `limit` items in the window; one page, so the prompt stays bounded."""
        params = {
            "symbols": ",".join(news_symbol(s) for s in symbols),
            "start": iso(start),
            "end": iso(end),
            "limit": str(limit),
            "sort": "desc",
            "include_content": "false",
        }
        return list(self._call(DATA_HOST, "GET", "v1beta1/news", params).get("news") or [])

    def stock_bars(self, symbols: list[str], start: datetime, end: datetime) -> dict[str, list[dict]]:
        params = {
            "symbols": ",".join(symbols),
            "timeframe": "1Day",
            "start": iso(start),
            "end": iso(end),
            "limit": "10000",
            "adjustment": "raw",
            "feed": STOCK_FEED,
            "sort": "asc",
        }
        return self._paged_bars("v2/stocks/bars", params)

    def crypto_bars(self, symbols: list[str], start: datetime, end: datetime) -> dict[str, list[dict]]:
        params = {
            "symbols": ",".join(symbols),
            "timeframe": "1Day",
            "start": iso(start),
            "end": iso(end),
            "limit": "10000",
            "sort": "asc",
        }
        return self._paged_bars("v1beta3/crypto/us/bars", params)

    def latest_stock_quotes(self, symbols: list[str]) -> dict[str, dict]:
        params = {"symbols": ",".join(symbols), "feed": STOCK_FEED}
        return self._call(DATA_HOST, "GET", "v2/stocks/quotes/latest", params).get("quotes", {})

    def latest_crypto_quotes(self, symbols: list[str]) -> dict[str, dict]:
        params = {"symbols": ",".join(symbols)}
        return self._call(DATA_HOST, "GET", "v1beta3/crypto/us/latest/quotes", params).get("quotes", {})

    def account(self) -> dict:
        return self._call(TRADING_HOST, "GET", "v2/account")

    def clock(self) -> dict:
        return self._call(TRADING_HOST, "GET", "v2/clock")

    def positions(self) -> list[dict]:
        return list(self._call(TRADING_HOST, "GET", "v2/positions"))

    def open_orders(self) -> list[dict]:
        return list(self._call(TRADING_HOST, "GET", "v2/orders", {"status": "open", "limit": "500"}))

    def order(self, order_id: str) -> dict:
        return self._call(TRADING_HOST, "GET", f"v2/orders/{order_id}")

    def submit_order(self, order: dict) -> dict:
        return self._call(TRADING_HOST, "POST", "v2/orders", body=order)
