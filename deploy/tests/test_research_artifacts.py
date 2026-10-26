"""The research store (DEC-2279 item 5), on what the workspace chart renders.

The research runtime's artifacts go to a store of their own, which only the model gateway writes,
and the runtime reads back. Which process may mount it, and how, is written out here, not read from
the chart, so a chart that drifts fails:
- the gateway mounts it read-write, and that is its one writable claim: the shared store stays
  read-only to it;
- the runtime mounts it read-only, and never waits for its layout, so the runtime starts whether or
  not the gateway runs;
- no other process mounts it or names it;
- every pod that mounts either store is colocated, both claims being ReadWriteOnce;
- the deployment managers' admission lists name it for the runtime, read-only, and for nothing else;
- the store is at least twice the gateway's fixed cap, read from the service's source.
"""

import copy
import json
import re
from pathlib import Path

from test_cell import (
    CELL,
    CHART,
    POLICY,
    TEMPLATES,
    every_image,
    helm,
    managed_pods,
    policy_variables,
    two_managers,
    workloads,
)

CLAIM = "research-artifacts"
SHARED = "artifacts"
MOUNT = "/var/lib/mandate/research-artifacts"
VARIABLE = "MANDATE_RESEARCH_ARTIFACTS"
ACCESS = {"model-gateway": "rw", "runtime": "ro"}
LAYOUT = "research-artifacts-layout"
LAYOUT_COMMAND = ["bash", "-c", f"mkdir -p {MOUNT}/sha256 {MOUNT}/tmp"]
WRITER = Path(__file__).resolve().parents[2] / "crates" / "mandate-gatewayd" / "src" / "writer.rs"
UNITS = {"Mi": 2**20, "Gi": 2**30, "Ti": 2**40}


def pod_spec(doc: dict) -> dict:
    return doc["spec"]["template"]["spec"]


def process_of(doc: dict) -> str:
    return doc["spec"]["template"]["metadata"]["labels"].get("mandate.dev/process", "")


def claim_volumes(spec: dict, claim: str) -> list[dict]:
    return [v for v in spec.get("volumes", []) if v.get("persistentVolumeClaim", {}).get("claimName") == claim]


def research_modes(docs: list[dict]) -> list[str]:
    """Each pod mounts the research store exactly as ACCESS says, read-only at the volume and every
    mount of a reader, names it in its variable, and lays it out (the gateway) or never waits for
    it (the runtime); no other pod mounts or names it, and none names a claim not rendered."""
    problems = []
    claims = [d for d in docs if d["kind"] == "PersistentVolumeClaim" and d["metadata"]["name"] == CLAIM]
    if len(claims) > 1:
        problems.append(f"{len(claims)} claims named {CLAIM}")
    for doc in workloads(docs):
        spec = pod_spec(doc)
        name = process_of(doc)
        want = ACCESS.get(name) if claims else None
        volumes = claim_volumes(spec, CLAIM)
        every = [*spec.get("initContainers", []), *spec["containers"]]
        if want is None:
            if volumes:
                problems.append(f"{name} mounts the research store, which it must not")
            for c in every:
                if any(e["name"] == VARIABLE for e in c.get("env", [])):
                    problems.append(f"{name}'s {c['name']} names the research store")
            continue
        if len(volumes) != 1:
            problems.append(f"{name} has {len(volumes)} volumes of the research store, want one")
            continue
        volume = volumes[0]
        if (volume["persistentVolumeClaim"].get("readOnly") is True) != (want == "ro"):
            problems.append(f"{name}'s volume of the research store is not {want}")
        layouts = [i for i in spec.get("initContainers", []) if any(m["name"] == volume["name"] for m in i.get("volumeMounts", []))]
        if want == "ro" and layouts:
            problems.append(f"{name} waits on or writes the research store before it starts: {[i['name'] for i in layouts]}")
        if want == "rw":
            if [i["name"] for i in layouts] != [LAYOUT]:
                problems.append(f"{name}'s init containers on the research store are {[i['name'] for i in layouts]}")
            elif layouts[0]["command"] != LAYOUT_COMMAND:
                problems.append(f"{name}'s layout step does not make exactly the store's directories")
        for c in spec["containers"]:
            mounts = [m for m in c.get("volumeMounts", []) if m["name"] == volume["name"]]
            if len(mounts) != 1 or mounts[0]["mountPath"] != MOUNT:
                problems.append(f"{name}'s {c['name']} mounts the research store as {mounts}, want once at {MOUNT}")
                continue
            if (mounts[0].get("readOnly") is True) != (want == "ro"):
                problems.append(f"{name}'s {c['name']} mounts the research store other than {want}")
            env = {e["name"]: e.get("value") for e in c.get("env", [])}
            if env.get(VARIABLE) != MOUNT:
                problems.append(f"{name}'s {c['name']} has {VARIABLE}={env.get(VARIABLE)}, want {MOUNT}")
    return problems


def writable_claims(doc: dict) -> set[str]:
    """The claims a pod can write: a volume not read-only, mounted somewhere not read-only."""
    spec = pod_spec(doc)
    writable = {
        v["name"]: v["persistentVolumeClaim"]["claimName"]
        for v in spec.get("volumes", [])
        if "persistentVolumeClaim" in v and v["persistentVolumeClaim"].get("readOnly") is not True
    }
    return {
        writable[m["name"]]
        for c in [*spec.get("initContainers", []), *spec["containers"]]
        for m in c.get("volumeMounts", [])
        if m["name"] in writable and m.get("readOnly") is not True
    }


def reach_problems(docs: list[dict]) -> list[str]:
    """The gateway can write the research store and no other claim; the runtime can write none."""
    problems = []
    for doc in workloads(docs):
        name = process_of(doc)
        if name == "model-gateway" and writable_claims(doc) != {CLAIM}:
            problems.append(f"the gateway can write {sorted(writable_claims(doc))}, want only {CLAIM}")
        if name == "runtime" and writable_claims(doc):
            problems.append(f"the runtime can write {sorted(writable_claims(doc))}")
    return problems


def colocation_problems(docs: list[dict]) -> list[str]:
    """Every pod that mounts either store carries the colocation label and its required affinity."""
    problems = []
    for doc in workloads(docs):
        spec = pod_spec(doc)
        if not (claim_volumes(spec, CLAIM) or claim_volumes(spec, SHARED)):
            continue
        labels = doc["spec"]["template"]["metadata"]["labels"]
        terms = spec.get("affinity", {}).get("podAffinity", {}).get("requiredDuringSchedulingIgnoredDuringExecution", [])
        if labels.get("mandate.dev/colocate") != "true" or not any(
            t.get("labelSelector", {}).get("matchLabels", {}).get("mandate.dev/colocate") == "true" for t in terms
        ):
            problems.append(f"{process_of(doc)} mounts a store but is not colocated")
    return problems


def admission_claims(docs: list[dict]) -> list[dict]:
    """Each deployment manager's admission policy's `claims` literal."""
    return [
        policy_variables(d)["claims"]
        for d in docs
        if d["kind"] == POLICY and d["metadata"]["name"].endswith("-pods")
    ]


def quantity(text: str) -> int:
    number, unit = re.fullmatch(r"([0-9]+)(Mi|Gi|Ti)", text).groups()
    return int(number) * UNITS[unit]


def store_cap() -> int:
    """`STORE_CAP` as the service's source writes it: a product of integers."""
    found = re.search(r"pub const STORE_CAP: u64 = ([0-9 *_]+);", WRITER.read_text())
    assert found, f"no STORE_CAP in {WRITER}"
    product = 1
    for factor in found.group(1).split("*"):
        product *= int(factor.strip().replace("_", ""))
    return product


def research_claim(docs: list[dict]) -> dict:
    return next(d for d in docs if d["kind"] == "PersistentVolumeClaim" and d["metadata"]["name"] == CLAIM)


def test_each_pod_gets_its_research_mode():
    docs = two_managers()
    assert research_modes(docs) == []
    assert {process_of(d) for d in workloads(docs) if claim_volumes(pod_spec(d), CLAIM)} == {"model-gateway", "runtime"}


def test_the_gateway_writes_only_the_research_store_and_the_runtime_writes_nothing():
    docs = two_managers()
    assert reach_problems(docs) == []
    gateway = next(d for d in workloads(docs) if process_of(d) == "model-gateway")
    assert claim_volumes(pod_spec(gateway), SHARED)[0]["persistentVolumeClaim"]["readOnly"] is True


def test_every_pod_on_either_store_is_colocated():
    assert colocation_problems(two_managers()) == []


def test_both_managers_admit_the_research_store_for_the_runtime_alone_and_read_only():
    docs = two_managers()
    lists = admission_claims(docs)
    assert len(lists) == 2, "both deployment managers' policies"
    for claims in lists:
        holders = {name for name, names in claims.items() if CLAIM in names}
        assert holders == {"runtime"}, claims
    runtimes = [w for w in managed_pods(docs) if process_of(w) == "runtime"]
    assert runtimes, "a managed runtime pod"
    for runtime in runtimes:
        assert claim_volumes(pod_spec(runtime), CLAIM)[0]["persistentVolumeClaim"]["readOnly"] is True


def test_the_store_is_at_least_twice_the_gateways_cap():
    cap = store_cap()
    assert cap == 512 * 2**20
    assert quantity(research_claim(two_managers())["spec"]["resources"]["requests"]["storage"]) >= 2 * cap
    for size in ("512Mi", "1023Mi", "0Gi"):
        small = helm(CHART, "-f", str(CELL / "values" / "k3d.yaml"), *every_image(), "--set", f"researchArtifacts.size={size}")
        assert small.returncode != 0 and "researchArtifacts" in small.stderr, f"{size} is refused"


def test_no_research_store_without_the_gateway_or_the_api_server():
    for unset in ("images.model-gateway=", "images.api="):
        docs = two_managers("--set", unset)
        assert not [d for d in docs if d["kind"] == "PersistentVolumeClaim" and d["metadata"]["name"] == CLAIM]
        assert research_modes(docs) == []
        assert not {process_of(d) for d in workloads(docs) if claim_volumes(pod_spec(d), CLAIM)}


EDITED: dict[int, dict[str, dict]] = {}


def workload(docs: list[dict], name: str) -> dict:
    found = [d for d in docs if d["kind"] in ("Deployment", "Job") and process_of(d) == name]
    if found:
        return found[0]
    pods = EDITED.setdefault(id(docs), {process_of(w): w for w in managed_pods(docs)})
    return pods[name]


def seeded(change):
    docs = copy.deepcopy(two_managers())
    change(docs)
    edited = EDITED.pop(id(docs), {})
    if edited:
        templates = next(d for d in docs if d["kind"] == "ConfigMap" and d["metadata"]["name"] == TEMPLATES)
        for name, w in edited.items():
            templates["data"][f"{name}.json"] = json.dumps(w["spec"]["template"])
    return docs


def mount(doc: dict, claim: str) -> dict:
    spec = pod_spec(doc)
    volume = claim_volumes(spec, claim)[0]["name"]
    return next(m for m in spec["containers"][0]["volumeMounts"] if m["name"] == volume)


def test_each_seeded_bug_is_caught():
    def runtime_writes(docs):
        mount(workload(docs, "runtime"), CLAIM)["readOnly"] = False
        claim_volumes(pod_spec(workload(docs, "runtime")), CLAIM)[0]["persistentVolumeClaim"]["readOnly"] = False

    def gateway_writes_the_shared_store(docs):
        gateway = workload(docs, "model-gateway")
        mount(gateway, SHARED)["readOnly"] = False
        claim_volumes(pod_spec(gateway), SHARED)[0]["persistentVolumeClaim"]["readOnly"] = False

    def executor_mounts_it(docs):
        spec = pod_spec(workload(docs, "executor"))
        spec["volumes"].append({"name": "research-artifacts", "persistentVolumeClaim": {"claimName": CLAIM, "readOnly": True}})

    def runtime_waits_for_it(docs):
        spec = pod_spec(workload(docs, "runtime"))
        spec["initContainers"].append(
            {"name": "wait", "command": ["true"], "volumeMounts": [{"name": "research-artifacts", "mountPath": MOUNT, "readOnly": True}]}
        )

    def gateway_without_layout(docs):
        spec = pod_spec(workload(docs, "model-gateway"))
        spec["initContainers"] = [i for i in spec["initContainers"] if i["name"] != LAYOUT]

    def runtime_variable_dropped(docs):
        env = pod_spec(workload(docs, "runtime"))["containers"][0]["env"]
        env[:] = [e for e in env if e["name"] != VARIABLE]

    def gateway_not_colocated(docs):
        workload(docs, "model-gateway")["spec"]["template"]["metadata"]["labels"]["mandate.dev/colocate"] = "false"

    checks = (research_modes, reach_problems, colocation_problems)
    for change in (
        runtime_writes,
        gateway_writes_the_shared_store,
        executor_mounts_it,
        runtime_waits_for_it,
        gateway_without_layout,
        runtime_variable_dropped,
        gateway_not_colocated,
    ):
        docs = seeded(change)
        assert any(check(docs) for check in checks), f"{change.__name__} was not caught"
