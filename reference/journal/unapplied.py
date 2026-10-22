"""Journal spec v0.78 §9.15's stream rule 264 (DEC-2351): an `OwnerAlertSent` of kind
`kill_switch_unapplied` names as its subject an unapplied crash loop, which the control stream alone
shows. That is a `ProcessCrashLooped` of an executor whose run began with a `kill_switch` start (the
latest of its starts before the loop whose cause is not `restart`) made while no agent of its account
was deployed and not stopped, with no `AgentDeployed` on one of its account's connections between
that start and the loop (DEC-2344 item 1).

This module builds the `kill_switch_unapplied` section: control streams of the records the rule
reads, each with its expected outcome. The stream fold reads the records once, in order; an
independent oracle reads each alert's subject backwards from it; both agree on every stream, and
each seeded bug is shown caught.
"""

from __future__ import annotations

import copy

SPEC = "docs/specs/journal.md v0.78 §9.15, rule 264 (DEC-2351)"
ACCOUNT = "01J8Z2ACCT00000000000000A1"
OTHER_ACCOUNT = "01J8Z2ACCT00000000000000A2"
CONNECTION = "conn_01J8Z2CONN000000000000C1"
OTHER_CONNECTION = "conn_01J8Z2CONN000000000000C2"
SWITCH = "01J8Z6KS00000000000000000S"


def event_id(index: int) -> str:
    return f"01J8Z6U{index:02d}".ljust(26, "0")


def established(index: int, connection: str = CONNECTION, account: str = ACCOUNT) -> dict:
    return {
        "event_id": event_id(index),
        "event_type": "ConnectionEstablished",
        "schema_version": 2,
        "causation_id": None,
        "payload": {"connection_id": connection, "account_ref": account},
    }


def deployed(index: int, connection: str = CONNECTION) -> dict:
    return {
        "event_id": event_id(index),
        "event_type": "AgentDeployed",
        "schema_version": 2,
        "causation_id": None,
        "payload": {"agent_id": f"agent_{index}", "connection_id": connection},
    }


def stopped(index: int, agent_index: int) -> dict:
    return {
        "event_id": event_id(index),
        "event_type": "AgentStopped",
        "schema_version": 1,
        "causation_id": None,
        "payload": {"agent_id": f"agent_{agent_index}"},
    }


def start(index: int, cause: str, generation: int, account: str = ACCOUNT, process: str = "executor") -> dict:
    return {
        "event_id": event_id(index),
        "event_type": "ProcessStartIssued",
        "schema_version": 1,
        "causation_id": SWITCH if cause == "kill_switch" else None,
        "payload": {"process": process, "subject": account, "generation": generation, "cause": cause},
    }


def looped(index: int, generation: int, account: str = ACCOUNT, process: str = "executor") -> dict:
    return {
        "event_id": event_id(index),
        "event_type": "ProcessCrashLooped",
        "schema_version": 1,
        "causation_id": None,
        "payload": {"process": process, "subject": account, "generation": generation},
    }


def alert(index: int, subject: dict, kind: str = "kill_switch_unapplied") -> dict:
    return {
        "event_id": event_id(index),
        "event_type": "OwnerAlertSent",
        "schema_version": 1,
        "causation_id": subject["event_id"],
        "payload": {"subject": subject["event_id"], "kind": kind, "owner_command": None},
    }


def stream_failure(records: list[dict], skip: frozenset[str] = frozenset()) -> int | None:
    """Rule 264 folded once, in order: per executor, the cause of its run's first start and whether
    a deployment on its account came since; per loop, whether it left a switch unapplied. Returns
    the index of the first `kill_switch_unapplied` alert whose subject is not an unapplied loop."""
    accounts: dict[str, str] = {}
    wanting: dict[str, set[str]] = {}
    agent_account: dict[str, str] = {}
    run: dict[str, tuple[str, bool]] = {}
    unapplied: dict[str, bool] = {}
    for i, record in enumerate(records):
        t, p = record["event_type"], record["payload"]
        if t == "ConnectionEstablished":
            accounts[p["connection_id"]] = p["account_ref"]
        elif t == "AgentDeployed":
            account = accounts.get(p["connection_id"])
            if account is not None:
                agent_account[p["agent_id"]] = account
                wanting.setdefault(account, set()).add(p["agent_id"])
            if account in run and "stream.264.deployed" not in skip:
                run[account] = (run[account][0], True)
        elif t == "AgentStopped":
            account = agent_account.get(p["agent_id"])
            if account is not None:
                wanting[account].discard(p["agent_id"])
        elif t == "ProcessStartIssued" and p["process"] == "executor":
            if p["cause"] != "restart" or "stream.264.restart" in skip:
                held = p["cause"] == "kill_switch" and (not wanting.get(p["subject"]) or "stream.264.wanted" in skip)
                run[p["subject"]] = ("kill_switch" if held else f"{p['cause']}_wanted" if p["cause"] == "kill_switch" else p["cause"], False)
        elif t == "ProcessCrashLooped":
            cause, redeployed = run.get(p["subject"], ("", False))
            is_executor = p["process"] == "executor" or "stream.264.process" in skip
            unapplied[record["event_id"]] = is_executor and cause == "kill_switch" and not redeployed
        elif t == "OwnerAlertSent" and p["kind"] == "kill_switch_unapplied":
            if not unapplied.get(p["subject"], False) and "stream.264.subject" not in skip:
                return i
    return None


def stream_cases() -> list[dict]:
    def case(name, clause, records, index):
        expect = {"outcome": "Valid"} if index is None else {"outcome": "Invalid", "rule": "rule 264", "index": index}
        return {"name": name, "clause": clause, "records": records, "expect": expect}

    est = established(0)
    first = start(1, "deployed", 1)
    switched = start(2, "kill_switch", 2)
    loop_after_restarts = looped(5, 4)
    return [
        case(
            "after_restarts",
            "rule 264: the run's first start was the switch's, whatever restarts followed",
            [est, first, switched, start(3, "restart", 3), start(4, "restart", 4), loop_after_restarts, alert(6, loop_after_restarts)],
            None,
        ),
        case(
            "at_once",
            "rule 264: a loop of the switch's own start",
            [est, first, switched, looped(3, 2), alert(4, looped(3, 2))],
            None,
        ),
        case(
            "an_ordinary_loop",
            "rule 264: a run that began with a deployment is ordinary",
            [est, first, start(2, "restart", 2), looped(3, 2), alert(4, looped(3, 2))],
            4,
        ),
        case(
            "redeployed",
            "rule 264: a deployment on the account since the switch's start makes the loop ordinary",
            [est, first, switched, deployed(3), start(4, "restart", 3), looped(5, 3), alert(6, looped(5, 3))],
            6,
        ),
        case(
            "deployed_elsewhere",
            "rule 264: a deployment on another account's connection does not end this run",
            [est, established(1, OTHER_CONNECTION, OTHER_ACCOUNT), start(2, "deployed", 1), start(3, "kill_switch", 2),
             deployed(4, OTHER_CONNECTION), start(5, "restart", 3), looped(6, 3), alert(7, looped(6, 3))],
            None,
        ),
        case(
            "retired_before_the_switch",
            "rule 264: an agent deployed and retired before the switch's start leaves it wanted by none",
            [est, deployed(11), first, stopped(12, 11), start(3, "kill_switch", 2), looped(4, 2), alert(5, looped(4, 2))],
            None,
        ),
        case(
            "switched_while_wanted",
            "rule 264: a switch's start of an executor an agent still wants is no held run",
            [est, deployed(11), first, start(2, "kill_switch", 2), start(3, "restart", 3), looped(4, 3), alert(5, looped(4, 3))],
            6,
        ),
        case(
            "a_new_run_after_a_loop",
            "rule 264: a later switch's start after an unapplied loop is a new run, and its loop is unapplied too",
            [est, first, switched, looped(3, 2), alert(4, looped(3, 2)), start(5, "kill_switch", 3),
             start(6, "restart", 4), looped(7, 4), alert(8, looped(7, 4))],
            None,
        ),
        case(
            "a_runtime_loop",
            "rule 264: only an executor's loop",
            [est, first, switched, looped(3, 2, process="runtime"), alert(4, looped(3, 2, process="runtime"))],
            4,
        ),
        case(
            "another_accounts_switch",
            "rule 264: the run is the looping executor's own",
            [est, first, start(2, "kill_switch", 2, account=OTHER_ACCOUNT), start(3, "restart", 2), looped(4, 2),
             alert(5, looped(4, 2))],
            5,
        ),
        case(
            "names_a_start",
            "rule 264: the subject is the loop, not the start",
            [est, first, switched, looped(3, 2), alert(4, switched)],
            4,
        ),
        case(
            "an_ordinary_alert_on_an_ordinary_loop",
            "rule 264 reads only `kill_switch_unapplied`",
            [est, first, start(2, "restart", 2), looped(3, 2)],
            None,
        ),
    ]


# --------------------------------------------------------------------------- independent oracle

ORACLE_CHECKS = ("streams.rule", "streams.oracle")


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def oracle(records: list[dict]) -> int | None:
    """Rule 264 read backwards: for each `kill_switch_unapplied` alert, find its subject, then walk
    back from it to the executor's last start that is not a restart, and look for a deployment on
    its account's connections in between."""
    by_id = {r["event_id"]: (i, r) for i, r in enumerate(records)}
    for i, record in enumerate(records):
        if record["event_type"] != "OwnerAlertSent" or record["payload"]["kind"] != "kill_switch_unapplied":
            continue
        hit = by_id.get(record["payload"]["subject"])
        if hit is None or hit[1]["event_type"] != "ProcessCrashLooped" or hit[1]["payload"]["process"] != "executor":
            return i
        at, loop = hit
        account = loop["payload"]["subject"]
        mine = {
            r["payload"]["connection_id"]
            for r in records[:at]
            if r["event_type"] == "ConnectionEstablished" and r["payload"]["account_ref"] == account
        }
        began = next(
            (
                j
                for j in range(at - 1, -1, -1)
                if records[j]["event_type"] == "ProcessStartIssued"
                and records[j]["payload"]["process"] == "executor"
                and records[j]["payload"]["subject"] == account
                and records[j]["payload"]["cause"] != "restart"
            ),
            None,
        )
        if began is None or records[began]["payload"]["cause"] != "kill_switch":
            return i
        deployed_then = {
            r["payload"]["agent_id"]
            for r in records[:began]
            if r["event_type"] == "AgentDeployed" and r["payload"]["connection_id"] in mine
        }
        stopped_then = {r["payload"]["agent_id"] for r in records[:began] if r["event_type"] == "AgentStopped"}
        if deployed_then - stopped_then:
            return i
        if any(
            r["event_type"] == "AgentDeployed" and r["payload"]["connection_id"] in mine
            for r in records[began + 1 : at]
        ):
            return i
    return None


def check_section(section: dict) -> list[str]:
    problems = []
    for case in section["streams"]:
        want = case["expect"].get("index")
        if stream_failure(case["records"]) != want:
            problems.append(found("streams.rule", f"{case['name']}: expected {case['expect']}"))
        if oracle(case["records"]) != want:
            problems.append(found("streams.oracle", f"{case['name']}: expected {case['expect']}"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

STREAM_MUTANTS = ("stream.264.wanted", "stream.264.deployed", "stream.264.restart", "stream.264.process", "stream.264.subject")


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    def mutated(fn) -> dict:
        out = copy.deepcopy(section)
        fn(out)
        return out

    def case(s, name):
        return next(c for c in s["streams"] if c["name"] == name)

    return [
        (
            "a stream case's expectation is moved off the alert the fold refuses",
            "streams.rule",
            mutated(lambda s: case(s, "redeployed")["expect"].update(index=5)),
        ),
        (
            "a stream case lets a redeployed run's alert pass",
            "streams.oracle",
            mutated(lambda s: case(s, "redeployed").update(expect={"outcome": "Valid"})),
        ),
    ]


def run_mutants(section: dict) -> list[str]:
    escaped = []
    for mutant in STREAM_MUTANTS:
        skip = frozenset([mutant])
        if all(stream_failure(c["records"], skip) == c["expect"].get("index") for c in section["streams"]):
            escaped.append(f"kill_switch_unapplied stream mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"kill_switch_unapplied check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {problem.split(":", 1)[0] for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"kill_switch_unapplied vector mutant: {name} (not caught by {check})")
    return escaped


def build_section() -> dict:
    return {"spec": SPEC, "streams": stream_cases()}
