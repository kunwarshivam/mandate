"""Journal spec v0.104's reference vectors for an owner's acknowledgment of a reconciliation hold
(§9.28, DEC-2830): an `OwnerAcknowledged` at version 2 with `subject` `reconciliation`, whose `event`
names the `AgentModeApplied` that held openings under `reconciliation:<subject>` (trading spec §7.1,
§11). The executor judges its copy by §9.28's four checks, the hold's record standing in for the
trigger, and an accepted one lifts that hold alone by an `AgentModeApplied` to `normal` under its
restriction, the record right after the copy.

The section's `executor` vectors have the shape of the `acknowledgments` section's: the control
stream, the account stream with the copies, the overlay, environment and founder connection, and
each acknowledgment copy's expected outcome, `lift` naming the agent and restriction it lifts. The
judge below reads checks 3 and 4 with `acknowledgments`' own functions, and an independent oracle
judges checks 1 and 2 and the lifts apart from it. Every seeded bug is shown caught.
"""

from __future__ import annotations

import copy

import acknowledgments as acks
from control import EXECUTOR, check_of, violations

SPEC = "docs/specs/journal.md v0.104 §9.28, an acknowledgment of a reconciliation hold (DEC-2830)"
ACCOUNT = acks.ACCOUNT
AGENT, OTHER = acks.AGENT, acks.OTHER_AGENT
T0 = acks.T0
INCOMPLETE, EXTERNAL, MISMATCH = "reconciliation:incomplete", "reconciliation:external_activity", "reconciliation:AAPL"


def uid(tag: str, n: int) -> str:
    return acks.uid(tag, n)


def held(n: int, restriction: str, agent: str = "*", to: str = "exits_only", at: int = T0 - 600) -> dict:
    """The executor's `AgentModeApplied` that holds (or, `to` `normal`, removes) a restriction."""
    payload = {"agent": agent, "to": to, "restriction": restriction, "originated": True, "risk_clock": acks.ts(at)}
    return acks.envelope(uid("HD", n), "AgentModeApplied", payload, EXECUTOR, ACCOUNT, at=at)


def acknowledgment(n: int, event: str, subject: str = "reconciliation", at: int = T0 + 60, **more) -> dict:
    """An owner's plain control-stream acknowledgment at version 2, naming `event`."""
    original = acks.plain(uid("AK", n), event=event, at=at, **more)
    original["schema_version"] = 2
    original["payload"]["subject"] = subject
    return original


def copied(original: dict, n: int, at: int | None = None) -> dict:
    out = acks.copied(original, uid("CP", n))
    out["schema_version"] = original["schema_version"]
    if at is not None:
        out["event_time"] = acks.ts(at)
    return out


def case(name, clause, control, account, expect, overlay=False):
    return {
        "name": name,
        "clause": clause,
        "overlay_independent": overlay,
        "environment": "paper",
        "founder_connection": False,
        "control": control,
        "account": account,
        "refused_at_append": [],
        "expect": expect,
    }


def lift(index: int, agent: str, restriction: str) -> dict:
    return {"index": index, "outcome": "lift", "agent": agent, "restriction": restriction}


def refuse(index: int, reason: str) -> dict:
    return {"index": index, "outcome": reason}


def executor_cases() -> list[dict]:
    hold = held(1, INCOMPLETE)
    a1 = acknowledgment(1, hold["event_id"])
    out = [
        case("the_incomplete_hold_lifts", "§9.28 v0.104: all four checks pass", [a1], [hold, copied(a1, 1)],
             [lift(1, "*", INCOMPLETE)]),
        case("an_agents_mismatch_hold_lifts", "the hold of one agent's instrument mismatch", [acknowledgment(1, uid("HD", 1))],
             [held(1, MISMATCH, AGENT, "paused"), copied(acknowledgment(1, uid("HD", 1)), 1)], [lift(1, AGENT, MISMATCH)]),
        case("one_hold_of_two_lifts", "it lifts the hold it names and no other",
             [acknowledgment(1, uid("HD", 2))],
             [held(1, INCOMPLETE), held(2, EXTERNAL), copied(acknowledgment(1, uid("HD", 2)), 1)],
             [lift(2, "*", EXTERNAL)]),
        case("an_unknown_hold", "check 1: no record has the id", [acknowledgment(1, uid("HD", 9))],
             [hold, copied(acknowledgment(1, uid("HD", 9)), 1)], [refuse(1, "trigger_unknown")]),
        case("a_removal_is_not_a_hold", "check 1: the record named moved the agents to normal",
             [acknowledgment(1, uid("HD", 2))],
             [hold, held(2, INCOMPLETE, to="normal"), copied(acknowledgment(1, uid("HD", 2)), 1)],
             [refuse(2, "trigger_unknown")]),
        case("a_hold_of_another_kind", "check 1: the record named holds no reconciliation restriction",
             [acknowledgment(1, uid("HD", 1))],
             [held(1, "kill_switch", AGENT, "stopped"), copied(acknowledgment(1, uid("HD", 1)), 1)],
             [refuse(1, "trigger_unknown")]),
        case("a_hold_after_the_copy", "check 1: the hold comes before the copy", [a1],
             [copied(a1, 1), hold], [refuse(0, "trigger_unknown")]),
        case("a_limit_subject_names_the_hold", "the subject decides the record: a `limit` names no hold",
             [acknowledgment(1, hold["event_id"], "limit")],
             [hold, copied(acknowledgment(1, hold["event_id"], "limit"), 1)], [refuse(1, "trigger_unknown")]),
        case("removed_before_the_copy", "check 2: the hold was removed before the acknowledgment", [a1],
             [hold, held(2, INCOMPLETE, to="normal", at=T0 - 300), copied(a1, 1)], [refuse(2, "trigger_superseded")]),
        case("applied_again_before_the_copy", "check 2: the hold applied again needs its own acknowledgment", [a1],
             [hold, held(2, INCOMPLETE, at=T0 - 300), copied(a1, 1)], [refuse(2, "trigger_superseded")]),
        case("another_agents_removal_leaves_it", "check 2: the same restriction of another agent supersedes nothing",
             [a1], [hold, held(2, INCOMPLETE, OTHER, "normal", at=T0 - 300), copied(a1, 1)], [lift(2, "*", INCOMPLETE)]),
        case("lifted_once", "check 2: the executor's own lift supersedes it for the next acknowledgment",
             [a1, acknowledgment(2, hold["event_id"], at=T0 + 90)],
             [hold, copied(a1, 1), copied(acknowledgment(2, hold["event_id"], at=T0 + 90), 2)],
             [lift(1, "*", INCOMPLETE), refuse(2, "trigger_superseded")]),
        case("a_refused_one_lifts_nothing", "a refused acknowledgment leaves the hold for the next",
             [acknowledgment(1, hold["event_id"], step_up=None), acknowledgment(2, hold["event_id"], at=T0 + 90)],
             [hold, copied(acknowledgment(1, hold["event_id"], step_up=None), 1),
              copied(acknowledgment(2, hold["event_id"], at=T0 + 90), 2)],
             [refuse(1, "step_up_missing"), lift(2, "*", INCOMPLETE)]),
        case("processed_late", "check 3: judged when processed", [a1], [hold, copied(a1, 1, at=T0 + 60 + 291)],
             [refuse(1, "step_up_stale")]),
        case("independence_under_the_overlay", "check 4: the overlay requires an independent user", [a1],
             [hold, copied(a1, 1)], [refuse(1, "not_independent")], overlay=True),
    ]
    return out


# --------------------------------------------------------------------------- the judge


def judge(vector: dict, skip: frozenset[str] = frozenset()) -> list[dict]:
    """The executor's judgment over a vector's account stream, forward, with its own lifts folded
    back in. Checks 3 and 4 are `acknowledgments`' own; checks 1 and 2 read the hold."""
    control, account = vector["control"], vector["account"]
    spent = acks.spent_before(control, frozenset())
    methods = acks.methods_for(vector["environment"], vector["founder_connection"])
    records: list[tuple[int, dict]] = []
    out = []
    for i, r in enumerate(account):
        p = r["payload"]
        if r["event_type"] == "OwnerAcknowledged" and r["schema_version"] == 2 and p.get("subject") == "reconciliation":
            outcome = judged(r, records, spent, vector["overlay_independent"], methods, skip)
            entry = {"index": i, "outcome": outcome}
            if outcome == "lift":
                named = next(h for _, h in records if h["event_id"] == p["event"])["payload"]
                entry.update(agent=named["agent"], restriction=named["restriction"])
                if "own_lift" not in skip:
                    records.append((i, held_removal(named)))
            out.append(entry)
        elif r["event_type"] == "OwnerAcknowledged":
            out.append({"index": i, "outcome": "trigger_unknown"})
        records.append((i, r))
    return out


def held_removal(named: dict) -> dict:
    return {"event_type": "AgentModeApplied", "event_id": None,
            "payload": {"agent": named["agent"], "to": "normal", "restriction": named["restriction"]}}


def judged(r, records, spent, overlay, methods, skip) -> str:
    p = r["payload"]
    before = [h for _, h in records]
    named = next((h for h in before if h["event_id"] == p["event"]), None)
    is_hold = (
        named is not None
        and named["event_type"] == "AgentModeApplied"
        and (named["payload"]["to"] != "normal" or "1.removal" in skip)
        and (named["payload"]["restriction"].startswith("reconciliation:") or "1.kind" in skip)
    )
    if not is_hold:
        return "trigger_unknown"
    hp = named["payload"]
    after = before[before.index(named) + 1:]
    if "2" not in skip and any(
        h["event_type"] == "AgentModeApplied"
        and h["payload"]["restriction"] == hp["restriction"]
        and (h["payload"]["agent"] == hp["agent"] or "2.any_agent" in skip)
        for h in after
    ):
        return "trigger_superseded"
    at = acks.seconds_of(r["event_time"])
    failed = acks.step_up_failure(p["step_up"], at, spent.get(r["causation_id"], set()), frozenset(), "acknowledgment",
                                  methods)
    if failed:
        return failed
    if (p["independent_required"] or overlay) and "4" not in skip:
        if p["request"] is None or p["user"] == p["requested_by"]:
            return "not_independent"
    return "lift"


def oracle(vector: dict) -> list[dict]:
    """Checks 1 and 2 and the lifts, judged apart from `judge`: each acknowledgment's hold is found by
    its position, and it stands when the latest record of the same agent and restriction before the
    copy, the executor's earlier lifts included, is the hold itself. Checks 3 and 4 are taken from
    `acknowledgments`, which has its own oracle."""
    account = vector["account"]
    spent = acks.spent_before(vector["control"], frozenset())
    methods = acks.methods_for(vector["environment"], vector["founder_connection"])
    position = {r["event_id"]: i for i, r in enumerate(account)}
    lifted: list[tuple[int, str, str]] = []
    out = []
    for i, r in enumerate(account):
        if r["event_type"] != "OwnerAcknowledged":
            continue
        p = r["payload"]
        if r["schema_version"] != 2 or p.get("subject") != "reconciliation":
            out.append({"index": i, "outcome": "trigger_unknown"})
            continue
        h = position.get(p["event"])
        hold = account[h] if h is not None and h < i else None
        if not (hold and hold["event_type"] == "AgentModeApplied" and hold["payload"]["to"] != "normal"
                and hold["payload"]["restriction"][:15] == "reconciliation:"):
            out.append({"index": i, "outcome": "trigger_unknown"})
            continue
        key = (hold["payload"]["agent"], hold["payload"]["restriction"])
        latest = max(
            [j for j, x in enumerate(account[:i]) if x["event_type"] == "AgentModeApplied"
             and (x["payload"]["agent"], x["payload"]["restriction"]) == key]
            + [j for j, a, rs in lifted if (a, rs) == key and j < i]
        )
        if latest != h:
            out.append({"index": i, "outcome": "trigger_superseded"})
            continue
        failed = acks.step_up_failure(p["step_up"], acks.seconds_of(r["event_time"]),
                                      spent.get(r["causation_id"], set()), frozenset(), "acknowledgment", methods)
        independent = p["independent_required"] or vector["overlay_independent"]
        if failed or (independent and (p["request"] is None or p["user"] == p["requested_by"])):
            out.append({"index": i, "outcome": failed or "not_independent"})
            continue
        lifted.append((i, *key))
        out.append({"index": i, "outcome": "lift", "agent": key[0], "restriction": key[1]})
    return out


# --------------------------------------------------------------------------- checks and seeded bugs

ORACLE_CHECKS = ("records.valid", "executor.judge", "executor.oracle")
MUTANTS = ("1.removal", "1.kind", "2", "2.any_agent", "own_lift", "4")


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def check_section(section: dict) -> list[str]:
    problems = []
    for vector in section["executor"]:
        for stream in ("control", "account"):
            for i, record in enumerate(vector[stream]):
                got = violations(record)
                if got:
                    problems.append(found("records.valid", f"{vector['name']} {stream} {i}: {got}"))
        if judge(vector) != vector["expect"]:
            problems.append(found("executor.judge", f"{vector['name']}: {judge(vector)} != {vector['expect']}"))
        if oracle(vector) != vector["expect"]:
            problems.append(found("executor.oracle", f"{vector['name']}: {oracle(vector)} != {vector['expect']}"))
    return problems


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    def mutated(fn) -> dict:
        out = copy.deepcopy(section)
        fn(out)
        return out

    def vector(s, name):
        return next(v for v in s["executor"] if v["name"] == name)

    return [
        ("a hold record that does not validate", "records.valid",
         mutated(lambda s: vector(s, "the_incomplete_hold_lifts")["account"][0]["payload"].update(to="held"))),
        ("an expectation that differs", "executor.judge",
         mutated(lambda s: vector(s, "removed_before_the_copy")["expect"][0].update(outcome="lift"))),
    ]


def run_mutants(section: dict) -> list[str]:
    escaped = []
    for mutant in MUTANTS:
        skip = frozenset([mutant])
        if not any(judge(v, skip) != v["expect"] for v in section["executor"]):
            escaped.append(f"reconciliation acknowledgment mutant {mutant}")
    for name, check, mutated in vector_mutants(section):
        caught = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught:
            escaped.append(f"reconciliation acknowledgment vector mutant: {name} (caught by {sorted(caught)})")
    return escaped


def build_section() -> dict:
    return {"spec": SPEC, "executor": executor_cases()}
