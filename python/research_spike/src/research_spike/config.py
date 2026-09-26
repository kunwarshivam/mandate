"""Basket, hard caps, model, and the data directory. Every cap is a constant; nothing here is tunable."""

import tomllib
from dataclasses import dataclass
from decimal import Decimal
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[4]
BASKET_PATH = REPO_ROOT / "config" / "research-basket.toml"
DATA_DIR = Path.home() / ".local" / "share" / "mandate" / "research-spike"

MAX_POSITION_USD = Decimal("2000")
MAX_ORDER_USD = Decimal("2000")
MAX_POSITIONS = 6
MIN_CONVICTION = Decimal("0.3")
DEFAULT_HORIZON_DAYS = 5
MAX_HORIZON_DAYS = 30
MAX_THESES = 4

MODEL_ID = "anthropic/claude-sonnet-5"
BAR_COUNT = 20
NEWS_HOURS = 48
NEWS_LIMIT = 50
BENCHMARK = "SPY"

ENTRY_LIMIT_FACTOR = Decimal("1.002")
EXIT_LIMIT_FACTOR = Decimal("0.998")
QUOTE_SANITY_FRACTION = Decimal("0.05")


@dataclass(frozen=True)
class Basket:
    equities: tuple[str, ...]
    crypto: tuple[str, ...]

    @property
    def symbols(self) -> tuple[str, ...]:
        return self.equities + self.crypto

    def __contains__(self, symbol: object) -> bool:
        return symbol in self.symbols


def load_basket(path: Path = BASKET_PATH) -> Basket:
    with path.open("rb") as handle:
        doc = tomllib.load(handle)
    return Basket(
        equities=tuple(doc["us-equity"]["symbols"]),
        crypto=tuple(doc["crypto"]["symbols"]),
    )


def is_crypto(symbol: str) -> bool:
    return "/" in symbol


def data_dir(path: Path = DATA_DIR) -> Path:
    path.mkdir(parents=True, exist_ok=True)
    return path
