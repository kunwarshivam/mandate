"""Deterministic sizing and order submission. No LLM here; every submission is journaled first."""

from dataclasses import asdict, dataclass
from datetime import UTC, date, datetime, timedelta
from decimal import ROUND_DOWN, ROUND_HALF_UP, Decimal

from research_spike.alpaca import AlpacaClient
from research_spike.config import (
    ENTRY_LIMIT_FACTOR,
    EXIT_LIMIT_FACTOR,
    MAX_ORDER_USD,
    MAX_POSITION_USD,
    MAX_POSITIONS,
    MIN_CONVICTION,
    QUOTE_SANITY_FRACTION,
    is_crypto,
)
from research_spike.http import HttpError
from research_spike.journal import Journal
from research_spike.propose import Thesis

CRYPTO_QTY_STEP = Decimal("0.000001")
TERMINAL_STATUSES = frozenset({"filled", "canceled", "expired", "rejected", "replaced", "done_for_day"})


class OrderSizeError(Exception):
    pass


@dataclass(frozen=True)
class Quote:
    bid: Decimal
    ask: Decimal


@dataclass(frozen=True)
class Position:
    symbol: str
    qty: Decimal


@dataclass(frozen=True)
class Intent:
    symbol: str
    side: str
    qty: Decimal
    limit_price: Decimal
    notional: Decimal
    time_in_force: str
    reason: str
    thesis_id: str
    expires_on: str

    def order(self, client_order_id: str) -> dict:
        return {
            "symbol": self.symbol,
            "qty": str(self.qty),
            "side": self.side,
            "type": "limit",
            "time_in_force": self.time_in_force,
            "limit_price": str(self.limit_price),
            "extended_hours": False,
            "client_order_id": client_order_id,
        }


def choose_quote(raw: dict | None, last_close: Decimal | None) -> tuple[Quote | None, str]:
    """The live quote when bid and ask are both within 5% of the last close; otherwise the last close.

    Without a last close nothing is priced: an unchecked quote never becomes an order."""
    if last_close is None or last_close <= 0:
        return None, "none"
    raw = raw or {}
    bid, ask = Decimal(str(raw.get("bp") or 0)), Decimal(str(raw.get("ap") or 0))
    tolerance = last_close * QUOTE_SANITY_FRACTION
    if bid > 0 and ask > 0 and abs(ask - last_close) <= tolerance and abs(bid - last_close) <= tolerance:
        return Quote(bid, ask), "quote"
    return Quote(last_close, last_close), "close"


def tick_size(price: Decimal, crypto: bool) -> Decimal:
    if crypto or price >= 1:
        return Decimal("0.01")
    return Decimal("0.0001")


def round_to_tick(price: Decimal, crypto: bool) -> Decimal:
    return price.quantize(tick_size(price, crypto), rounding=ROUND_HALF_UP)


def entry_notional(conviction: Decimal, confidence: Decimal) -> Decimal:
    notional = (conviction * confidence * MAX_POSITION_USD).quantize(Decimal("0.01"), rounding=ROUND_HALF_UP)
    return min(notional, MAX_ORDER_USD)


def entry_quantity(notional: Decimal, limit_price: Decimal, crypto: bool) -> Decimal:
    """Whole shares (crypto: six places) that the notional buys at the limit price, so that
    qty times limit never exceeds the notional and therefore never MAX_ORDER_USD."""
    raw = notional / limit_price
    return (
        raw.quantize(CRYPTO_QTY_STEP, rounding=ROUND_DOWN)
        if crypto
        else raw.to_integral_value(rounding=ROUND_DOWN)
    )


def entry_limit(ask: Decimal, crypto: bool) -> Decimal:
    return round_to_tick(ask * ENTRY_LIMIT_FACTOR, crypto)


def exit_limit(bid: Decimal, crypto: bool) -> Decimal:
    return round_to_tick(bid * EXIT_LIMIT_FACTOR, crypto)


def time_in_force(symbol: str) -> str:
    return "gtc" if is_crypto(symbol) else "day"


def plan_entries(
    theses: list[tuple[str, Thesis]],
    positions: list[Position],
    open_symbols: set[str],
    quotes: dict[str, Quote],
    today: date,
) -> list[Intent]:
    held = {p.symbol for p in positions}
    slots = MAX_POSITIONS - len(held) - len(open_symbols - held)
    intents: list[Intent] = []
    for thesis_id, thesis in theses:
        symbol = thesis.instrument
        if (
            thesis.conviction < MIN_CONVICTION
            or symbol in held
            or symbol in open_symbols
            or symbol not in quotes
        ):
            continue
        if slots - len(intents) <= 0:
            break
        crypto = is_crypto(symbol)
        ask = quotes[symbol].ask
        if ask <= 0:
            continue
        limit = entry_limit(ask, crypto)
        qty = entry_quantity(entry_notional(thesis.conviction, thesis.confidence), limit, crypto)
        if qty <= 0:
            continue
        expires_on = (today + timedelta(days=thesis.horizon_days)).isoformat()
        intents.append(
            Intent(
                symbol,
                "buy",
                qty,
                limit,
                (qty * limit).quantize(Decimal("0.01"), rounding=ROUND_HALF_UP),
                time_in_force(symbol),
                "entry",
                thesis_id,
                expires_on,
            )
        )
    return intents


def plan_exits(
    theses: list[tuple[str, Thesis]],
    positions: list[Position],
    open_symbols: set[str],
    quotes: dict[str, Quote],
    entries: dict[str, dict],
    today: date,
) -> list[Intent]:
    """Sell a whole position when a thesis turns against it or its horizon has passed."""
    bearish = {t.instrument: (tid, t) for tid, t in theses if t.conviction <= -MIN_CONVICTION}
    intents: list[Intent] = []
    for position in positions:
        symbol = position.symbol
        if symbol in open_symbols or symbol not in quotes or position.qty <= 0:
            continue
        entry = entries.get(symbol, {})
        thesis_id = entry.get("thesis_id", "")
        if symbol in bearish:
            reason = "exit_conviction"
            thesis_id = thesis_id or bearish[symbol][0]
        elif entry.get("expires_on") and date.fromisoformat(entry["expires_on"]) <= today:
            reason = "exit_horizon"
        else:
            continue
        crypto = is_crypto(symbol)
        limit = exit_limit(quotes[symbol].bid, crypto)
        intents.append(
            Intent(
                symbol,
                "sell",
                position.qty,
                limit,
                (position.qty * limit).quantize(Decimal("0.01"), rounding=ROUND_HALF_UP),
                time_in_force(symbol),
                reason,
                thesis_id,
                "",
            )
        )
    return intents


def latest_entries(journal: Journal) -> dict[str, dict]:
    """Per symbol, the latest real (not dry-run) buy submission: its thesis id and expiry."""
    entries: dict[str, dict] = {}
    for record in journal.records("order_submitted"):
        payload = record["payload"]
        if payload["side"] == "buy" and not payload["dry_run"]:
            entries[payload["symbol"]] = payload
    return entries


def submitted_today(journal: Journal, today: date) -> set[str]:
    return {
        r["payload"]["symbol"]
        for r in journal.records("order_submitted")
        if not r["payload"]["dry_run"] and r["ts"][:10] == today.isoformat()
    }


def submit(
    client: AlpacaClient | None, journal: Journal, intent: Intent, dry_run: bool, now: datetime
) -> dict:
    client_order_id = (
        f"rs-{now.strftime('%Y%m%d')}-{intent.side}-{intent.symbol.replace('/', '')}-{intent.thesis_id[:8]}"
    )
    order = intent.order(client_order_id)
    if intent.side == "buy" and intent.qty * intent.limit_price > MAX_ORDER_USD:
        raise OrderSizeError(
            f"{intent.symbol}: {intent.qty} x {intent.limit_price} exceeds {MAX_ORDER_USD} USD"
        )
    submitted = journal.append("order_submitted", {**asdict(intent), "order": order, "dry_run": dry_run}, now)
    if dry_run or client is None:
        return submitted
    try:
        reply = client.submit_order(order)
        update = {"order_id": reply.get("id"), "status": reply.get("status"), "reply": reply}
    except HttpError as error:
        update = {"order_id": None, "status": "rejected", "error": str(error)}
    journal.append(
        "order_update",
        {
            **update,
            "submission": submitted["hash"],
            "symbol": intent.symbol,
            "side": intent.side,
            "thesis_id": intent.thesis_id,
        },
    )
    return submitted


def sync_orders(client: AlpacaClient, journal: Journal) -> list[dict]:
    """Fetch every order that is neither terminal nor already filled in the journal; journal a status
    change as an update and a complete fill (quantity and price present) as a fill."""
    latest: dict[str, dict] = {}
    for update in journal.records("order_update"):
        latest[update["payload"]["submission"]] = update["payload"]
    fills: list[dict] = []
    filled = {f["payload"]["submission"] for f in journal.records("fill")}
    for submission, update in latest.items():
        if (
            not update.get("order_id")
            or submission in filled
            or update.get("status") in TERMINAL_STATUSES - {"filled"}
        ):
            continue
        order = client.order(update["order_id"])
        if order.get("status") != update.get("status"):
            journal.append("order_update", {**update, "status": order.get("status"), "reply": order})
        if order.get("status") == "filled" and order.get("filled_qty") and order.get("filled_avg_price"):
            fill = {
                "submission": submission,
                "order_id": update["order_id"],
                "thesis_id": update["thesis_id"],
                "symbol": update["symbol"],
                "side": update["side"],
                "qty": str(Decimal(str(order["filled_qty"]))),
                "price": str(Decimal(str(order["filled_avg_price"]))),
                "filled_at": str(order.get("filled_at")),
            }
            fills.append(journal.append("fill", fill, datetime.now(UTC)))
    return fills
