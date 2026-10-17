"""The cell's shared artifact store, on what the workspace chart renders.

One volume holds the workspace's artifacts (mandate documents, backtest reports), so a pod's restart
loses none. Which process may write it is written out here, not read from the chart, so a chart that
drifts fails: the API server and the control runs write, the runtime and the deployment manager only
read, and no other process mounts it at all.
"""

import copy

from test_cell import render_all

CLAIM = "artifacts"
MOUNT = "/var/lib/mandate/artifacts"
# The access each process type has; a process not named here has none.
ACCESS = {"api": "rw", "control": "rw", "runtime": "ro", "deploy-manager": "ro"}
ENV = {"api": "MANDATE_API_ARTIFACTS"}
DEFAULT_ENV = "MANDATE_ARTIFACTS"


def pod_spec(doc: dict) -> dict:
    return doc["spec"]["template"]["spec"]


def store_volumes(spec: dict) -> list[str]:
    return [
        v["name"]
        for v in spec.get("volumes", [])
        if v.get("persistentVolumeClaim", {}).get("claimName") == CLAIM
    ]


def artifact_modes(docs: list[dict]) -> list[str]:
    """Each pod mounts the store exactly as ACCESS says, at MOUNT, in its process's own containers and
    not its init containers, and names it in its variable; the claim survives an uninstall."""
    problems = []
    claims = [d for d in docs if d["kind"] == "PersistentVolumeClaim" and d["metadata"]["name"] == CLAIM]
    if len(claims) != 1:
        problems.append(f"{len(claims)} claims named {CLAIM}, want one")
    elif claims[0]["metadata"].get("annotations", {}).get("helm.sh/resource-policy") != "keep":
        problems.append("the artifact claim is deleted with the release")
    seen = set()
    for doc in docs:
        if doc["kind"] not in ("Deployment", "Job"):
            continue
        spec = pod_spec(doc)
        name = doc["spec"]["template"]["metadata"]["labels"].get("mandate.dev/process", "")
        want = ACCESS.get(name)
        volumes = store_volumes(spec)
        for init in spec.get("initContainers", []):
            for m in init.get("volumeMounts", []):
                if m["name"] in volumes:
                    problems.append(f"{name}: init container {init['name']} mounts the store")
        if want is None:
            if volumes:
                problems.append(f"{name} mounts the store, which it must not")
            for c in spec["containers"]:
                if any(e["name"] in (DEFAULT_ENV, *ENV.values()) for e in c.get("env", [])):
                    problems.append(f"{name}'s {c['name']} names the store")
            continue
        seen.add(name)
        if len(volumes) != 1:
            problems.append(f"{name} has {len(volumes)} volumes of the store, want one")
            continue
        for c in spec["containers"]:
            mounts = [m for m in c.get("volumeMounts", []) if m["name"] == volumes[0]]
            if len(mounts) != 1 or mounts[0]["mountPath"] != MOUNT:
                problems.append(f"{name}'s {c['name']} mounts the store as {mounts}, want once at {MOUNT}")
                continue
            read_only = mounts[0].get("readOnly", False) is True
            if read_only != (want == "ro"):
                problems.append(
                    f"{name}'s {c['name']} mounts the store {'read-only' if read_only else 'read-write'}, want {want}"
                )
            env = {e["name"]: e.get("value") for e in c.get("env", [])}
            variable = ENV.get(name, DEFAULT_ENV)
            if env.get(variable) != MOUNT:
                problems.append(f"{name}'s {c['name']} has {variable}={env.get(variable)}, want {MOUNT}")
            other = DEFAULT_ENV if variable != DEFAULT_ENV else ENV["api"]
            if other in env:
                problems.append(f"{name}'s {c['name']} also names {other}")
    for name in ("api", "control", "runtime"):
        if name not in seen:
            problems.append(f"{name} did not render with the store")
    return problems


def test_each_pod_gets_its_mode():
    """Every image is named, so the runtime renders too, as it will once it is built."""
    assert artifact_modes(render_all()) == []


def test_the_store_size_and_class_are_values_not_its_access():
    docs = render_all("--set", "artifacts.size=5Gi", "--set", "artifacts.storageClass=fast")
    claim = next(d for d in docs if d["kind"] == "PersistentVolumeClaim" and d["metadata"]["name"] == CLAIM)
    assert claim["spec"]["resources"]["requests"]["storage"] == "5Gi"
    assert claim["spec"]["storageClassName"] == "fast"
    assert artifact_modes(docs) == []


def workload(docs: list[dict], name: str) -> dict:
    return next(
        d
        for d in docs
        if d["kind"] in ("Deployment", "Job")
        and d["spec"]["template"]["metadata"]["labels"].get("mandate.dev/process") == name
    )


def mount_of(doc: dict) -> dict:
    spec = pod_spec(doc)
    volume = store_volumes(spec)[0]
    return next(m for m in spec["containers"][0]["volumeMounts"] if m["name"] == volume)


def seeded(change):
    docs = copy.deepcopy(render_all())
    change(docs)
    return docs


def test_each_seeded_bug_is_caught():
    def control_read_only(docs):
        mount_of(workload(docs, "control"))["readOnly"] = True

    def api_variable_renamed(docs):
        env = pod_spec(workload(docs, "api"))["containers"][0]["env"]
        next(e for e in env if e["name"] == "MANDATE_API_ARTIFACTS")["name"] = "MANDATE_ARTIFACTS"

    def executor_mounts_it(docs):
        spec = pod_spec(workload(docs, "executor"))
        spec.setdefault("volumes", []).append(
            {"name": "artifacts", "persistentVolumeClaim": {"claimName": CLAIM}}
        )

    def gate_mounts_it(docs):
        spec = pod_spec(workload(docs, "api"))
        spec["initContainers"][0].setdefault("volumeMounts", []).append(
            {"name": store_volumes(spec)[0], "mountPath": MOUNT}
        )

    def claim_not_kept(docs):
        claim = next(d for d in docs if d["kind"] == "PersistentVolumeClaim")
        claim["metadata"].pop("annotations", None)

    def control_loses_it(docs):
        spec = pod_spec(workload(docs, "control"))
        spec["volumes"] = [
            v for v in spec["volumes"] if v.get("persistentVolumeClaim", {}).get("claimName") != CLAIM
        ]

    for change in (
        control_read_only,
        api_variable_renamed,
        executor_mounts_it,
        gate_mounts_it,
        claim_not_kept,
        control_loses_it,
    ):
        assert artifact_modes(seeded(change)), change.__name__
