"""Journal spec v0.79 §9.34's reference vectors for `RetirementHeld` (DEC-2352): the runtime's
record that an agent's retirement has waited another 24 hours of risk clock for what the agent
still holds (DEC-2344 item 2), each in a batch with its `OwnerAlertSent` (`retirement_held`).

The schema and rule 265 live in `control.py`, beside §9.7's other agent-stream records, so one
validator judges every closed schema. This module builds the `retirements` section: a base draft,
an invalid draft for every member type and rule, valid drafts for the cases a rule might be misread
to refuse, and agent streams for stream rule 266. Its own oracle reads each stream's held periods
per cause without the stream fold, and every seeded bug is shown caught.
"""

from __future__ import annotations

import copy
from decimal import Decimal

from common import change, delete, digest_strings, epoch_seconds
from control import (
    AGENT_STREAM,
    RUNTIME,
    STREAM,
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

SPEC = "docs/specs/journal.md v0.79 §9.34 (DEC-2352)"
BOUND = "sha256:" + "1" * 64
NEWER = "sha256:" + "2" * 64
STOP = "01J8Z5RH0A000000000000000S"
LIMIT = "01J8Z5RH0B000000000000000M"
HELD = "01J8Z5RH10000000000000000H"
SINCE = "2026-09-21T15:00:00.000000000Z"
DAY = 86_400
BASE = "held"


def clock(seconds: int) -> str:
    """The risk clock `seconds` after `SINCE`, whole seconds as §9.2's `risk_clock`."""
    hours, rest = divmod(15 * 3600 + seconds, 3600)
    days, hour = divmod(hours, 24)
    minute, second = divmod(rest, 60)
    return f"2026-09-{21 + days:02d}T{hour:02d}:{minute:02d}:{second:02d}.000000000Z"


def held(
    period: int,
    *,
    cause: str = STOP,
    since: str = SINCE,
    after: int | None = None,
    waiting: tuple[str, ...] = ("position",),
    event_id: str | None = None,
    version: str = BOUND,
) -> dict:
    payload = {
        "held_since": since,
        "period": period,
        "waiting_on": list(waiting),
        "risk_clock": clock(DAY * period if after is None else after),
    }
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": event_id or f"01J8Z5RH2{period:01d}{cause[-1]}".ljust(26, "0"),
        "stream_id": AGENT_STREAM,
        "event_type": "RetirementHeld",
        "schema_version": 1,
        "event_time": payload["risk_clock"],
        "clock_source": "scheduler",
        "causation_id": cause,
        "correlation_id": None,
        "actor": dict(RUNTIME),
        "config_refs": {"mandate_version": version},
        "payload": payload,
        "artifact_refs": sorted(digest_strings(payload)),
        "pii_refs": [],
    }


def base_drafts() -> dict[str, dict]:
    return {BASE: held(1, event_id=HELD)}


def invalid(name, clause, changes, reason, path):
    return control_invalid(name, clause, BASE, changes, reason, path)


def valid(name, clause, changes):
    return control_valid(name, clause, BASE, changes)


# Each member, a value of the wrong JSON kind (`schema`) and one of the wrong form (`non_canonical`
# for a string, `schema` for a list item outside its enumeration).
MEMBER_CASES = (
    ("held_since", 7, "2026-09-21T15:00:00.5Z", "non_canonical"),
    ("period", "1", None, None),
    ("waiting_on", "position", ["orders"], "non_canonical"),
    ("risk_clock", 7, "2026-09-22T15:00:00Z", "non_canonical"),
)
NULLED = ("held_since", "period", "waiting_on", "risk_clock")


def member_drafts() -> list[dict]:
    out = []
    for member, wrong_kind, wrong_form, form_reason in MEMBER_CASES:
        path = f"payload.{member}"
        out.append(invalid(f"{member}.kind", "§9.34 types", [change(path, wrong_kind)], "schema", path))
        if wrong_form is not None:
            form_path = f"{path}[0]" if isinstance(wrong_form, list) else path
            out.append(invalid(f"{member}.form", "§9.34 types", [change(path, wrong_form)], form_reason, form_path))
    for member in NULLED:
        path = f"payload.{member}"
        out.append(invalid(f"{member}.null", "§9.34 types", [change(path, None)], "schema", path))
    out.append(invalid("missing", "§9.34 closed", [delete("payload.period")], "schema", "payload.period"))
    out.append(invalid("extra", "§9.34 closed", [change("payload.qty", "10")], "schema", "payload.qty"))
    return out


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule; together they cover every member type and rule 265."""
    return [
        *member_drafts(),
        invalid(
            "by_a_user",
            "rule 265: the runtime's own record",
            [change("actor.kind", "user")],
            "schema",
            "actor.kind",
        ),
        invalid(
            "by_the_system",
            "rule 265: the runtime's own record, not a workspace service's",
            [change("actor.kind", "system")],
            "schema",
            "actor.kind",
        ),
        invalid("uncaused", "rule 265: caused by what began the retirement", [change("causation_id", None)], "schema", "causation_id"),
        invalid("period_zero", "rule 265: periods count from 1", [change("payload.period", 0)], "schema", "payload.period"),
        invalid(
            "too_soon",
            "rule 265: no sooner than 24 hours after it began",
            [change("payload.risk_clock", clock(DAY - 1)), change("event_time", clock(DAY - 1))],
            "schema",
            "payload.risk_clock",
        ),
        invalid("waiting_on_nothing", "rule 265: it waits on something", [change("payload.waiting_on", [])], "schema", "payload.waiting_on"),
        invalid(
            "waiting_on_twice",
            "rule 265: each thing once",
            [change("payload.waiting_on", ["position", "position"])],
            "schema",
            "payload.waiting_on",
        ),
        invalid(
            "waiting_on_out_of_order",
            "rule 265: in order",
            [change("payload.waiting_on", ["working_order", "position"])],
            "schema",
            "payload.waiting_on",
        ),
        invalid(
            "without_its_mandate",
            "§9 row: `RetirementHeld` names `mandate_version`",
            [change("config_refs", {})],
            "missing_config_ref",
            "config_refs.mandate_version",
        ),
        invalid(
            "on_the_control_stream",
            "§9 row: `RetirementHeld` is an agent-stream record",
            [change("stream_id", STREAM)],
            "wrong_stream",
            "event_type",
        ),
        invalid("at_version_2", "§9.34: closed at schema version 1 only", [change("schema_version", 2)], "unknown_schema", "payload"),
    ]


def valid_drafts() -> list[dict]:
    """Cases a rule might be misread to refuse; each must be accepted."""
    return [
        valid("exactly_due", "rule 265: at the 24-hour mark itself", []),
        valid("late", "rule 265: a late period is fine; no sooner, not no later", [change("payload.risk_clock", clock(DAY + 3600))]),
        valid(
            "both",
            "rule 265: a position and a working order",
            [change("payload.waiting_on", ["position", "working_order"])],
        ),
        valid("working_order_alone", "rule 265: a working order alone", [change("payload.waiting_on", ["working_order"])]),
        valid(
            "third_period",
            "rule 265: period 3 at 72 hours",
            [change("payload.period", 3), change("payload.risk_clock", clock(3 * DAY))],
        ),
    ]


# --------------------------------------------------------------------------- stream rule 266

AGENT_ID = AGENT_STREAM.split(":")[-1]
OTHER_AGENT = "agent_other"
ENDING_COMMANDS = ("resume", "stop")


def alert_for(record: dict, index: int) -> dict:
    """The record's `OwnerAlertSent` (`retirement_held`), the next row of its batch."""
    return {
        "event_id": f"01J8Z5RHA{index:02d}".ljust(26, "0"),
        "event_type": "OwnerAlertSent",
        "causation_id": record["event_id"],
        "config_refs": {},
        "payload": {"subject": record["event_id"], "kind": "retirement_held", "owner_command": None},
    }


def with_alert(record: dict, index: int) -> list[dict]:
    record = copy.deepcopy(record)
    record["event_id"] = f"01J8Z5RH9{index:02d}".ljust(26, "0")
    return [record, alert_for(record, index)]


def ended(kind: str, index: int, agent: str = AGENT_ID, command: str = "resume", result: str = "applied") -> dict:
    """A record the verifier reads beside the agent stream: an `AgentStopped` or an owner's command
    (control stream), or a mandate version applied (account stream)."""
    payload = {
        "AgentStopped": {"agent_id": agent},
        "OwnerCommandIssued": {"command": command, "scope": "agent", "subject": agent},
        "MandateVersionApplied": {"agent_id": agent, "new_version": NEWER, "result": result},
    }[kind]
    return {
        "event_id": f"01J8Z5RHX{index:02d}".ljust(26, "0"),
        "event_type": kind,
        "causation_id": None,
        "config_refs": {},
        "payload": payload,
    }


def workspace_resume(index: int) -> dict:
    """An owner's resume of the whole workspace, which names no agent and so ends no sequence."""
    record = ended("OwnerCommandIssued", index)
    record["payload"] = {"command": "resume", "scope": "workspace", "subject": AGENT_ID}
    return record


def stream_failure(records: list[dict], skip: frozenset[str] = frozenset()) -> int | None:
    """Rule 266 folded once, over the agent stream with the records that end or move a retirement,
    in commit order. One sequence is open at a time; its records run 1, 2, ... under one cause and
    `held_since`, each at least 24 hours of risk clock after the one before, under the mandate
    version in effect; this agent's `AgentStopped` or an owner's resume or stop naming it ends it,
    and no record of its cause follows; a version applied to it lets the next record either go on
    under the new version or begin a new cause from 1. Each record and its `OwnerAlertSent` are
    adjacent. Returns the first index that breaks it, or `None`."""
    open_: dict | None = None
    finished: set[str] = set()
    own = "stream.266.own" in skip
    for i, record in enumerate(records):
        t, p = record["event_type"], record["payload"]
        if t == "AgentStopped" or t == "OwnerCommandIssued":
            mine = own or (
                p.get("agent_id") == AGENT_ID
                if t == "AgentStopped"
                else p["subject"] == AGENT_ID and p["scope"] == "agent" and p["command"] in ENDING_COMMANDS
            )
            if mine and open_ is not None and "stream.266.end" not in skip:
                finished.add(open_["cause"])
                open_ = None
            continue
        if t == "MandateVersionApplied":
            if (own or (p["agent_id"] == AGENT_ID and p["result"] == "applied")) and open_ is not None:
                open_["moved_to"] = p["new_version"]
            continue
        if t == "OwnerAlertSent" and p["kind"] == "retirement_held":
            before = records[i - 1] if i > 0 else None
            tied = before is not None and before["event_type"] == "RetirementHeld" and before["event_id"] == p["subject"]
            if not tied and "stream.266.subject" not in skip:
                return i
            continue
        if t != "RetirementHeld":
            continue
        after = records[i + 1] if i + 1 < len(records) else None
        alerted = after is not None and after["event_type"] == "OwnerAlertSent" and after["payload"]["subject"] == record["event_id"]
        if not alerted and "stream.266.alert" not in skip:
            return i
        cause, version, now = record["causation_id"], record["config_refs"]["mandate_version"], epoch_seconds(p["risk_clock"])
        if open_ is not None and cause != open_["cause"]:
            if open_.get("moved_to") is None and "stream.266.one" not in skip:
                return i
            finished.add(open_["cause"])
            open_ = None
        if open_ is None:
            if cause in finished and "stream.266.finished" not in skip:
                return i
            if p["period"] != 1 and "stream.266.period" not in skip:
                return i
            open_ = {"cause": cause, "period": p["period"], "since": p["held_since"], "version": version, "at": now}
            continue
        if p["period"] != open_["period"] + 1 and "stream.266.period" not in skip:
            return i
        if p["held_since"] != open_["since"] and "stream.266.since" not in skip:
            return i
        if version not in (open_["version"], open_.get("moved_to")) and "stream.266.version" not in skip:
            return i
        if now < open_["at"] + DAY and "stream.266.spacing" not in skip:
            return i
        open_ = {"cause": cause, "period": p["period"], "since": open_["since"], "version": version, "at": now}
    return None


def other(index: int) -> dict:
    """Another agent-stream record between the held periods, which rule 266 does not read."""
    return {
        "event_id": f"01J8Z5RH3{index:01d}".ljust(26, "0"),
        "event_type": "ApprovalTimedOut",
        "causation_id": None,
        "config_refs": {"mandate_version": BOUND},
        "payload": {},
    }


def stream_cases() -> list[dict]:
    def case(name, clause, parts, index):
        records = [r for part in parts for r in (part if isinstance(part, list) else [part])]
        expect = {"outcome": "Valid"} if index is None else {"outcome": "Invalid", "rule": "rule 266", "index": index}
        return {"name": name, "clause": clause, "records": records, "expect": expect}

    n = iter(range(1, 99))

    def h(period, **kw):
        return with_alert(held(period, **kw), next(n))

    bare = held(1, event_id="01J8Z5RH8000000000000000B1")
    newer = {"version": NEWER}
    return [
        case("one_period", "rule 266: the first period is 1", [h(1)], None),
        case("three_periods", "rule 266: each next period is one more", [h(1), h(2), h(3)], None),
        case("between_others", "rule 266: other records between periods are not read", [h(1), other(1), h(2)], None),
        case(
            "after_downtime",
            "rule 266: a runtime back after days down writes the next period, a day after the last",
            [h(1), h(2, after=5 * DAY), h(3, after=6 * DAY)],
            None,
        ),
        case(
            "drains_after_downtime",
            "rule 266: each record at least 24 hours after the one before, so missed periods never drain",
            [h(1), h(2, after=5 * DAY), h(3, after=5 * DAY + 60)],
            4,
        ),
        case(
            "what_it_waits_on_changes",
            "rule 266: what it waits on may change between periods",
            [h(1, waiting=("position", "working_order")), h(2, waiting=("position",))],
            None,
        ),
        case("a_new_cause_after_a_stop", "rule 266: after its end, a later retirement is a new sequence from 1", [h(1), ended("AgentStopped", 1), h(1, cause=LIMIT)], None),
        case("a_new_cause_after_a_resume", "rule 266: a resume ends the sequence", [h(1), ended("OwnerCommandIssued", 2), h(1, cause=LIMIT)], None),
        case(
            "a_new_cause_after_an_owner_stop",
            "rule 266: an owner's stop naming the agent ends the sequence",
            [h(1), ended("OwnerCommandIssued", 3, command="stop"), h(1, cause=LIMIT)],
            None,
        ),
        case(
            "still_held_under_a_newer_version",
            "rule 266: a version that leaves it retiring moves the sequence on, unbroken",
            [h(1), ended("MandateVersionApplied", 4), h(2, **newer), h(3, **newer)],
            None,
        ),
        case(
            "retired_again_under_the_newer_version",
            "rule 266: a retirement a newer version begins is a new cause, from 1",
            [h(1), ended("MandateVersionApplied", 5),
             h(1, cause=LIMIT, since=clock(3600), after=DAY + 3600, **newer),
             h(2, cause=LIMIT, since=clock(3600), after=2 * DAY + 3600, **newer)],
            None,
        ),
        case("starts_at_two", "rule 266: the first period is 1", [h(2)], 0),
        case("skips_a_period", "rule 266: each next period is one more", [h(1), h(3)], 2),
        case("repeats_a_period", "rule 266: each period once", [h(1), h(2), h(2, after=3 * DAY)], 4),
        case("another_since", "rule 266: one retirement began once", [h(1), h(2, since=clock(3600), after=2 * DAY + 3600)], 2),
        case("under_a_newer_version", "rule 266: a newer version with none applied is no continuation", [h(1), h(2, **newer)], 2),
        case(
            "under_a_rejected_version",
            "rule 266: a rejected version moves nothing",
            [h(1), ended("MandateVersionApplied", 6, result="rejected"), h(2, **newer)],
            3,
        ),
        case("a_second_cause_while_one_holds", "rule 266: one sequence per agent; a second cause starts nothing", [h(1), h(1, cause=LIMIT)], 2),
        case("a_second_cause_continuing_the_count", "rule 266: a record of another cause is no continuation", [h(1), h(2, cause=LIMIT)], 2),
        case("after_its_stop", "rule 266: none after the agent's AgentStopped", [h(1), ended("AgentStopped", 7), h(2)], 3),
        case("after_a_resume", "rule 266: none after a resume", [h(1), ended("OwnerCommandIssued", 8), h(1)], 3),
        case(
            "another_agents_stop",
            "rule 266: only this agent's own AgentStopped ends it",
            [h(1), ended("AgentStopped", 9, agent=OTHER_AGENT), h(1, cause=LIMIT)],
            3,
        ),
        case(
            "another_agents_resume",
            "rule 266: only a command naming this agent ends it",
            [h(1), ended("OwnerCommandIssued", 10, agent=OTHER_AGENT), h(1, cause=LIMIT)],
            3,
        ),
        case(
            "a_workspace_resume",
            "rule 266: only a command scoped to this agent ends it",
            [h(1), workspace_resume(13), h(1, cause=LIMIT)],
            3,
        ),
        case(
            "a_pause_naming_it",
            "rule 266: only a resume or a stop ends it",
            [h(1), ended("OwnerCommandIssued", 11, command="pause"), h(1, cause=LIMIT)],
            3,
        ),
        case(
            "another_agents_version",
            "rule 266: only a version applied to this agent moves it",
            [h(1), ended("MandateVersionApplied", 12, agent=OTHER_AGENT), h(1, cause=LIMIT)],
            3,
        ),
        case("without_its_alert", "rule 266: each record with its alert in the batch", [bare, other(2)], 0),
        case("an_alert_without_its_record", "rule 266: a `retirement_held` alert names the record just before it", [h(1), other(3), alert_for(bare, 90)], 3),
    ]


# --------------------------------------------------------------------------- the silent finding


def cancels(cause: dict, version: dict, skip: frozenset[str] = frozenset()) -> bool:
    """Whether a version applied cancels the retirement its cause began, from the cause record's
    and the new version's own fields alone: a `profit_stop` whose new `profit_level` is above the
    breach value the cause recorded, a passed `end_date` moved past the cause's date, or an
    `on_complete` changed from the one that retired it. The agent's current return is never read,
    so a dip never cancels a retirement whose exits are unfilled."""
    fields = version["payload"]["mandate"]
    if cause["kind"] == "profit_stop":
        raised = Decimal(fields["profit_level"]) > Decimal(cause["breach_value"])
        return raised or "silence.profit" in skip
    if cause["kind"] == "end_date":
        return fields["end_date"] > cause["date"]
    return fields["on_complete"] != cause["on_complete"]


def silent(cause: dict, records: list[dict], now: str, skip: frozenset[str] = frozenset()) -> bool:
    """The verifier's `retirement_held_silent` finding at risk clock `now`: the sequence's last
    `RetirementHeld` is more than 48 hours old, with no end and no version that cancels it since."""
    last = None
    for record in records:
        t = record["event_type"]
        if t == "RetirementHeld":
            last = epoch_seconds(record["payload"]["risk_clock"])
        elif last is not None and t in ("AgentStopped", "OwnerCommandIssued"):
            last = None
        elif last is not None and t == "MandateVersionApplied" and "silence.cancel" not in skip:
            if cancels(cause, record, skip):
                last = None
    return last is not None and epoch_seconds(now) - last > 2 * DAY


def version(level: str = "0.10", end_date: str = "2026-09-21", on_complete: str = "release") -> dict:
    return {
        "event_type": "MandateVersionApplied",
        "payload": {"agent_id": AGENT_ID, "new_version": NEWER, "result": "applied",
                    "mandate": {"profit_level": level, "end_date": end_date, "on_complete": on_complete}},
    }


def silence_cases() -> list[dict]:
    stop = {"kind": "profit_stop", "breach_value": "0.12"}
    end = {"kind": "end_date", "date": "2026-09-21"}
    goal = {"kind": "on_complete", "on_complete": "release"}
    first = held(1)

    def case(name, clause, cause, records, now, is_silent):
        return {"name": name, "clause": clause, "cause": cause, "records": records, "now": now, "silent": is_silent}

    return [
        case("held_and_writing", "§9.34: a record within 48 hours is no silence", stop, [first], clock(2 * DAY), False),
        case("silent", "§9.34: more than 48 hours with no record and no end", stop, [first], clock(3 * DAY + 1), True),
        case("ended", "§9.34: an end is no silence", stop, [first, ended("AgentStopped", 20)], clock(5 * DAY), False),
        case(
            "cancelled_by_a_raised_level",
            "§9.34: a version whose profit_level is above the breach value cancels it",
            stop, [first, version(level="0.15")], clock(5 * DAY), False,
        ),
        case(
            "a_level_still_breached",
            "§9.34: a level at or below the breach value cancels nothing, whatever the return since",
            stop, [first, version(level="0.12")], clock(5 * DAY), True,
        ),
        case(
            "cancelled_by_a_later_end_date",
            "§9.34: an end_date moved past the cause's cancels it",
            end, [first, version(end_date="2026-10-21")], clock(5 * DAY), False,
        ),
        case("the_same_end_date", "§9.34: the same end_date cancels nothing", end, [first, version()], clock(5 * DAY), True),
        case(
            "cancelled_by_on_complete",
            "§9.34: an on_complete changed from the one that retired it cancels it",
            goal, [first, version(on_complete="stop")], clock(5 * DAY), False,
        ),
    ]


# --------------------------------------------------------------------------- independent oracle

ORACLE_CHECKS = ("drafts.valid", "invalid_drafts", "valid_drafts", "streams.rule", "streams.oracle", "silence")


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def oracle(records: list[dict]) -> int | None:
    """Rule 266 read another way. Pairs records with alerts by position first. Then it reads the
    retirement's ends itself, without the fold's helper: this agent's `AgentStopped`, and an owner's
    `resume` or `stop` scoped to this agent. It cuts the stream at each end, and within a segment
    walks consecutive held records: a record of a new cause is allowed only first in its segment or
    straight after a version applied to this agent; every other one continues the last by one, with
    its `held_since`, a version in effect, and a day of risk clock between them."""
    bad = []
    for i, r in enumerate(records):
        if r["event_type"] == "RetirementHeld":
            nxt = records[i + 1] if i + 1 < len(records) else {}
            if nxt.get("event_type") != "OwnerAlertSent" or nxt["payload"]["subject"] != r["event_id"]:
                bad.append(i)
        if r["event_type"] == "OwnerAlertSent" and r["payload"]["kind"] == "retirement_held":
            prev = records[i - 1] if i else {}
            if prev.get("event_type") != "RetirementHeld" or prev["event_id"] != r["payload"]["subject"]:
                bad.append(i)

    def an_end(r: dict) -> bool:
        if r["event_type"] == "AgentStopped":
            return r["payload"]["agent_id"] == AGENT_ID
        if r["event_type"] == "OwnerCommandIssued":
            c = r["payload"]
            return (c["scope"], c["subject"], c["command"]) in {("agent", AGENT_ID, "resume"), ("agent", AGENT_ID, "stop")}
        return False

    seen: set[str] = set()
    last: dict | None = None
    moved: str | None = None
    for i, r in enumerate(records):
        if an_end(r):
            if last is not None:
                seen.add(last["causation_id"])
            last, moved = None, None
            continue
        if r["event_type"] == "MandateVersionApplied":
            if r["payload"]["agent_id"] == AGENT_ID and r["payload"]["result"] == "applied" and last is not None:
                moved = r["payload"]["new_version"]
            continue
        if r["event_type"] != "RetirementHeld":
            continue
        fresh = last is None or (moved is not None and r["causation_id"] != last["causation_id"])
        if fresh:
            if last is not None:
                seen.add(last["causation_id"])
            if r["causation_id"] in seen or r["payload"]["period"] != 1:
                bad.append(i)
                break
        else:
            ok = (
                r["causation_id"] == last["causation_id"]
                and r["payload"]["period"] == last["payload"]["period"] + 1
                and r["payload"]["held_since"] == last["payload"]["held_since"]
                and r["config_refs"]["mandate_version"] in {last["config_refs"]["mandate_version"], moved}
                and epoch_seconds(r["payload"]["risk_clock"]) - epoch_seconds(last["payload"]["risk_clock"]) >= DAY
            )
            if not ok:
                bad.append(i)
                break
        last, moved = r, None
    return min(bad) if bad else None


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    for case in section["silence"]:
        got = silent(case["cause"], case["records"], case["now"])
        if got != case["silent"]:
            problems.append(found("silence", f"{case['name']}: expected silent {case['silent']}, got {got}"))
    for name, draft in section["drafts"].items():
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
    for case in section["invalid_drafts"]:
        got = violations(draft_for(section, case))
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
    for case in section["valid_drafts"]:
        got = violations(draft_for(section, case))
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
    for case in section["streams"]:
        want = case["expect"].get("index")
        for record in case["records"]:
            if record["event_type"] == "RetirementHeld" and violations(record):
                problems.append(found("streams.rule", f"{case['name']}: a record breaks a draft rule"))
        if stream_failure(case["records"]) != want:
            problems.append(found("streams.rule", f"{case['name']}: expected {case['expect']}"))
        if oracle(case["records"]) != want:
            problems.append(found("streams.oracle", f"{case['name']}: expected {case['expect']}"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "rule.265.agent",
    "rule.265.cause",
    "rule.265.period",
    "rule.265.due",
    "rule.265.waiting",
    "record.extra",
    "record.missing",
    "config_refs.required",
    *(f"loose.payload.{member}" for member, _, _, _ in MEMBER_CASES),
    *(f"nullable.payload.{member}" for member in NULLED),
)
SILENCE_MUTANTS = ("silence.cancel", "silence.profit")
STREAM_MUTANTS = tuple(f"stream.266.{part}" for part in ("own", "end", "subject", "alert", "finished", "period", "one", "since", "version", "spacing"))


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    """Seeded bugs in the vectors, each registered against the one check that must reject it."""

    def mutated(fn) -> dict:
        out = copy.deepcopy(section)
        fn(out)
        return out

    def case(s, kind, name):
        return next(c for c in s[kind] if c["name"] == name)

    return [
        ("the base draft names no cause", "drafts.valid", mutated(lambda s: s["drafts"][BASE].update(causation_id=None))),
        (
            "a cancelled retirement is expected silent",
            "silence",
            mutated(lambda s: case(s, "silence", "cancelled_by_a_raised_level").update(silent=True)),
        ),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "too_soon")["expect"].update(path="payload.held_since")),
        ),
        (
            "a valid draft breaks rule 265",
            "valid_drafts",
            mutated(lambda s: case(s, "valid_drafts", "late")["changes"].append(change("payload.period", 0))),
        ),
        (
            "a stream case's expectation is moved off the record the fold refuses",
            "streams.rule",
            mutated(lambda s: case(s, "streams", "skips_a_period")["expect"].update(index=0)),
        ),
        (
            "a stream case lets a repeated period pass",
            "streams.oracle",
            mutated(lambda s: case(s, "streams", "repeats_a_period").update(expect={"outcome": "Valid"})),
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
            escaped.append(f"retirement validator mutant {mutant}")
    for mutant in STREAM_MUTANTS:
        skip = frozenset([mutant])
        if all(stream_failure(c["records"], skip) == c["expect"].get("index") for c in section["streams"]):
            escaped.append(f"retirement stream mutant {mutant}")
    for mutant in SILENCE_MUTANTS:
        skip = frozenset([mutant])
        if all(silent(c["cause"], c["records"], c["now"], skip) == c["silent"] for c in section["silence"]):
            escaped.append(f"retirement silence mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"retirement check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"retirement vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
        "streams": stream_cases(),
        "silence": silence_cases(),
    }
