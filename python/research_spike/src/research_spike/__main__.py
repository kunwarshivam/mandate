"""`python -m research_spike {run [--dry-run] | score | report | status}`."""

import argparse
import sys
from dataclasses import asdict
from datetime import UTC, datetime, timedelta
from decimal import Decimal

from research_spike import execute, openrouter, propose, score
from research_spike.alpaca import AlpacaClient, Credentials
from research_spike.config import BAR_COUNT, MODEL_ID, NEWS_HOURS, NEWS_LIMIT, Basket, data_dir, load_basket
from research_spike.execute import Intent, Position, Quote
from research_spike.journal import Journal


def ingest(
    client: AlpacaClient, basket: Basket, now: datetime
) -> tuple[dict[str, list[dict]], list[dict], dict[str, Quote]]:
    start = now - timedelta(days=BAR_COUNT * 2)
    bars = client.stock_bars(list(basket.equities), start, now)
    bars.update(client.crypto_bars(list(basket.crypto), start, now))
    bars = {symbol: rows[-BAR_COUNT:] for symbol, rows in bars.items()}
    news = client.news(list(basket.symbols), now - timedelta(hours=NEWS_HOURS), now, NEWS_LIMIT)
    raw_quotes = client.latest_stock_quotes(list(basket.equities)) | client.latest_crypto_quotes(
        list(basket.crypto)
    )
    quotes = {}
    for symbol in basket.symbols:
        last_close = Decimal(str(bars[symbol][-1]["c"])) if bars.get(symbol) else None
        quote, source = execute.choose_quote(raw_quotes.get(symbol), last_close)
        if quote is not None:
            quotes[symbol] = quote
        if source != "quote":
            print(f"quote: {symbol} has no usable live quote; pricing from the last close {last_close}")
    return bars, news, quotes


def describe(prefix: str, record: dict, *keys: str) -> str:
    return f"{prefix}: " + " ".join(f"{key}={record.get(key)}" for key in keys)


def run(dry_run: bool) -> None:
    now = datetime.now(UTC)
    today = now.date()
    client = AlpacaClient(Credentials.from_env())
    directory = data_dir()
    journal = Journal(directory)
    journal.verify()
    basket = load_basket()
    bars, news, quotes = ingest(client, basket, now)
    journal.append(
        "mark",
        {
            "as_of": now.isoformat(),
            "closes": {
                s: {"t": str(rows[-1]["t"]), "close": str(rows[-1]["c"])} for s, rows in bars.items() if rows
            },
        },
        now,
    )
    positions = [
        Position(p["symbol"], Decimal(str(p.get("qty_available", p["qty"])))) for p in client.positions()
    ]
    held = {p.symbol: str(p.qty) for p in positions}
    for fill in execute.sync_orders(client, journal):
        print(describe("fill", fill["payload"], "side", "qty", "symbol", "price"))

    already = [r for r in journal.records("llm_call") if r["ts"][:10] == today.isoformat()]
    if already:
        print(f"already proposed today ({len(already)} call(s)); proposing nothing new")
    else:
        system, user = propose.SYSTEM_PROMPT, propose.build_user_prompt(basket, bars, news, held, now)
        prices = openrouter.model_prices(MODEL_ID, directory)
        completion = openrouter.complete(MODEL_ID, system, user, prices, openrouter.api_key())
        call = journal.append(
            "llm_call",
            {
                "model": completion.model,
                "request_id": completion.request_id,
                "prompt_sha256": journal.store_artifact(system + "\n\n" + user),
                "response_sha256": journal.store_artifact(completion.content),
                "prompt_tokens": completion.prompt_tokens,
                "completion_tokens": completion.completion_tokens,
                "cost_usd": str(completion.cost_usd),
                "news_items": len(news),
            },
            now,
        )
        tokens = f"{completion.prompt_tokens}+{completion.completion_tokens}"
        print(f"llm: {completion.model} {tokens} tokens, cost {completion.cost_usd} USD")
        try:
            accepted, rejected = propose.parse_response(
                completion.content, basket, {str(n["id"]) for n in news}, set(held)
            )
        except propose.ThesisError as error:
            journal.append("thesis", {"status": "rejected", "call": call["hash"], "reason": str(error)}, now)
            print(f"response rejected: {error}")
            accepted, rejected = [], []
        for thesis in accepted:
            journal.append("thesis", {"status": "accepted", "call": call["hash"], **thesis.payload()}, now)
        for item in rejected:
            journal.append("thesis", {"status": "rejected", "call": call["hash"], **item}, now)
            print(f"thesis rejected: {item['reason']}")

    theses = [
        (
            r["hash"],
            propose.Thesis(
                **{k: r["payload"][k] for k in propose.FIELDS}
                | {
                    "conviction": Decimal(str(r["payload"]["conviction"])),
                    "confidence": Decimal(str(r["payload"]["confidence"])),
                    "evidence": tuple(r["payload"]["evidence"]),
                }
            ),
        )
        for r in journal.records("thesis")
        if r["payload"]["status"] == "accepted" and r["ts"][:10] == today.isoformat()
    ]
    for thesis_id, thesis in theses:
        print(
            describe(
                f"thesis {thesis_id[:12]}",
                thesis.payload(),
                "instrument",
                "conviction",
                "confidence",
                "horizon_days",
            )
        )
        print(f"  {thesis.thesis}")

    open_symbols = {o["symbol"] for o in client.open_orders()} | execute.submitted_today(journal, today)
    intents = execute.plan_exits(
        theses, positions, open_symbols, quotes, execute.latest_entries(journal), today
    )
    intents += execute.plan_entries(
        theses, positions, open_symbols | {i.symbol for i in intents}, quotes, today
    )
    market_open = bool(client.clock().get("is_open"))
    for intent in intents:
        _place(client, journal, intent, dry_run, market_open, now)
    if not intents:
        print("no orders")
    print(f"journal: {journal.path} ({len(journal.records())} records)")


def _place(
    client: AlpacaClient, journal: Journal, intent: Intent, dry_run: bool, market_open: bool, now: datetime
) -> None:
    line = describe(
        intent.reason, asdict(intent), "side", "qty", "symbol", "limit_price", "notional", "time_in_force"
    )
    if not dry_run and not market_open and intent.time_in_force == "day":
        print(f"{line}: not submitted, the regular session is closed")
        return
    execute.submit(client, journal, intent, dry_run, now)
    print(f"{line}: {'dry run, not submitted' if dry_run else 'submitted'}")


def status() -> None:
    client = AlpacaClient(Credentials.from_env())
    journal = Journal(data_dir())
    account = client.account()
    print(describe("account", account, "status", "equity", "cash", "buying_power"))
    for p in client.positions():
        print(describe("position", p, "symbol", "qty", "avg_entry_price", "market_value", "unrealized_pl"))
    for o in client.open_orders():
        print(describe("open order", o, "symbol", "side", "qty", "limit_price", "status", "submitted_at"))
    calls = journal.records("llm_call")
    spent = sum((Decimal(str(c["payload"]["cost_usd"])) for c in calls), Decimal(0))
    print(f"llm spend: {spent} USD over {len(calls)} call(s); journal records: {len(journal.records())}")


def score_command() -> None:
    directory = data_dir()
    journal = Journal(directory)
    journal.verify()
    print(score.write_report(journal, directory, datetime.now(UTC).isoformat(timespec="seconds")), end="")


def report() -> None:
    reports = sorted((data_dir() / "reports").glob("score-*.txt"))
    print(
        reports[-1].read_text(encoding="utf-8")
        if reports
        else "no report yet; run `python -m research_spike score`",
        end="",
    )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="research_spike")
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("run").add_argument("--dry-run", action="store_true")
    for name in ("score", "report", "status"):
        commands.add_parser(name)
    args = parser.parse_args(argv)
    match args.command:
        case "run":
            run(args.dry_run)
        case "score":
            score_command()
        case "report":
            report()
        case "status":
            status()
    return 0


if __name__ == "__main__":
    sys.exit(main())
