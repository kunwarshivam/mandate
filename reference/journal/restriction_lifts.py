"""Journal spec v0.102's reference vectors for lifting an account restriction (DEC-2824): §9.5's
`AccountRestrictionLifted`, §9.28's acknowledgment records at version 2, whose `subject` lets an
acknowledgment name an `AccountRestrictionChanged`, and the account stream's rules 309 (amended),
310, 311 and 312.

The schemas live in `control.py`, so one validator judges every closed schema. This module builds
the `restriction_lifts` section:
- base drafts of a lift for each kind of evidence and of both version-2 acknowledgment records;
- an invalid draft for every member type;
- valid drafts for each cause, each state left, and both subjects;
- account streams for the stream rules.

The rules are judged by `stream_failure`. An independent oracle replays each stream into a timeline
of causes and acknowledgments and judges every lift from that. It also judges a seeded fuzz of
random streams, on which the two must agree, and every seeded bug is shown caught.
"""

from __future__ import annotations

import copy
import random

import acknowledgments
import account_restrictions
from common import change, delete
from control import (
    EXECUTOR,
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

SPEC = "docs/specs/journal.md v0.102 §9.5 rules 309 to 312 and §9.28 version 2 (DEC-2824)"
ACCOUNT = account_restrictions.ACCOUNT
AT = account_restrictions.AT
RISK_CLOCK = account_restrictions.RISK_CLOCK
BLOCKED, CLOSING, ACTIVE = "blocked", "closing_only", "active"
REJECT, NOTICE, CONNECTION = "broker_reject", "broker_notice", "connection_unavailable"
CAUSES = (REJECT, NOTICE, CONNECTION)
BROKER = (REJECT, NOTICE)
STRICTNESS = {ACTIVE: 0, CLOSING: 1, BLOCKED: 2}
CONNECTION_ID = "conn_01"
OWNER = acknowledgments.REQUESTER
CLEAN = {"status": "ACTIVE", "trading_blocked": False, "account_blocked": False, "trade_suspended_by_user": False}
ROW_ONE = ("status", "trading_blocked", "account_blocked", "trade_suspended_by_user")


def uid(tag: str, n: int) -> str:
    return acknowledgments.uid(tag, n)


def envelope(event_id: str, event_type: str, payload: dict, causation: str | None = None, version: int = 1) -> dict:
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": event_id,
        "stream_id": ACCOUNT,
        "event_type": event_type,
        "schema_version": version,
        "event_time": AT,
        "clock_source": "local",
        "causation_id": causation,
        "correlation_id": None,
        "actor": dict(EXECUTOR),
        "config_refs": {},
        "payload": payload,
        "artifact_refs": [],
        "pii_refs": [],
    }


def raised(event_id: str, restriction: str, cause: str) -> dict:
    code = "account_trading_blocked" if restriction == BLOCKED else "account_restricted"
    return account_restrictions.restricted(event_id, restriction, cause, code)


def acknowledgment(event_id: str, event: str, subject: str = "restriction", version: int = 2) -> dict:
    """An owner's control-stream acknowledgment where no independent user is required, naming
    `event`; version 2 carries `subject`, version 1 none."""
    original = acknowledgments.plain(event_id, event=event)
    if version == 2:
        original["schema_version"] = 2
        original["payload"]["subject"] = subject
    return original


def copied(original: dict, event_id: str) -> dict:
    """The executor's account-stream copy, the original's payload and version unchanged."""
    out = acknowledgments.copied(original, event_id)
    out["schema_version"] = original["schema_version"]
    out["event_time"] = AT
    return out


def ack_copy(n: int, event: str, subject: str = "restriction", version: int = 2) -> dict:
    """The copy of acknowledgment `n` naming `event`; its original is `AK` `n`."""
    return copied(acknowledgment(uid("AK", n), event, subject, version), uid("CP", n))


def refused(n: int, original: str) -> dict:
    """§9.28's refusal of acknowledgment copy `n`, naming its control-stream original."""
    out = acknowledgments.refused(uid("RF", n), "step_up_stale", original)
    out["event_time"] = AT
    return out


def observed(event_id: str, **row_one) -> dict:
    """The broker's account report; clean unless a row-1 member is given."""
    payload = {
        **CLEAN,
        "crypto_status": "ACTIVE",
        "multiplier": 1,
        "equity": "1000",
        "cash": "1000",
        "buying_power": "1000",
        "non_marginable_buying_power": "1000",
        "accrued_fees": "0",
        "risk_clock": RISK_CLOCK,
        **row_one,
    }
    return envelope(event_id, "AccountStateObserved", payload)


def connection(event_id: str, frm: str, to: str, reason: str, causation: str | None = None) -> dict:
    """The connection's state change; an `acknowledged` one names the acknowledgment copy (rule 61)."""
    payload = {"connection_id": CONNECTION_ID, "from": frm, "to": to, "reason": reason, "risk_clock": RISK_CLOCK}
    return envelope(event_id, "ConnectionStateChanged", payload, causation=causation)


def lifted(event_id: str, cause: str, ack: str, evidence: str, restriction: str) -> dict:
    payload = {
        "cause": cause,
        "acknowledgment": ack,
        "evidence": evidence,
        "restriction": restriction,
        "risk_clock": RISK_CLOCK,
    }
    return envelope(event_id, "AccountRestrictionLifted", payload, causation=ack)


# --------------------------------------------------------------------------- drafts

BROKER_LIFT, CONNECTION_LIFT = "lifted_broker", "lifted_connection"
ACK_V2, ACK_V2_COPY, REQUEST_V2 = "acknowledged_v2", "acknowledged_v2_copy", "requested_v2"


def base_drafts() -> dict[str, dict]:
    request = acknowledgments.requested(uid("RQ", 2), event=uid("RS", 1))
    request["schema_version"] = 2
    request["payload"]["subject"] = "restriction"
    return {
        BROKER_LIFT: lifted(uid("XT", 1), REJECT, uid("CP", 1), uid("AS", 1), ACTIVE),
        CONNECTION_LIFT: lifted(uid("XT", 2), CONNECTION, uid("CP", 2), uid("CS", 1), CLOSING),
        ACK_V2: acknowledgment(uid("AK", 1), uid("RS", 1)),
        ACK_V2_COPY: ack_copy(1, uid("RS", 1)),
        REQUEST_V2: request,
    }


def invalid_drafts() -> list[dict]:
    """Each draft breaks the closed schema once, at the member it names."""

    def bad(name, clause, base, changes, path, reason="schema"):
        return control_invalid(name, clause, base, changes, reason, path)

    out = []
    for member, other in (("cause", "broker"), ("restriction", "normal")):
        path = f"payload.{member}"
        out += [
            bad(f"lift.{member}_unknown", f"§9.5: `{member}` is one of its values", BROKER_LIFT, [change(path, other)],
                path, "non_canonical"),
            bad(f"lift.{member}_null", f"§9.5: `{member}` is never null", BROKER_LIFT, [change(path, None)], path),
            bad(f"lift.{member}_number", f"§9.5: `{member}` is text", BROKER_LIFT, [change(path, 1)], path),
            bad(f"lift.{member}_missing", "§9.5: every member present", BROKER_LIFT, [delete(path)], path),
        ]
    for member in ("acknowledgment", "evidence"):
        path = f"payload.{member}"
        out += [
            bad(f"lift.{member}_not_a_ulid", f"§9.5: `{member}` is a ULID", BROKER_LIFT, [change(path, "evt_1")], path,
                "non_canonical"),
            bad(f"lift.{member}_null", f"§9.5: `{member}` is never null", BROKER_LIFT, [change(path, None)], path),
            bad(f"lift.{member}_missing", "§9.5: every member present", BROKER_LIFT, [delete(path)], path),
        ]
    out += [
        bad("lift.no_risk_clock", "§9.5: every member present", BROKER_LIFT, [delete("payload.risk_clock")],
            "payload.risk_clock"),
        bad("lift.extra_reason_code", "§9.5: the schema is closed", BROKER_LIFT,
            [change("payload.reason_code", "account_restricted")], "payload.reason_code"),
        bad("lift.version_2", "§9.5: registered at `schema_version` 1 only", BROKER_LIFT, [change("schema_version", 2)],
            "payload", "unknown_schema"),
        bad("lift.on_the_control_stream", "§9.5: an account-stream record", BROKER_LIFT,
            [change("stream_id", f"ctl:{acknowledgments.WORKSPACE}")], "event_type", "wrong_stream"),
    ]
    for base in (ACK_V2, ACK_V2_COPY, REQUEST_V2):
        out += [
            bad(f"{base}.subject_unknown", "§9.28 v2: `subject` is `limit` or `restriction`", base,
                [change("payload.subject", "account")], "payload.subject", "non_canonical"),
            bad(f"{base}.subject_null", "§9.28 v2: `subject` is never null", base, [change("payload.subject", None)],
                "payload.subject"),
            bad(f"{base}.subject_missing", "§9.28 v2: `subject` present at version 2", base, [delete("payload.subject")],
                "payload.subject"),
            bad(f"{base}.subject_at_version_1", "§9.28: version 1 has no `subject`", base, [change("schema_version", 1)],
                "payload.subject"),
        ]
    out += [
        bad("acknowledged_v2.rule_239", "rule 239 at version 2: independent with no request", ACK_V2,
            [change("payload.independent_required", True)], "payload.request"),
        bad("requested_v2.rule_239", "rule 239 at version 2: a request always requires independence", REQUEST_V2,
            [change("payload.independent_required", False)], "payload.independent_required"),
    ]
    return out


def valid_drafts() -> list[dict]:
    return [
        *[
            control_valid(f"lift.{cause}", "§9.5: a lift of each cause", BROKER_LIFT, [change("payload.cause", cause)])
            for cause in CAUSES
        ],
        *[
            control_valid(f"lift.leaves_{state}", "§9.5: each state the causes may leave", BROKER_LIFT,
                          [change("payload.restriction", state)])
            for state in (ACTIVE, CLOSING, BLOCKED)
        ],
        control_valid("lift.live", "§9.5: either environment", CONNECTION_LIFT, [change("environment", "live")]),
        control_valid("acknowledged_v2.limit", "§9.28 v2: `limit` means version 1's", ACK_V2,
                      [change("payload.subject", "limit")]),
        control_valid("acknowledged_v2_copy.limit", "§9.28 v2: a copy of either subject", ACK_V2_COPY,
                      [change("payload.subject", "limit")]),
        control_valid("requested_v2.limit", "§9.28 v2: a request of either subject", REQUEST_V2,
                      [change("payload.subject", "limit")]),
    ]


# --------------------------------------------------------------------------- the stream rules

RULE_309, RULE_310, RULE_311, RULE_312 = "rule 309", "rule 310", "rule 311", "rule 312"


def _clean(payload: dict, skip: frozenset[str]) -> bool:
    return all(
        f"312.clean.{member}" in skip or payload[member] == CLEAN[member] for member in ROW_ONE
    )


def stream_failure(records: list[dict], skip: frozenset[str] = frozenset()) -> dict | None:
    """Rules 309 to 312 over an account stream, in stream order and, for a lift, in rule order: the
    first record that breaks one, at the path that rule names. Every other record is passed over."""
    in_force: dict[str, int] = {}
    position = {r["event_id"]: i for i, r in enumerate(records)}

    def fail(i, path):
        return {"index": i, "reason": "schema", "path": path}

    for i, record in enumerate(records):
        kind, p = record["event_type"], record["payload"]
        if kind == "AccountRestrictionChanged":
            level = STRICTNESS[p["restriction"]]
            held = in_force.get(p["cause"])
            if held is not None and level <= held:
                return fail(i, "payload.cause")
            in_force[p["cause"]] = level
            continue
        if kind != "AccountRestrictionLifted":
            continue
        cause = p["cause"]
        if cause not in in_force and "310.cause" not in skip:
            return fail(i, "payload.cause")
        left = max((lvl for c, lvl in in_force.items() if c != cause), default=0)
        if STRICTNESS[p["restriction"]] != left and "310.restriction" not in skip:
            return fail(i, "payload.restriction")
        if not _acknowledged(records, position, i, p, skip):
            return fail(i, "payload.acknowledgment")
        if not _evidenced(records, position, i, p, skip):
            return fail(i, "payload.evidence")
        if "309.lift_resets" not in skip:
            in_force.pop(cause, None)
    return None


def _acknowledged(records, position, i, p, skip) -> bool:
    """Rule 311: the acknowledgment copy, earlier on the stream, is a version-2 restriction
    acknowledgment naming a record of the cause that stands to the lift, and unrefused. A lift ends
    its record's standing, so an acknowledgment lifts once."""
    at = position.get(p["acknowledgment"])
    if at is None or (at >= i and "311.before_lift" not in skip):
        return False
    copy_ = records[at]
    if copy_["event_type"] != "OwnerAcknowledged":
        return False
    if "311.subject" not in skip and (
        copy_["schema_version"] != 2 or copy_["payload"].get("subject") != "restriction"
    ):
        return False
    trigger_at = position.get(copy_["payload"]["event"])
    if trigger_at is None or (trigger_at >= at and "311.trigger_before_copy" not in skip):
        return False
    trigger = records[trigger_at]
    if trigger["event_type"] != "AccountRestrictionChanged":
        return False
    if trigger["payload"]["cause"] != p["cause"] and "311.cause" not in skip:
        return False
    stands_until = i if "311.stands_to_the_copy" not in skip else at
    if "311.stands" not in skip:
        for later in records[trigger_at + 1 : stands_until]:
            same = later["event_type"] in ("AccountRestrictionChanged", "AccountRestrictionLifted")
            if same and later["payload"]["cause"] == p["cause"]:
                return False
    if "311.refused" not in skip:
        original = copy_["causation_id"]
        for other in records[:i]:
            if (
                other["event_type"] == "OwnerCommandRefused"
                and other["payload"]["command"] == "acknowledge"
                and other["causation_id"] == original
            ):
                return False
    return True


def _evidenced(records, position, i, p, skip) -> bool:
    """Rule 312: the evidence completes the lift for its cause, in the order §7.3 says."""
    at = position.get(p["evidence"])
    if at is None or (at >= i and "312.before_lift" not in skip):
        return False
    evidence = records[at]
    ack_at = position[p["acknowledgment"]]
    if p["cause"] in BROKER:
        if evidence["event_type"] != "AccountStateObserved":
            return False
        if at <= ack_at and "312.after_ack" not in skip:
            return False
        if "312.latest" not in skip and any(r["event_type"] == "AccountStateObserved" for r in records[at + 1 : i]):
            return False
        return _clean(evidence["payload"], skip)
    if evidence["event_type"] != "ConnectionStateChanged" or evidence["payload"]["reason"] != "condition_cleared":
        return False
    trigger_at = position[records[ack_at]["payload"]["event"]]
    if "312.connection.order" not in skip and not trigger_at < at < ack_at:
        return False
    if "312.connection.degraded" not in skip:
        for later in records[at + 1 : i]:
            if later["event_type"] == "ConnectionStateChanged" and later["payload"]["to"] in ("degraded", "suspended"):
                return False
    if "312.connection.acknowledged" in skip:
        return True
    return any(
        r["event_type"] == "ConnectionStateChanged"
        and r["payload"]["reason"] == "acknowledged"
        and r["causation_id"] == p["acknowledgment"]
        for r in records[ack_at + 1 : i]
    )


# --------------------------------------------------------------------------- streams


def stream_cases() -> list[dict]:
    r = lambda n, restriction, cause: raised(uid("RS", n), restriction, cause)  # noqa: E731
    obs = lambda n, **row: observed(uid("AS", n), **row)  # noqa: E731
    cs = lambda n, frm, to, reason, by=None: connection(uid("CS", n), frm, to, reason, by)  # noqa: E731

    def lift(n, cause, ack, evidence, restriction):
        return lifted(uid("XT", n), cause, ack, evidence, restriction)

    rs, ob, cp, csid = (lambda n: uid("RS", n)), (lambda n: uid("AS", n)), (lambda n: uid("CP", n)), (lambda n: uid("CS", n))

    def case(name, clause, records, index=None, path=None):
        expect = (
            {"outcome": "Valid"}
            if index is None
            else {"outcome": "Invalid", "index": index, "reason": "schema", "path": path}
        )
        return {"name": name, "clause": clause, "records": records, "expect": expect}

    cause_path, state, ack, evid = "payload.cause", "payload.restriction", "payload.acknowledgment", "payload.evidence"
    broker = [r(1, CLOSING, REJECT), ack_copy(1, rs(1)), obs(1)]
    connected = [
        r(1, CLOSING, CONNECTION),
        cs(1, "degraded", "degraded", "condition_cleared"),
        ack_copy(1, rs(1)),
        cs(2, "degraded", "active", "acknowledged", uid("CP", 1)),
    ]
    out = [
        # valid
        case("a_reject_lifts_after_a_clean_report", f"{RULE_312}: acknowledged, then a clean report",
             [*broker, lift(1, REJECT, cp(1), ob(1), ACTIVE)]),
        case("a_block_lifts", f"{RULE_310}: a notice's block lifts to active",
             [r(1, BLOCKED, NOTICE), ack_copy(1, rs(1)), obs(1), lift(1, NOTICE, cp(1), ob(1), ACTIVE)]),
        case("one_cause_of_two_leaves_the_block", f"{RULE_310}: a reject's lift leaves a notice's block",
             [r(1, CLOSING, REJECT), r(2, BLOCKED, NOTICE), ack_copy(1, rs(1)), obs(1),
              lift(1, REJECT, cp(1), ob(1), BLOCKED)]),
        case("the_block_lifts_to_closing_only", f"{RULE_310}: a notice's lift leaves a reject's closing only",
             [r(1, CLOSING, REJECT), r(2, BLOCKED, NOTICE), ack_copy(1, rs(2)), obs(1),
              lift(1, NOTICE, cp(1), ob(1), CLOSING)]),
        case("a_cause_lifts_at_every_restriction", f"{RULE_310}: one lift for a cause raised at two",
             [r(1, CLOSING, NOTICE), r(2, BLOCKED, NOTICE), ack_copy(1, rs(2)), obs(1),
              lift(1, NOTICE, cp(1), ob(1), ACTIVE)]),
        case("the_connection_lifts_after_its_condition_clears", f"{RULE_312}: cleared, then acknowledged",
             [*connected, lift(1, CONNECTION, cp(1), csid(1), ACTIVE)]),
        case("a_cause_raised_again_after_its_lift", f"{RULE_309}: a lift ends the cause in force",
             [*broker, lift(1, REJECT, cp(1), ob(1), ACTIVE), r(2, CLOSING, REJECT)]),
        case("the_next_clean_report_lifts", f"{RULE_312}: a report still blocked lifts nothing; the next clean one does",
             [*broker[:2], obs(1, trading_blocked=True), obs(2), lift(1, REJECT, cp(1), ob(2), ACTIVE)]),
        case("a_second_acknowledgment_after_a_new_raise", f"{RULE_311}: the cause raised again is lifted by its own",
             [*broker, lift(1, REJECT, cp(1), ob(1), ACTIVE), r(2, CLOSING, REJECT), ack_copy(2, rs(2)), obs(2),
              lift(2, REJECT, cp(2), ob(2), ACTIVE)]),
        # rule 309, amended
        case("the_same_cause_twice_with_no_lift", f"{RULE_309}: still journaled once while in force",
             [r(1, CLOSING, REJECT), r(2, CLOSING, REJECT)], 1, cause_path),
        # rule 310
        case("a_lift_of_no_cause", f"{RULE_310}: no record raised it", [obs(1), lift(1, REJECT, cp(1), ob(1), ACTIVE)],
             1, cause_path),
        case("a_lift_of_another_cause", f"{RULE_310}: the cause in force is a reject",
             [*broker, lift(1, NOTICE, cp(1), ob(1), ACTIVE)], 3, cause_path),
        case("a_lift_twice", f"{RULE_310}: a lifted cause is no longer in force",
             [*broker, lift(1, REJECT, cp(1), ob(1), ACTIVE), lift(2, REJECT, cp(1), ob(1), ACTIVE)], 4, cause_path),
        case("active_beside_a_block", f"{RULE_310}: the notice's block is still in force",
             [r(1, CLOSING, REJECT), r(2, BLOCKED, NOTICE), ack_copy(1, rs(1)), obs(1),
              lift(1, REJECT, cp(1), ob(1), ACTIVE)], 4, state),
        case("closing_only_with_nothing_left", f"{RULE_310}: no cause is left, so the state is active",
             [*broker, lift(1, REJECT, cp(1), ob(1), CLOSING)], 3, state),
        case("closing_only_beside_a_block", f"{RULE_310}: the strictest left is the block",
             [r(1, CLOSING, REJECT), r(2, BLOCKED, NOTICE), ack_copy(1, rs(1)), obs(1),
              lift(1, REJECT, cp(1), ob(1), CLOSING)], 4, state),
        # rule 311
        case("an_unknown_acknowledgment", f"{RULE_311}: no copy has that id",
             [r(1, CLOSING, REJECT), obs(1), lift(1, REJECT, uid("CP", 9), ob(1), ACTIVE)], 2, ack),
        case("a_version_1_acknowledgment", f"{RULE_311}: a version-1 acknowledgment names a limit",
             [r(1, CLOSING, REJECT), ack_copy(1, rs(1), version=1), obs(1), lift(1, REJECT, cp(1), ob(1), ACTIVE)],
             3, ack),
        case("an_acknowledgment_of_a_limit", f"{RULE_311}: `subject` is `limit`",
             [r(1, CLOSING, REJECT), ack_copy(1, rs(1), "limit"), obs(1), lift(1, REJECT, cp(1), ob(1), ACTIVE)],
             3, ack),
        case("an_acknowledgment_of_another_cause", f"{RULE_311}: it names the notice's record",
             [r(1, CLOSING, REJECT), r(2, CLOSING, NOTICE), ack_copy(1, rs(2)), obs(1),
              lift(1, REJECT, cp(1), ob(1), CLOSING)], 4, ack),
        case("an_acknowledgment_naming_a_report", f"{RULE_311}: `event` names no restriction",
             [r(1, CLOSING, REJECT), obs(1), ack_copy(1, ob(1)), obs(2), lift(1, REJECT, cp(1), ob(2), ACTIVE)],
             4, ack),
        case("an_acknowledgment_before_its_restriction", f"{RULE_311}: the record named comes after the copy",
             [ack_copy(1, rs(1)), r(1, CLOSING, REJECT), obs(1), lift(1, REJECT, cp(1), ob(1), ACTIVE)], 3, ack),
        case("superseded_before_the_acknowledgment", f"{RULE_311}: the cause restricted further before it",
             [r(1, CLOSING, NOTICE), r(2, BLOCKED, NOTICE), ack_copy(1, rs(1)), obs(1),
              lift(1, NOTICE, cp(1), ob(1), ACTIVE)], 4, ack),
        case("raised_again_after_the_acknowledgment", f"{RULE_311}: the broker blocked after the owner acknowledged",
             [r(1, CLOSING, NOTICE), ack_copy(1, rs(1)), r(2, BLOCKED, NOTICE), obs(1),
              lift(1, NOTICE, cp(1), ob(1), ACTIVE)], 4, ack),
        case("a_refused_acknowledgment", f"{RULE_311}: the executor refused the copy",
             [*broker[:2], refused(1, uid("AK", 1)), obs(1), lift(1, REJECT, cp(1), ob(1), ACTIVE)], 4, ack),
        case("one_acknowledgment_twice", f"{RULE_311}: an acknowledgment lifts once",
             [*broker, lift(1, REJECT, cp(1), ob(1), ACTIVE), r(2, CLOSING, REJECT), obs(2),
              lift(2, REJECT, cp(1), ob(2), ACTIVE)], 6, ack),
        case("an_old_acknowledgment_after_a_new_raise", f"{RULE_311}: a lift came between its record and the copy",
             [*broker, lift(1, REJECT, cp(1), ob(1), ACTIVE), r(2, CLOSING, REJECT), ack_copy(2, rs(1)), obs(2),
              lift(2, REJECT, cp(2), ob(2), ACTIVE)], 7, ack),
        # rule 312, broker causes
        case("an_unknown_report", f"{RULE_312}: no record has that id",
             [*broker, lift(1, REJECT, cp(1), uid("AS", 9), ACTIVE)], 3, evid),
        case("a_report_before_the_acknowledgment", f"{RULE_312}: the refresh comes after the acknowledgment",
             [r(1, CLOSING, REJECT), obs(1), ack_copy(1, rs(1)), lift(1, REJECT, cp(1), ob(1), ACTIVE)], 3, evid),
        case("a_broker_lift_on_a_connection_record", f"{RULE_312}: a broker cause's evidence is a report",
             [*broker, cs(1, "degraded", "degraded", "condition_cleared"), lift(1, REJECT, cp(1), csid(1), ACTIVE)],
             4, evid),
        *[
            case(f"a_report_with_{member}", f"{RULE_312}: §7.3 row 1's `{member}` still restricts",
                 [*broker[:2], obs(1, **{member: bad}), lift(1, REJECT, cp(1), ob(1), ACTIVE)], 3, evid)
            for member, bad in (("status", "ACCOUNT_UPDATED"), ("trading_blocked", True), ("account_blocked", True),
                                ("trade_suspended_by_user", True))
        ],
        # rule 312, the connection
        case("the_connection_without_its_condition_cleared", f"{RULE_312}: a report is not the connection's evidence",
             [r(1, CLOSING, CONNECTION), ack_copy(1, rs(1)), obs(1), lift(1, CONNECTION, cp(1), ob(1), ACTIVE)],
             3, evid),
        case("the_connection_cleared_after_the_acknowledgment", f"{RULE_312}: cleared first, then acknowledged",
             [r(1, CLOSING, CONNECTION), ack_copy(1, rs(1)), cs(1, "degraded", "degraded", "condition_cleared"),
              cs(9, "degraded", "active", "acknowledged", uid("CP", 1)), lift(1, CONNECTION, cp(1), csid(1), ACTIVE)], 4, evid),
        case("the_connection_cleared_before_its_restriction", f"{RULE_312}: a clearance before the cause is not its",
             [cs(1, "degraded", "degraded", "condition_cleared"), r(1, CLOSING, CONNECTION), ack_copy(1, rs(1)),
              cs(9, "degraded", "active", "acknowledged", uid("CP", 1)), lift(1, CONNECTION, cp(1), csid(1), ACTIVE)], 4, evid),
        case("the_connection_degraded_again", f"{RULE_312}: no degradation after the clearance",
             [*connected[:3], cs(3, "degraded", "degraded", "network_errors"), cs(9, "degraded", "active", "acknowledged", uid("CP", 1)),
              lift(1, CONNECTION, cp(1), csid(1), ACTIVE)], 5, evid),
        case("the_connection_suspended_again", f"{RULE_312}: nor a suspension",
             [*connected[:3], cs(3, "degraded", "suspended", "credential_expired"),
              cs(9, "suspended", "active", "acknowledged", uid("CP", 1)), lift(1, CONNECTION, cp(1), csid(1), ACTIVE)],
             5, evid),
        case("the_connection_without_its_acknowledged_change", f"{RULE_312}: the connection's `acknowledged` change comes first",
             [*connected[:3], lift(1, CONNECTION, cp(1), csid(1), ACTIVE)], 3, evid),
        case("the_connection_acknowledged_by_another", f"{RULE_312}: the `acknowledged` change is caused by this copy",
             [*connected[:3], cs(9, "degraded", "active", "acknowledged", uid("CP", 7)),
              lift(1, CONNECTION, cp(1), csid(1), ACTIVE)], 4, evid),
        case("the_connection_degraded_before_the_acknowledgment", f"{RULE_312}: no degradation after the clearance",
             [r(1, CLOSING, CONNECTION), cs(1, "degraded", "degraded", "condition_cleared"),
              cs(3, "degraded", "degraded", "network_errors"), ack_copy(1, rs(1)), cs(9, "degraded", "active", "acknowledged", uid("CP", 1)),
              lift(1, CONNECTION, cp(1), csid(1), ACTIVE)], 5, evid),
        case("a_clean_report_contradicted", f"{RULE_312}: the evidence is the latest report before the lift",
             [r(1, BLOCKED, NOTICE), ack_copy(1, rs(1)), obs(1), obs(2, account_blocked=True),
              lift(1, NOTICE, cp(1), ob(1), ACTIVE)], 4, evid),
        case("a_report_after_the_lift", f"{RULE_312}: the evidence comes before the lift",
             [*broker[:2], lift(1, REJECT, cp(1), ob(1), ACTIVE), obs(1)], 2, evid),
        case("an_acknowledgment_after_the_lift", f"{RULE_311}: the acknowledgment comes before the lift",
             [r(1, CLOSING, REJECT), obs(1), lift(1, REJECT, cp(1), ob(1), ACTIVE), ack_copy(1, rs(1))], 2, ack),
        case("two_closing_causes_one_lifts", f"{RULE_310}: a lift beside an equal cause leaves the state unchanged",
             [r(1, CLOSING, REJECT), r(2, CLOSING, NOTICE), ack_copy(1, rs(1)), obs(1),
              lift(1, REJECT, cp(1), ob(1), CLOSING)]),
        case("the_connection_on_an_acknowledged_change", f"{RULE_312}: the evidence is `condition_cleared`",
             [*connected, lift(1, CONNECTION, cp(1), csid(2), ACTIVE)], 4, evid),
    ]
    return out


# --------------------------------------------------------------------------- independent oracle


def oracle(records: list[dict]) -> dict | None:
    """Every rule judged apart from `stream_failure`: the stream is first read into a timeline of
    each cause's raises and lifts, each acknowledgment's refusal, and each record's place; then every
    restriction and lift is judged against that timeline, and the earliest failure reported. Rule
    order within a lift is 310's cause, 310's state, 311, then 312."""
    by_id = {r["event_id"]: (i, r) for i, r in enumerate(records)}
    events = [(i, r["event_type"], r["payload"]) for i, r in enumerate(records)]
    raises = [(i, p["cause"], STRICTNESS[p["restriction"]]) for i, t, p in events if t == "AccountRestrictionChanged"]
    lifts = [(i, p) for i, t, p in events if t == "AccountRestrictionLifted"]

    def last_lift_before(cause, at):
        return max((i for i, p in lifts if p["cause"] == cause and i < at), default=-1)

    def in_force_at(at):
        """Each cause's strictest level raised after its last lift before `at`."""
        out = {}
        for i, cause, level in raises:
            if i < at and i > last_lift_before(cause, at):
                out[cause] = max(out.get(cause, 0), level)
        return out

    failures = []
    for i, cause, level in raises:
        if in_force_at(i).get(cause, 0) >= level:
            failures.append((i, "payload.cause"))
    for i, p in lifts:
        cause = p["cause"]
        forced = in_force_at(i)
        if cause not in forced:
            failures.append((i, "payload.cause"))
            continue
        left = max([lvl for c, lvl in forced.items() if c != cause] + [0])
        if left != STRICTNESS[p["restriction"]]:
            failures.append((i, "payload.restriction"))
            continue
        a = by_id.get(p["acknowledgment"])
        trigger = by_id.get(a[1]["payload"]["event"]) if a and a[1]["event_type"] == "OwnerAcknowledged" else None
        good_ack = (
            a is not None
            and a[0] < i
            and a[1]["schema_version"] == 2
            and a[1]["payload"].get("subject") == "restriction"
            and trigger is not None
            and trigger[0] < a[0]
            and trigger[1]["event_type"] == "AccountRestrictionChanged"
            and trigger[1]["payload"]["cause"] == cause
            and not any(trigger[0] < j < i for j, c, _ in raises if c == cause)
            and not any(trigger[0] < j < i for j, q in lifts if q["cause"] == cause)
            and a[1]["causation_id"] not in refused_originals_before(records, i)
        )
        if not good_ack:
            failures.append((i, "payload.acknowledgment"))
            continue
        e = by_id.get(p["evidence"])
        if cause in BROKER:
            last_report = next((j for j in range(i - 1, -1, -1) if events[j][1] == "AccountStateObserved"), None)
            good_evidence = (
                e is not None
                and e[0] == last_report
                and a[0] < e[0]
                and all(e[1]["payload"][m] == CLEAN[m] for m in ROW_ONE)
            )
        else:
            acknowledged_changes = [
                j for j, t, q in events
                if t == "ConnectionStateChanged" and q["reason"] == "acknowledged"
                and records[j]["causation_id"] == p["acknowledgment"]
            ]
            good_evidence = (
                e is not None
                and trigger[0] < e[0] < a[0]
                and e[1]["event_type"] == "ConnectionStateChanged"
                and e[1]["payload"]["reason"] == "condition_cleared"
                and not any(
                    t == "ConnectionStateChanged" and q["to"] in ("degraded", "suspended")
                    for j, t, q in events
                    if e[0] < j < i
                )
                and any(a[0] < j < i for j in acknowledged_changes)
            )
        if not good_evidence:
            failures.append((i, "payload.evidence"))
            continue
    if not failures:
        return None
    index, path = min(failures)
    return {"index": index, "reason": "schema", "path": path}


def refused_originals_before(records: list[dict], at: int) -> set[str]:
    return {
        r["causation_id"]
        for r in records[:at]
        if r["event_type"] == "OwnerCommandRefused" and r["payload"]["command"] == "acknowledge"
    }


# --------------------------------------------------------------------------- fuzz


def fuzz_stream(rng: random.Random, length: int) -> list[dict]:
    """A random account stream of raises, acknowledgments, refusals, reports, connection changes
    and lifts. About half the lifts are built to pass, from the generator's own count of the causes
    in force and the records each cause's lift needs, and the rest name what is at hand, so the
    stream mixes lifts that pass with lifts that break each rule, and noise between them."""
    out: list[dict] = []
    in_force: dict[str, int] = {}
    n = 0

    def add(record):
        out.append(record)
        if record["event_type"] == "AccountRestrictionChanged":
            p = record["payload"]
            in_force[p["cause"]] = max(in_force.get(p["cause"], 0), STRICTNESS[p["restriction"]])
        if record["event_type"] == "AccountRestrictionLifted":
            in_force.pop(record["payload"]["cause"], None)

    def latest(kind, cause=None):
        for r in reversed(out):
            if r["event_type"] == kind and (cause is None or r["payload"]["cause"] == cause):
                return r
        return None

    for _ in range(length):
        n += 1
        kind = rng.choice(["raise", "ack", "report", "connection", "lift", "lift", "refuse", "scenario"])
        acks = [r for r in out if r["event_type"] == "OwnerAcknowledged"]
        if kind == "raise":
            cause = rng.choice(CAUSES)
            add(raised(uid("RS", n), BLOCKED if cause == NOTICE and rng.random() < 0.4 else CLOSING, cause))
        elif kind == "ack" and in_force:
            trigger = latest("AccountRestrictionChanged", rng.choice(sorted(in_force)))
            add(ack_copy(n, trigger["event_id"], rng.choice(["restriction", "restriction", "limit"])))
        elif kind == "refuse" and acks:
            add(refused(n, rng.choice(acks)["causation_id"]))
        elif kind == "report":
            add(observed(uid("AS", n), **({rng.choice(ROW_ONE[1:]): True} if rng.random() < 0.3 else {})))
        elif kind == "connection":
            reason = rng.choice(["condition_cleared", "condition_cleared", "network_errors"])
            add(connection(uid("CS", n), "degraded", "degraded", reason))
        elif kind == "scenario":
            cause = rng.choice(CAUSES)
            if cause not in in_force:
                add(raised(uid("RS", n), CLOSING, cause))
            trigger = latest("AccountRestrictionChanged", cause)
            if cause == CONNECTION:
                add(connection(uid("CS", n), "degraded", "degraded", "condition_cleared"))
            add(ack_copy(n, trigger["event_id"]))
            if cause == CONNECTION:
                add(connection(uid("CK", n), "degraded", "active", "acknowledged", uid("CP", n)))
            else:
                add(observed(uid("AS", n)))
        elif kind == "lift" and acks:
            good = rng.random() < 0.5
            a = rng.choice(acks) if not good else acks[-1]
            named = next((r for r in out if r["event_id"] == a["payload"]["event"]), None)
            cause = named["payload"]["cause"] if named is not None and named["event_type"] == "AccountRestrictionChanged" and (good or rng.random() < 0.8) else rng.choice(CAUSES)
            pool = [r for r in out if r["event_type"] in ("AccountStateObserved", "ConnectionStateChanged")]
            if not pool:
                continue
            if good:
                if cause == CONNECTION:
                    before = out[: out.index(a)]
                    fitting = [r for r in before if r["event_type"] == "ConnectionStateChanged"
                               and r["payload"]["reason"] == "condition_cleared"]
                else:
                    fitting = [r for r in pool if r["event_type"] == "AccountStateObserved"]
                evidence = (fitting or pool)[-1]
                left = max([lvl for c, lvl in in_force.items() if c != cause] + [0])
                state = next(k for k, v in STRICTNESS.items() if v == left)
            else:
                evidence = rng.choice(pool)
                state = rng.choice([ACTIVE, ACTIVE, CLOSING, BLOCKED])
            add(lifted(uid("XT", n), cause, a["event_id"], evidence["event_id"], state))
    return out


FUZZ_SEED, FUZZ_STREAMS, FUZZ_LENGTH = 2824, 400, 12


def agent_entries(records: list[dict], skip: frozenset[str] = frozenset()) -> list[tuple[int, set[int]]]:
    """§9.5's agent effects over a stream the rules admit, as `(account state, account entries
    held)` after each record. A raise that makes the state stricter applies the new state's entry; a
    lift that changes the state applies the state left's entry unless held, then removes every other
    account entry. `agents.remove_stricter_only` seeds the bug of removing only the state lifted
    from."""
    in_force: dict[str, int] = {}
    held: set[int] = set()
    out = []
    for record in records:
        before = max(in_force.values(), default=0)
        kind, p = record["event_type"], record["payload"]
        if kind == "AccountRestrictionChanged":
            in_force[p["cause"]] = max(in_force.get(p["cause"], 0), STRICTNESS[p["restriction"]])
            after = max(in_force.values(), default=0)
            if after > before:
                held.add(after)
        elif kind == "AccountRestrictionLifted":
            in_force.pop(p["cause"], None)
            after = max(in_force.values(), default=0)
            if after != before:
                if after:
                    held.add(after)
                if "agents.remove_stricter_only" in skip:
                    held.discard(before)
                else:
                    held = {after} if after else set()
        out.append((max(in_force.values(), default=0), set(held)))
    return out


def agents_failure(records: list[dict], skip: frozenset[str] = frozenset()) -> str | None:
    """Invariant 6 checked after every record: the strictest account entry the agents hold is the
    account state, never looser and never left over once its causes are lifted."""
    for i, (state, held) in enumerate(agent_entries(records, skip)):
        if max(held, default=0) != state:
            return f"record {i}: state {state}, entries {sorted(held)}"
    return None


def fuzz_disagreements(skip: frozenset[str] = frozenset()) -> tuple[list[str], int]:
    """Random streams on which the rules and the oracle disagree, and how many streams had a valid
    lift, so the fuzz is shown to reach lifts that pass as well as those refused."""
    rng = random.Random(FUZZ_SEED)
    problems, passed = [], 0
    for k in range(FUZZ_STREAMS):
        records = fuzz_stream(rng, FUZZ_LENGTH)
        got, want = stream_failure(records, skip), oracle(records)
        if got != want:
            problems.append(f"fuzz stream {k}: rules {got}, oracle {want}")
        limit = len(records) if want is None else want["index"]
        agents = agents_failure(records[:limit], skip)
        if agents:
            problems.append(f"fuzz stream {k}: invariant 6 breaks at {agents}")
        passed += any(r["event_type"] == "AccountRestrictionLifted" for r in records[:limit])
    return problems, passed


# --------------------------------------------------------------------------- checks

ORACLE_CHECKS = (
    "drafts.valid",
    "invalid_drafts",
    "valid_drafts",
    "streams.records",
    "streams.rule",
    "streams.oracle",
    "streams.agents",
    "streams.fuzz",
)
FUZZ_MIN_PASSING = 20


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def expected(case: dict) -> dict | None:
    e = case["expect"]
    return None if e["outcome"] == "Valid" else {k: e[k] for k in ("index", "reason", "path")}


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
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
        for i, record in enumerate(case["records"]):
            got = violations(record)
            if got:
                problems.append(found("streams.records", f"{case['name']} record {i}: {got}"))
        if stream_failure(case["records"]) != expected(case):
            problems.append(found("streams.rule", f"{case['name']}: expected {case['expect']}"))
        if oracle(case["records"]) != expected(case):
            problems.append(found("streams.oracle", f"{case['name']}: the oracle disagrees"))
        admitted = case["records"] if expected(case) is None else case["records"][: expected(case)["index"]]
        agents = agents_failure(admitted)
        if agents:
            problems.append(found("streams.agents", f"{case['name']}: invariant 6 breaks at {agents}"))
    fuzzed, passed = fuzz_disagreements()
    problems += [found("streams.fuzz", p) for p in fuzzed]
    if passed < FUZZ_MIN_PASSING:
        problems.append(found("streams.fuzz", f"only {passed} fuzz streams reached a lift that passes"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

STREAM_MUTANTS = (
    "309.lift_resets",
    "310.cause",
    "310.restriction",
    "311.before_lift",
    "311.trigger_before_copy",
    "311.cause",
    "311.subject",
    "311.stands",
    "311.stands_to_the_copy",
    "311.refused",
    "312.before_lift",
    "312.after_ack",
    "312.latest",
    "312.connection.acknowledged",
    "312.connection.order",
    "312.connection.degraded",
    *(f"312.clean.{member}" for member in ROW_ONE),
)


AGENT_MUTANTS = ("agents.remove_stricter_only",)


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    """Seeded bugs in the vectors, each registered against the one check that must reject it."""

    def mutated(fn) -> dict:
        out = copy.deepcopy(section)
        fn(out)
        return out

    def case(s, kind, name):
        return next(c for c in s[kind] if c["name"] == name)

    return [
        ("the broker lift's state is unknown", "drafts.valid",
         mutated(lambda s: s["drafts"][BROKER_LIFT]["payload"].update(restriction="normal"))),
        ("an invalid draft's expectation differs", "invalid_drafts",
         mutated(lambda s: case(s, "invalid_drafts", "lift.cause_null")["expect"].update(path="payload.evidence"))),
        ("a valid draft drops its subject", "valid_drafts",
         mutated(lambda s: case(s, "valid_drafts", "acknowledged_v2.limit")["changes"].append(delete("payload.subject")))),
        ("a stream record blocks on a reject", "streams.records",
         mutated(lambda s: case(s, "streams", "a_block_lifts")["records"][0]["payload"].update(cause="broker_reject"))),
        ("a stream's expectation differs", "streams.rule",
         mutated(lambda s: case(s, "streams", "a_lift_twice")["expect"].update(outcome="Valid"))),
        ("a stream's expected path differs", "streams.oracle",
         mutated(lambda s: case(s, "streams", "a_refused_acknowledgment")["expect"].update(path="payload.evidence"))),
    ]


def run_mutants(section: dict) -> list[str]:
    """Every seeded bug caught: a stream mutant by a vector stream or the fuzz, and a vector mutant
    only by the check it is registered against."""
    escaped = []
    for mutant in STREAM_MUTANTS:
        skip = frozenset([mutant])
        caught = any(stream_failure(s["records"], skip) != expected(s) for s in section["streams"])
        if not caught:
            escaped.append(f"restriction lift stream mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if check not in ("streams.fuzz", "streams.agents") and not any(c == check for _, c, _ in registered):
            escaped.append(f"restriction lift check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"restriction lift vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    for mutant in AGENT_MUTANTS:
        caught = any(
            agents_failure(c["records"] if expected(c) is None else c["records"][: expected(c)["index"]], frozenset([mutant]))
            for c in section["streams"]
        )
        if not caught:
            escaped.append(f"restriction lift agent mutant {mutant}")
    for mutant in ("310.restriction", "311.stands", "312.connection.degraded"):
        fuzzed, _ = fuzz_disagreements(frozenset([mutant]))
        if not fuzzed:
            escaped.append(f"restriction lift fuzz misses mutant {mutant}")
    return escaped


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
        "streams": stream_cases(),
    }
