"""Journal spec v0.23 §9.11's reference vectors (DEC-672): the hold on new openings (DEC-191). They
are the `hold_openings` and `lift_hold` owner commands on the control stream, and the agent
runtime's two copies of them on the agent stream: `AgentModeChanged` version 2, which records the
hold, and `OwnerCommandRefused` version 2, which can refuse a lift.

The rules (90 to 95, and rule 26's new command) live in `control.py`, so one validator judges every
closed schema. This module builds the `hold` section and checks it with three oracles of its own:
the strictest mode each copy may leave, recomputed from mandate spec §5.9 and §6.1 by its own
ranking; that each command is copied at most once; and that a hold is a person's and a lift is a
user's. Every seeded bug is shown caught.
"""

from __future__ import annotations

import copy

from common import change, delete
from control import (
    AGENT,
    AGENT_STREAM,
    OWNER,
    RUNTIME,
    SERVICES,
    STREAM,
    USER,
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

SPEC = "docs/specs/journal.md v0.23 §9.11 (DEC-672)"
AT = "2026-10-08T16:00:00.000000000Z"
IDS = {
    "hold": "01J8Z6H0A000000000000000H1",
    "lift": "01J8Z6H1A000000000000000H2",
    "held": "01J8Z6H2A000000000000000H3",
    "lifted": "01J8Z6H3A000000000000000H4",
    "refused": "01J8Z6H4A000000000000000H5",
}
SECOND_LIFT = "01J8Z6H1B000000000000000H6"
AGENT_ACTOR = {"kind": "agent", "id": AGENT, "version": "0.1.0", "build": "sha256:" + "c" * 64}
BROKER = {"kind": "broker", "id": "broker_01", "version": "1", "build": None}
CLIENT = {"kind": "client", "id": "client_01", "version": "1", "build": None, "on_behalf_of": OWNER}
SUBMITTED_AT = 1791475200
STEP_UP = {"assertion_id": "assert_owner_09", "authenticated_at": 1791475190, "method": "cli_confirm"}
ACCOUNT_STREAM = "acct:ws_01J8Z2:01J8Z2ACCT00000000000000A1"


def envelope(name, event_type, actor, payload, causation=None, version=1, stream=STREAM) -> dict:
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": IDS[name],
        "stream_id": stream,
        "event_type": event_type,
        "schema_version": version,
        "event_time": AT,
        "clock_source": "local",
        "causation_id": causation,
        "correlation_id": None,
        "actor": dict(actor),
        "config_refs": {},
        "payload": payload,
        "artifact_refs": [],
        "pii_refs": [],
    }


def command(name: str, submitted_at: int, step_up) -> dict:
    """An agent-scope command with the CLI's members (`agent::issued`): the bid and release members
    are an owner exit's and a Stop's, so they are `null` here."""
    return {
        "agent": AGENT,
        "command": name,
        "scope": "agent",
        "subject": AGENT,
        "release": None,
        "warning_shown": None,
        "bid": None,
        "bid_size": None,
        "floor": None,
        "user": OWNER,
        "submitted_at": submitted_at,
        "step_up": step_up,
    }


def mode(from_, to, reason, lifecycle, held) -> dict:
    return {"from": from_, "to": to, "reason": reason, "lifecycle": lifecycle, "held": held}


def base_drafts() -> dict[str, dict]:
    hold = command("hold_openings", SUBMITTED_AT, None)
    lift = command("lift_hold", SUBMITTED_AT + 60, dict(STEP_UP))
    refused = {"command": "lift_hold", "reason": "step_up_stale", "effective_at": "2026-10-08T16:06:00.000000000Z"}
    agent = {"stream": AGENT_STREAM, "version": 2}
    return {
        "hold": envelope("hold", "OwnerCommandIssued", USER, hold),
        "lift": envelope("lift", "OwnerCommandIssued", USER, lift),
        "held": envelope(
            "held", "AgentModeChanged", RUNTIME, mode("normal", "exits_only", "owner_hold", "normal", True), IDS["hold"], **agent
        ),
        "lifted": envelope(
            "lifted", "AgentModeChanged", RUNTIME, mode("exits_only", "normal", "owner_lift_hold", "normal", False), IDS["lift"], **agent
        ),
        "lift_refused": envelope("refused", "OwnerCommandRefused", RUNTIME, refused, SECOND_LIFT, **agent),
    }


def invalid(name, clause, base, changes, reason, path, also=()):
    return control_invalid(name, clause, base, changes, reason, path, also)


def valid(name, clause, base, changes):
    return control_valid(name, clause, base, changes)


MEMBER_CASES = {
    "hold": (
        ("agent", 7, "agent a"),
        ("command", True, None),
        ("scope", 7, "connection"),
        ("subject", 7, "agent a"),
        ("release", True, None),
        ("bid", "101.5", None),
        ("submitted_at", "2026-10-08T16:00:00.000000000Z", None),
        ("step_up", "cli_confirm", None),
        ("user", 7, ""),
    ),
    "held": (
        ("from", 7, "halted"),
        ("to", 7, "held"),
        ("reason", 7, "owner_halt"),
        ("lifecycle", 7, "exits_only"),
        ("held", "true", None),
    ),
    "lift_refused": (
        ("command", 7, "pause"),
        ("reason", 7, "late"),
        ("effective_at", 1791475560, "2026-10-08T16:06:00Z"),
    ),
}
# `held` on a hold's copy is rule 93's, and `user` and `subject` rule 91's, at the member's own path with the same
# reason, so a mistyped or `null` value is refused identically whether or not its type is checked.
RULE_TYPED = ("held", "user", "subject")
NULLED = {
    "hold": ("agent", "command", "scope", "subject", "submitted_at", "user"),
    "held": ("from", "to", "reason", "lifecycle", "held"),
    "lift_refused": ("command", "reason", "effective_at"),
}


def member_drafts() -> list[dict]:
    out = []
    for base, cases in MEMBER_CASES.items():
        for member, wrong_kind, wrong_form in cases:
            path = f"payload.{member}"
            out.append(invalid(f"{base}.{member}.kind", "§9.11 types", base, [change(path, wrong_kind)], "schema", path))
            if wrong_form is not None:
                out.append(
                    invalid(f"{base}.{member}.form", "§9.11 types", base, [change(path, wrong_form)], "non_canonical", path)
                )
        for member in NULLED[base]:
            path = f"payload.{member}"
            out.append(invalid(f"{base}.{member}.null", "§9.11 types", base, [change(path, None)], "schema", path))
        some = MEMBER_CASES[base][0][0]
        out.append(invalid(f"{base}.missing", "§9.11 closed", base, [delete(f"payload.{some}")], "schema", f"payload.{some}"))
        out.append(invalid(f"{base}.extra", "§9.11 closed", base, [change("payload.note", "x")], "schema", "payload.note"))
    return out


def invalid_drafts() -> list[dict]:
    return [
        *member_drafts(),
        invalid(
            "lift_step_up_as_a_timestamp",
            "§9.11: times are integer risk-clock seconds, as §9.7's",
            "lift",
            [change("payload.step_up.authenticated_at", "2026-10-08T15:59:50.000000000Z")],
            "schema",
            "payload.step_up.authenticated_at",
        ),
        invalid(
            "mode_changed_unknown_version",
            "§9.11: version 2 is registered here; version 1 is §9.1's",
            "held",
            [change("schema_version", 3)],
            "unknown_schema",
            "payload",
        ),
        invalid(
            "refused_v1_lift",
            "§9.11: version 1's command list stays closed",
            "lift_refused",
            [change("schema_version", 1)],
            "non_canonical",
            "payload.command",
        ),
        invalid(
            "refused_lift_on_the_account_stream",
            "rule 26: the runtime judges a lift, so version 2 is the agent stream's",
            "lift_refused",
            [change("stream_id", ACCOUNT_STREAM)],
            "unknown_schema",
            "payload",
        ),
        invalid(
            "lift_refused_not_independent",
            "rule 28: a lift is refused only for its step-up",
            "lift_refused",
            [change("payload.reason", "not_independent")],
            "stream_mismatch",
            "payload.reason",
        ),
        invalid(
            "client_lifts_a_hold",
            "rule 90: lifting is the owner's alone (DEC-191)",
            "lift",
            [change("actor", dict(CLIENT))],
            "schema",
            "actor.kind",
        ),
        invalid(
            "client_pauses",
            "rule 90: a client issues no command but a hold (DEC-191)",
            "hold",
            [change("actor", dict(CLIENT)), change("payload.command", "pause")],
            "schema",
            "actor.kind",
        ),
        invalid(
            "client_kill_switch",
            "rule 90: the kill switch stays the human's (DEC-141 item 3)",
            "hold",
            [*kill_switch(CLIENT)],
            "schema",
            "actor.kind",
        ),
        invalid(
            "open_command_without_a_command",
            "§9.11: every command names itself",
            "hold",
            [change("payload.command", 7)],
            "schema",
            "payload.command",
        ),
        *(
            invalid(
                f"lift_by_{name}",
                "rule 90: only a user lifts a hold",
                "lift",
                [change("actor", dict(actor))],
                "schema",
                "actor.kind",
            )
            for name, actor in (("the_system", SERVICES), ("an_agent", AGENT_ACTOR), ("a_broker", BROKER))
        ),
        *(
            invalid(
                f"hold_by_{name}",
                "rule 90: a hold is a user's or a client's",
                "hold",
                [change("actor", dict(actor))],
                "schema",
                "actor.kind",
            )
            for name, actor in (("an_agent", AGENT_ACTOR), ("a_broker", BROKER))
        ),
        invalid(
            "client_hold_with_a_build",
            "rule 82: a client is external, so it has no build digest",
            "hold",
            [change("actor", dict(CLIENT) | {"build": "sha256:" + "d" * 64})],
            "schema",
            "actor.build",
        ),
        invalid(
            "version_1_mode_change_with_a_hold",
            "§9.11: version 1's reasons stay closed; a hold is version 2's",
            "held",
            [change("schema_version", 1), delete("payload.held")],
            "non_canonical",
            "payload.reason",
        ),
        invalid(
            "hold_by_the_system",
            "rule 90: a hold is a person's command",
            "hold",
            [change("actor", dict(SERVICES))],
            "schema",
            "actor.kind",
        ),
        invalid(
            "client_holds_on_an_agent_stream",
            "rule 83: a client writes only to the control stream",
            "held",
            [change("actor", dict(CLIENT))],
            "schema",
            "actor.kind",
        ),
        invalid(
            "hold_names_another_user",
            "rule 91: the command names its human",
            "hold",
            [change("payload.user", "user_owner_02")],
            "schema",
            "payload.user",
        ),
        invalid(
            "client_hold_names_its_own_id",
            "rule 91: a client's command names the user it acts for",
            "hold",
            [change("actor", dict(CLIENT)), change("payload.user", "client_01")],
            "schema",
            "payload.user",
        ),
        invalid(
            "hold_for_another_agent",
            "rule 91: an agent-scope command's subject is its agent",
            "hold",
            [change("payload.subject", "agent_b")],
            "schema",
            "payload.subject",
        ),
        invalid(
            "hold_with_step_up",
            "rule 92: a hold needs no step-up and carries none",
            "hold",
            [change("payload.step_up", dict(STEP_UP))],
            "schema",
            "payload.step_up",
        ),
        invalid(
            "hold_copied_as_not_held",
            "rule 93",
            "held",
            [change("payload.held", False), change("payload.to", "normal")],
            "schema",
            "payload.held",
        ),
        invalid(
            "lift_copied_as_held",
            "rule 93",
            "lifted",
            [change("payload.held", True), change("payload.to", "exits_only")],
            "schema",
            "payload.held",
        ),
        invalid(
            "resume_clears_the_hold",
            "rule 94: a resume lifts only the pause, never the hold (MI-3)",
            "held",
            [change("payload.from", "paused"), change("payload.to", "normal"), change("payload.reason", "owner_resume")],
            "schema",
            "payload.to",
        ),
        invalid(
            "lift_clears_the_pause",
            "rule 94: a lift lifts only the hold",
            "lifted",
            [change("payload.from", "paused"), change("payload.lifecycle", "paused")],
            "schema",
            "payload.to",
        ),
        invalid(
            "hold_copied_without_its_command",
            "rule 95",
            "held",
            [change("causation_id", None)],
            "schema",
            "causation_id",
        ),
        invalid(
            "lift_copied_without_its_command",
            "rule 95: a lift's copy names its command",
            "lifted",
            [change("causation_id", None)],
            "schema",
            "causation_id",
        ),
        invalid(
            "copy_with_a_null_hold",
            "§9.11 types: `held` is a boolean on every version-2 copy",
            "held",
            [change("payload.reason", "restriction_changed"), change("causation_id", None), change("payload.held", None)],
            "schema",
            "payload.held",
        ),
        invalid(
            "resume_copied_without_its_command",
            "rule 95: every owner copy at version 2",
            "lifted",
            [change("payload.reason", "owner_resume"), change("causation_id", None)],
            "schema",
            "causation_id",
        ),
    ]


def kill_switch(actor: dict) -> list[dict]:
    """The hold base turned into a connection kill switch, with the CLI's members."""
    return [
        change("actor", dict(actor)),
        change("payload.command", "kill_switch"),
        change("payload.scope", "connection"),
        change("payload.subject", "conn_01"),
        change("payload.agent", None),
    ]


def valid_drafts() -> list[dict]:
    return [
        valid(
            "version_1_mode_change_unchanged",
            "§9.11: version 1 stays registered",
            "held",
            [
                change("schema_version", 1),
                delete("payload.held"),
                change("payload.reason", "owner_pause"),
                change("payload.to", "paused"),
                change("payload.lifecycle", "paused"),
            ],
        ),
        valid(
            "kill_switch_stays_open",
            "§9.11: other commands are not closed here, and a kill switch is always recorded",
            "hold",
            kill_switch(USER),
        ),
        valid(
            "host_cli_kill_switch",
            "§9.11, rule 90: the host CLI's `system` kill switch is never refused (rule 13)",
            "hold",
            kill_switch(SERVICES),
        ),
        valid(
            "pause_with_any_members",
            "§9.11: an open command's other members are not judged here",
            "hold",
            [change("payload.command", "pause"), change("payload.note", "x")],
        ),
        valid(
            "client_holds",
            "rules 83, 90 and 91: a client may hold (DEC-191)",
            "hold",
            [change("actor", dict(CLIENT))],
        ),
        valid(
            "lift_without_step_up",
            "§9.11: the runtime refuses it as step_up_missing; the journal accepts it",
            "lift",
            [change("payload.step_up", None)],
        ),
        valid(
            "lift_while_a_limit_is_latched",
            "rule 94: a lift never clears a latched limit, so the mode may stay exits_only",
            "lifted",
            [change("payload.to", "exits_only")],
        ),
        valid(
            "resume_while_held",
            "rule 94: the resume leaves the hold",
            "held",
            [change("payload.from", "paused"), change("payload.reason", "owner_resume")],
        ),
        valid(
            "hold_while_paused",
            "rule 94: the mode stays paused and the hold is recorded",
            "held",
            [change("payload.from", "paused"), change("payload.to", "paused"), change("payload.lifecycle", "paused")],
        ),
        valid(
            "hold_after_stop",
            "rule 94: a stopped agent records the hold and stays stopped",
            "held",
            [change("payload.from", "stopped"), change("payload.to", "stopped"), change("payload.lifecycle", "stopped")],
        ),
        valid(
            "restriction_lifts_under_a_hold",
            "rules 94 and 95: not an owner copy, and the hold keeps exits_only",
            "held",
            [change("payload.reason", "restriction_changed"), change("payload.from", "paused"), change("causation_id", None)],
        ),
        valid(
            "lift_refused_for_no_step_up",
            "rule 26: a lift's refusal is the agent stream's",
            "lift_refused",
            [change("payload.reason", "step_up_missing")],
        ),
    ]


# --------------------------------------------------------------------------- §11's held_mismatch

HOLD_REASONS = ("owner_hold", "owner_lift_hold")


def held_mismatches(
    events: list[dict], from_seq: int, anchor: dict | None, skip: frozenset[str] = frozenset()
) -> list[int]:
    """§11's `held_mismatch` over one agent-stream range in `seq` order from `from_seq`: the index
    of every record that breaks it. Only `AgentModeChanged` records are read; any other event in
    between leaves the hold as it was.

    The check anchors on the stored chain before the range, which the caller derives and supplies:
    `anchor` is `{"v2_before": false}` when no version-2 `AgentModeChanged` precedes the range,
    `{"v2_before": true, "held": h}` for the `held` the last one carried, or `None` when the
    caller cannot tell. A range from seq 1 is anchored on nothing before it. The `held` carried is
    the expected one, derived from the reasons: a hold sets it, a lift clears it, and every other
    record keeps it whatever it wrote. So:

    - a hold or lift whose `held` contradicts its reason fails;
    - any other version-2 record whose `held` differs from the carried one fails, and with no
      anchor and no hold or lift yet in the range it fails closed, as having no anchor;
    - a version-1 record after any version-2 one, before the range or in it, fails. With no anchor
      a version-1 record before the range's first version-2 one passes: legacy streams are all
      version 1, and a full chain or an anchored range catches the rest."""
    if from_seq == 1 and "held.anchor_ignored" not in skip:
        anchor = {"v2_before": False}
    if "held.trust_first" in skip and from_seq != 1:
        anchor = None
    if anchor is None:
        last, versioned = None, False
    elif anchor["v2_before"]:
        last, versioned = anchor["held"], True
    else:
        last, versioned = False, False
    out = []
    for i, event in enumerate(events):
        if event["event_type"] != "AgentModeChanged":
            if "held.reset_on_other" in skip:
                last = None
            continue
        if event["schema_version"] == 1:
            if versioned and "held.v1_after_v2" not in skip:
                out.append(i)
            continue
        versioned = True
        p = event["payload"]
        reason = p["reason"]
        if reason in HOLD_REASONS and "held.carry" not in skip:
            expected = reason == "owner_hold"
            if p["held"] != expected and "held.reason_ignored" not in skip:
                out.append(i)
            last = p["held"] if "held.written_on_hold" in skip else expected
            continue
        if last is None:
            if "held.unanchored_passes" not in skip:
                out.append(i)
            continue
        if p["held"] != last:
            out.append(i)
            if "held.written_carried" in skip:
                last = p["held"]
        if "held.forget" in skip:
            last = None
    return out


def range_event(version: int, from_: str, to: str, reason: str, lifecycle: str, held: bool | None = None) -> dict:
    payload = {"from": from_, "to": to, "reason": reason, "lifecycle": lifecycle}
    if version == 2:
        payload["held"] = held
    return {"event_type": "AgentModeChanged", "schema_version": version, "payload": payload}


def other_event(event_type: str) -> dict:
    """An agent-stream event that is not a mode change; `held` carries across it."""
    return {"event_type": event_type, "schema_version": 1, "payload": {}}


def range_cases() -> list[dict]:
    hold = range_event(2, "normal", "exits_only", "owner_hold", "normal", True)
    pause = range_event(2, "exits_only", "paused", "owner_pause", "paused", True)
    resume = range_event(2, "paused", "exits_only", "owner_resume", "normal", True)
    lift = range_event(2, "exits_only", "normal", "owner_lift_hold", "normal", False)
    dropped = range_event(2, "exits_only", "exits_only", "restriction_changed", "normal", False)
    kept = range_event(2, "exits_only", "exits_only", "restriction_changed", "normal", True)
    clear_copy = range_event(2, "normal", "exits_only", "restriction_changed", "normal", False)
    v1_pause = range_event(1, "normal", "paused", "owner_pause", "paused")
    held_before = {"v2_before": True, "held": True}
    clear_before = {"v2_before": True, "held": False}
    none_before = {"v2_before": False}

    def case(name, events, expect, from_seq=1, anchor=None):
        return {"name": name, "from_seq": from_seq, "anchor": anchor, "events": events, "expect": expect}

    return [
        case(
            "the_hold_carried_through_a_pause_a_resume_and_other_events",
            [hold, other_event("KillSwitchActivated"), pause, other_event("IntentProposed"), resume, lift],
            [],
        ),
        case("a_restriction_drops_the_hold", [hold, other_event("IntentProposed"), dropped], [2]),
        case(
            "a_kill_switch_drops_the_hold",
            [hold, range_event(2, "exits_only", "stopped", "kill_switch", "stopped", False)],
            [1],
        ),
        case(
            "a_reconciliation_sets_a_hold_nobody_asked_for",
            [range_event(2, "normal", "exits_only", "awaiting_reconciliation", "normal", True)],
            [0],
        ),
        case("a_version_1_record_after_a_version_2_one", [hold, v1_pause], [1]),
        case("version_1_records_before_the_first_hold", [v1_pause, hold], []),
        case("a_full_chain_opens_with_a_copy_that_holds_nothing", [clear_copy], []),
        case("a_version_1_record_after_a_first_copy", [clear_copy, v1_pause], [1]),
        case("a_lift_then_a_copy_that_holds_nothing", [hold, lift, clear_copy], []),
        case(
            "a_lift_then_a_copy_that_keeps_the_hold",
            [hold, lift, range_event(2, "normal", "paused", "owner_pause", "paused", True)],
            [2],
        ),
        case(
            "the_carried_hold_is_the_expected_one_not_the_written_one",
            [hold, dropped, kept],
            [1],
        ),
        case("two_violations_and_only_the_first_reported", [hold, dropped, dropped], [1, 2]),
        case(
            "a_hold_whose_held_contradicts_its_reason",
            [range_event(2, "normal", "exits_only", "owner_hold", "normal", False), kept],
            [0],
        ),
        case("a_later_range_launders_a_dropped_hold", [dropped], [0], 40, held_before),
        case("a_later_range_starts_with_version_1_after_a_version_2", [v1_pause], [0], 40, clear_before),
        case(
            "a_later_range_whose_predecessor_was_not_held",
            [range_event(2, "normal", "normal", "restriction_changed", "normal", False)],
            [],
            40,
            clear_before,
        ),
        case("a_later_range_carries_the_hold_of_a_copy", [kept, resume], [], 40, held_before),
        case("a_later_range_with_no_version_2_before", [v1_pause, clear_copy], [], 40, none_before),
        case("an_unanchored_range_opens_with_a_hold", [hold, kept], [], 40),
        case("an_unanchored_range_anchors_on_its_own_hold_then_drops_it", [hold, dropped], [1], 40),
        case("an_unanchored_range_opens_with_version_1", [v1_pause, hold], [], 40),
        case("an_unanchored_range_opens_with_a_held_copy_and_fails_closed", [kept], [0], 40),
        case("an_unanchored_range_opens_with_a_clear_copy_and_fails_closed", [dropped], [0], 40),
        case("an_unanchored_range_with_version_1_after_its_hold", [hold, v1_pause], [1], 40),
    ]


RANGE_MUTANTS = (
    "held.v1_after_v2",
    "held.carry",
    "held.forget",
    "held.anchor_ignored",
    "held.trust_first",
    "held.reset_on_other",
    "held.unanchored_passes",
    "held.reason_ignored",
    "held.written_on_hold",
    "held.written_carried",
)


def range_problems(section: dict, skip: frozenset[str] = frozenset()) -> list[str]:
    problems = []
    for case in section["range_verification"]:
        got = held_mismatches(case["events"], case["from_seq"], case["anchor"], skip)
        if got != case["expect"]:
            problems.append(found("range", f"{case['name']}: expected {case['expect']}, got {got}"))
    return problems


# --------------------------------------------------------------------------- independent oracles

ORACLE_CHECKS = ("drafts.valid", "drafts.floor", "drafts.copies", "drafts.who", "range", "invalid_drafts", "valid_drafts")
RANK = {"normal": 0, "exits_only": 1, "paused": 2, "stopped": 3}


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def floor_problems(name: str, draft: dict) -> list[str]:
    """Mandate spec §5.9 and §6.1: the effective mode is the strictest of what holds it. The owner's
    lifecycle holds its own mode, and the hold holds `exits_only`."""
    p = dict(draft["payload"])
    if draft["schema_version"] == 1:
        p.setdefault("held", False)
    if not isinstance(p.get("held"), bool) or p.get("lifecycle") not in RANK or p.get("to") not in RANK:
        return [found("drafts.floor", f"{name}: no `held`, lifecycle, or mode to judge")]
    held_by = [p["lifecycle"]] + (["exits_only"] if p["held"] else [])
    strictest = max(RANK[m] for m in held_by)
    if RANK[p["to"]] < strictest:
        return [found("drafts.floor", f"{name}: left {p['to']} while held by {held_by}")]
    if p["reason"] == "owner_lift_hold" and p["held"]:
        return [found("drafts.floor", f"{name}: a lift that kept the hold")]
    return []


def who_problems(name: str, draft: dict) -> list[str]:
    actor = draft["actor"]
    p = draft["payload"]
    if draft["event_type"] != "OwnerCommandIssued":
        return []
    person = actor.get("on_behalf_of") if actor["kind"] == "client" else actor["id"]
    if p["command"] not in ("hold_openings", "lift_hold"):
        if actor["kind"] == "client":
            return [found("drafts.who", f"{name}: a client issued {p['command']}")]
        return []
    if p["command"] == "lift_hold" and actor["kind"] != "user":
        return [found("drafts.who", f"{name}: a lift by a {actor['kind']}")]
    if p["user"] != person:
        return [found("drafts.who", f"{name}: the command names {p['user']}, not {person}")]
    return []


def check_section(section: dict) -> list[str]:
    problems = []
    drafts = section["drafts"]
    accepted = []
    for name, draft in drafts.items():
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
        accepted.append((name, draft))
    causes = [d["causation_id"] for d in drafts.values() if d["stream_id"] == AGENT_STREAM]
    commands = {d["event_id"] for d in drafts.values() if d["event_type"] == "OwnerCommandIssued"} | {SECOND_LIFT}
    if len(causes) != len(set(causes)) or not set(causes) <= commands:
        problems.append(found("drafts.copies", f"the copies do not each name their own command: {causes}"))
    for case in section["invalid_drafts"]:
        got = violations(draft_for(section, case))
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
    for case in section["valid_drafts"]:
        draft = draft_for(section, case)
        got = violations(draft)
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
        accepted.append((case["name"], draft))
    for name, draft in accepted:
        if draft["event_type"] == "AgentModeChanged":
            problems += floor_problems(name, draft)
        problems += who_problems(name, draft)
    problems += range_problems(section)
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "rule.90",
    "rule.90.client_lifts",
    "rule.90.client_commands",
    "closed.owner_commands",
    "rule.91",
    "rule.91.subject",
    "types.null",
    "rule.92",
    "rule.93",
    "rule.94",
    "rule.94.held",
    "rule.95",
    "rule.28",
    "rule.83",
    "record.extra",
    "record.missing",
    *sorted(
        {f"loose.payload.{member}" for cases in MEMBER_CASES.values() for member, _, _ in cases}
        - {f"loose.payload.{member}" for member in RULE_TYPED}
    ),
    *sorted(
        {f"nullable.payload.{member}" for members in NULLED.values() for member in members}
        - {f"nullable.payload.{member}" for member in RULE_TYPED}
    ),
)


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    def mutated(fn) -> dict:
        out = copy.deepcopy(section)
        fn(out)
        return out

    def case(s, kind, name):
        return next(c for c in s[kind] if c["name"] == name)

    return [
        ("a base draft breaks rule 92", "drafts.valid", mutated(lambda s: s["drafts"]["hold"]["payload"].update(step_up=dict(STEP_UP)))),
        (
            "two copies of one lift",
            "drafts.copies",
            mutated(lambda s: s["drafts"]["lift_refused"].update(causation_id=IDS["lift"])),
        ),
        (
            "a valid resume clears the hold while rule 94 forgets it",
            "drafts.floor",
            mutated(lambda s: case(s, "valid_drafts", "resume_while_held")["changes"].append(change("payload.to", "normal"))),
        ),
        (
            "a valid hold names someone else while rule 91 is read loosely",
            "drafts.who",
            mutated(lambda s: case(s, "valid_drafts", "client_holds")["changes"].append(change("actor.on_behalf_of", "user_owner_02"))),
        ),
        (
            "a range case expects nothing of a dropped hold",
            "range",
            mutated(lambda s: s["range_verification"][1].update(expect=[])),
        ),
        (
            "a valid copy loses its `held`",
            "drafts.floor",
            mutated(lambda s: s["drafts"]["held"]["payload"].update(held=None)),
        ),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "hold_with_step_up")["expect"].update(path="actor.kind")),
        ),
        (
            "a valid draft breaks a rule",
            "valid_drafts",
            mutated(lambda s: case(s, "valid_drafts", "client_holds")["changes"].append(change("payload.command", "lift_hold"))),
        ),
    ]


def run_mutants(section: dict) -> list[str]:
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
            escaped.append(f"hold validator mutant {mutant}")
    for mutant in RANGE_MUTANTS:
        if not range_problems(section, frozenset([mutant])):
            escaped.append(f"hold range mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"hold check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"hold vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
        "range_verification": range_cases(),
    }
