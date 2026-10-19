"""Journal spec v0.58 §9.27's reference vectors (DEC-2301): the account stream's seven risk-limit
records, `RiskDayStarted`, `RiskLimitTriggered`, `RiskLimitLifted`, `HighWaterMarkReset`,
`PositionReleased`, `InstrumentRestrictionChanged`, and `GoalCompleted`.

The schemas and rules 230 to 238 live in `control.py`, beside §9.3's, so one validator judges every
closed schema. This module builds the `risk_limits` section: a base draft of each record shaped as the
reference cases' risk-state events are (`docs/specs/reference-cases/mandate.yaml`), an invalid draft
for every member type and rule, and valid drafts for the shapes the risk-state fold emits that a rule
might be misread to refuse. Its own oracle reads the base drafts' payloads directly, without the
validator, and every seeded bug is shown caught.
"""

from __future__ import annotations

import copy

from common import change, delete, digest_strings
from control import (
    ACCOUNT_STREAM_REF,
    AGENT,
    EXECUTOR,
    WORKSPACE,
    check_of,
    draft_for,
    reported,
    step_up,
    violations,
)
from control import (
    invalid as control_invalid,
)
from control import (
    valid as control_valid,
)

SPEC = "docs/specs/journal.md v0.58 §9.27 (DEC-2301)"
ACCOUNT = f"acct:{WORKSPACE}:{ACCOUNT_STREAM_REF}"
CONTROL = f"ctl:{WORKSPACE}"
BOUND = "sha256:" + "4" * 64
WARNING = "sha256:" + "5" * 64
ASSET = "7b4a1c2e-1111-4a2b-9c3d-000000000001"
ACKNOWLEDGED = "01J8Z4R0A000000000000000A1"
AT = "2026-09-22T14:30:00.000000000Z"
SNAPSHOT = {
    "agent_equity": "9300",
    "high_water_mark": "10000",
    "drawdown": "0.07",
    "day_start_equity": "9500",
    "capital_base": "10000",
    "inherited_loss": "0",
    "net_contributed": "10000",
}
EVENT_IDS = {
    "day_started": "01J8Z4R1A000000000000000B1",
    "rung_triggered": "01J8Z4R1A000000000000000B2",
    "tripwire_triggered": "01J8Z4R1A000000000000000B3",
    "rung_lifted": "01J8Z4R1A000000000000000B4",
    "tripwire_lifted": "01J8Z4R1A000000000000000B5",
    "mark_reset": "01J8Z4R1A000000000000000B6",
    "owner_released": "01J8Z4R1A000000000000000B7",
    "mark_stale": "01J8Z4R1A000000000000000B8",
    "goal_completed": "01J8Z4R1A000000000000000B9",
}
PAYLOADS = {
    "day_started": (
        "RiskDayStarted",
        {"day": "2026-09-22", "day_start_equity": "9500"},
    ),
    "rung_triggered": (
        "RiskLimitTriggered",
        {
            **SNAPSHOT,
            "limit": "drawdown_ladder[1]",
            "action": "exits_only",
            "reason": None,
            "breach_time_s": 300,
            "metric": None,
            "threshold": None,
            "value": None,
        },
    ),
    "tripwire_triggered": (
        "RiskLimitTriggered",
        {
            **SNAPSHOT,
            "limit": "tripwire:losing_streak",
            "action": "exits_only",
            "reason": "tripwire_condition",
            "breach_time_s": None,
            "metric": "consecutive_losing_exits",
            "threshold": "2",
            "value": "2",
        },
    ),
    "rung_lifted": (
        "RiskLimitLifted",
        {**SNAPSHOT, "limit": "drawdown_ladder[0]", "action": "scale_sizes", "reason": None},
    ),
    "tripwire_lifted": (
        "RiskLimitLifted",
        {**SNAPSHOT, "limit": "tripwire:losing_streak", "action": None, "reason": "owner_acknowledged"},
    ),
    "mark_reset": (
        "HighWaterMarkReset",
        {"from": "10500", "to": "9650", "user_id": "user_owner", "step_up": step_up("2026-09-22T14:29:10.000000000Z")},
    ),
    "owner_released": (
        "PositionReleased",
        {
            "instrument": ASSET,
            "qty": "0.15",
            "cause": "owner_release",
            "canceled_order_ids": ["order_stop_1"],
            "warning_ref": WARNING,
            "user_id": "user_owner",
            "step_up": step_up("2026-09-22T14:29:10.000000000Z"),
        },
    ),
    "mark_stale": (
        "InstrumentRestrictionChanged",
        {"instrument": ASSET, "restriction": "stale_mark", "reason": "no_sane_mark", "active": True},
    ),
    "goal_completed": (
        "GoalCompleted",
        {"reason": None, "then": None, "on_complete": "release"},
    ),
}
ACKNOWLEDGING = ("tripwire_lifted", "mark_reset")


def base_drafts() -> dict[str, dict]:
    out = {}
    for name, (event_type, members) in PAYLOADS.items():
        payload = {"agent_id": AGENT, **copy.deepcopy(members), "risk_clock": AT}
        out[name] = {
            "envelope_version": 1,
            "environment": "paper",
            "event_id": EVENT_IDS[name],
            "stream_id": ACCOUNT,
            "event_type": event_type,
            "schema_version": 1,
            "event_time": AT,
            "clock_source": "local",
            "causation_id": ACKNOWLEDGED if name in ACKNOWLEDGING else None,
            "correlation_id": None,
            "actor": dict(EXECUTOR),
            "config_refs": {"mandate_version": BOUND},
            "payload": payload,
            "artifact_refs": sorted(digest_strings(payload)),
            "pii_refs": [],
        }
    return out


def invalid(name, clause, base, changes, reason, path):
    return control_invalid(name, clause, base, changes, reason, path)


def valid(name, clause, base, changes):
    return control_valid(name, clause, base, changes)


def broken(name, clause, base, changes, path):
    """A rule's invalid draft: every §9.27 rule refuses as `schema`."""
    return invalid(name, clause, base, changes, "schema", path)


# Each base draft's own members: a value of the wrong JSON kind (`schema`) and, for a string, one of
# the wrong form (`non_canonical`), with `None` where the member's type has no wrong form.
MEMBER_CASES = {
    "day_started": (("day", 7, "22-09-2026"), ("day_start_equity", 7, "9,500")),
    "rung_triggered": (
        *((member, 7, "1,000") for member in SNAPSHOT),
        ("limit", 7, "daily"),
        ("action", 7, "paused"),
        ("reason", 7, "breach"),
        ("breach_time_s", "300", None),
    ),
    "tripwire_triggered": (
        ("metric", 7, "losing_exits"),
        ("threshold", 7, "two"),
        ("value", 7, "two"),
    ),
    "rung_lifted": (("limit", 7, "drawdown_ladder[01]"), ("action", 7, "resume"), ("reason", 7, "lifted")),
    "mark_reset": (("from", 7, "ten"), ("to", 7, "ten"), ("user_id", 7, "user owner")),
    "owner_released": (
        ("instrument", 7, ASSET.upper()),
        ("qty", 7, "0,15"),
        ("cause", 7, "release"),
        ("canceled_order_ids", "order_stop_1", None),
        ("warning_ref", 7, "sha256:short"),
        ("user_id", 7, "user owner"),
    ),
    "mark_stale": (
        ("instrument", 7, "not-an-asset"),
        ("restriction", 7, "halted"),
        ("reason", 7, "quote_gap"),
        ("active", "true", None),
    ),
    "goal_completed": (("reason", 7, "done"), ("then", 7, "retire"), ("on_complete", 7, "sell_all")),
}
# Members whose type is not nullable, each refused as `null`.
NULLED = {
    "day_started": ("day", "day_start_equity", "agent_id", "risk_clock"),
    "rung_triggered": (*SNAPSHOT, "limit", "action"),
    "mark_reset": ("from", "to", "user_id", "step_up"),
    "owner_released": ("instrument", "qty", "cause", "canceled_order_ids"),
    "mark_stale": ("instrument", "restriction", "reason", "active"),
}


def member_drafts() -> list[dict]:
    out = []
    for base, cases in MEMBER_CASES.items():
        for member, wrong_kind, wrong_form in cases:
            path = f"payload.{member}"
            refs = [change("artifact_refs", [])] if member == "warning_ref" else []
            out.append(
                invalid(f"{base}.{member}.kind", "§9.27 types", base, [change(path, wrong_kind), *refs], "schema", path)
            )
            if wrong_form is not None:
                out.append(
                    invalid(
                        f"{base}.{member}.form",
                        "§9.27 types",
                        base,
                        [change(path, wrong_form), *refs],
                        "non_canonical",
                        path,
                    )
                )
    for base, members in NULLED.items():
        for member in members:
            path = f"payload.{member}"
            out.append(invalid(f"{base}.{member}.null", "§9.27 types", base, [change(path, None)], "schema", path))
    for base in PAYLOADS:
        out.append(
            invalid(f"{base}.missing", "§9.27 closed", base, [delete("payload.risk_clock")], "schema", "payload.risk_clock")
        )
        out.append(
            invalid(f"{base}.extra", "§9.27 closed", base, [change("payload.notes", "x")], "schema", "payload.notes")
        )
        out.append(
            invalid(
                f"{base}.without_its_mandate",
                "§9 row: the risk-state records name `mandate_version`",
                base,
                [change("config_refs", {})],
                "missing_config_ref",
                "config_refs.mandate_version",
            )
        )
        out.append(
            invalid(
                f"{base}.on_the_control_stream",
                "§9 row: an account-stream record",
                base,
                [change("stream_id", CONTROL)],
                "wrong_stream",
                "event_type",
            )
        )
        out.append(
            invalid(
                f"{base}.at_version_2",
                "§9.27: closed at schema version 1 only",
                base,
                [change("schema_version", 2)],
                "unknown_schema",
                "payload",
            )
        )
    out += [
        invalid(
            "rung_triggered.limit.sixth_rung",
            "§9.27 `limit_key`: a ladder has at most five rungs",
            "rung_triggered",
            [change("payload.limit", "drawdown_ladder[5]")],
            "non_canonical",
            "payload.limit",
        ),
        invalid(
            "tripwire_triggered.limit.uppercase_id",
            "§9.27 `limit_key`: a tripwire id is the mandate schema's",
            "tripwire_triggered",
            [change("payload.limit", "tripwire:Losing")],
            "non_canonical",
            "payload.limit",
        ),
    ]
    return out


T, R, L = "tripwire_triggered", "rung_triggered", "rung_lifted"
C = change
NO_GESTURE = [C(f"payload.{m}", None) for m in ("warning_ref", "user_id", "step_up")]
GOAL = [C("payload.cause", "goal_release"), *NO_GESTURE, C("artifact_refs", [])]
REMOVED = C("payload.restriction", "removed_instrument")
PROFIT_STOP = [C("payload.then", "discretionary_exit_all_then_retire"), C("payload.on_complete", None)]
# (name, base, changes, the path the rule reports): each breaks exactly one of rules 230 to 238.
RULE_CASES = (
    ("230.tripwire_without_its_reason", T, [C("payload.reason", None)], "payload.reason"),
    ("230.tripwire_pauses", T, [C("payload.action", "flatten_and_pause")], "payload.action"),
    ("230.rung_with_the_tripwire_reason", R, [C("payload.reason", "tripwire_condition")], "payload.reason"),
    ("230.rung_ends_delegations", R, [C("payload.action", "end_delegations")], "payload.action"),
    ("230.daily_loss_scales", R, [C("payload.limit", "max_daily_loss"), C("payload.action", "scale_sizes")], "payload.action"),
    ("231.inherited_loss_negative", L, [C("payload.inherited_loss", "-1")], "payload.inherited_loss"),
    ("232.tripwire_with_breach_time", T, [C("payload.breach_time_s", 0)], "payload.breach_time_s"),
    ("232.tripwire_without_value", T, [C("payload.value", None)], "payload.value"),
    ("232.rung_without_breach_time", R, [C("payload.breach_time_s", None)], "payload.breach_time_s"),
    ("232.rung_with_a_metric", R, [C("payload.metric", "new_instruments")], "payload.metric"),
    ("233.short_of_threshold", T, [C("payload.value", "1")], "payload.value"),
    ("233.fractional_count", T, [C("payload.value", "2.5")], "payload.value"),
    ("233.count_threshold_zero", T, [C("payload.threshold", "0")], "payload.threshold"),
    ("233.count_threshold_over_1000", T, [C("payload.threshold", "1001"), C("payload.value", "1001")], "payload.threshold"),
    (
        "233.loss_threshold_below_a_cent",
        T,
        [C("payload.metric", "realized_loss_usd"), C("payload.threshold", "100.005"), C("payload.value", "101")],
        "payload.threshold",
    ),
    ("234.floor_lift_scales", L, [C("payload.limit", "lifetime_floor")], "payload.action"),
    ("234.rung_lift_with_action_and_reason", L, [C("payload.reason", "hard_breach_cleared")], "payload.action"),
    ("234.tripwire_lifts_without_the_owner", "tripwire_lifted", [C("payload.reason", None)], "payload.reason"),
    (
        "234.daily_loss_loosened",
        "tripwire_lifted",
        [C("payload.limit", "max_daily_loss"), C("payload.reason", "version_loosened"), C("causation_id", None)],
        "payload.reason",
    ),
    ("235.lift_without_its_acknowledgment", "tripwire_lifted", [C("causation_id", None)], "causation_id"),
    ("235.reset_without_its_acknowledgment", "mark_reset", [C("causation_id", None)], "causation_id"),
    ("236.owner_releases_nothing", "owner_released", [C("payload.qty", "0")], "payload.qty"),
    ("236.goal_releases_a_negative", "owner_released", [*GOAL, C("payload.qty", "-0.1")], "payload.qty"),
    (
        "236.owner_release_without_its_warning",
        "owner_released",
        [C("payload.warning_ref", None), C("artifact_refs", [])],
        "payload.warning_ref",
    ),
    ("236.owner_release_without_step_up", "owner_released", [C("payload.step_up", None)], "payload.step_up"),
    (
        "236.goal_release_with_a_user",
        "owner_released",
        [
            C("payload.cause", "goal_release"),
            C("payload.warning_ref", None),
            C("payload.step_up", None),
            C("artifact_refs", []),
        ],
        "payload.user_id",
    ),
    ("237.stale_mark_for_a_removal", "mark_stale", [C("payload.reason", "operator_halt")], "payload.reason"),
    ("237.stale_mark_cleared_as_set", "mark_stale", [C("payload.active", False)], "payload.active"),
    ("237.removed_for_no_mark", "mark_stale", [C("payload.restriction", "removed_instrument")], "payload.reason"),
    (
        "237.removed_by_an_admission",
        "mark_stale",
        [C("payload.restriction", "removed_instrument"), C("payload.reason", "thesis_admitted")],
        "payload.active",
    ),
    (
        "237.cleared_by_a_removal",
        "mark_stale",
        [C("payload.restriction", "removed_instrument"), C("payload.reason", "eligibility_lost"), C("payload.active", False)],
        "payload.active",
    ),
    ("238.both_forms", "goal_completed", [C("payload.then", "discretionary_exit_all_then_retire")], "payload.then"),
    ("238.neither_form", "goal_completed", [C("payload.on_complete", None)], "payload.then"),
    ("238.reason_with_on_complete", "goal_completed", [C("payload.reason", "target_qty")], "payload.reason"),
)
LOSS = [C("payload.limit", "tripwire:day_loss"), C("payload.action", "end_delegations"), C("payload.metric", "realized_loss_usd")]
# (name, the rule a misreading would break, base, changes): each a shape the risk-state fold emits.
VALID_CASES = (
    ("tripwire_ends_delegations", "rule 230", T, [C("payload.action", "end_delegations")]),
    ("daily_loss_flattens", "rule 230", R, [C("payload.limit", "max_daily_loss"), C("payload.action", "flatten_and_pause")]),
    ("rung_scales", "rule 230", R, [C("payload.limit", "drawdown_ladder[4]"), C("payload.action", "scale_sizes")]),
    ("hard_trigger_at_once", "rule 232", R, [C("payload.reason", "hard_trigger"), C("payload.breach_time_s", 0)]),
    ("pending_hard_breach", "rule 230: MC-R19", R, [C("payload.reason", "hard_breach_pending")]),
    ("drawdown_past_one", "rule 231: DD is unbounded", R, [C("payload.drawdown", "1.2"), C("payload.agent_equity", "-2000")]),
    ("no_drawdown", "rule 231", R, [C("payload.drawdown", "0")]),
    ("value_past_threshold", "rule 233", T, [C("payload.value", "3")]),
    ("loss_value_below_a_cent", "rule 233", T, [*LOSS, C("payload.threshold", "100"), C("payload.value", "100.000000000001")]),
    (
        "new_instruments_at_1000",
        "rule 233: V-044",
        T,
        [C("payload.metric", "new_instruments"), C("payload.threshold", "1000"), C("payload.value", "1000")],
    ),
    ("daily_loss_lifts_plainly", "rule 234", L, [C("payload.limit", "max_daily_loss"), C("payload.action", None)]),
    (
        "rung_hard_breach_cleared",
        "rule 234: MC-R19",
        L,
        [C("payload.limit", "drawdown_ladder[1]"), C("payload.action", None), C("payload.reason", "hard_breach_cleared")],
    ),
    (
        "floor_loosened",
        "rule 234",
        L,
        [C("payload.limit", "lifetime_floor"), C("payload.action", None), C("payload.reason", "version_loosened")],
    ),
    ("rung_acknowledged", "rule 235", "tripwire_lifted", [C("payload.limit", "drawdown_ladder[3]")]),
    ("goal_release", "rule 236", "owner_released", [*GOAL, C("payload.canceled_order_ids", [])]),
    ("goal_releases_nothing", "rule 236: a goal completes flat or not", "owner_released", [*GOAL, C("payload.qty", "0")]),
    ("sane_mark_clears", "rule 237", "mark_stale", [C("payload.reason", "sane_mark"), C("payload.active", False)]),
    *(
        (f"removed_{r}", "rule 237", "mark_stale", [REMOVED, C("payload.reason", r)])
        for r in ("thesis_expired", "operator_halt", "version_applied")
    ),
    *(
        (f"readmitted_{r}", "rule 237", "mark_stale", [REMOVED, C("payload.reason", r), C("payload.active", False)])
        for r in ("thesis_admitted", "version_applied")
    ),
    ("profit_stop_confirmed", "rule 238", "goal_completed", [C("payload.reason", "profit_stop_reached"), *PROFIT_STOP]),
    ("goal_with_no_on_complete", "rule 238", "goal_completed", PROFIT_STOP),
    ("day_started_at_zero", "§9.27", "day_started", [C("payload.day_start_equity", "0")]),
)


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule or type."""
    rules = [broken(name, f"rule {name[:3]}", base, changes, path) for name, base, changes, path in RULE_CASES]
    return [*member_drafts(), *rules]


def valid_drafts() -> list[dict]:
    """The shapes the risk-state fold emits that a rule might be misread to refuse."""
    return [valid(name, clause, base, list(changes)) for name, clause, base, changes in VALID_CASES]


# --------------------------------------------------------------------------- independent oracle

ORACLE_CHECKS = ("drafts.valid", "drafts.shape", "invalid_drafts", "valid_drafts")
REFERENCE_SHAPES = {
    "RiskLimitTriggered": {"limit", "action", "reason", "metric", "threshold", "value"},
    "RiskLimitLifted": {"limit", "action", "reason"},
    "RiskDayStarted": {"day_start_equity"},
    "HighWaterMarkReset": {"from", "to"},
    "PositionReleased": {"qty"},
    "InstrumentRestrictionChanged": {"restriction", "reason", "active"},
    "GoalCompleted": {"reason", "then", "on_complete"},
}


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    for name, draft in section["drafts"].items():
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
    seen = {d["event_type"] for d in section["drafts"].values()}
    if seen != set(REFERENCE_SHAPES):
        problems.append(found("drafts.shape", f"base drafts cover {sorted(seen)}"))
    for name, draft in section["drafts"].items():
        missing = REFERENCE_SHAPES.get(draft["event_type"], set()) - set(draft["payload"])
        if missing:
            problems.append(found("drafts.shape", f"{name} lacks the reference cases' members {sorted(missing)}"))
    for case in section["invalid_drafts"]:
        got = violations(draft_for(section, case))
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
    for case in section["valid_drafts"]:
        got = violations(draft_for(section, case))
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "record.extra",
    "record.missing",
    "config_refs.required",
    "types.limit_key",
    "types.limit_key_rung",
    *(f"rule.{n}" for n in range(230, 239)),
    "rule.230.scale",
    "boundary.rule_231_strict",
    "rule.233.whole",
    "boundary.rule_233_strict",
    "rule.234.floor",
    "rule.236.goal_zero",
    "rule.237.removing",
    "rule.238.reason",
    *(f"nullable.payload.{m}" for members in NULLED.values() for m in members if m not in ("agent_id", "risk_clock")),
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
        (
            "a base tripwire trigger names no metric",
            "drafts.valid",
            mutated(lambda s: s["drafts"]["tripwire_triggered"]["payload"].update(metric=None)),
        ),
        (
            "a base record drops a member the reference cases carry",
            "drafts.shape",
            mutated(lambda s: s["drafts"]["mark_reset"]["payload"].pop("to")),
        ),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "232.tripwire_without_value")["expect"].update(path="payload.metric")),
        ),
        (
            "a valid draft lifts a tripwire without the owner",
            "valid_drafts",
            mutated(
                lambda s: case(s, "valid_drafts", "rung_acknowledged")["changes"].append(
                    change("payload.limit", "tripwire:other")
                )
                or case(s, "valid_drafts", "rung_acknowledged")["changes"].append(change("payload.reason", None))
            ),
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
            escaped.append(f"risk-limit validator mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"risk-limit check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"risk-limit vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
    }
