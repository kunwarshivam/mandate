"""Journal spec v0.50 §9.18's reference vectors for the backtest and evaluation records (DEC-1001,
DEC-1002): `BacktestRunRecorded`, one backtest of a mandate version, and `EvaluationRunRecorded`,
one forward-paper evaluation, both on the workspace control stream at schema version 1, each naming
its report by artifact and the digests that reproduce it. `theses_head` holds bare record hashes
(`[digest]`, DEC-1142): a journal record's hash names no stored artifact, so the heads stay out of
`artifact_refs` and §11 check 6 never reads them.

The schemas, the required `config_refs` (§9's catalogue: `rule` and `man` for a backtest, `rule`
for an evaluation), rules 143 to 145, and DEC-1141's writer live in `control.py`, beside the other
closed schemas, so one validator judges every record. DEC-1141 reads §9.18's silence on the writer
as the tighter reading that adds no risk: the actor is the `user` who asked or the `system` runner
or evaluator that ran it, never an `agent`, a `broker`, or a `platform_operator` (a `client` is
already refused by rule 83), refused `schema` at `actor.kind`.

This module builds the `backtest_records` section: a base draft of each record, an invalid draft for
every member type and every rule, and valid drafts for the cases a rule might be misread to refuse.
Its own oracle reads the drafts' payloads directly, without the validator, and every seeded bug is
shown caught.
"""

from __future__ import annotations

import copy
import hashlib
import json
import re

from common import apply_change, artifact_ref, canon, change, delete, digest_strings
from control import (
    AGENT_STREAM,
    BACKTEST_RECORDS,
    OWNER,
    RUNTIME,
    SCHEMAS,
    STREAM,
    USER,
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

SPEC = "docs/specs/journal.md v0.50 §9.18 (DEC-1001, DEC-1002; writer DEC-1141; theses_head DEC-1142)"
AT = "2026-10-17T15:30:00.000000000Z"
CUT_OFF = "2026-10-16T20:00:00.000000000Z"
CAUSE = "01J8Z5B0A000000000000000Q1"
RUNNER = {"kind": "system", "id": "backtest_runner", "version": "0.1.0", "build": "sha256:" + "b" * 64}
EVALUATOR = {"kind": "system", "id": "paper_evaluator", "version": "0.1.0", "build": "sha256:" + "e" * 64}
BROKER = {"kind": "broker", "id": "alpaca", "version": "v2", "build": None}
OPERATOR = {"kind": "platform_operator", "id": "operator_01", "version": "1", "build": None}
CLIENT = {"kind": "client", "id": "client_01", "version": "1", "build": None, "on_behalf_of": OWNER}
BACKTEST = "BacktestRunRecorded"
EVALUATION = "EvaluationRunRecorded"
SECOND_AGENT = f"agent:{WORKSPACE}:agent_b"


def ref(fill: str) -> str:
    """A digest no stored artifact has: drafts that name one only parse, never append."""
    return "sha256:" + fill * 64


def record_hash(label: str) -> str:
    """A bare record hash (§3), as `theses_head` holds it: hex with no `sha256:` prefix."""
    return hashlib.sha256(label.encode("utf-8")).hexdigest()


def stand_in(kind: str, stands_for: str) -> dict:
    return {"kind": kind, "note": f"reference fixture: stands for {stands_for}"}


# The objects the base drafts name, stored so the stream verifies with §11 check 6. Each stands for
# the bytes §9.18 names; the report is an artifact (§6.3) stored before its record is appended.
STORED = {
    "mandate_version": stand_in("mandate", "the mandate version backtested"),
    "rule_set": stand_in("rule_set", "the rule set the run's gate applied"),
    "data_snapshot": stand_in("data_snapshot", "the bars the run read, with coverage and first-bar medians"),
    "model_outputs": stand_in("model_outputs", "the recorded model outputs the run replayed"),
    "backtest_theses": stand_in("theses", "the thesis records the run replayed"),
    "configuration": stand_in("backtest_config", "the fill model, fees, gate settings, account, cash, metrics"),
    "runner_build": stand_in("code_build", "the backtest runner's build"),
    "backtest_report": stand_in("backtest_report", "a report_version 2 backtest report (DEC-1001 item 2)"),
    "evaluation_theses": stand_in("theses", "the ThesisProposed and ThesisRevised records read"),
    "closes": stand_in("closes", "the close series"),
    "cost_model": stand_in("cost_model", "the round-trip cost model"),
    "evaluator_build": stand_in("code_build", "the evaluator's build"),
    "evaluation_report": stand_in("evaluation_report", "a report_version 1 evaluation report (DEC-1002)"),
}
STORED_REF = {name: artifact_ref(obj) for name, obj in STORED.items()}
VERSION = STORED_REF["mandate_version"]
OTHER_VERSION = ref("9")
RULES = STORED_REF["rule_set"]
HEADS = [record_hash("agent_a seq 41"), record_hash("agent_b seq 5")]
PAYLOADS: dict[str, dict] = {
    BACKTEST: {
        "backtest_id": "bt_01JBT0001",
        "mandate_version": VERSION,
        "data_snapshot": STORED_REF["data_snapshot"],
        "model_outputs": STORED_REF["model_outputs"],
        "theses": STORED_REF["backtest_theses"],
        "configuration": STORED_REF["configuration"],
        "code_build": STORED_REF["runner_build"],
        "report": STORED_REF["backtest_report"],
        "report_version": 2,
        "marker": "backtest",
        "result": "completed",
        "refusal": None,
    },
    EVALUATION: {
        "evaluation_id": "ev_01JEV0001",
        "decision": "DEC-122",
        "cut_off": CUT_OFF,
        "theses": STORED_REF["evaluation_theses"],
        "theses_from": [f"{AGENT_STREAM}:2", f"{SECOND_AGENT}:5"],
        "theses_to": [f"{AGENT_STREAM}:41", f"{SECOND_AGENT}:5"],
        "theses_head": list(HEADS),
        "closes": STORED_REF["closes"],
        "cost_model": STORED_REF["cost_model"],
        "code_build": STORED_REF["evaluator_build"],
        "report": STORED_REF["evaluation_report"],
        "report_version": 1,
        "passed": True,
    },
}
WRITERS = {BACKTEST: RUNNER, EVALUATION: EVALUATOR}
CONFIG_REFS = {BACKTEST: {"mandate_version": VERSION, "rule_set": RULES}, EVALUATION: {"rule_set": RULES}}
EVENT_IDS = {BACKTEST: "01J8Z5BT00000000000000BT01", EVALUATION: "01J8Z5EV00000000000000EV01"}
REPORT_VERSIONS = {BACKTEST: 2, EVALUATION: 1}
POSITION = re.compile(r"^(agent:[A-Za-z0-9_-]+:[A-Za-z0-9_-]+):([1-9][0-9]*)\Z")
HEAD_FORM = re.compile(r"^[0-9a-f]{64}\Z")


def snake(record: str) -> str:
    """A record's base-draft key: §4.1's key grammar admits no capital letter."""
    return re.sub(r"(?<!^)(?=[A-Z])", "_", record).lower()


def base_drafts() -> dict[str, dict]:
    return {
        snake(name): {
            "envelope_version": 1,
            "environment": "paper",
            "event_id": EVENT_IDS[name],
            "stream_id": STREAM,
            "event_type": name,
            "schema_version": 1,
            "event_time": AT,
            "clock_source": "local",
            "causation_id": None,
            "correlation_id": None,
            "actor": dict(WRITERS[name]),
            "config_refs": dict(CONFIG_REFS[name]),
            "payload": copy.deepcopy(PAYLOADS[name]),
            "artifact_refs": sorted(digest_strings(PAYLOADS[name])),
            "pii_refs": [],
        }
        for name in BACKTEST_RECORDS
    }


DRAFTS = base_drafts()


def restated(record: str, changes: list[dict]) -> list[dict]:
    """The changes, then `artifact_refs` restated from the changed payload, so a draft breaks only
    the rule it names."""
    draft = copy.deepcopy(DRAFTS[snake(record)])
    for item in changes:
        apply_change(draft, item)
    refs = sorted(digest_strings(draft["payload"])) if isinstance(draft["payload"], dict) else []
    if refs != draft["artifact_refs"]:
        return [*copy.deepcopy(changes), change("artifact_refs", refs)]
    return copy.deepcopy(changes)


def invalid(name, clause, record, changes, reason, path, also=()):
    return control_invalid(f"{record}.{name}", clause, snake(record), restated(record, changes), reason, path, also)


def valid(name, clause, record, changes):
    return control_valid(f"{record}.{name}", clause, snake(record), restated(record, changes))


def wrong_kind(ty) -> object:
    """A JSON value of another kind than the member's type, refused as `schema`."""
    inner = ty.inner if ty.kind == "nullable" else ty
    return {"int": "2", "bool": "true", "list": "x"}.get(inner.kind, 7)


def wrong_form(ty) -> object | None:
    """A string the member's type refuses as `non_canonical`, or `None` when no string is."""
    kind = ty.inner.kind if ty.kind == "nullable" else ty.kind
    return {
        "id": "not an id",
        "ref": "sha256:short",
        "digest": "sha256:" + HEADS[0],
        "str": "",
        "ts": "2026-10-16",
        "one_of": "not_listed",
    }.get(kind)


def member_drafts() -> list[dict]:
    """Per record, per member: a value of the wrong JSON kind (`schema`), a string of the wrong
    form (`non_canonical`), `null` where the type is not nullable (`schema`), for a list an item of
    the wrong form at its index, and per record one missing and one extra member."""
    out = []
    for record in BACKTEST_RECORDS:
        for member, ty in SCHEMAS[("ctl", record)].fields:
            path = f"payload.{member}"
            out.append(invalid(f"{member}.kind", "§9.18 types", record, [change(path, wrong_kind(ty))], "schema", path))
            if ty.kind == "list":
                items = copy.deepcopy(PAYLOADS[record][member])
                items[0] = wrong_form(ty.inner)
                out.append(
                    invalid(f"{member}.item", "§9.18 types", record, [change(path, items)], "non_canonical", f"{path}[0]")
                )
                items = copy.deepcopy(PAYLOADS[record][member])
                items[1] = 7
                out.append(invalid(f"{member}.item_kind", "§9.18 types", record, [change(path, items)], "schema", f"{path}[1]"))
            form = wrong_form(ty)
            if form is not None and ty.kind != "list":
                out.append(
                    invalid(f"{member}.form", "§9.18 types", record, [change(path, form)], "non_canonical", path)
                )
            if ty.kind != "nullable":
                out.append(invalid(f"{member}.null", "§9.18 types", record, [change(path, None)], "schema", path))
        fields = SCHEMAS[("ctl", record)].fields
        for member in (fields[0][0], fields[-1][0]):
            out.append(
                invalid(
                    f"missing_{member}", "§9.18 closed (§4.2)", record, [delete(f"payload.{member}")], "schema", f"payload.{member}"
                )
            )
        out.append(
            invalid("extra", "§9.18 closed", record, [change("payload.figures", {"total_return": "0.1"})], "schema", "payload.figures")
        )
    out += [
        invalid(
            "refusal.form",
            "§9.18 types: a refusal is non-empty text",
            BACKTEST,
            [change("payload.result", "refused"), change("payload.report", None), change("payload.refusal", "")],
            "non_canonical",
            "payload.refusal",
        ),
        invalid(
            "refusal.kind",
            "§9.18 types",
            BACKTEST,
            [change("payload.result", "refused"), change("payload.report", None), change("payload.refusal", 7)],
            "schema",
            "payload.refusal",
        ),
        invalid(
            "marker.paper",
            "§9.18: the marker is always backtest, never a paper or live result",
            BACKTEST,
            [change("payload.marker", "paper")],
            "non_canonical",
            "payload.marker",
        ),
        invalid(
            "marker.live",
            "§9.18: the marker is always backtest, never a paper or live result",
            BACKTEST,
            [change("payload.marker", "live")],
            "non_canonical",
            "payload.marker",
        ),
    ]
    return out


def rule_drafts() -> list[dict]:
    out = []
    for record in BACKTEST_RECORDS:
        for name, actor in (("by_an_agent", RUNTIME), ("by_a_broker", BROKER), ("by_an_operator", OPERATOR)):
            out.append(
                invalid(
                    name,
                    "DEC-1141: a user who asked or the system that ran it writes the record",
                    record,
                    [change("actor", dict(actor))],
                    "schema",
                    "actor.kind",
                )
            )
        out += [
            invalid(
                "by_a_client",
                "rule 83: a client writes only its own records",
                record,
                [change("actor", dict(CLIENT))],
                "schema",
                "actor.kind",
            ),
            invalid(
                "on_an_agent_stream",
                "§9.18: a workspace control-stream record",
                record,
                [change("stream_id", AGENT_STREAM)],
                "wrong_stream",
                "event_type",
            ),
            invalid(
                "at_version_2",
                "§9.18: closed at schema version 1 only",
                record,
                [change("schema_version", 2)],
                "unknown_schema",
                "payload",
            ),
            invalid(
                "no_rule_set",
                "§9 catalogue: the record requires rule",
                record,
                [delete("config_refs.rule_set")],
                "missing_config_ref",
                "config_refs.rule_set",
            ),
        ]
    out += [
        invalid(
            "no_mandate_version",
            "§9 catalogue: a backtest requires man",
            BACKTEST,
            [delete("config_refs.mandate_version")],
            "missing_config_ref",
            "config_refs.mandate_version",
            also=[("schema", "payload.mandate_version")],
        ),
        invalid(
            "completed_without_report",
            "rule 143: a completed run names its report",
            BACKTEST,
            [change("payload.report", None)],
            "schema",
            "payload.report",
        ),
        invalid(
            "refused_with_report",
            "rule 143: a refused run names no report",
            BACKTEST,
            [change("payload.result", "refused"), change("payload.refusal", "no_bars")],
            "schema",
            "payload.report",
        ),
        invalid(
            "refused_without_refusal",
            "rule 143: a refused run carries its reason code",
            BACKTEST,
            [change("payload.result", "refused"), change("payload.report", None)],
            "schema",
            "payload.refusal",
        ),
        invalid(
            "completed_with_refusal",
            "rule 143: a completed run carries no reason code",
            BACKTEST,
            [change("payload.refusal", "no_bars")],
            "schema",
            "payload.refusal",
        ),
        invalid(
            "refused_with_report_and_no_refusal",
            "rule 143: report first, then refusal",
            BACKTEST,
            [change("payload.result", "refused")],
            "schema",
            "payload.report",
            also=[("schema", "payload.refusal")],
        ),
        invalid(
            "another_version",
            "rule 144: the version backtested is the version bound",
            BACKTEST,
            [change("payload.mandate_version", OTHER_VERSION)],
            "schema",
            "payload.mandate_version",
        ),
        invalid(
            "bound_to_another_version",
            "rule 144: the version backtested is the version bound",
            BACKTEST,
            [change("config_refs.mandate_version", OTHER_VERSION)],
            "schema",
            "payload.mandate_version",
        ),
        invalid(
            "no_streams",
            "rule 145: at least one agent stream read",
            EVALUATION,
            [change("payload.theses_from", []), change("payload.theses_to", []), change("payload.theses_head", [])],
            "schema",
            "payload.theses_from",
        ),
        invalid(
            "to_shorter",
            "rule 145: one entry per stream in each list",
            EVALUATION,
            [change("payload.theses_to", [f"{AGENT_STREAM}:41"])],
            "schema",
            "payload.theses_from",
        ),
        invalid(
            "to_empty",
            "rule 145: one entry per stream in each list",
            EVALUATION,
            [change("payload.theses_to", [])],
            "schema",
            "payload.theses_from",
        ),
        invalid(
            "head_longer",
            "rule 145: one entry per stream in each list",
            EVALUATION,
            [change("payload.theses_head", [*HEADS, record_hash("agent_c seq 1")])],
            "schema",
            "payload.theses_from",
        ),
        invalid(
            "from_shorter",
            "rule 145: one entry per stream in each list",
            EVALUATION,
            [change("payload.theses_from", [f"{AGENT_STREAM}:2"])],
            "schema",
            "payload.theses_from",
        ),
        invalid(
            "head_as_ref",
            "§9.18 (DEC-1142): a head is a bare record hash (digest), never a sha256: ref",
            EVALUATION,
            [change("payload.theses_head", ["sha256:" + head for head in HEADS])],
            "non_canonical",
            "payload.theses_head[0]",
            also=[("non_canonical", "payload.theses_head[1]")],
        ),
        control_invalid(
            f"{EVALUATION}.head_listed_as_artifact",
            "§9 digest (DEC-1142): a record hash names no stored artifact, so it is not in artifact_refs",
            snake(EVALUATION),
            [change("artifact_refs", sorted([*digest_strings(PAYLOADS[EVALUATION]), "sha256:" + HEADS[0]]))],
            "artifact_refs",
            "artifact_refs",
        ),
        invalid(
            "from_empty",
            "rule 145: not empty",
            EVALUATION,
            [change("payload.theses_from", [])],
            "schema",
            "payload.theses_from",
            also=[("schema", "payload.theses_from")],
        ),
    ]
    return out


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule, or the rules its `also` lists; together they cover every
    member type and rule."""
    return [*member_drafts(), *rule_drafts()]


def valid_drafts() -> list[dict]:
    """The cases a rule might be misread to refuse; each must be accepted."""
    return [
        valid(
            "refused",
            "rule 143: a refused run, no report, its reason code",
            BACKTEST,
            [change("payload.result", "refused"), change("payload.report", None), change("payload.refusal", "no_bars")],
        ),
        valid("by_the_owner", "DEC-1141: the user who asked", BACKTEST, [change("actor", dict(USER))]),
        valid("by_the_founder", "DEC-1141: the user who asked", EVALUATION, [change("actor", dict(USER))]),
        valid(
            "by_control_services",
            "DEC-1141: any system actor",
            BACKTEST,
            [change("actor", {"kind": "system", "id": "control_services", "version": "0.1.0", "build": ref("d")})],
        ),
        valid(
            "extra_refs",
            "§9 catalogue: required refs are a floor, not a closed set",
            BACKTEST,
            [change("config_refs.fee_config", ref("e"))],
        ),
        valid(
            "same_digest_twice",
            "§3: artifact_refs is a set, so a digest named twice is listed once",
            BACKTEST,
            [change("payload.theses", STORED_REF["model_outputs"])],
        ),
        valid(
            "configuration_names_the_version",
            "rule 144 binds mandate_version only",
            BACKTEST,
            [change("payload.configuration", VERSION)],
        ),
        valid(
            "one_stream",
            "rule 145: one agent stream read",
            EVALUATION,
            [
                change("payload.theses_from", [f"{AGENT_STREAM}:2"]),
                change("payload.theses_to", [f"{AGENT_STREAM}:41"]),
                change("payload.theses_head", [HEADS[0]]),
            ],
        ),
        valid(
            "heads_not_artifacts",
            "§9 digest (DEC-1142): artifact_refs lists the refs only, never a thesis head",
            EVALUATION,
            [change("artifact_refs", sorted(digest_strings(PAYLOADS[EVALUATION])))],
        ),
        valid(
            "one_record_per_stream",
            "rule 145: the first and the last record read may be the same",
            EVALUATION,
            [change("payload.theses_to", [f"{AGENT_STREAM}:2", f"{SECOND_AGENT}:5"])],
        ),
        valid(
            "same_head",
            "rule 145 counts entries; it does not require distinct hashes",
            EVALUATION,
            [change("payload.theses_head", [HEADS[0]] * 2)],
        ),
        valid("not_passed", "§9.18: an evaluation that did not pass is recorded", EVALUATION, [change("payload.passed", False)]),
        valid(
            "with_a_mandate_version",
            "§9 catalogue: an evaluation may carry refs it does not require",
            EVALUATION,
            [change("config_refs.mandate_version", VERSION)],
        ),
        valid("caused", "§9.18: no rule reads the cause", BACKTEST, [change("causation_id", CAUSE)]),
        valid("caused", "§9.18: no rule reads the cause", EVALUATION, [change("causation_id", CAUSE)]),
        valid("live", "§9.18: no rule reads the environment", BACKTEST, [change("environment", "live")]),
    ]


# --------------------------------------------------------------------------- independent oracle

ORACLE_CHECKS = (
    "artifacts.stored",
    "drafts.valid",
    "drafts.records",
    "drafts.marker",
    "drafts.artifacts",
    "drafts.binding",
    "drafts.ranges",
    "invalid_drafts",
    "valid_drafts",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def digests_in(value) -> list[str]:
    """Every `sha256:` string anywhere in a payload, read without the validator's types."""
    if isinstance(value, dict):
        return [s for v in value.values() for s in digests_in(v)]
    if isinstance(value, list):
        return [s for v in value for s in digests_in(v)]
    return [value] if isinstance(value, str) and value.startswith("sha256:") else []


def ranges_problem(p: dict) -> str | None:
    """The oracle's own reading of rule 145 and §9.18's ranges: per agent stream read, in stream
    order, a first and a last `stream_id:seq` on the same agent stream, the first no later, and the
    last record's hash."""
    starts, ends, heads = p.get("theses_from"), p.get("theses_to"), p.get("theses_head")
    if not (isinstance(starts, list) and isinstance(ends, list) and isinstance(heads, list)) or not starts:
        return "no agent stream read"
    if not len(starts) == len(ends) == len(heads):
        return "the ranges and heads differ in count"
    streams = []
    for start, end, head in zip(starts, ends, heads, strict=True):
        first, last = POSITION.match(str(start)), POSITION.match(str(end))
        if not (first and last) or first.group(1) != last.group(1):
            return f"{start} to {end} is not a range on one agent stream"
        if int(first.group(2)) > int(last.group(2)):
            return f"{start} is after {end}"
        if not HEAD_FORM.match(str(head)):
            return f"{head} is not a bare record hash"
        streams.append(first.group(1))
    if len(set(streams)) != len(streams):
        return "an agent stream is read twice"
    return None


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    drafts = section["drafts"]
    records = {snake(r): r for r in BACKTEST_RECORDS}
    stored = set()
    for artifact in section["artifacts"]:
        text = artifact["canonical"]
        digest = "sha256:" + hashlib.sha256(text.encode("utf-8")).hexdigest()
        if digest != artifact["ref"] or json.loads(text) != artifact["object"]:
            problems.append(found("artifacts.stored", f"artifact {artifact['name']} is not its reference's bytes"))
        stored.add(digest)
    for name, draft in drafts.items():
        named = set(draft.get("artifact_refs", [])) | set((draft.get("config_refs") or {}).values())
        if not named <= stored:
            problems.append(found("artifacts.stored", f"base draft {name} names {sorted(named - stored)}, not stored"))
    if sorted(drafts) != sorted(records):
        problems.append(found("drafts.records", f"base drafts {sorted(drafts)} are not the two §9.18 records"))
    for name, draft in drafts.items():
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
        record = records.get(name)
        if draft.get("event_type") != record or draft.get("stream_id", "").split(":")[0] != "ctl":
            problems.append(found("drafts.records", f"base draft {name} is not that record on a control stream"))
        p = draft.get("payload") or {}
        if p.get("report_version") != REPORT_VERSIONS.get(record) or (
            record == BACKTEST and p.get("marker") != "backtest"
        ):
            problems.append(found("drafts.marker", f"base draft {name} is not its report version and marker"))
        if draft.get("artifact_refs") != sorted(set(digests_in(p))):
            problems.append(found("drafts.artifacts", f"base draft {name} does not list every digest it names"))
        heads = p.get("theses_head") if isinstance(p.get("theses_head"), list) else []
        listed = {str(r).removeprefix("sha256:") for r in draft.get("artifact_refs", [])}
        if listed & {str(h).removeprefix("sha256:") for h in heads}:
            problems.append(found("drafts.artifacts", f"base draft {name} lists a thesis head as an artifact"))
        if record == BACKTEST:
            bound = (draft.get("config_refs") or {}).get("mandate_version")
            completed = p.get("result") == "completed" and p.get("report") is not None and p.get("refusal") is None
            if bound != p.get("mandate_version") or not completed:
                problems.append(found("drafts.binding", f"base draft {name} is not a completed run of its bound version"))
        if record == EVALUATION:
            problem = ranges_problem(p)
            if problem:
                problems.append(found("drafts.ranges", f"base draft {name}: {problem}"))
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
    "types.str_nonempty",
    "types.timestamp",
    "types.digest",
    "config_refs.required",
    "rule.143.report",
    "rule.143.refusal",
    "rule.144",
    "rule.145.empty",
    "rule.145.length",
    "rule.writer",
)


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    """Seeded bugs in the vectors, each registered against the one check that must reject it."""

    def mutated(fn) -> dict:
        out = copy.deepcopy(section)
        fn(out)
        return out

    def case(s, kind, name):
        return next(c for c in s[kind] if c["name"] == name)

    bt, ev = snake(BACKTEST), snake(EVALUATION)
    return [
        (
            "a stored artifact's bytes differ from its reference",
            "artifacts.stored",
            mutated(lambda s: s["artifacts"][0].update(canonical=s["artifacts"][0]["canonical"].replace("mandate", "mandat"))),
        ),
        (
            "a base draft is written by an agent",
            "drafts.valid",
            mutated(lambda s: s["drafts"][bt].update(actor=dict(RUNTIME))),
        ),
        (
            "a base draft is filed as the other record",
            "drafts.records",
            mutated(lambda s: s["drafts"][ev].update(event_type=BACKTEST)),
        ),
        (
            "a backtest's base draft names E4-2's baseline report version",
            "drafts.marker",
            mutated(lambda s: s["drafts"][bt]["payload"].update(report_version=1)),
        ),
        (
            "a base draft leaves its report out of artifact_refs",
            "drafts.artifacts",
            mutated(lambda s: s["drafts"][ev]["artifact_refs"].remove(PAYLOADS[EVALUATION]["report"])),
        ),
        (
            "the backtest's base draft is a refused run",
            "drafts.binding",
            mutated(
                lambda s: s["drafts"][bt]["payload"].update(result="refused", report=None, refusal="no_bars")
                or s["drafts"][bt].update(artifact_refs=sorted(digest_strings(s["drafts"][bt]["payload"])))
            ),
        ),
        (
            "the evaluation's base draft reads a range backwards",
            "drafts.ranges",
            mutated(lambda s: s["drafts"][ev]["payload"].update(theses_to=[f"{AGENT_STREAM}:1", f"{SECOND_AGENT}:5"])),
        ),
        (
            "the evaluation's base draft lists a thesis head as an artifact",
            "drafts.artifacts",
            mutated(lambda s: s["drafts"][ev]["artifact_refs"].append("sha256:" + HEADS[0])),
        ),
        (
            "the evaluation's base draft names its heads as refs",
            "drafts.ranges",
            mutated(
                lambda s: s["drafts"][ev]["payload"].update(theses_head=["sha256:" + head for head in HEADS])
                or s["drafts"][ev].update(artifact_refs=sorted(digest_strings(s["drafts"][ev]["payload"])))
            ),
        ),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(
                lambda s: case(s, "invalid_drafts", f"{BACKTEST}.refused_without_refusal")["expect"].update(
                    path="payload.report"
                )
            ),
        ),
        (
            "a valid draft breaks rule 145",
            "valid_drafts",
            mutated(
                lambda s: case(s, "valid_drafts", f"{EVALUATION}.one_stream")["changes"].append(
                    change("payload.theses_head", [])
                )
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
            escaped.append(f"backtest-record validator mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"backtest-record check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(
                f"backtest-record vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})"
            )
    return escaped


# --------------------------------------------------------------------------- output


def artifacts() -> list[dict]:
    """Every object a base draft names, by its reference, with its canonical text (§6.3)."""
    return [
        {"name": name, "ref": STORED_REF[name], "object": copy.deepcopy(obj), "canonical": canon(obj)}
        for name, obj in STORED.items()
    ]


def build_section() -> dict:
    return {
        "spec": SPEC,
        "artifacts": artifacts(),
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
    }
