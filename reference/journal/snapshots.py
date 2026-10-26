"""Journal spec v0.89 §9.35's reference vectors for `AccountSnapshotRecorded` version 2 (DEC-2398):
the connector's declared account type and day-trading regime, and the broker's prior-close equity
and maintenance margin, beside version 1's members, which stay registered.

The schemas live in `control.py`, beside §9.2's version 1, so one validator judges every closed
schema and rule 24 reads both versions. This module builds the `snapshot_prior_close` section: two
version-2 base drafts (a reconciliation's, with the cash comparison, and a fee step's, without it)
and a version-1 base draft; an invalid draft for every new member's type and form, each missing or
extra member, each figure `null`, a version-1 draft carrying a version-2 member, rule 24 at version
2, and version 3; valid drafts for what no rule refuses (an undeclared type or regime, every
declared pair, a negative prior-close equity, a deficit, a negative maintenance margin, zero); the
reading of each base and valid draft (§9.35: a version 1 has no figures; a version 2's maintenance
excess is equity less maintenance margin, signed, and unreadable when the margin is negative,
DEC-524 item 2); and streams in which the latest snapshot wins across versions. Its own oracle reads
the drafts' members directly, without the validator or the reading, and every seeded bug is shown
caught.
"""

from __future__ import annotations

import copy
from decimal import Decimal
from fractions import Fraction

import re

from common import change, delete, normalize_decimal
from control import (
    ACCOUNT_STREAM_REF,
    EXECUTOR,
    SNAPSHOT_ACCOUNT_TYPES,
    SNAPSHOT_REGIMES,
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

SPEC = "docs/specs/journal.md v0.89 §9.35 (DEC-2398)"
ACCOUNT = f"acct:{WORKSPACE}:{ACCOUNT_STREAM_REF}"
EVENT_IDS = {
    "snapshot_v2": "01JAC0S0A000000000000000S1",
    "snapshot_v2_fees": "01JAC0S0A000000000000000S2",
    "snapshot_v1": "01JAC0S0A000000000000000S3",
}
V2 = "snapshot_v2"
V2_FEES = "snapshot_v2_fees"
V1 = "snapshot_v1"
ACCOUNT_VALUES = {
    "status": "ACTIVE",
    "crypto_status": "ACTIVE",
    "trading_blocked": False,
    "account_blocked": False,
    "trade_suspended_by_user": False,
    "multiplier": 1,
    "equity": "25000",
    "cash": "10000",
    "buying_power": "20000",
    "non_marginable_buying_power": "10000",
    "accrued_fees": "0",
}
V2_VALUES = {
    "account_type": "margin",
    "day_trading_regime": "intraday_margin",
    "last_equity": "24750.5",
    "maintenance_margin": "7500.25",
}
COMPARED = {"model_cash": "10000.5", "cash_band": "1.25", "cash_in_band": True}
NOT_COMPARED = {"model_cash": None, "cash_band": None, "cash_in_band": None}
DECLARED = ("account_type", "day_trading_regime")
FIGURES = ("last_equity", "maintenance_margin")
NEW_MEMBERS = (*DECLARED, *FIGURES)
OPTIONS = {"account_type": SNAPSHOT_ACCOUNT_TYPES, "day_trading_regime": SNAPSHOT_REGIMES}
# The oracle's own reading of journal spec §4.6's decimal input, written apart from `common`: an
# optional minus, digits with at most one point, and an optional exponent; the journal normalizes it.
DECIMAL_TEXT = re.compile(r"-?([0-9]+\.?[0-9]*|\.[0-9]+)([eE][+-]?[0-9]+)?\Z")


def snapshot(name: str) -> dict:
    version = 1 if name == V1 else 2
    at = "2026-10-26T21:00:00.000000000Z" if name == V2_FEES else "2026-10-26T14:05:00.000000000Z"
    payload = {
        **ACCOUNT_VALUES,
        **(V2_VALUES if version == 2 else {}),
        **(NOT_COMPARED if name == V2_FEES else COMPARED),
        "risk_clock": at,
    }
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": EVENT_IDS[name],
        "stream_id": ACCOUNT,
        "event_type": "AccountSnapshotRecorded",
        "schema_version": version,
        "event_time": at,
        "clock_source": "broker",
        "causation_id": None,
        "correlation_id": None,
        "actor": dict(EXECUTOR),
        "config_refs": {},
        "payload": payload,
        "artifact_refs": [],
        "pii_refs": [],
    }


def base_drafts() -> dict[str, dict]:
    return {V2: snapshot(V2), V2_FEES: snapshot(V2_FEES), V1: snapshot(V1)}


def invalid(name, clause, base, changes, reason, path):
    return control_invalid(name, clause, base, changes, reason, path)


def valid(name, clause, base, changes):
    return control_valid(name, clause, base, changes)


def v1_with(member: str) -> dict:
    path = f"payload.{member}"
    return invalid(
        f"v1_with.{member}",
        "§9.35: version 1 is never edited (§8); its members stay its own",
        V1,
        [change(path, V2_VALUES[member])],
        "schema",
        path,
    )


def member_drafts() -> list[dict]:
    out = []
    for member in FIGURES:
        path = f"payload.{member}"
        out += [
            invalid(f"{member}.kind", "§9.35 types: a decimal is text", V2, [change(path, 7)], "schema", path),
            invalid(f"{member}.form", "§9.35 types: §4.6's decimal form", V2, [change(path, "24750,5")], "non_canonical", path),
            invalid(f"{member}.signed", "§9.35 types: §4.6's decimal form", V2, [change(path, "+24750.5")], "non_canonical", path),
            invalid(f"{member}.null", "§9.35 types: never null", V2, [change(path, None)], "schema", path),
            invalid(f"{member}.null.fees", "§9.35 types: never null, a fee step's too", V2_FEES, [change(path, None)], "schema", path),
            invalid(f"{member}.missing", "§9.35 closed: every member", V2, [delete(path)], "schema", path),
            v1_with(member),
        ]
    for member in DECLARED:
        path = f"payload.{member}"
        upper = V2_VALUES[member].upper()
        out += [
            invalid(f"{member}.kind", "§9.35 types: a closed name is text", V2, [change(path, 7)], "schema", path),
            invalid(f"{member}.form", "§9.35 types: the names exactly as listed", V2, [change(path, upper)], "non_canonical", path),
            invalid(f"{member}.unlisted", "§9.35 types: only the names listed", V2, [change(path, "portfolio_margin")], "non_canonical", path),
            invalid(f"{member}.missing", "§9.35 closed: every member, `null` when undeclared", V2, [delete(path)], "schema", path),
            v1_with(member),
        ]
    out += [
        invalid(
            "account_type.as_regime",
            "§9.35 types: each member has its own names",
            V2,
            [change("payload.account_type", "intraday_margin")],
            "non_canonical",
            "payload.account_type",
        ),
        invalid("extra.regime", "§9.35 closed: no member beyond the table", V2, [change("payload.regime", "intraday_margin")], "schema", "payload.regime"),
        invalid(
            "extra.last_maintenance_margin",
            "§9.35 closed: the prior close's margin is not journaled",
            V2,
            [change("payload.last_maintenance_margin", "7400")],
            "schema",
            "payload.last_maintenance_margin",
        ),
        invalid("equity.kind", "§9.35: version 1's members keep their types", V2, [change("payload.equity", 25000)], "schema", "payload.equity"),
        invalid("risk_clock.missing", "§9.35: `risk_clock` is required, as at version 1", V2, [delete("payload.risk_clock")], "schema", "payload.risk_clock"),
    ]
    return out


def rule_drafts() -> list[dict]:
    return [
        invalid(
            "band_without_model_cash",
            "rule 24 at version 2: the comparison's three members are null together",
            V2_FEES,
            [change("payload.cash_band", "1")],
            "schema",
            "payload.cash_band",
        ),
        invalid(
            "flag_contradicts_band",
            "rule 24 at version 2: the flag matches its own numbers",
            V2,
            [change("payload.cash_in_band", False)],
            "schema",
            "payload.cash_in_band",
        ),
    ]


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule; together they cover each new member and rule 24."""
    return [
        *member_drafts(),
        *rule_drafts(),
        invalid("at_version_3", "§9.35: versions 1 and 2 only", V2, [change("schema_version", 3)], "unknown_schema", "payload"),
    ]


def valid_drafts() -> list[dict]:
    """What no rule refuses: the broker's figures and the connector's declaration as they stand."""
    return [
        valid("type_undeclared", "§9.35: a connector that declares no type writes `null`", V2, [change("payload.account_type", None)]),
        valid("regime_undeclared", "§9.35: a connector that declares no regime writes `null`", V2, [change("payload.day_trading_regime", None)]),
        valid(
            "nothing_declared",
            "§9.35: the figures stand without a declaration",
            V2,
            [change("payload.account_type", None), change("payload.day_trading_regime", None)],
        ),
        valid(
            "cash_legacy_pdt",
            "§9.35: no rule ties the regime to the type",
            V2,
            [change("payload.account_type", "cash"), change("payload.day_trading_regime", "legacy_pdt")],
        ),
        valid("margin_legacy_pdt", "§9.35: legacy_pdt in a margin account", V2, [change("payload.day_trading_regime", "legacy_pdt")]),
        valid("prior_close_above_equity", "§9.35: no rule ties `last_equity` to `equity`", V2, [change("payload.last_equity", "30000")]),
        valid("prior_close_negative", "§9.35: a prior close in deficit is recorded as reported", V2, [change("payload.last_equity", "-120.75")]),
        valid("prior_close_zero", "§9.35: zero is a figure", V2, [change("payload.last_equity", "0")]),
        valid(
            "figures_normalized",
            "§4.6: a decimal input with a trailing zero or an exponent is normalized, not refused",
            V2,
            [change("payload.last_equity", "24750.50"), change("payload.maintenance_margin", "7.50025E+3")],
        ),
        valid("margin_zero", "§9.35: no requirement is a figure", V2, [change("payload.maintenance_margin", "0")]),
        valid("margin_equal_to_equity", "§9.35: an excess of zero", V2, [change("payload.maintenance_margin", "25000")]),
        valid("margin_above_equity", "§9.35: a deficit is recorded, never floored", V2, [change("payload.maintenance_margin", "25000.01")]),
        valid(
            "margin_negative",
            "§9.35: no rule refuses a negative margin; the reading does (DEC-524 item 2)",
            V2,
            [change("payload.maintenance_margin", "-1")],
        ),
        valid("fees_step_figures", "§9.35: a fee step's snapshot carries the figures too", V2_FEES, [change("payload.last_equity", "25000")]),
        valid(
            "v1_fees_step",
            "§9.35: version 1 stays registered as it was",
            V1,
            [change("payload.model_cash", None), change("payload.cash_band", None), change("payload.cash_in_band", None)],
        ),
    ]


# --------------------------------------------------------------------------- the gate's reading


def reading_of(draft: dict, skip: frozenset[str] = frozenset()) -> dict:
    """§9.35: version 1 has no figures and no declaration; version 2's declaration is as written,
    `null` when undeclared, its prior-close equity is `last_equity` as it stands, and its
    maintenance excess is `equity` less `maintenance_margin`, signed, unreadable (`null`) when the
    margin is negative (DEC-524 item 2)."""
    p = draft["payload"]
    if draft["schema_version"] == 1:
        if "reading.v1_as_zero" in skip:
            return {
                "reading": "figures",
                "account_type": "margin",
                "day_trading_regime": "intraday_margin",
                "prior_close_equity": "0",
                "maintenance_excess": p["equity"],
            }
        return {"reading": "no_figures"}
    equity = Decimal(p["cash"] if "reading.excess_from_cash" in skip else p["equity"])
    margin = Decimal(p["maintenance_margin"])
    if margin < 0 and "reading.negative_margin_read" not in skip:
        excess = None
    else:
        difference = equity - margin
        if "reading.excess_floored" in skip:
            difference = max(difference, Decimal(0))
        excess = normalize_decimal(format(difference, "f"))
    prior = normalize_decimal(p["equity"] if "reading.prior_close_is_equity" in skip else p["last_equity"])
    regime = p["day_trading_regime"]
    if regime is None and "reading.regime_defaulted" in skip:
        regime = "intraday_margin"
    return {
        "reading": "figures",
        "account_type": p["account_type"],
        "day_trading_regime": regime,
        "prior_close_equity": prior,
        "maintenance_excess": excess,
    }


def named_drafts(section: dict) -> list[tuple[str, dict]]:
    named = [(name, copy.deepcopy(draft)) for name, draft in section["drafts"].items()]
    named += [(case["name"], draft_for(section, case)) for case in section["valid_drafts"]]
    return named


def readings(section: dict) -> list[dict]:
    return [{"draft": name, **reading_of(draft)} for name, draft in named_drafts(section)]


# --------------------------------------------------------------------------- latest wins

# §9.35 over a stream: a reader takes the latest snapshot, whatever its version, so a version 1
# after a version 2 leaves no figures, and never the older version 2's (a writer that could not read
# the figures wrote version 1, and the older figures are no longer the broker's). Each expectation
# is a literal written from that sentence; `fold_latest` is the reference reading it checks.


def step(base: str, risk_clock: str, *changes) -> dict:
    return {"base_draft": base, "changes": [change("payload.risk_clock", risk_clock), change("event_time", risk_clock), *changes]}


FIGURES_V2 = {
    "reading": "figures",
    "account_type": "margin",
    "day_trading_regime": "intraday_margin",
    "prior_close_equity": "24750.5",
    "maintenance_excess": "17499.75",
}


def streams() -> list[dict]:
    first, second, third = (
        "2026-10-26T14:05:00.000000000Z",
        "2026-10-26T15:05:00.000000000Z",
        "2026-10-26T16:05:00.000000000Z",
    )
    return [
        {
            "name": "v1_then_v2",
            "clause": "§9.35: a version 2 after a version 1 brings the figures",
            "steps": [step(V1, first), step(V2, second)],
            "expect": copy.deepcopy(FIGURES_V2),
        },
        {
            "name": "v2_then_v1",
            "clause": "§9.35: the latest snapshot wins; a version 1 after a version 2 leaves no figures",
            "steps": [step(V2, first), step(V1, second)],
            "expect": {"reading": "no_figures"},
        },
        {
            "name": "v2_then_v2",
            "clause": "§9.35: the latest version 2's figures, not the earlier one's",
            "steps": [
                step(V2, first),
                step(V2, second, change("payload.last_equity", "25100"), change("payload.maintenance_margin", "26000")),
            ],
            "expect": {**FIGURES_V2, "prior_close_equity": "25100", "maintenance_excess": "-1000"},
        },
        {
            "name": "v2_v1_v2",
            "clause": "§9.35: figures return with the next version 2",
            "steps": [
                step(V2, first),
                step(V1, second),
                step(V2_FEES, third, change("payload.day_trading_regime", None)),
            ],
            "expect": {**FIGURES_V2, "day_trading_regime": None},
        },
        {
            "name": "v2_then_negative_margin",
            "clause": "§9.35: the latest version 2's negative margin leaves no excess, not the earlier one's",
            "steps": [step(V2, first), step(V2, second, change("payload.maintenance_margin", "-1"))],
            "expect": {**FIGURES_V2, "maintenance_excess": None},
        },
    ]


def fold_latest(section: dict, stream: dict, skip: frozenset[str] = frozenset()) -> dict:
    drafts = [draft_for(section, item) for item in stream["steps"]]
    if "fold.first_wins" in skip:
        return reading_of(drafts[0])
    if "fold.skip_v1" in skip:
        drafts = [d for d in drafts if d["schema_version"] == 2] or drafts
    return reading_of(drafts[-1])


# --------------------------------------------------------------------------- independent oracle

ORACLE_CHECKS = (
    "drafts.valid",
    "drafts.versions",
    "invalid_drafts",
    "invalid_drafts.oracle",
    "valid_drafts",
    "readings",
    "streams.records",
    "streams.order",
    "streams",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def oracle_new_members_well_formed(draft: dict) -> bool:
    """Read apart from the validator: a version 2 holds both figures as decimal text and each
    declaration as `null` or a listed name, and a version 1 holds none of the four. Only the members
    this section adds are judged."""
    p = draft["payload"]
    if draft["schema_version"] == 1:
        return not any(m in p for m in NEW_MEMBERS)
    figures = all(isinstance(p.get(m), str) and DECIMAL_TEXT.match(p[m]) for m in FIGURES)
    declared = all(m in p and (p[m] is None or p[m] in OPTIONS[m]) for m in DECLARED)
    return figures and declared


def oracle_reading(draft: dict) -> dict:
    """The reading by exact fractions: a version without `last_equity` reads no figures."""
    p = draft["payload"]
    if "last_equity" not in p:
        return {"reading": "no_figures"}
    if not oracle_new_members_well_formed(draft):
        return {"reading": "malformed"}
    margin = Fraction(p["maintenance_margin"])
    return {
        "reading": "figures",
        "account_type": p["account_type"],
        "day_trading_regime": p["day_trading_regime"],
        "prior_close_equity": Fraction(p["last_equity"]),
        "maintenance_excess": None if margin < 0 else Fraction(p["equity"]) - margin,
    }


def as_fractions(entry: dict) -> dict:
    out = dict(entry)
    for key in ("prior_close_equity", "maintenance_excess"):
        if out.get(key) is not None:
            out[key] = Fraction(out[key])
    return out


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    for name, draft in section["drafts"].items():
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
    versions = {name: d["schema_version"] for name, d in section["drafts"].items()}
    if versions != {V2: 2, V2_FEES: 2, V1: 1} or section["drafts"][V2_FEES]["payload"].get("model_cash") is not None:
        problems.append(found("drafts.versions", f"the base drafts are not two version 2s and a version 1: {versions}"))
    for case in section["invalid_drafts"]:
        draft = draft_for(section, case)
        got = violations(draft)
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
        names_member = case["name"].startswith(NEW_MEMBERS) or case["name"].startswith("v1_with.")
        if names_member == oracle_new_members_well_formed(draft):
            problems.append(found("invalid_drafts.oracle", f"{case['name']}: the oracle disagrees on the new members"))
    for case in section["valid_drafts"]:
        draft = draft_for(section, case)
        got = violations(draft)
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
        if not oracle_new_members_well_formed(draft):
            problems.append(found("valid_drafts", f"{case['name']}: the oracle refuses its new members"))
    want = [{"draft": name, **oracle_reading(draft)} for name, draft in named_drafts(section)]
    got = [as_fractions(entry) for entry in section["readings"]]
    if got != want:
        problems.append(found("readings", f"expected {want}, got {section['readings']}"))
    for stream in section["streams"]:
        drafts = [draft_for(section, item) for item in stream["steps"]]
        for i, draft in enumerate(drafts):
            got = violations(draft)
            if got:
                problems.append(found("streams.records", f"{stream['name']} step {i}: {got}"))
        clocks = [d["payload"]["risk_clock"] for d in drafts]
        if any(a >= b for a, b in zip(clocks, clocks[1:], strict=False)):
            problems.append(found("streams.order", f"{stream['name']}: the risk clock does not rise: {clocks}"))
        if as_fractions(stream["expect"]) != oracle_reading(drafts[-1]):
            problems.append(found("streams", f"{stream['name']}: the oracle reads the latest as {oracle_reading(drafts[-1])}"))
        if fold_latest(section, stream) != stream["expect"]:
            problems.append(found("streams", f"{stream['name']}: expected {stream['expect']}, folded {fold_latest(section, stream)}"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "record.extra",
    "record.missing",
    *(f"loose.payload.{member}" for member in NEW_MEMBERS),
    *(f"nullable.payload.{member}" for member in FIGURES),
    "rule.24",
    "rule.24.in_band",
    "versions.snapshot_v2",
)
READING_MUTANTS = (
    "reading.v1_as_zero",
    "reading.excess_floored",
    "reading.excess_from_cash",
    "reading.negative_margin_read",
    "reading.prior_close_is_equity",
    "reading.regime_defaulted",
)
FOLD_MUTANTS = ("fold.first_wins", "fold.skip_v1")


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    """Seeded bugs in the vectors, each registered against the one check that must reject it."""

    def mutated(fn) -> dict:
        out = copy.deepcopy(section)
        fn(out)
        return out

    def case(s, kind, name):
        return next(c for c in s[kind] if c["name"] == name)

    return [
        ("a base draft is at version 3", "drafts.valid", mutated(lambda s: s["drafts"][V2].update(schema_version=3))),
        (
            "the fee step's base compares cash",
            "drafts.versions",
            mutated(lambda s: s["drafts"][V2_FEES]["payload"].update(model_cash="10000", cash_band="0", cash_in_band=True)),
        ),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "last_equity.null")["expect"].update(path="payload.equity")),
        ),
        (
            "a member case is relabelled as another member's",
            "invalid_drafts.oracle",
            mutated(lambda s: case(s, "invalid_drafts", "extra.regime").update(name="maintenance_margin.extra")),
        ),
        (
            "a valid draft's prior close is not a decimal",
            "valid_drafts",
            mutated(lambda s: case(s, "valid_drafts", "prior_close_zero")["changes"].append(change("payload.last_equity", "zero"))),
        ),
        (
            "a reading floors a deficit at zero",
            "readings",
            mutated(lambda s: next(r for r in s["readings"] if r["draft"] == "margin_above_equity").update(maintenance_excess="0")),
        ),
        (
            "a stream step is at version 3",
            "streams.records",
            mutated(lambda s: s["streams"][0]["steps"][0]["changes"].append(change("schema_version", 3))),
        ),
        (
            "a stream's clocks run backwards",
            "streams.order",
            mutated(lambda s: s["streams"][0]["steps"][1]["changes"].append(change("payload.risk_clock", "2026-10-26T13:05:00.000000000Z"))),
        ),
        (
            "a later version 1 keeps the earlier figures",
            "streams",
            mutated(lambda s: s["streams"][1].update(expect=copy.deepcopy(FIGURES_V2))),
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
            escaped.append(f"snapshot validator mutant {mutant}")
    for mutant in READING_MUTANTS:
        skip = frozenset([mutant])
        got = [{"draft": name, **reading_of(draft, skip)} for name, draft in named_drafts(section)]
        if got == section["readings"]:
            escaped.append(f"snapshot reading mutant {mutant}")
    for mutant in FOLD_MUTANTS:
        skip = frozenset([mutant])
        if [fold_latest(section, q, skip) for q in section["streams"]] == [q["expect"] for q in section["streams"]]:
            escaped.append(f"snapshot fold mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"snapshot check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"snapshot vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
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
    section["streams"] = streams()
    return section
