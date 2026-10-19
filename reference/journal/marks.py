"""Journal spec v0.63 §9.30's reference vectors for `MarkUpdated` version 2 (DEC-2305): the risk
mark's bid, its sanity verdict, and its session, beside version 1, which stays registered.

The schemas and rule 244 live in `control.py`, beside §9.5's other account-stream records, so one
validator judges every closed schema. This module builds the `risk_marks` section: a version-2 and
a version-1 base draft, an invalid draft for every member type, each missing or extra member, and
each clause of rule 244, valid drafts for the marks rule 244 might be misread to refuse and for
every session, the risk fold's reading of each base and valid draft (DEC-2305 item 3), and staleness
sequences in which only version-1 marks follow a held instrument's last sane mark. Its own
oracle reads the drafts' members directly, without the validator, and every seeded bug is shown
caught.
"""

from __future__ import annotations

import copy
import functools
from datetime import UTC, date, datetime, time, timedelta
from decimal import Decimal, InvalidOperation
from zoneinfo import ZoneInfo

from common import change, delete, digest_strings
from control import (
    ACCOUNT_STREAM_REF,
    EXECUTOR,
    MARK_SESSIONS,
    ROOT,
    WORKSPACE,
    check_of,
    draft_for,
    reported,
    violations,
)
from control import (
    invalid as control_invalid,
)
from control import (
    valid as control_valid,
)

SPEC = "docs/specs/journal.md v0.63 §9.30 (DEC-2305)"
ACCOUNT = f"acct:{WORKSPACE}:{ACCOUNT_STREAM_REF}"
AT = "2026-10-19T14:30:00.250000000Z"
RISK_CLOCK = "2026-10-19T14:30:01.000000000Z"
EVENT_IDS = {
    "mark_v2": "01JAB0M0A000000000000000M1",
    "mark_v1": "01JAB0M0A000000000000000M2",
}
V1_PAYLOAD = {
    "instrument_id": "AAPL",
    "price": "187.42",
    "source": "quote",
    "feed": "iex",
}
V2_ADDED = {"bid": "187.42", "sane": True, "session": "regular"}
V2 = "mark_v2"
V1 = "mark_v1"


def mark(name: str) -> dict:
    version = 2 if name == V2 else 1
    payload = {**V1_PAYLOAD, **(V2_ADDED if version == 2 else {}), "risk_clock": RISK_CLOCK}
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": EVENT_IDS[name],
        "stream_id": ACCOUNT,
        "event_type": "MarkUpdated",
        "schema_version": version,
        "event_time": AT,
        "clock_source": "local",
        "causation_id": None,
        "correlation_id": None,
        "actor": dict(EXECUTOR),
        "config_refs": {},
        "payload": payload,
        "artifact_refs": sorted(digest_strings(payload)),
        "pii_refs": [],
    }


def base_drafts() -> dict[str, dict]:
    return {V2: mark(V2), V1: mark(V1)}


def invalid(name, clause, base, changes, reason, path):
    return control_invalid(name, clause, base, changes, reason, path)


def valid(name, clause, base, changes):
    return control_valid(name, clause, base, changes)


# Each member, a value of the wrong JSON kind (`schema`) and, for a string, one of the wrong form
# (`non_canonical`), with `None` where the member's type has no wrong form.
MEMBER_CASES = (
    ("instrument_id", 7, ""),
    ("price", 7, "187,42"),
    ("source", 7, ""),
    ("feed", 7, ""),
    ("bid", 7, "187,42"),
    ("sane", "true", None),
    ("session", 7, "closed"),
    ("risk_clock", 7, "2026-10-19T14:30:01.500000000Z"),
)
V1_MEMBERS = ("instrument_id", "price", "source", "feed", "risk_clock")
NULLED = ("instrument_id", "price", "source", "feed", "sane", "session", "risk_clock")
V2_ONLY = ("bid", "sane", "session")


def member_drafts() -> list[dict]:
    out = []
    for member, wrong_kind, wrong_form in MEMBER_CASES:
        path = f"payload.{member}"
        bases = [V2, V1] if member in V1_MEMBERS else [V2]
        for base in bases:
            tag = "" if base == V2 else ".v1"
            out.append(invalid(f"{member}.kind{tag}", "§9.30 types", base, [change(path, wrong_kind)], "schema", path))
            if wrong_form is not None:
                out.append(
                    invalid(f"{member}.form{tag}", "§9.30 types", base, [change(path, wrong_form)], "non_canonical", path)
                )
    for member in NULLED:
        path = f"payload.{member}"
        out.append(invalid(f"{member}.null", "§9.30 types: only `bid` is nullable", V2, [change(path, None)], "schema", path))
    for member in (*V1_MEMBERS, *V2_ONLY):
        path = f"payload.{member}"
        out.append(invalid(f"{member}.missing", "§9.30 closed: every member", V2, [delete(path)], "schema", path))
    out.append(
        invalid("extra.ask", "§9.30 closed: no member beyond the table", V2, [change("payload.ask", "187.45")], "schema", "payload.ask")
    )
    for member in V2_ONLY:
        path = f"payload.{member}"
        out.append(
            invalid(
                f"v1_with.{member}",
                "§9.30: version 1 is never edited (§8); its members stay its own",
                V1,
                [change(path, V2_ADDED[member])],
                "schema",
                path,
            )
        )
    return out


def rule_drafts() -> list[dict]:
    return [
        invalid("sane_without_bid", "rule 244: a sane mark carries a bid", V2, [change("payload.bid", None)], "schema", "payload.bid"),
        invalid("sane_at_zero", "rule 244: a sane bid is greater than 0", V2, [change("payload.bid", "0")], "schema", "payload.bid"),
        invalid("sane_negative", "rule 244: not a negative bid", V2, [change("payload.bid", "-0.01")], "schema", "payload.bid"),
        invalid(
            "sane_without_bid_after_hours",
            "rule 244: in every session",
            V2,
            [change("payload.session", "after_hours"), change("payload.bid", None)],
            "schema",
            "payload.bid",
        ),
    ]


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule; together they cover every member type and rule 244."""
    return [
        *member_drafts(),
        *rule_drafts(),
        invalid("at_version_3", "§9.30: versions 1 and 2 only", V2, [change("schema_version", 3)], "unknown_schema", "payload"),
    ]


def valid_drafts() -> list[dict]:
    """The marks rule 244 might be misread to refuse, every session, and members no rule reads."""
    return [
        valid("not_sane_without_bid", "rule 244: a quote with no bid is never sane", V2, [change("payload.sane", False), change("payload.bid", None)]),
        valid("not_sane_at_zero", "rule 244: a mark that is not sane may carry any bid", V2, [change("payload.sane", False), change("payload.bid", "0")]),
        valid("not_sane_negative", "rule 244: any bid, if not sane", V2, [change("payload.sane", False), change("payload.bid", "-1")]),
        valid("not_sane_regular", "§9.30: the verdict is the writer's", V2, [change("payload.sane", False)]),
        valid("bid_above_price", "§9.30: no rule ties `bid` to `price`", V2, [change("payload.bid", "187.5")]),
        valid("smallest_bid", "rule 244: greater than 0 is enough", V2, [change("payload.bid", "0.0001")]),
        *(
            valid(f"session.{session}", "§9.30: no rule ties `sane` to `session`", V2, [change("payload.session", session)])
            for session in MARK_SESSIONS
            if session != "regular"
        ),
        valid("v1_another_source", "§9.30: version 1 stays registered as it was", V1, [change("payload.source", "last_trade")]),
        valid("caused", "§9.30: no rule reads the cause", V2, [change("causation_id", EVENT_IDS[V1])]),
    ]


# --------------------------------------------------------------------------- the fold's reading


def reading_of(draft: dict, skip: frozenset[str] = frozenset()) -> dict:
    """DEC-2305 item 3: version 1 is no verdict; version 2 is its verdict, with `bid` only when sane."""
    p = draft["payload"]
    if draft["schema_version"] == 1:
        if "reading.v1_as_sane" in skip:
            return {"reading": "verdict", "sane": True, "session": "regular", "bid": p["price"]}
        return {"reading": "no_verdict"}
    sane = p["sane"] or "reading.always_sane" in skip
    out = {"reading": "verdict", "sane": sane, "session": p["session"]}
    if sane or "reading.bid_when_not_sane" in skip:
        out["bid"] = p["bid"]
    return out


def readings(section: dict) -> list[dict]:
    named = [(name, copy.deepcopy(draft)) for name, draft in section["drafts"].items()]
    named += [(case["name"], draft_for(section, case)) for case in section["valid_drafts"]]
    return [{"draft": name, **reading_of(draft)} for name, draft in named]


# --------------------------------------------------------------------------- staleness under v1 marks

# DEC-2305 item 3 over a sequence: a held instrument whose only marks after its last sane mark are
# version 1 reaches `stale_mark` once its age reaches `mark_max_age_s` (mandate spec §5.2: age >=
# limit, credited before the input is applied). An equity's age counts regular-session seconds of
# the trading calendar (trading spec §2.2, §4.3), which the risk fold's `SessionClock` supplies:
# `mandate-time`'s committed US-equities data, which `regular_seconds` reads from the same file, so
# its weekends, holidays, and early closes cannot drift from the Rust calendar's. Crypto counts all
# time. A sane version-2 mark restarts the count only in a counted session (an equity's regular
# session); a mark that is not sane restricts at once and restarts nothing. Once stale, openings and
# increases in the instrument are denied `stale_mark`; no exit, protective order, or kill switch is
# held by it, and the agent's mode does not change (`AGENTS.md` rule 13; §5.2: a stale mark never
# triggers a flatten). The expectations below are literals written from those sentences;
# `fold_staleness` is the reference reading they check.
NY = ZoneInfo("America/New_York")
CALENDAR = ROOT / "crates/mandate-time/data/us-equities.calendar"
OPENINGS = ("open", "increase")
EXITS = ("discretionary_exit", "risk_exit", "owner_exit", "protective", "kill_switch")
DENIED = "denied_stale_mark"
NOT_HELD = "not_held_by_stale_mark"
FRESH = {
    "stale_mark": False,
    "mode": "normal",
    "orders": {
        "open": NOT_HELD,
        "increase": NOT_HELD,
        "discretionary_exit": NOT_HELD,
        "risk_exit": NOT_HELD,
        "owner_exit": NOT_HELD,
        "protective": NOT_HELD,
        "kill_switch": NOT_HELD,
    },
}
STALE = {
    "stale_mark": True,
    "mode": "normal",
    "orders": {
        "open": DENIED,
        "increase": DENIED,
        "discretionary_exit": NOT_HELD,
        "risk_exit": NOT_HELD,
        "owner_exit": NOT_HELD,
        "protective": NOT_HELD,
        "kill_switch": NOT_HELD,
    },
}


def step(base: str, risk_clock: str, expect: dict, *changes) -> dict:
    return {
        "draft": {"base_draft": base, "changes": [change("payload.risk_clock", risk_clock), *changes]},
        "expect": copy.deepcopy(expect),
    }


def crypto(*changes) -> tuple:
    return (change("payload.instrument_id", "BTC-USD"), *changes)


def equity(name: str, clause: str, limit: int, opened_at: str, steps: list[dict]) -> dict:
    return {
        "name": name,
        "clause": clause,
        "asset_class": "us_equity",
        "calendar": "crates/mandate-time/data/us-equities.calendar",
        "mark_max_age_s": limit,
        "opened_at": opened_at,
        "steps": steps,
    }


def staleness_sequences() -> list[dict]:
    after_hours = change("payload.session", "after_hours")
    not_sane = change("payload.sane", False)
    return [
        equity(
            "equity_v1_after_sane",
            "DEC-2305 item 3: v1 marks after a sane v2 mark never restart the count; stale at the limit, not before",
            60,
            "2026-10-19T14:29:00.000000000Z",
            [
                step(V2, "2026-10-19T14:30:00.000000000Z", FRESH),
                step(V1, "2026-10-19T14:30:30.000000000Z", FRESH),
                step(V1, "2026-10-19T14:30:59.000000000Z", FRESH),
                step(V1, "2026-10-19T14:31:00.000000000Z", STALE),
                step(V1, "2026-10-19T14:31:30.000000000Z", STALE),
                step(V2, "2026-10-19T14:31:40.000000000Z", FRESH),
            ],
        ),
        equity(
            "equity_not_sane_in_the_regular_session",
            "§5.2: a regular-session mark that is not sane restricts at once and restarts nothing; a sane one clears",
            60,
            "2026-10-19T14:29:00.000000000Z",
            [
                step(V2, "2026-10-19T14:30:00.000000000Z", FRESH),
                step(V2, "2026-10-19T14:30:20.000000000Z", STALE, not_sane),
                step(V1, "2026-10-19T14:30:50.000000000Z", STALE),
                step(V2, "2026-10-19T14:31:00.000000000Z", FRESH),
            ],
        ),
        equity(
            "equity_only_v1_across_the_close",
            "DEC-2305 item 3: only v1 marks since opening; an equity's age counts regular-session seconds only",
            120,
            "2026-10-19T19:59:00.000000000Z",
            [
                step(V1, "2026-10-19T19:59:30.000000000Z", FRESH),
                step(V1, "2026-10-19T20:30:00.000000000Z", FRESH, change("payload.price", "187.10")),
                step(V1, "2026-10-20T13:30:00.000000000Z", FRESH),
                step(V1, "2026-10-20T13:30:59.000000000Z", FRESH),
                step(V1, "2026-10-20T13:31:00.000000000Z", STALE),
            ],
        ),
        equity(
            "equity_friday_to_monday",
            "the calendar: a weekend counts no regular seconds; a sane after-hours v2 mark restarts nothing",
            120,
            "2026-10-23T19:59:00.000000000Z",
            [
                step(V1, "2026-10-23T19:59:30.000000000Z", FRESH),
                step(V2, "2026-10-23T20:30:00.000000000Z", FRESH, after_hours),
                step(V1, "2026-10-24T15:00:00.000000000Z", FRESH),
                step(V1, "2026-10-25T15:00:00.000000000Z", FRESH),
                step(V1, "2026-10-26T13:30:59.000000000Z", FRESH),
                step(V1, "2026-10-26T13:31:00.000000000Z", STALE),
            ],
        ),
        equity(
            "equity_across_thanksgiving",
            "the calendar: `closed 2026-11-26 Thanksgiving Day` counts no regular seconds",
            120,
            "2026-11-25T20:59:00.000000000Z",
            [
                step(V1, "2026-11-25T20:59:30.000000000Z", FRESH),
                step(V1, "2026-11-26T16:00:00.000000000Z", FRESH),
                step(V1, "2026-11-27T14:30:59.000000000Z", FRESH),
                step(V1, "2026-11-27T14:31:00.000000000Z", STALE),
            ],
        ),
        equity(
            "equity_across_an_early_close",
            "the calendar: `early_close 2026-11-27 13:00` ends the regular session at 13:00 New York time",
            120,
            "2026-11-27T17:59:00.000000000Z",
            [
                step(V1, "2026-11-27T17:59:30.000000000Z", FRESH),
                step(V1, "2026-11-27T18:30:00.000000000Z", FRESH),
                step(V1, "2026-11-30T14:30:59.000000000Z", FRESH),
                step(V1, "2026-11-30T14:31:00.000000000Z", STALE),
            ],
        ),
        {
            "name": "crypto_only_v1",
            "clause": "DEC-2305 item 3: crypto's age counts all time, a weekend included",
            "asset_class": "crypto",
            "calendar": None,
            "mark_max_age_s": 60,
            "opened_at": "2026-10-24T00:00:00.000000000Z",
            "steps": [
                step(V1, "2026-10-24T00:00:59.000000000Z", FRESH, *crypto()),
                step(V1, "2026-10-24T00:01:00.000000000Z", STALE, *crypto()),
                step(V2, "2026-10-24T00:01:30.000000000Z", FRESH, *crypto(change("payload.session", "crypto"))),
            ],
        },
    ]


def instant(text: str) -> datetime:
    return datetime.strptime(text[:19], "%Y-%m-%dT%H:%M:%S").replace(tzinfo=UTC)


def wall(text: str) -> time:
    hour, minute = text.split(":")
    return time(int(hour), int(minute))


@functools.cache
def exchange_calendar() -> dict:
    """`mandate-time`'s US-equities calendar, read as its header describes: the valid range, the
    regular open and close of a full day, the closed weekdays, and each early close's regular close."""
    out: dict = {"closed": set(), "early_close": {}}
    for line in CALENDAR.read_text(encoding="utf-8").split("\n"):
        if not line or line.startswith("#"):
            continue
        record, *fields = line.split(" ")
        if record == "valid":
            out["valid"] = (date.fromisoformat(fields[0]), date.fromisoformat(fields[1]))
        elif record == "hours":
            out["open"], out["close"] = wall(fields[1]), wall(fields[2])
        elif record == "closed":
            out["closed"].add(date.fromisoformat(fields[0]))
        elif record == "early_close":
            out["early_close"][date.fromisoformat(fields[0])] = wall(fields[1])
        else:
            raise ValueError(f"{CALENDAR}: unknown record {record}")
    return out


def regular_seconds(t0: datetime, t1: datetime, skip: frozenset[str] = frozenset()) -> int:
    """Whole seconds of the calendar's regular sessions in [t0, t1)."""
    cal = exchange_calendar()
    total = 0
    day = t0.astimezone(NY).date()
    while day <= t1.astimezone(NY).date():
        if not cal["valid"][0] <= day <= cal["valid"][1]:
            raise ValueError(f"{day} is outside {CALENDAR}'s valid range")
        weekend = day.weekday() >= 5 and "fold.weekend_counted" not in skip
        holiday = day in cal["closed"] and "fold.holiday_counted" not in skip
        if not (weekend or holiday):
            closes = cal["close"] if "fold.early_close_ignored" in skip else cal["early_close"].get(day, cal["close"])
            lo = max(datetime.combine(day, cal["open"], tzinfo=NY), t0)
            hi = min(datetime.combine(day, closes, tzinfo=NY), t1)
            if hi > lo:
                total += int((hi - lo).total_seconds())
        day += timedelta(days=1)
    return total


def order_outcomes(stale: bool, skip: frozenset[str]) -> dict:
    """What `stale_mark` does to each order path: it denies openings and increases, and holds nothing else."""
    orders = {}
    for purpose in OPENINGS:
        orders[purpose] = DENIED if stale and "fold.stale_allows_openings" not in skip else NOT_HELD
    for purpose in EXITS:
        held = stale and ("fold.stale_holds_exits" in skip or f"fold.stale_holds_{purpose}" in skip)
        orders[purpose] = DENIED if held else NOT_HELD
    paused = stale and "fold.stale_pauses" in skip
    return {"mode": "paused" if paused else "normal", "orders": orders}


def fold_staleness(section: dict, sequence: dict, skip: frozenset[str] = frozenset()) -> list[dict]:
    """The reference reading of one sequence: the restriction, mode, and order outcomes after each step."""
    crypto_clock = sequence["asset_class"] == "crypto" or "fold.equity_all_hours" in skip
    limit = sequence["mark_max_age_s"]
    age, stale, at = 0, False, instant(sequence["opened_at"])
    out = []
    for item in sequence["steps"]:
        draft = draft_for(section, item["draft"])
        p = draft["payload"]
        now = instant(p["risk_clock"])
        age += int((now - at).total_seconds()) if crypto_clock else regular_seconds(at, now, skip)
        at = now
        reached = age > limit if "fold.strict_boundary" in skip else age >= limit
        if reached:
            stale = True
        reading = reading_of(draft)
        if reading["reading"] == "no_verdict":
            if "fold.v1_restarts_count" in skip:
                age = 0
            if "fold.v1_as_not_sane" in skip:
                stale = True
        counted = (
            sequence["asset_class"] == "crypto"
            or reading.get("session") == "regular"
            or "fold.after_hours_restarts" in skip
        )
        if reading["reading"] == "verdict" and counted:
            if reading["sane"] or "fold.not_sane_restarts" in skip:
                age, stale = 0, False
            else:
                stale = True
        out.append({"stale_mark": stale, **order_outcomes(stale, skip)})
    return out


# --------------------------------------------------------------------------- independent oracle

ORACLE_CHECKS = (
    "drafts.valid",
    "drafts.mark",
    "invalid_drafts",
    "invalid_drafts.oracle",
    "valid_drafts",
    "valid_drafts.oracle",
    "readings",
    "staleness.records",
    "staleness",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def oracle_244(draft: dict) -> bool:
    """Rule 244 counted apart from the validator: a version-2 mark whose `sane` is the JSON `true`
    holds a bid above zero. A bid missing, of another JSON kind, or not a number is its type's
    refusal, not the rule's."""
    p = draft["payload"]
    if draft["schema_version"] != 2 or p.get("sane") is not True or "bid" not in p:
        return True
    bid = p["bid"]
    if bid is None:
        return False
    if not isinstance(bid, str):
        return True
    try:
        number = Decimal(bid)
    except InvalidOperation:
        return True
    return number.compare(Decimal(0)) == 1


def oracle_reading(name: str, draft: dict) -> dict:
    """The reading by the member names alone: no verdict without `sane`, the bid only beside true."""
    p = draft["payload"]
    if "sane" not in p:
        return {"draft": name, "reading": "no_verdict"}
    entry = {"draft": name, "reading": "verdict", "sane": p["sane"], "session": p["session"]}
    if p["sane"] is True:
        entry["bid"] = p["bid"]
    return entry


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    for name, draft in section["drafts"].items():
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
    v2, v1 = section["drafts"][V2], section["drafts"][V1]
    v2_ok = v2["schema_version"] == 2 and v2["payload"].get("sane") is True and v2["payload"].get("session") == "regular"
    v1_ok = v1["schema_version"] == 1 and not any(m in v1["payload"] for m in V2_ONLY)
    if not (v2_ok and v1_ok):
        problems.append(found("drafts.mark", "the base drafts are not a sane regular-session version 2 and a plain version 1"))
    for case in section["invalid_drafts"]:
        draft = draft_for(section, case)
        got = violations(draft)
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
        names_244 = case["clause"].startswith("rule 244")
        if names_244 == oracle_244(draft):
            problems.append(found("invalid_drafts.oracle", f"{case['name']}: the oracle disagrees on rule 244"))
    for case in section["valid_drafts"]:
        draft = draft_for(section, case)
        got = violations(draft)
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
        if not oracle_244(draft):
            problems.append(found("valid_drafts.oracle", f"{case['name']}: the oracle refuses it by rule 244"))
    named = [(name, draft) for name, draft in section["drafts"].items()]
    named += [(case["name"], draft_for(section, case)) for case in section["valid_drafts"]]
    want = [oracle_reading(name, draft) for name, draft in named]
    if section["readings"] != want:
        problems.append(found("readings", f"expected {want}, got {section['readings']}"))
    for sequence in section["staleness"]:
        for i, item in enumerate(sequence["steps"]):
            got = violations(draft_for(section, item["draft"]))
            if got:
                problems.append(found("staleness.records", f"{sequence['name']} step {i}: {got}"))
        expected = [item["expect"] for item in sequence["steps"]]
        folded = fold_staleness(section, sequence)
        if folded != expected:
            problems.append(found("staleness", f"{sequence['name']}: expected {expected}, folded {folded}"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "record.extra",
    "record.missing",
    *(f"loose.payload.{member}" for member, _, _ in MEMBER_CASES),
    *(f"nullable.payload.{member}" for member in NULLED),
    "rule.244",
    "rule.244.null",
    "boundary.rule_244_zero",
    "versions.mark_updated_v1",
    "versions.mark_updated_v2",
)
READING_MUTANTS = ("reading.v1_as_sane", "reading.always_sane", "reading.bid_when_not_sane")
FOLD_MUTANTS = (
    "fold.strict_boundary",
    "fold.equity_all_hours",
    "fold.weekend_counted",
    "fold.holiday_counted",
    "fold.early_close_ignored",
    "fold.after_hours_restarts",
    "fold.not_sane_restarts",
    "fold.stale_pauses",
    *(f"fold.stale_holds_{purpose}" for purpose in EXITS),
    "fold.v1_restarts_count",
    "fold.v1_as_not_sane",
    "fold.stale_allows_openings",
    "fold.stale_holds_exits",
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
        ("the base draft is at version 3", "drafts.valid", mutated(lambda s: s["drafts"][V2].update(schema_version=3))),
        ("the base draft is not sane", "drafts.mark", mutated(lambda s: s["drafts"][V2]["payload"].update(sane=False))),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "sane_at_zero")["expect"].update(path="payload.price")),
        ),
        (
            "a rule-244 draft is relabelled as a type case",
            "invalid_drafts.oracle",
            mutated(lambda s: case(s, "invalid_drafts", "sane_negative").update(clause="§9.30 types")),
        ),
        (
            "a valid draft's sane mark has a zero bid",
            "valid_drafts",
            mutated(lambda s: case(s, "valid_drafts", "smallest_bid")["changes"].append(change("payload.bid", "0"))),
        ),
        (
            "a valid draft's sane mark has no bid",
            "valid_drafts.oracle",
            mutated(lambda s: case(s, "valid_drafts", "bid_above_price")["changes"].append(change("payload.bid", None))),
        ),
        (
            "a staleness step's record is at version 3",
            "staleness.records",
            mutated(lambda s: s["staleness"][0]["steps"][0]["draft"]["changes"].append(change("schema_version", 3))),
        ),
        (
            "a staleness step expects the instrument fresh at the limit",
            "staleness",
            mutated(lambda s: s["staleness"][0]["steps"][3].update(expect=copy.deepcopy(FRESH))),
        ),
        (
            "a reading takes version 1's price as a sane bid",
            "readings",
            mutated(lambda s: s["readings"][1].update(reading="verdict", sane=True, bid="187.42")),
        ),
    ]


def run_mutants(section: dict) -> list[str]:
    """Every seeded bug caught; a vector mutant only by the check it is registered against."""
    escaped = []
    cases = [(copy.deepcopy(d), None) for d in section["drafts"].values()]
    cases += [(draft_for(section, c), None) for c in section["valid_drafts"]]
    cases += [(draft_for(section, c), c["expect"]) for c in section["invalid_drafts"]]
    for mutant in VALIDATOR_MUTANTS:
        skip = frozenset([mutant])
        caught = False
        for draft, want in cases:
            got = violations(draft, skip)
            caught |= bool(got) if want is None else not reported(got, want)
        if not caught:
            escaped.append(f"mark validator mutant {mutant}")
    for mutant in READING_MUTANTS:
        skip = frozenset([mutant])
        named = [(name, copy.deepcopy(d)) for name, d in section["drafts"].items()]
        named += [(c["name"], draft_for(section, c)) for c in section["valid_drafts"]]
        got = [{"draft": name, **reading_of(draft, skip)} for name, draft in named]
        if got == section["readings"]:
            escaped.append(f"mark reading mutant {mutant}")
    for mutant in FOLD_MUTANTS:
        skip = frozenset([mutant])
        expected = [[item["expect"] for item in q["steps"]] for q in section["staleness"]]
        if [fold_staleness(section, q, skip) for q in section["staleness"]] == expected:
            escaped.append(f"mark fold mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"mark check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"mark vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    section = {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
    }
    section["readings"] = readings(section)
    section["staleness"] = staleness_sequences()
    return section
