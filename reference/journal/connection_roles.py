"""Journal spec v0.77 §9.33's reference vectors for the role provisioner's records (DEC-2350): the
Kubernetes-auth roles one OAuth connect's token exchange and executor log in with, made on the
request; the exchange's is removed when the request closes without an establishment or the
connection is revoked, and the executor's never is.

The schemas and rule 261 live in `control.py`, beside the other closed schemas, so one validator
judges every record. Stream rules 262 and 263 need the stream, so they live here, as rule 254 does in
`step_up_assertions.py`. This module builds the `connection_roles` section: a base draft of each
record, an invalid draft for every member type and each clause of rule 261, valid drafts for the
cases it might be misread to refuse, and control streams for rules 262 and 263. Its own oracle
rebuilds each record's history directly, without the validator, and every seeded bug is shown
caught.
"""

from __future__ import annotations

import copy
import re

import connection_requested
import connections
from common import apply_change, change, delete
from control import (
    ACCOUNT_STREAM_REF,
    AGENT_STREAM,
    CR_RECORDS,
    SCHEMAS,
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

SPEC = "docs/specs/journal.md v0.77 §9.33 (DEC-2350)"
AT = "2026-10-22T16:00:00.000000000Z"
CONNECTION = connections.CONNECTION
OTHER_CONNECTION = connections.OTHER_CONNECTION
ACCOUNT = ACCOUNT_STREAM_REF
OTHER_ACCOUNT = connections.OTHER_ACCOUNT_REF
REQUEST = connection_requested.EVENT_IDS[connection_requested.OAUTH]
CLOSER = "01J8Z4CR0A000000000000000C"
OPAQUE = re.compile(r"^[A-Za-z0-9_-]{1,64}\Z")

PAYLOADS: dict[str, dict] = {
    "ConnectionRolesMade": {"connection_id": CONNECTION},
    "ConnectionRolesRefused": {"connection_id": CONNECTION, "reason": "vault_unavailable"},
    "ConnectionRolesRemoved": {"connection_id": CONNECTION, "role": "token_exchange", "occasion": "request_closed"},
}
CAUSES = {"ConnectionRolesMade": REQUEST, "ConnectionRolesRefused": REQUEST, "ConnectionRolesRemoved": CLOSER}
EVENT_IDS = {name: f"01J8Z4CR{i:02d}".ljust(26, "0") for i, name in enumerate(CR_RECORDS)}


def snake(record: str) -> str:
    """A record's base-draft key: §4.1's key grammar admits no capital letter."""
    return re.sub(r"(?<!^)(?=[A-Z])", "_", record).lower()


def role_record(name: str, event_id: str | None = None, cause: str | None = None, **payload) -> dict:
    """One §9.33 record, by the provisioner, its payload the base's with `payload` over it."""
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": event_id or EVENT_IDS[name],
        "stream_id": STREAM,
        "event_type": name,
        "schema_version": 1,
        "event_time": AT,
        "clock_source": "local",
        "causation_id": cause or CAUSES[name],
        "correlation_id": None,
        "actor": dict(SERVICES),
        "config_refs": {},
        "payload": {**copy.deepcopy(PAYLOADS[name]), **payload},
        "artifact_refs": [],
        "pii_refs": [],
    }


def base_drafts() -> dict[str, dict]:
    return {snake(name): role_record(name) for name in CR_RECORDS}


def invalid(name, clause, base, changes, reason, path):
    return control_invalid(f"{base}.{name}", clause, snake(base), changes, reason, path)


def valid(name, clause, base, changes):
    return control_valid(f"{base}.{name}", clause, snake(base), changes)


def member_drafts() -> list[dict]:
    """Per record, per member: a value of the wrong JSON kind (`schema`), a string of the wrong
    form (`non_canonical`), and `null` (`schema`), since no member is nullable; and per record one
    missing and one extra member."""
    out = []
    for record in CR_RECORDS:
        for member, ty in SCHEMAS[("ctl", record)].fields:
            path = f"payload.{member}"
            out.append(invalid(f"{member}.kind", "§9.33 types", record, [change(path, 7)], "schema", path))
            form = {"id": "not an id", "one_of": "not_listed"}[ty.kind]
            out.append(invalid(f"{member}.form", "§9.33 types", record, [change(path, form)], "non_canonical", path))
            out.append(invalid(f"{member}.null", "§9.33 types", record, [change(path, None)], "schema", path))
        out.append(
            invalid("missing", "§9.33 closed", record, [delete("payload.connection_id")], "schema", "payload.connection_id")
        )
        out.append(invalid("extra", "§9.33 closed", record, [change("payload.role_name", "x")], "schema", "payload.role_name"))
    return out


def rule_drafts() -> list[dict]:
    out = []
    for record in CR_RECORDS:
        out += [
            invalid(
                "by_a_user",
                "rule 261: the provisioner writes it as a system actor",
                record,
                [change("actor", dict(USER))],
                "schema",
                "actor.kind",
            ),
            invalid("uncaused", "rule 261: it names its cause", record, [change("causation_id", None)], "schema", "causation_id"),
            invalid(
                "on_an_agent_stream",
                "§9 row: a control-stream record",
                record,
                [change("stream_id", AGENT_STREAM)],
                "wrong_stream",
                "event_type",
            ),
            invalid(
                "at_version_2",
                "§9.33: closed at schema version 1 only",
                record,
                [change("schema_version", 2)],
                "unknown_schema",
                "payload",
            ),
        ]
    return out


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule; together they cover every member type and rule 261, and
    the executor's role, which is never removed (DEC-2350 item 3)."""
    executor = invalid(
        "of_the_executor",
        "§9.33: the executor's role is never removed",
        "ConnectionRolesRemoved",
        [change("payload.role", "executor"), change("payload.occasion", "revoked")],
        "non_canonical",
        "payload.role",
    )
    return [*member_drafts(), *rule_drafts(), executor]


def valid_drafts() -> list[dict]:
    """The cases rule 261 or the types might be misread to refuse; each must be accepted."""
    removed, refused = "ConnectionRolesRemoved", "ConnectionRolesRefused"
    return [
        valid("refused_by_the_vault", "§9.33: a role the vault refused", refused, [change("payload.reason", "role_refused")]),
        valid(
            "exchange_on_revoke",
            "§9.33: the exchange's role, on a revocation",
            removed,
            [change("payload.occasion", "revoked")],
        ),
        valid("in_live", "§9.33: either environment", "ConnectionRolesMade", [change("environment", "live")]),
    ]


# --------------------------------------------------------------------------- stream rules 262 and 263


def stream_id(index: int) -> str:
    return f"01J8Z4CS{index:02d}".ljust(26, "0")


def requested(index: int, credential: str = "oauth", connection: str = CONNECTION, account: str = ACCOUNT) -> dict:
    """A version-2 `ConnectionRequested`: an OAuth connect's, or an API-key one's."""
    record = connection_requested.request(connection_requested.OAUTH if credential == "oauth" else connection_requested.API_KEY)
    record["event_id"] = stream_id(index)
    record["payload"]["connection_id"] = connection
    record["payload"]["account_ref"] = account
    return record


def closed(index: int, request: dict) -> dict:
    """The `ConnectionRefused` with occasion `connect` that closes `request` (rule 131)."""
    record = copy.deepcopy(connections.BASES["refused_scope"])
    record["event_id"] = stream_id(index)
    for member in ("connection_id", "broker", "environment", "user", "step_up"):
        record["payload"][member] = copy.deepcopy(request["payload"][member])
    record["payload"]["occasion"] = "connect"
    return record


def revoked(index: int, connection: str = CONNECTION) -> dict:
    record = copy.deepcopy(connections.BASES["rotated"])
    for item in (
        change("event_id", stream_id(index)),
        change("event_type", "ConnectionRevoked"),
        change("payload", {"connection_id": connection}),
    ):
        apply_change(record, item)
    return record


def process(index: int, kind: str, generation: int, account: str = ACCOUNT) -> dict:
    """The manager's start or stop of one generation of `account`'s executor (§9.21)."""
    start = kind == "start"
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": stream_id(index),
        "stream_id": STREAM,
        "event_type": "ProcessStartIssued" if start else "ProcessStopIssued",
        "schema_version": 1,
        "event_time": AT,
        "clock_source": "local",
        "causation_id": None,
        "correlation_id": None,
        "actor": dict(SERVICES),
        "config_refs": {},
        "payload": {
            "process": "executor",
            "subject": account,
            "generation": generation,
            "cause": ("deployed" if generation == 1 else "restart") if start else "agent_stopped",
        },
        "artifact_refs": [],
        "pii_refs": [],
    }


def made(index: int, request: dict, connection: str | None = None) -> dict:
    return role_record("ConnectionRolesMade", stream_id(index), request["event_id"],
                       connection_id=connection or request["payload"]["connection_id"])


def refused(index: int, request: dict) -> dict:
    return role_record("ConnectionRolesRefused", stream_id(index), request["event_id"],
                       connection_id=request["payload"]["connection_id"])


def removed(index: int, cause: dict, role: str, occasion: str) -> dict:
    return role_record("ConnectionRolesRemoved", stream_id(index), cause["event_id"],
                       connection_id=cause["payload"]["connection_id"], role=role, occasion=occasion)


def stream_failure(records: list[dict], skip: frozenset[str] = frozenset()) -> int | None:
    """The index of the first §9.33 record that breaks rule 262 or 263, or None."""
    requests: dict[str, dict] = {}
    answered: set[str] = set()
    made_for: set[str] = set()
    gone: set[str] = set()
    closed_requests: set[str] = set()
    revoked_connections: set[str] = set()
    for i, record in enumerate(records):
        t, p = record["event_type"], record["payload"]
        if t == "ConnectionRequested":
            requests[record["event_id"]] = record
        elif t == "ConnectionRefused" and p.get("occasion") == "connect":
            closed_requests.add(p["connection_id"])
        elif t == "ConnectionRevoked":
            revoked_connections.add(p["connection_id"])
        elif t in ("ConnectionRolesMade", "ConnectionRolesRefused"):
            request = requests.get(record["causation_id"])
            oauth = request is not None and request["schema_version"] == 2 and request["payload"].get("credential") == "oauth"
            if not oauth and "stream.262.request" not in skip:
                return i
            if request is not None and request["payload"]["connection_id"] != p["connection_id"] and "stream.262.connection" not in skip:
                return i
            if record["causation_id"] in answered and "stream.262.once" not in skip:
                return i
            answered.add(record["causation_id"])
            if t == "ConnectionRolesMade" and request is not None:
                made_for.add(p["connection_id"])
                gone.discard(p["connection_id"])
        elif t == "ConnectionRolesRemoved":
            connection = p["connection_id"]
            if connection not in made_for and "stream.263.made" not in skip:
                return i
            if connection in gone and "stream.263.once" not in skip:
                return i
            if p["occasion"] == "request_closed" and connection not in closed_requests and "stream.263.closed" not in skip:
                return i
            if p["occasion"] == "revoked" and connection not in revoked_connections and "stream.263.revoked" not in skip:
                return i
            gone.add(connection)
    return None


def stream_cases() -> list[dict]:
    def case(name, clause, records, index):
        expect = {"outcome": "Valid"} if index is None else {"outcome": "Invalid", "rule": clause.split(":")[0], "index": index}
        return {"name": name, "clause": clause, "records": records, "expect": expect}

    oauth = requested(0)
    api_key = requested(0, "api_key")
    other = requested(1, connection=OTHER_CONNECTION, account=OTHER_ACCOUNT)
    refusal = closed(2, oauth)
    revocation = revoked(3)
    return [
        case("made", "rule 262: after its OAuth request", [oauth, made(1, oauth)], None),
        case("refused", "rule 262: a refusal after its OAuth request", [oauth, refused(1, oauth)], None),
        case("made_without_a_request", "rule 262: names a request", [made(0, oauth)], 0),
        case("made_for_an_api_key_connect", "rule 262: an OAuth request alone", [api_key, made(1, api_key)], 1),
        case("made_for_another_connection", "rule 262: of the same connection", [oauth, made(1, oauth, OTHER_CONNECTION)], 1),
        case("made_twice", "rule 262: at most one per request", [oauth, made(1, oauth), made(2, oauth)], 2),
        case("made_then_refused", "rule 262: at most one of either per request", [oauth, made(1, oauth), refused(2, oauth)], 2),
        case("two_requests", "rule 262: one per request, each its own", [oauth, other, made(2, oauth), made(3, other)], None),
        case(
            "removed_on_close",
            "rule 263: the exchange's role once the request closed",
            [oauth, made(1, oauth), refusal, removed(3, refusal, "token_exchange", "request_closed")],
            None,
        ),
        case("removed_never_made", "rule 263: after its roles were made", [oauth, refusal, removed(3, refusal, "token_exchange", "request_closed")], 2),
        case(
            "removed_twice",
            "rule 263: at most once after its roles were made",
            [oauth, made(1, oauth), refusal, removed(3, refusal, "token_exchange", "request_closed"),
             removed(4, refusal, "token_exchange", "request_closed")],
            4,
        ),
        case(
            "removed_on_close_while_open",
            "rule 263: a closed request's",
            [oauth, made(1, oauth), removed(2, refusal, "token_exchange", "request_closed")],
            2,
        ),
        case(
            "removed_on_revoke_before_it",
            "rule 263: a revoked connection's",
            [oauth, made(1, oauth), removed(2, revocation, "token_exchange", "revoked")],
            2,
        ),
        case(
            "revoked_exchange_at_once",
            "rule 263: the exchange's role at once on a revocation, its executor still running",
            [oauth, made(1, oauth), process(2, "start", 1), revocation, removed(4, revocation, "token_exchange", "revoked")],
            None,
        ),
        case(
            "removed_twice_on_revoke",
            "rule 263: at most once after its roles were made",
            [oauth, made(1, oauth), revocation, removed(4, revocation, "token_exchange", "revoked"),
             removed(5, revocation, "token_exchange", "revoked")],
            4,
        ),
    ]


# --------------------------------------------------------------------------- independent oracle

ORACLE_CHECKS = (
    "drafts.valid",
    "drafts.records",
    "drafts.no_text",
    "invalid_drafts",
    "valid_drafts",
    "streams.records",
    "streams.rule",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def plain_strings(value) -> list[str]:
    """Every string in a payload that is not an opaque ID or listed value: the oracle's own reading
    of "no content", done without the validator's types."""
    if isinstance(value, dict):
        return [s for v in value.values() for s in plain_strings(v)]
    if isinstance(value, str) and not OPAQUE.match(value):
        return [value]
    return []


def oracle(records: list[dict]) -> int | None:
    """Rules 262 and 263 from each record's own history, rebuilt at every §9.33 record."""
    for i, record in enumerate(records):
        t, p, before = record["event_type"], record["payload"], records[:i]
        if t in ("ConnectionRolesMade", "ConnectionRolesRefused"):
            request = next((r for r in before if r["event_id"] == record["causation_id"]), None)
            if request is None or request["event_type"] != "ConnectionRequested" or request["schema_version"] != 2:
                return i
            if request["payload"].get("credential") != "oauth" or request["payload"]["connection_id"] != p["connection_id"]:
                return i
            if any(r["event_type"] in ("ConnectionRolesMade", "ConnectionRolesRefused") and r["causation_id"] == record["causation_id"]
                   for r in before):
                return i
        elif t == "ConnectionRolesRemoved":
            connection = p["connection_id"]
            makes = [j for j, r in enumerate(before) if r["event_type"] == "ConnectionRolesMade" and r["payload"]["connection_id"] == connection]
            if not makes:
                return i
            since = before[makes[-1]:]
            if any(r["event_type"] == "ConnectionRolesRemoved" and r["payload"]["connection_id"] == connection for r in since):
                return i
            closers = {"request_closed": "ConnectionRefused", "revoked": "ConnectionRevoked"}
            if not any(r["event_type"] == closers[p["occasion"]] and r["payload"]["connection_id"] == connection
                       and (r["event_type"] == "ConnectionRevoked" or r["payload"].get("occasion") == "connect") for r in before):
                return i
    return None


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    drafts = section["drafts"]
    if sorted(drafts) != sorted(snake(r) for r in CR_RECORDS):
        problems.append(found("drafts.records", f"base drafts {sorted(drafts)} are not the three §9.33 records"))
    records = {snake(r): r for r in CR_RECORDS}
    for name, draft in drafts.items():
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
        if draft.get("event_type") != records.get(name) or draft.get("actor", {}).get("kind") != "system":
            problems.append(found("drafts.records", f"base draft {name} is not that record by a system actor"))
        if plain_strings(draft.get("payload")):
            problems.append(found("drafts.no_text", f"base draft {name} carries text"))
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
        want = case["expect"].get("index")
        if stream_failure(case["records"]) != want or oracle(case["records"]) != want:
            problems.append(found("streams.rule", f"{case['name']}: expected {case['expect']}"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = ("record.extra", "record.missing", "rule.261.system", "rule.261.cause")
STREAM_MUTANTS = (
    "stream.262.request",
    "stream.262.connection",
    "stream.262.once",
    "stream.263.made",
    "stream.263.once",
    "stream.263.closed",
    "stream.263.revoked",
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
            "a base draft is written by a user",
            "drafts.valid",
            mutated(lambda s: s["drafts"]["connection_roles_made"].update(actor=dict(USER))),
        ),
        (
            "a base draft is filed as another record",
            "drafts.records",
            mutated(lambda s: s["drafts"]["connection_roles_removed"].update(event_type="ConnectionRolesMade")),
        ),
        (
            "a base draft carries free text in an id-shaped member",
            "drafts.no_text",
            mutated(lambda s: s["drafts"]["connection_roles_made"]["payload"].update(connection_id="x" * 65)),
        ),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "ConnectionRolesMade.uncaused")["expect"].update(path="actor.kind")),
        ),
        (
            "a valid draft has no cause",
            "valid_drafts",
            mutated(lambda s: case(s, "valid_drafts", "ConnectionRolesMade.in_live")["changes"].append(change("causation_id", None))),
        ),
        (
            "a stream record is written by a user",
            "streams.records",
            mutated(lambda s: case(s, "streams", "made")["records"][1].update(actor=dict(USER))),
        ),
        (
            "a stream case lets a removal repeat",
            "streams.rule",
            mutated(lambda s: case(s, "streams", "removed_twice").update(expect={"outcome": "Valid"})),
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
            escaped.append(f"connection-roles validator mutant {mutant}")
    for mutant in STREAM_MUTANTS:
        skip = frozenset([mutant])
        if not any(stream_failure(c["records"], skip) != c["expect"].get("index") for c in section["streams"]):
            escaped.append(f"connection-roles stream mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"connection-roles check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"connection-roles vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
        "streams": stream_cases(),
    }
