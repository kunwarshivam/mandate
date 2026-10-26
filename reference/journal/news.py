"""Journal spec v0.97 §9.37's reference vectors (DEC-2278): the news stream `news:{workspace_id}`, its
`StreamOpened`, and `NewsItemStored`, one record per news item the workspace's news reader stored,
with rules 283 to 286.

The schemas and the per-record rules 283 and 286 live in `control.py`, beside §9.19's meter stream,
so one validator judges every closed schema. This module builds the `news_stream` section:

1. base drafts: the stream's opening, an item about two tickers, and a licensed item;
2. an invalid draft for each member type and each clause of rules 283 and 286, and valid drafts for
   the near-misses (1 and 32 tickers, `knowledge_at` equal to `published_at`, the item id's whole
   alphabet and length, a prefix that only looks reserved);
3. streams for rules 284 and 285, which read the stream's earlier records.

Its own oracles read the members directly, without the validator: rule 283 over a product of
tickers, ids and times, and rules 284 and 285 by comparing every pair of records over random
streams. Every seeded bug is shown caught.
"""

from __future__ import annotations

import copy
import itertools
import random

from common import apply_change, change, delete
from control import WORKSPACE, check_of, reported, violations

SPEC = "docs/specs/journal.md v0.97 §9.37 rules 283 to 286 (DEC-2278)"
NEWS = f"news:{WORKSPACE}"
READER = {"kind": "system", "id": "news_reader", "version": "0.1.0", "build": "sha256:" + "7" * 64}
USER = {"kind": "user", "id": "user_owner_01", "version": "0.1.0", "build": None}
AGENT = {"kind": "agent", "id": "agent_a", "version": "0.1.0", "build": "sha256:" + "a" * 64}
ITEM_REF = "sha256:" + "4e" * 32
LICENSED_REF = "sha256:" + "5f" * 32
OTHER_REF = "sha256:" + "6a" * 32
PUBLISHED = "2026-10-26T14:58:12.000000000Z"
RECEIVED = "2026-10-26T15:00:01.250000000Z"
OPENED = "opened"
ITEM = "item"
LICENSED = "licensed"
RULE_283 = "rule 283"
RULE_284 = "rule 284"
RULE_285 = "rule 285"
RULE_286 = "rule 286"
TICKERS_33 = [f"T{n:02d}" for n in range(33)]
GRAMMAR = "alpaca:41827733.v2_A-z"


def record(event_id: str, event_type: str, payload: dict, refs: list[str]) -> dict:
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": event_id,
        "stream_id": NEWS,
        "event_type": event_type,
        "schema_version": 1,
        "event_time": RECEIVED,
        "clock_source": "local",
        "causation_id": None,
        "correlation_id": None,
        "actor": dict(READER),
        "config_refs": {},
        "payload": payload,
        "artifact_refs": refs,
        "pii_refs": [],
    }


def item_payload(item_id: str, knowledge_at: str, ref: str = ITEM_REF, **members) -> dict:
    payload = {
        "source": "alpaca.news",
        "allowlist_version": 3,
        "item_id": item_id,
        "symbols": ["AAPL", "SPY"],
        "published_at": PUBLISHED,
        "knowledge_at": knowledge_at,
        "item_ref": ref,
        "licensed": False,
    }
    payload.update(members)
    return payload


def opened(event_id: str = "01JAF0N0000000000000000N01") -> dict:
    return record(event_id, "StreamOpened", {"stream_type": "news", "workspace_id": WORKSPACE}, [])


def item(n: int, item_id: str, knowledge_at: str, ref: str = ITEM_REF) -> dict:
    return record(f"01JAF0N00000000000000000{n:02d}", "NewsItemStored", item_payload(item_id, knowledge_at, ref), [ref])


def invalid(name, clause, base, changes, reason, path, also=()):
    """`also` lists, in report order, the later rules a draft breaks on purpose: the journal reports
    only the first, and the generator checks the whole order."""
    expect = {"outcome": "Invalid", "reason": reason, "path": path}
    if also:
        expect["also"] = [{"reason": r, "path": p} for r, p in also]
    return {"name": name, "clause": clause, "base_draft": base, "changes": changes, "expect": expect}


def valid(name, clause, base, changes):
    return {"name": name, "clause": clause, "base_draft": base, "changes": changes, "expect": {"outcome": "Valid"}}


def base_drafts() -> dict[str, dict]:
    """The stream's opening, written in the batch of its first item; an item about two tickers,
    known at its receipt; and a licensed item, whose record names a typed record holding no text."""
    licensed = item(3, "alpaca:41827801", RECEIVED, LICENSED_REF)
    licensed["payload"].update(licensed=True, symbols=["MSFT"], source="alpaca.news")
    return {OPENED: opened(), ITEM: item(2, "alpaca:41827733", RECEIVED), LICENSED: licensed}


def invalid_drafts() -> list[dict]:
    """Each draft breaks the closed schema or one clause of rules 283 and 286, at the member it names;
    `also` lists what a draft breaks on purpose after the first."""

    def bad(name, clause, base, changes, path, reason="schema", also=()):
        return invalid(name, clause, base, changes, reason, path, also)

    return [
        bad("item_ref_missing", "§9.37: every member present", ITEM, [delete("payload.item_ref"), change("artifact_refs", [])], "payload.item_ref"),
        bad("licensed_missing", "§9.37: every member present", ITEM, [delete("payload.licensed")], "payload.licensed"),
        bad("headline_kept", "§9.37: the record carries no headline", ITEM, [change("payload.headline", "Shares rise")], "payload.headline"),
        bad("raw_ref_kept", "§9.37: the record names no raw response", ITEM, [change("payload.raw_ref", OTHER_REF), change("artifact_refs", [ITEM_REF, OTHER_REF])], "payload.raw_ref"),
        bad("allowlist_version_text", "§9.37: `allowlist_version` is an integer", ITEM, [change("payload.allowlist_version", "3")], "payload.allowlist_version"),
        bad("licensed_text", "§9.37: `licensed` is a boolean", ITEM, [change("payload.licensed", "false")], "payload.licensed"),
        bad("symbol_number", "§9.37: each ticker is text", ITEM, [change("payload.symbols", ["AAPL", 7])], "payload.symbols[1]"),
        bad("item_id_null", "§9.37: `item_id` is never null", ITEM, [change("payload.item_id", None)], "payload.item_id"),
        bad("published_with_offset", "§4: a timestamp is UTC, written with `Z`", ITEM, [change("payload.published_at", "2026-10-26T10:58:12.000000000-04:00")], "payload.published_at", "non_canonical"),
        bad("item_ref_not_a_digest", "§4: a `ref` is `sha256:` and 64 hex digits", ITEM, [change("payload.item_ref", "sha256:4E"), change("artifact_refs", [])], "payload.item_ref", "non_canonical"),
        bad("item_ref_unlisted", "§3: `artifact_refs` lists the payload's refs", ITEM, [change("artifact_refs", [])], "artifact_refs", "artifact_refs"),
        bad("no_tickers", f"{RULE_283}: one to 32 tickers", ITEM, [change("payload.symbols", [])], "payload.symbols"),
        bad("thirty_three_tickers", f"{RULE_283}: one to 32 tickers", ITEM, [change("payload.symbols", TICKERS_33)], "payload.symbols"),
        bad("tickers_unsorted", f"{RULE_283}: tickers sorted by bytes", ITEM, [change("payload.symbols", ["SPY", "AAPL"])], "payload.symbols"),
        bad("tickers_by_case", f"{RULE_283}: sorted by bytes, so `SPY` before `aapl`", ITEM, [change("payload.symbols", ["aapl", "SPY"])], "payload.symbols"),
        bad("ticker_repeated", f"{RULE_283}: no ticker twice", ITEM, [change("payload.symbols", ["SPY", "SPY"])], "payload.symbols"),
        bad("item_id_empty", "§4: empty text is not canonical, before rule 283's grammar", ITEM, [change("payload.item_id", "")], "payload.item_id", "non_canonical"),
        bad("item_id_too_long", f"{RULE_283}: the item id's grammar, one to 128 characters", ITEM, [change("payload.item_id", "a" * 129)], "payload.item_id"),
        bad("item_id_space", f"{RULE_283}: the item id's grammar", ITEM, [change("payload.item_id", "alpaca 41827733")], "payload.item_id"),
        bad("item_id_slash", f"{RULE_283}: the item id's grammar", ITEM, [change("payload.item_id", "alpaca/41827733")], "payload.item_id"),
        bad("item_id_bar", f"{RULE_283}: never the research core's `bar:`", ITEM, [change("payload.item_id", "bar:SPY:2026-10-26")], "payload.item_id"),
        bad("item_id_quote", f"{RULE_283}: never the research core's `quote:`", ITEM, [change("payload.item_id", "quote:SPY")], "payload.item_id"),
        bad("item_id_instrument", f"{RULE_283}: never the research core's `instrument:`", ITEM, [change("payload.item_id", "instrument:SPY")], "payload.item_id"),
        bad("known_before_published", f"{RULE_283}: known not before published", ITEM, [change("payload.knowledge_at", "2026-10-26T14:58:11.999999999Z")], "payload.knowledge_at"),
        bad("tickers_before_id", f"{RULE_283}: its clauses in order, tickers first", ITEM, [change("payload.symbols", ["SPY", "AAPL"]), change("payload.item_id", "bar:SPY"), change("payload.knowledge_at", "2026-10-26T14:00:00.000000000Z")], "payload.symbols"),
        bad("id_before_known", f"{RULE_283}: its clauses in order, the id before the time", ITEM, [change("payload.item_id", "quote:SPY"), change("payload.knowledge_at", "2026-10-26T14:00:00.000000000Z")], "payload.item_id"),
        bad("written_by_a_user", f"{RULE_286}: the news reader, a `system` actor", ITEM, [change("actor", dict(USER))], "actor.kind"),
        bad("written_by_an_agent", f"{RULE_286}: the news reader, a `system` actor", ITEM, [change("actor", dict(AGENT))], "actor.kind"),
        bad("opened_by_a_user", f"{RULE_286}: every record on the stream, its opening included", OPENED, [change("actor", dict(USER))], "actor.kind"),
        bad("rule_283_then_286", f"{RULE_283} before {RULE_286}'s writer", ITEM, [change("payload.symbols", []), change("actor", dict(USER))], "payload.symbols", also=[("schema", "actor.kind")]),
        bad("opened_for_another_workspace", f"{RULE_286}: `stream_id` is `news:{{workspace_id}}`", OPENED, [change("payload.workspace_id", "ws_other")], "stream_id", "stream_mismatch"),
        bad("opened_as_a_meter", "§9.37: the news stream's opening names `news`", OPENED, [change("payload.stream_type", "meter")], "payload.stream_type", "non_canonical"),
        bad("writer_before_mismatch", f"{RULE_286}: its writer reported before its subject", OPENED, [change("actor", dict(USER)), change("payload.workspace_id", "ws_other")], "actor.kind", also=[("stream_mismatch", "stream_id")]),
        bad("version_2", "§9.37: registered at `schema_version` 1 only", ITEM, [change("schema_version", 2)], "payload", "unknown_schema"),
        bad("on_the_meter_stream", "§9.37: a news-stream record", ITEM, [change("stream_id", f"meter:{WORKSPACE}")], "event_type", "wrong_stream"),
        bad("on_the_control_stream", "§9.37: a news-stream record", ITEM, [change("stream_id", f"ctl:{WORKSPACE}")], "event_type", "wrong_stream"),
        bad("stream_with_three_segments", "§2: `news:{workspace_id}`", ITEM, [change("stream_id", f"news:{WORKSPACE}:agent_a")], "stream_id", "non_canonical"),
    ]


def valid_drafts() -> list[dict]:
    """What the rules might be misread to refuse."""

    def ok(name, clause, base, changes):
        return valid(name, clause, base, changes)

    return [
        ok("one_ticker", f"{RULE_283}: one ticker", ITEM, [change("payload.symbols", ["SPY"])]),
        ok("thirty_two_tickers", f"{RULE_283}: 32 tickers", ITEM, [change("payload.symbols", TICKERS_33[:32])]),
        ok("known_when_published", f"{RULE_283}: `knowledge_at` may equal `published_at`", ITEM, [change("payload.knowledge_at", PUBLISHED)]),
        ok("published_in_the_future_known_then", f"{RULE_283}: a vendor clock ahead is clamped to its `published_at`", ITEM,
           [change("payload.published_at", "2026-10-26T15:00:01.900000000Z"), change("payload.knowledge_at", "2026-10-26T15:00:01.900000000Z")]),
        ok("the_whole_alphabet", f"{RULE_283}: letters, digits, `.`, `_`, `:`, and `-`", ITEM, [change("payload.item_id", GRAMMAR)]),
        ok("one_hundred_twenty_eight", f"{RULE_283}: 128 characters", ITEM, [change("payload.item_id", "a" * 128)]),
        ok("one_character", f"{RULE_283}: one character", ITEM, [change("payload.item_id", "7")]),
        ok("bars_is_not_bar", f"{RULE_283}: only the prefix `bar:` is reserved", ITEM, [change("payload.item_id", "bars:SPY")]),
        ok("bar_inside", f"{RULE_283}: a reserved word inside the id", ITEM, [change("payload.item_id", "alpaca:bar:1")]),
        ok("lower_case_tickers", f"{RULE_283}: tickers are the vendor's, sorted by bytes", ITEM, [change("payload.symbols", ["BRK.B", "aapl"])]),
        ok("another_allowlist_version", "§9.37: the version in effect at the fetch", ITEM, [change("payload.allowlist_version", 4)]),
        ok("live", "§9.37: either environment", ITEM, [change("environment", "live")]),
        ok("caused", "§9.37: no rule reads `causation_id`", ITEM, [change("causation_id", "01JAF0N000000000000000C001")]),
    ]


# --------------------------------------------------------------------------- rules 284 and 285


def stream_failure(drafts: list[dict], skip: frozenset[str] = frozenset()) -> dict | None:
    """Rules 284 and 285 over a news stream, in that order for each record: the first record that
    breaks one, with the member reported."""
    seen: set[str] = set()
    last: str | None = None
    for i, d in enumerate(drafts):
        if d["event_type"] != "NewsItemStored":
            continue
        p = d["payload"]
        if p["item_id"] in seen and "stream.284" not in skip:
            return {"index": i, "reason": "schema", "path": "payload.item_id"}
        earlier = last is not None and p["knowledge_at"] < last
        if "loose.285.strict" in skip:
            earlier = last is not None and p["knowledge_at"] <= last
        if earlier and "stream.285" not in skip:
            return {"index": i, "reason": "schema", "path": "payload.knowledge_at"}
        seen.add(p["item_id"])
        last = p["knowledge_at"]
    return None


def at(second: int) -> str:
    return f"2026-10-26T15:{second // 60:02d}:{second % 60:02d}.000000000Z"


def stream_cases() -> list[dict]:
    def case(name, clause, records, expect):
        return {"name": name, "clause": clause, "records": [opened(), *records], "expect": expect}

    def fails(index, path):
        return {"outcome": "Invalid", "index": index, "reason": "schema", "path": path}

    ok = {"outcome": "Valid"}
    return [
        case("two_items", f"{RULE_284}, {RULE_285}: two items, later known", [item(2, "a:1", at(1)), item(3, "a:2", at(2))], ok),
        case("one_response", f"{RULE_285}: equal is allowed, one response holds many items", [item(2, "a:1", at(1)), item(3, "a:2", at(1))], ok),
        case("a_later_version", f"{RULE_284}: a vendor's later version is a new item id", [item(2, "a:1:v1", at(1)), item(3, "a:1:v2", at(2))], ok),
        case("stored_twice", f"{RULE_284}: one record per item id", [item(2, "a:1", at(1)), item(3, "a:1", at(2))], fails(2, "payload.item_id")),
        case("stored_again_later", f"{RULE_284}: a refetch after others stores nothing", [item(2, "a:1", at(1)), item(3, "a:2", at(2)), item(4, "a:1", at(3))], fails(3, "payload.item_id")),
        case("known_earlier", f"{RULE_285}: known times never go back", [item(2, "a:1", at(2)), item(3, "a:2", at(1))], fails(2, "payload.knowledge_at")),
        case("known_earlier_than_the_last_only", f"{RULE_285}: against the previous record", [item(2, "a:1", at(1)), item(3, "a:2", at(3)), item(4, "a:3", at(2))], fails(3, "payload.knowledge_at")),
        case("repeat_before_time", f"{RULE_284} before {RULE_285}", [item(2, "a:1", at(2)), item(3, "a:1", at(1))], fails(2, "payload.item_id")),
    ]


# --------------------------------------------------------------------------- independent oracles

ORACLE_CHECKS = (
    "drafts.valid",
    "invalid_drafts",
    "valid_drafts",
    "streams.records",
    "streams.rule",
    "items.oracle",
    "streams.oracle",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def expected_item(symbols: list[str], item_id: str, published: str, known: str) -> list[tuple[str, str]]:
    """Rule 283 read directly: the first clause that fails, tickers, then the id, then the time. An
    empty id is refused before any rule, as §4 refuses empty text."""
    if item_id == "":
        return [("non_canonical", "payload.item_id")]
    tickers_ok = 1 <= len(symbols) <= 32 and all(a < b for a, b in zip(symbols, symbols[1:]))
    if not tickers_ok:
        return [("schema", "payload.symbols")]
    alphabet = set("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._:-")
    id_ok = 1 <= len(item_id) <= 128 and set(item_id) <= alphabet
    id_ok = id_ok and item_id.split(":")[0] + ":" not in ("bar:", "quote:", "instrument:")
    if not id_ok:
        return [("schema", "payload.item_id")]
    if known < published:
        return [("schema", "payload.knowledge_at")]
    return []


def expected_stream(records: list[dict]) -> dict | None:
    """Rules 284 and 285 read pairwise: a record fails when any earlier item names its id, or when the
    item just before it was known later."""
    items = [(i, d["payload"]) for i, d in enumerate(records) if d["event_type"] == "NewsItemStored"]
    for k, (i, p) in enumerate(items):
        if any(q["item_id"] == p["item_id"] for _, q in items[:k]):
            return {"index": i, "reason": "schema", "path": "payload.item_id"}
        if k and items[k - 1][1]["knowledge_at"] > p["knowledge_at"]:
            return {"index": i, "reason": "schema", "path": "payload.knowledge_at"}
    return None


def expected_failure(case: dict) -> dict | None:
    e = case["expect"]
    return None if e["outcome"] == "Valid" else {k: e[k] for k in ("index", "reason", "path")}


def draft_for(section: dict, case: dict) -> dict:
    draft = copy.deepcopy(section["drafts"][case["base_draft"]])
    for change_ in case["changes"]:
        apply_change(draft, change_)
    return draft


def check_section(section: dict, skip: frozenset[str] = frozenset()) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    for name, draft in section["drafts"].items():
        got = violations(draft, skip)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
    for case in section["invalid_drafts"]:
        got = violations(draft_for(section, case), skip)
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
    for case in section["valid_drafts"]:
        got = violations(draft_for(section, case), skip)
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
    for case in section["streams"]:
        for i, record_ in enumerate(case["records"]):
            got = violations(record_, skip)
            if got:
                problems.append(found("streams.records", f"{case['name']} record {i}: {got}"))
        got = stream_failure(case["records"], skip)
        if got != expected_failure(case):
            problems.append(found("streams.rule", f"{case['name']}: expected {case['expect']}, got {got}"))
    tickers = (["SPY"], ["AAPL", "SPY"], ["SPY", "AAPL"], ["SPY", "SPY"], [], TICKERS_33[:32], TICKERS_33)
    ids = ("a:1", GRAMMAR, "a" * 128, "a" * 129, "", "a b", "bar:1", "bars:1", "quote:x", "instrument:y", "x:bar:1")
    times = ("2026-10-26T14:58:11.999999999Z", PUBLISHED, RECEIVED)
    for symbols, item_id, known in itertools.product(tickers, ids, times):
        draft = copy.deepcopy(section["drafts"][ITEM])
        draft["payload"].update(symbols=list(symbols), item_id=item_id, knowledge_at=known)
        got = [(v.reason, v.path) for v in violations(draft, skip)]
        if got != expected_item(list(symbols), item_id, PUBLISHED, known):
            problems.append(found("items.oracle", f"{symbols} {item_id!r} {known}: rule 283 and §9.37 disagree: {got}"))
    rng = random.Random(section["fuzz_seed"])
    for n in range(200):
        records = [opened()]
        for k in range(rng.randint(1, 6)):
            records.append(item(2 + k, f"a:{rng.randint(1, 4)}", at(rng.randint(0, 4))))
        if stream_failure(records, skip) != expected_stream(records):
            problems.append(found("streams.oracle", f"random stream {n}: rules 284 and 285 and the pairwise reading disagree"))
            break
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "rule.283",
    "rule.283.sorted",
    "rule.283.unique",
    "rule.283.grammar",
    "rule.283.prefix",
    "rule.283.known",
    "boundary.rule_283_none",
    "boundary.rule_283_33",
    "order.rule_283_known_first",
    "rule.286.actor",
    "rule.286",
    "stream.284",
    "stream.285",
    "loose.285.strict",
)


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    """Seeded bugs in the vectors, each registered against the one check that must reject it."""

    def mutated(fn) -> dict:
        out = copy.deepcopy(section)
        fn(out)
        return out

    def case(s, kind, name):
        return next(c for c in s[kind] if c["name"] == name)

    return [
        ("the base item is the runtime's", "drafts.valid", mutated(lambda s: s["drafts"][ITEM].update(actor=dict(AGENT)))),
        ("an invalid draft's path differs", "invalid_drafts",
         mutated(lambda s: case(s, "invalid_drafts", "item_id_bar")["expect"].update(path="payload.symbols"))),
        ("a valid draft holds 33 tickers", "valid_drafts",
         mutated(lambda s: case(s, "valid_drafts", "thirty_two_tickers")["changes"][0].update(value=TICKERS_33))),
        ("a stream record is written by a user", "streams.records",
         mutated(lambda s: case(s, "streams", "two_items")["records"][1].update(actor=dict(USER)))),
        ("a stream case expects the wrong record", "streams.rule",
         mutated(lambda s: case(s, "streams", "stored_again_later")["expect"].update(index=2))),
    ]


def run_mutants(section: dict) -> list[str]:
    """Every seeded bug caught; a vector mutant only by the check it is registered against; and each
    oracle shown to catch a rule it reads directly."""
    escaped = []
    for mutant in VALIDATOR_MUTANTS:
        if not check_section(section, frozenset([mutant])):
            escaped.append(f"news mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not check.endswith(".oracle") and not any(c == check for _, c, _ in registered):
            escaped.append(f"news check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"news vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    for mutant, oracle in (("rule.283.prefix", "items.oracle"), ("loose.285.strict", "streams.oracle")):
        if oracle not in {check_of(p) for p in check_section(section, frozenset([mutant]))}:
            escaped.append(f"news oracle {oracle} misses {mutant}")
    return escaped


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
        "streams": stream_cases(),
        "fuzz_seed": 2278,
    }


__all__ = ["build_section", "check_section", "run_mutants", "stream_failure", "vector_mutants"]
