"""Journal spec v0.117's reference vectors for an owner's request (DEC-2858): the `DecisionMade` the
order builder writes for an `OwnerRequestSubmitted` (§9.9) was produced by no §8.3 evaluation, so
rule 8 has it carry no evaluation numbers, outputs or weights while `clips_applied` still lists the
bounds that cut the owner's quantity; and the `ApprovalRequested` that asks for it carries no score,
with rule 319 on the request and on the batch that holds it and its decision.

The decisions are `generate.py`'s: its caller hands this module whole `DecisionMade` drafts and its
own validator for them, so a decision is judged as `append` judges it. The requests are
`requests.py`'s version 1 and 2 bases, and the score-free ones this module derives from them; the
request rules live in `control.py`, beside §9.6's others. A batch is judged as `approval_deferrals`
judges one (each draft on its own, then rule 292), then by rule 319's batch clause. Its own oracle
reads rule 8's owner branch and rule 319's batch clause without the validators, recomputes every
content hash with `json` and `hashlib`, and every seeded bug is shown caught.
"""

from __future__ import annotations

import copy
from collections.abc import Callable

from approval_deferrals import batch_violations as deferral_batch_violations
from common import Violation, apply_change, change, digest_strings
from control import check_of, draft_for, reported, violations
from requests import V1, V2, base_drafts, oracle_hash

SPEC = "docs/specs/journal.md v0.117 §9.1 rules 7 and 8 and §9.6 rule 319 (DEC-2858)"
SCORED = V1
UNSCORED = "unscored_v1"
UNSCORED_V2 = "unscored_v2"
SCORE = "payload.combined_score"
NESTED = "payload.content.evidence.combined_score"
OUTPUTS = "payload.content.evidence.outputs"
OWNER_REQUESTED = ("owner", "client")
NUMBERS = ("exit_conviction", "buy_conviction", "combined_score")
LISTS = ("outputs_used", "model_weights", "clips_applied")

Validator = Callable[[dict, frozenset[str]], list[Violation]]


def restate(draft: dict) -> dict:
    """The draft's content hash and artifact references, recomputed after its content changed."""
    draft["payload"]["content_hash"] = oracle_hash(draft["payload"]["content"])
    draft["artifact_refs"] = sorted(digest_strings(draft["payload"]))
    return draft


def unscored(base: str) -> dict:
    """`requests.py`'s base with no score at either level and no output cited."""
    out = copy.deepcopy(base_drafts()[base])
    out["payload"]["combined_score"] = None
    out["payload"]["content"]["evidence"]["combined_score"] = None
    out["payload"]["content"]["evidence"]["outputs"] = []
    return restate(out)


def bases() -> dict[str, dict]:
    return {SCORED: copy.deepcopy(base_drafts()[V1]), UNSCORED: unscored(V1), UNSCORED_V2: unscored(V2)}


def again(base: str, changes: list[dict]) -> list[dict]:
    """`changes` on this section's `base`, then the hash and references the changed draft states."""
    after = copy.deepcopy(bases()[base])
    for item in changes:
        apply_change(after, item)
    restate(after)
    return [
        *changes,
        change("payload.content_hash", after["payload"]["content_hash"]),
        change("artifact_refs", after["artifact_refs"]),
    ]


def invalid(name, clause, base, changes, reason, path):
    return {
        "name": name,
        "clause": clause,
        "base_draft": base,
        "changes": changes,
        "expect": {"outcome": "Invalid", "reason": reason, "path": path},
    }


def valid(name, clause, base, changes):
    return {"name": name, "clause": clause, "base_draft": base, "changes": changes, "expect": {"outcome": "Valid"}}


CITED = {"event_id": "01J8Z3N8A000000000000000Q9", "artifact": None, "label": "Output of software you selected"}
LABELLED = {"label": "combined model score, not a probability of profit", "value": "0.5"}


def invalid_drafts() -> list[dict]:
    """Each request breaks exactly one rule: a score at one level only, a null score citing an
    output, and a score object whose value is null."""
    return [
        invalid(
            "score_without_its_object",
            "§9.6 mirror: a top-level score and its object are null together",
            UNSCORED,
            again(UNSCORED, [change(SCORE, "0.5")]),
            "schema",
            SCORE,
        ),
        invalid(
            "object_without_its_score",
            "§9.6 mirror: a score object beside a null top-level score",
            UNSCORED,
            again(UNSCORED, [change(NESTED, dict(LABELLED))]),
            "schema",
            SCORE,
        ),
        invalid(
            "scored_request_drops_its_top_level_score",
            "§9.6 mirror: a null top-level score beside the object",
            SCORED,
            again(SCORED, [change(SCORE, None), change(OUTPUTS, [])]),
            "schema",
            SCORE,
        ),
        invalid(
            "unscored_request_cites_an_output",
            "rule 319: no score, no output",
            UNSCORED,
            again(UNSCORED, [change(OUTPUTS, [dict(CITED)])]),
            "schema",
            OUTPUTS,
        ),
        invalid(
            "unscored_v2_request_cites_an_output",
            "rule 319 at version 2",
            UNSCORED_V2,
            again(UNSCORED_V2, [change(OUTPUTS, [dict(CITED)])]),
            "schema",
            OUTPUTS,
        ),
        invalid(
            "score_object_with_a_null_value",
            "§9.6: a present score object holds a decimal",
            UNSCORED,
            again(UNSCORED, [change(NESTED, {"label": LABELLED["label"], "value": None})]),
            "schema",
            f"{NESTED}.value",
        ),
    ]


def valid_drafts() -> list[dict]:
    """Requests a rule might be misread to refuse; each must be accepted."""
    return [
        valid("unscored_v1", "rule 319: a null score, no output, no label to check", UNSCORED, []),
        valid("unscored_v2", "rule 319 at version 2", UNSCORED_V2, []),
        valid("scored_request_unchanged", "rule 319: a score keeps its outputs", SCORED, []),
    ]


def decision_case(name, clause, decision, changes, expect=None):
    return {"name": name, "clause": clause, "decision": decision, "changes": changes, "expect": expect or {"outcome": "Valid"}}


def refused_at(path: str) -> dict:
    return {"outcome": "Invalid", "reason": "schema", "path": path}


EVALUATED = [
    change("payload.exit_conviction", "0.85554"),
    change("payload.buy_conviction", "0.75554"),
    change("payload.combined_score", "0.882"),
]


def decision_cases(decisions: dict[str, dict]) -> list[dict]:
    """Rule 8's owner branch: an owner's or a client's request carries no evaluation numbers,
    outputs, or weights whatever its purpose, and lists every clip; an agent's opening still needs
    its numbers. Rule 7's owner clauses (item 10): an owner's opening is never asked, and only an
    owner's `auto` opening carries the `owner_request` label."""
    agent = decisions["agent_asked"]["payload"]
    return [
        decision_case("owner_open_unevaluated", "rule 8: an owner request", "owner_auto", []),
        decision_case(
            "owner_open_clipped_twice",
            "rule 8: an owner request lists every bound that cut it",
            "owner_auto",
            [change("payload.clips_applied", ["max_order_usd", "position_cap"])],
        ),
        decision_case(
            "owner_open_unclipped",
            "rule 8: an owner request the bounds left whole",
            "owner_auto",
            [change("payload.clips_applied", [])],
        ),
        decision_case(
            "owner_increase_unevaluated",
            "rule 8: whatever its purpose",
            "owner_auto",
            [change("payload.purpose", "increase")],
        ),
        decision_case(
            "owner_open_auto",
            "rule 8: an owner request the rules let run",
            "owner_auto",
            [change("payload.autonomy", "auto"), change("payload.decided_by", "default")],
        ),
        decision_case(
            "owner_open_denied_at_its_cap",
            "rule 8, DEC-2858 item 7: a clip that leaves nothing is the gate's denial, no clip listed",
            "owner_auto",
            [
                change("payload.clips_applied", []),
                change("payload.dry_run", "deny"),
                change("payload.reason_code", "max_order_size"),
                change("payload.autonomy", None),
                change("payload.decided_by", None),
            ],
        ),
        decision_case(
            "owner_open_denied_by_a_rule_reading_the_score",
            "rule 8, DEC-2858 item 8: a rule that reads the missing score denies, labelled with it",
            "owner_auto",
            [change("payload.autonomy", "deny"), change("payload.decided_by", "rule:low_score")],
        ),
        decision_case("client_open_unevaluated", "rule 8: a client request", "client_asked", []),
        decision_case(
            "owner_open_asked",
            "rule 7, DEC-2858 item 10: an owner's own opening is never asked",
            "owner_auto",
            [change("payload.autonomy", "ask"), change("payload.decided_by", "default")],
            refused_at("payload.autonomy"),
        ),
        decision_case(
            "owner_increase_asked",
            "rule 7, DEC-2858 item 10: nor its increase",
            "owner_auto",
            [
                change("payload.purpose", "increase"),
                change("payload.autonomy", "ask"),
                change("payload.decided_by", "rule:large_orders"),
            ],
            refused_at("payload.autonomy"),
        ),
        decision_case(
            "owner_request_label_on_a_client_request",
            "rule 7: the owner's label is the owner's alone",
            "client_asked",
            [change("payload.decided_by", "owner_request")],
            refused_at("payload.decided_by"),
        ),
        decision_case(
            "owner_request_label_on_the_agents_opening",
            "rule 7: the agent's own opening never claims the owner's Send",
            "agent_asked",
            [change("payload.autonomy", "auto"), change("payload.decided_by", "owner_request")],
            refused_at("payload.decided_by"),
        ),
        decision_case(
            "owner_request_label_on_an_owner_denial",
            "rule 7: the label names only an owner's request that ran",
            "owner_auto",
            [change("payload.autonomy", "deny")],
            refused_at("payload.decided_by"),
        ),
        decision_case(
            "owner_open_with_evaluation_numbers",
            "rule 8: the first offending number",
            "owner_auto",
            list(EVALUATED),
            refused_at("payload.exit_conviction"),
        ),
        decision_case(
            "owner_open_with_a_score",
            "rule 8: no score on an owner request",
            "owner_auto",
            [change("payload.combined_score", "0.882")],
            refused_at("payload.combined_score"),
        ),
        decision_case(
            "owner_open_citing_outputs",
            "rule 8: no output on an owner request",
            "owner_auto",
            [change("payload.outputs_used", list(agent["outputs_used"]))],
            refused_at("payload.outputs_used"),
        ),
        decision_case(
            "owner_open_with_weights",
            "rule 8: no weight on an owner request",
            "owner_auto",
            [change("payload.model_weights", copy.deepcopy(agent["model_weights"]))],
            refused_at("payload.model_weights"),
        ),
        decision_case(
            "client_open_with_evaluation_numbers",
            "rule 8: a client request is not evaluated either",
            "client_asked",
            list(EVALUATED),
            refused_at("payload.exit_conviction"),
        ),
        decision_case(
            "agent_open_without_numbers",
            "rule 8: the agent's opening was evaluated",
            "agent_asked",
            [
                change("payload.exit_conviction", None),
                change("payload.buy_conviction", None),
                change("payload.combined_score", None),
                change("payload.outputs_used", []),
                change("payload.model_weights", []),
            ],
            refused_at("payload.exit_conviction"),
        ),
    ]


def decision_for(section: dict, case: dict) -> dict:
    out = copy.deepcopy(section["decisions"][case["decision"]])
    for item in case["changes"]:
        apply_change(out, item)
    return out


def batch(name, clause, decision, request_base, changes, expect=None, request_first=False):
    """A batch of the `decision` this section holds, then a request on `request_base` that names it,
    with `changes`, or the request first when `request_first`. `expect` is `None` for a batch
    `append` accepts."""
    return {
        "name": name,
        "clause": clause,
        "decision": decision,
        "request": {"base_draft": request_base, "changes": changes},
        "order": "request_first" if request_first else "decision_first",
        "expect": expect or {"outcome": "Valid"},
    }


def refused(index: int) -> dict:
    return {"outcome": "Invalid", "index": index, "reason": "schema", "path": SCORE}


def batches() -> list[dict]:
    """Rule 319 on the batch: the request has no score exactly when its decision is an owner's or a
    client's request. Only a client's is ever asked (item 10), so the cases pair the client's."""
    return [
        batch("client_decision_and_its_unscored_request", "rule 319", "client_asked", UNSCORED, []),
        batch("client_decision_and_its_unscored_v2_request", "rule 319", "client_asked", UNSCORED_V2, []),
        batch("agent_decision_and_its_scored_request", "rule 319: the agent's request keeps its score", "agent_asked", SCORED, []),
        batch("client_decision_and_a_scored_request", "rule 319: no score is invented", "client_asked", SCORED, [], refused(1)),
        batch("agent_decision_and_an_unscored_request", "rule 319: the agent's score is never dropped", "agent_asked", UNSCORED, [], refused(1)),
        batch("agent_decision_and_an_unscored_v2_request", "rule 319 at version 2", "agent_asked", UNSCORED_V2, [], refused(1)),
        batch(
            "scored_request_written_before_the_client_decision",
            "rule 319 judges the batch, whatever the order",
            "client_asked",
            SCORED,
            [],
            refused(0),
            request_first=True,
        ),
        batch(
            "unscored_request_naming_another_decision",
            "rule 319 binds only the decision the request names",
            "agent_asked",
            UNSCORED,
            [change("causation_id", "01J8Z3N9A000000000000000Q7")],
        ),
    ]


def request_of(section: dict, case: dict, decision: dict) -> dict:
    """The case's request, naming `decision` unless its changes name another."""
    out = copy.deepcopy(section["drafts"][case["request"]["base_draft"]])
    out["causation_id"] = decision["event_id"]
    for item in case["request"]["changes"]:
        apply_change(out, item)
    return out


def batch_drafts(section: dict, case: dict) -> list[dict]:
    decision = section["decisions"][case["decision"]]
    pair = [decision, request_of(section, case, decision)]
    return pair[::-1] if case["order"] == "request_first" else pair


def batch_violations(
    drafts: list[dict], decide: Validator, skip: frozenset[str] = frozenset()
) -> list[tuple[int, Violation]]:
    """An `append` batch (§5.1), refused whole on any violation: each draft's own and rule 292, as
    `approval_deferrals` judges them, then rule 319 on an `ApprovalRequested` whose `DecisionMade` is
    in the batch."""
    out = deferral_batch_violations(drafts, decide, skip)
    if out or "rule.319.batch" in skip:
        return out
    decisions = {d["event_id"]: d for d in drafts if d["event_type"] == "DecisionMade"}
    for i, draft in enumerate(drafts):
        cause = decisions.get(draft["causation_id"]) if draft["event_type"] == "ApprovalRequested" else None
        if cause is None:
            continue
        owner = cause["payload"]["requested_by"] in OWNER_REQUESTED
        if (draft["payload"]["combined_score"] is None) != owner:
            out.append((i, Violation("rule.319.batch", "schema", SCORE)))
    return out


# --------------------------------------------------------------------------- independent oracle

ORACLE_CHECKS = (
    "drafts.valid",
    "drafts.hash",
    "decisions.valid",
    "decisions.requested",
    "decision_cases",
    "invalid_drafts",
    "valid_drafts",
    "batches",
)


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def outcome(got: list[tuple[int, Violation]]) -> dict:
    if not got:
        return {"outcome": "Valid"}
    index, first = got[0]
    return {"outcome": "Invalid", "index": index, "reason": first.reason, "path": first.path}


def expected_decision(decision: dict) -> dict:
    """The oracle's own reading of rule 7's owner clauses and then rule 8, for a decision every other
    rule accepts: an owner's opening asked, or the owner's label anywhere else, first; then who
    asked decides whether an evaluation produced it, and the six members are walked in the table's
    order."""
    p = decision["payload"]
    opening = p["purpose"] in ("open", "increase")
    owners_own = p["requested_by"] == "owner" and opening
    if owners_own and p["autonomy"] == "ask":
        return refused_at("payload.autonomy")
    if p["decided_by"] == "owner_request" and not (owners_own and p["autonomy"] == "auto"):
        return refused_at("payload.decided_by")
    asked_by_owner = p["requested_by"] != "agent"
    produced = not asked_by_owner and (p["purpose"] in ("open", "increase") or p["exit_origin"] == "signal")
    for member in NUMBERS:
        if (p[member] is None) == produced:
            return refused_at(f"payload.{member}")
    if not produced:
        for member in LISTS:
            if member == "clips_applied" and asked_by_owner:
                continue
            if len(p[member]) > 0:
                return refused_at(f"payload.{member}")
    return {"outcome": "Valid"}


def decision_outcome(got: list[Violation]) -> dict:
    if not got:
        return {"outcome": "Valid"}
    if len(got) != 1:
        return {"outcome": "Invalid", "reason": "more than one rule", "path": ", ".join(v.path for v in got)}
    return {"outcome": "Invalid", "reason": got[0].reason, "path": got[0].path}


def expected_batch(section: dict, case: dict) -> dict:
    """The oracle's own reading of rule 319's batch clause, by who asked and whether a score is
    stated at the top level."""
    decision = section["decisions"][case["decision"]]
    request = request_of(section, case, decision)
    if request["causation_id"] != decision["event_id"]:
        return {"outcome": "Valid"}
    by_owner = decision["payload"]["requested_by"] in ("owner", "client")
    stated = "combined_score" in request["payload"] and request["payload"]["combined_score"] is not None
    if by_owner != stated:
        return {"outcome": "Valid"}
    return refused(0 if case["order"] == "request_first" else 1)


def check_section(section: dict, decide: Validator) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    for name, base in section["drafts"].items():
        if violations(base):
            problems.append(found("drafts.valid", f"base draft {name}: {violations(base)}"))
        if base["payload"]["content_hash"] != oracle_hash(base["payload"]["content"]):
            problems.append(found("drafts.hash", f"base draft {name} states another hash than its content's"))
    for name in (UNSCORED, UNSCORED_V2):
        evidence = section["drafts"][name]["payload"]["content"]["evidence"]
        if section["drafts"][name]["payload"]["combined_score"] is not None or evidence != {"combined_score": None, "outputs": []}:
            problems.append(found("drafts.valid", f"base draft {name} states a score or an output"))
    for name, decision in section["decisions"].items():
        got = decide(decision, frozenset())
        if got:
            problems.append(found("decisions.valid", f"decision {name}: {got}"))
    who = {name: d["payload"]["requested_by"] for name, d in section["decisions"].items()}
    if who != {"agent_asked": "agent", "owner_auto": "owner", "client_asked": "client"}:
        problems.append(found("decisions.requested", f"the decisions are asked by {who}"))
    autonomy = {name: (d["payload"]["autonomy"], d["payload"]["decided_by"]) for name, d in section["decisions"].items()}
    if autonomy["owner_auto"] != ("auto", "owner_request") or any(
        autonomy[name][0] != "ask" for name in ("agent_asked", "client_asked")
    ):
        problems.append(found("decisions.requested", f"the owner's runs unasked and the others ask, not {autonomy}"))
    if section["decisions"]["owner_auto"]["payload"]["clips_applied"] == []:
        problems.append(found("decisions.requested", "the owner's decision lists no clip"))
    for case in section["decision_cases"]:
        d = decision_for(section, case)
        got = decision_outcome(decide(d, frozenset()))
        if got != case["expect"] or case["expect"] != expected_decision(d):
            problems.append(
                found("decision_cases", f"{case['name']}: stated {case['expect']}, got {got}, oracle {expected_decision(d)}")
            )
    for case in section["invalid_drafts"]:
        got = violations(draft_for(section, case))
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
    for case in section["valid_drafts"]:
        got = violations(draft_for(section, case))
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
    for case in section["batches"]:
        got = outcome(batch_violations(batch_drafts(section, case), decide))
        want = case["expect"]
        if got != want or want != expected_batch(section, case):
            problems.append(found("batches", f"{case['name']}: stated {want}, got {got}, oracle {expected_batch(section, case)}"))
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "rule.7.owner_request",
    "rule.7.owner_request_label",
    "regress.rule_8_owner_request_evaluated",
    "regress.rule_8_owner_request_without_clips",
    "rule.319.outputs",
    "rule.319.batch",
    "regress.label_on_a_null_score",
    "rule.9.6.mirror.combined_score",
)


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    """Seeded bugs in the vectors, each registered against the one check that must reject it."""

    def mutated(fn) -> dict:
        out = copy.deepcopy(section)
        fn(out)
        return out

    def case(s, kind, name):
        return next(c for c in s[kind] if c["name"] == name)

    def unhashed(s):
        s["drafts"][UNSCORED]["payload"]["content"]["evidence"]["outputs"] = [dict(CITED)]

    def scored(s):
        s["drafts"][UNSCORED_V2]["payload"]["combined_score"] = "0.5"
        s["drafts"][UNSCORED_V2]["payload"]["content"]["evidence"]["combined_score"] = dict(LABELLED)
        restate(s["drafts"][UNSCORED_V2])

    return [
        ("the unscored base cites an output under the old hash", "drafts.hash", mutated(unhashed)),
        ("the unscored version 2 base states a score", "drafts.valid", mutated(scored)),
        (
            "the owner's decision carries its evaluation's score",
            "decisions.valid",
            mutated(lambda s: s["decisions"]["owner_auto"]["payload"].update(combined_score="0.882")),
        ),
        (
            "the client's decision is recorded as the agent's",
            "decisions.requested",
            mutated(lambda s: s["decisions"]["client_asked"]["payload"].update(requested_by="agent", client_id=None)),
        ),
        (
            "a refused decision is stated accepted",
            "decision_cases",
            mutated(lambda s: case(s, "decision_cases", "owner_open_with_a_score").update(expect={"outcome": "Valid"})),
        ),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "unscored_request_cites_an_output")["expect"].update(path=NESTED)),
        ),
        (
            "a valid draft cites an output under a restated hash",
            "valid_drafts",
            mutated(lambda s: case(s, "valid_drafts", "unscored_v1").update(changes=again(UNSCORED, [change(OUTPUTS, [dict(CITED)])]))),
        ),
        (
            "a refused batch is stated accepted",
            "batches",
            mutated(lambda s: case(s, "batches", "client_decision_and_a_scored_request").update(expect={"outcome": "Valid"})),
        ),
    ]


def run_mutants(section: dict, decide: Validator) -> list[str]:
    """Every seeded bug caught; a vector mutant only by the check it is registered against."""
    escaped = []
    singles = [(copy.deepcopy(d), None) for d in section["drafts"].values()]
    singles += [(draft_for(section, c), None) for c in section["valid_drafts"]]
    singles += [(draft_for(section, c), c["expect"]) for c in section["invalid_drafts"]]
    for mutant in VALIDATOR_MUTANTS:
        skip = frozenset([mutant])
        caught = False
        for d, want in singles:
            got = violations(d, skip)
            caught |= bool(got) if want is None else not reported(got, want)
        for c in section["decision_cases"]:
            caught |= decision_outcome(decide(decision_for(section, c), skip)) != c["expect"]
        for c in section["batches"]:
            caught |= outcome(batch_violations(batch_drafts(section, c), decide, skip)) != c["expect"]
        if not caught:
            escaped.append(f"owner request validator mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"owner request check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated, decide)}
        if check not in caught_by:
            escaped.append(f"owner request vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section(decisions: dict[str, dict]) -> dict:
    """`decisions` holds generate.py's asked openings: `agent_asked` (the chain's evaluated
    decision), `owner_asked` and `client_asked` (the same opening requested by the owner and by a
    connected client, carrying no evaluation figures and the chain's clip), whole drafts."""
    return {
        "spec": SPEC,
        "decisions": copy.deepcopy(decisions),
        "drafts": bases(),
        "decision_cases": decision_cases(decisions),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
        "batches": batches(),
    }
