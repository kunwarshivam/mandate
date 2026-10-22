"""Journal spec v0.78 §9.15's stream rule 264 (DEC-2351): an `OwnerAlertSent` of kind
`kill_switch_unapplied` names as its subject an unapplied crash loop, which the control stream alone
shows. That is a `ProcessCrashLooped` of an executor whose run is held, with no agent of its account
deployed and not stopped at the loop. A run is held from a `kill_switch` start made while no agent of
the account was deployed and not stopped, until a later start for a deployment, a request, or an
exchange, or a stop of the executor (DEC-2344 items 1 and 3).

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


def requested(index: int, connection: str = CONNECTION) -> dict:
    """An OAuth connect's request, which rule 264 does not read: only agents hold or free a run."""
    return {
        "event_id": event_id(index),
        "event_type": "ConnectionRequested",
        "schema_version": 2,
        "causation_id": None,
        "payload": {"connection_id": connection, "account_ref": ACCOUNT},
    }


def revoked(index: int, connection: str = CONNECTION) -> dict:
    """A revocation, which rule 264 does not read: a deployment on the connection counts until its
    `AgentStopped`."""
    return {
        "event_id": event_id(index),
        "event_type": "ConnectionRevoked",
        "schema_version": 1,
        "causation_id": None,
        "payload": {"connection_id": connection},
    }


def stop(index: int, generation: int, account: str = ACCOUNT) -> dict:
    return {
        "event_id": event_id(index),
        "event_type": "ProcessStopIssued",
        "schema_version": 1,
        "causation_id": None,
        "payload": {"process": "executor", "subject": account, "generation": generation, "cause": "agent_stopped"},
    }


CLEARING = ("deployed", "requested", "exchanged")


def stream_failure(records: list[dict], skip: frozenset[str] = frozenset()) -> int | None:
    """Rule 264 folded once, in order: per executor, whether its run is held, set by a
    `kill_switch` start made while no agent of its account was deployed and not stopped, and
    cleared by a later start for a deployment, a request, or an exchange, or by a stop; per loop,
    whether it is held with no agent wanting the executor at the loop. Returns the index of the
    first `kill_switch_unapplied` alert whose subject is not such a loop."""
    accounts: dict[str, str] = {}
    wanting: dict[str, set[str]] = {}
    agent_account: dict[str, str] = {}
    held: dict[str, bool] = {}
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
        elif t == "AgentStopped":
            account = agent_account.get(p["agent_id"])
            if account is not None:
                wanting[account].discard(p["agent_id"])
        elif t == "ProcessStartIssued" and p["process"] == "executor":
            account = p["subject"]
            if p["cause"] == "kill_switch" and (not wanting.get(account) or "stream.264.wanted" in skip):
                held[account] = True
            elif p["cause"] in CLEARING and "stream.264.cleared" not in skip:
                held[account] = False
        elif t == "ProcessStopIssued" and p["process"] == "executor":
            if "stream.264.stop" not in skip:
                held[p["subject"]] = False
        elif t == "ProcessCrashLooped":
            account = p["subject"]
            is_executor = p["process"] == "executor" or "stream.264.process" in skip
            free = not wanting.get(account) or "stream.264.at_loop" in skip
            unapplied[record["event_id"]] = is_executor and held.get(account, False) and free
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
            "rule 264: a held run stays held through its restarts",
            [est, first, switched, start(3, "restart", 3), start(4, "restart", 4), loop_after_restarts, alert(6, loop_after_restarts)],
            None,
        ),
        case("at_once", "rule 264: a loop of the switch's own start", [est, first, switched, looped(3, 2), alert(4, looped(3, 2))], None),
        case(
            "an_ordinary_loop",
            "rule 264: a run a deployment began is not held",
            [est, first, start(2, "restart", 2), looped(3, 2), alert(4, looped(3, 2))],
            4,
        ),
        case(
            "wanted_at_the_loop",
            "rule 264: an agent deployed and not stopped at the loop makes it ordinary",
            [est, first, switched, deployed(13), start(4, "restart", 3), looped(5, 3), alert(6, looped(5, 3))],
            6,
        ),
        case(
            "deployed_and_retired_during_the_run",
            "rule 264: a deployment and retirement during the run leave it held",
            [est, first, switched, deployed(13), stopped(14, 13), start(4, "restart", 3), looped(5, 3), alert(6, looped(5, 3))],
            None,
        ),
        case(
            "a_wanted_switch_after_a_held_one",
            "rule 264: a later switch's start while wanted does not clear the hold",
            [est, first, switched, deployed(13), start(4, "kill_switch", 3), stopped(14, 13), looped(5, 3), alert(6, looped(5, 3))],
            None,
        ),
        case(
            "stopped_since",
            "rule 264: the manager's stop of its generation ends the hold",
            [est, first, switched, stop(3, 2), deployed(13), start(4, "restart", 3), stopped(14, 13), looped(5, 3), alert(6, looped(5, 3))],
            8,
        ),
        case(
            "deployed_elsewhere",
            "rule 264: a deployment on another account's connection does not make this one wanted",
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
            "rule 264: a switch's start of an executor an agent still wants holds nothing",
            [est, deployed(11), first, start(2, "kill_switch", 2), stopped(12, 11), start(3, "restart", 3), looped(4, 3), alert(5, looped(4, 3))],
            7,
        ),
        case(
            "a_new_run_after_a_loop",
            "rule 264: a later switch's start after an unapplied loop is held too",
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
            "rule 264: the hold is the looping executor's own",
            [est, first, start(2, "kill_switch", 2, account=OTHER_ACCOUNT), start(3, "restart", 2), looped(4, 2), alert(5, looped(4, 2))],
            5,
        ),
        case("names_a_start", "rule 264: the subject is the loop, not the start", [est, first, switched, looped(3, 2), alert(4, switched)], 4),
        case(
            "an_ordinary_alert_on_an_ordinary_loop",
            "rule 264 reads only `kill_switch_unapplied`",
            [est, first, start(2, "restart", 2), looped(3, 2)],
            None,
        ),
        case(
            "switched_during_a_connect",
            "rule 264: a connect's request wants the executor but holds and frees nothing",
            [est, first, requested(13), switched, start(3, "restart", 3), looped(4, 3), alert(5, looped(4, 3))],
            None,
        ),
        case(
            "redeployed_then_retired",
            "rule 264: a deployment and that agent's retirement during the run leave it held",
            [est, first, switched, deployed(13), start(3, "restart", 3), stopped(14, 13), looped(4, 3), alert(5, looped(4, 3))],
            None,
        ),
        case(
            "an_exchange_loop",
            "rule 264: a token exchange's loop is never an unapplied one",
            [est, first, switched, looped(3, 2, process="token_exchange"), alert(4, looped(3, 2, process="token_exchange"))],
            4,
        ),
        case(
            "deployed_on_a_revoked_connection",
            "rule 264: a deployment on a connection revoked since still wants the executor until its stop",
            [est, first, switched, deployed(13), revoked(14), start(3, "restart", 3), looped(4, 3), alert(5, looped(4, 3))],
            7,
        ),
        case(
            "cleared_by_a_request",
            "rule 264: a start for a request ends the hold",
            [est, first, switched, start(3, "requested", 3), looped(4, 3), alert(5, looped(4, 3))],
            5,
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
    back from it to the executor's last start for a deployment, a request, or an exchange, or its
    last stop, and require a `kill_switch` start after that made when no agent of the account was
    deployed and not stopped, and none such at the loop."""

    def wanted_before(index: int, account: str) -> bool:
        mine = {
            r["payload"]["connection_id"]
            for r in records[:index]
            if r["event_type"] == "ConnectionEstablished" and r["payload"]["account_ref"] == account
        }
        deployed_ = {r["payload"]["agent_id"] for r in records[:index] if r["event_type"] == "AgentDeployed" and r["payload"]["connection_id"] in mine}
        stopped_ = {r["payload"]["agent_id"] for r in records[:index] if r["event_type"] == "AgentStopped"}
        return bool(deployed_ - stopped_)

    by_id = {r["event_id"]: (i, r) for i, r in enumerate(records)}
    for i, record in enumerate(records):
        if record["event_type"] != "OwnerAlertSent" or record["payload"]["kind"] != "kill_switch_unapplied":
            continue
        hit = by_id.get(record["payload"]["subject"])
        if hit is None or hit[1]["event_type"] != "ProcessCrashLooped" or hit[1]["payload"]["process"] != "executor":
            return i
        at, loop = hit
        account = loop["payload"]["subject"]

        def mine(r: dict) -> bool:
            return r["payload"].get("process") == "executor" and r["payload"].get("subject") == account

        clear = max(
            (
                j
                for j in range(at)
                if mine(records[j])
                and (
                    records[j]["event_type"] == "ProcessStopIssued"
                    or (records[j]["event_type"] == "ProcessStartIssued" and records[j]["payload"]["cause"] in CLEARING)
                )
            ),
            default=-1,
        )
        holds = any(
            records[j]["event_type"] == "ProcessStartIssued"
            and mine(records[j])
            and records[j]["payload"]["cause"] == "kill_switch"
            and not wanted_before(j, account)
            for j in range(clear + 1, at)
        )
        if not holds or wanted_before(at, account):
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

STREAM_MUTANTS = ("stream.264.wanted", "stream.264.cleared", "stream.264.stop", "stream.264.at_loop", "stream.264.process", "stream.264.subject")


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
            mutated(lambda s: case(s, "wanted_at_the_loop")["expect"].update(index=5)),
        ),
        (
            "a stream case lets an alert on a loop an agent wants pass",
            "streams.oracle",
            mutated(lambda s: case(s, "wanted_at_the_loop").update(expect={"outcome": "Valid"})),
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
