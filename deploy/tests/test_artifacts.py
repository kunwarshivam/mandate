"""The cell's shared artifact store, on what the workspace chart renders.

One volume holds the workspace's artifacts (mandate documents, backtest reports), so a pod's restart
loses none. Which process may write it is written out here, not read from the chart, so a chart that
drifts fails: the API server and the control runs write, the runtime and the deployment manager only
read, and no other process mounts it at all. Before any process starts, the store has `sha256/` and
`tmp/`: the API server refuses a root without `sha256/`, and the readers need both.
"""

import copy

from test_cell import render_all

CLAIM = "artifacts"
MOUNT = "/var/lib/mandate/artifacts"
# The access each process type has; a process not named here has none.
ACCESS = {"api": "rw", "control": "rw", "runtime": "ro", "deploy-manager": "ro"}
ENV = {"api": "MANDATE_API_ARTIFACTS"}
DEFAULT_ENV = "MANDATE_ARTIFACTS"
LAYOUT = "artifacts-layout"
LAYOUT_DIRS = (f"{MOUNT}/sha256", f"{MOUNT}/tmp")


def pod_spec(doc: dict) -> dict:
    return doc["spec"]["template"]["spec"]


def store_volumes(spec: dict) -> list[dict]:
    return [
        v for v in spec.get("volumes", []) if v.get("persistentVolumeClaim", {}).get("claimName") == CLAIM
    ]


def process_of(doc: dict) -> str:
    return doc["spec"]["template"]["metadata"]["labels"].get("mandate.dev/process", "")


def layout_problems(name: str, init: dict, want: str) -> list[str]:
    """The layout step makes both directories in a writer's pod, and only waits for them, read-only,
    in a reader's."""
    script = " ".join(init.get("command", []))
    problems = [f"{name}'s layout step names no {d}" for d in LAYOUT_DIRS if d not in script]
    if want == "rw" and "mkdir -p" not in script:
        problems.append(f"{name}'s layout step does not make the store's directories")
    if want == "ro" and ("mkdir" in script or "exit 1" not in script):
        problems.append(f"{name}'s layout step writes, or never gives up waiting")
    return problems


def artifact_modes(docs: list[dict]) -> list[str]:
    """Each pod mounts the store exactly as ACCESS says, at MOUNT, read-only at both the volume and
    every mount of a reader, names it in its variable, and lays it out (a writer) or waits for its
    layout (a reader) before its process starts; no other init container mounts it, and no pod names
    a claim that is not rendered."""
    problems = []
    claims = [d for d in docs if d["kind"] == "PersistentVolumeClaim" and d["metadata"]["name"] == CLAIM]
    if len(claims) > 1:
        problems.append(f"{len(claims)} claims named {CLAIM}")
    for doc in docs:
        if doc["kind"] not in ("Deployment", "Job"):
            continue
        spec = pod_spec(doc)
        name = process_of(doc)
        want = ACCESS.get(name)
        volumes = store_volumes(spec)
        if volumes and not claims:
            problems.append(f"{name} names the claim {CLAIM}, which is not rendered")
        if want is None or not claims:
            if want is None and volumes:
                problems.append(f"{name} mounts the store, which it must not")
            for c in [*spec.get("initContainers", []), *spec["containers"]]:
                if any(e["name"] in (DEFAULT_ENV, *ENV.values()) for e in c.get("env", [])):
                    problems.append(f"{name}'s {c['name']} names the store")
            continue
        if len(volumes) != 1:
            problems.append(f"{name} has {len(volumes)} volumes of the store, want one")
            continue
        volume = volumes[0]
        if (volume["persistentVolumeClaim"].get("readOnly") is True) != (want == "ro"):
            problems.append(f"{name}'s volume of the store is not {want}")
        layouts = []
        for init in spec.get("initContainers", []):
            mounts = [m for m in init.get("volumeMounts", []) if m["name"] == volume["name"]]
            if not mounts:
                continue
            if init["name"] != LAYOUT:
                problems.append(f"{name}: init container {init['name']} mounts the store")
                continue
            layouts.append(init)
            if (mounts[0].get("readOnly") is True) != (want == "ro"):
                problems.append(f"{name}'s layout step mounts the store other than {want}")
            problems.extend(layout_problems(name, init, want))
        if len(layouts) != 1:
            problems.append(f"{name} has {len(layouts)} layout steps, want one")
        for c in spec["containers"]:
            mounts = [m for m in c.get("volumeMounts", []) if m["name"] == volume["name"]]
            if len(mounts) != 1 or mounts[0]["mountPath"] != MOUNT:
                problems.append(f"{name}'s {c['name']} mounts the store as {mounts}, want once at {MOUNT}")
                continue
            read_only = mounts[0].get("readOnly") is True
            if read_only != (want == "ro"):
                problems.append(
                    f"{name}'s {c['name']} mounts the store {'ro' if read_only else 'rw'}, want {want}"
                )
            env = {e["name"]: e.get("value") for e in c.get("env", [])}
            variable = ENV.get(name, DEFAULT_ENV)
            if env.get(variable) != MOUNT:
                problems.append(f"{name}'s {c['name']} has {variable}={env.get(variable)}, want {MOUNT}")
            other = DEFAULT_ENV if variable != DEFAULT_ENV else ENV["api"]
            if other in env:
                problems.append(f"{name}'s {c['name']} also names {other}")
    return problems


def rendered_with_store(docs: list[dict]) -> set[str]:
    return {process_of(d) for d in docs if d["kind"] in ("Deployment", "Job") and store_volumes(pod_spec(d))}


def test_each_pod_gets_its_mode():
    """Every image is named, so the runtime renders too, as it will once it is built."""
    docs = render_all()
    assert artifact_modes(docs) == []
    assert rendered_with_store(docs) == {"api", "control", "runtime"}


def test_the_store_size_and_class_are_values_not_its_access():
    docs = render_all("--set", "artifacts.size=5Gi", "--set", "artifacts.storageClass=fast")
    claim = next(d for d in docs if d["kind"] == "PersistentVolumeClaim" and d["metadata"]["name"] == CLAIM)
    assert claim["spec"]["resources"]["requests"]["storage"] == "5Gi"
    assert claim["spec"]["storageClassName"] == "fast"
    assert artifact_modes(docs) == []


def test_no_claim_and_no_mount_without_a_long_running_process_to_bind_it():
    """As `cell up` renders on a host that pulls images: no API image, so only the control run would
    mount the store. A claim on a class that binds on first use would never bind under `helm --wait`,
    and a Job naming an absent claim would stay pending, so neither renders."""
    docs = render_all("--set", "images.api=", "--set", "images.runtime=")
    assert not [d for d in docs if d["kind"] == "PersistentVolumeClaim"]
    assert "control" in {process_of(d) for d in docs if d["kind"] == "Job"}
    assert artifact_modes(docs) == []
    assert rendered_with_store(docs) == set()


def workload(docs: list[dict], name: str) -> dict:
    return next(d for d in docs if d["kind"] in ("Deployment", "Job") and process_of(d) == name)


def main_mount(doc: dict) -> dict:
    spec = pod_spec(doc)
    volume = store_volumes(spec)[0]["name"]
    return next(m for m in spec["containers"][0]["volumeMounts"] if m["name"] == volume)


def layout(doc: dict) -> dict:
    return next(i for i in pod_spec(doc)["initContainers"] if i["name"] == LAYOUT)


def seeded(change, *args):
    docs = copy.deepcopy(render_all(*args))
    change(docs)
    return docs


def test_each_seeded_bug_is_caught():
    def control_read_only(docs):
        main_mount(workload(docs, "control"))["readOnly"] = True

    def runtime_writes(docs):
        main_mount(workload(docs, "runtime"))["readOnly"] = False

    def runtime_volume_writable(docs):
        store_volumes(pod_spec(workload(docs, "runtime")))[0]["persistentVolumeClaim"]["readOnly"] = False

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
            {"name": "artifacts", "mountPath": MOUNT}
        )

    def control_loses_it(docs):
        spec = pod_spec(workload(docs, "control"))
        spec["volumes"] = [
            v for v in spec["volumes"] if v.get("persistentVolumeClaim", {}).get("claimName") != CLAIM
        ]

    def api_without_layout(docs):
        spec = pod_spec(workload(docs, "api"))
        spec["initContainers"] = [i for i in spec["initContainers"] if i["name"] != LAYOUT]

    def api_layout_without_tmp(docs):
        command = layout(workload(docs, "api"))["command"]
        command[-1] = command[-1].replace(f" {MOUNT}/tmp", "")

    def runtime_layout_writes(docs):
        layout(workload(docs, "runtime"))["command"][-1] = f"mkdir -p {MOUNT}/sha256 {MOUNT}/tmp"

    for change in (
        control_read_only,
        runtime_writes,
        runtime_volume_writable,
        api_variable_renamed,
        executor_mounts_it,
        gate_mounts_it,
        control_loses_it,
        api_without_layout,
        api_layout_without_tmp,
        runtime_layout_writes,
    ):
        assert artifact_modes(seeded(change)), change.__name__

    def control_names_an_absent_claim(docs):
        spec = pod_spec(workload(docs, "control"))
        spec["volumes"].append({"name": "artifacts", "persistentVolumeClaim": {"claimName": CLAIM}})

    assert artifact_modes(
        seeded(control_names_an_absent_claim, "--set", "images.api=", "--set", "images.runtime=")
    )
