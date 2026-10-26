"""Journal spec v0.54 §9.24's reference vectors (DEC-1171): `AgentDeployed` version 2, which names
the connection of the stored mandate document its version names, so the deployment manager (§9.21)
learns a deployment's account from the control stream alone.

The schema lives in `control.py`, beside version 1, so one validator judges every closed schema.
This module builds the `agent_deployed` section:

1. base drafts of both versions, the stored mandate document and deployment record they name, and
   the connection records a control stream holds around them;
2. an invalid draft for every member type of version 2 and for rule 22 at version 2, and valid
   drafts for the cases a rule might be misread to refuse, among them a mismatch that `append`
   leaves to the mapping (rule 200);
3. mapping cases for rule 200: §9.2's mapping to `AgentVersionActive`, judged where the document is
   read, refusing a version-2 record whose connection differs from the document's;
4. control streams for the stream rule 201, each record judged from the stream's start, with the
   processes §9.21's deployment manager runs for that stream, typed by hand;
5. ranges of those streams for §11's `connection_lifecycle_mismatch`, judged from a connection
   anchor or failing closed without one. Rules 66 to 68 and 131 are `connections.py`'s; here a range
   judges rule 201 only, and fails closed at every record §11's unanchored run judges;
6. pairs of an agent stream and a control stream for the stream rule 246 (v0.66, DEC-2039): a
   workspace service that takes the owner's Stop claims the agent's stream by opening it as an
   actor other than `agent`, writes nothing more there, and retires the agent with reason
   `owner_stop` and no loss; an `AgentStopped` the agent's runtime writes is not judged. They are a
   list of their own, `stops`, so the readers of `streams` keep their meaning.

Every stream record passes every per-draft rule, every expectation is typed from the rules rather
than computed by the code under test, and every seeded bug, in the validator, the mapping, the
stream rule, the range reading, the manager's reading, and the vectors, is shown caught by the
check it is registered against.
"""

from __future__ import annotations

import copy

import clients
from common import (
    apply_change,
    artifact_ref,
    canon,
    change,
    delete,
    digest_strings,
    normalize_decimal,
    sha256_hex,
)
from control import (
    ACCOUNT_STREAM_REF,
    AGENT_STREAM,
    EXECUTOR,
    RUNTIME,
    SERVICES,
    STREAM,
    USER,
    WORKSPACE,
    artifacts,
    check_of,
    envelope_violations,
    reported,
    violations,
)

SPEC = "docs/specs/journal.md v0.66 §9.21, §9.24 (DEC-1171, DEC-1173, DEC-2039)"
AT = "2026-10-17T16:00:00.000000000Z"
ARTIFACTS = {name: obj for name, obj in artifacts().items() if name in ("mandate_document", "deployment_record")}
DOCUMENT = ARTIFACTS["mandate_document"]
VERSION = artifact_ref(DOCUMENT)
RECORD = artifact_ref(ARTIFACTS["deployment_record"])
OTHER_VERSION = VERSION[:-1] + ("0" if VERSION[-1] != "0" else "1")
CONNECTION = DOCUMENT["connection_id"]
OTHER_CONNECTION = "conn_alpaca_paper_02"
ACCOUNT = ACCOUNT_STREAM_REF
OTHER_ACCOUNT = "01J8Z2ACCT00000000000000B2"
AGENT = "agent_a"
OTHER_AGENT = "agent_b"
CHECKED = "01J9DA00C000000000000000K1"
PII = "pii_01J8Z2PR0000000000000000A1"
STEP_UP = {"assertion_id": "assert_owner_11", "authenticated_at": "2026-10-17T15:59:50.000000000Z", "method": "cli_confirm"}
NOTHING = {"runtimes": [], "executors": []}
# The record types §11's unanchored run fails closed at on a control stream (§11, rule 201 added).
JUDGED = ("ConnectionRequested", "ConnectionEstablished", "ConnectionCredentialRotated", "ConnectionRefused")


def envelope(event_id: str, event_type: str, payload: dict, version: int = 1, **more) -> dict:
    draft = {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": event_id,
        "stream_id": STREAM,
        "event_type": event_type,
        "schema_version": version,
        "event_time": AT,
        "clock_source": "local",
        "causation_id": None,
        "correlation_id": None,
        "actor": dict(USER),
        "config_refs": {},
        "payload": payload,
        "artifact_refs": [],
        "pii_refs": [],
    }
    draft.update(more)
    draft["artifact_refs"] = sorted(digest_strings(draft["payload"]))
    return draft


def deployed(event_id: str, agent: str = AGENT, connection: str | None = CONNECTION) -> dict:
    """A deployment of the base version: version 2 naming `connection`, or version 1 when `None`."""
    payload = {"agent_id": agent, "mandate_version": VERSION, "record_ref": RECORD}
    if connection is None:
        return envelope(event_id, "AgentDeployed", payload, config_refs={"mandate_version": VERSION})
    return envelope(
        event_id, "AgentDeployed", {**payload, "connection_id": connection}, 2, config_refs={"mandate_version": VERSION}
    )


def established(event_id: str, connection: str = CONNECTION, account: str | None = ACCOUNT) -> dict:
    """A control-stream establishment: version 2 binding `account`, or version 1 when `None`."""
    payload = {"connection_id": connection, "broker": "alpaca", "environment": "paper", "scopes": ["data", "trading"]}
    if account is None:
        return envelope(event_id, "ConnectionEstablished", payload, actor=dict(SERVICES))
    payload |= {"account_ref": account, "user": USER["id"], "step_up": dict(STEP_UP), "margin_attestation": None}
    return envelope(event_id, "ConnectionEstablished", payload, 2, actor=dict(SERVICES), causation_id=CHECKED)


def on_account(draft: dict, **more) -> dict:
    """`draft` as the executor's record on the account stream of `ACCOUNT` (journal spec §9.8)."""
    draft.update(stream_id=f"acct:{WORKSPACE}:{ACCOUNT}", actor=dict(EXECUTOR), **more)
    return draft


def checked(event_id: str) -> dict:
    """The executor's passing `connect` check, which an account stream's establishment copy rests on (rule 68)."""
    payload = {
        "connection_id": CONNECTION,
        "occasion": "connect",
        "results": [{"check": c, "result": "passed", "reason": None} for c in ("account", "environment", "scope")],
        "account_pii_ref": PII,
        "risk_clock": AT,
    }
    return on_account(envelope(event_id, "ConnectionChecked", payload), pii_refs=[PII])


def copied(event_id: str, original: str) -> dict:
    """The account stream's copy of a version-2 establishment (§9.8): never a control-stream record."""
    draft = established(event_id)
    draft["payload"]["risk_clock"] = AT
    return on_account(draft, causation_id=original)


def revoked(event_id: str, connection: str = CONNECTION) -> dict:
    return envelope(event_id, "ConnectionRevoked", {"connection_id": connection}, actor=dict(SERVICES))


def stopped(event_id: str, agent: str = AGENT) -> dict:
    payload = {
        "agent_id": agent,
        "connection_id": CONNECTION,
        "reason": "owner_stop",
        "retired_on": "2026-10-17",
        "loss_added": "0",
    }
    return envelope(event_id, "AgentStopped", payload, config_refs={"mandate_version": VERSION})


def started(event_id: str, subject: str = AGENT, process: str = "runtime", generation: int = 1) -> dict:
    """The deployment manager's `ProcessStartIssued` (§9.21): the first start of a slot, or a restart."""
    payload = {
        "process": process,
        "subject": subject,
        "generation": generation,
        "cause": "deployed" if generation == 1 else "restart",
    }
    return envelope(event_id, "ProcessStartIssued", payload, actor=dict(SERVICES))


def retired(event_id: str, loss: str, reason: str = "owner_stop") -> dict:
    """The agent's runtime retiring it, with the loss its own risk state reads: rule 246 does not
    judge a runtime's record."""
    draft = stopped(event_id)
    draft["payload"].update(loss_added=loss, reason=reason)
    draft["actor"] = dict(RUNTIME)
    return draft


EARLIER_RUNTIME = {**RUNTIME, "version": "0.0.9", "build": "sha256:" + "b" * 64}


def opened(event_id: str, actor: dict) -> dict:
    """The agent stream's `StreamOpened` (§9.1): the runtime's, as an `agent` actor, or a claim by
    any other actor (rule 246)."""
    payload = {"stream_type": "agent", "workspace_id": WORKSPACE, "agent_id": AGENT}
    return envelope(event_id, "StreamOpened", payload, stream_id=AGENT_STREAM, actor=dict(actor))


def observed(event_id: str, actor: dict = RUNTIME) -> dict:
    """A record after the agent stream's `StreamOpened`: the runtime ran. Only its envelope is
    judged here; the agent stream's payloads are `agent_stream`'s."""
    payload = {"source": "fixture", "observed_at": AT, "payload_ref": None}
    return envelope(event_id, "ObservationRecorded", payload, stream_id=AGENT_STREAM, actor=dict(actor))


DRAFTS = {
    "deployed_v2": deployed("01J9DA00D000000000000000V2"),
    "deployed_v1": deployed("01J9DA00D000000000000000V1", connection=None),
    "established": established("01J9DA00E000000000000000E2"),
    "established_v1": established("01J9DA00E000000000000000E1", account=None),
    "revoked": revoked("01J9DA00R000000000000000R1"),
    "stopped": stopped("01J9DA00S000000000000000S1"),
}


def invalid(name, clause, base, changes, reason, path):
    expect = {"outcome": "Invalid", "reason": reason, "path": path}
    return {"name": f"{base}.{name}", "clause": clause, "base_draft": base, "changes": changes, "expect": expect}


def valid(name, clause, base, changes):
    return {"name": f"{base}.{name}", "clause": clause, "base_draft": base, "changes": changes, "expect": {"outcome": "Valid"}}


def draft_for(section: dict, case: dict) -> dict:
    draft = copy.deepcopy(section["drafts"][case["base_draft"]])
    for item in case["changes"]:
        apply_change(draft, item)
    return draft


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule; together they cover version 2's member and rule 22."""
    v2, v1 = "deployed_v2", "deployed_v1"
    other = change("payload.mandate_version", OTHER_VERSION)
    other_refs = change("artifact_refs", sorted([OTHER_VERSION, RECORD]))
    return [
        invalid("connection_id.kind", "§9.24 types", v2, [change("payload.connection_id", 7)], "schema", "payload.connection_id"),
        invalid("connection_id.form", "§9.24 types: an id", v2, [change("payload.connection_id", "not an id")], "non_canonical", "payload.connection_id"),
        invalid("connection_id.null", "§9.24: not nullable", v2, [change("payload.connection_id", None)], "schema", "payload.connection_id"),
        invalid("connection_id.missing", "§9.24: every member is present", v2, [delete("payload.connection_id")], "schema", "payload.connection_id"),
        invalid("extra", "§9.24: closed", v2, [change("payload.note", "x")], "schema", "payload.note"),
        invalid("at_version_4", "§9.24 v0.84: versions 1 to 3 only", v2, [change("schema_version", 4)], "unknown_schema", "payload"),
        invalid("rule_22", "rule 22 at version 2: the payload's version is the config ref's", v2, [other, other_refs], "schema", "payload.mandate_version"),
        invalid("no_mandate_ref", "§9: `AgentDeployed` requires `man` at version 2 too", v2, [change("config_refs", {})], "missing_config_ref", "config_refs.mandate_version"),
        invalid("by_a_client", "rule 83: a client never deploys (identity ID-11)", v2, [change("actor", dict(clients.CLIENT))], "schema", "actor.kind"),
        invalid("on_an_agent_stream", "§9 row: a control-stream record", v2, [change("stream_id", AGENT_STREAM)], "wrong_stream", "event_type"),
        invalid("connection_id", "§8: version 1 is unchanged, and has no `connection_id`", v1, [change("payload.connection_id", CONNECTION)], "schema", "payload.connection_id"),
    ]


def valid_drafts() -> list[dict]:
    """The cases a rule might be misread to refuse; each must be accepted."""
    v2 = "deployed_v2"
    return [
        valid("another_connection", "rule 200 is the mapping's: `append` reads no mandate document", v2, [change("payload.connection_id", OTHER_CONNECTION)]),
        valid("live", "§9.24: either environment", v2, [change("environment", "live")]),
        valid("longest_id", "§9.1: an id of 64 characters", v2, [change("payload.connection_id", "c" * 64)]),
    ]


# --------------------------------------------------------------------------- rule 200: the mapping


def named_versions(skip: frozenset[str]) -> tuple[int, ...]:
    """The `AgentDeployed` versions that name their connection: version 2, and version 3 (v0.84,
    DEC-2131), which every reader reads as version 2."""
    return (2,) if "v3.read_as_v1" in skip else (2, 3)


def stored(names: list[str]) -> dict[str, dict]:
    return {artifact_ref(ARTIFACTS[n]): ARTIFACTS[n] for n in names}


def map_deployed(draft: dict, documents: dict[str, dict], skip: frozenset[str] = frozenset()) -> dict:
    """§9.2's mapping of an `AgentDeployed`, either version: the fact, or the path it is refused at."""
    p = draft["payload"]
    v2 = draft["schema_version"] in named_versions(skip)
    if v2 and "map.v2_document_skipped" in skip:
        return {"refused": None, "fact": {"kind": "AgentVersionActive", "agent": p["agent_id"], "connection_id": p["connection_id"]}}
    document = documents.get(p["mandate_version"])
    if document is None:
        return {"refused": "payload.mandate_version"}
    judged = v2 or "map.200.v1" in skip
    if judged and p.get("connection_id") != document["connection_id"] and "map.200" not in skip:
        return {"refused": "payload.connection_id"}
    return {
        "refused": None,
        "fact": {
            "kind": "AgentVersionActive",
            "agent": p["agent_id"],
            "connection_id": document["connection_id"],
            "environment": document["environment"],
            "allocation_usd": normalize_decimal(document["capital"]["allocation_usd"]),
            "pinned": sorted(i["asset_id"] for i in document["universe"]["pinned_instruments"]),
        },
    }


def mapping_cases() -> list[dict]:
    fact = {
        "kind": "AgentVersionActive",
        "agent": AGENT,
        "connection_id": "conn_alpaca_paper_01",
        "environment": "paper",
        "allocation_usd": "10000",
        "pinned": ["7b4a1c2e-1111-4a2b-9c3d-000000000001"],
    }
    both = ["mandate_document", "deployment_record"]

    def case(name, clause, base, changes, documents, expect):
        return {"name": name, "clause": clause, "base_draft": base, "changes": changes, "stored": documents, "expect": expect}

    other = [change("payload.connection_id", OTHER_CONNECTION)]
    return [
        case("version_1", "§9.2: from the stored document", "deployed_v1", [], both, {"refused": None, "fact": fact}),
        case("version_2", "§9.24: the same fact from the same document", "deployed_v2", [], both, {"refused": None, "fact": fact}),
        case("version_2_live_envelope", "§9.24: the fact is the document's, whatever the envelope says", "deployed_v2", [change("environment", "live")], both, {"refused": None, "fact": fact}),
        case("another_connection", "rule 200: refused where the document is read", "deployed_v2", other, both, {"refused": "payload.connection_id"}),
        case("version_1_not_stored", "§9.2: a document that is not stored is refused", "deployed_v1", [], [], {"refused": "payload.mandate_version"}),
        case("version_2_not_stored", "§9.2: the document is read first, for either version", "deployed_v2", [], [], {"refused": "payload.mandate_version"}),
        case("another_connection_not_stored", "§9.2: with no document, rule 200 has nothing to compare", "deployed_v2", other, [], {"refused": "payload.mandate_version"}),
    ]


# --------------------------------------------------------------------------- rule 201 and §9.21


def new_state() -> dict:
    return {"v2": {}, "latest": {}}


def fold(state: dict, d: dict, skip: frozenset[str] = frozenset()) -> None:
    """Rule 201's fold of the control stream: an account stream's copy of a record is not the
    control stream's."""
    if d["stream_id"] != STREAM and "stream.201.account_copy" not in skip:
        return
    c = d["payload"].get("connection_id")
    if d["event_type"] == "ConnectionEstablished":
        if d["schema_version"] == 2:
            state["v2"].setdefault(c, d["payload"]["account_ref"])
        state["latest"][c] = "established"
    elif d["event_type"] == "ConnectionRevoked":
        state["latest"][c] = "revoked"


def breaks_201(state: dict, d: dict, skip: frozenset[str], later: list[dict]) -> bool:
    if d["event_type"] != "AgentDeployed":
        return False
    if d["schema_version"] not in named_versions(skip):
        if "stream.201.v1_judged" not in skip:
            return False
        connection = CONNECTION
    else:
        connection = d["payload"]["connection_id"]
    if "stream.201" in skip:
        return False
    v2, latest = state["v2"], state["latest"]
    if "stream.201.connection" in skip:
        return not v2
    if "stream.201.later" in skip and any(
        r["event_type"] == "ConnectionEstablished" and r["payload"]["connection_id"] == connection for r in later
    ):
        return False
    if "stream.201.later_revocation" in skip and any(
        r["event_type"] == "ConnectionRevoked" and r["payload"]["connection_id"] == connection for r in later
    ):
        return True
    has = connection in v2 or ("stream.201.version" in skip and connection in latest)
    live = latest.get(connection) == "established" or "stream.201.revoked" in skip
    return not (has and live)


def stream_failure(drafts: list[dict], skip: frozenset[str] = frozenset(), state: dict | None = None, base: int = 0) -> dict | None:
    """Rule 201 over control-stream records, from `state` (the stream's start when `None`): the first
    version-2 `AgentDeployed` whose connection was not established with an `account_ref` before it,
    or is revoked."""
    state = copy.deepcopy(state) if state is not None else new_state()
    for i, d in enumerate(drafts):
        if breaks_201(state, d, skip, drafts[i + 1 :]):
            return {"index": base + i, "rule": 201, "path": "payload.connection_id"}
        fold(state, d, skip)
    return None


def anchor(prefix: list[dict], skip: frozenset[str] = frozenset()) -> dict | None:
    """The connection anchor's part rule 201 reads: the fold of a prefix that breaks no rule, or
    none when it breaks one (§11: a broken prefix gives no anchor)."""
    if stream_failure(prefix, skip) is not None:
        return None
    state = new_state()
    for d in prefix:
        fold(state, d, skip)
    return state


def claimed_agents(agent_stream: list[dict], skip: frozenset[str]) -> set[str]:
    """The agents whose stream opens with a claim: a `StreamOpened` no `agent` actor wrote, told by
    the actor's kind alone, never by which process wrote it."""
    if not agent_stream or agent_stream[0]["event_type"] != "StreamOpened":
        return set()
    first = agent_stream[0]
    claim = first["actor"]["kind"] != "agent"
    if "stream.246.instance" in skip:
        claim = first["actor"] != RUNTIME
    if "stream.246.any_opener" in skip:
        claim = True
    return {first["payload"]["agent_id"]} if claim else set()


def stop_failure(agent_stream: list[dict], control: list[dict], skip: frozenset[str] = frozenset()) -> dict | None:
    """Rule 246 over an agent stream and a control stream, each from its start: the first record
    after a claim on the agent stream, else the first `AgentStopped` no runtime wrote that retires
    an agent whose stream no claim opened, or names another reason or a loss."""
    if "stream.246" in skip:
        return None
    claims = claimed_agents(agent_stream, skip)
    if claims and len(agent_stream) > 1 and "stream.246.after" not in skip:
        return {"stream": "agent", "index": 1, "rule": 246, "path": "actor.kind"}
    for i, d in enumerate(control):
        if d["event_type"] != "AgentStopped":
            continue
        if d["actor"]["kind"] == "agent" and "stream.246.runtime_judged" not in skip:
            continue
        p = d["payload"]
        claimed = p["agent_id"] in claims
        if "stream.246.any_agent" in skip:
            claimed = bool(claims)
        if not claimed:
            return {"stream": "control", "index": i, "rule": 246, "path": "payload.agent_id"}
        if p["reason"] != "owner_stop" and "stream.246.reason" not in skip:
            return {"stream": "control", "index": i, "rule": 246, "path": "payload.reason"}
        if normalize_decimal(p["loss_added"]) != "0" and "stream.246.loss" not in skip:
            return {"stream": "control", "index": i, "rule": 246, "path": "payload.loss_added"}
    return None


def range_failure(drafts: list[dict], start: int, anchored: bool, skip: frozenset[str] = frozenset()) -> dict | None:
    """§11's `connection_lifecycle_mismatch` for rule 201 over records `start` on: from the anchor of
    the records before it, or, with none, failing closed at the first record the unanchored run
    judges. A range from the stream's start is anchored on nothing before it."""
    records = drafts[start:]
    if anchored or start == 0:
        state = anchor(drafts[:start], skip) if "range.201.anchor_ignored" not in skip else new_state()
        if state is None:
            return {"index": start, "cause": "unanchored"}
        return stream_failure(records, skip, state, start)
    if "range.201.unanchored_open" in skip:
        return stream_failure(records, skip, None, start)
    for i, d in enumerate(records):
        v2_deployment = d["event_type"] == "AgentDeployed" and d["schema_version"] in named_versions(skip)
        if d["event_type"] in JUDGED or (v2_deployment and "range.201.unanchored_v1_only" not in skip):
            return {"index": start + i, "cause": "unanchored"}
    return None


def processes(drafts: list[dict], skip: frozenset[str] = frozenset()) -> dict:
    """§9.21's reading of the deployments: a runtime for each agent deployed and not stopped whose
    account the stream names, and an executor for each such account. The account is the
    `account_ref` of the version-2 deployment's connection, established before it and not since
    revoked (the deployments rule 201 admits); any other deployment starts nothing."""
    state = new_state()
    agents: dict[str, str] = {}
    gone: set[str] = set()
    for i, d in enumerate(drafts):
        p = d["payload"]
        if d["event_type"] == "AgentDeployed":
            agent = p["agent_id"]
            if agent in agents or (agent in gone and "manager.stop_not_final" not in skip):
                continue
            if d["schema_version"] in named_versions(skip):
                connection = p["connection_id"]
            elif "manager.v1_document" in skip:
                connection = DOCUMENT["connection_id"]
            else:
                continue
            account = state["v2"].get(connection)
            if account is None and "manager.later_establishment" in skip:
                later = [r for r in drafts[i + 1 :] if r["event_type"] == "ConnectionEstablished" and r["schema_version"] == 2]
                account = next((r["payload"]["account_ref"] for r in later if r["payload"]["connection_id"] == connection), None)
            revoked_now = state["latest"].get(connection) == "revoked" and "manager.revoked" not in skip
            if account is not None and not revoked_now:
                agents[agent] = account
        elif d["event_type"] == "AgentStopped" and "manager.stop_ignored" not in skip:
            known = agents.pop(p["agent_id"], None)
            if known is not None or "manager.stop_of_unread_ignored" not in skip:
                gone.add(p["agent_id"])
        elif d["event_type"] == "ConnectionRevoked" and "manager.revocation_stops" in skip:
            account = state["v2"].get(p["connection_id"])
            agents = {agent: held for agent, held in agents.items() if held != account}
        fold(state, d, skip)
    accounts = list(agents.values()) if "manager.executor_per_agent" in skip else sorted(set(agents.values()))
    return {"runtimes": sorted(agents), "executors": sorted(accounts)}


def stream_cases() -> list[dict]:
    """Control streams, each record its own `append`. `expect` is rule 201's verdict and `processes`
    what the deployment manager runs after the stream, both typed from the rules."""

    def ids(n: int, count: int) -> list[str]:
        return [f"01J9DA{n:02d}{i:02d}".ljust(26, "0") for i in range(count)]

    def case(n, name, clause, build, expect, runs):
        records = build(iter(ids(n, 8)))
        return {"name": name, "clause": clause, "records": records, "expect": expect, "processes": runs}

    ok = {"outcome": "Valid"}

    def at(index):
        return {"outcome": "Invalid", "index": index, "rule": 201, "path": "payload.connection_id"}

    one = {"runtimes": [AGENT], "executors": [ACCOUNT]}
    return [
        case(1, "established_then_deployed", "rule 201; §9.21: the account is the establishment's `account_ref`",
             lambda i: [established(next(i)), deployed(next(i))], ok, one),
        case(2, "two_agents_one_account", "§9.21: one executor per account",
             lambda i: [established(next(i)), deployed(next(i)), deployed(next(i), OTHER_AGENT)], ok,
             {"runtimes": [AGENT, OTHER_AGENT], "executors": [ACCOUNT]}),
        case(3, "two_connections", "§9.21: each deployment its own connection's account",
             lambda i: [established(next(i)), established(next(i), OTHER_CONNECTION, OTHER_ACCOUNT),
                        deployed(next(i)), deployed(next(i), OTHER_AGENT, OTHER_CONNECTION)], ok,
             {"runtimes": [AGENT, OTHER_AGENT], "executors": [ACCOUNT, OTHER_ACCOUNT]}),
        case(4, "reconnected", "rule 201: the latest record of the connection is an establishment",
             lambda i: [established(next(i)), revoked(next(i)), established(next(i)), deployed(next(i))], ok, one),
        case(5, "stopped", "§9.21: an `AgentStopped` stops it",
             lambda i: [established(next(i)), deployed(next(i)), stopped(next(i))], ok, NOTHING),
        case(6, "stopped_is_final", "§9.21: a later deployment of a stopped agent starts nothing",
             lambda i: [established(next(i)), deployed(next(i)), stopped(next(i)), deployed(next(i))], ok, NOTHING),
        case(7, "revoked_after_the_deployment", "rule 201 judges the deployment only; §9.21 stops nothing for a revocation",
             lambda i: [established(next(i)), deployed(next(i)), revoked(next(i))], ok, one),
        case(8, "version_1_deployment", "rule 201 does not judge version 1; §9.21: it names no account, so nothing starts",
             lambda i: [established(next(i)), deployed(next(i), connection=None)], ok, NOTHING),
        case(9, "version_1_with_no_connection", "rule 201 does not judge version 1",
             lambda i: [deployed(next(i), connection=None)], ok, NOTHING),
        case(10, "version_1_then_version_2", "§9.21: an unread deployment holds nothing back; the next is read",
             lambda i: [established(next(i)), deployed(next(i), connection=None), deployed(next(i))], ok, one),
        case(11, "never_established", "rule 201: an establishment earlier on the stream",
             lambda i: [deployed(next(i))], at(0), NOTHING),
        case(12, "established_after", "rule 201: earlier; §9.21: a later establishment revives nothing",
             lambda i: [deployed(next(i)), established(next(i))], at(0), NOTHING),
        case(13, "another_connection_established", "rule 201: the deployment's own connection",
             lambda i: [established(next(i), OTHER_CONNECTION, OTHER_ACCOUNT), deployed(next(i))], at(1), NOTHING),
        case(14, "revoked_before", "rule 201: not since revoked",
             lambda i: [established(next(i)), revoked(next(i)), deployed(next(i))], at(2), NOTHING),
        case(15, "version_1_establishment", "rule 201: established at version 2, with an `account_ref`",
             lambda i: [established(next(i), account=None), deployed(next(i))], at(1), NOTHING),
        case(16, "version_1_stopped_then_version_2", "§9.21: a stop is final, the deployment it stops read or not",
             lambda i: [established(next(i)), deployed(next(i), connection=None), stopped(next(i)), deployed(next(i))], ok, NOTHING),
        case(17, "account_stream_copy_only", "rule 201: an establishment on the control stream, never an account stream's copy",
             lambda i: [checked(next(i)), copied(next(i), "01J9DA00E000000000000000E2"), deployed(next(i))], at(2), NOTHING),
    ]


def stop_cases() -> list[dict]:
    """Pairs of an agent stream and a control stream for rule 246, each record its own `append`.
    `expect` is the rule's verdict, typed from the rule: whether the owner's workspace service
    claimed the agent's stream and retired the agent itself."""

    def ids(n: int, count: int) -> list[str]:
        return [f"01J9DB{n:02d}{i:02d}".ljust(26, "0") for i in range(count)]

    def case(n, name, clause, agent, control, expect):
        i = iter(ids(n, 12))
        return {"name": name, "clause": clause, "agent": agent(i), "control": control(i), "expect": expect}

    ok = {"outcome": "Valid"}

    def at(stream, index, path):
        return {"outcome": "Invalid", "stream": stream, "index": index, "rule": 246, "path": path}

    def with_payload(draft: dict, **members) -> dict:
        draft["payload"].update(members)
        return draft

    def claim(i):
        return [opened(next(i), USER)]

    def ran(i):
        return [opened(next(i), RUNTIME), observed(next(i))]

    def retiring(i, *before):
        return [established(next(i)), deployed(next(i)), *(b(next(i)) for b in before), stopped(next(i))]

    return [
        case(1, "claimed_and_retired", "rule 246: the service's claim opens the agent's stream",
             claim, lambda i: retiring(i), ok),
        case(2, "version_1_claimed", "rule 246: an unread deployment's stream, claimed",
             claim, lambda i: [deployed(next(i), connection=None), stopped(next(i))], ok),
        case(3, "refused_by_rule_201", "rule 246: a deployment rule 201 refuses, its stream claimed",
             claim, lambda i: [deployed(next(i)), stopped(next(i))], ok),
        case(4, "claimed_by_another_service", "rule 246: a claim is any actor but an `agent`",
             lambda i: [opened(next(i), SERVICES)], lambda i: retiring(i), ok),
        case(5, "unclaimed", "rule 246: an empty agent stream is no claim",
             lambda i: [], lambda i: retiring(i), at("control", 2, "payload.agent_id")),
        case(6, "runtime_opened", "rule 246: a stream the runtime opened is no claim",
             lambda i: [opened(next(i), RUNTIME)], lambda i: retiring(i), at("control", 2, "payload.agent_id")),
        case(7, "runtime_ran", "rule 246: a runtime that ran, started by the manager",
             ran, lambda i: retiring(i, started), at("control", 3, "payload.agent_id")),
        case(8, "a_restart_resumes", "rule 246: told by kind, so a later build's restart resumes its stream",
             lambda i: [opened(next(i), EARLIER_RUNTIME), observed(next(i))],
             lambda i: [established(next(i)), deployed(next(i)), started(next(i)), started(next(i), generation=2)], ok),
        case(9, "written_after_the_claim", "rule 246: a claimed stream holds nothing after its claim",
             lambda i: [opened(next(i), USER), observed(next(i))], lambda i: [], at("agent", 1, "actor.kind")),
        case(10, "another_agents_stop", "rule 246: the claim is of the stopped agent's own stream",
             claim, lambda i: [established(next(i)), deployed(next(i)), deployed(next(i), OTHER_AGENT),
                               stopped(next(i), OTHER_AGENT)], at("control", 3, "payload.agent_id")),
        case(11, "the_runtime_retires_it", "rule 246: an `AgentStopped` the agent's runtime writes is not judged",
             ran, lambda i: [established(next(i)), deployed(next(i)), started(next(i)), retired(next(i), "12.5")], ok),
        case(12, "a_loss", "rule 246: `loss_added` is 0",
             claim, lambda i: [established(next(i)), deployed(next(i)), with_payload(stopped(next(i)), loss_added="3")],
             at("control", 2, "payload.loss_added")),
        case(13, "not_the_owners_stop", "rule 246: the reason is `owner_stop`",
             claim, lambda i: [established(next(i)), deployed(next(i)), with_payload(stopped(next(i)), reason="goal_complete")],
             at("control", 2, "payload.reason")),
    ]


def range_cases() -> list[dict]:
    """Ranges of the stream cases, by name: `start` is the first record in the range."""

    def case(name, clause, stream, start, anchored, expect):
        return {"name": name, "clause": clause, "stream": stream, "start": start, "anchored": anchored, "expect": expect}

    ok = {"outcome": "Valid"}

    def at(index):
        return {"outcome": "Invalid", "index": index, "rule": 201, "path": "payload.connection_id"}

    def closed(index):
        return {"outcome": "Invalid", "index": index, "cause": "unanchored"}

    return [
        case("anchored_after_the_establishment", "§11: the anchor holds the establishment", "established_then_deployed", 1, True, ok),
        case("unanchored_after_the_establishment", "§11: with no anchor, a version-2 deployment fails closed", "established_then_deployed", 1, False, closed(1)),
        case("anchored_after_the_revocation", "§11: the anchor holds the latest revocation", "revoked_before", 2, True, at(2)),
        case("anchored_after_the_reconnect", "§11: the anchor holds the latest establishment", "reconnected", 3, True, ok),
        case("anchored_mid_reconnect", "§11: the range judges the reconnect itself", "reconnected", 2, True, ok),
        case("unanchored_at_an_establishment", "§11: rule 66's records fail closed first", "reconnected", 2, False, closed(2)),
        case("unanchored_version_1_only", "§11: a version-1 deployment is not judged, so never fails closed", "version_1_deployment", 1, False, ok),
        case("unanchored_stop_only", "§11: an `AgentStopped` is not judged", "stopped", 2, False, ok),
        case("from_the_start", "§11: a full chain is anchored on nothing before it", "never_established", 0, False, at(0)),
        case("anchored_on_another_connection", "§11: the anchor's connection is not the deployment's", "another_connection_established", 1, True, at(1)),
        case("anchored_after_the_deployment", "§11: a revocation after the deployment is not judged", "revoked_after_the_deployment", 2, True, ok),
    ]


# --------------------------------------------------------------------------- independent checks

# Checks a vector cannot break on its own, since they compare two readings of the same records; each
# is shown to catch a seeded bug in the reading instead (`INVARIANT_MUTANTS`).
INVARIANT_CHECKS = ("ranges.agree",)
INVARIANT_MUTANTS = {"ranges.agree": "range.201.anchor_ignored"}
ORACLE_CHECKS = (
    "artifacts.rehash",
    "drafts.valid",
    "drafts.document",
    "invalid_drafts",
    "valid_drafts",
    "mappings.drafts",
    "mappings.rule",
    "streams.records",
    "streams.rule",
    "streams.processes",
    "ranges.rule",
    "ranges.agree",
    "stops.records",
    "stops.rule",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def expected_failure(expect: dict) -> dict | None:
    return None if expect["outcome"] == "Valid" else {k: v for k, v in expect.items() if k != "outcome"}


def check_section(section: dict, skip: frozenset[str] = frozenset()) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    documents = {}
    for a in section["artifacts"]:
        if "sha256:" + sha256_hex(canon(a["object"]).encode()) != a["ref"]:
            problems.append(found("artifacts.rehash", f"artifact {a['name']} does not re-hash to its ref"))
        documents[a["name"]] = a["object"]
    drafts = section["drafts"]
    for name, draft in drafts.items():
        got = violations(draft, skip)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
    document = documents.get("mandate_document", {})
    v2, v1 = drafts["deployed_v2"]["payload"], drafts["deployed_v1"]["payload"]
    if v2.get("connection_id") != document.get("connection_id") or "connection_id" in v1:
        problems.append(found("drafts.document", "the version-2 base does not name its document's connection"))
    for case in section["invalid_drafts"]:
        got = violations(draft_for(section, case), skip)
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
    for case in section["valid_drafts"]:
        got = violations(draft_for(section, case), skip)
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
    by_ref = {a["ref"]: a["object"] for a in section["artifacts"]}
    refs = {a["name"]: a["ref"] for a in section["artifacts"]}
    for case in section["mappings"]:
        draft = draft_for(section, case)
        got = violations(draft, skip)
        if got:
            problems.append(found("mappings.drafts", f"{case['name']}: the draft is refused at append: {got}"))
        stored_now = {refs[n]: by_ref[refs[n]] for n in case["stored"] if n in refs}
        got = map_deployed(draft, stored_now, skip)
        if got != case["expect"]:
            problems.append(found("mappings.rule", f"{case['name']}: expected {case['expect']}, got {got}"))
    streams = {}
    for case in section["streams"]:
        records = case["records"]
        streams[case["name"]] = records
        for i, record in enumerate(records):
            got = violations(record, skip)
            if got:
                problems.append(found("streams.records", f"{case['name']} record {i}: {got}"))
        got = stream_failure(records, skip)
        if got != expected_failure(case["expect"]):
            problems.append(found("streams.rule", f"{case['name']}: expected {case['expect']}, got {got}"))
        got = processes(records, skip)
        if got != case["processes"]:
            problems.append(found("streams.processes", f"{case['name']}: expected {case['processes']}, got {got}"))
    for case in section["ranges"]:
        records = streams.get(case["stream"], [])
        got = range_failure(records, case["start"], case["anchored"], skip)
        if got != expected_failure(case["expect"]):
            problems.append(found("ranges.rule", f"{case['name']}: expected {case['expect']}, got {got}"))
    for case in section["stops"]:
        for i, record in enumerate(case["agent"]):
            got = envelope_violations(record, skip)
            if got or record["stream_id"] != AGENT_STREAM:
                problems.append(found("stops.records", f"{case['name']} agent record {i}: {got}"))
        for i, record in enumerate(case["control"]):
            got = violations(record, skip)
            if got:
                problems.append(found("stops.records", f"{case['name']} control record {i}: {got}"))
        got = stop_failure(case["agent"], case["control"], skip)
        if got != expected_failure(case["expect"]):
            problems.append(found("stops.rule", f"{case['name']}: expected {case['expect']}, got {got}"))
    for name, records in streams.items():
        full = stream_failure(records, skip)
        for start in range(len(records) + 1):
            if full is not None and full["index"] < start:
                break
            got = range_failure(records, start, True, skip)
            if got != full:
                problems.append(found("ranges.agree", f"{name} from {start}: anchored {got}, full chain {full}"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "record.extra",
    "record.missing",
    "config_refs.required",
    "rule.22",
    "rule.22.prefix",
    "rule.83",
    "versions.agent_deployed_v2",
    "versions.agent_deployed_v1_schema",
    "map.200",
    "map.200.v1",
    "map.v2_document_skipped",
    *(f"stream.201{n}" for n in ("", ".v1_judged", ".connection", ".later", ".later_revocation", ".version", ".revoked", ".account_copy")),
    *(f"range.201.{n}" for n in ("anchor_ignored", "unanchored_open", "unanchored_v1_only")),
    *(f"stream.246{n}" for n in ("", ".instance", ".any_opener", ".after", ".runtime_judged", ".any_agent", ".reason", ".loss")),
    *(
        f"manager.{n}"
        for n in (
            "stop_not_final",
            "stop_of_unread_ignored",
            "v1_document",
            "later_establishment",
            "revoked",
            "revocation_stops",
            "stop_ignored",
            "executor_per_agent",
        )
    ),
)


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    """Seeded bugs in the vectors, each registered against the one check that must reject it."""

    def mutated(fn) -> dict:
        out = copy.deepcopy(section)
        fn(out)
        return out

    def case(s, kind, name):
        return next(c for c in s[kind] if c["name"] == name)

    def edit_canonical(s):
        s["artifacts"][0]["object"]["name"] = "edited"

    return [
        ("a stored document edited after hashing", "artifacts.rehash", mutated(edit_canonical)),
        ("a base draft names no mandate version ref", "drafts.valid", mutated(lambda s: s["drafts"]["deployed_v2"].update(config_refs={}))),
        ("the version-2 base names another connection", "drafts.document", mutated(lambda s: s["drafts"]["deployed_v2"]["payload"].update(connection_id=OTHER_CONNECTION))),
        ("an invalid draft's path differs", "invalid_drafts", mutated(lambda s: case(s, "invalid_drafts", "deployed_v2.rule_22")["expect"].update(path="payload.connection_id"))),
        ("a valid draft drops its connection", "valid_drafts", mutated(lambda s: case(s, "valid_drafts", "deployed_v2.live")["changes"].append(change("payload.connection_id", None)))),
        ("a mapping draft is refused at append", "mappings.drafts", mutated(lambda s: case(s, "mappings", "version_2")["changes"].append(change("schema_version", 4)))),
        ("a mismatch is listed as mapped", "mappings.rule", mutated(lambda s: case(s, "mappings", "another_connection").update(expect=case(s, "mappings", "version_2")["expect"]))),
        ("a stream record is written by a client", "streams.records", mutated(lambda s: case(s, "streams", "stopped")["records"][2].update(actor=dict(clients.CLIENT)))),
        ("a stream case expects the wrong record", "streams.rule", mutated(lambda s: case(s, "streams", "revoked_before")["expect"].update(index=1))),
        ("a version-1 deployment is listed as running", "streams.processes", mutated(lambda s: case(s, "streams", "version_1_deployment").update(processes={"runtimes": [AGENT], "executors": [ACCOUNT]}))),
        ("a stop's record is written by a client", "stops.records", mutated(lambda s: case(s, "stops", "claimed_and_retired")["control"][2].update(actor=dict(clients.CLIENT)))),
        ("a claim is written to another agent's stream", "stops.records", mutated(lambda s: case(s, "stops", "claimed_and_retired")["agent"][0].update(stream_id=f"agent:{WORKSPACE}:{OTHER_AGENT}"))),
        ("a stop of a stream the runtime opened is listed as valid", "stops.rule", mutated(lambda s: case(s, "stops", "runtime_opened").update(expect={"outcome": "Valid"}))),
        ("an unanchored range is listed as passing", "ranges.rule", mutated(lambda s: case(s, "ranges", "unanchored_after_the_establishment").update(expect={"outcome": "Valid"}))),
    ]


def run_mutants(section: dict) -> list[str]:
    """Every seeded bug caught; a vector mutant only by the check it is registered against."""
    escaped = []
    for mutant in VALIDATOR_MUTANTS:
        if not check_section(section, frozenset([mutant])):
            escaped.append(f"agent-deployed mutant {mutant}")
    for check, mutant in INVARIANT_MUTANTS.items():
        if check not in {check_of(problem) for problem in check_section(section, frozenset([mutant]))}:
            escaped.append(f"agent-deployed invariant {check} does not catch {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if check in INVARIANT_CHECKS:
            continue
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"agent-deployed check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"agent-deployed vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    return {
        "spec": SPEC,
        "stream_id": STREAM,
        "artifacts": [{"name": name, "ref": artifact_ref(obj), "object": copy.deepcopy(obj)} for name, obj in ARTIFACTS.items()],
        "drafts": copy.deepcopy(DRAFTS),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
        "mappings": mapping_cases(),
        "streams": stream_cases(),
        "ranges": range_cases(),
        "stops": stop_cases(),
    }


__all__ = ["build_section", "check_section", "run_mutants", "vector_mutants"]
