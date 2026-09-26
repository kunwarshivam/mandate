import json
from pathlib import Path

import pytest

from research_spike.config import Basket

FIXTURES = Path(__file__).parent / "fixtures"


def fixture_text(name: str) -> str:
    return (FIXTURES / name).read_text(encoding="utf-8")


@pytest.fixture
def basket() -> Basket:
    return Basket(equities=("SPY", "AAPL"), crypto=("BTC/USD",))


class FakeTransport:
    """Serves recorded responses by URL substring, in order, and records every request."""

    def __init__(self, routes: list[tuple[str, str]]):
        self.routes = list(routes)
        self.requests: list[tuple[str, str, dict, bytes | None]] = []

    def __call__(self, method: str, url: str, headers: dict, body: bytes | None) -> tuple[int, str]:
        self.requests.append((method, url, headers, body))
        for index, (needle, text) in enumerate(self.routes):
            if needle in url:
                del self.routes[index]
                return 200, text
        return 404, json.dumps({"message": f"no fixture for {url}"})
